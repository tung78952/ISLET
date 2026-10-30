//! Focus sessions and Pomodoro, from Luma: a session runs for some minutes with an optional
//! label; a Pomodoro is rounds of work with breaks between them (a longer one after every
//! fourth). The interval reminders keep quiet while working and come in the breaks.

use std::{collections::BTreeMap, path::Path};

use chrono::{DateTime, Local, TimeDelta};
use serde::{Deserialize, Serialize};

use super::config::{strip_bom, write_json};
use super::scheduler::minutes as unit;

/// "A few more minutes" at the end of a focus session.
pub const MORE_MINUTES: u32 = 5;
/// Longest focus session that can be started.
const MAX_MINUTES: u32 = 8 * 60;
/// A Pomodoro's longer break, after every fourth round.
pub const LONG_REST_MINUTES: u32 = 15;
const LONG_REST_EVERY: u32 = 4;

/// A running focus session.
pub struct Focus {
    until: DateTime<Local>,
    /// Length of the stretch now running (work or break).
    pub minutes: u32,
    /// What it's for ("IELTS reading"); may be empty.
    pub label: String,
    /// Time left when it was paused, while it is.
    paused_left: Option<TimeDelta>,
    pub pomodoro: Option<Pomodoro>,
}

/// Rounds of `work` minutes with `rest` minutes between them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pomodoro {
    pub work: u32,
    pub rest: u32,
    pub rounds: u32,
    /// 1-based.
    pub round: u32,
    pub resting: bool,
}

impl Pomodoro {
    /// Moves on from the stretch that just ended: the next stretch's minutes and the minutes of
    /// work just finished (0 after a break).
    fn advance(&mut self) -> (u32, u32) {
        if self.resting {
            self.round += 1;
            self.resting = false;
            (self.work, 0)
        } else {
            self.resting = true;
            let rest = if self.round % LONG_REST_EVERY == 0 { LONG_REST_MINUTES } else { self.rest };
            (rest, self.work)
        }
    }
}

/// What `Focus::over` found.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    /// The session is over; carries the whole session's minutes for the time's-up card.
    End(u32),
    /// A Pomodoro moved on to a break or its next round; carries minutes of work just done.
    Phase(u32),
}

impl Focus {
    pub fn new(minutes: u32, label: &str, pomodoro: Option<Pomodoro>, now: DateTime<Local>) -> Self {
        let minutes = minutes.clamp(1, MAX_MINUTES);
        Focus { until: now + unit(minutes), minutes, label: label.trim().chars().take(60).collect(), paused_left: None, pomodoro }
    }

    pub fn pomodoro(work: u32, rest: u32, rounds: u32, label: &str, now: DateTime<Local>) -> Self {
        let p = Pomodoro { work: work.clamp(5, 120), rest: rest.clamp(1, 30), rounds: rounds.clamp(1, 12), round: 1, resting: false };
        Self::new(p.work, label, Some(p), now)
    }

    /// A break on its own (the Pomodoro page's "Short Break"): ends when the break does.
    pub fn rest(minutes: u32, now: DateTime<Local>) -> Self {
        let p = Pomodoro { work: 25, rest: minutes, rounds: 1, round: 1, resting: true };
        Self::new(minutes, "", Some(p), now)
    }

    /// Demo captures: a session part-way through.
    pub fn set_left(&mut self, left: TimeDelta, now: DateTime<Local>) {
        self.until = now + left;
    }

    pub fn left(&self, now: DateTime<Local>) -> TimeDelta {
        self.paused_left.unwrap_or(self.until - now).max(TimeDelta::zero())
    }

    pub fn paused(&self) -> bool {
        self.paused_left.is_some()
    }

    /// A Pomodoro's break is running.
    pub fn resting(&self) -> bool {
        self.pomodoro.is_some_and(|p| p.resting)
    }

