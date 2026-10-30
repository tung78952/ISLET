//! Luma's own sense of timing, so there is nothing to set but on and off.
//!
//! - How often: water is spread over the user's day (from when they usually start to bedtime)
//!   and slows right down once the day's cups are in; eyes, stretch and breathing follow fixed,
//!   well-known rhythms. Each is stretched out a little (up to 1.5×) when the user keeps
//!   skipping it.
//! - When exactly: a reminder that falls due waits a few minutes for a natural break (the user
//!   just came back, stopped typing for a moment, or switched apps) instead of cutting in
//!   mid-sentence. At hours when the user usually skips it, it waits longer.
//! - How firmly: the care level (light / normal / strict) sets the popup, the heads-up and how
//!   soon an unanswered reminder asks again.
//!
//! What it learns (outcomes of the last weeks, when the day usually starts) stays in
//! habits.json on this PC.

use super::config::{Care, Config, FullscreenApps, Kind, PopupStyle, strip_bom, write_json};
use chrono::{DateTime, Local, NaiveTime, TimeDelta, Timelike};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, collections::HashMap, path::Path};

const HABITS_FILE: &str = "habits.json";
/// Outcomes kept, and how far back they count.
const MAX_EVENTS: usize = 3000;
const LEARN_DAYS: i64 = 14;
/// Below this many answers for a reminder, nothing is adjusted yet.
const MIN_EVENTS: usize = 8;
/// Once the goal is reached, water comes this rarely ("thirsty? have a little more").
const WATER_AFTER_GOAL: u32 = 120;
const WATER_MIN: u32 = 30;
const WATER_MAX: u32 = 120;
/// Eyes: the 20-20-20 rule. Stretch: get up about every 50 minutes. Breathing: now and then.
const EYES_MINUTES: u32 = 20;
const STRETCH_MINUTES: u32 = 50;
const BREATHE_MINUTES: u32 = 90;
/// How long a due reminder may wait for a natural break.
const HOLD_MINUTES: i64 = 8;
const HOLD_EYES_MINUTES: i64 = 4;
/// At an hour the user usually skips it, a reminder waits up to this long.
const HOLD_BAD_HOUR_MINUTES: i64 = 20;

/// One answer to a reminder.
#[derive(Clone, Serialize, Deserialize)]
pub struct Event {
    /// Unix seconds.
    t: i64,
    kind: String,
    /// "done", "skip", "snooze" or "timeout" (left unanswered).
    outcome: String,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Habits {
    events: Vec<Event>,
    /// "YYYY-MM-DD" -> "HH:MM": when the PC was first used that day.
    first_seen: BTreeMap<String, String>,
}

impl Habits {
    pub fn load(dir: &Path) -> Self {
        std::fs::read_to_string(dir.join(HABITS_FILE))
            .ok()
            .and_then(|text| serde_json::from_str(strip_bom(&text)).ok())
            .unwrap_or_default()
    }

    fn save(&self, dir: &Path) {
        write_json(dir, HABITS_FILE, self);
    }

    /// Notes how a reminder was answered.
    pub fn record(&mut self, dir: &Path, kind: &str, outcome: &str, now: DateTime<Local>) {
        self.events.push(Event { t: now.timestamp(), kind: kind.into(), outcome: outcome.into() });
        if self.events.len() > MAX_EVENTS {
            self.events.drain(..self.events.len() - MAX_EVENTS);
        }
        self.save(dir);
    }

    /// The first activity of the day, once a day.
    pub fn note_first_seen(&mut self, dir: &Path, now: DateTime<Local>, idle_secs: u64) {
        let day = now.format("%Y-%m-%d").to_string();
        if idle_secs > 60 || now.hour() < 4 || self.first_seen.contains_key(&day) {
            return;
        }
        self.first_seen.insert(day, now.format("%H:%M").to_string());
        while self.first_seen.len() > 60 {
            let oldest = self.first_seen.keys().next().cloned().unwrap_or_default();
            self.first_seen.remove(&oldest);
        }
        self.save(dir);
    }

    /// When the user's day usually starts: the median of the last week's first activity,
    /// 08:00 until there is any.
    pub fn day_start(&self) -> NaiveTime {
        let mut mins: Vec<u32> = self
            .first_seen
            .values()
            .rev()
            .take(7)
            .filter_map(|t| NaiveTime::parse_from_str(t, "%H:%M").ok())
            .map(|t| t.hour() * 60 + t.minute())
            .collect();
        if mins.is_empty() {
            return NaiveTime::from_hms_opt(8, 0, 0).unwrap_or_default();
        }
        mins.sort_unstable();
        let m = mins[mins.len() / 2].clamp(4 * 60, 12 * 60);
        NaiveTime::from_hms_opt(m / 60, m % 60, 0).unwrap_or_default()
    }

