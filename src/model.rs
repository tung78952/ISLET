//! Persistent feature state: settings, water reminder, to-do, focus timer and shelf.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use windows::Win32::System::SystemInformation::GetLocalTime;

pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let d = base.join("Islet");
    let _ = fs::create_dir_all(&d);
    d
}

pub fn shelf_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let d = base.join("Islet").join("Shelf");
    let _ = fs::create_dir_all(&d);
    d
}

fn read_kv(name: &str) -> HashMap<String, String> {
    let text = fs::read_to_string(data_dir().join(name)).unwrap_or_default();
    text.lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect()
}

fn write_kv(name: &str, pairs: &[(&str, String)]) {
    let body: String = pairs.iter().map(|(k, v)| format!("{k}={v}\n")).collect();
    let path = data_dir().join(name);
    let tmp = path.with_extension("tmp");
    if fs::write(&tmp, body).is_ok() {
        let _ = fs::rename(&tmp, &path);
    }
}

fn num<T: std::str::FromStr>(m: &HashMap<String, String>, k: &str, d: T) -> T {
    m.get(k).and_then(|v| v.parse().ok()).unwrap_or(d)
}

pub struct Clock {
    pub year: u16,
    pub month: u16,
    pub day: u16,
    pub dow: u16,
    pub hour: u16,
    pub minute: u16,
}

pub fn now() -> Clock {
    let t = unsafe { GetLocalTime() };
    Clock { year: t.wYear, month: t.wMonth, day: t.wDay, dow: t.wDayOfWeek, hour: t.wHour, minute: t.wMinute }
}

impl Clock {
    pub fn ymd(&self) -> u32 {
        self.year as u32 * 10000 + self.month as u32 * 100 + self.day as u32
    }
    pub fn minutes(&self) -> u32 {
        self.hour as u32 * 60 + self.minute as u32
    }
    pub fn date_label(&self) -> String {
        const D: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
        const M: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
        format!("{} {} {} {}", D[self.dow as usize % 7], self.day, M[(self.month as usize + 11) % 12], self.year)
    }
    pub fn time_label(&self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }
}

// ---------------------------------------------------------------- settings

pub struct Settings {
    pub pet: bool,
    pub minimal: bool,
    /// Lyrics text size in the expanded view (15 = about five lines, 26 = one large line).
    pub lyric_size: f32,
    /// Turn off Windows' corner banners for apps whose notifications Islet shows.
    pub hide_banners: bool,
    /// The separate Luma app was turned off (Islet runs its reminders now).
    pub luma_app_off: bool,
    /// Lyrics stay on across restarts.
    pub lyrics: bool,
    /// Notifications show their text at once (off: only when hovered).
    pub show_text: bool,
    /// Focus stretch, break (minutes) and rounds.
    pub pomo_work: u32,
    pub pomo_rest: u32,
    pub pomo_rounds: u32,
    /// How often the lyric bar grows for the eyes: 0 never, 1 every 1-2 min, 2 every 3-5 min.
    pub jam_mode: u8,
    /// Focus page: work and break in rounds, rather than one plain stretch.
    pub pomodoro: bool,
}

impl Settings {
    pub fn load() -> Self {
        let m = read_kv("settings.txt");
        Settings {
            pet: num(&m, "pet", 1u8) == 1,
            minimal: num(&m, "minimal", 0u8) == 1,
            lyric_size: num(&m, "lyric_size", 15.0f32).clamp(15.0, 26.0),
            hide_banners: num(&m, "hide_banners", 1u8) == 1,
            luma_app_off: num(&m, "luma_app_off", 0u8) == 1,
            lyrics: num(&m, "lyrics", 1u8) == 1,
            show_text: num(&m, "show_text", 0u8) == 1,
            pomo_work: num(&m, "pomo_work", 25u32).clamp(5, 120),
            pomo_rest: num(&m, "pomo_rest", 5u32).clamp(1, 30),
            pomo_rounds: num(&m, "pomo_rounds", 4u32).clamp(1, 8),
            jam_mode: num(&m, "jam_mode", 1u8).min(2),
            pomodoro: num(&m, "pomodoro", 0u8) == 1,
        }
    }
    pub fn save(&self) {
        write_kv(
            "settings.txt",
            &[
                ("pet", (self.pet as u8).to_string()),
                ("minimal", (self.minimal as u8).to_string()),
                ("lyric_size", self.lyric_size.to_string()),
                ("hide_banners", (self.hide_banners as u8).to_string()),
                ("luma_app_off", (self.luma_app_off as u8).to_string()),
                ("lyrics", (self.lyrics as u8).to_string()),
                ("show_text", (self.show_text as u8).to_string()),
                ("pomo_work", self.pomo_work.to_string()),
                ("pomo_rest", self.pomo_rest.to_string()),
                ("pomo_rounds", self.pomo_rounds.to_string()),
                ("jam_mode", self.jam_mode.to_string()),
                ("pomodoro", (self.pomodoro as u8).to_string()),
            ],
        );
    }
}

