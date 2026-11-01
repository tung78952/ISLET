//! Luma's reminders inside Islet: water spread over the user's day, eye, stretch and breathing
//! breaks, bedtime, the user's own timed reminders, and focus sessions / Pomodoro. The timing
//! logic (config, scheduler, smart, focus) comes from the Luma app unchanged; `Brain` runs it
//! the way Luma's ticker did and hands Islet what to put in the notch.

// Parts of Luma's logic not used yet (Quiet now, insights, the week table) are kept as they
// are: Luma's Settings window and tray menu come over next and use them.
#[allow(dead_code)]
pub mod config;
pub mod focus;
#[allow(dead_code)]
pub mod scheduler;
#[allow(dead_code)]
pub mod smart;

use std::path::{Path, PathBuf};

use chrono::{DateTime, Local, TimeDelta};

use config::{Care, Config, CustomReminder, Kind, Stats};
use focus::{Focus, FocusStore, Step};
use scheduler::{Due, Outcome, Scheduler};
use smart::{Habits, Live};

/// An unanswered card asks again later, like a snooze (Luma's corner card did the same).
pub const CARD_TIMEOUT: TimeDelta = TimeDelta::seconds(45);
const SCHEDULE_FILE: &str = "schedule.json";
/// Ticks (2 s each) between schedule saves.
const SAVE_EVERY: u32 = 15;

/// What the notch should do after a tick.
#[derive(Debug, PartialEq)]
pub enum Event {
    /// A reminder comes in `secs` seconds: show the heads-up countdown.
    HeadsUp(Due, u32),
    /// Show the reminder card.
    Show(Due),
    /// A Pomodoro moved on: a break (`true`) or the next round of work started.
    Phase { resting: bool, round: u32, rounds: u32, minutes: u32 },
}

/// The reminder Islet is showing (heads-up or card), until it is answered.
pub struct Showing {
    pub due: Due,
    /// Counting down to the card.
    pub heads_up_until: Option<DateTime<Local>>,
    /// When the card itself appeared.
    pub card_at: Option<DateTime<Local>>,
}

pub struct Brain {
    /// Where the data lives; None in demo mode (nothing is written).
    dir: Option<PathBuf>,
    pub cfg: Config,
    pub stats: Stats,
    habits: Habits,
    sched: Scheduler,
    live: Live,
    pub focus: Option<Focus>,
    focus_store: FocusStore,
    pub showing: Option<Showing>,
    /// Off until the separate Luma app has been turned off, so nothing reminds twice.
    pub enabled: bool,
    ticks: u32,
}

/// Islet's copy of Luma's data.
pub fn data_dir() -> PathBuf {
    crate::model::data_dir().join("luma")
}

/// The Luma app's own folder.
fn luma_app_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("com.pause.desktop"))
}

/// First run: Luma's settings, today's counters, history and what it learned come along. The
/// Luma app's folder is only read, never changed.
fn migrate(dir: &Path) {
    if dir.join("config.json").exists() {
        return;
    }
    let _ = std::fs::create_dir_all(dir);
    let Some(src) = luma_app_dir() else { return };
    for name in ["config.json", "stats.json", "history.json", "habits.json", "schedule.json", "focus.json"] {
        let _ = std::fs::copy(src.join(name), dir.join(name));
    }
}

fn today(now: DateTime<Local>) -> String {
    now.format("%Y-%m-%d").to_string()
}

impl Brain {
    pub fn open(now: DateTime<Local>) -> Self {
        let dir = data_dir();
        migrate(&dir);
        Self::load(Some(dir), now)
    }

    /// Defaults and no files, for demo captures.
    pub fn demo(now: DateTime<Local>) -> Self {
        Self::load(None, now)
    }

