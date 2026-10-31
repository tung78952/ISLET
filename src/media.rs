//! Now-playing state from Windows media sessions (SMTC) and synced lyrics from LRCLIB.

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::core::{Interface, Ref, HSTRING};
use windows::Foundation::{TypedEventHandler, Uri};
use windows_future::IAsyncOperation;
use windows::Media::Control::*;
use windows::Storage::Streams::DataReader;
use windows::Web::Http::HttpClient;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::System::SystemInformation::GetSystemTimePreciseAsFileTime;
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::{WM_LYRICS, WM_MEDIA};

pub struct Art {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
    /// Average color (0xRRGGBB), used to tint the lyrics background.
    pub avg: u32,
}

fn average(px: &[u8]) -> u32 {
    let (mut r, mut g, mut b) = (0u64, 0u64, 0u64);
    let n = (px.len() / 4).max(1) as u64;
    for p in px.chunks_exact(4) {
        b += p[0] as u64;
        g += p[1] as u64;
        r += p[2] as u64;
    }
    ((r / n) << 16 | (g / n) << 8 | (b / n)) as u32
}

#[derive(Default, Clone)]
pub struct MediaInfo {
    pub has: bool,
    pub title: String,
    pub artist: String,
    pub playing: bool,
    pub pos: f64,
    pub dur: f64,
    pub stamp: Option<Instant>,
    pub art: Option<Arc<Art>>,
    pub art_id: u64,
}

impl MediaInfo {
    pub fn position(&self) -> f64 {
        let mut p = self.pos;
        if self.playing {
            if let Some(s) = self.stamp {
                p += s.elapsed().as_secs_f64();
            }
        }
        if self.dur > 0.0 {
            p.clamp(0.0, self.dur)
        } else {
            p.max(0.0)
        }
    }
}

pub enum Cmd {
    Toggle,
    Next,
    Prev,
    /// Jump to a position in seconds.
    Seek(f64),
    /// A media session reported a change: read it again.
    Refresh,
}

/// Another source must play this long before it may take over from one still playing.
const TAKE_OVER: Duration = Duration::from_secs(15);

/// What `choose` needs to know about a media session.
struct Source<'a> {
    id: &'a str,
    playing: bool,
    /// Has an artist: a song rather than a clip.
    song: bool,
    /// How long it has been playing without a break.
    played: Duration,
}

/// Which source the notch follows. What is shown stays while it plays: only a song that has played
/// for a while can take over from another song, or from a clip. Once it stops, a playing source
/// follows at once, songs first.
fn choose(shown: Option<&str>, sources: &[Source]) -> Option<usize> {
    let cur = shown.and_then(|id| sources.iter().position(|s| s.id == id));
    let rival = |c: &Source, s: &Source| s.id != c.id && s.playing && s.played >= TAKE_OVER && s.song >= c.song;
    if let Some(i) = cur.filter(|i| sources[*i].playing) {
        return Some(sources.iter().position(|s| rival(&sources[i], s)).unwrap_or(i));
    }
    let playing = sources.iter().enumerate().filter(|(_, s)| s.playing);
    playing.max_by_key(|(_, s)| (s.song, s.played)).map(|(i, _)| i).or(cur).or((!sources.is_empty()).then_some(0))
}

/// Change subscriptions on one session; removed when the session goes away.
struct Watched {
    session: GlobalSystemMediaTransportControlsSession,
    tokens: [i64; 3],
}

