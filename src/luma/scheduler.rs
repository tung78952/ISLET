//! Decides when each reminder is due. Pure logic: the caller feeds it the clock and idle time.

use super::config::{Config, CustomReminder, Kind};
use chrono::{DateTime, Datelike, Days, Local, NaiveTime, TimeDelta, TimeZone, Timelike};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::OnceLock};

/// Minimum quiet time after a popup before an interval reminder (water, eyes…) may come, so
/// they never pile up one after another.
const MIN_GAP_MINUTES: u32 = 2;
/// A reminder set for a time (the user's own, bedtime) that is already waiting comes this soon
/// after the popup before it closes: one at a time, in the order of their times.
const QUEUE_GAP: TimeDelta = TimeDelta::seconds(20);
/// How long after bedtime the bedtime reminder may still fire (e.g. the PC was off at 23:00).
const BEDTIME_WINDOW: TimeDelta = TimeDelta::hours(3);
/// After a break, water comes no sooner than this.
const WATER_GRACE_MINUTES: u32 = 10;

/// The interval clocks as saved between runs (schedule.json), in Unix seconds.
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    at: i64,
    next: HashMap<Kind, i64>,
    paused_until: Option<i64>,
}

/// A user-made reminder missed (PC off, paused, away) still fires if it's at most this late.
const CUSTOM_WINDOW: TimeDelta = TimeDelta::minutes(30);

/// What the scheduler wants on screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Due {
    Builtin(Kind),
    /// A user-made reminder, by id.
    Custom(String),
    /// A focus session of this many minutes has run out.
    Focus(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Done,
    Snooze,
    Skip,
}

/// One "minute" of configuration. `ISLET_FAST=1` shrinks it to 2 s for testing.
pub fn minutes(m: u32) -> TimeDelta {
    static UNIT: OnceLock<i64> = OnceLock::new();
    let unit = *UNIT.get_or_init(|| if std::env::var_os("ISLET_FAST").is_some() { 2 } else { 60 });
    TimeDelta::seconds(m as i64 * unit)
}

/// Minutes since `bed` if `now` lies between it and the morning, across midnight too.
pub fn minutes_past(now: NaiveTime, bed: NaiveTime, morning: NaiveTime) -> Option<u32> {
    let mins = |t: NaiveTime| (t.hour() * 60 + t.minute()) as i32;
    let since = (mins(now) - mins(bed)).rem_euclid(1440);
    (since < (mins(morning) - mins(bed)).rem_euclid(1440)).then_some(since as u32)
}

pub fn parse_hm(text: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(text, "%H:%M").ok()
}

#[derive(Default)]
pub struct Scheduler {
    next: HashMap<Kind, DateTime<Local>>,
    paused_until: Option<DateTime<Local>>,
    last_closed: Option<DateTime<Local>>,
    /// When the popup (or heads-up) that closed last was opened: a user-made reminder whose
    /// time fell while it was on screen was held up, not missed.
    last_opened: Option<DateTime<Local>>,
    away: bool,
    quiet: bool,
    /// How early a reminder set for a time is handed out, for its heads-up card to count down
    /// to the time (zero without a heads-up).
    lead: TimeDelta,
    bed_last: Option<DateTime<Local>>,
    bed_snooze: Option<DateTime<Local>>,
    custom_snooze: HashMap<String, DateTime<Local>>,
}

impl Scheduler {
    pub fn new(cfg: &Config, now: DateTime<Local>) -> Self {
        let mut s = Self::default();
        s.reset(cfg, now);
        s
    }

    /// Restart every interval timer from now.
    fn reset(&mut self, cfg: &Config, now: DateTime<Local>) {
        for kind in Kind::INTERVAL {
            if let Some(iv) = cfg.reminders.interval(kind) {
                self.next.insert(kind, now + minutes(iv.minutes));
            }
        }
    }

    /// Back from a break (away from the PC, or the PC was off): the screen breaks start over,
    /// since being away *was* a break. Water keeps counting from the last drink, since being
    /// away doesn't mean you drank, but gives a few minutes before it comes.
    fn after_break(&mut self, cfg: &Config, now: DateTime<Local>) {
        let water = self.next.get(&Kind::Water).copied();
        self.reset(cfg, now);
        if let Some(at) = water {
            self.next.insert(Kind::Water, at.max(now + minutes(WATER_GRACE_MINUTES)));
        }
    }