// ---------------------------------------------------------------- water

/// Cup sizes and daily goals (ml) the Water settings offer.
pub const CUPS: [u32; 5] = [150, 200, 250, 330, 500];
pub const GOALS: [u32; 7] = [1000, 1500, 2000, 2500, 3000, 3500, 4000];

// ---------------------------------------------------------------- to-do

pub struct Task {
    pub title: String,
    pub done_day: u32,
    /// Optional "HH:MM" or "HH:MM-HH:MM" schedule.
    pub time: String,
}

pub struct Todo {
    pub tasks: Vec<Task>,
}

impl Todo {
    pub fn load() -> Self {
        let today = now().ymd();
        let text = fs::read_to_string(data_dir().join("todo.txt")).unwrap_or_default();
        let tasks = text
            .lines()
            .filter_map(|l| {
                let mut parts = l.splitn(3, '|');
                let day = parts.next()?;
                let time = parts.next()?;
                let title = parts.next()?;
                Some(Task { title: title.to_string(), done_day: day.parse().unwrap_or(0), time: time.to_string() })
            })
            // Completed tasks from earlier days are cleared on start.
            .filter(|t| t.done_day == 0 || t.done_day == today)
            .collect();
        Todo { tasks }
    }

    pub fn save(&self) {
        let body: String = self.tasks.iter().map(|t| format!("{}|{}|{}\n", t.done_day, t.time, t.title)).collect();
        let _ = fs::write(data_dir().join("todo.txt"), body);
    }

    pub fn done_today(&self) -> usize {
        let today = now().ymd();
        self.tasks.iter().filter(|t| t.done_day == today).count()
    }

    /// Indices with open tasks first, then today's completed ones.
    pub fn order(&self) -> Vec<usize> {
        let mut v: Vec<usize> = (0..self.tasks.len()).filter(|&i| self.tasks[i].done_day == 0).collect();
        v.extend((0..self.tasks.len()).filter(|&i| self.tasks[i].done_day != 0));
        v
    }

    pub fn toggle(&mut self, i: usize) {
        if let Some(t) = self.tasks.get_mut(i) {
            t.done_day = if t.done_day == 0 { now().ymd() } else { 0 };
            self.save();
        }
    }

    pub fn later(&mut self, i: usize) {
        if i < self.tasks.len() {
            let t = self.tasks.remove(i);
            self.tasks.push(t);
            self.save();
        }
    }
}


// ---------------------------------------------------------------- shelf

pub struct Shelf {
    pub items: Vec<PathBuf>,
}

pub const SHELF_MAX: usize = 30;

impl Shelf {
    pub fn load() -> Self {
        let text = fs::read_to_string(data_dir().join("shelf.txt")).unwrap_or_default();
        let items = text.lines().map(PathBuf::from).filter(|p| p.exists()).take(SHELF_MAX).collect();
        Shelf { items }
    }
    pub fn save(&self) {
        let body: String = self.items.iter().map(|p| format!("{}\n", p.display())).collect();
        let _ = fs::write(data_dir().join("shelf.txt"), body);
    }
    /// Free places left on the shelf.
    pub fn room(&self) -> usize {
        SHELF_MAX.saturating_sub(self.items.len())
    }
    /// Takes new files while there is room, in the order they came, and never pushes old ones
    /// off: returns how many were taken and how many did not fit (files already on it count
    /// as neither).
    pub fn add(&mut self, paths: Vec<PathBuf>) -> (usize, usize) {
        let (took, refused) = self.take(paths);
        if took > 0 {
            self.save();
        }
        (took, refused)
    }
    fn take(&mut self, paths: Vec<PathBuf>) -> (usize, usize) {
        let mut fresh: Vec<PathBuf> = Vec::new();
        for p in paths {
            if !self.items.contains(&p) && !fresh.contains(&p) {
                fresh.push(p);
            }
        }
        let took = fresh.len().min(self.room());
        let refused = fresh.len() - took;
        self.items.splice(0..0, fresh.into_iter().take(took));
        (took, refused)
    }
    pub fn remove(&mut self, i: usize) {
        if i < self.items.len() {
            self.items.remove(i);
            self.save();
        }
    }
    pub fn clear(&mut self) {
        self.items.clear();
        self.save();
    }
}