    fn load(dir: Option<PathBuf>, now: DateTime<Local>) -> Self {
        let cfg = dir.as_deref().map(|d| Config::load(d).0).unwrap_or_default();
        let stats = dir.as_deref().map(|d| Stats::load(d, &today(now))).unwrap_or_else(|| Stats { date: today(now), ..Stats::default() });
        let habits = dir.as_deref().map(Habits::load).unwrap_or_default();
        let mut cfg = cfg;
        smart::apply(&mut cfg, &habits, stats.water, now);
        let mut sched = Scheduler::new(&cfg, now);
        if let Some(d) = dir.as_deref() {
            let saved = std::fs::read_to_string(d.join(SCHEDULE_FILE))
                .ok()
                .and_then(|t| serde_json::from_str(config::strip_bom(&t)).ok())
                .unwrap_or_default();
            sched.restore(&saved, &cfg, now);
        }
        let focus_store = dir.as_deref().map(FocusStore::load).unwrap_or_default();
        Brain { dir, cfg, stats, habits, sched, live: Live::default(), focus: None, focus_store, showing: None, enabled: true, ticks: 0 }
    }

    fn save_stats(&self) {
        if let Some(d) = &self.dir {
            self.stats.save(d);
        }
    }

    pub fn save_config(&self) {
        if let Some(d) = &self.dir {
            self.cfg.save(d);
        }
    }

    pub fn save_schedule(&self, now: DateTime<Local>) {
        if let Some(d) = &self.dir {
            config::write_json(d, SCHEDULE_FILE, &self.sched.snapshot(now));
        }
    }

    /// Every 2 s. `idle`: seconds without input; `window`: the foreground window; `busy`: a
    /// fullscreen app or presentation is in front (no heads-up, card without taking focus).
    pub fn tick(&mut self, now: DateTime<Local>, idle: u64, window: isize, busy: bool) -> Vec<Event> {
        let mut out = Vec::new();
        self.ticks = self.ticks.wrapping_add(1);
        if self.stats.roll(&today(now)) {
            self.save_stats();
        }
        self.live.observe(now, idle, window);
        if let Some(d) = &self.dir {
            self.habits.note_first_seen(d, now, idle);
        }
        smart::apply(&mut self.cfg, &self.habits, self.stats.water, now);
        if self.ticks % SAVE_EVERY == 0 {
            self.save_schedule(now);
        }
        // Focus and Pomodoro run on even while reminders are off.
        if let Some(step) = self.focus.as_mut().and_then(|f| f.over(now)) {
            match step {
                Step::Phase(worked) => {
                    self.add_focus(worked, now);
                    if let Some((f, p)) = self.focus.as_ref().and_then(|f| f.pomodoro.map(|p| (f, p))) {
                        out.push(Event::Phase { resting: p.resting, round: p.round, rounds: p.rounds, minutes: f.minutes });
                    }
                }
                Step::End(total) => {
                    let f = self.focus.take();
                    if let Some(f) = f.filter(|f| !f.resting()) {
                        self.add_focus(f.minutes, now);
                    }
                    if total > 0 && self.showing.is_none() {
                        self.sched.opened(now);
                        self.showing = Some(Showing { due: Due::Focus(total), heads_up_until: None, card_at: None });
                        out.push(Event::Show(Due::Focus(total)));
                    }
                }
            }
        }
        // A heads-up that has counted down becomes the card.
        if let Some(s) = self.showing.as_mut().filter(|s| s.heads_up_until.is_some_and(|t| now >= t)) {
            s.heads_up_until = None;
            out.push(Event::Show(s.due.clone()));
        }
        if !self.enabled || self.showing.is_some() {
            return out;
        }
        let heads_up = self.cfg.heads_up && !busy;
        let secs = if heads_up { self.cfg.heads_up_seconds } else { 0 };
        self.sched.set_lead(TimeDelta::seconds(secs as i64));
        let resting = self.focus.as_ref().is_some_and(Focus::resting);
        let working = self.focus.as_ref().is_some_and(|f| !f.resting());
        let due = self
            .sched
            .tick(&self.cfg, now, idle, &self.stats.fired)
            // While working, the interval reminders keep quiet; bedtime and the user's own come.
            .filter(|due| !(working && matches!(due, Due::Builtin(k) if *k != Kind::Sleep)))
            .filter(|due| !matches!(due, Due::Builtin(kind) if self.live.hold(&self.habits, *kind, now, idle, resting)));
        if let Some(due) = due {
            self.sched.opened(now);
            if heads_up {
                self.showing = Some(Showing { due: due.clone(), heads_up_until: Some(now + TimeDelta::seconds(secs as i64)), card_at: None });
                out.push(Event::HeadsUp(due, secs));
            } else {
                self.showing = Some(Showing { due: due.clone(), heads_up_until: None, card_at: None });
                out.push(Event::Show(due));
            }
        }
        out
    }