    pub fn toggle_pause(&mut self, now: DateTime<Local>) {
        match self.paused_left.take() {
            Some(left) => self.until = now + left,
            None => self.paused_left = Some(self.left(now)),
        }
    }

    /// Ends the running stretch now (skips a break or the rest of a round).
    pub fn skip(&mut self, now: DateTime<Local>) {
        self.paused_left = None;
        self.until = now;
    }

    /// Minutes of work done so far in the running stretch (a break counts none).
    pub fn worked(&self, now: DateTime<Local>) -> u32 {
        if self.resting() {
            return 0;
        }
        let left = (self.left(now).num_seconds() + 59) / unit(1).num_seconds().max(1);
        self.minutes.saturating_sub(left as u32)
    }

    /// Called on each tick: a Pomodoro goes on to its break or next round; at the very end the
    /// session is over.
    pub fn over(&mut self, now: DateTime<Local>) -> Option<Step> {
        if self.paused() || now < self.until {
            return None;
        }
        let total = self.pomodoro.map_or(self.minutes, |p| p.work * p.rounds);
        let Some(p) = self.pomodoro.as_mut() else { return Some(Step::End(self.minutes)) };
        // After the last round's work it is over; a break only comes last when it was started
        // on its own.
        if p.round >= p.rounds {
            return Some(Step::End(if p.resting { 0 } else { total }));
        }
        let (minutes, worked) = p.advance();
        self.minutes = minutes;
        self.until = now + unit(minutes);
        Some(Step::Phase(worked))
    }
}

/// What focus remembers between runs (focus.json): minutes per day, the lengths used lately.
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FocusStore {
    /// "YYYY-MM-DD" -> minutes focused that day.
    pub history: BTreeMap<String, u32>,
    /// Lengths started lately, newest first.
    pub recent: Vec<u32>,
    /// Where Luma's floating timer was (kept for Luma's settings page).
    pub pos: Option<(i32, i32)>,
}

const FOCUS_FILE: &str = "focus.json";
const HISTORY_DAYS: usize = 120;

impl FocusStore {
    pub fn load(dir: &Path) -> Self {
        std::fs::read_to_string(dir.join(FOCUS_FILE))
            .ok()
            .and_then(|text| serde_json::from_str(strip_bom(&text)).ok())
            .unwrap_or_default()
    }

    pub fn add(&mut self, dir: Option<&Path>, day: &str, minutes: u32) {
        *self.history.entry(day.to_string()).or_default() += minutes;
        while self.history.len() > HISTORY_DAYS {
            let oldest = self.history.keys().next().cloned().unwrap_or_default();
            self.history.remove(&oldest);
        }
        if let Some(dir) = dir {
            write_json(dir, FOCUS_FILE, self);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn pomodoro_alternates_work_and_breaks() {
        let mut p = Pomodoro { work: 25, rest: 5, rounds: 6, round: 1, resting: false };
        assert_eq!(p.advance(), (5, 25));
        assert!(p.resting);
        assert_eq!(p.advance(), (25, 0));
        assert_eq!(p.round, 2);
        p.round = 4;
        assert_eq!(p.advance(), (LONG_REST_MINUTES, 25));
    }

    #[test]
    fn a_pomodoro_ends_after_its_last_round_and_a_lone_break_after_itself() {
        let now = Local.with_ymd_and_hms(2026, 10, 30, 9, 0, 0).unwrap();
        let mut f = Focus::pomodoro(25, 5, 2, "IELTS", now);
        let t = |m: i64| now + TimeDelta::minutes(m);
        assert_eq!(f.over(t(25)), Some(Step::Phase(25)));
        assert!(f.resting());
        assert_eq!(f.over(t(30)), Some(Step::Phase(0)));
        assert_eq!(f.over(t(55)), Some(Step::End(50)));
        let mut b = Focus::rest(5, now);
        assert_eq!(b.over(t(4)), None);
        assert_eq!(b.over(t(5)), Some(Step::End(0)));
    }
}