impl Watched {
    fn new(session: GlobalSystemMediaTransportControlsSession, tx: &Sender<Cmd>) -> Self {
        let t = tx.clone();
        let props = session
            .MediaPropertiesChanged(&TypedEventHandler::new(move |_: Ref<GlobalSystemMediaTransportControlsSession>, _: Ref<MediaPropertiesChangedEventArgs>| {
                let _ = t.send(Cmd::Refresh);
                Ok(())
            }))
            .unwrap_or_default();
        let t = tx.clone();
        let playback = session
            .PlaybackInfoChanged(&TypedEventHandler::new(move |_: Ref<GlobalSystemMediaTransportControlsSession>, _: Ref<PlaybackInfoChangedEventArgs>| {
                let _ = t.send(Cmd::Refresh);
                Ok(())
            }))
            .unwrap_or_default();
        let t = tx.clone();
        let timeline = session
            .TimelinePropertiesChanged(&TypedEventHandler::new(move |_: Ref<GlobalSystemMediaTransportControlsSession>, _: Ref<TimelinePropertiesChangedEventArgs>| {
                let _ = t.send(Cmd::Refresh);
                Ok(())
            }))
            .unwrap_or_default();
        Watched { session, tokens: [props, playback, timeline] }
    }
}

impl Drop for Watched {
    fn drop(&mut self) {
        let [p, b, t] = self.tokens;
        let _ = self.session.RemoveMediaPropertiesChanged(p);
        let _ = self.session.RemovePlaybackInfoChanged(b);
        let _ = self.session.RemoveTimelinePropertiesChanged(t);
    }
}

#[derive(Default, PartialEq, Eq, Clone, Copy)]
pub enum LyrStatus {
    #[default]
    None,
    Loading,
    Ready,
    Missing,
}

#[derive(Default)]
pub struct Lyrics {
    pub key: String,
    pub status: LyrStatus,
    /// (start seconds, text); a negative start marks unsynced plain lyrics.
    pub lines: Vec<(f64, String)>,
}

impl Lyrics {
    pub fn line_at(&self, t: f64) -> Option<&str> {
        if self.status != LyrStatus::Ready {
            return None;
        }
        if self.lines.first().is_some_and(|l| l.0 < 0.0) {
            return None;
        }
        self.lines.iter().rev().find(|l| l.0 <= t + 0.25).map(|l| l.1.as_str())
    }
}

fn post(hwnd: isize, msg: u32) {
    unsafe {
        let _ = PostMessageW(Some(HWND(hwnd as _)), msg, WPARAM(0), LPARAM(0));
    }
}

fn filetime_now() -> i64 {
    let ft = unsafe { GetSystemTimePreciseAsFileTime() };
    ((ft.dwHighDateTime as i64) << 32) | ft.dwLowDateTime as i64
}

