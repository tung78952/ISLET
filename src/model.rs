//! Persistent feature state: settings, water reminder, to-do, focus timer and shelf.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

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
}

impl Settings {
    pub fn load() -> Self {
        let m = read_kv("settings.txt");
        Settings {
            pet: num(&m, "pet", 1u8) == 1,
            minimal: num(&m, "minimal", 0u8) == 1,
            lyric_size: num(&m, "lyric_size", 15.0f32).clamp(15.0, 26.0),
        }
    }
    pub fn save(&self) {
        write_kv(
            "settings.txt",
            &[
                ("pet", (self.pet as u8).to_string()),
                ("minimal", (self.minimal as u8).to_string()),
                ("lyric_size", self.lyric_size.to_string()),
            ],
        );
    }
}

// ---------------------------------------------------------------- water

pub const BOTTLES: [u32; 5] = [250, 330, 500, 750, 1000];
pub const GOALS: [u32; 4] = [1500, 2000, 2500, 3000];

pub enum WaterAlert {
    Drink(u32),
    Refill,
}

pub struct Water {
    pub bottle: u32,
    pub goal: u32,
    pub start: u32,
    pub end: u32,
    pub day: u32,
    pub taken: u32,
    pub inb: u32,
    pub fired: u32,
    pub hours: [u32; 24],
    pub on: bool,
    pub snooze_at: Option<u32>,
}

impl Water {
    pub fn load() -> Self {
        let m = read_kv("water.txt");
        let mut hours = [0u32; 24];
        if let Some(h) = m.get("hours") {
            for (i, v) in h.split(',').take(24).enumerate() {
                hours[i] = v.parse().unwrap_or(0);
            }
        }
        let bottle = num(&m, "bottle", 500);
        let mut w = Water {
            bottle,
            goal: num(&m, "goal", 2000),
            start: num(&m, "start", 8 * 60),
            end: num(&m, "end", 17 * 60),
            day: num(&m, "day", 0),
            taken: num(&m, "taken", 0),
            inb: num(&m, "inb", bottle),
            fired: num(&m, "fired", 0),
            hours,
            on: num(&m, "on", 1u8) == 1,
            snooze_at: None,
        };
        w.roll_day();
        w
    }

    pub fn save(&self) {
        let hours = self.hours.iter().map(|h| h.to_string()).collect::<Vec<_>>().join(",");
        write_kv(
            "water.txt",
            &[
                ("bottle", self.bottle.to_string()),
                ("goal", self.goal.to_string()),
                ("start", self.start.to_string()),
                ("end", self.end.to_string()),
                ("day", self.day.to_string()),
                ("taken", self.taken.to_string()),
                ("inb", self.inb.to_string()),
                ("fired", self.fired.to_string()),
                ("hours", hours),
                ("on", (self.on as u8).to_string()),
            ],
        );
    }

    /// Resets daily counters when the local date changes. Returns true on rollover.
    pub fn roll_day(&mut self) -> bool {
        let today = now().ymd();
        if self.day == today {
            return false;
        }
        self.day = today;
        self.taken = 0;
        self.fired = 0;
        self.hours = [0; 24];
        self.inb = self.bottle;
        self.snooze_at = None;
        self.save();
        true
    }

    /// Reminders fire every 15 minutes inside the active hours, offset by half an interval.
    pub const INTERVAL: u32 = 15;

    pub fn slots(&self) -> u32 {
        (self.end.saturating_sub(self.start) / Self::INTERVAL).max(1)
    }

    fn slot_minute(&self, k: u32) -> u32 {
        self.start + Self::INTERVAL / 2 + k * Self::INTERVAL
    }

    /// Millilitres to drink at each reminder.
    pub fn each(&self) -> u32 {
        (self.goal as f32 / self.slots() as f32).round().max(1.0) as u32
    }

    pub fn per_bottle(&self) -> u32 {
        (self.bottle as f32 / self.each() as f32).round() as u32
    }

