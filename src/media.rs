//! Now-playing state from Windows media sessions (SMTC) and synced lyrics from LRCLIB.

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::core::{Interface, HSTRING};
use windows::Foundation::Uri;
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
    std::thread::spawn(move || {
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
        // After a seek the session keeps reporting the old position for a moment; hold the
        // requested one until it catches up so the UI doesn't jump back and forth.
        let mut hold: Option<(f64, Instant)> = None;
        loop {
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(cmd) => {
                    if let Cmd::Seek(t) = cmd {
                        hold = Some((t, Instant::now()));
                    }
                    if let Ok(s) = mgr.GetCurrentSession() {
                        let r = match cmd {
                            Cmd::Toggle => s.TryTogglePlayPauseAsync(),
                            Cmd::Next => s.TrySkipNextAsync(),
                            Cmd::Prev => s.TrySkipPreviousAsync(),
                            Cmd::Seek(secs) => s.TryChangePlaybackPositionAsync((secs.max(0.0) * 1e7) as i64),
                        };
                        let _ = r.and_then(|op| op.join());
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return,
                Err(RecvTimeoutError::Timeout) => {}
            }

            let mut next = MediaInfo::default();
            if let Ok(s) = mgr.GetCurrentSession() {
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
                if key != last_key && !next.title.is_empty() {
                    last_key = key.clone();
                    art = props.as_ref().and_then(|p| load_art(p).ok());
                    art_id += 1;
                    start_lyrics(hwnd, lyrics.clone(), key, next.title.clone(), next.artist.clone(), next.dur);
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
        unsafe {
            let _ = RoInitialize(RO_INIT_MULTITHREADED);
        }
        let lines = fetch_any(&title, &artist, dur);
        let mut l = lyrics.lock().unwrap();
        if l.key != key {
            return;
        }
        match lines {
            Some(v) if !v.is_empty() => {
                l.lines = v;
                l.status = LyrStatus::Ready;
            }
            _ => l.status = LyrStatus::Missing,
        }
        drop(l);
        post(hwnd, WM_LYRICS);
    });
}

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
fn fetch_any(title: &str, artist: &str, dur: f64) -> Option<Vec<(f64, String)>> {
    let t = strip_feat(&clean_title(title));
    let a = clean_artist(artist);
    let mut tries: Vec<(String, String)> = vec![(t.clone(), a.clone())];
    if let Some((left, right)) = t.split_once(" - ").or_else(|| t.split_once(" – ")) {
        tries.push((strip_feat(right.trim()), left.trim().to_string()));
    }
    tries.push((t.clone(), String::new()));
    for (tt, aa) in tries {
        if let Some(v) = fetch(&tt, &aa, dur).filter(|v| !v.is_empty()) {
            return Some(v);
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

fn fetch(title: &str, artist: &str, dur: f64) -> Option<Vec<(f64, String)>> {
    let mut url = format!("https://lrclib.net/api/search?track_name={}", enc(title));
    if !artist.is_empty() {
        url.push_str(&format!("&artist_name={}", enc(artist)));
    }
    search(&url, dur, false)
}

fn fetch_query(q: &str, dur: f64) -> Option<Vec<(f64, String)>> {
    // Free text can match other songs, so only accept a result whose length agrees.
    search(&format!("https://lrclib.net/api/search?q={}", enc(q.trim())), dur, true)
}

fn search(url: &str, dur: f64, strict: bool) -> Option<Vec<(f64, String)>> {
    let client = HttpClient::new().ok()?;
    let _ = client
        .DefaultRequestHeaders()
        .and_then(|h| h.UserAgent())
        .and_then(|ua| ua.TryParseAdd(&HSTRING::from("Islet/0.1 (Windows)")));
    let uri = Uri::CreateUri(&HSTRING::from(url)).ok()?;
    let body = client.GetStringAsync(&uri).ok()?.join().ok()?.to_string();
    let v: serde_json::Value = serde_json::from_str(&body).ok()?;
    let items: Vec<&serde_json::Value> = v
        .as_array()?
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