pub fn spawn(hwnd: isize, info: Arc<Mutex<MediaInfo>>, lyrics: Arc<Mutex<Lyrics>>) -> Sender<Cmd> {
    let (tx, rx) = mpsc::channel::<Cmd>();
    let events = tx.clone();
    std::thread::spawn(move || {
        crate::sys::name_thread("islet-media");
        unsafe {
            let _ = RoInitialize(RO_INIT_MULTITHREADED);
        }
        let mgr = loop {
            match GlobalSystemMediaTransportControlsSessionManager::RequestAsync().and_then(|op| op.join()) {
                Ok(m) => break m,
                Err(_) => std::thread::sleep(Duration::from_secs(5)),
            }
        };
        let mut last_key = String::new();
        let mut art: Option<Arc<Art>> = None;
        let mut art_id = 0u64;
        // A browser can send the cover after the title, so a new song's art is read again for a
        // moment until this time.
        let mut art_until: Option<Instant> = None;
        // After a seek the session keeps reporting the old position for a moment; hold the
        // requested one until it catches up so the UI doesn't jump back and forth.
        let mut hold: Option<(f64, Instant)> = None;
        // Sessions report their own changes, so this thread sleeps until one does. The playing
        // position is extrapolated on the UI side; a slow safety read covers players whose
        // events are unreliable.
        let t = events.clone();
        let _ = mgr.SessionsChanged(&TypedEventHandler::new(move |_: Ref<GlobalSystemMediaTransportControlsSessionManager>, _: Ref<SessionsChangedEventArgs>| {
            let _ = t.send(Cmd::Refresh);
            Ok(())
        }));
        let mut tracked: Vec<Tracked> = Vec::new();
        let mut shown: Option<String> = None;
        // A browser reports one session for all its tabs: a clip in another tab replaces the
        // song in it. Since when such a clip has been held back.
        let mut clip_since: Option<Instant> = None;
        let mut first = true;
        loop {
            let wait = if std::mem::take(&mut first) { Duration::ZERO } else { Duration::from_secs(5) };
            let mut changed_props = false;
            match rx.recv_timeout(wait) {
                Ok(Cmd::Refresh) => {
                    // Events come in bursts (title, art, playback, timeline): read once.
                    std::thread::sleep(Duration::from_millis(60));
                    while let Ok(Cmd::Refresh) = rx.try_recv() {}
                    changed_props = true;
                }
                Ok(cmd) => {
                    if let Cmd::Seek(t) = cmd {
                        hold = Some((t, Instant::now()));
                    }
                    if let Some(s) = tracked.iter().find(|t| Some(&t.id) == shown.as_ref()).map(|t| t.watched.session.clone()) {
                        let r = match cmd {
                            Cmd::Toggle => Some(s.TryTogglePlayPauseAsync()),
                            Cmd::Next => Some(s.TrySkipNextAsync()),
                            Cmd::Prev => Some(s.TrySkipPreviousAsync()),
                            Cmd::Seek(secs) => Some(s.TryChangePlaybackPositionAsync((secs.max(0.0) * 1e7) as i64)),
                            Cmd::Refresh => None,
                        };
                        if let Some(r) = r {
                            let _ = r.and_then(|op| op.join());
                        }
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return,
                Err(RecvTimeoutError::Timeout) => {}
            }

            let mut next = MediaInfo::default();
            sync_sessions(&mgr, &mut tracked, &events, changed_props);
            let sources: Vec<Source> = tracked
                .iter()
                .map(|t| Source { id: &t.id, playing: t.playing_since.is_some(), song: t.song, played: t.playing_since.map(|p| p.elapsed()).unwrap_or_default() })
                .collect();
            let pick = choose(shown.as_deref(), &sources);
            drop(sources);
            let same_source = pick.is_some_and(|i| Some(&tracked[i].id) == shown.as_ref());
            shown = pick.map(|i| tracked[i].id.clone());
            if let Some(s) = pick.map(|i| tracked[i].watched.session.clone()) {
                next.has = true;
                let props = s.TryGetMediaPropertiesAsync().and_then(|op| op.join()).ok();
                if let Some(p) = &props {
                    next.title = p.Title().map(|h| h.to_string()).unwrap_or_default();
                    next.artist = p.Artist().map(|h| h.to_string()).unwrap_or_default();
                }
                next.playing = s
                    .GetPlaybackInfo()
                    .and_then(|i| i.PlaybackStatus())
                    .map(|st| st == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing)
                    .unwrap_or(false);
                if let Ok(tl) = s.GetTimelineProperties() {
                    let pos = tl.Position().map(|t| t.Duration).unwrap_or(0) as f64 / 1e7;
                    let end = tl.EndTime().map(|t| t.Duration).unwrap_or(0) as f64 / 1e7;
                    let updated = tl.LastUpdatedTime().map(|d| d.UniversalTime).unwrap_or(0);
                    let mut p = pos;
                    if next.playing && updated > 0 {
                        let drift = (filetime_now() - updated) as f64 / 1e7;
                        if (0.0..36000.0).contains(&drift) {
                            p += drift;
                        }
                    }
                    if let Some((t, at)) = hold {
                        let expect = t + if next.playing { at.elapsed().as_secs_f64() } else { 0.0 };
                        if at.elapsed() < Duration::from_millis(2500) && (p - expect).abs() > 1.0 {
                            p = expect;
                        } else {
                            hold = None;
                        }
                    }
                    next.pos = p;
                    next.dur = end;
                    next.stamp = Some(Instant::now());
                }
                let key = format!("{}\u{1}{}", next.title, next.artist);
                let clip = same_source && next.playing && key != last_key && next.artist.trim().is_empty() && last_key.split('\u{1}').nth(1).is_some_and(|a| !a.trim().is_empty());
                clip_since = if clip { clip_since.or(Some(Instant::now())) } else { None };
                if clip_since.is_some_and(|t| t.elapsed() < TAKE_OVER) {
                    continue;
                }
                if key != last_key && !next.title.is_empty() {
                    last_key = key.clone();
                    art = props.as_ref().and_then(|p| load_art(p).ok());
                    art_id += 1;
                    art_until = Some(Instant::now() + Duration::from_secs(6));
                    let again = events.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(1200));
                        let _ = again.send(Cmd::Refresh);
                        std::thread::sleep(Duration::from_millis(2800));
                        let _ = again.send(Cmd::Refresh);
                    });
                    start_lyrics(hwnd, lyrics.clone(), key, next.title.clone(), next.artist.clone(), next.dur);
                } else if changed_props && art_until.is_some_and(|t| Instant::now() < t) {
                    if let Some(new) = props.as_ref().and_then(|p| load_art(p).ok()) {
                        if art.as_ref().is_none_or(|old| old.px != new.px) {
                            art = Some(new);
                            art_id += 1;
                        }
                    }
                }
                next.art = art.clone();
                next.art_id = art_id;
            } else {
                last_key.clear();
                art = None;
            }

            let changed = {
                let mut cur = info.lock().unwrap();
                let changed = cur.has != next.has
                    || cur.title != next.title
                    || cur.artist != next.artist
                    || cur.playing != next.playing
                    || cur.art_id != next.art_id
                    || (cur.position() - next.position()).abs() > 2.0;
                *cur = next;
                changed
            };
            if changed {
                post(hwnd, WM_MEDIA);
            }
        }
    });
    tx
}