    pub fn refills(&self) -> u32 {
        self.goal.div_ceil(self.bottle.max(1)).saturating_sub(1)
    }

    pub fn percent(&self) -> u32 {
        (self.taken as f32 * 100.0 / self.goal.max(1) as f32).round() as u32
    }

    /// Next reminder time; after the last slot this is tomorrow's first one.
    pub fn next_minute(&self, now_min: u32) -> u32 {
        if let Some(s) = self.snooze_at {
            return s;
        }
        (0..self.slots()).map(|k| self.slot_minute(k)).find(|&t| t > now_min && t <= self.end).unwrap_or(self.slot_minute(0))
    }

    pub fn check_due(&mut self, now_min: u32) -> Option<WaterAlert> {
        if !self.on || self.taken >= self.goal {
            return None;
        }
        let due = if let Some(s) = self.snooze_at {
            if now_min >= s {
                self.snooze_at = None;
                true
            } else {
                false
            }
        } else {
            let passed = (0..self.slots()).filter(|&k| self.slot_minute(k) <= now_min.min(self.end)).count() as u32;
            if passed > self.fired {
                self.fired = passed;
                true
            } else {
                false
            }
        };
        if !due {
            return None;
        }
        self.save();
        Some(if self.inb < self.each() { WaterAlert::Refill } else { WaterAlert::Drink(self.each()) })
    }

    pub fn drank(&mut self, hour: usize) {
        let e = self.each();
        self.taken += e;
        self.inb = self.inb.saturating_sub(e);
        self.hours[hour.min(23)] += e;
        self.snooze_at = None;
        self.save();
    }

    pub fn refilled(&mut self) {
        self.inb = self.bottle;
        self.save();
    }

    pub fn snooze(&mut self, now_min: u32) {
        self.snooze_at = Some(now_min + 10);
    }

    pub fn reset(&mut self) {
        self.taken = 0;
        self.hours = [0; 24];
        self.inb = self.bottle;
        self.snooze_at = None;
        self.save();
    }
}

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

pub struct Pomo {
    pub phase: Phase,
    pub remaining: f64,
    pub ends_at: Option<Instant>,
    pub done_focus: u32,
    pub task: Option<String>,
}

impl Pomo {
    pub fn new() -> Self {
        Pomo { phase: Phase::Focus, remaining: Phase::Focus.secs(), ends_at: None, done_focus: 0, task: None }
    }
    pub fn running(&self) -> bool {
        self.ends_at.is_some()
    }
    pub fn left(&self) -> f64 {
        match self.ends_at {
            Some(t) => t.saturating_duration_since(Instant::now()).as_secs_f64(),
            None => self.remaining,
        }
    }
    pub fn label(&self) -> String {
        let s = self.left().ceil() as u64;
        format!("{:02}:{:02}", s / 60, s % 60)
    }
    pub fn start(&mut self) {
        if self.ends_at.is_none() {
            self.ends_at = Some(Instant::now() + std::time::Duration::from_secs_f64(self.remaining));
        }
    }
    pub fn pause(&mut self) {
        self.remaining = self.left();
        self.ends_at = None;
    }
    pub fn set_phase(&mut self, p: Phase) {
        self.phase = p;
        self.remaining = p.secs();
        self.ends_at = None;
    }
    pub fn reset(&mut self) {
        self.set_phase(self.phase);
    }
    /// Advances when the current phase ends; returns the phase that just started.
    pub fn tick(&mut self) -> Option<Phase> {
        if self.ends_at.is_none() || self.left() > 0.0 {
            return None;
        }
        self.advance(true)
    }
    pub fn advance(&mut self, auto_start_break: bool) -> Option<Phase> {
        let next = if self.phase == Phase::Focus {
            self.done_focus += 1;
            if self.done_focus % 4 == 0 {
                Phase::Long
            } else {
                Phase::Short
            }
        } else {
            Phase::Focus
        };
        self.set_phase(next);
        if next != Phase::Focus && auto_start_break {
            self.start();
        }
        Some(next)
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