    /// The card for the showing reminder is on screen now (its timeout counts from here).
    pub fn card_shown(&mut self, now: DateTime<Local>) {
        if let Some(s) = self.showing.as_mut().filter(|s| s.heads_up_until.is_none() && s.card_at.is_none()) {
            s.card_at = Some(now);
        }
    }

    /// The card has been out of sight for `by`: its timeout waits.
    pub fn delay_card(&mut self, by: TimeDelta) {
        if let Some(t) = self.showing.as_mut().and_then(|s| s.card_at.as_mut()) {
            *t += by;
        }
    }

    /// "Start now" on the heads-up.
    pub fn heads_up_now(&mut self) -> Option<Due> {
        let s = self.showing.as_mut().filter(|s| s.heads_up_until.is_some())?;
        s.heads_up_until = None;
        Some(s.due.clone())
    }

    /// An unanswered card has been up long enough (never on the strict level, which waits).
    pub fn timed_out(&self, now: DateTime<Local>) -> bool {
        self.cfg.care != Care::Strict && self.showing.as_ref().and_then(|s| s.card_at).is_some_and(|t| now - t >= CARD_TIMEOUT)
    }

    /// Seconds before "Later" may be pressed (strict level).
    pub fn skip_after(&self) -> u32 {
        smart::skip_after(&self.cfg)
    }

    /// The showing reminder was answered (or `timed_out`). Returns a focus session to start:
    /// "Start" on a reminder with a timer.
    pub fn answer(&mut self, outcome: Outcome, timed_out: bool, now: DateTime<Local>) -> Option<(u32, String)> {
        let s = self.showing.take()?;
        let answer = if timed_out { "timeout".to_string() } else { format!("{outcome:?}").to_lowercase() };
        match &s.due {
            Due::Builtin(kind) => {
                if s.heads_up_until.is_none() {
                    self.record(kind.id(), &answer, now);
                }
                self.sched.finish(*kind, outcome, &self.cfg, now);
                if outcome == Outcome::Done {
                    self.stats.bump(*kind);
                    self.save_stats();
                }
                None
            }
            Due::Custom(id) => {
                self.record("custom", &answer, now);
                self.finish_custom(id, outcome, now);
                let c = self.cfg.custom.iter().find(|c| &c.id == id).filter(|c| c.start && c.minutes > 0);
                (outcome == Outcome::Done).then(|| c.map(|c| (c.minutes, c.title.clone()))).flatten()
            }
            Due::Focus(_) => {
                self.sched.finish_focus(now);
                (outcome == Outcome::Snooze).then(|| (focus::MORE_MINUTES, String::new()))
            }
        }
    }

    fn record(&mut self, kind: &str, answer: &str, now: DateTime<Local>) {
        if let Some(d) = &self.dir {
            self.habits.record(d, kind, answer, now);
        }
    }

    /// A user-made reminder closed: it has had its turn today; a one-off one turns itself off.
    fn finish_custom(&mut self, id: &str, outcome: Outcome, now: DateTime<Local>) {
        self.sched.finish_custom(id, outcome, &self.cfg, now);
        if !self.stats.fired.iter().any(|f| f == id) {
            self.stats.fired.push(id.to_string());
        }
        if outcome == Outcome::Done {
            self.stats.custom += 1;
        }
        self.save_stats();
        if outcome != Outcome::Snooze {
            if let Some(c) = self.cfg.custom.iter_mut().find(|c| c.id == id && c.is_once()) {
                c.enabled = false;
                self.save_config();
            }
        }
    }

    // ------------------------------------------------------------ water