/// A session being followed: its subscriptions and what the choice needs.
struct Tracked {
    id: String,
    watched: Watched,
    song: bool,
    playing_since: Option<Instant>,
}

fn is_song(s: &GlobalSystemMediaTransportControlsSession) -> bool {
    s.TryGetMediaPropertiesAsync().and_then(|op| op.join()).and_then(|p| p.Artist()).is_ok_and(|a| !a.to_string().trim().is_empty())
}

/// Brings `tracked` up to date with Windows' sessions. Properties are another process's to
/// answer, so they are only asked for after a session reported a change.
fn sync_sessions(mgr: &GlobalSystemMediaTransportControlsSessionManager, tracked: &mut Vec<Tracked>, events: &Sender<Cmd>, changed: bool) {
    let Ok(sessions) = mgr.GetSessions() else { return };
    let sessions: Vec<_> = sessions.into_iter().collect();
    let id = |s: &GlobalSystemMediaTransportControlsSession| s.SourceAppUserModelId().map(|h| h.to_string()).unwrap_or_default();
    let ids: Vec<String> = sessions.iter().map(id).collect();
    tracked.retain(|t| ids.contains(&t.id));
    for (s, sid) in sessions.into_iter().zip(ids) {
        if !tracked.iter().any(|t| t.id == sid) {
            let song = is_song(&s);
            tracked.push(Tracked { id: sid, watched: Watched::new(s, events), song, playing_since: None });
        }
    }
    for t in tracked.iter_mut() {
        let s = &t.watched.session;
        let playing = s
            .GetPlaybackInfo()
            .and_then(|i| i.PlaybackStatus())
            .is_ok_and(|st| st == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing);
        t.playing_since = if playing { t.playing_since.or(Some(Instant::now())) } else { None };
        if changed {
            t.song = is_song(s);
        }
    }
}

