//! Persisted settings and today's counters, stored as JSON in the app config dir.

use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

const CONFIG_FILE: &str = "config.json";
const STATS_FILE: &str = "stats.json";
const HISTORY_FILE: &str = "history.json";
/// Days of counters kept in the history.
const HISTORY_DAYS: usize = 120;

/// Past days' counters, by date ("YYYY-MM-DD").
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct History {
    days: std::collections::BTreeMap<String, Stats>,
}

impl History {
    pub fn load(dir: &Path) -> Self {
        fs::read_to_string(dir.join(HISTORY_FILE))
            .ok()
            .and_then(|text| serde_json::from_str(strip_bom(&text)).ok())
            .unwrap_or_default()
    }

    pub fn get(&self, date: &str) -> Option<&Stats> {
        self.days.get(date)
    }

    /// The first day on record ("YYYY-MM-DD").
    pub fn first_day(&self) -> Option<&str> {
        self.days.keys().next().map(String::as_str)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Water,
    Stretch,
    Eyes,
    Breathe,
    Sleep,
}

impl Kind {
    pub const ALL: [Kind; 5] = [Kind::Water, Kind::Stretch, Kind::Eyes, Kind::Breathe, Kind::Sleep];
    /// Reminders that repeat on a fixed interval (everything except bedtime).
    pub const INTERVAL: [Kind; 4] = [Kind::Water, Kind::Stretch, Kind::Eyes, Kind::Breathe];

    pub fn id(self) -> &'static str {
        match self {
            Kind::Water => "water",
            Kind::Stretch => "stretch",
            Kind::Eyes => "eyes",
            Kind::Breathe => "breathe",
            Kind::Sleep => "sleep",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.id() == id)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PopupStyle {
    #[default]
    Fullscreen,
    Corner,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    #[default]
    BottomRight,
}

impl Corner {
    pub fn is_left(self) -> bool {
        matches!(self, Corner::TopLeft | Corner::BottomLeft)
    }
    pub fn is_top(self) -> bool {
        matches!(self, Corner::TopLeft | Corner::TopRight)
    }
}

/// How firmly Luma reminds (smart::apply turns it into the popup style, heads-up and snooze).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Care {
    /// A corner card that slips away on its own; asks again in 15 minutes.
    Light,
    /// A corner card after a short heads-up; asks again in 10 minutes.
    #[default]
    Normal,
    /// Fullscreen after a heads-up; "Skip" shows after 10 seconds; asks again in 5 minutes.
    Strict,
}

/// Size of the floating focus timer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FocusWidget {
    #[default]
    Small,
    Large,
}

/// What to do when a fullscreen app (game, video, slideshow) is in front.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FullscreenApps {
    Defer,
    /// A corner card that leaves the keyboard and mouse with the game.
    #[default]
    Corner,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Interval {
    pub enabled: bool,
    pub minutes: u32,
    /// Length of the guided activity (per stretch move, eye break, breathing session).
    pub seconds: u32,
}

impl Interval {
    const fn new(enabled: bool, minutes: u32, seconds: u32) -> Self {
        Self { enabled, minutes, seconds }
    }
}