    /// What survives a restart: the interval clocks and a pause, as Unix seconds.
    pub fn snapshot(&self, now: DateTime<Local>) -> Saved {
        Saved {
            at: now.timestamp(),
            next: self.next.iter().map(|(k, t)| (*k, t.timestamp())).collect(),
            paused_until: self.paused_until.map(|t| t.timestamp()),
        }
    }

    /// Picks up where the last run left off. Closed only briefly (less than the "away" time):
    /// as if it never closed. Longer: that was a break (see `after_break`).
    pub fn restore(&mut self, saved: &Saved, cfg: &Config, now: DateTime<Local>) {
        let at = |s: i64| Local.timestamp_opt(s, 0).single();
        let Some(closed) = at(saved.at) else { return };
        for (k, t) in &saved.next {
            if let Some(t) = at(*t) {
                self.next.insert(*k, t);
            }
        }
        self.paused_until = saved.paused_until.and_then(at).filter(|t| *t > now);
        if now - closed >= minutes(cfg.idle_reset_minutes) {
            self.after_break(cfg, now);
        }
    }

    /// Settings changed: restart only the timers whose schedule changed.
    pub fn reconfigure(&mut self, old: &Config, new: &Config, now: DateTime<Local>) {
        for kind in Kind::INTERVAL {
            let (a, b) = (old.reminders.interval(kind), new.reminders.interval(kind));
            if let (Some(a), Some(b)) = (a, b) {
                if a.enabled != b.enabled || a.minutes != b.minutes {
                    self.next.insert(kind, now + minutes(b.minutes));
                }
            }
        }
        if old.reminders.sleep.time != new.reminders.sleep.time {
            self.bed_snooze = None;
        }
    }

    /// User-made reminders that were just added, turned on, or moved to another time or days.
    /// Returns (still ahead today: may come today even if they came at their old time,
    /// already past today: must not come now just because their new time has gone by). Their
    /// snoozes are dropped too, since those were for the old time.
    pub fn rearm_custom(&mut self, old: &Config, new: &Config, now: DateTime<Local>) -> (Vec<String>, Vec<String>) {
        let (mut ahead, mut past) = (Vec::new(), Vec::new());
        for c in new.custom.iter().filter(|c| c.enabled) {
            let changed = match old.custom.iter().find(|o| o.id == c.id) {
                Some(o) => !o.enabled || o.time != c.time || o.days != c.days,
                None => true,
            };
            if !changed {
                continue;
            }
            self.custom_snooze.remove(&c.id);
            match custom_time_today(c, now) {
                Some(at) if at > now => ahead.push(c.id.clone()),
                Some(_) => past.push(c.id.clone()),
                None => {}
            }
        }
        (ahead, past)
    }

    /// Reminders set for a time (the user's own, bedtime) come this much early.
    pub fn set_lead(&mut self, lead: TimeDelta) {
        self.lead = lead;
    }

    /// A popup (or a heads-up card) has just come on screen.
    pub fn opened(&mut self, now: DateTime<Local>) {
        self.last_opened = Some(now);
    }

    /// Quiet until the user resumes (stored as a pause ten years long, so it survives restarts).
    pub fn pause_until_off(&mut self, now: DateTime<Local>) {
        self.pause_for(TimeDelta::days(3650), now);
    }

    /// Paused with no end the user picked (see `pause_until_off`).
    pub fn paused_off(&self, now: DateTime<Local>) -> bool {
        self.paused_until(now).is_some_and(|until| until - now > TimeDelta::days(30))
    }

    pub fn pause_for(&mut self, duration: TimeDelta, now: DateTime<Local>) {
        self.paused_until = Some(now + duration);
    }

    pub fn pause_until_tomorrow(&mut self, now: DateTime<Local>) {
        self.paused_until = (now.date_naive() + Days::new(1))
            .and_time(NaiveTime::MIN)
            .and_local_timezone(Local)
            .earliest();
    }

    pub fn resume(&mut self, cfg: &Config, now: DateTime<Local>) {
        self.paused_until = None;
        self.reset(cfg, now);
    }