// ---------------------------------------------------------------- lyric offsets

const LYRIC_OFFSETS: &str = "lyric_offsets.txt";
/// How far the words of one song may be moved, either way, in seconds.
pub const LYRIC_OFFSET_MAX: f64 = 30.0;

/// Songs whose words run early or late in the lyrics found for them: seconds to add to the
/// playing position, per song (title and artist).
#[derive(Default)]
pub struct LyricOffsets {
    by_song: HashMap<String, f64>,
}

impl LyricOffsets {
    pub fn load() -> Self {
        Self::parse(&fs::read_to_string(data_dir().join(LYRIC_OFFSETS)).unwrap_or_default())
    }

    fn parse(text: &str) -> Self {
        let by_song = text
            .lines()
            .filter_map(|l| l.rsplit_once('\t'))
            .filter_map(|(k, v)| Some((k.to_string(), v.trim().parse::<f64>().ok()?.clamp(-LYRIC_OFFSET_MAX, LYRIC_OFFSET_MAX))))
            .filter(|(_, v)| *v != 0.0)
            .collect();
        Self { by_song }
    }

    fn text(&self) -> String {
        let mut rows: Vec<_> = self.by_song.iter().collect();
        rows.sort_by(|a, b| a.0.cmp(b.0));
        rows.iter().map(|(k, v)| format!("{k}\t{v}\n")).collect()
    }

    pub fn get(&self, song: &str) -> f64 {
        self.by_song.get(song).copied().unwrap_or(0.0)
    }

    /// Moves a song's words by `by` seconds and returns where they now stand.
    pub fn shift(&mut self, song: &str, by: f64) -> f64 {
        let v = ((self.get(song) + by) * 2.0).round() / 2.0;
        let v = v.clamp(-LYRIC_OFFSET_MAX, LYRIC_OFFSET_MAX);
        if v == 0.0 {
            self.by_song.remove(song);
        } else {
            self.by_song.insert(song.to_string(), v);
        }
        v
    }

    pub fn save(&self) {
        let path = data_dir().join(LYRIC_OFFSETS);
        let tmp = path.with_extension("tmp");
        if fs::write(&tmp, self.text()).is_ok() {
            let _ = fs::rename(&tmp, &path);
        }
    }
}

#[cfg(test)]
mod lyric_offset_tests {
    use super::*;

    #[test]
    fn offsets_round_trip_and_stay_in_range() {
        let mut o = LyricOffsets::default();
        assert_eq!(o.get("a\u{1}b"), 0.0);
        assert_eq!(o.shift("a\u{1}b", 0.5), 0.5);
        assert_eq!(o.shift("a\u{1}b", 100.0), LYRIC_OFFSET_MAX);
        assert_eq!(o.shift("c\u{1}d", -1.0), -1.0);
        let back = LyricOffsets::parse(&o.text());
        assert_eq!(back.get("a\u{1}b"), LYRIC_OFFSET_MAX);
        assert_eq!(back.get("c\u{1}d"), -1.0);
        assert_eq!(o.shift("c\u{1}d", 1.0), 0.0);
        assert!(!o.text().contains("c\u{1}d"), "a song back in sync is not kept");
    }
}

#[cfg(test)]
mod shelf_tests {
    use super::*;

    fn files(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn a_full_shelf_turns_new_files_away_and_keeps_the_old_ones() {
        let old: Vec<PathBuf> = (0..SHELF_MAX - 2).map(|i| PathBuf::from(i.to_string())).collect();
        let mut s = Shelf { items: old.clone() };
        assert_eq!(s.take(files(&["a", "b", "c", "d", "e"])), (2, 3));
        assert_eq!(s.items[..2], files(&["a", "b"])[..]);
        assert_eq!(s.items[2..], old[..]);
        assert_eq!(s.room(), 0);
        assert_eq!(s.take(files(&["f"])), (0, 1));
        assert_eq!(s.items.len(), SHELF_MAX);
    }

    #[test]
    fn files_already_on_the_shelf_are_neither_taken_nor_turned_away() {
        let mut s = Shelf { items: files(&["1"]) };
        assert_eq!(s.take(files(&["1", "a", "a"])), (1, 0));
        assert_eq!(s.items, files(&["a", "1"]));
    }
}