    fn recent<'a>(&'a self, kind: &'a str, now: DateTime<Local>) -> impl Iterator<Item = &'a Event> + 'a {
        let since = (now - TimeDelta::days(LEARN_DAYS)).timestamp();
        self.events.iter().filter(move |e| e.kind == kind && e.t >= since)
    }

    /// How much to stretch a reminder's rhythm: 1 normally, up to 1.5 when it's mostly
    /// skipped or left unanswered.
    pub fn spacing(&self, kind: &str, now: DateTime<Local>) -> f32 {
        let answers: Vec<&Event> = self.recent(kind, now).collect();
        if answers.len() < MIN_EVENTS {
            return 1.0;
        }
        let ignored = answers.iter().filter(|e| e.outcome == "skip" || e.outcome == "timeout").count();
        let rate = ignored as f32 / answers.len() as f32;
        (1.0 + (rate - 0.4).max(0.0) * 1.25).min(1.5)
    }

    /// True at an hour of day when the user mostly ignores this reminder.
    fn bad_hour(&self, kind: &str, now: DateTime<Local>) -> bool {
        let hour = now.hour();
        let (mut done, mut ignored) = (0, 0);
        for e in self.recent(kind, now) {
            let Some(t) = DateTime::from_timestamp(e.t, 0) else { continue };
            if t.with_timezone(&Local).hour() != hour {
                continue;
            }
            match e.outcome.as_str() {
                "done" => done += 1,
                "skip" | "timeout" => ignored += 1,
                _ => {}
            }
        }
        done + ignored >= 4 && ignored * 10 >= (done + ignored) * 7
    }

    /// The hour (0–23) the user most often does this reminder, with enough answers to tell.
    fn best_hour(&self, kind: &str, now: DateTime<Local>) -> Option<u32> {
        let mut by_hour = [0u32; 24];
        for e in self.recent(kind, now).filter(|e| e.outcome == "done") {
            if let Some(t) = DateTime::from_timestamp(e.t, 0) {
                by_hour[t.with_timezone(&Local).hour() as usize] += 1;
            }
        }
        let (hour, n) = by_hour.iter().enumerate().max_by_key(|(_, n)| **n)?;
        (*n >= 4).then_some(hour as u32)
    }

    /// Local hours of the "done" answers for `kind` since `since` (Unix seconds).
    pub fn done_hours(&self, kind: &str, since: i64) -> Vec<u32> {
        self.events
            .iter()
            .filter(|e| e.kind == kind && e.outcome == "done" && e.t >= since)
            .filter_map(|e| DateTime::from_timestamp(e.t, 0).map(|t| t.with_timezone(&Local).hour()))
            .collect()
    }

    fn answers(&self, now: DateTime<Local>) -> usize {
        let since = (now - TimeDelta::days(LEARN_DAYS)).timestamp();
        self.events.iter().filter(|e| e.t >= since).count()
    }
}

/// Minutes between water reminders: the cups still to drink spread over what's left of the day
/// (from its usual start, or now, to bedtime), between 30 and 120; once the goal is reached,
/// every two hours.
pub fn water_minutes(day_start: NaiveTime, bedtime: NaiveTime, now: NaiveTime, cups: u32, goal: u32) -> u32 {
    let left = goal.saturating_sub(cups);
    if left == 0 {
        return WATER_AFTER_GOAL;
    }
    let mins = |t: NaiveTime| (t.hour() * 60 + t.minute()) as i64;
    let from = mins(now).max(mins(day_start));
    // Bedtime after midnight belongs to the same day.
    let mut until = mins(bedtime);
    if until < mins(day_start) {
        until += 24 * 60;
    }
    let span = (until - from).max(60);
    ((span / left as i64) as u32).clamp(WATER_MIN, WATER_MAX)
}