impl Default for Interval {
    fn default() -> Self {
        Self::new(true, 45, 20)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Bedtime {
    pub enabled: bool,
    /// Local time, "HH:MM".
    pub time: String,
    pub snooze_minutes: u32,
    /// Shut the PC down (after a cancellable countdown) when the user goes to bed.
    pub shutdown: bool,
}

impl Default for Bedtime {
    fn default() -> Self {
        Self { enabled: true, time: "23:00".into(), snooze_minutes: 15, shutdown: true }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Reminders {
    pub water: Interval,
    pub stretch: Interval,
    pub eyes: Interval,
    pub breathe: Interval,
    pub sleep: Bedtime,
}

impl Default for Reminders {
    fn default() -> Self {
        Self {
            water: Interval::new(true, 45, 0),
            stretch: Interval::new(true, 60, 15),
            eyes: Interval::new(true, 20, 20),
            breathe: Interval::new(false, 90, 60),
            sleep: Bedtime::default(),
        }
    }
}

impl Reminders {
    pub fn interval(&self, kind: Kind) -> Option<&Interval> {
        match kind {
            Kind::Water => Some(&self.water),
            Kind::Stretch => Some(&self.stretch),
            Kind::Eyes => Some(&self.eyes),
            Kind::Breathe => Some(&self.breathe),
            Kind::Sleep => None,
        }
    }

    fn interval_mut(&mut self, kind: Kind) -> Option<&mut Interval> {
        match kind {
            Kind::Water => Some(&mut self.water),
            Kind::Stretch => Some(&mut self.stretch),
            Kind::Eyes => Some(&mut self.eyes),
            Kind::Breathe => Some(&mut self.breathe),
            Kind::Sleep => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct QuietHours {
    pub enabled: bool,
    pub start: String,
    pub end: String,
}

impl Default for QuietHours {
    fn default() -> Self {
        Self { enabled: false, start: "22:00".into(), end: "08:00".into() }
    }
}

/// Light or dark windows; `Auto` follows the Windows app theme.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    #[default]
    Auto,
    Light,
    Dark,
}

/// Look of the windows: the character (who also brings every colour) and light or dark.
/// The id names a character the frontend bundles; one it doesn't know falls back to its
/// default there.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Appearance {
    pub mascot: String,
    pub mode: ThemeMode,
}

impl Default for Appearance {
    fn default() -> Self {
        Self { mascot: "luma".into(), mode: ThemeMode::Auto }
    }
}

impl Appearance {
    fn sanitized(mut self) -> Self {
        // The default character was called Neko before she became Lumi.
        if self.mascot == "neko" {
            self.mascot = "lumi".into();
        }
        if !is_id(&self.mascot) {
            self.mascot = Self::default().mascot;
        }
        self
    }
}

fn is_id(s: &str) -> bool {
    (1..=32).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Most user-made reminders kept; more is noise in the list rather than a real need.
pub const MAX_CUSTOM: usize = 50;

/// A reminder the user made: a fixed time, on some weekdays (or once).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CustomReminder {
    /// Stable id made by the settings page, `[a-z0-9-]`.
    pub id: String,
    pub enabled: bool,
    pub title: String,
    pub note: String,
    /// Local time, "HH:MM".
    pub time: String,
    /// Weekdays it repeats on, 0 = Monday … 6 = Sunday. Empty: once, then it turns itself off.
    pub days: Vec<u8>,
    pub emoji: String,
    /// Which animation plays (an id the frontend knows, e.g. "walk").
    pub anim: String,
    /// Slot in the character's palette, so the colour follows the character.
    pub color: u8,
    /// It's for starting something (study, leaving for school): the card says "Start", not
    /// "Done".
    pub start: bool,
    /// With `start`: "Start" also begins a focus timer this long (0: no timer).
    pub minutes: u32,
}

impl Default for CustomReminder {
    fn default() -> Self {
        Self {
            id: String::new(),
            enabled: true,
            title: String::new(),
            note: String::new(),
            time: "17:00".into(),
            days: (0..7).collect(),
            emoji: "⏰".into(),
            anim: "remind".into(),
            color: 0,
            start: false,
            minutes: 0,
        }
    }
}

impl CustomReminder {
    pub fn is_once(&self) -> bool {
        self.days.is_empty()
    }
}

/// Most important dates kept.
pub const MAX_DATES: usize = 50;

/// A day that matters (an exam, a trip), counted down to in the Today tab.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ImportantDate {
    /// Stable id made by the settings page, `[a-z0-9-]`.
    pub id: String,
    pub title: String,
    /// Local date, "YYYY-MM-DD".
    pub date: String,
}

fn clip(s: &str, max: usize) -> String {
    s.trim().chars().take(max).collect()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    pub language: String,
    pub autostart: bool,
    pub sound: bool,
    /// 0–100, for every reminder sound.
    pub sound_volume: u8,
    /// Play the character's spoken line with each sound, when she has one.
    pub voice: bool,
    /// Language of those lines ("ja", "en"); a character without it falls back to plain sounds.
    pub voice_lang: String,
    pub popup_style: PopupStyle,
    pub corner: Corner,
    pub fullscreen_apps: FullscreenApps,
    pub snooze_minutes: u32,
    /// Being away from the keyboard this long counts as a break and restarts the timers.
    pub idle_reset_minutes: u32,
    pub quiet: QuietHours,
    pub reminders: Reminders,
    pub appearance: Appearance,
    pub custom: Vec<CustomReminder>,
    /// Before a fullscreen popup, a small card counts down 10 seconds in the corner first.
    pub heads_up: bool,
    /// How long the heads-up card counts down (5–60 s). A reminder set for a time starts its
    /// heads-up this much early, so its popup still opens on time.
    pub heads_up_seconds: u32,
    pub dates: Vec<ImportantDate>,
    pub focus_widget: FocusWidget,
    /// Say hello the first time each day, with what today holds.
    pub greeting: bool,
    /// How firmly Luma reminds. The popup style, heads-up, snooze and every reminder's rhythm
    /// are worked out from it and from the user's habits (smart.rs), not set by hand.
    pub care: Care,
    /// Cups a day (Islet's water page sets it as a goal in litres).
    pub water_goal: u32,
    /// Millilitres in one cup, for showing litres.
    pub cup_ml: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            language: "vi".into(),
            autostart: true,
            sound: true,
            sound_volume: 70,
            voice: true,
            voice_lang: "ja".into(),
            popup_style: PopupStyle::default(),
            corner: Corner::default(),
            fullscreen_apps: FullscreenApps::default(),
            snooze_minutes: 5,
            idle_reset_minutes: 5,
            quiet: QuietHours::default(),
            reminders: Reminders::default(),
            appearance: Appearance::default(),
            custom: Vec::new(),
            heads_up: true,
            heads_up_seconds: 10,
            dates: Vec::new(),
            focus_widget: FocusWidget::default(),
            greeting: true,
            care: Care::default(),
            water_goal: 8,
            cup_ml: 250,
        }
    }
}

impl Config {
    /// Returns the saved config and whether this is the first run.
    pub fn load(dir: &Path) -> (Self, bool) {
        match fs::read_to_string(dir.join(CONFIG_FILE)) {
            Ok(text) => (serde_json::from_str::<Self>(strip_bom(&text)).unwrap_or_default().sanitized(), false),
            Err(_) => (Self::default(), true),
        }
    }

    pub fn save(&self, dir: &Path) {
        write_json(dir, CONFIG_FILE, self);
    }

    /// Clamp values coming from disk or the settings page into sane ranges.
    pub fn sanitized(mut self) -> Self {
        if self.language != "en" {
            self.language = "vi".into();
        }
        self.snooze_minutes = self.snooze_minutes.clamp(1, 120);
        self.sound_volume = self.sound_volume.min(100);
        if !(self.voice_lang.len() == 2 && self.voice_lang.bytes().all(|b| b.is_ascii_lowercase())) {
            self.voice_lang = "ja".into();
        }
        self.idle_reset_minutes = self.idle_reset_minutes.clamp(1, 60);
        self.water_goal = self.water_goal.clamp(1, 20);
        self.cup_ml = self.cup_ml.clamp(100, 1000);
        self.heads_up_seconds = self.heads_up_seconds.clamp(5, 60);
        self.reminders.sleep.snooze_minutes = self.reminders.sleep.snooze_minutes.clamp(1, 120);
        for kind in Kind::INTERVAL {
            if let Some(iv) = self.reminders.interval_mut(kind) {
                iv.minutes = iv.minutes.clamp(1, 480);
                iv.seconds = iv.seconds.clamp(5, 600);
            }
        }
        self.appearance = self.appearance.sanitized();
        let mut seen = std::collections::HashSet::new();
        self.custom.retain(|c| {
            is_id(&c.id) && super::scheduler::parse_hm(&c.time).is_some() && seen.insert(c.id.clone())
        });
        self.custom.truncate(MAX_CUSTOM);
        for c in &mut self.custom {
            c.title = clip(&c.title, 60);
            c.note = clip(&c.note, 120);
            c.emoji = clip(&c.emoji, 8);
            if !is_id(&c.anim) {
                c.anim = "remind".into();
            }
            c.days.retain(|d| *d < 7);
            c.days.sort_unstable();
            c.days.dedup();
            c.minutes = c.minutes.min(8 * 60);
        }
        let mut seen = std::collections::HashSet::new();
        self.dates.retain(|d| {
            is_id(&d.id)
                && chrono::NaiveDate::parse_from_str(&d.date, "%Y-%m-%d").is_ok()
                && seen.insert(d.id.clone())
        });
        self.dates.truncate(MAX_DATES);
        for d in &mut self.dates {
            d.title = clip(&d.title, 60);
        }
        self
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Stats {
    /// Local date these counters belong to, "YYYY-MM-DD".
    pub date: String,
    pub water: u32,
    pub stretch: u32,
    pub eyes: u32,
    pub breathe: u32,
    pub sleep: u32,
    /// User-made reminders marked done today.
    pub custom: u32,
    /// Minutes of focus sessions finished today.
    pub focus: u32,
    /// The day's greeting has been shown.
    pub greeted: bool,
    /// User-made reminders already shown today (so each fires once a day).
    pub fired: Vec<String>,
}

impl Stats {
    pub fn load(dir: &Path, today: &str) -> Self {
        let mut stats: Self = fs::read_to_string(dir.join(STATS_FILE))
            .ok()
            .and_then(|text| serde_json::from_str(strip_bom(&text)).ok())
            .unwrap_or_default();
        stats.roll(today);
        stats
    }

    /// Saves today's counters, and files a copy under its date in the history, so the week
    /// can be looked back on after the counters start over.
    pub fn save(&self, dir: &Path) {
        write_json(dir, STATS_FILE, self);
        if !self.date.is_empty() {
            let mut history = History::load(dir);
            history.days.insert(self.date.clone(), Stats { fired: Vec::new(), ..self.clone() });
            while history.days.len() > HISTORY_DAYS {
                let oldest = history.days.keys().next().cloned().unwrap_or_default();
                history.days.remove(&oldest);
            }
            write_json(dir, HISTORY_FILE, &history);
        }
    }

    /// Start fresh counters when the day changes. Returns true if it did.
    pub fn roll(&mut self, today: &str) -> bool {
        if self.date == today {
            return false;
        }
        *self = Self { date: today.into(), ..Self::default() };
        true
    }

    pub fn bump(&mut self, kind: Kind) {
        let n = match kind {
            Kind::Water => &mut self.water,
            Kind::Stretch => &mut self.stretch,
            Kind::Eyes => &mut self.eyes,
            Kind::Breathe => &mut self.breathe,
            Kind::Sleep => &mut self.sleep,
        };
        *n += 1;
    }
}

/// Files edited by hand (Notepad, PowerShell) may start with a UTF-8 BOM.
pub(crate) fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

pub(crate) fn write_json<T: Serialize>(dir: &Path, file: &str, value: &T) {
    let _ = fs::create_dir_all(dir);
    if let Ok(text) = serde_json::to_string_pretty(value) {
        let _ = fs::write(dir.join(file), text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_config_gets_default_appearance() {
        let cfg: Config = serde_json::from_str(r#"{"language":"vi","sound":false}"#).unwrap();
        assert_eq!(cfg.sanitized().appearance, Appearance::default());
    }

    #[test]
    fn appearance_keeps_character_and_mode_and_drops_old_colour_fields() {
        // Configs saved while themes and custom colours existed still load.
        let json = r##"{"appearance":{"mascot":"hutao","theme":"plum","mode":"dark",
            "accent":"#ff0000","kindColors":{"water":"#12a58a"}}}"##;
        let a = serde_json::from_str::<Config>(json).unwrap().sanitized().appearance;
        assert_eq!(a, Appearance { mascot: "hutao".into(), mode: ThemeMode::Dark });
        assert_eq!(serde_json::to_string(&a).unwrap(), r#"{"mascot":"hutao","mode":"dark"}"#);
    }

    #[test]
    fn custom_reminders_are_cleaned_up() {
        let json = r#"{"custom":[
            {"id":"a1","title":"  Đi học  ","time":"13:00","days":[0,4,4,9]},
            {"id":"a1","time":"14:00"},
            {"id":"Bad Id","time":"14:00"},
            {"id":"b2","time":"25:99"},
            {"id":"c3","time":"07:30","days":[],"anim":"???"}
        ]}"#;
        let custom = serde_json::from_str::<Config>(json).unwrap().sanitized().custom;
        assert_eq!(custom.len(), 2);
        assert_eq!(custom[0].title, "Đi học");
        assert_eq!(custom[0].days, vec![0, 4]);
        assert!(custom[1].is_once());
        assert_eq!(custom[1].anim, "remind");
    }

    #[test]
    fn neko_became_lumi() {
        let a = serde_json::from_str::<Config>(r#"{"appearance":{"mascot":"neko"}}"#).unwrap().sanitized().appearance;
        assert_eq!(a.mascot, "lumi");
    }

    #[test]
    fn bad_character_id_falls_back() {
        let json = r#"{"appearance":{"mascot":"../evil"}}"#;
        let a = serde_json::from_str::<Config>(json).unwrap().sanitized().appearance;
        assert_eq!(a.mascot, "luma");
    }
}