    pub fn paused_until(&self, now: DateTime<Local>) -> Option<DateTime<Local>> {
        self.paused_until.filter(|t| *t > now)
    }

    /// Called every few seconds while no popup is open. Returns the reminder to show, if any.
    /// `fired` lists the user-made reminders already shown today.
    pub fn tick(&mut self, cfg: &Config, now: DateTime<Local>, idle_secs: u64, fired: &[String]) -> Option<Due> {
        if let Some(until) = self.paused_until {
            if now < until {
                return None;
            }
            self.resume(cfg, now);
        }

        // Away from the PC long enough: that was a break. Restart the clocks when they return.
        // Bedtime still comes: someone reading or watching without touching anything is
        // just who it is for.
        if idle_secs as i64 >= minutes(cfg.idle_reset_minutes).num_seconds() {
            self.away = true;
            return self.bedtime_due(cfg, now).then_some(Due::Builtin(Kind::Sleep));
        }
        if self.away {
            self.away = false;
            self.after_break(cfg, now);
        }

        // Right after a popup: nothing at all for a moment, and no interval reminder for a while.
        if self.last_closed.is_some_and(|t| now < t + QUEUE_GAP) {
            return None;
        }
        let settling = self.last_closed.is_some_and(|t| now < t + minutes(MIN_GAP_MINUTES));

        if self.bedtime_due(cfg, now) {
            return Some(Due::Builtin(Kind::Sleep));
        }

        if in_quiet_hours(cfg, now) {
            self.quiet = true;
            return None;
        }
        if self.quiet {
            self.quiet = false;
            self.reset(cfg, now);
        }

        // A fixed time the user picked goes before the rolling interval reminders.
        if let Some(id) = self.custom_due(cfg, now, fired) {
            return Some(Due::Custom(id));
        }
        if settling {
            return None;
        }

        Kind::INTERVAL
            .into_iter()
            .filter(|k| cfg.reminders.interval(*k).is_some_and(|iv| iv.enabled))
            .filter_map(|k| self.next.get(&k).filter(|t| **t <= now).map(|t| (k, *t)))
            .min_by_key(|(_, t)| *t)
            .map(|(k, _)| Due::Builtin(k))
    }

    /// The earliest user-made reminder that is due: snoozed and back, or at its time today
    /// (at most `CUSTOM_WINDOW` late) and not shown yet today.
    fn custom_due(&self, cfg: &Config, now: DateTime<Local>, fired: &[String]) -> Option<String> {
        cfg.custom
            .iter()
            .filter(|c| c.enabled)
            .filter_map(|c| {
                let soon = now + self.lead;
                if let Some(at) = self.custom_snooze.get(&c.id) {
                    return (soon >= *at).then_some((c, *at));
                }
                let at = custom_time_today(c, now)?;
                (soon >= at && now - self.custom_since(at) < CUSTOM_WINDOW && !fired.contains(&c.id)).then_some((c, at))
            })
            .min_by_key(|(_, at)| *at)
            .map(|(c, _)| c.id.clone())
    }

    /// When a user-made reminder's half hour starts. Its time fell while another popup was on
    /// screen (or just after): it was held up, so its half hour counts from when that one closed.
    fn custom_since(&self, at: DateTime<Local>) -> DateTime<Local> {
        let held = matches!((self.last_opened, self.last_closed),
            (Some(o), Some(c)) if o <= at && at <= c + minutes(MIN_GAP_MINUTES));
        if held { self.last_closed.unwrap_or(at) } else { at }
    }

    /// User-made reminders whose half hour to catch up has passed without them being shown
    /// today (the PC was off, asleep or left alone), soonest first: the "you missed" card
    /// lists them once.
    pub fn missed_custom(&self, cfg: &Config, now: DateTime<Local>, fired: &[String]) -> Vec<String> {
        let mut missed: Vec<_> = cfg
            .custom
            .iter()
            .filter(|c| c.enabled && !fired.contains(&c.id) && !self.custom_snooze.contains_key(&c.id))
            .filter_map(|c| {
                let at = custom_time_today(c, now)?;
                (at <= now && now - self.custom_since(at) >= CUSTOM_WINDOW).then_some((at, c.id.clone()))
            })
            .collect();
        missed.sort();
        missed.into_iter().map(|(_, id)| id).collect()
    }