/// Fills in everything the user no longer sets: each reminder's rhythm, and the popup style,
/// heads-up and snooze of the care level.
pub fn apply(cfg: &mut Config, habits: &Habits, cups: u32, now: DateTime<Local>) {
    let bedtime = NaiveTime::parse_from_str(&cfg.reminders.sleep.time, "%H:%M")
        .ok()
        .filter(|_| cfg.reminders.sleep.enabled)
        .unwrap_or_else(|| NaiveTime::from_hms_opt(23, 0, 0).unwrap_or_default());
    let scale = |base: u32, kind: Kind| ((base as f32 * habits.spacing(kind.id(), now)).round() as u32).max(1);
    let water = water_minutes(habits.day_start(), bedtime, now.time(), cups, cfg.water_goal);
    let r = &mut cfg.reminders;
    r.water.minutes = scale(water, Kind::Water).min(WATER_AFTER_GOAL);
    r.eyes.minutes = scale(EYES_MINUTES, Kind::Eyes);
    r.stretch.minutes = scale(STRETCH_MINUTES, Kind::Stretch);
    r.breathe.minutes = scale(BREATHE_MINUTES, Kind::Breathe);
    r.eyes.seconds = 20;
    r.stretch.seconds = 15;
    r.breathe.seconds = 60;
    r.water.seconds = 5;
    r.sleep.snooze_minutes = 15;

    let (style, heads_up, snooze) = match cfg.care {
        Care::Light => (PopupStyle::Corner, false, 15),
        Care::Normal => (PopupStyle::Corner, true, 10),
        Care::Strict => (PopupStyle::Fullscreen, true, 5),
    };
    cfg.popup_style = style;
    cfg.heads_up = heads_up;
    cfg.heads_up_seconds = 10;
    cfg.snooze_minutes = snooze;
    cfg.fullscreen_apps = FullscreenApps::Corner;
    cfg.idle_reset_minutes = 5;
}

/// Seconds before "Skip" shows on a strict popup.
pub fn skip_after(cfg: &Config) -> u32 {
    if cfg.care == Care::Strict { 10 } else { 0 }
}

/// What the ticker notices between ticks, to find natural breaks.
#[derive(Default)]
pub struct Live {
    prev_idle: u64,
    /// The user came back after a while away.
    returned_at: Option<DateTime<Local>>,
    window: isize,
    window_changed_at: Option<DateTime<Local>>,
    /// Since when each due reminder has been waiting for a break.
    held_since: HashMap<Kind, DateTime<Local>>,
}

impl Live {
    pub fn observe(&mut self, now: DateTime<Local>, idle_secs: u64, window: isize) {
        if self.prev_idle >= 120 && idle_secs < 5 {
            self.returned_at = Some(now);
        }
        self.prev_idle = idle_secs;
        if window != 0 && window != self.window {
            if self.window != 0 {
                self.window_changed_at = Some(now);
            }
            self.window = window;
        }
    }

    /// A good moment to step in: just back at the PC, a pause in typing, or just switched apps.
    fn natural_break(&self, now: DateTime<Local>, idle_secs: u64) -> bool {
        let recent = |t: Option<DateTime<Local>>, secs: i64| t.is_some_and(|t| now - t < TimeDelta::seconds(secs));
        (20..300).contains(&idle_secs) || recent(self.returned_at, 90) || recent(self.window_changed_at, 15)
    }

    /// True to keep a due interval reminder waiting a little longer for a natural break.
    /// `resting`: a Pomodoro break, which is the right moment already.
    pub fn hold(&mut self, habits: &Habits, kind: Kind, now: DateTime<Local>, idle_secs: u64, resting: bool) -> bool {
        if kind == Kind::Sleep || resting || self.natural_break(now, idle_secs) {
            self.held_since.remove(&kind);
            return false;
        }
        let since = *self.held_since.entry(kind).or_insert(now);
        let limit = if habits.bad_hour(kind.id(), now) {
            HOLD_BAD_HOUR_MINUTES
        } else if kind == Kind::Eyes {
            HOLD_EYES_MINUTES
        } else {
            HOLD_MINUTES
        };
        if now - since >= TimeDelta::minutes(limit) {
            self.held_since.remove(&kind);
            return false;
        }
        true
    }
}

