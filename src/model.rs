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
            ],
        );
    }
}

// ---------------------------------------------------------------- water

/// Cup sizes and daily goals (ml) the Water settings offer.
pub const CUPS: [u32; 5] = [150, 200, 250, 330, 500];
pub const GOALS: [u32; 4] = [1500, 2000, 2500, 3000];

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

// ---------------------------------------------------------------- focus timer

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Focus,
    Short,
    Long,
}

impl Phase {
    pub fn secs(self) -> f64 {
        match self {
            Phase::Focus => 25.0 * 60.0,
            Phase::Short => 5.0 * 60.0,
            Phase::Long => 15.0 * 60.0,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Phase::Focus => "Focus time",
            Phase::Short => "Short break",
            Phase::Long => "Long break",
        }
    }
    pub fn chip(self) -> &'static str {
        match self {
            Phase::Focus => "Pomodoro",
            Phase::Short => "Short Break",
            Phase::Long => "Long Break",
        }
    }
}

// ---------------------------------------------------------------- shelf

pub struct Shelf {
    pub items: Vec<PathBuf>,
}

pub const SHELF_MAX: usize = 10;

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
    pub fn add(&mut self, paths: Vec<PathBuf>) {
        for p in paths {
            if !self.items.contains(&p) {
                self.items.insert(0, p);
            }
        }
        self.items.truncate(SHELF_MAX);
        self.save();
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