    /// A focus session's time's-up card was closed: the next reminder keeps its distance too.
    pub fn finish_focus(&mut self, now: DateTime<Local>) {
        self.last_closed = Some(now);
    }

    /// A user-made reminder's popup was closed.
    pub fn finish_custom(&mut self, id: &str, outcome: Outcome, cfg: &Config, now: DateTime<Local>) {
        self.last_closed = Some(now);
        if outcome == Outcome::Snooze {
            self.custom_snooze.insert(id.to_string(), now + minutes(cfg.snooze_minutes));
        } else {
            self.custom_snooze.remove(id);
        }
    }

    /// The soonest enabled user-made reminder: its title and how long until it fires.
    pub fn custom_next(&self, cfg: &Config, now: DateTime<Local>, fired: &[String]) -> Option<(String, TimeDelta)> {
        cfg.custom
            .iter()
            .filter(|c| c.enabled)
            .filter_map(|c| {
                let at = match self.custom_snooze.get(&c.id) {
                    Some(at) => *at,
                    None => next_custom_time(c, now, fired.contains(&c.id))?,
                };
                Some((c.title.clone(), at - now))
            })
            .min_by_key(|(_, d)| *d)
    }

    /// The popup for `kind` was closed with `outcome`.
    pub fn finish(&mut self, kind: Kind, outcome: Outcome, cfg: &Config, now: DateTime<Local>) {
        self.last_closed = Some(now);
        let Some(iv) = cfg.reminders.interval(kind) else {
            self.bed_last = Some(now);
            self.bed_snooze =
                (outcome == Outcome::Snooze).then(|| now + minutes(cfg.reminders.sleep.snooze_minutes));
            return;
        };
        let wait = if outcome == Outcome::Snooze { cfg.snooze_minutes } else { iv.minutes };
        self.next.insert(kind, now + minutes(wait));

        // Stretching or breathing means you were not staring at the screen either.
        if outcome == Outcome::Done && matches!(kind, Kind::Stretch | Kind::Breathe) {
            self.next.insert(Kind::Eyes, now + minutes(cfg.reminders.eyes.minutes));
        }
    }

    /// The soonest enabled interval reminder and how long until it fires.
    pub fn next_up(&self, cfg: &Config, now: DateTime<Local>) -> Option<(Kind, TimeDelta)> {
        Kind::INTERVAL
            .into_iter()
            .filter(|k| cfg.reminders.interval(*k).is_some_and(|iv| iv.enabled))
            .filter_map(|k| self.next.get(&k).map(|t| (k, *t - now)))
            .min_by_key(|(_, d)| *d)
    }

    /// Seconds until each enabled interval reminder fires (0 when it is already due).
    pub fn next_in(&self, cfg: &Config, now: DateTime<Local>) -> Vec<(Kind, i64)> {
        Kind::INTERVAL
            .into_iter()
            .filter(|k| cfg.reminders.interval(*k).is_some_and(|iv| iv.enabled))
            .filter_map(|k| self.next.get(&k).map(|t| (k, (*t - now).num_seconds().max(0))))
            .collect()
    }

    fn bedtime_due(&self, cfg: &Config, now: DateTime<Local>) -> bool {
        if !cfg.reminders.sleep.enabled {
            return false;
        }
        let soon = now + self.lead;
        if let Some(at) = self.bed_snooze {
            return soon >= at;
        }
        let Some(start) = bedtime_start(cfg, soon) else { return false };
        soon - start < BEDTIME_WINDOW && self.bed_last.is_none_or(|last| last < start)
    }
}

/// When `c` is set to fire today, if today is one of its days.
fn custom_time_today(c: &CustomReminder, now: DateTime<Local>) -> Option<DateTime<Local>> {
    let weekday = now.weekday().num_days_from_monday() as u8;
    if !c.is_once() && !c.days.contains(&weekday) {
        return None;
    }
    now.date_naive().and_time(parse_hm(&c.time)?).and_local_timezone(Local).earliest()
}