fn load_art(p: &GlobalSystemMediaTransportControlsSessionMediaProperties) -> windows::core::Result<Arc<Art>> {
    let stream = p.Thumbnail()?.OpenReadAsync()?.join()?;
    let size = stream.Size()? as u32;
    let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0)?)?;
    let op: IAsyncOperation<u32> = reader.LoadAsync(size)?.cast()?;
    let n = op.join()?;
    let mut buf = vec![0u8; n as usize];
    reader.ReadBytes(&mut buf)?;
    decode_art(&buf)
}

/// Decodes encoded image bytes into a center-cropped square premultiplied BGRA bitmap.
pub fn decode_art(buf: &[u8]) -> windows::core::Result<Arc<Art>> {
    unsafe {
        let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let ist = SHCreateMemStream(Some(buf)).ok_or_else(windows::core::Error::empty)?;
        let dec = wic.CreateDecoderFromStream(&ist, std::ptr::null(), WICDecodeMetadataCacheOnDemand)?;
        let frame = dec.GetFrame(0)?;
        let (mut w, mut h) = (0u32, 0u32);
        frame.GetSize(&mut w, &mut h)?;
        // Center-crop to a square: browser thumbnails are often 16:9 around square art.
        let side = w.min(h);
        let clip = wic.CreateBitmapClipper()?;
        let rect = WICRect { X: ((w - side) / 2) as i32, Y: ((h - side) / 2) as i32, Width: side as i32, Height: side as i32 };
        clip.Initialize(&frame, &rect)?;
        const OUT: u32 = 160;
        let scaler = wic.CreateBitmapScaler()?;
        scaler.Initialize(&clip, OUT, OUT, WICBitmapInterpolationModeFant)?;
        let conv = wic.CreateFormatConverter()?;
        conv.Initialize(&scaler, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)?;
        let mut px = vec![0u8; (OUT * OUT * 4) as usize];
        conv.CopyPixels(std::ptr::null(), OUT * 4, &mut px)?;
        let avg = average(&px);
        Ok(Arc::new(Art { w: OUT, h: OUT, px, avg }))
    }
}

// ---------------------------------------------------------------- lyrics

fn start_lyrics(hwnd: isize, lyrics: Arc<Mutex<Lyrics>>, key: String, title: String, artist: String, dur: f64) {
    {
        let mut l = lyrics.lock().unwrap();
        l.key = key.clone();
        l.status = LyrStatus::Loading;
        l.lines.clear();
    }
    post(hwnd, WM_LYRICS);
    std::thread::spawn(move || {
        crate::sys::name_thread("islet-lyrics");
        unsafe {
            let _ = RoInitialize(RO_INIT_MULTITHREADED);
        }
        // A service that is down says nothing about the song, so keep asking while it plays.
        let mut attempt = 0;
        loop {
            let lines = fetch_any(&title, &artist, dur);
            let mut l = lyrics.lock().unwrap();
            if l.key != key {
                return;
            }
            let failed = lines.is_err();
            match lines {
                Ok(Some(v)) if !v.is_empty() => {
                    l.lines = v;
                    l.status = LyrStatus::Ready;
                }
                _ => l.status = LyrStatus::Missing,
            }
            drop(l);
            post(hwnd, WM_LYRICS);
            if !failed {
                return;
            }
            std::thread::sleep(retry_delay(attempt));
            attempt += 1;
            if lyrics.lock().unwrap().key != key {
                return;
            }
        }
    });
}

/// How long to wait before asking a failing lyrics service again.
fn retry_delay(attempt: u32) -> Duration {
    Duration::from_secs(match attempt {
        0 => 30,
        1 => 120,
        _ => 300,
    })
}

/// The service could not be reached or gave a bad answer.
struct Unreachable;

fn clean_artist(a: &str) -> String {
    let a = a.trim();
    let a = a.strip_suffix(" - Topic").unwrap_or(a);
    let a = a.strip_suffix("VEVO").unwrap_or(a);
    a.trim().to_string()
}