    /// A cup from the Water page (no card asked): counted, and the water clock starts over.
    pub fn drink(&mut self, now: DateTime<Local>) {
        if matches!(self.showing.as_ref().map(|s| &s.due), Some(Due::Builtin(Kind::Water))) {
            self.answer(Outcome::Done, false, now);
            return;
        }
        self.record("water", "done", now);
        self.sched.finish(Kind::Water, Outcome::Done, &self.cfg, now);
        self.stats.bump(Kind::Water);
        self.save_stats();
    }

    pub fn reset_water(&mut self) {
        self.stats.water = 0;
        self.save_stats();
    }

    /// Changes the care settings and reschedules for them.
    pub fn configure(&mut self, change: impl FnOnce(&mut Config), now: DateTime<Local>) {
        let old = self.cfg.clone();
        change(&mut self.cfg);
        self.sched.reconfigure(&old, &self.cfg, now);
        self.save_config();
    }

    pub fn set_water_enabled(&mut self, on: bool, now: DateTime<Local>) {
        let old = self.cfg.clone();
        self.cfg.reminders.water.enabled = on;
        self.sched.reconfigure(&old, &self.cfg, now);
        self.save_config();
    }

    /// Seconds until the water reminder, if it is on.
    pub fn water_in(&self, now: DateTime<Local>) -> Option<i64> {
        self.next_in(Kind::Water, now)
    }

    /// Seconds until a reminder, if it is on.
    pub fn next_in(&self, kind: Kind, now: DateTime<Local>) -> Option<i64> {
        self.sched.next_in(&self.cfg, now).into_iter().find(|(k, _)| *k == kind).map(|(_, s)| s)
    }

    pub fn add_date(&mut self, title: &str, date: &str, now: DateTime<Local>) {
        if self.cfg.dates.len() < config::MAX_DATES {
            self.cfg.dates.push(config::ImportantDate { id: format!("d{}", now.timestamp_millis()), title: title.to_string(), date: date.to_string() });
            self.save_config();
        }
    }

    pub fn delete_date(&mut self, id: &str) {
        self.cfg.dates.retain(|d| d.id != id);
        self.save_config();
    }

    /// Cups drunk in each hour today (from the answers Luma keeps).
    pub fn cups_by_hour(&self, now: DateTime<Local>) -> [u32; 24] {
        let mut out = [0u32; 24];
        let start = now.date_naive().and_hms_opt(0, 0, 0).and_then(|t| t.and_local_timezone(Local).earliest());
        let Some(start) = start else { return out };
        for hour in self.habits.done_hours("water", start.timestamp()) {
            out[hour as usize % 24] += 1;
        }
        out
    }

    /// When the user's day usually starts and bedtime, for the Water page.
    pub fn water_day(&self) -> (chrono::NaiveTime, chrono::NaiveTime) {
        let bed = scheduler::parse_hm(&self.cfg.reminders.sleep.time).unwrap_or_default();
        (self.habits.day_start(), bed)
    }

    // ------------------------------------------------------------ focus

    fn add_focus(&mut self, minutes: u32, now: DateTime<Local>) {
        if minutes == 0 {
            return;
        }
        self.stats.focus += minutes;
        self.save_stats();
        self.focus_store.add(self.dir.as_deref(), &today(now), minutes);
    }

    pub fn start_focus(&mut self, minutes: u32, label: &str, now: DateTime<Local>) {
        self.stop_focus(now);
        self.focus = Some(Focus::new(minutes, label, None, now));
    }

    pub fn start_pomodoro(&mut self, work: u32, rest: u32, rounds: u32, label: &str, now: DateTime<Local>) {
        self.stop_focus(now);
        self.focus = Some(Focus::pomodoro(work, rest, rounds, label, now));
    }

    pub fn start_rest(&mut self, minutes: u32, now: DateTime<Local>) {
        self.stop_focus(now);
        self.focus = Some(Focus::rest(minutes, now));
    }

    /// Stopped early: the minutes worked so far still count.
    pub fn stop_focus(&mut self, now: DateTime<Local>) {
        if let Some(f) = self.focus.take() {
            self.add_focus(f.worked(now), now);
        }
    }

    pub fn toggle_pause(&mut self, now: DateTime<Local>) {
        if let Some(f) = self.focus.as_mut() {
            f.toggle_pause(now);
        }
    }