/// The next time `c` will fire, within the coming week (now, if it's due and not shown yet).
fn next_custom_time(c: &CustomReminder, now: DateTime<Local>, shown_today: bool) -> Option<DateTime<Local>> {
    let time = parse_hm(&c.time)?;
    (0..8u64).find_map(|offset| {
        let day = now.date_naive() + Days::new(offset);
        let weekday = day.weekday().num_days_from_monday() as u8;
        if !c.is_once() && !c.days.contains(&weekday) {
            return None;
        }
        let at = day.and_time(time).and_local_timezone(Local).earliest()?;
        if offset == 0 && (shown_today || now - at >= CUSTOM_WINDOW) {
            return None;
        }
        Some(at.max(now))
    })
}

/// The most recent bedtime at or before `now`.
fn bedtime_start(cfg: &Config, now: DateTime<Local>) -> Option<DateTime<Local>> {
    let time = parse_hm(&cfg.reminders.sleep.time)?;
    let today = now.date_naive().and_time(time).and_local_timezone(Local).earliest()?;
    Some(if today > now { today - TimeDelta::days(1) } else { today })
}

fn in_quiet_hours(cfg: &Config, now: DateTime<Local>) -> bool {
    let q = &cfg.quiet;
    let (Some(start), Some(end)) = (parse_hm(&q.start), parse_hm(&q.end)) else { return false };
    if !q.enabled || start == end {
        return false;
    }
    let t = now.time();
    if start < end { t >= start && t < end } else { t >= start || t < end }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::config::CustomReminder;
    use chrono::TimeZone;

    /// Wednesday 2026-10-21 at `h:m`.
    fn at(h: u32, m: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 10, 21, h, m, 0).unwrap()
    }

    fn cfg_with(days: Vec<u8>) -> Config {
        let mut cfg = Config::default();
        cfg.reminders.water.enabled = false;
        cfg.reminders.stretch.enabled = false;
        cfg.reminders.eyes.enabled = false;
        cfg.reminders.sleep.enabled = false;
        cfg.custom = vec![CustomReminder { id: "school".into(), time: "13:00".into(), days, ..Default::default() }];
        cfg
    }

    fn due(s: &mut Scheduler, cfg: &Config, now: DateTime<Local>, fired: &[String]) -> Option<Due> {
        s.tick(cfg, now, 0, fired)
    }

    #[test]
    fn past_bedtime_runs_over_midnight_and_ends_in_the_morning() {
        let t = |h, m| NaiveTime::from_hms_opt(h, m, 0).unwrap();
        let (bed, morning) = (t(23, 0), t(7, 0));
        assert_eq!(minutes_past(t(22, 59), bed, morning), None);
        assert_eq!(minutes_past(t(23, 0), bed, morning), Some(0));
        assert_eq!(minutes_past(t(0, 38), bed, morning), Some(98));
        assert_eq!(minutes_past(t(8, 0), bed, morning), None);
    }

    #[test]
    fn fires_at_its_time_once_a_day() {
        let cfg = cfg_with((0..7).collect());
        let mut s = Scheduler::new(&cfg, at(12, 0));
        assert_eq!(due(&mut s, &cfg, at(12, 59), &[]), None);
        assert_eq!(due(&mut s, &cfg, at(13, 0), &[]), Some(Due::Custom("school".into())));
        // Shown already today: not again.
        assert_eq!(due(&mut s, &cfg, at(13, 5), &["school".into()]), None);
    }

    #[test]
    fn bedtime_comes_even_when_left_alone() {
        let mut cfg = Config::default();
        cfg.reminders.sleep.enabled = true;
        cfg.reminders.sleep.time = "23:00".into();
        cfg.quiet.enabled = false;
        let mut s = Scheduler::new(&cfg, at(20, 0));
        let idle = 30 * 60;
        // Water was due long ago, but nobody is at the keyboard: only bedtime comes.
        assert_eq!(s.tick(&cfg, at(22, 30), idle, &[]), None);
        assert_eq!(s.tick(&cfg, at(23, 0), idle, &[]), Some(Due::Builtin(Kind::Sleep)));
    }

    #[test]
    fn missed_once_its_half_hour_is_over() {
        let cfg = cfg_with((0..7).collect());
        let s = Scheduler::new(&cfg, at(9, 0));
        assert!(s.missed_custom(&cfg, at(13, 20), &[]).is_empty()); // still catches up
        assert_eq!(s.missed_custom(&cfg, at(14, 0), &[]), vec!["school".to_string()]);
        assert!(s.missed_custom(&cfg, at(14, 0), &["school".into()]).is_empty());
        assert!(s.missed_custom(&cfg, at(12, 0), &[]).is_empty()); // not yet
    }

    #[test]
    fn catches_up_only_within_half_an_hour() {
        let cfg = cfg_with((0..7).collect());
        let mut s = Scheduler::new(&cfg, at(12, 0));
        assert_eq!(due(&mut s, &cfg, at(13, 29), &[]), Some(Due::Custom("school".into())));
        let mut s = Scheduler::new(&cfg, at(12, 0));
        assert_eq!(due(&mut s, &cfg, at(13, 31), &[]), None);
    }

    #[test]
    fn two_at_the_same_time_both_come() {
        let mut cfg = cfg_with((0..7).collect());
        let mut other = cfg.custom[0].clone();
        other.id = "study".into();
        cfg.custom.push(other);
        let mut s = Scheduler::new(&cfg, at(12, 0));
        assert_eq!(due(&mut s, &cfg, at(13, 0), &[]), Some(Due::Custom("school".into())));
        // The first one stays on screen for 40 minutes; the second still comes after it.
        s.opened(at(13, 0));
        s.finish_custom("school", Outcome::Done, &cfg, at(13, 40));
        let fired = ["school".to_string()];
        assert_eq!(due(&mut s, &cfg, at(13, 40), &fired), None, "keeps its distance");
        assert_eq!(due(&mut s, &cfg, at(13, 41), &fired), Some(Due::Custom("study".into())));
    }

    #[test]
    fn moving_a_reminder_only_brings_it_back_when_still_ahead() {
        let cfg = cfg_with((0..7).collect());
        let mut s = Scheduler::new(&cfg, at(12, 0));
        let mut moved = cfg.clone();
        moved.custom[0].time = "13:45".into();
        // Shown at 13:00, moved to 13:45 at 13:30: it comes again.
        assert_eq!(s.rearm_custom(&cfg, &moved, at(13, 30)), (vec!["school".to_string()], vec![]));
        // Moved to a time already past (a wheel rolling through 13:20 on its way): it must not
        // fire right now.
        let mut back = cfg.clone();
        back.custom[0].time = "13:20".into();
        assert_eq!(s.rearm_custom(&cfg, &back, at(13, 30)), (vec![], vec!["school".to_string()]));
        // Only the title changed: nothing.
        let mut renamed = cfg.clone();
        renamed.custom[0].title = "School".into();
        assert_eq!(s.rearm_custom(&cfg, &renamed, at(12, 30)), (vec![], vec![]));
    }

    #[test]
    fn a_popup_closing_does_not_wake_the_mornings_reminders() {
        // 07:00 and 10:00 passed unseen (the PC was off); a popup at 14:43 must not bring them.
        let mut cfg = cfg_with((0..7).collect());
        cfg.custom[0].time = "07:00".into();
        let mut s = Scheduler::new(&cfg, at(14, 0));
        s.opened(at(14, 43));
        s.finish_custom("other", Outcome::Done, &cfg, at(14, 44));
        assert_eq!(due(&mut s, &cfg, at(14, 46), &[]), None);
    }

    #[test]
    fn same_time_reminders_come_one_after_the_other() {
        let mut cfg = cfg_with((0..7).collect());
        let mut other = cfg.custom[0].clone();
        other.id = "study".into();
        cfg.custom.push(other);
        let mut s = Scheduler::new(&cfg, at(12, 0));
        assert_eq!(due(&mut s, &cfg, at(13, 0), &[]), Some(Due::Custom("school".into())));
        s.opened(at(13, 0));
        s.finish_custom("school", Outcome::Done, &cfg, at(13, 1));
        let fired = ["school".to_string()];
        assert_eq!(due(&mut s, &cfg, at(13, 1), &fired), None, "a moment's pause first");
        let soon = at(13, 1) + TimeDelta::seconds(25);
        assert_eq!(due(&mut s, &cfg, soon, &fired), Some(Due::Custom("study".into())), "then the next one");
    }

    #[test]
    fn interval_reminders_wait_longer_after_a_popup() {
        let mut cfg = cfg_with(vec![]);
        cfg.custom.clear();
        cfg.reminders.eyes.enabled = true;
        let mut s = Scheduler::new(&cfg, at(12, 0));
        s.opened(at(12, 30));
        s.finish_custom("x", Outcome::Done, &cfg, at(12, 31));
        // Eyes is due from 12:20, but waits two minutes after a popup.
        assert_eq!(due(&mut s, &cfg, at(12, 32), &[]), None);
        assert_eq!(due(&mut s, &cfg, at(12, 33), &[]), Some(Due::Builtin(Kind::Eyes)));
    }

    #[test]
    fn a_quick_restart_keeps_the_clocks() {
        let mut cfg = cfg_with(vec![]);
        cfg.reminders.water.enabled = true;
        let s = Scheduler::new(&cfg, at(12, 0));
        let saved = s.snapshot(at(12, 30));
        let mut again = Scheduler::new(&cfg, at(12, 32));
        again.restore(&saved, &cfg, at(12, 32));
        assert_eq!(again.next[&Kind::Water], s.next[&Kind::Water], "water still at 12:45");
    }

    #[test]
    fn a_long_break_restarts_screen_breaks_but_not_water() {
        let mut cfg = cfg_with(vec![]);
        cfg.reminders.water.enabled = true;
        cfg.reminders.eyes.enabled = true;
        let s = Scheduler::new(&cfg, at(12, 0)); // water 12:45, eyes 12:20
        let saved = s.snapshot(at(12, 10));
        let mut again = Scheduler::new(&cfg, at(14, 0));
        again.restore(&saved, &cfg, at(14, 0));
        assert_eq!(again.next[&Kind::Eyes], at(14, 20), "eyes start over");
        assert_eq!(again.next[&Kind::Water], at(14, 10), "water overdue, after a short grace");
    }

    #[test]
    fn a_heads_up_starts_early_so_the_popup_is_on_time() {
        let cfg = cfg_with((0..7).collect());
        let mut s = Scheduler::new(&cfg, at(12, 0));
        s.set_lead(TimeDelta::seconds(30));
        let early = at(13, 0) - TimeDelta::seconds(30);
        assert_eq!(due(&mut s, &cfg, early - TimeDelta::seconds(2), &[]), None);
        assert_eq!(due(&mut s, &cfg, early, &[]), Some(Due::Custom("school".into())));
    }

    #[test]
    fn only_on_its_weekdays() {
        // 2026-10-21 is a Wednesday (2); Monday and Friday only.
        let cfg = cfg_with(vec![0, 4]);
        let mut s = Scheduler::new(&cfg, at(12, 0));
        assert_eq!(due(&mut s, &cfg, at(13, 0), &[]), None);
    }

    #[test]
    fn respects_quiet_hours() {
        let mut cfg = cfg_with((0..7).collect());
        cfg.quiet.enabled = true;
        cfg.quiet.start = "12:30".into();
        cfg.quiet.end = "14:00".into();
        let mut s = Scheduler::new(&cfg, at(12, 0));
        assert_eq!(due(&mut s, &cfg, at(13, 0), &[]), None);
    }

    #[test]
    fn snooze_brings_it_back() {
        let cfg = cfg_with((0..7).collect());
        let mut s = Scheduler::new(&cfg, at(12, 0));
        s.finish_custom("school", Outcome::Snooze, &cfg, at(13, 0));
        let fired = ["school".to_string()];
        assert_eq!(due(&mut s, &cfg, at(13, 4), &fired), None);
        assert_eq!(due(&mut s, &cfg, at(13, 5), &fired), Some(Due::Custom("school".into())));
    }

    #[test]
    fn next_one_is_tomorrow_once_shown() {
        let cfg = cfg_with((0..7).collect());
        let s = Scheduler::new(&cfg, at(12, 0));
        let (_, left) = s.custom_next(&cfg, at(12, 0), &[]).unwrap();
        assert_eq!(left, TimeDelta::hours(1));
        let (_, left) = s.custom_next(&cfg, at(13, 10), &["school".into()]).unwrap();
        assert_eq!(left, TimeDelta::hours(24) - TimeDelta::minutes(10));
    }
}