/// A few lines for the Today tab about what Luma has picked up.
pub fn insights(habits: &Habits, cfg: &Config, cups: u32, now: DateTime<Local>) -> Vec<String> {
    let en = cfg.language == "en";
    let mut out = Vec::new();
    let n = habits.answers(now);
    if n < 20 {
        out.push(if en {
            format!("Luma is still getting to know your rhythm ({n}/20 reminders).")
        } else {
            format!("Luma đang làm quen với nhịp của bạn ({n}/20 lần nhắc).")
        });
    }
    if cfg.reminders.water.enabled {
        let start = habits.day_start().format("%H:%M");
        out.push(match (cups >= cfg.water_goal, en) {
            (true, true) => "Today's water is done. Luma only checks in now and then.".to_string(),
            (true, false) => "Đủ nước hôm nay rồi, Luma chỉ hỏi thăm thưa thôi.".to_string(),
            (false, true) => format!("Your day usually starts around {start}; water is spread out to bedtime, about every {} min.", cfg.reminders.water.minutes),
            (false, false) => format!("Ngày của bạn thường bắt đầu khoảng {start}; Luma chia nước đều tới giờ ngủ, khoảng {} phút một lần.", cfg.reminders.water.minutes),
        });
    }
    for kind in Kind::INTERVAL {
        if !cfg.reminders.interval(kind).is_some_and(|iv| iv.enabled) {
            continue;
        }
        if habits.spacing(kind.id(), now) > 1.05 {
            let name = kind_name(kind, en);
            out.push(if en {
                format!("You often skip {name}, so Luma brings it up less often.")
            } else {
                format!("Bạn hay bỏ qua {name}, nên Luma nhắc thưa hơn một chút.")
            });
        } else if let Some(h) = habits.best_hour(kind.id(), now) {
            let name = kind_name(kind, en);
            out.push(if en { format!("You're most up for {name} around {h}:00.") } else { format!("Bạn hay {name} nhất vào khoảng {h} giờ.") });
        }
    }
    out.truncate(3);
    out
}

fn kind_name(kind: Kind, en: bool) -> &'static str {
    match (kind, en) {
        (Kind::Water, true) => "water",
        (Kind::Water, false) => "uống nước",
        (Kind::Stretch, true) => "stretching",
        (Kind::Stretch, false) => "giãn cơ",
        (Kind::Eyes, true) => "eye breaks",
        (Kind::Eyes, false) => "nghỉ mắt",
        (Kind::Breathe, true) => "breathing",
        (Kind::Breathe, false) => "hít thở",
        (Kind::Sleep, true) => "bedtime",
        (Kind::Sleep, false) => "đi ngủ",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    #[test]
    fn water_spreads_the_cups_over_the_day() {
        // 8 cups from 08:00 to 23:00 (900 min): about every 112 min.
        assert_eq!(water_minutes(t(8, 0), t(23, 0), t(7, 0), 0, 8), 112);
        // Behind in the afternoon: 6 cups left, 15:00 to 23:00 → 80 min.
        assert_eq!(water_minutes(t(8, 0), t(23, 0), t(15, 0), 2, 8), 80);
        // Far behind late: never closer than 30 min.
        assert_eq!(water_minutes(t(8, 0), t(23, 0), t(22, 0), 1, 8), 30);
        // Goal reached: every two hours.
        assert_eq!(water_minutes(t(8, 0), t(23, 0), t(12, 0), 8, 8), WATER_AFTER_GOAL);
        // Bedtime after midnight still counts as tonight.
        assert_eq!(water_minutes(t(8, 0), t(0, 30), t(20, 0), 4, 8), 67);
    }

    #[test]
    fn skipped_reminders_come_less_often() {
        let now = Local.with_ymd_and_hms(2026, 10, 28, 15, 0, 0).unwrap();
        let mut h = Habits::default();
        assert_eq!(h.spacing("stretch", now), 1.0);
        for i in 0..10 {
            let outcome = if i < 8 { "skip" } else { "done" };
            h.events.push(Event { t: now.timestamp() - i * 3600, kind: "stretch".into(), outcome: outcome.into() });
        }
        // 80 % ignored: 1 + 0.4 × 1.25 = 1.5.
        assert!((h.spacing("stretch", now) - 1.5).abs() < 1e-6);
        assert_eq!(h.spacing("water", now), 1.0);
    }

    #[test]
    fn waits_for_a_natural_break_but_not_forever() {
        let now = Local.with_ymd_and_hms(2026, 10, 28, 15, 0, 0).unwrap();
        let h = Habits::default();
        let mut live = Live::default();
        live.observe(now, 1, 7);
        // Typing away: it waits.
        assert!(live.hold(&h, Kind::Water, now, 1, false));
        assert!(live.hold(&h, Kind::Water, now + TimeDelta::minutes(5), 1, false));
        // A pause in typing is a good moment.
        assert!(!live.hold(&h, Kind::Water, now + TimeDelta::minutes(6), 30, false));
        // Not forever: after a few minutes it comes anyway.
        assert!(live.hold(&h, Kind::Stretch, now, 1, false));
        assert!(!live.hold(&h, Kind::Stretch, now + TimeDelta::minutes(HOLD_MINUTES), 1, false));
        // Bedtime and Pomodoro breaks never wait.
        assert!(!live.hold(&h, Kind::Sleep, now, 1, false));
        assert!(!live.hold(&h, Kind::Eyes, now, 1, true));
    }
}