fn clean_title(t: &str) -> String {
    const NOISE: [&str; 7] = ["official", "video", "lyric", "audio", "mv", "visualizer", "remaster"];
    let mut out = String::new();
    let mut depth = 0;
    let mut group = String::new();
    for c in t.chars() {
        match c {
            '(' | '[' => {
                depth += 1;
                group.clear();
            }
            ')' | ']' if depth > 0 => {
                depth -= 1;
                let g = group.to_lowercase();
                if !NOISE.iter().any(|n| g.contains(n)) {
                    out.push('(');
                    out.push_str(&group);
                    out.push(')');
                }
            }
            _ if depth > 0 => group.push(c),
            _ => out.push(c),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Drops "feat. X" / "ft. X" credits, which lyric databases rarely include in titles.
fn strip_feat(t: &str) -> String {
    let lower = t.to_lowercase();
    let cut = [" feat. ", " ft. ", " feat ", " ft "].iter().filter_map(|m| lower.find(m)).min();
    cut.map(|i| t[..i].to_string()).unwrap_or_else(|| t.to_string()).trim().to_string()
}

/// Tries progressively looser queries: as reported, then "Artist - Title" split out of a video
/// title, then the title alone, then a free-text search.
fn fetch_any(title: &str, artist: &str, dur: f64) -> Result<Option<Vec<(f64, String)>>, Unreachable> {
    let t = strip_feat(&clean_title(title));
    let a = clean_artist(artist);
    let mut tries: Vec<(String, String)> = vec![(t.clone(), a.clone())];
    if let Some((left, right)) = t.split_once(" - ").or_else(|| t.split_once(" – ")) {
        tries.push((strip_feat(right.trim()), left.trim().to_string()));
    }
    tries.push((t.clone(), String::new()));
    for (tt, aa) in tries {
        if let Some(v) = fetch(&tt, &aa, dur)?.filter(|v| !v.is_empty()) {
            return Ok(Some(v));
        }
    }
    fetch_query(&format!("{t} {a}"), dur)
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn fetch(title: &str, artist: &str, dur: f64) -> Result<Option<Vec<(f64, String)>>, Unreachable> {
    let mut url = format!("https://lrclib.net/api/search?track_name={}", enc(title));
    if !artist.is_empty() {
        url.push_str(&format!("&artist_name={}", enc(artist)));
    }
    search(&url, dur, false)
}

fn fetch_query(q: &str, dur: f64) -> Result<Option<Vec<(f64, String)>>, Unreachable> {
    // Free text can match other songs, so only accept a result whose length agrees.
    search(&format!("https://lrclib.net/api/search?q={}", enc(q.trim())), dur, true)
}

fn search(url: &str, dur: f64, strict: bool) -> Result<Option<Vec<(f64, String)>>, Unreachable> {
    let client = HttpClient::new().map_err(|_| Unreachable)?;
    let _ = client
        .DefaultRequestHeaders()
        .and_then(|h| h.UserAgent())
        .and_then(|ua| ua.TryParseAdd(&HSTRING::from("Islet/0.1 (Windows)")));
    let uri = Uri::CreateUri(&HSTRING::from(url)).map_err(|_| Unreachable)?;
    let body = client.GetStringAsync(&uri).and_then(|op| op.join()).map_err(|_| Unreachable)?.to_string();
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|_| Unreachable)?;
    Ok(best_lyrics(v.as_array().ok_or(Unreachable)?, dur, strict))
}

/// The best match in a search result: synced lyrics closest in length, else plain ones.
fn best_lyrics(results: &[serde_json::Value], dur: f64, strict: bool) -> Option<Vec<(f64, String)>> {
    let items: Vec<&serde_json::Value> = results
        .iter()
        .filter(|it| !strict || (dur > 0.0 && it.get("duration").and_then(|d| d.as_f64()).is_some_and(|d| (d - dur).abs() < 4.0)))
        .collect();
    let score = |it: &serde_json::Value| -> f64 {
        let d = it.get("duration").and_then(|d| d.as_f64()).unwrap_or(0.0);
        if dur > 0.0 && d > 0.0 {
            (d - dur).abs()
        } else {
            1000.0
        }
    };
    let synced = items
        .iter()
        .filter(|it| it.get("syncedLyrics").and_then(|s| s.as_str()).is_some_and(|s| !s.is_empty()))
        .min_by(|a, b| score(a).total_cmp(&score(b)));
    if let Some(it) = synced {
        return Some(parse_lrc(it["syncedLyrics"].as_str()?));
    }
    let plain = items.iter().find_map(|it| it.get("plainLyrics").and_then(|s| s.as_str()).filter(|s| !s.is_empty()))?;
    Some(plain.lines().map(|l| (-1.0, l.to_string())).collect())
}

pub fn parse_lrc(text: &str) -> Vec<(f64, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let mut rest = line.trim();
        let mut stamps = Vec::new();
        while let Some(r) = rest.strip_prefix('[') {
            let Some((tag, after)) = r.split_once(']') else { break };
            if let Some((m, s)) = tag.split_once(':') {
                if let (Ok(m), Ok(s)) = (m.parse::<f64>(), s.parse::<f64>()) {
                    stamps.push(m * 60.0 + s);
                }
            }
            rest = after;
        }
        let text = rest.trim();
        let text = if text.is_empty() { "♪".to_string() } else { text.to_string() };
        for t in stamps {
            out.push((t, text.clone()));
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(id: &str, playing: bool, song: bool, secs: u64) -> Source<'_> {
        Source { id, playing, song, played: Duration::from_secs(secs) }
    }

    #[test]
    fn a_failing_service_is_asked_less_and_less_often() {
        let waits: Vec<u64> = (0..5).map(|i| retry_delay(i).as_secs()).collect();
        assert_eq!(waits, [30, 120, 300, 300, 300]);
    }

    #[test]
    fn an_empty_search_means_no_lyrics_not_a_failure() {
        assert_eq!(best_lyrics(&[], 200.0, false), None);
        let found = serde_json::json!([{ "duration": 200.0, "syncedLyrics": "[00:01.00] Hello" }]);
        assert_eq!(best_lyrics(found.as_array().unwrap(), 200.0, false), Some(vec![(1.0, "Hello".to_string())]));
    }

    #[test]
    fn a_clip_does_not_take_over_a_playing_song() {
        let s = [src("yt", true, true, 200), src("fb", true, false, 5)];
        assert_eq!(choose(Some("yt"), &s), Some(0));
        // Not even after a long while: only a song can take over from a song.
        let s = [src("yt", true, true, 200), src("fb", true, false, 60)];
        assert_eq!(choose(Some("yt"), &s), Some(0));
    }

    #[test]
    fn another_song_takes_over_after_a_while() {
        assert_eq!(choose(Some("a"), &[src("a", true, true, 99), src("b", true, true, 5)]), Some(0));
        assert_eq!(choose(Some("a"), &[src("a", true, true, 99), src("b", true, true, 15)]), Some(1));
    }

    #[test]
    fn a_stopped_source_gives_way_at_once_songs_first() {
        let s = [src("yt", false, true, 0), src("clip", true, false, 1), src("sp", true, true, 1)];
        assert_eq!(choose(Some("yt"), &s), Some(2));
        assert_eq!(choose(Some("yt"), &[src("yt", false, true, 0), src("x", false, true, 0)]), Some(0));
    }

    #[test]
    fn nothing_shown_yet() {
        assert_eq!(choose(None, &[]), None);
        assert_eq!(choose(None, &[src("a", false, false, 0)]), Some(0));
    }
}