    pub fn skip_phase(&mut self, now: DateTime<Local>) {
        if let Some(f) = self.focus.as_mut() {
            f.skip(now);
        }
    }

    // ------------------------------------------------------------ tasks (user-made reminders)

    /// Today's user-made reminders (their days include today, or once and still on), with
    /// whether each is done (shown today) — the To-do page.
    pub fn tasks_today(&self, now: DateTime<Local>) -> Vec<(&CustomReminder, bool)> {
        use chrono::Datelike;
        let weekday = now.weekday().num_days_from_monday() as u8;
        let mut v: Vec<_> = self
            .cfg
            .custom
            .iter()
            .filter(|c| if c.is_once() { c.enabled || self.stats.fired.contains(&c.id) } else { c.enabled && c.days.contains(&weekday) })
            .map(|c| (c, self.stats.fired.contains(&c.id)))
            .collect();
        v.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.time.cmp(&b.0.time)));
        v
    }

    /// A one-off task for today at `time` ("HH:MM"); `minutes` > 0 makes "Start" begin focus.
    pub fn add_task(&mut self, title: &str, time: &str, minutes: u32, now: DateTime<Local>) {
        let id = format!("t{}", now.timestamp_millis());
        let c = CustomReminder {
            id,
            title: title.trim().chars().take(60).collect(),
            time: time.to_string(),
            days: Vec::new(),
            emoji: "📝".into(),
            start: minutes > 0,
            minutes: minutes.min(8 * 60),
            ..CustomReminder::default()
        };
        if scheduler::parse_hm(&c.time).is_none() || self.cfg.custom.len() >= config::MAX_CUSTOM {
            return;
        }
        let old = self.cfg.clone();
        self.cfg.custom.push(c);
        self.sched.rearm_custom(&old, &self.cfg, now);
        self.save_config();
    }

    /// Ticked off (or back) on the To-do page.
    /// Adds a reminder, or changes the one with this id.
    pub fn save_reminder(&mut self, id: Option<&str>, title: &str, time: &str, days: Vec<u8>, now: DateTime<Local>) {
        let old = self.cfg.clone();
        let title: String = title.trim().chars().take(60).collect();
        let at = id.and_then(|id| self.cfg.custom.iter().position(|c| c.id == id));
        match at {
            Some(i) => {
                let c = &mut self.cfg.custom[i];
                c.title = title;
                c.time = time.to_string();
                c.days = days;
                c.enabled = true;
            }
            None if self.cfg.custom.len() < config::MAX_CUSTOM => {
                self.cfg.custom.push(CustomReminder { id: format!("t{}", now.timestamp_millis()), title, time: time.to_string(), days, ..CustomReminder::default() });
            }
            None => return,
        }
        self.sched.rearm_custom(&old, &self.cfg, now);
        self.save_config();
    }

    pub fn set_reminder_on(&mut self, id: &str, on: bool, now: DateTime<Local>) {
        let old = self.cfg.clone();
        if let Some(c) = self.cfg.custom.iter_mut().find(|c| c.id == id) {
            c.enabled = on;
        }
        self.sched.rearm_custom(&old, &self.cfg, now);
        self.save_config();
    }

    pub fn set_task_done(&mut self, id: &str, done: bool) {
        let was = self.stats.fired.iter().any(|f| f == id);
        if done && !was {
            self.stats.fired.push(id.to_string());
            self.stats.custom += 1;
        } else if !done && was {
            self.stats.fired.retain(|f| f != id);
            self.stats.custom = self.stats.custom.saturating_sub(1);
        }
        self.save_stats();
        if let Some(c) = self.cfg.custom.iter_mut().find(|c| c.id == id && c.is_once()) {
            c.enabled = !done;
            self.save_config();
        }
    }

    pub fn delete_task(&mut self, id: &str) {
        self.cfg.custom.retain(|c| c.id != id);
        self.stats.fired.retain(|f| f != id);
        self.save_stats();
        self.save_config();
    }

    pub fn task(&self, id: &str) -> Option<&CustomReminder> {
        self.cfg.custom.iter().find(|c| c.id == id)
    }

}
