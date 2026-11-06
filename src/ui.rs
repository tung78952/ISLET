//! Layout and drawing. Coordinates are layout units measured from the reference
//! captures; panel-local x is relative to the notch's left edge.

use crate::gfx::{Align, Font, Rc};
use crate::devices::DeviceKind;
use crate::media::LyrStatus;
use crate::model;
use crate::{App, ToastKind};

use crate::stage::Seat;
use crate::Gfx;

const WHITE: u32 = 0xFFFFFF;
const GREEN: u32 = 0x21FF54;
const RED: u32 = 0xFF4F4F;
const CHEVRON: u32 = 0x474747;
const GREY: u32 = 0x8C8C8C;
const BLUE: u32 = 0x4DBDFF;
const CYAN: u32 = 0x66DBFF;
/// Water is water-coloured whoever the character is.
pub const WATER: u32 = 0x35D6E8;
pub const EVENING: u32 = 0xFF8A2A;
/// The colour rising behind the reminder form.
pub const CALM: u32 = 0x3E7BFF;
/// The Reminders page's own colour: what is on, chosen or to be set.
const REMIND: u32 = CALM;
/// On: switches, chosen days.
const ON: u32 = 0x6FE3A5;
const BUTTON: u32 = 0x2A2A2C;
const CORAL: u32 = 0xFF8A65;
const LINE: u32 = 0x2C2C2E;
/// Header text, list text and the line under it.
const HEAD: f32 = 14.0;
const BODY: f32 = 14.0;
const SMALL: f32 = 12.0;
/// A list's two rows and the gap between their middles, right edge of what is in them.
const LIST_ROWS: usize = 2;
const LIST_Y: f32 = 60.0;
const LIST_GAP: f32 = 38.0;
const LIST_RIGHT: f32 = 380.0;
const SWITCH: (f32, f32) = (38.0, 22.0);
/// Fixed places in the header: nothing there can land on anything else.
const SLOT_CLOCK: (f32, f32) = (150.0, 290.0);
const SLOT_CLEAR: (f32, f32) = (306.0, 380.0);
const SLOT_LIVE: (f32, f32) = (318.0, 342.0);
const SLOT_ADD: (f32, f32) = (346.0, 370.0);
const SLOT_BELL: (f32, f32) = (372.0, 396.0);
/// The wash after a drink: it rises for a moment, then fades slowly.
const WASH_RISE: f32 = 0.35;
pub const WASH_SECS: f32 = 1.75;

/// How much of the wash shows `t` seconds after a press, starting from the level it was at.
pub fn wash_curve(t: f32, from: f32) -> f32 {
    let smooth = |k: f32| k * k * (3.0 - 2.0 * k);
    if t < WASH_RISE {
        from + (1.0 - from) * smooth(t / WASH_RISE)
    } else {
        1.0 - smooth(((t - WASH_RISE) / (WASH_SECS - WASH_RISE)).min(1.0))
    }
}

/// Every day, weekdays and weekends, as the form's quick choices.
pub const PRESETS: [[bool; 7]; 3] = [[true; 7], [true, true, true, true, true, false, false], [false, false, false, false, false, true, true]];

/// How far the calendar has zoomed out.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cal {
    Days,
    Months,
    Years,
}

/// Lengths on the focus wheel: 5 to 120 minutes.
pub const FOCUS_STEPS: usize = 24;

pub fn focus_minutes(step: usize) -> u32 {
    (step as u32 + 1) * 5
}

pub fn focus_step(minutes: u32) -> usize {
    ((minutes / 5).max(1) as usize - 1).min(FOCUS_STEPS - 1)
}

/// What the open notch shows besides the pages.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Page,
    History,
    OldNotes,
    Form,
    Care(Tile),
    /// Choosing the cup and the daily goal.
    Goal,
    /// Choosing bedtime.
    Bed,
    Calendar,
    Guide,
    /// The morning hello, or what went by while away.
    Brief,
}

/// The small tiles on Care, each with its own colour when opened.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tile {
    Stretch,
    Eyes,
    Breathe,
    Sleep,
}

impl Tile {
    /// The activity a reminder leads to, for those that have one.
    pub fn of(due: Option<&crate::luma::scheduler::Due>) -> Option<Tile> {
        use crate::luma::{config::Kind as K, scheduler::Due};
        match due? {
            Due::Builtin(K::Stretch) => Some(Tile::Stretch),
            Due::Builtin(K::Eyes) => Some(Tile::Eyes),
            Due::Builtin(K::Breathe) => Some(Tile::Breathe),
            _ => None,
        }
    }

    const ALL: [Tile; 4] = [Tile::Stretch, Tile::Eyes, Tile::Breathe, Tile::Sleep];

    pub fn kind(self) -> crate::luma::config::Kind {
        use crate::luma::config::Kind as K;
        match self {
            Tile::Stretch => K::Stretch,
            Tile::Eyes => K::Eyes,
            Tile::Breathe => K::Breathe,
            Tile::Sleep => K::Sleep,
        }
    }

    pub fn colour(self) -> u32 {
        match self {
            Tile::Stretch => 0xFF9F43,
            Tile::Eyes => 0x5FD07A,
            Tile::Breathe => 0xA78BFA,
            Tile::Sleep => 0x6C7BFF,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Tile::Stretch => "Stretch",
            Tile::Eyes => "Eye rest",
            Tile::Breathe => "Breathe",
            Tile::Sleep => "Sleep",
        }
    }

    fn about(self) -> &'static str {
        match self {
            Tile::Stretch => "Stand up, stretch and relax.",
            Tile::Eyes => "Look far away for a moment.",
            Tile::Breathe => "One slow minute of breathing.",
            Tile::Sleep => "A nudge when it is time for bed.",
        }
    }

    fn key(self) -> SetKey {
        match self {
            Tile::Stretch => SetKey::Stretch,
            Tile::Eyes => SetKey::RestEyes,
            Tile::Breathe => SetKey::Breathe,
            Tile::Sleep => SetKey::Sleep,
        }
    }

    pub fn picture(self) -> &'static str {
        match self {
            Tile::Stretch => "ic_stretch",
            Tile::Eyes => "ic_eyes",
            Tile::Breathe => "ic_breathe",
            Tile::Sleep => "ic_sleep",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Today,
    Music,
    Pomo,
    Water,
    Shelf,
    Todo,
}

impl Page {
    pub const ALL: [Page; 6] = [Page::Today, Page::Music, Page::Pomo, Page::Water, Page::Todo, Page::Shelf];
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|p| *p == self).unwrap_or(0)
    }
    pub fn next(self) -> Page {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }
    pub fn prev(self) -> Page {
        Self::ALL[(self.index() + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Act {
    None,
    PagePrev,
    /// Seek the player to this many milliseconds.
    Seek(u32),
    /// End-of-phase toast: start the prepared phase, or dismiss.
    ToastStartPhase,
    ToastDismiss,
    /// Luma's reminder card: done (drank, started, going to bed), or later (snooze).
    LumaDone,
    LumaLater,
    /// The heads-up: show the card now.
    LumaNow,
    /// Turn the separate Luma app off (Islet takes over its reminders).
    LumaAppOff,
    /// Timeline bar at (x, width) in tenths of a layout unit; dragging it scrubs.
    Scrub(i32, i32),
    PageNext,
    Page(Page),
    Prev,
    Toggle,
    Next,
    Lyrics,
    PomoToggle,
    PomoReset,
    PomoMinutes(u32),
    View(View),
    Month(i32),
    Day(i32, u32, u32),
    CalMode(Cal),
    PickMonth(u32),
    PickYear(i32),
    SaveWheels,
    RowTrash(u8, usize),
    RowDelete,
    BriefClose,
    GuideStart,
    GuidePause,
    GuideBack,
    ShutdownCancel,
    FocusMode(bool),
    /// The reminder form's repeat: Every day (0), Mon–Fri (1) or Once (2).
    FormRepeat(u8),
    /// Reminders: the delete question was answered "Cancel".
    RowCancel,
    /// The Focus page's wheel of lengths: slide it in (true) or put it away.
    FocusPick(bool),
    PomoPlan(u32, u32),
    DoNow(Tile),
    Drank,
    /// A rolling picker: it is dragged, turned and tapped, not clicked.
    Wheel(WheelId),
    Set(SetKey),
    ReminderAdd,
    ReminderEdit(usize),
    ReminderSwitch(usize),
    OldNotes(bool),
    WaterReset,
    FormNext,
    FormBack,
    FormClear,
    ShelfOpen(usize),
    ShelfRemove(usize),
    ShelfClear,
    /// Notification detail view: open what it is about, or dismiss it.
    NoticeOpen,
    NoticeClose,
    /// Device card: open the drive that was plugged in.
    DeviceOpen,
    /// Notification history: show it or go back, open a row, clear it.
    History(bool),
    HistoryOpen(usize),
    HistoryClear,
}

fn secs_label(s: f64) -> String {
    let s = s.max(0.0) as u64;
    format!("{:02}:{:02}", s / 60, s % 60)
}

fn hovering(a: &App, r: Rc) -> bool {
    r.contains(a.mouse.0, a.mouse.1)
}

/// Hover/press feedback for a button: eases toward 1 on hover (-0.6 while pressed) and zooms the
/// drawing that follows about the button's center, so every target visibly grows like in Nusa.
/// Returns the eased amount; call `a.gfx.unzoom()` once the button is drawn.
fn btn(a: &mut App, r: Rc, grow: f32) -> f32 {
    let key = ((r.x * 4.0).round() as i32, (r.y * 4.0).round() as i32);
    let hot = hovering(a, r);
    let pressed = hot && a.down.is_some_and(|(x, y, _)| r.contains(x, y));
    // Liquid glass: pressed, it swells instead of sinking, and springs back with a little bounce.
    let glass = a.settings.glass;
    let target = if pressed { if glass { 1.8 } else { -0.6 } } else if hot { 1.0 } else { 0.0 };
    let e = a.hover.entry(key).or_insert((crate::Spring::new(0.0, 320.0, 24.0), 0.0));
    if glass {
        e.0.tune(360.0, 15.0);
    } else {
        e.0.tune(320.0, 24.0);
    }
    let changed = e.1 != target;
    e.1 = target;
    let h = e.0.x;
    if changed {
        a.kick();
    }
    if glass && pressed {
        // Held and dragged: it follows the pointer a little (tanh, so never far) and stretches
        // toward it (after LiquidButton).
        let (dx, dy) = a.down.map_or((0.0, 0.0), |(x, y, _)| (a.mouse.0 - x, a.mouse.1 - y));
        let (sx, sy, ox, oy) = liquid_drag(r, dx, dy, 1.0 + grow * h);
        a.gfx.zoom_xy(r.cx(), r.cy(), sx, sy, ox, oy);
    } else {
        a.gfx.zoom(r.cx(), r.cy(), 1.0 + grow * h);
    }
    h
}

/// The toggle knob's squash at speed `v` (its spring's, units per second): wider and flatter
/// the faster it goes, never more than a fifth (after LiquidToggle).
pub fn toggle_squash(v: f32) -> (f32, f32) {
    let v = v / 8.0;
    (1.0 / (1.0 - (v.abs() * 0.75).min(0.2)), 1.0 - (v.abs() * 0.25).min(0.2))
}

/// How a held liquid button follows a drag of (dx, dy): its scale (x, y) and offset, starting
/// from the press scale `base`.
pub fn liquid_drag(r: Rc, dx: f32, dy: f32, base: f32) -> (f32, f32, f32, f32) {
    let reach = r.h * 0.5;
    let follow = |d: f32| reach * (0.05 * d / reach).tanh();
    let longest = r.w.max(r.h);
    let stretch = 4.0 / r.h;
    let sx = base + stretch * (dx / longest).abs().min(1.0) * (r.w / r.h).min(1.0);
    let sy = base + stretch * (dy / longest).abs().min(1.0) * (r.h / r.w).min(1.0);
    (sx, sy, follow(dx), follow(dy))
}

/// Blends two 0xRRGGBB colors.
fn mix(c0: u32, c1: u32, t: f32) -> u32 {
    let t = t.clamp(0.0, 1.0);
    let ch = |sh: u32| {
        let (x, y) = (((c0 >> sh) & 255) as f32, ((c1 >> sh) & 255) as f32);
        ((x + (y - x) * t).round() as u32) << sh
    };
    ch(16) | ch(8) | ch(0)
}

fn hit(a: &mut App, r: Rc, act: Act) {
    a.hits.push((r, act));
}

// ---------------------------------------------------------------- icons

mod icon {
    use crate::gfx::Gfx;

    pub fn chevron(g: &Gfx, cx: f32, cy: f32, left: bool, hex: u32, a: f32) {
        let d = if left { 1.0 } else { -1.0 };
        g.polyline(&[(cx + d * 1.6, cy - 3.6), (cx - d * 2.0, cy), (cx + d * 1.6, cy + 3.6)], 1.9, hex, a);
    }
    pub fn charging(g: &Gfx, cx: f32, cy: f32, r: f32, level: f32, a: f32) {
        // The charge as a ring in the battery's colour, with a white bolt.
        g.stroke_circle(cx, cy, r - 0.6, 3.0, 0x3A3A3C, a);
        g.arc(cx, cy, r - 0.6, level, 3.0, super::battery_tone(level).0, a);
        g.bolt(cx, cy, r * 0.56, 0xFFFFFF, a);
    }
    pub fn play(g: &Gfx, cx: f32, cy: f32, s: f32, hex: u32, a: f32) {
        g.polygon(&[(cx - s * 0.38, cy - s * 0.5), (cx + s * 0.5, cy), (cx - s * 0.38, cy + s * 0.5)], hex, a);
    }
    pub fn pause(g: &Gfx, cx: f32, cy: f32, s: f32, hex: u32, a: f32) {
        let w = s * 0.26;
        g.fill_rrect(crate::gfx::Rc::new(cx - s * 0.36, cy - s * 0.5, w, s), 1.2, hex, a);
        g.fill_rrect(crate::gfx::Rc::new(cx + s * 0.36 - w, cy - s * 0.5, w, s), 1.2, hex, a);
    }
    /// Double triangle (fast-forward when `fwd`, rewind otherwise).
    pub fn skip(g: &Gfx, cx: f32, cy: f32, s: f32, fwd: bool, hex: u32, a: f32) {
        let d = if fwd { 1.0 } else { -1.0 };
        let h = s * 0.37;
        for off in [-s * 0.5, 0.0] {
            let x0 = cx + d * off;
            g.polygon(&[(x0, cy - h), (x0 + d * s * 0.5, cy), (x0, cy + h)], hex, a);
        }
    }
    pub fn quote(g: &Gfx, cx: f32, cy: f32, hex: u32, a: f32) {
        g.stroke_rrect(crate::gfx::Rc::new(cx - 8.4, cy - 7.4, 16.8, 12.6), 4.0, 1.5, hex, a);
        g.polyline(&[(cx - 4.4, cy + 5.2), (cx - 6.0, cy + 9.0), (cx - 0.6, cy + 5.2)], 1.5, hex, a);
        for dx in [-2.6f32, 2.4] {
            g.fill_circle(cx + dx, cy - 1.8, 1.6, 1.6, hex, a);
            g.polygon(&[(cx + dx + 0.6, cy - 1.4), (cx + dx + 0.6, cy + 1.8), (cx + dx - 1.2, cy - 0.8)], hex, a);
        }
    }
    pub fn stopwatch(g: &Gfx, cx: f32, cy: f32, r: f32, hex: u32, a: f32) {
        // Proportions measured at r = 16.2: stroke 4, crown bar 15.2 wide, side stub at 45°.
        let k = r / 16.2;
        let w = 4.0 * k;
        g.stroke_circle(cx, cy, r, w, hex, a);
        g.fill_rrect(crate::gfx::Rc::new(cx - 7.6 * k, cy - 23.7 * k, 15.2 * k, 4.0 * k), 1.6 * k, hex, a);
        g.line(cx, cy - 20.5 * k, cx, cy - r, w, hex, a);
        g.line(cx + 13.4 * k, cy - 13.9 * k, cx + 16.0 * k, cy - 16.5 * k, w, hex, a);
        g.line(cx, cy - 0.9 * k, cx, cy - 8.1 * k, w, hex, a);
    }
    pub fn reset(g: &Gfx, cx: f32, cy: f32, r: f32, hex: u32, a: f32) {
        use std::f32::consts::PI;
        // A ring open at the top; the arrowhead sits in the gap, so the whole is on its centre.
        let (from, to) = (-0.32 * PI, 1.32 * PI);
        g.arc_between(cx, cy, r, from, to, 1.7, hex, a);
        let (ex, ey) = (cx + r * to.cos(), cy + r * to.sin());
        g.polyline(&[(ex - 1.0, ey - 3.0), (ex + 3.0, ey), (ex - 0.6, ey + 3.2)], 1.7, hex, a);
    }
    pub fn droplet(g: &Gfx, cx: f32, cy: f32, s: f32, hex: u32, a: f32) {
        g.droplet(cx, cy, s, hex, a);
    }
    pub fn bell(g: &Gfx, cx: f32, cy: f32, hex: u32, a: f32) {
        // Dome with a flared rim and a clapper below.
        const S: f32 = 1.3;
        let half = [(4.4f32, 2.6f32), (3.8, 1.8), (3.4, 0.0), (3.2, -2.4), (2.6, -3.8), (1.6, -4.6), (0.0, -4.9)];
        let mut pts: Vec<(f32, f32)> = half.iter().map(|&(x, y)| (cx + x * S, cy + y * S)).collect();
        pts.extend(half.iter().rev().skip(1).map(|&(x, y)| (cx - x * S, cy + y * S)));
        g.polygon(&pts, hex, a);
        g.fill_circle(cx, cy + 4.6 * S, 1.6, 1.6, hex, a);
    }
    pub fn trash(g: &Gfx, cx: f32, cy: f32, hex: u32, a: f32) {
        g.line(cx - 5.0, cy - 3.8, cx + 5.0, cy - 3.8, 1.4, hex, a);
        g.stroke_rrect(crate::gfx::Rc::new(cx - 3.6, cy - 2.6, 7.2, 8.2), 1.4, 1.3, hex, a);
        g.line(cx - 1.6, cy - 5.6, cx + 1.6, cy - 5.6, 1.4, hex, a);
    }
    pub fn open(g: &Gfx, cx: f32, cy: f32, hex: u32, a: f32) {
        g.polyline(&[(cx + 1.0, cy - 6.0), (cx - 6.0, cy - 6.0), (cx - 6.0, cy + 6.0), (cx + 6.0, cy + 6.0), (cx + 6.0, cy - 1.0)], 1.5, hex, a);
        g.polyline(&[(cx - 1.0, cy + 1.0), (cx + 6.5, cy - 6.5)], 1.5, hex, a);
        g.polyline(&[(cx + 2.5, cy - 6.5), (cx + 6.5, cy - 6.5), (cx + 6.5, cy - 2.5)], 1.5, hex, a);
    }
    pub fn cross(g: &Gfx, cx: f32, cy: f32, d: f32, hex: u32, a: f32) {
        g.line(cx - d, cy - d, cx + d, cy + d, 2.0, hex, a);
        g.line(cx - d, cy + d, cx + d, cy - d, 2.0, hex, a);
    }
    pub fn tick(g: &Gfx, cx: f32, cy: f32, hex: u32, a: f32) {
        g.line(cx - 6.0, cy + 0.4, cx - 1.8, cy + 4.6, 2.6, hex, a);
        g.line(cx - 1.8, cy + 4.6, cx + 6.4, cy - 4.2, 2.6, hex, a);
    }
    pub fn plus(g: &Gfx, cx: f32, cy: f32, s: f32, width: f32, hex: u32, a: f32) {
        g.line(cx - s, cy, cx + s, cy, width, hex, a);
        g.line(cx, cy - s, cx, cy + s, width, hex, a);
    }
    /// Inbox: a down arrow over a dim back panel and a bright notched front tray.
    pub fn tray(g: &Gfx, cx: f32, cy: f32, colors: [u32; 3], a: f32) {
        let [arrow, back, front] = colors;
        let p = |x: f32, y: f32| (cx + x, cy + y);
        g.fill_rrect(crate::gfx::Rc::new(cx - 1.6, cy - 11.6, 3.2, 6.4), 0.6, arrow, a);
        g.polygon(&[p(-5.2, -5.6), p(5.2, -5.6), p(0.0, 0.4)], arrow, a);
        g.polygon(&[p(-10.4, 3.6), p(10.4, 3.6), p(13.2, 11.2), p(-13.2, 11.2)], back, a);
        g.polygon(&[p(-14.0, 12.4), p(-5.2, 12.4), p(-2.8, 15.6), p(2.8, 15.6), p(5.2, 12.4), p(14.0, 12.4), p(14.0, 20.4), p(-14.0, 20.4)], front, a);
    }
    pub fn x_circle(g: &Gfx, cx: f32, cy: f32, r: f32, bg: u32, fg: u32, a: f32) {
        g.fill_circle(cx, cy, r, r, bg, a);
        let s = r * 0.42;
        g.line(cx - s, cy - s, cx + s, cy + s, 1.5, fg, a);
        g.line(cx - s, cy + s, cx + s, cy - s, 1.5, fg, a);
    }
    pub fn note(g: &Gfx, cx: f32, cy: f32, hex: u32, a: f32) {
        g.fill_circle(cx - 3.6, cy + 5.0, 2.8, 2.2, hex, a);
        g.fill_circle(cx + 4.4, cy + 3.4, 2.8, 2.2, hex, a);
        g.line(cx - 1.0, cy + 5.0, cx - 1.0, cy - 6.0, 1.4, hex, a);
        g.line(cx + 7.0, cy + 3.4, cx + 7.0, cy - 7.6, 1.4, hex, a);
        g.line(cx - 1.0, cy - 6.0, cx + 7.0, cy - 7.6, 2.4, hex, a);
    }
    /// Speaker whose waves follow the (eased) level as in Nusa: one wave per third of the range,
    /// each fading and growing in as the level passes it; muted or zero shows a cross instead.
    pub fn speaker(g: &Gfx, cx: f32, cy: f32, level: f32, muted: bool, hex: u32, a: f32) {
        // (cx, cy) is the mouth of the cone; waves are centered on it.
        g.polygon(&[(cx - 10.0, cy - 4.4), (cx - 4.8, cy - 4.4), (cx, cy - 8.0), (cx, cy + 8.0), (cx - 4.8, cy + 4.4), (cx - 10.0, cy + 4.4)], hex, a);
        let l = level.clamp(0.0, 1.0);
        let cross = if muted { 1.0 } else { (1.0 - l * 25.0).clamp(0.0, 1.0) };
        if cross > 0.0 {
            g.line(cx + 3.8, cy - 3.6, cx + 10.2, cy + 3.6, 2.0, hex, a * cross);
            g.line(cx + 3.8, cy + 3.6, cx + 10.2, cy - 3.6, 2.0, hex, a * cross);
        }
        for (k, r) in [4.2f32, 7.6, 10.8].iter().enumerate() {
            let w = (l * 3.0 - k as f32 + 0.3).clamp(0.0, 1.0) * (1.0 - cross);
            if w > 0.0 {
                g.arc_between(cx, cy, r * (0.85 + 0.15 * w), -0.8, 0.8, 1.8, hex, a * w);
            }
        }
    }
    /// Pomodoro bubble: a dark glass with red liquid whose level is the time left and whose
    /// surface slowly tilts and ripples (Nusa's compact timer).
    /// Messenger-style chat icon (drawn, not the trademark artwork): a blue-to-violet bubble with
    /// a white zig-zag bolt.
    pub fn messenger(g: &Gfx, cx: f32, cy: f32, r: f32, a: f32) {
        let rc = crate::gfx::Rc::new(cx - r, cy - r, 2.0 * r, 2.0 * r);
        g.fill_rrect_hgrad(rc, r, (0x0A84FF, a), (0xA33BFF, a));
        g.polygon(&[(cx - r * 0.42, cy + r * 0.52), (cx - r * 0.52, cy + r * 0.95), (cx - r * 0.05, cy + r * 0.72)], 0x6A5CFF, a);
        let s = r * 0.5;
        let p = |x: f32, y: f32| (cx + x * s, cy + y * s);
        g.polyline(&[p(-1.0, 0.5), p(-0.25, -0.35), p(0.2, 0.15), p(1.0, -0.5)], r * 0.17, 0xFFFFFF, a);
    }
    pub fn bolt_small(g: &Gfx, cx: f32, cy: f32, hex: u32, a: f32) {
        g.polygon(&[(cx + 1.2, cy - 6.0), (cx - 3.6, cy + 0.8), (cx - 0.4, cy + 0.8), (cx - 1.2, cy + 6.0), (cx + 3.6, cy - 0.8), (cx + 0.4, cy - 0.8)], hex, a);
    }
    /// Eight rays turned by `rot` radians (clockwise) around a still disc. With the (eased) level
    /// the rays lengthen, move out and brighten, as in Nusa (dim and short at 0 %).
    pub fn sun(g: &Gfx, cx: f32, cy: f32, rot: f32, level: f32, hex: u32, a: f32) {
        let l = level.clamp(0.0, 1.0);
        g.fill_circle(cx, cy, 3.2, 3.2, hex, a);
        let (r0, r1) = (5.2 + 1.6 * l, 7.4 + 2.6 * l);
        for k in 0..8 {
            let t = rot + k as f32 * std::f32::consts::FRAC_PI_4;
            let (c, s) = (t.cos(), t.sin());
            g.line(cx + c * r0, cy + s * r0, cx + c * r1, cy + s * r1, 1.6, hex, a * (0.35 + 0.65 * l));
        }
    }
    /// Wi-Fi glyph whose bounding box is centred on (cx, cy).
    pub fn wifi_centered(g: &Gfx, cx: f32, cy: f32, arcs: f32, hex: u32, a: f32) {
        // Scaled to fit a badge: dot centre at +4.6, outer arc top at -7.4 (bounding box ±6.0).
        let base = cy + 4.0;
        g.fill_circle(cx, base, 1.5, 1.5, hex, a);
        for (i, r) in [3.6f32, 6.8, 10.0].iter().enumerate() {
            let k = (arcs - i as f32).clamp(0.0, 1.0);
            if k > 0.0 {
                g.arc_between(cx, base + 1.4, *r, -2.36, -0.78, 1.8, hex, a * k);
            }
        }
    }
    pub fn bluetooth(g: &Gfx, cx: f32, cy: f32, hex: u32, a: f32) {
        // The rune: a vertical stem with two closed loops on the right and two crossing arms.
        let (h, d) = (6.4, 3.4);
        g.polyline(&[(cx - d, cy - h * 0.5), (cx + d, cy + h * 0.5), (cx, cy + h), (cx, cy - h), (cx + d, cy - h * 0.5), (cx - d, cy + h * 0.5)], 1.6, hex, a);
    }
    /// The icon for a kind of device, about 16 units across, centred on (cx, cy).
    pub fn device(g: &Gfx, kind: crate::devices::DeviceKind, cx: f32, cy: f32, hex: u32, a: f32) {
        use crate::devices::DeviceKind as K;
        use crate::gfx::Rc;
        match kind {
            K::Headphones | K::Other => headphones(g, cx, cy, hex, a),
            K::Headset => {
                headphones(g, cx, cy - 1.0, hex, a);
                // Microphone boom from the left cup.
                g.polyline(&[(cx - 7.2, cy + 6.0), (cx - 6.6, cy + 8.4), (cx - 2.4, cy + 8.6)], 1.5, hex, a);
                g.fill_circle(cx - 1.8, cy + 8.6, 1.4, 1.4, hex, a);
            }
            K::Speaker => {
                g.stroke_rrect(Rc::new(cx - 5.2, cy - 7.6, 10.4, 15.2), 2.6, 1.6, hex, a);
                g.stroke_circle(cx, cy + 2.2, 2.9, 1.5, hex, a);
                g.fill_circle(cx, cy - 4.0, 1.1, 1.1, hex, a);
            }
            K::Mouse => {
                g.stroke_rrect(Rc::new(cx - 4.6, cy - 7.6, 9.2, 15.2), 4.6, 1.6, hex, a);
                g.line(cx, cy - 6.6, cx, cy - 2.6, 1.6, hex, a);
            }
            K::Keyboard => {
                g.stroke_rrect(Rc::new(cx - 8.0, cy - 5.0, 16.0, 10.0), 2.2, 1.5, hex, a);
                for (row, y) in [(0, cy - 2.0), (1, cy + 0.6)] {
                    for i in 0..4 {
                        let x = cx - 4.8 + i as f32 * 3.2 + if row == 1 { 0.8 } else { 0.0 };
                        g.fill_rrect(Rc::new(x - 0.8, y - 0.7, 1.6, 1.4), 0.4, hex, a);
                    }
                }
                g.line(cx - 3.0, cy + 3.0, cx + 3.0, cy + 3.0, 1.4, hex, a);
            }
            K::Gamepad => {
                g.stroke_rrect(Rc::new(cx - 8.4, cy - 4.8, 16.8, 9.6), 4.8, 1.6, hex, a);
                g.line(cx - 6.0, cy, cx - 2.4, cy, 1.5, hex, a);
                g.line(cx - 4.2, cy - 1.8, cx - 4.2, cy + 1.8, 1.5, hex, a);
                g.fill_circle(cx + 3.4, cy + 1.0, 1.1, 1.1, hex, a);
                g.fill_circle(cx + 5.4, cy - 1.0, 1.1, 1.1, hex, a);
            }
            K::Phone => {
                g.stroke_rrect(Rc::new(cx - 4.6, cy - 8.0, 9.2, 16.0), 2.2, 1.6, hex, a);
                g.line(cx - 1.6, cy + 5.4, cx + 1.6, cy + 5.4, 1.4, hex, a);
            }
            K::Drive => {
                g.stroke_rrect(Rc::new(cx - 7.6, cy - 4.6, 15.2, 9.2), 2.2, 1.6, hex, a);
                g.fill_circle(cx + 4.4, cy + 1.2, 1.1, 1.1, hex, a);
                g.line(cx - 4.6, cy + 1.2, cx + 1.0, cy + 1.2, 1.4, hex, a);
            }
        }
    }
    /// A phone handset, tilted down when hanging up.
    /// The USB trident.
    pub fn usb(g: &Gfx, cx: f32, cy: f32, hex: u32, a: f32) {
        g.line(cx, cy + 6.6, cx, cy - 5.0, 1.6, hex, a);
        g.polygon(&[(cx, cy - 8.0), (cx - 2.2, cy - 4.6), (cx + 2.2, cy - 4.6)], hex, a);
        g.polyline(&[(cx, cy + 3.0), (cx - 4.2, cy + 0.2), (cx - 4.2, cy - 1.8)], 1.5, hex, a);
        g.fill_circle(cx - 4.2, cy - 2.6, 1.4, 1.4, hex, a);
        g.polyline(&[(cx, cy + 1.2), (cx + 4.2, cy - 1.6), (cx + 4.2, cy - 3.0)], 1.5, hex, a);
        g.fill_rrect(crate::gfx::Rc::new(cx + 3.0, cy - 5.2, 2.4, 2.4), 0.3, hex, a);
        g.fill_circle(cx, cy + 6.8, 1.6, 1.6, hex, a);
    }
    pub fn headphones(g: &Gfx, cx: f32, cy: f32, hex: u32, a: f32) {
        g.arc_between(cx, cy + 1.0, 7.2, std::f32::consts::PI, std::f32::consts::TAU, 2.0, hex, a);
        for dx in [-7.2f32, 7.2] {
            g.fill_rrect(crate::gfx::Rc::new(cx + dx - 2.0, cy + 0.4, 4.0, 7.2), 1.6, hex, a);
        }
    }
    /// The arrow alone; `bar` is the underline's opacity (0 to draw it separately).
    pub fn caps(g: &Gfx, cx: f32, cy: f32, filled: bool, hex: u32, a: f32, bar: f32) {
        let pts = [(cx, cy - 7.6), (cx + 7.2, cy), (cx + 3.4, cy), (cx + 3.4, cy + 4.2), (cx - 3.4, cy + 4.2), (cx - 3.4, cy), (cx - 7.2, cy)];
        if filled {
            g.polygon(&pts, hex, a);
        } else {
            let mut p = pts.to_vec();
            p.push(pts[0]);
            g.polyline(&p, 1.6, hex, a);
        }
        if bar > 0.0 {
            caps_bar(g, cx, cy, hex, bar);
        }
    }
    pub fn caps_bar(g: &Gfx, cx: f32, cy: f32, hex: u32, a: f32) {
        g.line(cx - 3.4, cy + 7.2, cx + 3.4, cy + 7.2, 1.6, hex, a);
    }
}

// ---------------------------------------------------------------- frame

pub fn draw(a: &mut App) {
    a.hits.clear();
    a.loop_rect = None;
    a.live_alpha = 0.0;
    a.eye_draws = (0, 0);
    let (visual_cx, visual_w) = a.notch_geometry();
    let (w, h) = (visual_w.max(1.0), a.sh.x.max(1.0));
    let cx = visual_cx;
    let t = ((h - 30.0) / 110.0).clamp(0.0, 1.0);
    // Corner and ear radii fitted to the reference silhouette (compact 30 tall → expanded 140 tall).
    let radius = 12.4 + 19.2 * t;
    let ear = 10.0 + 12.2 * t;
    if a.minimal() && !a.yielded && a.mode == crate::Mode::Compact {
        // A near-invisible pad keeps the tiny minimal notch easy to hover.
        a.gfx.fill_rrect(Rc::new(cx - 70.0, 0.0, 140.0, 18.0), 0.0, 0x000000, 0.005);
    }
    if a.catch_all {
        a.gfx.fill_rrect(Rc::new(0.0, 0.0, crate::WIN_W, crate::WIN_H), 0.0, 0x000000, 1.5 / 255.0);
    }
    let na = 1.0;
    a.gfx.alpha = na;
    let (bw, bh) = (w, h);
    // Liquid glass: closed, the outline leans toward a pointer nearby; what is on it stays.
    let (lean, lean_w) = (a.lean.x, a.lean_w.x.max(0.0));
    // Liquid glass, open: the screen behind shows through the notch, blurred, brighter in colour
    // and bent at the rim; a thin black still lies over it so the words stay readable.
    let glass_k = a.glass_mix.x.clamp(0.0, 1.0);
    a.glass_frame = None;
    if glass_k > 0.01 {
        let target = a.target();
        if let Some((bmp, map, at, k, squeeze)) = a.backdrop_frame(cx, bw, bh, target) {
            let clipped = a.gfx.push_notch_clip(cx, bw, bh, radius, ear);
            let base = a.gfx.alpha;
            a.gfx.alpha = base * glass_k;
            a.gfx.backdrop_glass(&bmp, &map, at, k, squeeze, 10.0 * a.gfx.scale, 10.0 * a.gfx.scale);
            if glass_k > 0.6 {
                a.glass_frame = Some((bmp.clone(), at, k));
            }
            a.gfx.alpha = base;
            if clipped {
                a.gfx.pop_layer();
            }
        }
    }
    a.gfx.notch(cx + lean, bw + lean_w, bh, radius, ear, 0x000000, 1.0 - 0.7 * glass_k);
    if glass_k > 0.01 {
        // Light caught at the rim, thin: bright toward two opposite corners, faint between.
        a.gfx.stroke_notch_lit(cx, bw - 0.8, bh - 0.4, radius, ear, 0.8, 0.32 * glass_k, 0.02 * glass_k);
    }
    let left = cx - w / 2.0;

    // The drop light sits behind everything on the panel, header included.
    if (a.page == Page::Shelf && a.drag_over) || a.ripple.is_some() {
        // Light effects live inside the notch's own outline, never past its rim.
        let clipped = a.gfx.push_notch_clip(cx, w, h, radius, ear);
        a.gfx.alpha = 1.0;
        let (fx, fy, st) = aurora_state(a);
        aurora(a, fx, fy, st.min(1.0));
        aurora_highlight(a, fx, st);
        drop_wave(a);
        a.gfx.alpha = 1.0;
        if clipped {
            a.gfx.pop_layer();
        }
    }
    let seat = a.eye_seat();
    let lm = a.lyr_mix.x.clamp(0.0, 1.0);
    if a.page == Page::Music && lm > 0.01 && a.fade.x > 0.01 {
        lyrics_backdrop(a, cx - bw / 2.0, bw, bh, radius, lm);
    }
    if a.glow_mix.x > 0.01 || a.wash > 0.01 || a.focus_mix.x > 0.01 {
        let clipped = a.gfx.push_notch_clip(cx, bw, bh, radius, ear);
        ambient(a, cx - bw / 2.0, bw, bh);
        if clipped {
            a.gfx.pop_layer();
        }
    }

    let exp_alpha = a.fade.x.clamp(0.0, 1.0) * ((h - 70.0) / 50.0).clamp(0.0, 1.0);
    let tall = a.toast.as_ref().or(a.last_toast.as_ref()).is_some_and(|t| t.kind.tall()) && a.toast_mix.x > 0.5;
    let fade_from = if tall { 84.0 } else { 36.0 };
    let mut small_alpha = (1.0 - (h - fade_from) / 14.0).clamp(0.0, 1.0) * (1.0 - a.fade.x).clamp(0.0, 1.0);
    // A notification grows into its detail view: the card leaves early and the detail comes in
    // while it is still going, so the notch is never empty on the way.
    let notice = a.notice_toast() && a.fade.x < 0.01 && !a.hidden;
    let grown = if notice {
        let card_h = a.notice_card_height();
        ((h - card_h) / (crate::NOTICE_OPEN[1] - card_h)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    if notice {
        small_alpha = (1.0 - grown / 0.35).clamp(0.0, 1.0);
    }
    if a.hidden {
        small_alpha = 0.0;
    }

    if exp_alpha > 0.01 {
        a.gfx.alpha = na * exp_alpha;
        a.gfx.push_clip(Rc::new(left, 0.0, w, h));
        expanded(a, left);
        a.gfx.pop_clip();
    } else if small_alpha > 0.01 {
        // Compact content, lyric pill and toast crossfade into each other while the notch resizes.
        let tm = a.toast_mix.x.clamp(0.0, 1.0);
        let pm = a.pill_mix.x.clamp(0.0, 1.0) * (1.0 - tm);
        let base = (1.0 - tm - pm).max(0.0);
        if base > 0.01 && h > 18.0 && !a.minimal() && !a.yielded {
            a.gfx.alpha = na * small_alpha * base;
            compact(a, left, w);
        } else if seat == Seat::Aside && a.eyes.show.x > 0.3 {
            // The eyes shut as the notch leaves, inside its shrinking outline.
            let clipped = a.gfx.push_notch_clip(cx, w, h, radius, ear);
            a.gfx.alpha = 1.0;
            eyes(a, left + w / 2.0, 14.8, (a.eyes.show.x.clamp(0.0, 1.0) - 0.3) / 0.7, 14.0);
            if clipped {
                a.gfx.pop_layer();
            }
        }
        // While the notch steps aside only its sliver remains: nothing may be drawn over the app
        // below (reminders that need an answer keep their size and stay).
        let urgent = a.interactive_toast();
        if pm > 0.01 && a.lyric_line().is_some() && !a.yielded {
            // The eyes take over from the lyrics for a moment, in a notch that keeps its size.
            let show = a.eyes.show.x.clamp(0.0, 1.0);
            a.gfx.alpha = na * small_alpha * pm * (1.0 - (show / 0.3).min(1.0));
            lyric_pill(a, left, w);
            a.gfx.alpha = na * small_alpha * pm;
            if show > 0.3 && seat == Seat::Lyrics {
                eyes(a, left + w / 2.0, h / 2.0, (show - 0.3) / 0.7, (w / 2.0 - 48.0).max(14.0));
            }
            a.gfx.alpha = 1.0;
        }
        if tm > 0.01 && (!a.yielded || urgent) {
            let sw = a.toast_swap.x.clamp(0.0, 1.0);
            let base = na * small_alpha * tm;
            match (a.toast.clone(), a.last_toast.clone()) {
                (Some(cur), Some(old)) if sw < 0.999 => {
                    a.gfx.alpha = base * (1.0 - sw);
                    toast(a, left, w, &old, false);
                    a.gfx.alpha = base * sw;
                    toast(a, left, w, &cur, true);
                }
                (Some(cur), _) => {
                    a.gfx.alpha = base;
                    toast(a, left, w, &cur, true);
                }
                (None, Some(old)) => {
                    a.gfx.alpha = base;
                    toast(a, left, w, &old, false);
                }
                _ => {}
            }
        }
    }
    focus_sun(a, left, w, h);
    if notice {
        let d = ((grown - 0.25) / 0.45).clamp(0.0, 1.0);
        if d > 0.01 {
            // The colour of the app that wrote rises behind it.
            if let Some(tint) = a.notice.clone().map(|n| a.notice_accent(&n)) {
                let clipped = a.gfx.push_notch_clip(cx, w, h, radius, ear);
                a.gfx.fill_rrect_vgrad(Rc::new(left, h * 0.5, w, h * 0.5), 0.0, (tint, 0.0), (tint, 0.42 * d));
                if clipped {
                    a.gfx.pop_layer();
                }
            }
            a.gfx.alpha = na * d;
            a.gfx.push_clip(Rc::new(left, 0.0, w, h));
            notice_detail(a, left, w, h, d > 0.6);
            a.gfx.pop_clip();
        }
    }
    a.gfx.alpha = 1.0;
}

// ---------------------------------------------------------------- compact

fn compact(a: &mut App, left: f32, w: f32) {
    let cy = 14.8;
    let charging = a.power.is_some_and(|p| p.battery && p.ac);
    let bar = a.eye_seat() == Seat::Bar;
    let show = if bar { a.eyes.show.x.clamp(0.0, 1.0) } else { 0.0 };
    // The eyes have the notch to themselves: everything else steps back while they are open.
    let rest = 1.0 - (show / 0.3).min(1.0);
    if charging {
        icon::charging(&a.gfx, left + 20.0, cy, 8.2, a.charge_level(), rest);
    }
    // A running focus shows what is left of it instead of the time of day.
    let time = match &a.brain.focus {
        Some(_) => clock_text(a.pomo_view().0),
        None => a.clock().time_label(),
    };
    a.gfx.ui(&time, Rc::new(left, 5.0, w, 20.0), 16.0, 700, WHITE, rest, Align::C);
    let base = a.gfx.alpha;
    a.gfx.alpha = base * rest;
    right_indicator(a, left + w - 28.0, cy);
    a.gfx.alpha = base;
    if show > 0.3 {
        eyes(a, left + w / 2.0, cy, (show - 0.3) / 0.7, (w / 2.0 - 48.0).max(14.0));
    }
    // The battery reaches past the compact notch, so that scene redraws everything.
    if a.eyes.scene.x > 0.01 || a.eye_feel_wide() {
        a.loop_rect = None;
    } else if show > 0.01 || a.sample_running() {
        a.loop_rect = Some(Rc::new(left + 2.0, 0.0, w - 4.0, 30.0));
    }
}

fn clock_text(left_s: f64) -> String {
    let secs = left_s.ceil() as u64;
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

/// The focus sun's colours: bright orange red at its top, red below.
const SUN_HI: u32 = 0xFF8A66;
/// What the sun's lower red deepens to by the end of a session.
const SUN_DEEP: u32 = 0xB0140E;

/// How far the sun's centre sits below the horizon (negative: above) when `used` of the time is
/// gone: well up at the start, wholly under the line at the end.
/// Where a waiting sun sits, below the horizon by this many radii: about a quarter shows.
const SUN_WAITS: f32 = 0.5;

fn sun_below(used: f32, r: f32) -> f32 {
    -r * 0.7 + used.clamp(0.0, 1.0) * r * 1.9
}

/// The focus timer: a sun of lit lines, like the eyes, setting over a sea of hairlines. Its dark
/// gaps are thin up high and thicker toward the sea, and slide down slowly while the time
/// runs; it deepens to a darker red as it goes, and its light on the water goes with it. `big`:
/// the Focus page; otherwise the small one in the closed notch. `rise`: 0 while a session waits
/// to start (only its top peeks over the line), 1 once it runs.
#[allow(clippy::too_many_arguments)]
fn sun_scene(a: &mut App, cx: f32, horizon: f32, r: f32, left: f32, running: bool, big: bool, rise: f32) {
    let t = a.born.elapsed().as_secs_f32();
    let used = 1.0 - left.clamp(0.0, 1.0);
    let sun_y = horizon + SUN_WAITS * r + (sun_below(used, r) - SUN_WAITS * r) * rise.clamp(0.0, 1.0);
    // How much of it is still above the line.
    let up = ((horizon - (sun_y - r)) / (2.0 * r)).clamp(0.0, 1.0);
    let g = &a.gfx;
    // Its light in the sky: wide and bright at first, smaller as it goes, breathing a little.
    let breath = 0.5 + 0.5 * (t * 1.1).sin();
    let halo = 1.0 - 0.45 * used;
    g.glow_ellipse(cx, horizon - r * 0.2, r * 2.6 * halo, r * 1.5 * halo, RED, (0.1 + 0.22 * up) * (0.85 + 0.15 * breath));
    let hi = mix(SUN_HI, RED, used * 0.7);
    let lo = mix(RED, SUN_DEEP, used * 0.85);
    g.push_clip(Rc::new(cx - r * 3.0, horizon - r * 3.0, r * 6.0, r * 3.0));
    g.stroke_circle(cx, sun_y, r * 1.22, 1.0, hi, 0.25 + 0.2 * up);
    g.clip_circle(cx, sun_y, r, || {
        g.fill_rrect_vgrad(Rc::new(cx - r, sun_y - r, 2.0 * r, 2.0 * r), 0.0, (hi, 1.0), (lo, 1.0));
        // The gaps are the notch's own black, as between the eyes' lines.
        let pitch = if big { 5.2 } else { 3.2 };
        let slide = if running { (t * if big { 1.6 } else { 0.8 }) % pitch } else { 0.0 };
        let mut y = sun_y - r - pitch + slide;
        while y < sun_y + r {
            // Thicker the nearer they are to the sea, so whatever is still up keeps its lines.
            let k = 1.0 - (horizon - y) / (r * 1.3);
            if k >= 0.0 {
                let w = if big { 0.6 } else { 0.5 } + if big { 2.6 } else { 1.4 } * k.min(1.0);
                g.fill_rrect(Rc::new(cx - r - 1.0, y - w / 2.0, 2.0 * r + 2.0, w), 0.0, 0x000000, 1.0);
            }
            y += pitch;
        }
    });
    g.pop_clip();
    // The horizon: a red hairline fading out at both ends.
    let reach = if big { r * 3.2 } else { r * 1.4 };
    g.fill_rrect_hgrad(Rc::new(cx - reach, horizon - 0.6, reach, 1.2), 0.0, (RED, 0.0), (RED, 0.95));
    g.fill_rrect_hgrad(Rc::new(cx, horizon - 0.6, reach, 1.2), 0.0, (RED, 0.95), (RED, 0.0));
    // The sea: hairlines, and on them the sun's light, narrower further down, drifting slowly.
    let (rows, gap) = if big { (5, 6.0) } else { (2, 2.6) };
    let s = r / 30.0;
    for i in 0..rows {
        let k = i as f32;
        let y = horizon + gap * (k + 1.0);
        let w = reach * (0.9 - 0.14 * k);
        g.line(cx - w, y, cx + w, y, 1.0, 0x8E8E8E, 0.5 - 0.08 * k);
        let half = r * (1.05 - 0.18 * k) * (0.25 + 0.75 * up) + (t * 0.8 + k).sin() * 1.5 * s;
        let drift = (t * 0.5 + k * 1.7).sin() * 2.0 * s;
        let band = if big { 2.0 } else { 1.2 };
        if half > 1.0 {
            g.fill_rrect(Rc::new(cx + drift - half, y - band / 2.0, 2.0 * half, band), band / 2.0, RED, (0.9 - 0.16 * k) * (0.3 + 0.7 * up));
        }
    }
    // Done: a slow glow on the horizon where it went down.
    if used >= 1.0 {
        g.glow_ellipse(cx, horizon, r * 1.6, r * 0.35, RED, 0.25 + 0.15 * (t * 1.6).sin());
    }
}

/// The focus timer on the Focus page; `rise` as in `sun_scene`.
fn sun_slices(a: &mut App, cx: f32, left: f32, running: bool, rise: f32) {
    sun_scene(a, cx, 104.0, 28.0, left, running, true, rise);
}

/// The small sun that stays in the notch while focus runs, moving with it as it opens.
fn focus_sun(a: &mut App, left: f32, w: f32, h: f32) {
    if a.brain.focus.is_none() || a.live_alpha <= 0.01 {
        return;
    }
    let (time, total, running, _) = a.pomo_view();
    let open = ((h - 30.0) / 110.0).clamp(0.0, 1.0);
    let open = open * open * (3.0 - 2.0 * open);
    let ring = (left + w - 19.0, 14.8);
    let row = (left + (SLOT_LIVE.0 + SLOT_LIVE.1) / 2.0, 19.0);
    let at = (ring.0 + (row.0 - ring.0) * open, ring.1 + (row.1 - ring.1) * open);
    let base = a.gfx.alpha;
    // The Focus page has the big one; two suns would be one too many.
    a.gfx.alpha = a.live_alpha * if a.page == Page::Pomo && a.view == View::Page { 1.0 - open } else { 1.0 };
    sun_scene(a, at.0, at.1 + 8.0 * 0.42, 8.0, (time / total) as f32, running, false, 1.0);
    a.gfx.alpha = base;
}

/// Equalizer bars while music plays; while focus runs, how visible its sun may be.
fn right_indicator(a: &mut App, x: f32, cy: f32) {
    // The only part that moves when nothing else does: frames can redraw just this box.
    a.loop_rect = Some(Rc::new(x - 3.0, cy - 12.0, 27.0, 24.0));
    // Focus has its sun, drawn by `focus_sun`; this only says how visible it may be.
    if a.brain.focus.is_some() {
        a.live_alpha = a.gfx.alpha;
        return;
    }
    let mix = a.eq_mix.x.clamp(0.0, 1.0);
    if mix < 0.01 {
        return;
    }
    let t = a.born.elapsed().as_secs_f32();
    // The sound itself when it can be heard (bass on the left, treble on the right), else a sway.
    let heard = crate::spectrum::bands();
    for i in 0..5 {
        let fi = i as f32;
        let level = match heard {
            Some(_) => a.eq_bars[i],
            None => {
                let wave = 0.55 * (t * (2.1 + fi * 0.37) + fi * 1.9).sin() + 0.45 * (t * (3.3 - fi * 0.29) + fi * 0.7).sin();
                (0.5 + 0.5 * wave).clamp(0.0, 1.0)
            }
        };
        let bh = (3.0 + 7.4 * level) * mix;
        a.gfx.fill_rrect(Rc::new(x + i as f32 * 4.05, cy - bh / 2.0, 2.2, bh), 1.1, WHITE, (2.0 * mix).min(1.0));
    }
}

/// Looking up or down: how far the pair moves (-0.5 up to 1 down), and how much the lids lower, as
/// a thin bar has little room to go down in.
fn look_down(d: f32) -> (f32, f32) {
    let d = d.clamp(-1.0, 1.0);
    (if d > 0.0 { d } else { 0.5 * d }, 1.0 - 0.4 * d.max(0.0))
}

/// How far the pair tips when looking all the way to one side (about 8°).
const TILT: f32 = 0.14;

/// One eye: scan lines of light, with dark gaps that stay put as its outline moves. The gaps are
/// lines of the notch's own black, so what shows through is the lit part.
fn lens(g: &Gfx, outline: &[(f32, f32)], gap: f32, a: f32) {
    let c = crate::CHARACTER;
    g.fill_poly(outline, c, a);
    let mut rim = outline.to_vec();
    rim.push(rim[0]);
    g.polyline(&rim, 1.6, c, 0.35 * a);
    // Inside the outline only: on a coloured glow, lines past the edge would show.
    if g.push_outline_clip(outline) {
        stripes(g, outline, gap, a);
        g.pop_layer();
    }
}

fn stripes(g: &Gfx, shape: &[(f32, f32)], gap: f32, a: f32) {
    let span = |f: fn(&(f32, f32)) -> f32| shape.iter().map(f).fold((f32::MAX, f32::MIN), |r, v| (r.0.min(v), r.1.max(v)));
    let ((x0, x1), (y0, y1)) = (span(|p| p.0), span(|p| p.1));
    let mut y = (y0 / gap).floor() * gap;
    while y <= y1 + 1.0 {
        g.line(x0 - 3.0, y, x1 + 3.0, y, gap * 0.34, 0x000000, a);
        y += gap;
    }
}

/// The eyes in the notch, lit lines on black.
fn eyes(a: &mut App, cx: f32, cy: f32, show: f32, reach: f32) {
    a.eye_draws.0 += 1;
    /// Notch units per pixel of the design sheet.
    const SCALE: f32 = 0.165;
    const GAP: f32 = 3.4;
    let e = &a.eyes;
    // In the battery scene the eyes stay on their side and lunge at it.
    let scene = e.scene.x.clamp(0.0, 1.0);
    let centre = cx;
    let cx = cx - 46.0 * scene + 62.0 * e.lunge.x * scene;
    let reach = reach * (1.0 - scene);
    let lean = e.lean.x;
    let t = a.born.elapsed().as_secs_f32();
    let breath = 0.35 * (t * 1.7).sin();
    let g = &a.gfx;
    // Looking moves the whole pair, tipped a little as a head would be; the eyes keep their size.
    let (lower, squash) = look_down(e.drop.x);
    let (px, py) = (cx + lean * reach, cy + breath + e.hop.x + 6.0 * lower);
    g.rotate(px, py, TILT * lean);
    // They look all round inside the notch, never past its edge.
    let half = a.sw.x / 2.0;
    g.push_clip(Rc::new(centre - half + 6.0, 1.0, half * 2.0 - 12.0, a.sh.x - 2.0));
    for (i, side) in [-1.0f32, 1.0].into_iter().enumerate() {
        let outline: Vec<(f32, f32)> = e.pair[i].points.iter().map(|p| (px + p.0 * SCALE, py + p.1 * SCALE * squash)).collect();
        g.glow_ellipse(px + side * 20.0, py, 18.0, 15.0, crate::CHARACTER, 0.16 * show);
        lens(g, &outline, GAP, show * (1.0 - e.dizzy.x.clamp(0.0, 1.0)));
        let (ex, ey) = (px + side * 20.0, py);
        let dizzy = e.dizzy.x.clamp(0.0, 1.0);
        if dizzy > 0.01 {
            let turn = t * 7.0 * side;
            let spiral: Vec<(f32, f32)> = (0..=44)
                .map(|n| {
                    let k = n as f32 / 44.0;
                    let a = turn + k * 15.0;
                    (ex + 13.0 * k * a.cos(), ey + 11.0 * k * a.sin())
                })
                .collect();
            g.polyline(&spiral, 3.4, crate::CHARACTER, show * dizzy);
            stripes(g, &spiral, GAP, show * dizzy);
        }
    }
    // A file on its way: a "?" of the same lit lines comes up by the inner eye, then fades.
    let curious = a.drag_near.filter(|_| !a.drag_over).map(|t| t.elapsed().as_secs_f32());
    let question = (px + 37.0, py - 9.0);
    g.pop_clip();
    g.unzoom();
    // The battery never leaves the notch as it narrows again.
    g.push_clip(Rc::new(centre - half, 0.0, half * 2.0, a.sh.x));
    if scene > 0.01 {
        // What they are after slides in from the right and takes the blows.
        let (bx, by) = (centre + 66.0 + 60.0 * (1.0 - scene) + e.shove.x, cy);
        if a.nudging_bell {
            g.rotate(bx, by - 6.0, 0.5 * e.rock.x);
            g.zoom(bx, by, 1.5);
            icon::bell(g, bx, by, WHITE, scene * show);
            g.unzoom();
            g.unzoom();
            g.pop_clip();
            return;
        }
        let pct = a.power.map_or(10, |p| p.pct);
        g.rotate(bx, by, 0.09 * e.rock.x);
        crate::anim::battery(g, bx, by, 40.0, 20.0, e.dent.x.clamp(0.0, 1.0));
        g.unzoom();
        let rise = 3.0 * e.dent.x.clamp(0.0, 1.0);
        let label = format!("{pct}%");
        let at = Rc::new(bx + 28.0 + e.shove.x * 0.4, cy - 10.0 - rise, 44.0, 20.0);
        a.gfx.ui(&label, at, 15.0, 700, 0xFF3B30, scene * show, Align::L);
    }
    if let Some(age) = curious {
        let from = crate::CURIOUS_FROM + 0.4;
        let k = ((age - from) / 0.2).clamp(0.0, 1.0) * (1.0 - (age - crate::CURIOUS_UNTIL + 0.6) / 0.6).clamp(0.0, 1.0);
        if k > 0.01 {
            let r = Rc::new(question.0 - 8.0, question.1 - 9.0 - 2.0 * k, 16.0, 18.0);
            a.gfx.ui("?", r, 17.0, 800, crate::CHARACTER, show * k, Align::C);
            let rect = [(r.x, r.y), (r.x + r.w, r.y), (r.x + r.w, r.y + r.h), (r.x, r.y + r.h)];
            stripes(&a.gfx, &rect, GAP, show * k);
        }
        if age < crate::CURIOUS_UNTIL + 0.2 {
            a.kick();
        }
    }
    a.gfx.pop_clip();
}

/// The small eyes, `scale` notch units to a pixel of their design sheet.
fn mini_eyes(a: &mut App, cx: f32, cy: f32, scale: f32) {
    let hop = a.mini.hop.x;
    for eye in &a.mini.pair {
        let outline: Vec<(f32, f32)> = eye.points.iter().map(|p| (cx + p.0 * scale, cy + hop + p.1 * scale)).collect();
        lens(&a.gfx, &outline, 2.4, 1.0);
    }
}

/// Music notes rising on either side of `x`.
fn jam_notes(a: &mut App, x: f32, reach: f32, h: f32) {
    let t = a.born.elapsed().as_secs_f32();
    for &(born, side, pair) in &a.jam.notes {
        let k = (t - born) / crate::eyes::NOTE_LIFE;
        if (0.0..1.0).contains(&k) {
            let nx = x + side * reach + 2.0 * (k * 7.0).sin();
            crate::anim::note(&a.gfx, nx, h - 7.0 - (h - 14.0) * k, 5.0, pair, crate::CHARACTER, 1.0 - k * k * k);
        }
    }
}

fn lyric_pill(a: &mut App, left: f32, w: f32) {
    let line = a.lyric_line().unwrap_or_default();
    // Art-tinted glow rising from the bottom edge, as on the expanded lyrics view.
    let h = a.sh.x;
    let radius = 12.4 + 19.2 * ((h - 30.0) / 110.0).clamp(0.0, 1.0);
    a.gfx.fill_rrect_vgrad(Rc::new(left, 0.0, w, h), radius, (art_tint(a), 0.0), (art_tint(a), 0.85));
    // The lyrics keep their place when the bar grows to one side for the eyes.
    let (bar, bar_w) = (crate::WIN_W / 2.0 - a.sw.x / 2.0, a.sw.x);
    icon::note(&a.gfx, bar + 22.0, 16.0, 0xBDBDBD, 1.0);
    // Now and then the eyes take the bar's place and sway to the music, notes rising round them.
    let out = a.jam_x.x.clamp(0.0, 1.0);
    if out > 0.02 {
        let x = bar + bar_w / 2.0;
        let sway = a.jam_sway();
        let base = a.gfx.alpha;
        a.gfx.alpha = base * out;
        a.gfx.push_clip(Rc::new(left, 0.0, w, h));
        let (px, py) = (x + 5.0 * sway, h / 2.0 + a.mini.hop.x);
        a.gfx.rotate(px, py, 0.2 * sway);
        for side in [-1.0f32, 1.0] {
            let oval: Vec<(f32, f32)> = (0..24)
                .map(|n| {
                    let k = n as f32 / 24.0 * std::f32::consts::TAU;
                    (px + side * 9.0 + 5.2 * k.cos(), py + 7.0 * k.sin())
                })
                .collect();
            lens(&a.gfx, &oval, 2.4, 1.0);
        }
        a.gfx.unzoom();
        jam_notes(a, x, 34.0, h);
        a.gfx.pop_clip();
        a.gfx.alpha = base;
    }
    let lyric = 1.0 - out;
    if line != a.pill_cur {
        let old = std::mem::replace(&mut a.pill_cur, line.clone());
        a.pill_prev = if old.is_empty() { None } else { Some((std::time::Instant::now(), old)) };
        a.kick();
    }
    // The outgoing line lifts away as the new one rises into place.
    let text = |a: &mut App, s: &str, dy: f32, al: f32| {
        let base = a.gfx.alpha;
        a.gfx.alpha = base * al * lyric;
        a.gfx.push_clip(Rc::new(left, 0.0, w, h));
        a.gfx.ui(s, Rc::new(bar + 44.0, 6.0 + dy, bar_w - 88.0, 22.0), 13.0, 600, WHITE, 1.0, Align::C);
        a.gfx.pop_clip();
        a.gfx.alpha = base;
    };
    match a.pill_prev.clone() {
        Some((t0, old)) if t0.elapsed().as_secs_f32() < 0.45 => {
            let p = settle(t0.elapsed().as_secs_f32());
            text(a, &old, -12.0 * p, 1.0 - p);
            text(a, &line, 12.0 * (1.0 - p), p);
        }
        _ => text(a, &line, 0.0, 1.0),
    }
}

// ---------------------------------------------------------------- toast

fn toast(a: &mut App, left: f32, w: f32, t: &crate::Toast, live: bool) {
    let (kind, title, value, level) = (t.kind, t.title.clone(), t.value.clone(), t.level);
    if kind.tall() {
        tall_toast(a, left, w, t, live);
        return;
    }
    let cy = 17.6;
    let ix = left + 24.0;
    match kind {
        ToastKind::Volume(muted) => {
            // The endpoint impact belongs to the toast silhouette.  Keeping
            // the meter itself stable avoids a second, conflicting stretch.
            let lv = if live { a.vol_level.x } else { level.unwrap_or(0.0) }.clamp(0.0, 1.0);
            icon::speaker(&a.gfx, ix + 1.2, cy, lv, muted, WHITE, 1.0);
            let bar = Rc::new(left + 49.6, cy - 2.8, w - 49.6 - 24.0, 6.4);
            a.gfx.fill_rrect(bar, 3.2, 0x474747, 1.0);
            if lv > 0.0 {
                a.gfx.fill_rrect(Rc::new(bar.x, bar.y, (bar.w * lv).max(bar.h), bar.h), 3.2, WHITE, 1.0);
            }
            return;
        }
        ToastKind::Brightness => {
            let lv = if live { a.vol_level.x } else { level.unwrap_or(0.0) }.clamp(0.0, 1.0);
            icon::sun(&a.gfx, ix + 1.2, cy, a.sun_rot.x.to_radians(), lv, WHITE, 1.0);
            let bar = Rc::new(left + 49.6, cy - 2.8, w - 49.6 - 24.0, 6.4);
            a.gfx.fill_rrect(bar, 3.2, 0x474747, 1.0);
            if lv > 0.0 {
                a.gfx.fill_rrect(Rc::new(bar.x, bar.y, (bar.w * lv).max(bar.h), bar.h), 3.2, WHITE, 1.0);
            }
            return;
        }
        ToastKind::Caps(_) => {
            let k = a.caps_mix.x.clamp(0.0, 1.0);
            icon::caps(&a.gfx, ix, cy, false, WHITE, 1.0 - k, 0.0);
            icon::caps(&a.gfx, ix, cy, true, WHITE, k, 0.0);
            icon::caps_bar(&a.gfx, ix, cy, WHITE, 0.35 + 0.65 * k);
        }
        ToastKind::Charging(_) | ToastKind::Battery => icon::charging(&a.gfx, ix, cy, 8.2, level.unwrap_or_else(|| a.charge_level()), 1.0),
        ToastKind::Focus | ToastKind::Break | ToastKind::Task => icon::stopwatch(&a.gfx, ix, cy + 1.6, 6.0, RED, 1.0),
        ToastKind::Net(ok) => {
            for r in [9.0f32, 5.8, 2.6] {
                a.gfx.arc_between(ix, cy + 5.0, r, -2.36, -0.78, 1.7, if ok { WHITE } else { GREY }, 1.0);
            }
        }
        _ => {
            a.gfx.fill_circle(ix, cy, 9.0, 9.0, 0x2A2A2A, 1.0);
            a.gfx.ui(&value, Rc::new(ix - 9.0, cy - 9.0, 18.0, 18.0), 9.5, 700, WHITE, 1.0, Align::C);
        }
    }
    a.gfx.ui(&title, Rc::new(left + 49.8, cy - 9.8, w - 140.0, 20.0), 13.0, 400, WHITE, 1.0, Align::L);
    let vcol = |k: ToastKind| match k {
        ToastKind::Caps(true) | ToastKind::Charging(true) | ToastKind::Net(true) | ToastKind::Bluetooth(true) => GREEN,
        ToastKind::Battery => RED,
        _ => GREY,
    };
    let vr = |dy: f32| Rc::new(left + w - 24.0 - 90.0, cy - 9.8 + dy, 90.0, 20.0);
    // A changed value rolls: the new one slides in from below (on) or above (off) as the old leaves.
    match a.roll.clone().filter(|_| live && a.roll_t.x < 0.999) {
        Some((old, dir)) => {
            let r = a.roll_t.x.clamp(0.0, 1.0);
            let base = a.gfx.alpha;
            let old_kind = match kind {
                ToastKind::Caps(on) => ToastKind::Caps(!on),
                k => k,
            };
            a.gfx.alpha = base * (1.0 - r);
            a.gfx.ui(&old, vr(-dir * 9.0 * r), 13.0, 400, vcol(old_kind), 1.0, Align::R);
            a.gfx.alpha = base * r;
            a.gfx.ui(&value, vr(dir * 9.0 * (1.0 - r)), 13.0, 400, vcol(kind), 1.0, Align::R);
            a.gfx.alpha = base;
        }
        None => a.gfx.ui(&value, vr(0.0), 13.0, 400, vcol(kind), 1.0, Align::R),
    }
}

/// Icon drawn inside a status badge or indicator ring.
#[derive(Clone, Copy, PartialEq)]
enum Glyph {
    Bluetooth,
    /// Wi-Fi arcs revealed 0..3; `true` when connected.
    Wifi(f32, bool),
    /// Signal bars lit 0..4.
    Bars(f32),
    Bolt,
    Battery,
    Check(f32),
    Drop,
    Bell,
    /// Nothing: the caller draws over the badge.
    Blank,
    /// What a connected device is (headphones, mouse, drive...).
    Device(DeviceKind),
    Usb,
}

fn glyph(a: &mut App, g: Glyph, cx: f32, cy: f32, hex: u32) {
    match g {
        Glyph::Device(k) => icon::device(&a.gfx, k, cx, cy, hex, 1.0),
        Glyph::Usb => icon::usb(&a.gfx, cx, cy, hex, 1.0),
        Glyph::Blank => {}
        Glyph::Bluetooth => icon::bluetooth(&a.gfx, cx, cy, hex, 1.0),
        // The arcs' bounding box is centred on (cx, cy): dot 4.6 below, outer arc 7.4 above.
        Glyph::Wifi(arcs, _) => icon::wifi_centered(&a.gfx, cx, cy, arcs, hex, 1.0),
        Glyph::Bars(lit) => {
            for i in 0..4 {
                let h = 3.4 + i as f32 * 2.6;
                // Each bar pops up in turn with a small overshoot.
                let k = (lit - i as f32).clamp(0.0, 1.0);
                let pop = if k > 0.0 && k < 1.0 { 1.0 + 0.25 * (std::f32::consts::PI * k).sin() } else { 1.0 };
                let hh = h * if k > 0.0 { pop } else { 1.0 };
                let r = Rc::new(cx - 7.0 + i as f32 * 3.8, cy + 5.0 - hh, 2.6, hh);
                a.gfx.fill_rrect(r, 1.3, mix(0x3A3A3A, hex, k), 1.0);
            }
        }
        Glyph::Bolt => icon::bolt_small(&a.gfx, cx, cy, hex, 1.0),
        Glyph::Battery => {
            a.gfx.stroke_rrect(Rc::new(cx - 6.0, cy - 3.6, 11.0, 7.2), 1.8, 1.4, hex, 1.0);
            a.gfx.fill_rrect(Rc::new(cx + 5.4, cy - 1.6, 1.6, 3.2), 0.6, hex, 1.0);
        }
        Glyph::Check(k) => {
            // Drawn stroke by stroke.
            let pts = [(cx - 5.0, cy + 0.2), (cx - 1.4, cy + 3.8), (cx + 5.4, cy - 3.6)];
            let k = k.clamp(0.0, 1.0);
            if k > 0.0 {
                let first = (k / 0.4).min(1.0);
                let p1 = (pts[0].0 + (pts[1].0 - pts[0].0) * first, pts[0].1 + (pts[1].1 - pts[0].1) * first);
                let mut line = vec![pts[0], p1];
                if k > 0.4 {
                    let s = (k - 0.4) / 0.6;
                    line.push((pts[1].0 + (pts[2].0 - pts[1].0) * s, pts[1].1 + (pts[2].1 - pts[1].1) * s));
                }
                a.gfx.polyline(&line, 2.0, hex, 1.0);
            }
        }
        Glyph::Drop => icon::droplet(&a.gfx, cx, cy + 1.4, 8.0, hex, 1.0),
        Glyph::Bell => {
            a.gfx.zoom(cx, cy, 1.3);
            icon::bell(&a.gfx, cx, cy, hex, 1.0);
            a.gfx.unzoom();
        }
    }
}

/// The shared card layout of every two-line toast, modelled on the Bluetooth toast: a round badge
/// (accent ring, dark centre, glyph) on the left, bold title over a grey line, and an optional
/// indicator ring on the right that fills to `level` while ripples run out.
struct Card {
    accent: u32,
    dark: u32,
    badge: Glyph,
    /// Badge motion: shake amplitude (radians) for alarms, 0 for none.
    shake: f32,
    /// Right indicator: (glyph inside, ring fill 0..1); None leaves the space for buttons/text.
    ring: Option<(Glyph, f32)>,
    /// Right edge reserved for buttons.
    reserve: f32,
}

fn card(a: &mut App, left: f32, w: f32, h: f32, t: &crate::Toast, title: &str, sub: &str, c: &Card) {
    let age = t.born.elapsed().as_secs_f32();
    let cy = h / 2.0;
    let bx = left + 32.0;
    let tx = left + 62.0;
    // Badge: a ripple leaves it once, the ring breathes gently.
    let q = (age / 0.8).clamp(0.0, 1.0);
    if matches!(c.badge, Glyph::Bell) {
        bell(a, bx, cy, 28.0, age);
    } else {
        if q < 1.0 {
            a.gfx.stroke_circle(bx, cy, 14.4 + 8.0 * q, 1.4, c.accent, 0.5 * (1.0 - q));
        }
        a.gfx.fill_circle(bx, cy, 14.4, 14.4, c.accent, 1.0);
        a.gfx.fill_circle(bx, cy, 11.2, 11.2, c.dark, 1.0);
        // Three shakes, then still (a card waiting for an answer stops redrawing; see effects()).
        let shake = c.shake > 0.0 && age < 4.8;
        if shake {
            a.gfx.rotate(bx, cy, ring_angle(age, c.shake));
        }
        glyph(a, c.badge, bx, cy, WHITE);
        if shake {
            a.gfx.unzoom();
        }
    }
    let right_w = if c.ring.is_some() { 56.0 } else { c.reserve };
    let tw = w - (tx - left) - right_w - 8.0;
    // The words slide out from behind the icon, the second line a beat later.
    let settle = |d: f32| {
        let k = ((age - d) / 0.32).clamp(0.0, 1.0) - 1.0;
        1.0 + 2.7 * k * k * k + 1.7 * k * k
    };
    a.gfx.push_clip(Rc::new(tx - 4.0, 0.0, tw + 8.0, h));
    let (k1, k2) = (settle(0.04), settle(0.12));
    a.gfx.ui(title, Rc::new(tx - 34.0 * (1.0 - k1), cy - 17.6, tw, 20.0), 15.0, 700, WHITE, k1.clamp(0.0, 1.0), Align::L);
    a.gfx.ui(sub, Rc::new(tx - 34.0 * (1.0 - k2), cy + 2.4, tw, 16.0), 11.5, 400, 0xA0A0A0, k2.clamp(0.0, 1.0), Align::L);
    a.gfx.pop_clip();
    if let Some((g, level)) = c.ring {
        let ex = left + w - 36.0;
        for d in [0.0f32, 0.35] {
            let q = ((age - d) / 0.8).clamp(0.0, 1.0);
            if q > 0.0 && q < 1.0 {
                a.gfx.stroke_circle(ex, cy, 15.0 + 9.0 * q, 1.4, c.accent, 0.5 * (1.0 - q));
            }
        }
        let settle = ((age - 1.0) / 0.5).clamp(0.0, 1.0);
        a.gfx.fill_circle(ex, cy, 15.0, 15.0, 0x0E0E10, 1.0);
        a.gfx.stroke_circle(ex, cy, 15.0, 2.0, 0x2A2D30, 1.0);
        if level > 0.0 {
            a.gfx.arc(ex, cy, 15.0, level, 2.0, mix(c.accent, mix(c.accent, 0x3A3A3A, 0.6), settle), 1.0);
        }
        glyph(a, g, ex, cy, WHITE);
    }
}

/// A notification grown into its detail view: app row, title, wrapped content (or a screenshot
/// preview) and Open / Close buttons on the bottom row.
fn notice_detail(a: &mut App, left: f32, w: f32, h: f32, live: bool) {
    let Some(n) = a.notice.as_ref() else { return };
    let info = n.clone();
    let (kind, app, site, title, body, born, thumb, file, size, openable, aumid) = (
        n.kind,
        n.app.clone(),
        n.site.clone(),
        n.title.clone(),
        n.body.clone(),
        n.born,
        n.thumb.clone(),
        n.file.clone(),
        n.file_size.clone(),
        n.openable(),
        n.aumid.clone(),
    );
    let pad = 22.0;
    let (x0, x1) = (left + pad, left + w - pad);
    // Header: badge, source name, age.
    let hy = 26.0;
    match kind {
        crate::NoticeKind::Chat | crate::NoticeKind::Phone => notice_badge(a, &info, x0 + 11.0, hy, 11.0),
        _ => {
            let icon = (!aumid.is_empty()).then(|| std::path::PathBuf::from(format!("shell:AppsFolder\\{aumid}")));
            let bmp = icon.and_then(|p| a.shelf_icon(&p).0);
            match bmp {
                Some(b) => a.gfx.image_fit(&b, Rc::new(x0, hy - 11.0, 22.0, 22.0), 5.0, 1.0, false),
                None => {
                    a.gfx.fill_rrect(Rc::new(x0, hy - 11.0, 22.0, 22.0), 6.0, 0x2C2C2E, 1.0);
                    let letter: String = app.chars().take(1).collect::<String>().to_uppercase();
                    a.gfx.ui(&letter, Rc::new(x0, hy - 11.0, 22.0, 22.0), 12.0, 700, WHITE, 1.0, Align::C);
                }
            }
        }
    }
    let source = match (&kind, &site) {
        (crate::NoticeKind::Chat, _) => "Messenger".to_string(),
        (crate::NoticeKind::Phone, _) => info.source_name(),
        (crate::NoticeKind::Screenshot, _) => "Snipping Tool".to_string(),
        (_, Some(s)) => format!("{app} · {}", s.trim_start_matches("www.")),
        _ => app.clone(),
    };
    let secs = born.elapsed().as_secs();
    let age = if secs < 60 { "now".to_string() } else { format!("{} min", secs / 60) };
    a.gfx.ui(&age, Rc::new(x1 - 60.0, hy - 9.0, 60.0, 18.0), 11.5, 400, 0x8E8E93, 1.0, Align::R);
    a.gfx.ui(&source, Rc::new(x0 + 32.0, hy - 9.0, x1 - x0 - 32.0 - 66.0, 18.0), 12.0, 600, 0xA0A0A5, 1.0, Align::L);

    // Content, with the answers in a column of their own on the right.
    let top = 46.0;
    let bottom = h - 14.0;
    let x1 = x1 - 136.0;
    if kind == crate::NoticeKind::Screenshot {
        let tr = Rc::new(x0, top, 128.0, 72.0);
        match &thumb {
            Some(b) => a.gfx.image_fit(b, tr, 8.0, 1.0, true),
            None => a.gfx.fill_rrect(tr, 8.0, 0x1C1C1E, 1.0),
        }
        a.gfx.stroke_rrect(tr, 8.0, 1.0, 0x3A3A3C, 1.0);
        let tx = tr.x + tr.w + 12.0;
        let tw = (x1 - tx + 40.0).max(40.0);
        a.gfx.ui("Screenshot", Rc::new(tx, top + 2.0, tw, 20.0), 14.0, 600, WHITE, 1.0, Align::L);
        let line = match &file {
            Some(f) => f.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
            None => "Copied to clipboard".to_string(),
        };
        a.gfx.ui(&line, Rc::new(tx, top + 26.0, tw, 17.0), 12.0, 400, 0xC7C7CC, 1.0, Align::L);
        if !size.is_empty() {
            a.gfx.ui(&size, Rc::new(tx, top + 45.0, tw, 16.0), 11.5, 400, 0x8E8E93, 1.0, Align::L);
        }
    } else {
        a.gfx.ui(&title, Rc::new(x0, top, x1 - x0, 20.0), 15.0, 600, WHITE, 1.0, Align::L);
        let by = top + 24.0;
        let room = (bottom - by).max(0.0);
        let tall = a.gfx.measure_wrapped(&body, 13.0, 400, x1 - x0);
        a.notice_scroll_max = (tall - room).max(0.0);
        let down = a.notice_scroll.x.clamp(0.0, a.notice_scroll_max);
        a.gfx.push_clip(Rc::new(x0, by, x1 - x0, room));
        a.gfx.text(&body, Rc::new(x0, by - down, x1 - x0, tall + 4.0), 13.0, 400, 0xD8D8DC, 1.0, Align::LW, Font::Ui);
        a.gfx.pop_clip();
        if a.notice_scroll_max > 0.0 {
            // How much is left, on the text's right edge.
            let bar = room * room / tall;
            let y = by + (room - bar) * down / a.notice_scroll_max;
            a.gfx.fill_rrect(Rc::new(x1 + 6.0, by, 3.0, room), 1.5, WHITE, 0.1);
            a.gfx.fill_rrect(Rc::new(x1 + 6.0, y, 3.0, bar), 1.5, WHITE, 0.6);
        }
    }

    // Close always; Open when it leads somewhere. Clicking the content opens too.
    let cy = h / 2.0 + 8.0;
    if live {
        hit(a, Rc::new(left, top - 4.0, x1 - left, h - top), if openable { Act::NoticeOpen } else { Act::None });
    }
    labelled_hot(a, left + w - pad - 22.0, cy, "Close", live, 0xE5484D, Act::NoticeClose, |g, x, y| icon::cross(g, x, y, 5.5, WHITE, 1.0));
    if openable {
        let label = if kind == crate::NoticeKind::Chat { "Reply" } else { "Open" };
        labelled(a, left + w - pad - 84.0, cy, label, live, Act::NoticeOpen, |g, x, y| icon::open(g, x, y, WHITE, 1.0));
    }
}

const AMBER: u32 = 0xFFB020;

/// Battery colours (accent, dark badge): above 50 % green, 21-50 % amber, 20 % and below red.
/// Called with the counting-up level, so a card climbs through the colours to the real one.
fn battery_tone(level: f32) -> (u32, u32) {
    let blend = |lo: f32, x: f32| ((x - lo) / 0.01).clamp(0.0, 1.0);
    let l = level.clamp(0.0, 1.0);
    if l > 0.5 {
        let k = blend(0.5, l);
        (mix(AMBER, GREEN, k), mix(0x3A2A08, 0x0C2E14, k))
    } else if l > 0.2 {
        let k = blend(0.2, l);
        (mix(RED, AMBER, k), mix(0x3A0E10, 0x3A2A08, k))
    } else {
        (RED, 0x3A0E10)
    }
}

/// Count-up shared by every card: 0 → 1 over 0.9 s, eased out.
fn count_up(t: &crate::Toast) -> f32 {
    let k = (t.born.elapsed().as_secs_f32() / 0.9).clamp(0.0, 1.0);
    1.0 - (1.0 - k).powi(3)
}

/// Rounded action button used on interactive cards (same height, radius and type everywhere).
fn card_button(a: &mut App, r: Rc, label: &str, bg: u32, fg: u32, live: bool, act: Act) {
    let h = if live { btn(a, r, 0.07) } else { 0.0 };
    a.gfx.fill_rrect(r, r.h / 2.0, mix(bg, mix(bg, 0xFFFFFF, 0.12), h), 1.0);
    a.gfx.ui(label, r, 12.0, 600, fg, 1.0, Align::C);
    a.gfx.unzoom();
    if live {
        hit(a, r, act);
    }
}

pub fn prop_bytes(name: &str) -> Option<&'static [u8]> {
    Some(match name {
        "ic_water" => include_bytes!("../assets/props/ic_water.png"),
        "ic_stretch" => include_bytes!("../assets/props/ic_stretch.png"),
        "ic_eyes" => include_bytes!("../assets/props/ic_eyes.png"),
        "ic_breathe" => include_bytes!("../assets/props/ic_breathe.png"),
        "ic_sleep" => include_bytes!("../assets/props/ic_sleep.png"),
        "ic_focus" => include_bytes!("../assets/props/ic_focus.png"),
        "ic_star" => include_bytes!("../assets/props/ic_star.png"),
        "t_today" => include_bytes!("../assets/props/t_today.png"),
        "t_focus" => include_bytes!("../assets/props/t_focus.png"),
        "t_care" => include_bytes!("../assets/props/t_care.png"),
        "t_todo" => include_bytes!("../assets/props/t_todo.png"),
        "t_shelf" => include_bytes!("../assets/props/t_shelf.png"),
        "bell0" => include_bytes!("../assets/props/bell0.png"),
        "bell1" => include_bytes!("../assets/props/bell1.png"),
        _ => return None,
    })
}

/// One part of a prop: placed by its box, turned about `pivot`.
fn part(a: &mut App, name: &'static str, tinted: bool, r: Rc, pivot: (f32, f32), turn: f32) {
    let Some(bmp) = a.prop(name, tinted) else { return };
    a.gfx.rotate(pivot.0, pivot.1, turn);
    a.gfx.image(&bmp, r, 0.0, 1.0);
    a.gfx.unzoom();
}

/// The bell on plain cards: it rings for a moment, then waits.
fn bell(a: &mut App, x: f32, y: f32, s: f32, t: f32) {
    let swing = |t: f32, reach: f32| {
        let q = (t.rem_euclid(2.6) / 1.7).clamp(0.0, 1.0);
        reach * (std::f32::consts::PI * q).sin().max(0.0).powf(0.7) * (1.0 - q) * (q * 22.0).sin()
    };
    let r = Rc::new(x - s / 2.0, y - s / 2.0, s, s);
    let top = (x, y - s / 2.0);
    part(a, "bell1", true, r, top, swing(t - 0.07, 0.55));
    part(a, "bell0", false, r, top, swing(t, 0.38));
}

/// The large eyes on an act's card, drawn apart by `spread`.
fn act_eyes(a: &mut App, cx: f32, cy: f32, scale: f32, spread: f32) {
    a.eye_draws.1 += 1;
    a.card_eyes_x = cx;
    let hop = a.eyes.hop.x;
    let lean = a.eyes.lean.x;
    let (lower, squash) = look_down(a.eyes.drop.x);
    let (px, py) = (cx + lean * 7.0, cy + hop + 9.0 * lower);
    a.gfx.rotate(px, py, TILT * lean);
    for (i, side) in [-1.0f32, 1.0].into_iter().enumerate() {
        let x = px + side * spread;
        a.gfx.glow_ellipse(x + side * 120.0 * scale, py, 16.0, 14.0, crate::CHARACTER, 0.14);
        let outline: Vec<(f32, f32)> = a.eyes.pair[i].points.iter().map(|p| (x + p.0 * scale, py + p.1 * scale * squash)).collect();
        lens(&a.gfx, &outline, 3.0, 1.0);
    }
    a.gfx.unzoom();
}

fn act_card(a: &mut App, left: f32, w: f32, h: f32, title: &str, value: &str, kind: crate::eyes::ActKind, e: f32) {
    use crate::eyes::ActKind as K;
    const S: f32 = 40.0;
    let f = crate::eyes::act(kind, e);
    let cy = h / 2.0;
    let centre = left + w / 2.0;
    let out = f.title;
    let (shove, rock) = (a.eyes.shove.x, a.eyes.rock.x);
    // The prop slides out from the eyes as they hit it.
    let px = centre - 110.0 + shove - 46.0 * (1.0 - out);
    let shown = out.clamp(0.0, 1.0);
    if shown > 0.01 {
        let base = a.gfx.alpha;
        a.gfx.alpha = base * shown;
        match kind {
            K::Water => {
                a.gfx.rotate(px, cy + S / 2.0, 0.09 * rock + 0.03 * (e * 1.6).sin());
                let drink = a.drink_time();
                if drink > 0.0 {
                    // Drunk from the top down.
                    let age = a.reaction.map_or(0.0, |r| r.0.elapsed().as_secs_f32());
                    let top = cy - S / 2.0 + S * (age / drink).clamp(0.0, 1.0);
                    a.gfx.push_clip(Rc::new(px - S, top, 2.0 * S, S));
                    a.gfx.droplet(px, cy + 3.0, S * 0.48, WATER, 1.0);
                    a.gfx.pop_clip();
                }
                pic(a, "ic_water", px, cy + 0.6 * (e * 3.4).sin(), S, WATER, 1.0);
                a.gfx.unzoom();
            }
            K::Sleep | K::Nod => {
                a.gfx.rotate(px, cy, 0.1 * rock);
                pic(a, "ic_sleep", px, cy + 3.0 - 7.0 * f.play, S, WHITE, 1.0);
                a.gfx.unzoom();
            }
            K::Breathe => {
                pic(a, "ic_breathe", px + 3.0 * f.swell, cy, S * (1.0 + 0.12 * f.swell), WHITE, 1.0);
            }
            K::Stretch | K::Rest => {}
        }
        a.gfx.alpha = base;
    }
    let prop = !matches!(kind, K::Stretch | K::Rest);
    let scale = 0.13 * (1.0 + 0.14 * f.swell);
    // Rest to contact: the near eye's edge ends exactly on the edge of what it hits.
    let eyes_at = if prop { centre - 187.0 } else { centre - 130.0 - if kind == K::Rest { 24.0 } else { 0.0 } };
    let reach = 36.0;
    a.gfx.push_clip(Rc::new(left + 12.0, 0.0, w - 24.0, h));
    act_eyes(a, eyes_at + reach * a.eyes.lunge.x, cy, scale, 33.0 * f.stretch);
    a.gfx.pop_clip();
    if kind == K::Nod {
        // Each nod lets out a few soft z's over the eyes.
        let beat = e % crate::eyes::NOD;
        for (i, size) in [9.0, 11.0, 13.0].into_iter().enumerate() {
            let k = (beat - 0.9 - 0.5 * i as f32) / 2.4;
            if (0.0..1.0).contains(&k) {
                let at = Rc::new(eyes_at + 20.0 + 9.0 * i as f32, cy - 26.0 - 22.0 * k, 16.0, 16.0);
                a.gfx.ui("z", at, size, 600, 0xB8B8BC, (1.0 - k) * (k * 6.0).min(1.0), Align::C);
            }
        }
    }
    // The words come out from behind the prop, the second line a beat later.
    let tx = centre - 72.0 + if kind == K::Rest { -24.0 } else { 0.0 };
    let tw = 170.0;
    a.gfx.push_clip(Rc::new(tx - 4.0, 0.0, tw + 70.0, h));
    // The words give way: the eye's edge can push them but never pass through.
    let eye_edge = eyes_at + reach * a.eyes.lunge.x + 33.0 * f.stretch + 166.0 * scale + 7.0;
    let drift = 6.0 * f.swell + (eye_edge + 10.0 - tx).max(0.0);
    let (k1, k2) = (f.title, f.subtitle);
    a.gfx.ui(title, Rc::new(tx - 60.0 * (1.0 - k1) + 1.0 * shove + drift, cy - 17.6, tw, 20.0), 15.0, 700, WHITE, k1.clamp(0.0, 1.0), Align::L);
    a.gfx.ui(value, Rc::new(tx - 60.0 * (1.0 - k2) + 0.8 * shove + drift, cy + 2.4, tw, 16.0), 11.5, 400, 0xA0A0A0, k2.clamp(0.0, 1.0), Align::L);
    a.gfx.pop_clip();
}

/// A notification's text on its small card: hidden until hovered unless the user wants it shown.
fn notice_text<'a>(a: &App, text: &'a str) -> &'a str {
    if a.settings.show_text {
        text
    } else {
        "New notification"
    }
}

/// Two-line toasts, all on the shared `card` layout.
fn tall_toast(a: &mut App, left: f32, w: f32, t: &crate::Toast, live: bool) {
    let (kind, title, value) = (t.kind, t.title.as_str(), t.value.as_str());
    let age = t.born.elapsed().as_secs_f32();
    let h = a.sh.x.max(40.0);
    let up = count_up(t);
    // Buttons: 28 tall, right-aligned with a 14 margin and 8 between.
    let buttons = |w1: f32, w2: f32| {
        let cy = h / 2.0;
        let b2 = Rc::new(left + w - 14.0 - w2, cy - 14.0, w2, 28.0);
        let b1 = Rc::new(b2.x - 8.0 - w1, cy - 14.0, w1, 28.0);
        (b1, b2)
    };
    match kind {
        ToastKind::Song | ToastKind::Sample(7) => {
            // What has just started: cover, title and artist, the music moving on the right.
            let cy = h / 2.0;
            let cover = Rc::new(left + 18.0, cy - 19.0, 38.0, 38.0);
            if a.art.is_some() {
                art(a, cover);
            } else {
                a.gfx.fill_rrect(cover, 8.0, 0x2A2A2A, 1.0);
                icon::note(&a.gfx, cover.cx(), cover.cy(), 0x8A8A8A, 1.0);
            }
            let (tx, tw) = (left + 68.0, w - 68.0 - 50.0);
            a.gfx.ui(title, Rc::new(tx, cy - 17.6, tw, 20.0), 15.0, 700, WHITE, 1.0, Align::L);
            a.gfx.ui(value, Rc::new(tx, cy + 2.4, tw, 16.0), 11.5, 400, 0xA0A0A0, 1.0, Align::L);
            right_indicator(a, left + w - 42.0, cy);
        }
        ToastKind::Sample(i) if i <= 4 => {
            if let Some((act, e)) = a.act_now() {
                act_card(a, left, w, h, title, value, act, e);
                let shown = crate::eyes::act(act, e).buttons;
                if a.reaction.is_none() {
                    let base = a.gfx.alpha;
                    a.gfx.alpha = base * shown;
                    card_marks(a, left, w, h, t, false, &[(Mark::Later, "Later", Act::None), (Mark::Tick(if i == 0 { WATER } else { WHITE }), "Done", Act::None)]);
                    a.gfx.alpha = base;
                }
            }
        }
        ToastKind::Sample(i) => {
            // Nothing on the right, so the picture and text sit together in the middle of the card.
            const ICON: f32 = 60.0;
            const GAP: f32 = 14.0;
            let cy = h / 2.0;
            let clock = a.born.elapsed().as_secs_f32();
            let text_w = a.gfx.measure(title, 15.0, 700).max(a.gfx.measure(value, 11.5, 400));
            let x0 = left + (w - (ICON + GAP + text_w)) / 2.0;
            if i == 5 {
                bell(a, x0 + 30.0, cy, 34.0, clock);
            } else {
                a.mini.pose = crate::eyes::JAM_POSES[(clock / 3.0) as usize % 3];
                mini_eyes(a, x0 + 30.0, cy, 0.085);
                jam_notes(a, x0 + 30.0, 24.0, h);
            }
            let tx = x0 + ICON + GAP;
            a.gfx.ui(title, Rc::new(tx, cy - 17.6, text_w + 8.0, 20.0), 15.0, 700, WHITE, 1.0, Align::L);
            a.gfx.ui(value, Rc::new(tx, cy + 2.4, text_w + 8.0, 16.0), 11.5, 400, 0xA0A0A0, 1.0, Align::L);
            a.loop_rect = Some(Rc::new(x0 - 6.0, 0.0, ICON + 12.0, h));
        }
        ToastKind::Message => {
            // iPhone lock-screen style: app icon, sender + "now", then two lines of message.
            let k = (age / 0.35).clamp(0.0, 1.0);
            let drop = -10.0 * (1.0 - k).powi(3);
            let off = a.gfx.off;
            a.gfx.offset(off.0, off.1 + drop);
            let ix = left + 34.0;
            let info = a.notice.clone();
            let phone = info.as_ref().and_then(|n| n.phone_app.clone());
            match info.as_ref().filter(|_| phone.is_some()) {
                Some(n) => {
                    let accent = a.notice_accent(n);
                    a.gfx.glow(ix, h / 2.0, 30.0, accent, 0.28);
                    notice_badge(a, n, ix, h / 2.0, 19.0);
                }
                // Only a Facebook chat gets the chat bubble; anything else shows its own app.
                None if info.as_ref().is_some_and(|n| n.kind == crate::NoticeKind::Chat) => icon::messenger(&a.gfx, ix, h / 2.0, 19.0, 1.0),
                None => {
                    let aumid = info.as_ref().map(|n| n.aumid.clone()).unwrap_or_default();
                    let bmp = (!aumid.is_empty()).then(|| std::path::PathBuf::from(format!("shell:AppsFolder\\{aumid}"))).and_then(|p| a.shelf_icon(&p).0);
                    match bmp {
                        Some(b) => a.gfx.image_fit(&b, Rc::new(ix - 19.0, h / 2.0 - 19.0, 38.0, 38.0), 9.0, 1.0, false),
                        None => icon::bell(&a.gfx, ix, h / 2.0, WHITE, 1.0),
                    }
                }
            }
            let tx = left + 64.0;
            let tw = w - 64.0 - 22.0;
            // Sender and message are centred together on the card, one line or two; a phone
            // notification names its app above them.
            let title = notice_text(a, title);
            let body_h = a.gfx.measure_wrapped(title, 13.0, 400, tw).clamp(17.0, 34.0);
            let app_h = if phone.is_some() { 14.0 } else { 0.0 };
            // Optical centre: glyphs sit low in their line boxes.
            let top = ((h - (app_h + 20.0 + 2.0 + body_h)) / 2.0 - 1.5).max(3.0) + app_h;
            match &phone {
                Some(app) => {
                    a.gfx.ui(app, Rc::new(tx, top - 14.0, tw - 40.0, 14.0), 10.5, 600, 0xA0A0A5, 1.0, Align::L);
                    a.gfx.ui("now", Rc::new(left + w - 22.0 - 40.0, top - 14.0, 40.0, 14.0), 10.5, 400, 0x8E8E93, 1.0, Align::R);
                    a.gfx.ui(value, Rc::new(tx, top, tw, 20.0), 15.0, 700, WHITE, 1.0, Align::L);
                }
                None => {
                    a.gfx.ui(value, Rc::new(tx, top, tw - 40.0, 20.0), 15.0, 700, WHITE, 1.0, Align::L);
                    a.gfx.ui("now", Rc::new(left + w - 22.0 - 40.0, top + 1.0, 40.0, 18.0), 11.5, 400, 0x8E8E93, 1.0, Align::R);
                }
            }
            let by = top + 22.0;
            a.gfx.push_clip(Rc::new(tx, by, tw, body_h + 2.0));
            a.gfx.text(title, Rc::new(tx, by, tw, 40.0), 13.0, 400, 0xE5E5EA, 1.0, Align::LW, Font::Ui);
            a.gfx.pop_clip();
            a.gfx.offset(off.0, off.1);
        }
        ToastKind::Bluetooth(on) => {
            let lv = t.level.unwrap_or(0.0) * up;
            // A nearly flat battery (mice especially) is said out loud and turns the ring red.
            let low = on && t.level.is_some_and(|l| l <= 0.2);
            let sub = match t.level {
                Some(_) if low => format!("Low battery · {}%", (lv * 100.0).round()),
                Some(_) => format!("{value} · {}%", (lv * 100.0).round()),
                None => value.to_string(),
            };
            let kind = match a.device_kind {
                DeviceKind::Other => DeviceKind::Headphones,
                k => k,
            };
            // With a known battery the card takes its colour; without one it stays Bluetooth blue.
            let (accent, dark) = match t.level {
                Some(_) if on => battery_tone(lv),
                _ if on => (0x56C1F5, 0x0D2C44),
                _ => (0x6B6B6B, 0x222222),
            };
            let c = Card {
                accent,
                dark,
                badge: Glyph::Bluetooth,
                shake: if low { 0.18 } else { 0.0 },
                ring: Some((Glyph::Device(kind), if on { lv } else { 0.0 })),
                reserve: 0.0,
            };
            card(a, left, w, h, t, title, &sub, &c);
        }
        ToastKind::Device(on) => {
            let drive = on && a.device_root.is_some();
            let b = buttons(0.0, 64.0).1;
            let c = Card {
                accent: if on { GREEN } else { 0x6B6B6B },
                dark: if on { 0x0C2E14 } else { 0x222222 },
                badge: Glyph::Usb,
                shake: 0.0,
                ring: if drive { None } else { Some((Glyph::Device(a.device_kind), if on { up } else { 0.0 })) },
                reserve: if drive { b.w + 14.0 } else { 0.0 },
            };
            card(a, left, w, h, t, title, value, &c);
            if drive {
                card_button(a, b, "Open", 0x1E50C8, 0xDCE8FF, live, Act::DeviceOpen);
            }
        }
        ToastKind::Net(on) => {
            let bars = t.level.unwrap_or(0.8);
            let c = Card {
                accent: if on { BLUE } else { 0x6B6B6B },
                dark: if on { 0x0D2C44 } else { 0x222222 },
                badge: Glyph::Wifi(if on { 3.0 * up } else { 3.0 }, on),
                shake: 0.0,
                ring: Some((Glyph::Bars(if on { 4.0 * bars * up } else { 0.0 }), if on { bars * up } else { 0.0 })),
                reserve: 0.0,
            };
            card(a, left, w, h, t, title, value, &c);
            if !on {
                // A slash draws across the Wi-Fi glyph.
                let (bx, cy) = (left + 32.0, h / 2.0);
                let d = 6.4 * up;
                a.gfx.line(bx - 6.4, cy - 6.4, bx - 6.4 + 2.0 * d, cy - 6.4 + 2.0 * d, 1.8, 0xD0D0D0, 1.0);
            }
        }
        ToastKind::Charging(_) | ToastKind::Battery => {
            let charging = matches!(kind, ToastKind::Charging(true));
            let low = kind == ToastKind::Battery;
            let lv = t.level.unwrap_or(1.0).clamp(0.0, 1.0);
            let shown = lv * up;
            // "Charger connected · 94%", counting up like the Bluetooth battery.
            let sub = if value.contains('%') { value.to_string() } else { format!("{value} · {}%", (shown * 100.0).round()) };
            // Charging is always green; on battery the card shows how much is left.
            let (accent, dark) = if low {
                (RED, 0x3A0E10)
            } else if charging {
                (GREEN, 0x0C2E14)
            } else {
                battery_tone(shown)
            };
            let c = Card {
                accent,
                dark,
                badge: if charging { Glyph::Bolt } else { Glyph::Battery },
                shake: if low { 0.18 } else { 0.0 },
                ring: Some((if charging { Glyph::Bolt } else { Glyph::Battery }, shown)),
                reserve: 0.0,
            };
            card(a, left, w, h, t, title, &sub, &c);
        }
        ToastKind::Goal => {
            let c = Card { accent: CYAN, dark: 0x0B2A33, badge: Glyph::Drop, shake: 0.0, ring: Some((Glyph::Check(up), up)), reserve: 0.0 };
            card(a, left, w, h, t, title, value, &c);
        }
        ToastKind::Focus | ToastKind::Break => {
            let focus = kind == ToastKind::Focus;
            // A time's-up card offers a few more minutes; a phase notice skips the break or
            // pauses the round.
            let (label, mark) = if a.focus_end_card() {
                ("+5 min", Mark::Plus)
            } else if focus {
                ("Pause", Mark::Pause)
            } else {
                ("Skip", Mark::Skip)
            };
            plain_card(a, left, w, h, title, value, |_, _, _| {});
            pic(a, "ic_focus", left + 104.0, h / 2.0, 26.0, WHITE, 1.0);
            card_marks(a, left, w, h, t, live, &[(Mark::Cross, "Close", Act::ToastDismiss), (mark, label, Act::ToastStartPhase)]);
        }
        ToastKind::Task => {
            plain_card(a, left, w, h, title, value, |g, x, y| icon::bell(g, x, y, WHITE, 1.0));
            card_marks(a, left, w, h, t, live, &[(Mark::Later, "Later", Act::LumaLater), (Mark::Play, "Start", Act::LumaDone)]);
        }
        ToastKind::Remind(k) => {
            use crate::luma::config::Kind as K;
            let sleep = matches!(k, K::Sleep | K::Water);
            if let Some((act, e)) = a.act_now() {
                act_card(a, left, w, h, title, value, act, e);
                let base = a.gfx.alpha;
                let shown = crate::eyes::act(act, e).buttons;
                a.gfx.alpha = base * shown;
                card_marks(a, left, w, h, t, live && shown > 0.6 && a.reaction.is_none(), &[(Mark::Later, "Later", Act::LumaLater), if sleep { (Mark::Tick(WHITE), "Sleep", Act::LumaDone) } else { (Mark::Play, "Start", Act::GuideStart) }]);
                a.gfx.alpha = base;
            }
        }
        ToastKind::HeadsUp => {
            // Counts down to the card.
            let secs_left = a.brain.showing.as_ref().and_then(|s| s.heads_up_until).map(|u| (u - chrono::Local::now()).num_milliseconds().max(0) as f32 / 1000.0).unwrap_or(0.0);
            plain_card(a, left, w, h, title, &format!("Coming up in {}s", secs_left.ceil() as u32), |g, x, y| icon::bell(g, x, y, WHITE, 1.0));
            card_marks(a, left, w, h, t, live, &[(Mark::Later, "Later", Act::LumaLater), (Mark::Play, "Now", Act::LumaNow)]);
        }
        ToastKind::LumaApp => {
            plain_card(a, left, w, h, title, value, |g, x, y| icon::bell(g, x, y, WHITE, 1.0));
            card_marks(a, left, w, h, t, live, &[(Mark::Later, "Later", Act::ToastDismiss), (Mark::Cross, "Turn off", Act::LumaAppOff)]);
        }
        ToastKind::Shutdown => {
            let left_s = a.shutdown_at.map_or(0, |at| at.saturating_duration_since(std::time::Instant::now()).as_secs() + 1);
            if let Some((act, e)) = a.act_now() {
                act_card(a, left, w, h, &format!("Shutting down in {left_s} s"), title, act, e);
            }
            card_marks(a, left, w, h, t, live, &[(Mark::Cross, "Cancel", Act::ShutdownCancel)]);
        }
        ToastKind::Water => {
            if let Some((act, e)) = a.act_now() {
                act_card(a, left, w, h, title, value, act, e);
                let base = a.gfx.alpha;
                let shown = crate::eyes::act(act, e).buttons;
                a.gfx.alpha = base * shown;
                card_marks(a, left, w, h, t, live && shown > 0.6 && a.reaction.is_none(), &[(Mark::Later, "Later", Act::LumaLater), (Mark::Tick(WATER), "Drank", Act::LumaDone)]);
                a.gfx.alpha = base;
            }
        }
        _ => {
            // App notification: the app's icon on its own colour, sender over message.
            let info = a.notice.clone().filter(|_| live);
            let accent = info.as_ref().map(|n| a.notice_accent(n)).unwrap_or(0xF5B833);
            let has_icon = info.as_ref().and_then(|n| n.icon.as_ref()).is_some_and(|p| a.shelf_icon(p).0.is_some());
            let c = Card { accent, dark: mix(0x000000, accent, 0.18), badge: if has_icon { Glyph::Blank } else { Glyph::Bell }, shake: if has_icon { 0.0 } else { 0.32 }, ring: None, reserve: 14.0 };
            card(a, left, w, h, t, value, notice_text(a, title), &c);
            if let Some(n) = info.filter(|_| has_icon) {
                notice_badge(a, &n, left + 32.0, h / 2.0, 10.0);
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Mark {
    Later,
    Tick(u32),
    Play,
    Plus,
    Pause,
    Skip,
    Cross,
}

/// A card that wants an answer without an act: its icon, a line, the words.
fn plain_card(a: &mut App, left: f32, w: f32, h: f32, title: &str, value: &str, draw: impl FnOnce(&Gfx, f32, f32)) {
    let cy = h / 2.0;
    act_eyes(a, left + 56.0, cy, 0.13, 0.0);
    draw(&a.gfx, left + 104.0, cy);
    divider(a, left + 132.0, cy - 20.0, cy + 20.0);
    let tw = w - 292.0;
    a.gfx.ui(title, Rc::new(left + 146.0, cy - 21.0, tw, 22.0), 15.0, 600, WHITE, 1.0, Align::L);
    a.gfx.ui(value, Rc::new(left + 146.0, cy + 1.0, tw, 18.0), SMALL, 400, 0xA0A0A5, 1.0, Align::L);
}

/// The same round answers on every card, right-aligned. On the strict care level "Later" only
/// appears after a few seconds.
fn card_marks(a: &mut App, left: f32, w: f32, h: f32, t: &crate::Toast, live: bool, marks: &[(Mark, &str, Act)]) {
    let cy = h / 2.0 - 9.0;
    for (i, (mark, label, act)) in marks.iter().rev().enumerate() {
        let cx = left + w - 46.0 - 66.0 * i as f32;
        let wait = a.brain.skip_after() as f32 - t.born.elapsed().as_secs_f32();
        if matches!(mark, Mark::Later) && wait > 0.0 {
            let left_s = wait.ceil() as u32;
            let text = format!("{left_s}s");
            labelled(a, cx, cy, label, false, *act, |_, _, _| {});
            a.gfx.ui(&text, Rc::new(cx - 18.0, cy - 18.0, 36.0, 36.0), SMALL, 600, 0x8A8A8A, 1.0, Align::C);
            continue;
        }
        let mark = *mark;
        labelled(a, cx, cy, label, live, *act, |g, x, y| match mark {
            Mark::Later => {
                g.stroke_circle(x, y, 8.0, 1.5, WHITE, 1.0);
                g.polyline(&[(x, y - 4.8), (x, y), (x + 4.0, y + 2.4)], 1.5, WHITE, 1.0);
            }
            Mark::Tick(hex) => icon::tick(g, x, y, hex, 1.0),
            Mark::Play => icon::play(g, x + 1.0, y, 11.0, WHITE, 1.0),
            Mark::Plus => icon::plus(g, x, y, 6.0, 1.9, WHITE, 1.0),
            Mark::Pause => icon::pause(g, x, y, 11.0, WHITE, 1.0),
            Mark::Skip => icon::skip(g, x, y, 10.0, true, WHITE, 1.0),
            Mark::Cross => icon::cross(g, x, y, 5.0, WHITE, 1.0),
        });
    }
}

/// Alarm-style shake: bursts of a fast decaying wobble every 1.6 s, amplitude `amp` radians.
fn ring_angle(age: f32, amp: f32) -> f32 {
    let p = age % 1.6;
    if p > 0.7 {
        return 0.0;
    }
    amp * (-p * 5.0).exp() * (p * 38.0).sin()
}


// ---------------------------------------------------------------- expanded

/// An icon at its true size in pixels, so it stays sharp.
fn pic(a: &mut App, name: &'static str, cx: f32, cy: f32, s: f32, tint: u32, alpha: f32) {
    let px = (s * a.gfx.scale).round().max(4.0) as u32;
    if let Some(b) = a.pic(name, px, tint) {
        a.gfx.image(&b, Rc::new(cx - s / 2.0, cy - s / 2.0, s, s), 0.0, alpha);
    }
}

/// The top row, the same everywhere; each thing has its own slot, so nothing can overlap.
/// Where the clock and date start, centred in their slot.
fn clock_at(a: &mut App, lx: f32) -> (f32, f32, f32) {
    let (time, date) = (a.clock().time_label(), short_date());
    let (tw, dw) = (a.gfx.measure(&time, HEAD, 600), a.gfx.measure(&date, SMALL, 400));
    (lx + (SLOT_CLOCK.0 + SLOT_CLOCK.1) / 2.0 - (tw + 8.0 + dw) / 2.0, tw, dw)
}

/// The Clear chip, halfway between the Shelf title and what stands left of the clock.
fn clear_chip(a: &mut App, lx: f32) -> Rc {
    let title_end = shelf_title_end(a, lx);
    let clock = clock_at(a, lx).0 - if a.power.is_some_and(|p| p.battery && p.ac) { 13.0 + 7.6 } else { 0.0 };
    Rc::new((title_end + clock) / 2.0 - 20.0, 10.2, 40.0, 17.6)
}

fn header(a: &mut App, lx: f32) {
    let time = a.clock().time_label();
    let date = short_date();
    let (x, tw, dw) = clock_at(a, lx);
    a.gfx.ui(&time, Rc::new(x, 8.0, tw + 2.0, 22.0), HEAD, 600, WHITE, 1.0, Align::L);
    a.gfx.ui(&date, Rc::new(x + tw + 8.0, 8.0, dw + 2.0, 22.0), SMALL, 400, 0xA0A0A5, 1.0, Align::L);
    if a.view == View::History {
        return;
    }
    if a.power.is_some_and(|p| p.battery && p.ac) {
        icon::charging(&a.gfx, x - 13.0, 19.0, 7.6, a.charge_level(), 1.0);
    }
    right_indicator(a, lx + SLOT_LIVE.0, 19.0);
    if !a.history.is_empty() && a.view != View::Brief {
        let bell = Rc::new(lx + SLOT_BELL.0, 7.0, SLOT_BELL.1 - SLOT_BELL.0, 24.0);
        let h = btn(a, bell, 0.15);
        a.gfx.fill_circle(bell.cx(), bell.cy(), 12.0, 12.0, 0xFFFFFF, 0.1 + 0.12 * h);
        icon::bell(&a.gfx, bell.cx(), bell.cy() - 0.5, WHITE, 1.0);
        if a.history_unread > 0 {
            let n = a.history_unread.min(99).to_string();
            let w = (a.gfx.measure(&n, 8.5, 700) + 6.0).max(11.0);
            let dot = Rc::new(bell.cx() + 4.0, bell.y - 2.0, w, 11.0);
            a.gfx.fill_rrect(dot, 5.5, RED, 1.0);
            a.gfx.ui(&n, dot, 8.5, 700, WHITE, 1.0, Align::C);
        }
        a.gfx.unzoom();
        hit(a, bell, Act::History(true));
    }
}

fn short_date() -> String {
    chrono::Local::now().format("%a %-d %b").to_string()
}

/// A round button in the header row with a line icon: the back button and its kin.
/// Light liquid glass on `r`: a clear fill that lets the colour behind show, a sheen from the
/// top, and a rim lit from the top left that fades toward the bottom right.
fn glass(g: &Gfx, r: Rc, rad: f32, h: f32) {
    let h = h.clamp(0.0, 1.5);
    g.fill_rrect(r, rad, WHITE, 0.10 + 0.06 * h);
    g.fill_rrect_vgrad(r, rad, (WHITE, 0.08), (WHITE, 0.0));
    let rim = Rc::new(r.x + 0.5, r.y + 0.5, r.w - 1.0, r.h - 1.0);
    g.stroke_rrect_diag(rim, (rad - 0.5).max(0.0), 1.0, (WHITE, 0.42 + 0.2 * h.min(1.0)), (WHITE, 0.05));
}

/// A liquid glass button (after Kyant0's LiquidButton): the screen behind bent at its rim, then a
/// clear white surface (or the page's colour for a main button), and light caught at its rim;
/// brighter while pressed. Without the screen behind (glass not open) it is the light glass.
fn liquid(a: &mut App, r: Rc, rad: f32, h: f32, tint: Option<u32>) {
    let press = ((h - 1.0) / 0.8).clamp(0.0, 1.0);
    let s = a.gfx.scale;
    let bent = a.glass_frame.clone().is_some_and(|(strip, at, k)| {
        let (w, hh, rr) = ((r.w * s).round() as u32, (r.h * s).round() as u32, (rad * s).round() as u32);
        a.lens_map(w.max(4), hh.max(4), rr).is_some_and(|map| a.gfx.liquid_lens(&strip, at, k, r, rad, &map, r.h * s * 0.33))
    });
    if !bent {
        glass(&a.gfx, r, rad, h);
        return;
    }
    match tint {
        Some(c) => a.gfx.fill_rrect(r, rad, c, 0.62 + 0.1 * press),
        None => a.gfx.fill_rrect(r, rad, WHITE, 0.10 + 0.06 * h.clamp(0.0, 1.0)),
    }
    a.gfx.fill_rrect(r, rad, WHITE, 0.08 * press);
    let rim = Rc::new(r.x + 0.4, r.y + 0.4, r.w - 0.8, r.h - 0.8);
    a.gfx.stroke_rrect_lit(rim, (rad - 0.4).max(0.0), 0.8, 0.5, 0.04);
}

fn head_button(a: &mut App, r: Rc, act: Act, draw: impl FnOnce(&Gfx, f32, f32)) {
    let h = btn(a, r, 0.15);
    if a.settings.glass {
        liquid(a, r, r.w / 2.0, h, None);
    } else {
        a.gfx.fill_circle(r.cx(), r.cy(), r.w / 2.0, r.w / 2.0, 0xFFFFFF, 0.1 + 0.12 * h);
    }
    draw(&a.gfx, r.cx(), r.cy());
    a.gfx.unzoom();
    hit(a, r, act);
}

/// How far down a list is, on its right edge, whenever there is more than fits.
fn scrollbar(a: &mut App, lx: f32, top: usize, rows: usize) {
    if rows <= LIST_ROWS {
        return;
    }
    const TRACK: (f32, f32) = (44.0, 76.0);
    let x = lx + 390.0;
    a.gfx.fill_rrect(Rc::new(x, TRACK.0, 3.0, TRACK.1), 1.5, WHITE, 0.1);
    let h = TRACK.1 * LIST_ROWS as f32 / rows as f32;
    let at = top.min(rows - LIST_ROWS) as f32 / (rows - LIST_ROWS) as f32;
    a.gfx.fill_rrect(Rc::new(x, TRACK.0 + (TRACK.1 - h) * at, 3.0, h), 1.5, WHITE, 0.6);
}

/// The arrows only come out when the pointer nears the edge, so they never sit on the content.
fn chevrons(a: &mut App, lx: f32) {
    let show = a.chev_mix.x.clamp(0.0, 1.0);
    if show < 0.02 {
        return;
    }
    for (x, left, act) in [(lx + 14.0, true, Act::PagePrev), (lx + 406.0, false, Act::PageNext)] {
        let r = Rc::new(x - 12.0, 58.0, 24.0, 24.0);
        let h = btn(a, r, 0.18);
        a.gfx.fill_circle(x, 70.0, 10.0, 10.0, mix(CHEVRON, 0x8A8A8A, h), show);
        icon::chevron(&a.gfx, x, 70.0, left, 0x000000, show);
        a.gfx.unzoom();
        hit(a, r, act);
    }
}

fn dots(a: &mut App, lx: f32) {
    let n = Page::ALL.len();
    // The active pill glides between dots: each dot's width and brightness follow its distance
    // from the eased page position.
    let pos = a.strip.x.rem_euclid(n as f32);
    let (aw, dw, gap) = (14.4, 4.0, 4.0);
    let total = aw + dw * (n - 1) as f32 + gap * (n - 1) as f32;
    let mut x = lx + 210.0 - total / 2.0;
    for i in 0..n {
        let d = (i as f32 - pos).abs();
        let w8 = (1.0 - d.min(n as f32 - d)).clamp(0.0, 1.0);
        let wdt = dw + (aw - dw) * w8;
        let r = Rc::new(x - 2.0, 126.0, wdt + 4.0, 14.0);
        let h = btn(a, r, 0.25);
        a.gfx.fill_rrect(Rc::new(x, 131.2, wdt, 4.0), 2.0, mix(mix(0x5C5C5C, 0x9A9A9A, h.max(0.0)), WHITE, w8), 1.0);
        a.gfx.unzoom();
        hit(a, r, Act::Page(Page::ALL[i]));
        x += wdt + gap;
    }
}

fn title(a: &mut App, lx: f32, text: &str) {
    let (name, tint) = match text {
        "Today" => ("t_today", RED),
        "Focus" => ("t_focus", CORAL),
        "Care" => ("t_care", WATER),
        "Reminders" => ("t_todo", REMIND),
        _ => ("t_shelf", WHITE),
    };
    // The header row holds still while pages change: the two names fade into each other there.
    let (keep, alpha) = (a.gfx.off, a.gfx.alpha);
    a.gfx.offset(a.title_at.0, keep.1);
    a.gfx.alpha = a.title_at.1;
    pic(a, name, lx + 33.0, 19.0, 18.0, tint, 1.0);
    a.gfx.ui(text, Rc::new(lx + 49.0, 8.0, 100.0, 22.0), HEAD, 600, WHITE, 1.0, Align::L);
    a.gfx.offset(keep.0, keep.1);
    a.gfx.alpha = alpha;
}

/// Eases towards `on`, each `key` keeping its own motion.
fn flip(a: &mut App, key: i32, on: bool) -> f32 {
    let entry = a.switches.entry((2, key)).or_insert((crate::Spring::new(if on { 1.0 } else { 0.0 }, 220.0, 26.0), on));
    let changed = entry.1 != on;
    entry.1 = on;
    let k = entry.0.x.clamp(0.0, 1.0);
    if changed {
        a.kick();
    }
    k
}


/// Notification history: newest first, three rows at a time, the wheel scrolls.
fn history(a: &mut App, lx: f32) {
    back_title(a, lx, "Notifications", Act::History(false));
    let clear = Rc::new(lx + SLOT_CLEAR.0, 7.0, SLOT_CLEAR.1 - SLOT_CLEAR.0, 24.0);
    let h = btn(a, clear, 0.05);
    a.gfx.fill_rrect(clear, 12.0, mix(0x242426, 0x3A3A3C, h), 1.0);
    a.gfx.ui("Clear all", clear, SMALL, 500, WHITE, 1.0, Align::C);
    a.gfx.unzoom();
    hit(a, clear, Act::HistoryClear);
    let pos = a.list_pos.x;
    let first = pos.floor().max(0.0) as usize;
    let rows: Vec<_> = a.history.iter().cloned().enumerate().skip(first).take(LIST_ROWS + 1).collect();
    a.gfx.push_clip(Rc::new(lx, 38.0, 420.0, 92.0));
    for (i, n) in rows {
        let y = 42.0 + (i as f32 - pos) * 42.0;
        let r = Rc::new(lx + 20.0, y, 362.0, 38.0);
        let h = btn(a, r, 0.01);
        if h > 0.0 {
            a.gfx.fill_rrect(r, 12.0, 0x222222, h.min(1.0));
        }
        notice_badge(a, &n, lx + 42.0, y + 19.0, 14.0);
        let tx = lx + 66.0;
        let secs = n.born.elapsed().as_secs();
        let age = if secs < 60 { "now".to_string() } else if secs < 3600 { format!("{} min", secs / 60) } else { format!("{} h", secs / 3600) };
        a.gfx.ui(&age, Rc::new(lx + 316.0, y + 3.0, 62.0, 16.0), 11.0, 400, 0x8E8E93, 1.0, Align::R);
        a.gfx.ui(&n.title, Rc::new(tx, y + 1.0, 240.0, 18.0), 13.0, 600, WHITE, 1.0, Align::L);
        let sub = if n.body.is_empty() { n.source_name() } else { n.body.clone() };
        a.gfx.ui(&sub, Rc::new(tx, y + 19.0, 310.0, 16.0), SMALL, 400, 0xA0A0A5, 1.0, Align::L);
        a.gfx.unzoom();
        hit(a, r, Act::HistoryOpen(i));
    }
    a.gfx.pop_clip();
    let (top, total) = (a.history_top, a.history.len());
    scrollbar(a, lx, top, total);
    if a.history.is_empty() {
        a.gfx.ui("No notifications", Rc::new(lx, 72.0, 420.0, 18.0), 13.0, 400, 0x6D6D6D, 1.0, Align::C);
    }
}

/// A notification's badge: the app's real icon, else its initial on its colour.
fn notice_badge(a: &mut App, n: &crate::NoticeInfo, cx: f32, cy: f32, r: f32) {
    let rect = Rc::new(cx - r, cy - r, 2.0 * r, 2.0 * r);
    if n.kind == crate::NoticeKind::Chat {
        icon::messenger(&a.gfx, cx, cy, r, 1.0);
    } else if let Some(b) = n.icon.as_ref().and_then(|p| a.shelf_icon(p).0) {
        a.gfx.image_fit(&b, rect, r * 0.45, 1.0, false);
    } else {
        a.gfx.fill_rrect(rect, r * 0.45, a.notice_accent(n), 1.0);
        let letter: String = n.source_name().chars().take(1).collect::<String>().to_uppercase();
        a.gfx.ui(&letter, rect, r * 1.1, 700, WHITE, 1.0, Align::C);
    }
}

fn expanded(a: &mut App, lx: f32) {
    let lyr = if a.page == Page::Music && a.view == View::Page { a.lyr_mix.x.clamp(0.0, 1.0) } else { 0.0 };
    let base = a.gfx.alpha;
    a.gfx.alpha = base * (1.0 - lyr);
    if a.gfx.alpha > 0.01 {
        header(a, lx);
    }
    a.gfx.alpha = base;
    // The view being left fades one way as the new one comes in the other.
    let s = a.view_mix.x.clamp(0.0, 1.0);
    let dir = a.view_dir;
    if s > 0.01 {
        let n = a.hits.len();
        a.gfx.alpha = base * s;
        a.gfx.offset(-dir * 24.0 * (1.0 - s), 0.0);
        let from = a.view_from;
        body(a, lx, from);
        a.hits.truncate(n);
    }
    a.gfx.alpha = base * (1.0 - s);
    a.gfx.offset(dir * 24.0 * s, 0.0);
    let view = a.view;
    body(a, lx, view);
    a.gfx.offset(0.0, 0.0);
    a.gfx.alpha = base;
    // The open pages have moving parts of their own: a frame always redraws all of it.
    a.loop_rect = None;
    // While the Focus wheel is out, no page arrows to hit by mistake (as on the setting screens).
    if view == View::Page && lyr < 0.99 && !a.focus_pick {
        a.gfx.alpha = base * (1.0 - lyr) * (1.0 - s);
        chevrons(a, lx);
        dots(a, lx);
        a.gfx.alpha = base;
    }
}

fn body(a: &mut App, lx: f32, view: View) {
    match view {
        View::Page => pages(a, lx),
        View::History => history(a, lx),
        View::OldNotes => old_notes(a, lx),
        View::Form => {
            if a.form.is_some() {
                form(a, lx);
            }
        }
        View::Care(tile) => care_detail(a, lx, tile),
        View::Goal => goal(a, lx),
        View::Bed => bed(a, lx),
        View::Calendar => calendar(a, lx),
        View::Guide => guide_page(a, lx),
        View::Brief => brief(a, lx),
    }
}

/// The pages lie side by side like one long picture; the notch looks at where the strip stands.
fn pages(a: &mut App, lx: f32) {
    let at = a.strip.x;
    let (ox, _) = a.gfx.off;
    let base = a.gfx.alpha;
    let n = Page::ALL.len() as i32;
    let first = at.floor() as i32;
    let settled = (at - at.round()).abs() < 0.04;
    for i in first..=first + 1 {
        let d = i as f32 - at;
        // Each page slides a short way and fades, the one leaving gone before the next is clear.
        let shown = (1.0 - d.abs() * PAGE_FADE).max(0.0);
        if d.abs() >= 1.0 || shown < 0.01 {
            continue;
        }
        let p = Page::ALL[i.rem_euclid(n) as usize];
        let hits = a.hits.len();
        a.gfx.offset(ox + d * PAGE_SLIDE, 0.0);
        a.gfx.alpha = base * shown;
        a.title_at = (ox, base * (1.0 - d.abs()));
        page(a, p, lx);
        // Buttons are only live on the page at rest in front.
        if !(settled && p == a.page) {
            a.hits.truncate(hits);
        }
    }
    a.gfx.alpha = base;
    a.gfx.offset(ox, 0.0);
    a.title_at = (ox, base);
}

/// How far a page slides as it changes, and how quickly it fades.
const PAGE_SLIDE: f32 = 56.0;
const PAGE_FADE: f32 = 1.7;

fn page(a: &mut App, p: Page, lx: f32) {
    match p {
        Page::Today => today(a, lx),
        Page::Music => music(a, lx),
        Page::Pomo => pomo(a, lx),
        Page::Water => care(a, lx),
        Page::Shelf => shelf(a, lx),
        Page::Todo => todo(a, lx),
    }
}

// ---------------------------------------------------------------- music

fn transport(a: &mut App, quote: (f32, f32), xs: [f32; 3], cy: f32, playing: bool, lyrics_on: bool) {
    let live = {
        let m = a.media.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        m.has && !m.title.is_empty()
    };
    let qr = Rc::new(quote.0 - 13.0, quote.1 - 12.0, 26.0, 24.0);
    let h = if live { btn(a, qr, 0.18) } else { 0.0 };
    a.gfx.fill_circle(quote.0, quote.1, 13.0, 13.0, WHITE, 0.10 * h.max(0.0));
    icon::quote(&a.gfx, quote.0, quote.1, if lyrics_on { WHITE } else { mix(0xBFBFBF, WHITE, h) }, 1.0);
    a.gfx.unzoom();
    hit(a, qr, Act::Lyrics);
    let rects = [
        Rc::new(xs[0] - 15.0, cy - 13.0, 30.0, 26.0),
        Rc::new(xs[1] - 15.0, cy - 14.0, 30.0, 28.0),
        Rc::new(xs[2] - 15.0, cy - 13.0, 30.0, 26.0),
    ];
    for (i, (r, act)) in rects.into_iter().zip([Act::Prev, Act::Toggle, Act::Next]).enumerate() {
        let h = if live { btn(a, r, 0.18) } else { 0.0 };
        a.gfx.fill_circle(xs[i], cy, 14.0, 14.0, WHITE, 0.10 * h.max(0.0));
        match i {
            0 => icon::skip(&a.gfx, xs[0], cy, 18.4, false, WHITE, 1.0),
            1 if playing => icon::pause(&a.gfx, xs[1], cy, 16.0, WHITE, 1.0),
            1 => icon::play(&a.gfx, xs[1], cy, 17.0, WHITE, 1.0),
            _ => icon::skip(&a.gfx, xs[2], cy, 18.4, true, WHITE, 1.0),
        }
        a.gfx.unzoom();
        hit(a, r, act);
    }
}

fn art(a: &mut App, r: Rc) {
    if let Some((_, bmp, _)) = a.art.clone() {
        // Liquid glass: the cover as a piece of thick glass, its rim bending the picture, lit from
        // the top left.
        let size = unsafe { bmp.GetSize() };
        let (w, h) = (size.width.round() as u32, size.height.round() as u32);
        let glassy = a.settings.glass && w > 8 && h > 8 && {
            let amount = w.min(h) as f32 * 0.06;
            a.glass_map(w, h).is_some_and(|map| a.gfx.image_glass(&bmp, &map, r, 6.0, amount, 1.0))
        };
        if glassy {
            let rim = Rc::new(r.x + 0.5, r.y + 0.5, r.w - 1.0, r.h - 1.0);
            a.gfx.stroke_rrect_diag(rim, 5.5, 1.2, (WHITE, 0.55), (WHITE, 0.06));
        } else {
            a.gfx.image(&bmp, r, 6.0, 1.0);
        }
    } else {
        a.gfx.fill_rrect(r, 6.0, 0x2A2A2A, 1.0);
        icon::note(&a.gfx, r.cx(), r.cy(), 0x6A6A6A, 1.0);
    }
}

fn progress(a: &mut App, pos: f64, dur: f64, x0: f32, bar: (f32, f32), x1: f32, y: f32) {
    a.gfx.ui(&secs_label(pos), Rc::new(x0, y - 10.5, 60.0, 21.0), 17.0, 400, WHITE, 1.0, Align::L);
    let remain = format!("-{}", secs_label(dur - pos));
    a.gfx.ui(&remain, Rc::new(x1 - 70.0, y - 10.5, 70.0, 21.0), 17.0, 400, WHITE, 1.0, Align::R);
    let (bx, bw) = bar;
    // The bar thickens under the cursor and follows the drag while scrubbing.
    let zone = Rc::new(bx - 4.0, y - 10.0, bw + 8.0, 16.0);
    let scrubbing = a.scrub.filter(|s| (s.0 - bx).abs() < 0.5);
    let h = if dur > 0.0 { btn(a, zone, 0.0).max(0.0) } else { 0.0 }.max(if scrubbing.is_some() { 1.0 } else { 0.0 });
    a.gfx.unzoom();
    let th = 3.2 + 0.8 * h;
    let cy = y - 0.8;
    a.gfx.fill_rrect(Rc::new(bx, cy - th / 2.0, bw, th), th / 2.0, 0x3A3A3A, 1.0);
    let _ = scrubbing;
    let frac = if dur > 0.0 { a.tl_frac.x.clamp(0.0, 1.0) } else { 0.0 };
    let _ = pos;
    a.gfx.fill_rrect(Rc::new(bx, cy - th / 2.0, (bw * frac).max(th), th), th / 2.0, WHITE, 1.0);
    if dur > 0.0 {
        hit(a, zone, Act::Scrub((bx * 10.0) as i32, (bw * 10.0) as i32));
    }
}

fn music(a: &mut App, lx: f32) {
    let m = a.media.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    let k = a.lyr_mix.x.clamp(0.0, 1.0);
    let l = |x: f32, y: f32| x + (y - x) * k;
    let has = m.has && !m.title.is_empty();
    let ar = Rc::new(lx + l(44.4, 23.6), 12.0, 62.4, 62.4);
    if has {
        art(a, ar);
    } else {
        // No session: a placeholder tile and a disabled player, as Nusa shows it.
        a.gfx.fill_rrect(ar, 10.0, 0x2A2A2A, 1.0);
        a.gfx.zoom(ar.cx(), ar.cy(), 1.9);
        icon::note(&a.gfx, ar.cx(), ar.cy(), 0x8A8A8A, 1.0);
        a.gfx.unzoom();
    }
    let (title, artist) = if has { (m.title.clone(), m.artist.clone()) } else { ("No media".to_string(), String::new()) };
    let tw = l(258.0, 142.0);
    let ty = if has { l(36.4, 38.0) } else { 36.4 + 7.0 };
    a.gfx.ui(&title, Rc::new(lx + l(122.0, 101.2), ty, tw, 22.0), 15.0, 600, WHITE, 1.0, Align::L);
    a.gfx.ui(&artist, Rc::new(lx + l(122.0, 101.2), l(53.6, 56.0), tw, 16.0), 11.5, 400, 0x8E8E8E, 1.0, Align::L);
    // While scrubbing, the lyrics follow the dragged position.
    let pos = match a.scrub {
        Some((_, _, f)) if m.dur > 0.0 => f as f64 * m.dur,
        _ => m.position(),
    };
    let base = a.gfx.alpha;
    if !has {
        a.gfx.alpha = base * 0.4;
    }
    progress(a, m.position(), m.dur, lx + l(44.4, 26.0), (lx + l(96.4, 77.2), l(222.4, 107.2)), lx + l(376.4, 242.0), l(91.6, 92.4));
    let tty = l(118.0, 117.6);
    transport(a, (lx + l(74.8, 53.2), 117.6), [lx + l(162.4, 92.4), lx + l(210.0, 131.6), lx + l(257.2, 170.8)], tty, m.playing, a.lyrics_on);
    if !has {
        // Nothing to control: drop the transport's hit areas.
        a.hits.retain(|(_, act)| !matches!(act, Act::Prev | Act::Toggle | Act::Next | Act::Lyrics | Act::Scrub(..)));
    }
    a.gfx.alpha = base;
    if k > 0.01 {
        a.gfx.alpha = base * k;
        lyrics_column(a, lx, pos, k);
        a.gfx.alpha = base;
    }
}

/// The album art's average color, normalized so its brightest channel is 0x7A.
fn art_tint(a: &App) -> u32 {
    let avg = a.art.as_ref().map(|x| x.2).unwrap_or(0x707070);
    let (r, g, b) = ((avg >> 16) & 255, (avg >> 8) & 255, avg & 255);
    let max = r.max(g).max(b).max(1);
    let k = 0x7A as f32 / max as f32;
    (((r as f32 * k) as u32) << 16) | (((g as f32 * k) as u32) << 8) | ((b as f32 * k) as u32)
}

fn lyrics_backdrop(a: &mut App, left: f32, w: f32, h: f32, radius: f32, mix: f32) {
    let tint = art_tint(a);
    let alpha = a.fade.x.clamp(0.0, 1.0) * mix;
    a.gfx.fill_rrect_vgrad(Rc::new(left, h * 0.3, w, h * 0.7), radius, (tint, 0.0), (tint, 0.95 * alpha));
}

/// The colour rising from the bottom edge: only while something is opened, or for a moment after
/// a drink. Plain pages stay black.
fn ambient(a: &mut App, left: f32, w: f32, h: f32) {
    let open = a.fade.x.clamp(0.0, 1.0);
    let rise = Rc::new(left, h * 0.4, w, h * 0.6);
    let glow = a.glow_mix.x.clamp(0.0, 1.0) * open;
    if glow > 0.01 {
        a.gfx.fill_rrect_vgrad(rise, 0.0, (a.glow, 0.0), (a.glow, 0.42 * glow));
    }
    if a.page == Page::Water {
        let wash = a.wash.clamp(0.0, 1.0) * open;
        if wash > 0.01 {
            a.gfx.fill_rrect_vgrad(rise, 0.0, (WATER, 0.0), (WATER, 0.5 * wash));
        }
    }
    // A running focus breathes slowly along the bottom.
    let focus = a.focus_mix.x.clamp(0.0, 1.0) * open * (a.page == Page::Pomo && a.view == View::Page) as u8 as f32;
    if focus > 0.01 {
        let breath = 0.26 + 0.07 * (a.born.elapsed().as_secs_f32() * 0.9).sin();
        a.gfx.fill_rrect_vgrad(Rc::new(left, h * 0.55, w, h * 0.45), 0.0, (RED, 0.0), (RED, breath * focus));
    }
}

/// Critically damped ease (0 → 1) used by the lyric cascade.
fn settle(t: f32) -> f32 {
    if t <= 0.0 {
        return 0.0;
    }
    let w = 19.0;
    1.0 - (1.0 + w * t) * (-w * t).exp()
}

fn lyrics_column(a: &mut App, lx: f32, pos: f64, k: f32) {
    // Words that run early or late for this song are moved by what the user set.
    let off = a.lyric_offset();
    let pos = pos + off;
    a.gfx.line(lx + 253.2, 22.0, lx + 253.2, 124.0, 1.0, 0x5A5A5A, 0.5);
    // Laid out at the final lyrics width so wrapping never changes while the notch resizes.
    let col = Rc::new(lx + 265.2, 10.0, crate::LYRICS[0] - 265.2 - 22.0, 124.0);
    let rise = 30.0 * (1.0 - k);
    let (status, idx, lines, times, key) = {
        let l = a.lyrics.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if l.held {
            return;
        }
        let idx = l.lines.iter().rposition(|x| x.0 >= 0.0 && x.0 <= pos + 0.25);
        (l.status, idx, l.lines.iter().map(|x| x.1.clone()).collect::<Vec<_>>(), l.lines.iter().map(|x| x.0).collect::<Vec<_>>(), l.key.clone())
    };
    match status {
        LyrStatus::Ready if !lines.is_empty() => {
            const GAP: f32 = 7.0;
            // Text size eases with Ctrl + wheel; heights are cached per (lyrics, size).
            let size = (a.lyr_font.x * 2.0).round() / 2.0;
            let hkey = format!("{key}\u{1}{size}");
            if a.lyr_heights.0 != hkey || a.lyr_heights.1.len() != lines.len() {
                // New metrics: any running scroll was measured in the old ones.
                a.lyr_anim = None;
                // Another song: the line it was on means nothing here.
                if !a.lyr_heights.0.starts_with(&format!("{key}\u{1}")) {
                    a.lyr_idx = None;
                }
                let hs = lines.iter().map(|l| a.gfx.measure_wrapped(l, size, 700, col.w).max(size * 1.2)).collect();
                a.lyr_heights = (hkey, hs);
            }
            let hs = a.lyr_heights.1.clone();
            let tops: Vec<f32> = hs.iter().scan(0.0, |y, h| {
                let t = *y;
                *y += h + GAP;
                Some(t)
            }).collect();
            let mut i = idx.unwrap_or(0);
            // Small backward wobbles of the reported position must not bounce the current line.
            if let Some(old) = a.lyr_idx.filter(|&o| o < lines.len() && i < o) {
                if pos > times[old] - 1.0 {
                    i = old;
                }
            }
            let now = std::time::Instant::now();
            // Where the list sits for a given current line, and where it was mid-animation.
            let scroll_for = |j: usize| tops[j];
            let shown = |anim: Option<(std::time::Instant, f32, f32, Option<usize>)>, delay: f32| match anim {
                Some((t0, from, to, _)) => from + (to - from) * settle(now.duration_since(t0).as_secs_f32() - delay),
                None => 0.0,
            };
            if a.lyr_idx != Some(i) {
                let from = if a.lyr_anim.is_some() { shown(a.lyr_anim, 0.0) } else { a.lyr_idx.filter(|&o| o < tops.len()).map(scroll_for).unwrap_or(scroll_for(i)) };
                a.lyr_anim = Some((now, from, scroll_for(i), a.lyr_idx));
                a.lyr_idx = Some(i);
                a.kick();
            }
            // No scroll in flight counts as one that finished long ago.
            let settled = now.checked_sub(std::time::Duration::from_secs(10)).unwrap_or(now);
            let anim = a.lyr_anim.unwrap_or((settled, scroll_for(i), scroll_for(i), None));
            let prev = anim.3;
            let since = now.duration_since(anim.0).as_secs_f32();
            // The current line sits in the middle of the notch at any text size.
            let anchor = (crate::LYRICS[1] - hs[i]) / 2.0 + rise;
            a.lyr_bounds = (-tops[i], tops[tops.len() - 1] - tops[i]);
            let dim = 0.36;
            a.gfx.push_clip(col);
            let zoom = a.lyr_zoom.x;
            if (zoom - 1.0).abs() > 0.001 {
                a.gfx.zoom(col.x, anchor + hs[i] / 2.0, zoom);
            }
            let mut spans = Vec::new();
            // With a manual scroll any line may be in view; otherwise only the neighbourhood.
            let (first, last) = if a.lyr_user.x.abs() > 1.0 { (0, lines.len()) } else { (i.saturating_sub(4), (i + 6).min(lines.len())) };
            for j in first..last {
                // Lines below the current one follow a little later than those above (cascade).
                let delay = (j as f32 - i as f32 + 1.0).clamp(0.0, 6.0) * 0.025;
                let s = shown(Some(anim), delay) + a.lyr_user.x;
                let y = anchor + tops[j] - s;
                if y > col.y + col.h || y + hs[j] < col.y {
                    continue;
                }
                let p = settle(since - delay);
                let target = if j == i { 1.0 } else if j + 1 == i || j == i + 1 { dim } else { dim * 0.6 };
                let before = if Some(j) == prev { 1.0 } else if j + 1 == i || j == i + 1 { dim } else { dim * 0.6 };
                let mut al = before + (target - before) * p;
                if j != i && Rc::new(col.x, y, col.w, hs[j] + 4.0).contains(a.mouse.0, a.mouse.1) {
                    // Hovered lines light up to show they can be clicked.
                    al = al.max(0.8);
                }
                a.gfx.text(&lines[j], Rc::new(col.x, y, col.w, hs[j] + 4.0), size, 700, WHITE, al, Align::LW, Font::Ui);
                spans.push((j, y, hs[j]));
            }
            a.gfx.unzoom();
            a.gfx.pop_clip();
            for (j, y, hh) in spans {
                let top = y.max(col.y);
                let bottom = (y + hh + 4.0).min(col.y + col.h);
                if bottom > top && times[j] >= 0.0 {
                    hit(a, Rc::new(col.x, top, col.w, bottom - top), Act::Seek(((times[j] - off).max(0.0) * 1000.0) as u32));
                }
            }
            // Just moved: how far, for a moment, then it fades.
            if let Some(age) = a.lyr_offset_at.map(|t| t.elapsed().as_secs_f32()).filter(|age| *age < 1.9) {
                let fade = (1.0 - (age - 1.5) / 0.4).clamp(0.0, 1.0);
                let label = if off == 0.0 { "Lyrics in sync".to_string() } else { format!("Lyrics {}{:.1}s", if off > 0.0 { "+" } else { "\u{2212}" }, off.abs()) };
                a.gfx.ui(&label, Rc::new(col.x, col.y + col.h - 16.0, col.w, 16.0), 11.0, 600, WHITE, 0.6 * fade, Align::R);
                a.kick();
            }
        }
        LyrStatus::Missing | LyrStatus::Ready => {
            a.gfx.ui("Lyrics not available", Rc::new(col.x, 62.0 + rise, col.w, 20.0), 13.0, 600, WHITE, 0.45, Align::L);
        }
        _ => {
            a.gfx.ui("Loading lyrics...", Rc::new(col.x, 62.0 + rise, col.w, 20.0), 13.0, 600, WHITE, 0.45, Align::L);
        }
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    #[test]
    fn nothing_in_the_header_can_overlap() {
        assert!(SLOT_CLOCK.1 <= SLOT_LIVE.0 && SLOT_LIVE.1 <= SLOT_ADD.0 && SLOT_ADD.1 <= SLOT_BELL.0);
        assert!(SLOT_CLOCK.1 <= SLOT_CLEAR.0);
        assert!(SLOT_BELL.1 <= 420.0);
    }

    #[test]
    fn a_held_button_follows_a_drag_only_a_little() {
        let r = Rc::new(0.0, 0.0, 36.0, 36.0);
        let (_, _, ox, _) = liquid_drag(r, 400.0, 0.0, 1.1);
        assert!(ox > 0.0 && ox <= 18.0, "toward the drag, never past half its height: {ox}");
        let (sx, sy, _, _) = liquid_drag(r, 30.0, 0.0, 1.0);
        assert!(sx > sy, "it stretches along the drag");
        let (q, p) = toggle_squash(40.0);
        assert!((1.0..=1.25).contains(&q) && (0.8..=1.0).contains(&p), "the knob squashes, at most a fifth");
        assert_eq!(toggle_squash(0.0), (1.0, 1.0));
    }

    #[test]
    fn the_repeat_is_one_of_three_or_none() {
        assert_eq!(repeat_choice([true; 7]), Some(0));
        assert_eq!(repeat_choice(REPEATS[1]), Some(1));
        assert_eq!(repeat_choice([false; 7]), Some(2));
        assert_eq!(repeat_choice([true, false, true, false, false, false, false]), None);
        assert_eq!(days_label(&REPEATS[1]), "Mon–Fri");
    }

    #[test]
    fn the_wash_rises_without_a_jump_and_fades_away() {
        assert_eq!(wash_curve(0.0, 0.0), 0.0);
        assert!(wash_curve(0.05, 0.0) < 0.1);
        assert!((wash_curve(WASH_RISE, 0.0) - 1.0).abs() < 0.01);
        assert!(wash_curve(WASH_SECS, 0.0).abs() < 0.001);
        assert!((wash_curve(0.0, 0.6) - 0.6).abs() < 0.001, "a second press carries on from where it is");
    }

    #[test]
    fn the_pages_run_round() {
        assert!(Page::ALL[5].next() == Page::ALL[0]);
        assert!(Page::ALL[0].prev() == Page::ALL[5]);
    }

    #[test]
    fn days_passed_read_as_past() {
        assert_eq!(days_left(-3), "3 days ago");
        assert_eq!(days_left(-1), "yesterday");
        assert_eq!(days_left(12), "in 12 days");
    }

    #[test]
    fn a_calendar_month_covers_every_day_in_six_weeks() {
        use chrono::Datelike;
        for (y, m, days) in [(2024, 2, 29), (2026, 2, 28), (2026, 11, 30), (2025, 12, 31), (2027, 3, 31)] {
            let grid = month_grid(y, m);
            assert_eq!(grid.len(), 42);
            assert_eq!(grid[0].weekday(), chrono::Weekday::Mon);
            assert_eq!(grid.iter().filter(|d| d.month() == m).count(), days);
        }
    }

    #[test]
    fn focus_lengths_round_trip_through_the_wheel() {
        for m in [5, 25, 50, 90, 120] {
            assert_eq!(focus_minutes(focus_step(m)), m);
        }
    }
}

// ---------------------------------------------------------------- focus

/// Choosing a length, then the running timer: one crossfades into the other.
fn pomo(a: &mut App, lx: f32) {
    title(a, lx, "Focus");
    let run = a.focus_mix.x.clamp(0.0, 1.0);
    // One sun for both: it waits at the horizon, rises as a session starts, then sets with it.
    let (left_s, total_s, running, _) = a.pomo_view();
    let left = if a.brain.focus.is_some() { (left_s / total_s).clamp(0.0, 1.0) as f32 } else { 1.0 };
    // Pointing at Start lifts it a little, as if about to come up.
    let start = Rc::new(lx + FOCUS_GO.0 - 20.0, FOCUS_GO.1 - 20.0, 40.0, 40.0);
    let near = run < 0.5 && !a.focus_pick && hovering(a, start);
    let peek = flip(a, 40, near) * 0.12;
    sun_slices(a, lx + 104.0, left, running, run + peek * (1.0 - run));
    let base = a.gfx.alpha;
    if run < 0.99 {
        a.gfx.alpha = base * (1.0 - run);
        focus_idle(a, lx, run < 0.5);
    }
    if run > 0.01 {
        a.gfx.alpha = base * run;
        focus_running(a, lx, run >= 0.5);
    }
    a.gfx.alpha = base;
}

fn divider(a: &mut App, x: f32, y0: f32, y1: f32) {
    a.gfx.line(x, y0, x, y1, 1.0, LINE, 1.0);
}

/// A round button with its word underneath.
fn labelled(a: &mut App, cx: f32, cy: f32, label: &str, live: bool, act: Act, draw: impl FnOnce(&Gfx, f32, f32)) {
    labelled_hot(a, cx, cy, label, live, 0x5A5A5E, act, draw);
}

/// The same, turning `hot` under the pointer and lifting a little.
#[allow(clippy::too_many_arguments)]
fn labelled_hot(a: &mut App, cx: f32, cy: f32, label: &str, live: bool, hot: u32, act: Act, draw: impl FnOnce(&Gfx, f32, f32)) {
    const R: f32 = 18.0;
    let r = Rc::new(cx - R, cy - R, 2.0 * R, 2.0 * R);
    let h = if live { btn(a, r, 0.08) } else { 0.0 };
    let y = cy - 1.5 * h.clamp(0.0, 1.0);
    if a.settings.glass {
        // The main buttons wear the page's colour; the rest are clear.
        let tint = (hot != 0x5A5A5E).then_some(hot);
        liquid(a, Rc::new(cx - R, y - R, 2.0 * R, 2.0 * R), R, h, tint);
    } else {
        a.gfx.fill_circle(cx, y, R, R, mix(BUTTON, hot, h.clamp(0.0, 1.0)), 1.0);
    }
    draw(&a.gfx, cx, y);
    a.gfx.unzoom();
    a.gfx.ui(label, Rc::new(cx - 34.0, cy + R + 2.0, 68.0, 16.0), SMALL, 500, 0xE0E0E2, 1.0, Align::C);
    if live {
        hit(a, r, act);
    }
}

/// Where the Start (and, while the wheel is out, Done) button sits on the Focus page.
const FOCUS_GO: (f32, f32) = (385.0, 72.0);

/// The Focus page before a session: the sun waits on the left; on the right the kind of session,
/// the length, and Start. "···" slides the controls away for the wheel; Done brings them back.
fn focus_idle(a: &mut App, lx: f32, live: bool) {
    let pomodoro = a.settings.pomodoro;
    let pick = flip(a, 41, a.focus_pick);
    divider(a, lx + 200.0, 48.0, 112.0);
    let base = a.gfx.alpha;
    let off = a.gfx.off;
    if pick < 0.99 {
        let live = live && !a.focus_pick;
        a.gfx.alpha = base * (1.0 - pick);
        a.gfx.offset(off.0 - 26.0 * pick, off.1);
        // Plain focus or Pomodoro: a pill glides to the one in use.
        let mode = flip(a, 1, pomodoro);
        const PILL: f32 = 68.0;
        a.gfx.fill_rrect(Rc::new(lx + 212.0 + mode * PILL, 40.0, PILL, 26.0), 13.0, BUTTON, 1.0);
        for (i, (name, on)) in [("Focus", false), ("Pomodoro", true)].into_iter().enumerate() {
            let r = Rc::new(lx + 212.0 + i as f32 * PILL, 40.0, PILL, 26.0);
            let lit = if on { mode } else { 1.0 - mode };
            let h = if live { btn(a, r, 0.03) } else { 0.0 };
            a.gfx.ui(name, r, SMALL, 500, mix(0x8E8E93, WHITE, lit.max(h.clamp(0.0, 1.0))), 1.0, Align::C);
            a.gfx.unzoom();
            if live {
                hit(a, r, Act::FocusMode(on));
            }
        }
        // The lengths: three set ones, and "···" for the wheel, which shows any other choice.
        let (chosen, rest) = (a.settings.pomo_work, a.settings.pomo_rest);
        let plans: [(u32, u32); 3] = [(25, 5), (50, 10), (90, 15)];
        const CHIP: f32 = 34.0;
        let mut set = false;
        for (i, (work, pause)) in plans.into_iter().enumerate() {
            let r = Rc::new(lx + 212.0 + i as f32 * (CHIP + 4.0), 82.0, CHIP, 28.0);
            let on = work == chosen && (!pomodoro || pause == rest);
            set |= on;
            let words = if pomodoro { format!("{work}/{pause}") } else { work.to_string() };
            focus_chip(a, r, 10 + i as i32, on, &words, live, if pomodoro { Act::PomoPlan(work, pause) } else { Act::PomoMinutes(work) });
        }
        let r = Rc::new(lx + 212.0 + 3.0 * (CHIP + 4.0), 82.0, CHIP, 28.0);
        let other = if pomodoro { format!("×{}", a.settings.pomo_rounds) } else if set { "···".to_string() } else { chosen.to_string() };
        focus_chip(a, r, 13, !pomodoro && !set, &other, live, Act::FocusPick(true));
        // Start, red with a soft glow behind it.
        let (gx, gy) = (lx + FOCUS_GO.0, FOCUS_GO.1);
        a.gfx.glow_ellipse(gx, gy, 26.0, 26.0, RED, 0.16);
        labelled(a, gx, gy, "Start", live, Act::PomoToggle, |g, x, y| icon::play(g, x + 1.0, y, 12.0, RED, 1.0));
        a.gfx.offset(off.0, off.1);
    }
    if pick > 0.01 {
        let live = live && a.focus_pick;
        a.gfx.alpha = base * pick;
        a.gfx.offset(off.0 + 30.0 * (1.0 - pick), off.1);
        let area = Rc::new(lx + 222.0, 30.0, 110.0, 96.0);
        pick_band(a, area.x, area.x + area.w, area.cy(), 32.0);
        if pomodoro {
            let rounds: Vec<String> = (1..=8).map(|n| if n == 1 { "1 round".to_string() } else { format!("{n} rounds") }).collect();
            wheel_sized(a, WheelId::Rounds, area, &rounds, 18.0, RED);
        } else {
            let lengths: Vec<String> = (0..FOCUS_STEPS).map(|i| format!("{} min", focus_minutes(i))).collect();
            wheel_sized(a, WheelId::FocusMinutes, area, &lengths, 20.0, RED);
        }
        if !live {
            // Only the wheel in use takes the pointer.
            a.hits.retain(|(_, act)| !matches!(act, Act::Wheel(WheelId::Rounds | WheelId::FocusMinutes)));
        }
        labelled(a, lx + FOCUS_GO.0, FOCUS_GO.1, "Done", live, Act::FocusPick(false), |g, x, y| icon::tick(g, x, y, RED, 1.0));
        a.gfx.offset(off.0, off.1);
    }
    a.gfx.alpha = base;
}

/// One length chip on the Focus page: outlined red when it is the one chosen.
#[allow(clippy::too_many_arguments)]
fn focus_chip(a: &mut App, r: Rc, key: i32, on: bool, words: &str, live: bool, act: Act) {
    let k = flip(a, key, on);
    let h = if live { btn(a, r, 0.06) } else { 0.0 };
    if a.settings.glass {
        liquid(a, r, 14.0, h, None);
    } else {
        a.gfx.fill_rrect(r, 14.0, mix(BUTTON, 0x48484C, h), 1.0 - k);
    }
    a.gfx.stroke_rrect(r, 14.0, 1.5, RED, k);
    let size = if words.chars().count() > 3 { 11.5 } else { 13.0 };
    a.gfx.ui(words, r, size, 600, mix(0xE0E0E2, RED, k), 1.0, Align::C);
    a.gfx.unzoom();
    if live {
        hit(a, r, act);
    }
}

fn focus_running(a: &mut App, lx: f32, live: bool) {
    let (left_s, _, running, phase) = a.pomo_view();
    pic(a, "ic_focus", lx + 217.0, 58.0, 15.0, RED, 1.0);
    a.gfx.ui(&phase, Rc::new(lx + 230.0, 48.0, 76.0, 20.0), SMALL, 400, 0xB8B8BC, 1.0, Align::L);
    a.gfx.ui(&clock_text(left_s), Rc::new(lx + 208.0, 68.0, 96.0, 38.0), 28.0, 600, WHITE, 1.0, Align::L);
    labelled(a, lx + 330.0, 72.0, if running { "Pause" } else { "Resume" }, live, Act::PomoToggle, |g, x, y| {
        if running {
            icon::pause(g, x, y, 11.0, WHITE, 1.0);
        } else {
            icon::play(g, x + 1.0, y, 11.0, WHITE, 1.0);
        }
    });
    labelled(a, lx + 382.0, 72.0, "Stop", live, Act::PomoReset, |g, x, y| g.fill_rrect(Rc::new(x - 5.5, y - 5.5, 11.0, 11.0), 2.0, WHITE, 1.0));
}

// ---------------------------------------------------------------- briefing

/// Up to three lines for the right of the briefing: what was missed, then what is coming, with
/// the important day and bedtime kept in if there is room.
fn brief_lines(b: &crate::Brief) -> Vec<(BriefRow, String, String)> {
    let mut v: Vec<(BriefRow, String, String)> = b.missed.iter().map(|(t, at)| (BriefRow::Missed, t.clone(), at.clone())).collect();
    if b.hello {
        v.extend(b.coming.iter().map(|(t, at)| (BriefRow::Coming, t.clone(), at.clone())));
        let keep = 3 - b.date.is_some() as usize - b.bedtime.is_some() as usize;
        v.truncate(keep);
        if let Some((title, left)) = &b.date {
            v.push((BriefRow::Date, format!("{title} {}", days_left(*left)), String::new()));
        }
        if let Some(at) = &b.bedtime {
            v.push((BriefRow::Bed, "Bedtime".to_string(), at.clone()));
        }
    }
    v.truncate(3);
    v
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum BriefRow {
    Missed,
    Coming,
    Date,
    Bed,
}

fn brief(a: &mut App, lx: f32) {
    let Some(b) = a.brief.as_ref() else { return };
    // The words come in once the eyes have noticed you.
    let age = b.opened_s();
    let come = |at: f32| {
        let k = ((age - at) / 0.4).clamp(0.0, 1.0);
        1.0 - (1.0 - k) * (1.0 - k)
    };
    let lines = brief_lines(b);
    let (hello, today) = (b.hello, b.date.clone().filter(|d| d.1 == 0));
    title(a, lx, "Today");
    head_button(a, Rc::new(lx + SLOT_BELL.0, 7.0, 24.0, 24.0), Act::BriefClose, |g, x, y| icon::cross(g, x, y, 4.5, WHITE, 1.0));
    let eyes_y = if hello && today.is_none() { 74.0 } else { 84.0 };
    act_eyes(a, lx + 92.0, eyes_y, 0.19, 0.0);
    divider(a, lx + 186.0, 50.0, 122.0);
    if let Some((name, _)) = today {
        // The day itself: nothing else on the right.
        pic(a, "ic_star", lx + 212.0, 74.0, 17.0, RED, 1.0);
        a.gfx.text(&format!("{name} is today"), Rc::new(lx + 230.0, 58.0, 150.0, 56.0), 17.0, 500, WHITE, 1.0, Align::LW, Font::Ui);
        return;
    }
    if hello {
        use chrono::Timelike;
        let hello_at = match chrono::Local::now().hour() {
            0..=10 => "Good morning",
            11..=17 => "Good afternoon",
            _ => "Good evening",
        };
        let k = come(1.0);
        a.gfx.ui(hello_at, Rc::new(lx + 24.0 - 24.0 * (1.0 - k), 100.0, 136.0, 24.0), 16.0, 500, WHITE, k, Align::C);
    } else {
        a.gfx.ui("While you were away", Rc::new(lx + 202.0, 50.0, 190.0, 22.0), BODY, 500, WHITE, 1.0, Align::L);
    }
    let (y0, gap) = if hello { (60.0, 30.0) } else { (92.0, 26.0) };
    for (i, (row, text, time)) in lines.into_iter().enumerate() {
        let y = y0 + i as f32 * gap;
        let dim = if row == BriefRow::Missed { 0.55 } else { 1.0 };
        match row {
            BriefRow::Missed => icon::bell(&a.gfx, lx + 210.0, y, WHITE, dim),
            BriefRow::Coming => pic(a, "t_today", lx + 210.0, y, 14.0, WHITE, 1.0),
            BriefRow::Date => pic(a, "ic_star", lx + 210.0, y, 13.0, RED, 1.0),
            BriefRow::Bed => pic(a, "ic_sleep", lx + 210.0, y, 14.0, 0xFFD9A0, 1.0),
        }
        let w = if time.is_empty() { 172.0 } else { 124.0 };
        let k = come(1.2 + 0.12 * i as f32);
        let dx = 16.0 * (1.0 - k);
        a.gfx.ui(&text, Rc::new(lx + 226.0 + dx, y - 11.0, w, 22.0), 13.0, 400, WHITE, dim * k, Align::L);
        a.gfx.ui(&time, Rc::new(lx + 340.0 + dx, y - 11.0, 56.0, 22.0), 12.0, 400, 0xC8C8CC, dim * k, Align::R);
    }
}

#[cfg(test)]
mod brief_tests {
    use super::*;

    fn b(hello: bool, missed: usize, coming: usize, date: bool, bed: bool) -> crate::Brief {
        let items = |n| (0..n).map(|i| (format!("t{i}"), "10:00".to_string())).collect();
        crate::Brief::sample(hello, items(missed), items(coming), date.then(|| ("Day".to_string(), 3)), bed.then(|| "23:00".to_string()))
    }

    #[test]
    fn the_morning_keeps_the_day_and_bedtime_in_view() {
        let rows: Vec<_> = brief_lines(&b(true, 0, 5, true, true)).into_iter().map(|r| r.0).collect();
        assert_eq!(rows, [BriefRow::Coming, BriefRow::Date, BriefRow::Bed]);
    }

    #[test]
    fn away_lists_only_what_was_missed() {
        let rows: Vec<_> = brief_lines(&b(false, 2, 5, true, true)).into_iter().map(|r| r.0).collect();
        assert_eq!(rows, [BriefRow::Missed, BriefRow::Missed]);
    }
}

// ---------------------------------------------------------------- guide

/// The eyes lead an activity: them on the left, the step and its time on the right.
fn guide_page(a: &mut App, lx: f32) {
    let Some(g) = a.guide.as_ref() else { return };
    let (tile, paused, time, ended) = (g.tile, g.paused(), g.time(), g.ended);
    let moment = crate::guide::at(&g.steps, time).map(|(i, u, left)| (g.steps[i], u, left));
    back_title(a, lx, tile.name(), Act::GuideBack);
    let (cx, cy) = (lx + 100.0, 84.0);
    let [dx, dy, tilt, size] = a.guide_pose.each_ref().map(|s| s.x);
    a.gfx.rotate(cx + dx, cy + dy, tilt);
    a.eye_draws.1 += 1;
    let scale = 0.17 * size;
    for i in 0..2 {
        let outline: Vec<(f32, f32)> = a.eyes.pair[i].points.iter().map(|p| (cx + dx + p.0 * scale, cy + dy + a.eyes.hop.x + p.1 * scale)).collect();
        lens(&a.gfx, &outline, 3.4, 1.0);
    }
    a.gfx.unzoom();
    divider(a, lx + 196.0, 50.0, 118.0);
    let tint = tile.colour();
    match (moment, ended) {
        (_, Some((_, true))) => a.gfx.ui("Well done", Rc::new(lx + 212.0, 66.0, 130.0, 26.0), 18.0, 600, WHITE, 1.0, Align::L),
        (_, Some(_)) => a.gfx.ui("Maybe later", Rc::new(lx + 212.0, 66.0, 130.0, 26.0), 18.0, 600, WHITE, 1.0, Align::L),
        (Some((step, u, left)), _) => {
            a.gfx.ui(step.label, Rc::new(lx + 212.0, 50.0, 136.0, 22.0), BODY, 500, WHITE, 1.0, Align::L);
            use crate::guide::Move as M;
            match step.moves {
                M::NeckRoll => {
                    for i in 0..12 {
                        let t = -std::f32::consts::FRAC_PI_2 + i as f32 / 12.0 * std::f32::consts::TAU;
                        let on = i as f32 / 12.0 <= u;
                        a.gfx.fill_circle(lx + 238.0 + 16.0 * t.cos(), 96.0 + 16.0 * t.sin(), 1.8, 1.8, tint, if on { 1.0 } else { 0.25 });
                    }
                }
                M::BreatheIn | M::Hold | M::BreatheOut | M::HoldEmpty => {
                    let bar = Rc::new(lx + 212.0, 92.0, 130.0, 6.0);
                    a.gfx.fill_rrect(bar, 3.0, WHITE, 0.14);
                    a.gfx.fill_rrect(Rc::new(bar.x, bar.y, (bar.w * u).max(bar.h), bar.h), 3.0, tint, 1.0);
                }
                _ => a.gfx.ui(&format!("{}", left.ceil() as u32), Rc::new(lx + 212.0, 72.0, 90.0, 40.0), 30.0, 600, WHITE, 1.0, Align::L),
            }
        }
        _ => {}
    }
    if ended.is_none() {
        labelled(a, lx + 372.0, 72.0, if paused { "Resume" } else { "Pause" }, true, Act::GuidePause, |g, x, y| {
            if paused {
                icon::play(g, x + 1.0, y, 11.0, WHITE, 1.0);
            } else {
                icon::pause(g, x, y, 11.0, WHITE, 1.0);
            }
        });
    }
}

// ---------------------------------------------------------------- today

/// The red button a swiped row shows, sliding in from the right.
fn delete_button(a: &mut App, right: f32, cy: f32, w: f32, k: f32) {
    let r = Rc::new(right - w + (1.0 - k) * 40.0, cy - 13.0, w, 26.0);
    let h = btn(a, r, 0.05).clamp(0.0, 1.0);
    let base = a.gfx.alpha;
    a.gfx.alpha = base * k;
    a.gfx.fill_rrect(r, 13.0, mix(0xD93A40, 0xF0525A, h), 1.0);
    a.gfx.ui("Delete", r, SMALL, 600, WHITE, 1.0, Align::C);
    a.gfx.alpha = base;
    a.gfx.unzoom();
    if k > 0.5 {
        hit(a, r, Act::RowDelete);
    }
}

/// How far this row has slid aside.
fn swipe_of(a: &App, key: &str) -> f32 {
    if a.swipe_last == key {
        a.swipe_mix.x.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn days_left(left: i64) -> String {
    match left {
        0 => "today".to_string(),
        1 => "tomorrow".to_string(),
        -1 => "yesterday".to_string(),
        n if n < 0 => format!("{} days ago", -n),
        n => format!("in {n} days"),
    }
}

/// The week with what matters in it, and the day so far along the bottom.
fn today(a: &mut App, lx: f32) {
    use chrono::Datelike;
    title(a, lx, "Today");
    let now = chrono::Local::now().date_naive();
    let monday = now - chrono::TimeDelta::days(now.weekday().num_days_from_monday() as i64);
    let marked: Vec<chrono::NaiveDate> = a.brain.cfg.dates.iter().filter_map(|d| chrono::NaiveDate::parse_from_str(&d.date, "%Y-%m-%d").ok()).collect();
    let strip = Rc::new(lx + 24.0, 38.0, 372.0, 54.0);
    let line_box = Rc::new(lx + 28.0, 98.0, 220.0, 24.0);
    let h = btn(a, strip, 0.0);
    if h > 0.0 {
        a.gfx.fill_rrect(strip, 14.0, 0xFFFFFF, 0.05 * h.min(1.0));
    }
    let col = strip.w / 7.0;
    for i in 0..7 {
        let day = monday + chrono::TimeDelta::days(i);
        let cx = strip.x + col * (i as f32 + 0.5);
        let on = day == now;
        if on {
            a.gfx.fill_rrect(Rc::new(cx - 18.0, 40.0, 36.0, 46.0), 11.0, mix(0xC42A30, 0xD93A40, h.clamp(0.0, 1.0)), 1.0);
        }
        let name = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][i as usize];
        a.gfx.ui(name, Rc::new(cx - 20.0, 43.0, 40.0, 14.0), 11.0, 400, if on { WHITE } else { 0x9A9A9E }, 1.0, Align::C);
        a.gfx.ui(&day.day().to_string(), Rc::new(cx - 20.0, 58.0, 40.0, 22.0), 16.0, 600, WHITE, 1.0, Align::C);
        if marked.contains(&day) {
            a.gfx.fill_circle(cx, if on { 82.0 } else { 86.0 }, 2.2, 2.2, if on { WHITE } else { RED }, 1.0);
        }
    }
    a.gfx.unzoom();
    hit(a, strip, Act::View(View::Calendar));
    hit(a, line_box, Act::View(View::Calendar));
    const CY: f32 = 110.0;
    let next = a.all_dates().into_iter().find(|d| d.2 >= 0);
    pic(a, "ic_star", lx + 38.0, CY, 13.0, RED, 1.0);
    let line = match &next {
        Some((_, name, left)) => format!("{name} {}", days_left(*left)),
        _ => "Add a day that matters".to_string(),
    };
    a.gfx.ui(&line, Rc::new(lx + 54.0, CY - 11.0, 176.0, 22.0), 13.0, 400, 0xD8D8DA, 1.0, Align::L);
    divider(a, lx + 236.0, CY - 8.0, CY + 8.0);
    let cfg = &a.brain.cfg;
    let litres = (a.brain.stats.water * cfg.cup_ml) as f32 / 1000.0;
    pic(a, "ic_water", lx + 251.0, CY, 13.0, WATER, 1.0);
    a.gfx.ui(&format!("{litres:.1} L"), Rc::new(lx + 261.0, CY - 11.0, 40.0, 22.0), 13.0, 400, 0xD8D8DA, 1.0, Align::L);
    divider(a, lx + 304.0, CY - 8.0, CY + 8.0);
    let focus = a.brain.stats.focus;
    a.gfx.ui(&format!("Focus {focus} min"), Rc::new(lx + 311.0, CY - 11.0, 90.0, 22.0), 13.0, 400, 0xD8D8DA, 1.0, Align::L);
}

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

/// A month's days as the calendar lays them out: six rows of seven from the Monday on or before
/// the first.
fn month_grid(year: i32, month: u32) -> Vec<chrono::NaiveDate> {
    use chrono::Datelike;
    let first = chrono::NaiveDate::from_ymd_opt(year, month, 1).unwrap_or_default();
    let start = first - chrono::TimeDelta::days(first.weekday().num_days_from_monday() as i64);
    (0..42).map(|i| start + chrono::TimeDelta::days(i)).collect()
}

/// The month, tall, and what is coming up beside it.
fn calendar(a: &mut App, lx: f32) {
    use chrono::Datelike;
    back_title(a, lx, "Month", Act::View(View::Page));
    let (year, month) = a.month;
    let (x0, col) = (lx + 22.0, 34.0);
    let centre = x0 + 3.5 * col;
    let (heading, up) = match a.cal {
        Cal::Days => (format!("{} {year}", MONTHS[month as usize - 1]), Cal::Months),
        Cal::Months => (year.to_string(), Cal::Years),
        Cal::Years => (format!("{} – {}", year / 10 * 10, year / 10 * 10 + 9), Cal::Days),
    };
    let top = Rc::new(centre - 80.0, 38.0, 160.0, 22.0);
    let h = btn(a, top, 0.03);
    a.gfx.fill_rrect(top, 11.0, 0xFFFFFF, 0.08 * h.clamp(0.0, 1.0));
    a.gfx.ui(&heading, top, BODY, 600, WHITE, 1.0, Align::C);
    a.gfx.unzoom();
    hit(a, top, Act::CalMode(up));
    for (x, left, by) in [(centre - 100.0, true, -1), (centre + 100.0, false, 1)] {
        head_button(a, Rc::new(x - 11.0, 38.0, 22.0, 22.0), Act::Month(by), move |g, cx, cy| icon::chevron(g, cx, cy, left, WHITE, 1.0));
    }
    let today = chrono::Local::now().date_naive();
    let (base, k) = (a.gfx.alpha, a.month_mix.x.clamp(0.0, 1.0));
    let ox = a.gfx.off.0;
    a.gfx.offset(ox + a.month_dir * 18.0 * k, 0.0);
    a.gfx.alpha = base * (1.0 - k);
    match a.cal {
        Cal::Days => days_grid(a, x0, col, (year, month), today),
        Cal::Months => {
            for m in 1..=12u32 {
                let (cw, ch) = (col * 7.0 / 3.0, 44.0);
                let r = Rc::new(x0 + ((m - 1) % 3) as f32 * cw + 3.0, 72.0 + ((m - 1) / 3) as f32 * ch + 3.0, cw - 6.0, ch - 6.0);
                let on = m == today.month() && year == today.year();
                pick_cell(a, r, &MONTHS[m as usize - 1][..3], on, false, Act::PickMonth(m));
            }
        }
        Cal::Years => {
            let first = year / 10 * 10 - 1;
            for i in 0..12 {
                let (cw, ch) = (col * 7.0 / 3.0, 44.0);
                let r = Rc::new(x0 + (i % 3) as f32 * cw + 3.0, 72.0 + (i / 3) as f32 * ch + 3.0, cw - 6.0, ch - 6.0);
                // The first and last are outside the decade in the heading.
                pick_cell(a, r, &(first + i).to_string(), first + i == today.year(), i == 0 || i == 11, Act::PickYear(first + i));
            }
        }
    }
    a.gfx.offset(ox, 0.0);
    a.gfx.alpha = base;
    divider(a, lx + 268.0, 44.0, 254.0);
    a.gfx.ui("Upcoming", Rc::new(lx + 282.0, 38.0, 120.0, 22.0), BODY, 600, WHITE, 1.0, Align::L);
    let dates = a.all_dates();
    if dates.is_empty() {
        a.gfx.ui("Press a day to add one", Rc::new(lx + 282.0, 70.0, 124.0, 34.0), SMALL, 400, 0x8E8E93, 1.0, Align::LW);
    }
    for (i, (id, name, left)) in dates.into_iter().take(4).enumerate() {
        let y = 68.0 + i as f32 * 46.0;
        let dim = if left < 0 { 0.5 } else { 1.0 };
        // The trash waits until the row is pointed at, so the name has the room.
        let key = format!("d:{id}");
        let slid = swipe_of(a, &key);
        let near = slid < 0.05 && hovering(a, Rc::new(lx + 278.0, y, 138.0, 40.0));
        let tw = if near { 76.0 } else { 104.0 } - 46.0 * slid;
        let x = lx + 308.0 - 26.0 * slid;
        pic(a, "ic_star", lx + 293.0, y + 18.0, 13.0, RED, dim * (1.0 - slid));
        a.gfx.ui(&name, Rc::new(x, y + 2.0, tw, 18.0), 13.0, 500, WHITE, dim, Align::L);
        a.gfx.ui(&days_left(left), Rc::new(x, y + 20.0, tw, 16.0), SMALL, 400, 0xA0A0A5, dim, Align::L);
        if near {
            head_button(a, Rc::new(lx + 388.0, y + 8.0, 22.0, 22.0), Act::RowTrash(1, i), |g, x, y| icon::trash(g, x, y, WHITE, 1.0));
        }
        if slid > 0.01 {
            delete_button(a, lx + 410.0, y + 19.0, 58.0, slid);
        }
    }
}

/// A month or year to jump to: plain text, the current one red with a dot under it.
fn pick_cell(a: &mut App, r: Rc, text: &str, on: bool, faint: bool, act: Act) {
    let h = btn(a, r, 0.03).clamp(0.0, 1.0);
    let rest = if faint { 0x6E6E72 } else { 0xD8D8DA };
    let (colour, weight) = if on { (RED, 600) } else { (mix(rest, WHITE, h), if h > 0.5 { 600 } else { 400 }) };
    a.gfx.ui(text, r, 15.0, weight, colour, 1.0, Align::C);
    if on {
        a.gfx.fill_circle(r.cx(), r.cy() + 14.0, 2.0, 2.0, RED, 1.0);
    }
    a.gfx.unzoom();
    hit(a, r, act);
}

fn days_grid(a: &mut App, x0: f32, col: f32, (year, month): (i32, u32), today: chrono::NaiveDate) {
    use chrono::Datelike;
    let (y0, row) = (88.0, 29.0);
    for (i, name) in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"].into_iter().enumerate() {
        a.gfx.ui(name, Rc::new(x0 + i as f32 * col, 62.0, col, 16.0), 11.0, 400, 0x9A9A9E, 1.0, Align::C);
    }
    let marked: Vec<chrono::NaiveDate> = a.brain.cfg.dates.iter().filter_map(|d| chrono::NaiveDate::parse_from_str(&d.date, "%Y-%m-%d").ok()).collect();
    for (i, day) in month_grid(year, month).into_iter().enumerate() {
        let cx = x0 + (i % 7) as f32 * col + col / 2.0;
        let cy = y0 + (i / 7) as f32 * row + row / 2.0;
        let r = Rc::new(cx - 14.0, cy - 13.0, 28.0, 26.0);
        let inside = day.month() == month;
        let h = btn(a, r, 0.1);
        if day == today {
            a.gfx.fill_circle(cx, cy, 12.0, 12.0, RED, 1.0);
        } else if h > 0.0 {
            a.gfx.fill_circle(cx, cy, 12.0, 12.0, 0xFFFFFF, 0.12 * h.min(1.0));
        }
        a.gfx.ui(&day.day().to_string(), r, 13.0, if day == today { 600 } else { 400 }, if inside { WHITE } else { 0x6A6A6E }, 1.0, Align::C);
        if marked.contains(&day) {
            a.gfx.fill_circle(cx, cy + 10.5, 1.8, 1.8, if day == today { WHITE } else { RED }, 1.0);
        }
        a.gfx.unzoom();
        hit(a, r, Act::Day(day.year(), day.month(), day.day()));
    }
}

// ---------------------------------------------------------------- care

/// Water large on the left, the other reminders as tiles beside it.
/// The water on the Care page: a drop made of lit lines, like the eyes and the Focus sun. The
/// lines below the water's level are lit blue, the ones above grey; a cup just drunk (`since`
/// seconds ago) rocks the surface into a wave and throws a few short lines out to the sides.
/// Full, the whole drop glows. Under it, a few hairlines of reflection.
fn water_drop(a: &mut App, cx: f32, level: f32, since: f32, hot: f32) {
    // The shape: a point at the top, a round body below (tangent to the two straight sides).
    const TIP: f32 = 36.0;
    const C: f32 = 78.0;
    const R: f32 = 26.0;
    let bottom = C + R;
    let swell = 1.0 + 0.04 * a.water_pulse.x.clamp(-1.0, 2.0);
    let d = C - TIP;
    let touch = C - R * R / d;
    let touch_half = R * (1.0 - (R / d).powi(2)).sqrt();
    let half_at = |y: f32| -> f32 {
        let h = if y <= TIP {
            0.0
        } else if y < touch {
            touch_half * (y - TIP) / (touch - TIP)
        } else {
            (R * R - (y - C).powi(2)).max(0.0).sqrt()
        };
        h * swell
    };
    let surface = bottom - level * (bottom - TIP);
    // A cup just drunk: the surface rocks, settling over a second.
    let amp = if since < 1.2 { 3.2 * (1.0 - since / 1.2).powf(1.5) } else { 0.0 };
    let wave = |x: f32| surface + amp * ((x - cx) * 0.26 - since * 10.0).sin();
    let g = &a.gfx;
    if level > 0.999 {
        let beat = if since < 1.6 { (1.0 - since / 1.6) * 0.25 } else { 0.0 };
        g.glow_ellipse(cx, C - 6.0, R * 1.5, R * 1.9, WATER, 0.2 + beat);
    } else if level > 0.01 {
        g.glow_ellipse(cx, (surface + bottom) / 2.0, R * 1.2, (bottom - surface) * 0.7 + 4.0, WATER, 0.08);
    }
    const PITCH: f32 = 4.4;
    const BAR: f32 = 2.4;
    let lit_tone = |y: f32| mix(WATER, 0x9AF0FA, (1.0 - (y - surface) / 30.0).clamp(0.0, 1.0) * 0.55);
    let dim = |y: f32| (0.3 + 0.45 * ((y - TIP) / (bottom - TIP))).clamp(0.0, 1.0) + 0.18 * hot;
    let mut y = TIP + 3.0;
    while y < bottom {
        let h = half_at(y);
        if h > 1.0 {
            if amp < 0.05 || (y - surface).abs() > amp + BAR {
                if y > surface {
                    g.fill_rrect(Rc::new(cx - h, y - BAR / 2.0, 2.0 * h, BAR), BAR / 2.0, lit_tone(y), 1.0);
                } else {
                    g.fill_rrect(Rc::new(cx - h, y - BAR / 2.0, 2.0 * h, BAR), BAR / 2.0, 0x4A4A4E, dim(y));
                }
            } else {
                // Where the wave passes, the line is lit in pieces: under the wave lit, over it grey.
                const PIECES: usize = 8;
                let step = 2.0 * h / PIECES as f32;
                for i in 0..PIECES {
                    let x = cx - h + step * i as f32;
                    let piece = Rc::new(x, y - BAR / 2.0, step + 0.3, BAR);
                    if y > wave(x + step / 2.0) {
                        g.fill_rrect(piece, 0.6, lit_tone(y), 1.0);
                    } else {
                        g.fill_rrect(piece, 0.6, 0x4A4A4E, dim(y));
                    }
                }
            }
        }
        y += PITCH;
    }
    // The splash: three short lines thrown out on each side, arcing up and away.
    if since < 0.9 && level > 0.01 {
        let e = since / 0.9;
        let edge = half_at(surface.clamp(TIP + 6.0, bottom - 2.0));
        for side in [-1.0f32, 1.0] {
            for j in 0..3 {
                let k = j as f32;
                let x = cx + side * (edge + 3.0 + (10.0 + 6.0 * k) * e);
                let y = surface - (14.0 - 3.0 * k) * (std::f32::consts::PI * e).sin() + 4.0 * e * e - k * 2.5;
                let len = 8.0 - 2.0 * k;
                g.fill_rrect(Rc::new(x - len / 2.0, y - 1.3, len, 2.6), 1.3, 0x9AF0FA, (1.0 - e * e) * (1.0 - 0.15 * k));
            }
        }
    }
    // The reflection: hairlines under the drop, fading out at both ends.
    let tint = if level > 0.01 { WATER } else { 0x6A6A6E };
    for (i, (dy, reach, alpha)) in [(3.6, 34.0, 0.7), (6.8, 24.0, 0.4)].into_iter().enumerate() {
        let y = bottom + dy;
        let alpha = alpha * if level > 0.01 { 0.5 + 0.5 * level } else { 0.6 };
        let reach = reach + 2.0 * (since * 3.0 + i as f32).sin() * (amp / 3.2);
        g.fill_rrect_hgrad(Rc::new(cx - reach, y - 0.6, reach, 1.2), 0.0, (tint, 0.0), (tint, alpha));
        g.fill_rrect_hgrad(Rc::new(cx, y - 0.6, reach, 1.2), 0.0, (tint, alpha), (tint, 0.0));
    }
}

fn care(a: &mut App, lx: f32) {
    title(a, lx, "Care");
    let cfg = &a.brain.cfg;
    let goal = cfg.water_goal * cfg.cup_ml;
    let cx = lx + 72.0;
    let ring = Rc::new(cx - 36.0, 36.0, 72.0, 94.0);
    // Pointing at it shows it can be set.
    let hot = btn(a, ring, 0.02).clamp(0.0, 1.0);
    a.gfx.unzoom();
    let since = a.drank_at.map_or(9.0, |t| std::time::Instant::now().saturating_duration_since(t).as_secs_f32());
    water_drop(a, cx, a.ring.x.clamp(0.0, 1.0), since, hot);
    // The amount under it, the goal in grey; pointing turns it into "Set goal".
    let (have, of) = (format!("{:.1}", a.ring.x.max(0.0) * goal as f32 / 1000.0), format!(" / {:.1} L", goal as f32 / 1000.0));
    let (w0, w1) = (a.gfx.measure(&have, SMALL, 700), a.gfx.measure(&of, SMALL, 500));
    let x0 = cx - (w0 + w1) / 2.0;
    let line = Rc::new(x0, 113.0, w0 + w1, 16.0);
    a.gfx.ui(&have, Rc::new(x0, line.y, w0 + 2.0, line.h), SMALL, 700, WHITE, 1.0 - hot, Align::L);
    a.gfx.ui(&of, Rc::new(x0 + w0, line.y, w1 + 2.0, line.h), SMALL, 500, 0x8E8E93, 1.0 - hot, Align::L);
    a.gfx.ui("Set goal", Rc::new(cx - 40.0, line.y, 80.0, line.h), SMALL, 600, WHITE, hot, Align::C);
    hit(a, ring, Act::View(View::Goal));
    head_button(a, Rc::new(lx + 124.0, 49.0, 32.0, 32.0), Act::Drank, |g, x, y| icon::plus(g, x, y, 6.0, 1.9, WHITE, 1.0));
    head_button(a, Rc::new(lx + 124.0, 87.0, 32.0, 32.0), Act::WaterReset, |g, x, y| icon::reset(g, x, y, 5.6, WHITE, 1.0));
    divider(a, lx + 168.0, 48.0, 124.0);
    let stats = a.brain.stats.clone();
    let sleep = a.brain.cfg.reminders.sleep.clone();
    for (i, tile) in Tile::ALL.into_iter().enumerate() {
        let r = Rc::new(lx + 178.0 + (i % 2) as f32 * 112.0, 44.0 + (i / 2) as f32 * 42.0, 106.0, 37.0);
        let on = reminder_on(a, tile);
        let value = match tile {
            Tile::Stretch => stats.stretch.to_string(),
            Tile::Eyes => stats.eyes.to_string(),
            Tile::Breathe => stats.breathe.to_string(),
            Tile::Sleep => sleep.time.clone(),
        };
        let h = btn(a, r, 0.03);
        const CARD: u32 = 0x18181A;
        let bg = mix(CARD, 0x2C2C2E, h);
        a.gfx.fill_rrect(r, 11.0, bg, 1.0);
        let dim = if on { 1.0 } else { 0.4 };
        pic(a, tile.picture(), r.x + 18.0, r.cy(), 20.0, WHITE, dim);
        if on {
            a.gfx.fill_circle(r.x + r.w - 11.0, r.y + 11.0, 3.0, 3.0, tile.colour(), 1.0);
        }
        a.gfx.ui(tile.name(), Rc::new(r.x + 34.0, r.y + 4.0, r.w - 36.0, 15.0), SMALL, 500, WHITE, dim, Align::L);
        a.gfx.ui(if on { &value } else { "Off" }, Rc::new(r.x + 34.0, r.y + 19.0, r.w - 36.0, 14.0), 11.0, 400, 0xA0A0A5, dim, Align::L);
        a.gfx.unzoom();
        hit(a, r, Act::View(View::Care(tile)));
    }
}

fn reminder_on(a: &App, tile: Tile) -> bool {
    let r = &a.brain.cfg.reminders;
    match tile {
        Tile::Stretch => r.stretch.enabled,
        Tile::Eyes => r.eyes.enabled,
        Tile::Breathe => r.breathe.enabled,
        Tile::Sleep => r.sleep.enabled,
    }
}

/// One reminder opened: its switch, when it comes next, and doing it now.
fn care_detail(a: &mut App, lx: f32, tile: Tile) {
    back_title(a, lx, tile.name(), Act::View(View::Page));
    pic(a, tile.picture(), lx + 66.0, 80.0, 46.0, WHITE, 1.0);
    divider(a, lx + 112.0, 50.0, 112.0);
    let on = reminder_on(a, tile);
    a.gfx.ui(tile.name(), Rc::new(lx + 128.0, 46.0, 170.0, 22.0), 15.0, 600, WHITE, 1.0, Align::L);
    a.gfx.ui(tile.about(), Rc::new(lx + 128.0, 66.0, 200.0, 18.0), SMALL, 400, 0xB8B8BC, 1.0, Align::L);
    pic(a, "ic_focus", lx + 137.0, 100.0, 14.0, WHITE, 0.85);
    let next = if !on {
        "Off".to_string()
    } else {
        match a.brain.next_in(tile.kind(), chrono::Local::now()) {
            Some(s) if s >= 3600 => format!("Next: in {} h {} min", s / 3600, s % 3600 / 60),
            Some(s) => format!("Next: in {} min", (s / 60).max(1)),
            None => "Next: soon".to_string(),
        }
    };
    let text = if tile == Tile::Sleep { format!("At {}", a.brain.cfg.reminders.sleep.time) } else { next };
    let line = Rc::new(lx + 150.0, 89.0, 124.0, 22.0);
    if tile == Tile::Sleep {
        let h = btn(a, line, 0.03);
        a.gfx.ui(&text, line, SMALL, 500, mix(0xD8D8DA, WHITE, h), 1.0, Align::L);
        a.gfx.unzoom();
        hit(a, line, Act::View(View::Bed));
    } else {
        a.gfx.ui(&text, line, SMALL, 400, 0xD8D8DA, 1.0, Align::L);
    }
    toggle(a, Rc::new(lx + 354.0, 46.0, SWITCH.0, SWITCH.1), (0, tile as i32), on, Act::Set(tile.key()), SWITCH_GREEN);
    let now = Rc::new(lx + 284.0, 88.0, 108.0, 28.0);
    let h = btn(a, now, 0.05);
    a.gfx.fill_rrect(now, 14.0, 0xFFFFFF, 0.14 + 0.1 * h);
    icon::play(&a.gfx, now.x + 18.0, now.cy(), 9.0, WHITE, 1.0);
    a.gfx.ui("Do it now", Rc::new(now.x + 28.0, now.y, now.w - 30.0, now.h), SMALL, 500, WHITE, 1.0, Align::L);
    a.gfx.unzoom();
    hit(a, now, Act::DoNow(tile));
}

/// The cup and the day's goal; only the side being set is lit.
fn goal(a: &mut App, lx: f32) {
    back_title(a, lx, "Water goal", Act::View(View::Page));
    let cups: Vec<String> = model::CUPS.iter().map(|c| format!("{c} ml")).collect();
    let goals: Vec<String> = model::GOALS.iter().map(|g| format!("{:.1} L", *g as f32 / 1000.0)).collect();
    let (base, k) = (a.gfx.alpha, a.pick_mix.x.clamp(0.0, 1.0));
    a.gfx.alpha = base * (1.0 - 0.72 * k);
    pick_band(a, lx + 64.0, lx + 336.0, 82.0, 32.0);
    wheel_sized(a, WheelId::Cup, Rc::new(lx + 60.0, 36.0, 140.0, 92.0), &cups, 20.0, WATER);
    a.gfx.alpha = base * (0.28 + 0.72 * k);
    wheel_sized(a, WheelId::Goal, Rc::new(lx + 210.0, 36.0, 130.0, 92.0), &goals, 20.0, WATER);
    a.gfx.alpha = base;
    form_button(a, lx + 376.0, 82.0, FormButton::Done, Act::SaveWheels);
}

fn bed(a: &mut App, lx: f32) {
    back_title(a, lx, "Bedtime", Act::View(View::Care(Tile::Sleep)));
    let hours: Vec<String> = (0..24).map(|h| format!("{h:02}")).collect();
    let quarters: Vec<String> = (0..4).map(|q| format!("{:02}", q * 15)).collect();
    pick_band(a, lx + 118.0, lx + 302.0, 82.0, 38.0);
    wheel_sized(a, WheelId::BedHour, Rc::new(lx + 118.0, 36.0, 76.0, 92.0), &hours, 28.0, WHITE);
    a.gfx.ui(":", Rc::new(lx + 198.0, 60.0, 24.0, 40.0), 26.0, 600, WHITE, 1.0, Align::C);
    wheel_sized(a, WheelId::BedMinute, Rc::new(lx + 226.0, 36.0, 76.0, 92.0), &quarters, 28.0, WHITE);
    form_button(a, lx + 376.0, 82.0, FormButton::Done, Act::SaveWheels);
}

#[allow(clippy::too_many_arguments)]
/// The iOS-style band behind the chosen row of one or more wheels side by side: from `x0` to
/// `x1`, centred on `cy`, `h` tall.
fn pick_band(a: &mut App, x0: f32, x1: f32, cy: f32, h: f32) {
    a.gfx.fill_rrect(Rc::new(x0, cy - h / 2.0, x1 - x0, h), 14.0, 0xFFFFFF, 0.1);
}

/// What a rolling picker sets.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WheelId {
    Cup,
    Goal,
    BedHour,
    BedMinute,
    FormHour,
    FormMinute,
    FocusMinutes,
    Rounds,
}

/// A setting flipped or stepped from a panel.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SetKey {
    Water,
    LyricSize,
    Banners,
    Text,
    Eyes,
    Jam,
    Autostart,
    Minimal,
    Glass,
    Stretch,
    RestEyes,
    Breathe,
    Sleep,
    Care,
    Lyrics,
}



fn back_title(a: &mut App, lx: f32, title: &str, act: Act) {
    head_button(a, Rc::new(lx + 18.0, 7.0, 24.0, 24.0), act, |g, x, y| icon::chevron(g, x, y, true, WHITE, 1.0));
    a.gfx.ui(title, Rc::new(lx + 50.0, 8.0, 130.0, 22.0), HEAD, 600, WHITE, 1.0, Align::L);
}

/// A rolling picker: the value in the middle is the choice, the others lean back and fade.
/// The chosen value is lit in the page's own colour (`tint`); the band behind it comes from
/// `pick_band`, drawn by the screen so it can run under several wheels, as on iOS.
fn wheel_sized(a: &mut App, id: WheelId, r: Rc, labels: &[String], big: f32, tint: u32) {
    let row = a.wheels[id as usize].row;
    let pos = a.wheels[id as usize].pos;
    a.gfx.push_clip(r);
    let first = (pos.floor() as i32 - 2).max(0);
    for i in first..(pos.ceil() as i32 + 3).min(labels.len() as i32) {
        let d = i as f32 - pos;
        if d.abs() > 1.9 {
            continue;
        }
        // Rows bend back around a drum: closer together away from the middle.
        let y = r.cy() + row * 1.2 * (d * 0.85).sin() / 0.85;
        let near = (1.0 - d.abs()).clamp(0.0, 1.0);
        let size = big * (0.52 + 0.48 * near);
        let tone = mix(0x6A6A6A, tint, near);
        let alpha = (1.05 - 0.5 * d.abs()).clamp(0.0, 1.0);
        a.gfx.ui(&labels[i as usize], Rc::new(r.x, y - big, r.w, big * 2.0), size, if near > 0.5 { 600 } else { 400 }, tone, alpha, Align::C);
    }
    a.gfx.pop_clip();
    hit(a, r, Act::Wheel(id));
}

// ---------------------------------------------------------------- shelf

/// Each Shelf tile's place along the row; the row slides with the wheel past four or so.
pub const SHELF_STEP: f32 = 76.0;
const SHELF_ROW_W: f32 = 336.8;
const SHELF_FADE: f32 = 28.0;

/// How far the Shelf row can slide with `n` files on it.
pub fn shelf_scroll_max(n: usize) -> f32 {
    (n as f32 * SHELF_STEP - 6.0 - SHELF_ROW_W).max(0.0)
}

fn shelf_title_end(a: &mut App, lx: f32) -> f32 {
    lx + 49.0 + a.gfx.measure("Shelf", HEAD, 600)
}

/// A drag the Shelf cannot take all of: its ten places, the free ones as dashed boxes, and
/// below them how much still fits.
fn shelf_tight(a: &mut App, lx: f32, items: &[std::path::PathBuf], area: Rc, room: usize) {
    const N: usize = model::SHELF_MAX;
    // "7/10" by the title, where the Clear chip stands otherwise (no use mid-drag).
    let count = format!("{}/{N}", items.len());
    let x = shelf_title_end(a, lx) + 8.0;
    a.gfx.ui(&count, Rc::new(x, 10.2, 60.0, 17.6), SMALL, 400, if room == 0 { 0xD6D6D6 } else { GREY }, 1.0, Align::L);
    const ICON: f32 = 26.0;
    const SLOTS: usize = 10;
    let step = area.w / SLOTS as f32;
    let y = area.y + 12.0;
    // Ten small places: the newest files, a "+N" for the rest, then the free places dashed.
    let more = items.len().saturating_sub(SLOTS);
    for i in 0..SLOTS {
        let c = area.x + step * (i as f32 + 0.5);
        let ib = Rc::new(c - ICON / 2.0, y, ICON, ICON);
        if more > 0 && i == SLOTS - 1 {
            a.gfx.fill_rrect(ib, 5.0, 0x2A2A2A, 1.0);
            a.gfx.ui(&format!("+{}", more + 1), ib, 10.5, 600, 0xD6D6D6, 1.0, Align::C);
            continue;
        }
        match items.get(i) {
            Some(p) => match a.shelf_icon(p) {
                (Some(bmp), _) => a.gfx.image_fit(&bmp, ib, 0.0, 1.0, false),
                _ => a.gfx.fill_rrect(ib, 5.0, 0x2A2A2A, 1.0),
            },
            None => a.gfx.stroke_rrect_dashed(ib, 5.0, 1.2, 3.0, 2.5, 0xBDBDBD, 1.0),
        }
    }
    let (text, tone) = if room == 0 { (format!("Shelf full · {N} max"), 0xBDBDBD) } else { (format!("Room for {room} more"), WHITE) };
    let w = a.gfx.measure(&text, SMALL, 400) + 28.0;
    let chip = Rc::new(area.cx() - w / 2.0, y + ICON + 16.0, w, 22.0);
    a.gfx.fill_rrect(chip, 11.0, 0x1E1E1E, 1.0);
    a.gfx.ui(&text, chip, SMALL, 400, tone, 1.0, Align::C);
}

fn shelf(a: &mut App, lx: f32) {
    title(a, lx, "Shelf");
    let area = Rc::new(lx + 43.6, 32.0, SHELF_ROW_W, 91.2);
    let drag = a.drag_over;
    let items = a.shelf.items.clone();
    let room = a.shelf.room();
    if drag && !items.is_empty() && a.drag_count > room {
        shelf_tight(a, lx, &items, area, room);
        return;
    }
    if drag && !items.is_empty() {
        // Nusa keeps the Clear chip next to the title while files are dragged in.
        let clear = clear_chip(a, lx);
        a.gfx.fill_rrect(clear, 8.8, 0x2A2A2A, 1.0);
        a.gfx.ui("Clear", clear, 10.0, 600, 0xD6D6D6, 1.0, Align::C);
    }
    if items.is_empty() || drag {
        if drag {
            // Items already on the shelf stay faintly visible behind the drop frame.
            let n = items.len().min(crate::model::SHELF_MAX);
            if n > 0 {
                let base = a.gfx.alpha;
                a.gfx.alpha = base * 0.22;
                a.gfx.push_clip(area);
                for (i, p) in items.iter().take(n).enumerate() {
                    let tile = Rc::new(area.x + i as f32 * SHELF_STEP - a.shelf_scroll.x, area.y, SHELF_STEP - 6.0, area.h - 4.0);
                    if tile.x + tile.w < area.x || tile.x > area.x + area.w {
                        continue;
                    }
                    if let (Some(bmp), _) = a.shelf_icon(p) {
                        a.gfx.image_fit(&bmp, Rc::new(tile.cx() - 20.0, tile.y + 8.0, 40.0, 40.0), 0.0, 1.0, false);
                    }
                    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    a.gfx.ui(&name, Rc::new(tile.x + 2.0, tile.y + 53.0, tile.w - 4.0, 14.0), 10.5, 600, WHITE, 1.0, Align::C);
                }
                a.gfx.pop_clip();
                a.gfx.alpha = base;
            }
            // Nusa keeps the target itself near-neutral charcoal.  The colour
            // comes from the broad violet aurora behind it, not from a blue
            // fill laid over every shelf item.
            // The drop frame is a translucent grey layer: dark at the top, lifting the blue below.
            a.gfx.fill_rrect_vgrad(area, 12.0, (0x18191C, 0.62), (0x2E2E2E, 0.62));
            drag_bloom(a, area);
        } else {
            a.gfx.fill_rrect(area, 12.0, 0x0B0B0B, 1.0);
            drop_bloom(a, area);
        }
        a.gfx.stroke_rrect_dashed(area, 12.0, 1.2, 4.0, 3.0, if drag { 0xBDBDBD } else { 0x5A5A5A }, 1.0);
        let tint = if drag { [0xFFFFFF, 0x8A8A8A, 0xFFFFFF] } else { [0x777777, 0x4D4D4D, 0xDADADA] };
        icon::tray(&a.gfx, area.cx(), 58.4, tint, 1.0);
        let t = if drag { "Release to add" } else { "Drop files here" };
        a.gfx.ui(t, Rc::new(area.x, 88.0, area.w, 18.0), 13.0, 400, WHITE, 1.0, Align::C);
        if !drag {
            a.gfx.ui("Drag out anytime · click to open", Rc::new(area.x, 104.4, area.w, 13.0), 9.5, 400, 0x777777, 1.0, Align::C);
        }
        return;
    }
    // The empty drop area dissolves under the newly dropped items.
    if let Some((start, _, _)) = a.ripple {
        let k = (1.0 - start.elapsed().as_secs_f32() / 0.16).clamp(0.0, 1.0);
        if k > 0.0 {
            let base = a.gfx.alpha;
            a.gfx.alpha = base * k;
            a.gfx.stroke_rrect_dashed(area, 12.0, 1.2, 4.0, 3.0, 0xBDBDBD, 0.6);
            icon::tray(&a.gfx, area.cx(), 58.4, [0x777777, 0x4D4D4D, 0xDADADA], 0.6);
            a.gfx.ui("Drop files here", Rc::new(area.x, 88.0, area.w, 18.0), 13.0, 400, WHITE, 0.5, Align::C);
            a.gfx.alpha = base;
        }
    }
    let clear = clear_chip(a, lx);
    let h = btn(a, clear, 0.08);
    a.gfx.fill_rrect(clear, 8.8, mix(0x2A2A2A, 0x444444, h), 1.0);
    a.gfx.ui("Clear", clear, 10.0, 600, 0xD6D6D6, 1.0, Align::C);
    a.gfx.unzoom();
    hit(a, clear, Act::ShelfClear);

    let n = items.len().min(crate::model::SHELF_MAX);
    let step = SHELF_STEP;
    let scroll = a.shelf_scroll.x;
    let clip = Rc::new(area.x, area.y - 4.0, area.w, area.h + 8.0);
    a.gfx.push_clip(clip);
    for (i, p) in items.iter().take(n).enumerate() {
        let tile = Rc::new(area.x + i as f32 * step - scroll, area.y, step - 6.0, area.h - 4.0);
        // Tiles slid out of the row are neither drawn nor clickable; one cut by an edge only
        // answers on its visible part.
        let (l, r) = (tile.x.max(area.x), (tile.x + tile.w).min(area.x + area.w));
        if r - l < 1.0 {
            continue;
        }
        let seen = Rc::new(l, tile.y, r - l, tile.h);
        // Freshly dropped items fade and rise into place.
        let enter = a.ripple.map(|r| ((r.0.elapsed().as_secs_f32() - 0.02) / 0.12).clamp(0.0, 1.0)).unwrap_or(1.0);
        let (base, off) = (a.gfx.alpha, a.gfx.off);
        a.gfx.alpha = base * enter;
        a.gfx.offset(off.0, off.1 + 8.0 * (1.0 - enter).powi(2));
        let hot = hovering(a, seen);
        let h = btn(a, seen, 0.05);
        if h > 0.0 {
            a.gfx.fill_rrect(tile, 10.0, 0x2A2A2A, 0.6 * h.min(1.0));
        }
        let ib = Rc::new(tile.cx() - 20.0, tile.y + 8.0, 40.0, 40.0);
        let (bmp, size) = a.shelf_icon(p);
        if let Some(bmp) = bmp {
            a.gfx.image_fit(&bmp, ib, 0.0, 1.0, false);
        } else {
            a.gfx.fill_rrect(ib, 6.0, 0x2A2A2A, 1.0);
            let ext = p.extension().map(|e| e.to_string_lossy().to_uppercase()).unwrap_or_else(|| "FILE".into());
            let ext: String = ext.chars().take(4).collect();
            a.gfx.ui(&ext, ib, 10.5, 600, WHITE, 0.9, Align::C);
        }
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        a.gfx.ui(&name, Rc::new(tile.x + 2.0, tile.y + 53.0, tile.w - 4.0, 14.0), 10.5, 600, WHITE, 1.0, Align::C);
        a.gfx.ui(&size, Rc::new(tile.x + 2.0, tile.y + 67.0, tile.w - 4.0, 12.0), 9.0, 400, GREY, 1.0, Align::C);
        a.gfx.unzoom();
        a.gfx.offset(off.0, off.1);
        a.gfx.alpha = base;
        hit(a, seen, Act::ShelfOpen(i));
        if hot && tile.x + tile.w <= area.x + area.w + 0.5 {
            icon::x_circle(&a.gfx, tile.x + tile.w - 7.0, tile.y + 7.0, 7.0, 0x4A4A4A, WHITE, 1.0);
            hit(a, Rc::new(tile.x + tile.w - 15.0, tile.y - 1.0, 16.0, 16.0), Act::ShelfRemove(i));
        }
    }
    // Files out of sight on a side: that edge of the row fades out.
    let max = shelf_scroll_max(n);
    let (left, right) = ((scroll / 30.0).clamp(0.0, 1.0), ((max - scroll) / 30.0).clamp(0.0, 1.0));
    if left > 0.01 {
        a.gfx.fill_rrect_hgrad(Rc::new(area.x, clip.y, SHELF_FADE, clip.h), 0.0, (0x000000, left), (0x000000, 0.0));
    }
    if right > 0.01 {
        a.gfx.fill_rrect_hgrad(Rc::new(area.x + area.w - SHELF_FADE, clip.y, SHELF_FADE, clip.h), 0.0, (0x000000, 0.0), (0x000000, right));
    }
    a.gfx.pop_clip();
    // A drop that did not all fit says so for a moment, under the files.
    if let Some((at, took, refused)) = a.shelf_note {
        let t = at.elapsed().as_secs_f32();
        let k = ((crate::SHELF_NOTE.as_secs_f32() - t) / 0.8).clamp(0.0, 1.0) * (t / 0.15).clamp(0.0, 1.0);
        if k > 0.0 {
            let text = if took == 0 { format!("Shelf full · {refused} didn't fit") } else { format!("Took {took} · {refused} didn't fit") };
            a.gfx.ui(&text, Rc::new(area.x, 111.0, area.w, 14.0), 10.5, 400, GREY, k, Align::C);
        }
    }
}

/// Bottom-edge light of the panel during a drag and after a drop. Colours are sampled from
/// Nusa's own screen recording (effectvid/new4, frame 100, bottom row) across the panel width;
/// each column rises from black near the top to that colour at the bottom edge.
const AURORA: [u32; 13] = [
    0x072635, 0x0E2A41, 0x152F50, 0x1C365B, 0x213E6D, 0x284476, 0x2B4477, 0x273B70, 0x223262, 0x1C2A53, 0x1A1D43, 0x141336, 0x161236,
];

fn aurora(a: &mut App, _focus_x: f32, _focus_y: f32, s: f32) {
    if s <= 0.01 {
        return;
    }
    let panel = Rc::new(crate::WIN_W / 2.0 - a.sw.x / 2.0, 0.0, a.sw.x, a.sh.x);
    // Columns sampled at panel x = 12.5 + 40·k px of a 525 px panel; the gradient spans that range.
    let x0 = panel.x + 12.5 / 1.25;
    let x1 = x0 + 480.0 / 1.25;
    let band = Rc::new(panel.x, panel.y + 18.0, panel.w, panel.h - 18.0);
    a.gfx.push_clip(band);
    a.gfx.fill_rrect(Rc::new(panel.x, band.y, x0 - panel.x + 0.5, band.h), 0.0, AURORA[0], s);
    a.gfx.fill_rrect(Rc::new(x1 - 0.5, band.y, panel.x + panel.w - x1 + 0.5, band.h), 0.0, AURORA[AURORA.len() - 1], s);
    a.gfx.fill_rect_hstops(Rc::new(x0, band.y, x1 - x0, band.h), &AURORA, s);
    // Black from the top fades out toward the bottom edge, leaving the colour only below.
    a.gfx.fill_rrect_vgrad(band, 0.0, (0x000000, 1.0), (0x000000, 0.0));
    a.gfx.pop_clip()
}

/// A single moving, edgeless highlight above the dimmed shelf content.
/// It deliberately has neither a core dot nor a drawn outline.
fn drag_bloom(a: &mut App, area: Rc) {
    if !a.drag_over {
        return;
    }
    let x = a.blob.0.x.clamp(area.x + 12.0, area.x + area.w - 12.0);
    let y = a.blob.1.x.clamp(area.y + 12.0, area.y + area.h - 12.0);
    let strength = 0.72 + 0.28 * a.drag_energy;
    // Measured on the Nusa recording: a broad, faint grey lift (peak ≈ +0x20 over the frame).
    a.gfx.glow_ellipse(x, y + 10.0, 90.0, 52.0, 0xB4B8C6, 0.16 * strength);
    a.gfx.glow_ellipse(x, y + 6.0, 48.0, 30.0, 0xD0D2DA, 0.08 * strength);
}

/// The bright charge left at the drop point, after the drag cursor disappears.
fn drop_bloom(a: &mut App, area: Rc) {
    let Some((start, x, y)) = a.ripple else { return };
    let t = start.elapsed().as_secs_f32();
    let linger = (1.0 - t / 2.65).clamp(0.0, 1.0).powf(1.35);
    if linger <= 0.01 {
        return;
    }
    let x = x.clamp(area.x + 12.0, area.x + area.w - 12.0);
    let y = y.clamp(area.y + 12.0, area.y + area.h - 12.0);
    let pos = ((x - area.x) / area.w).clamp(0.0, 1.0);
    let tint = if pos < 0.5 { mix(0x0B6179, 0x727787, pos * 2.0) } else { mix(0x727787, 0x5A348B, (pos - 0.5) * 2.0) };
    let spread = (1.0 - linger) * 72.0;
    // Keep a coloured charge at the landing point instead of letting the
    // white ripple wash the whole effect into a neutral halo.
    a.gfx.glow_ellipse(x, y, 122.0 + t * 18.0, 38.0 + t * 7.0, tint, 0.22 * linger);
    a.gfx.glow_ellipse(x - spread * 0.55, y + 3.0, 92.0 + t * 10.0, 27.0, 0x0B6179, 0.15 * linger);
    a.gfx.glow_ellipse(x + spread * 0.55, y - 2.0, 96.0 + t * 10.0, 30.0, 0x5A348B, 0.18 * linger);
    a.gfx.glow_ellipse(x, y, 54.0 + t * 8.0, 25.0 + t * 5.0, tint, 0.42 * linger);
    a.gfx.glow_ellipse(x, y, 25.0 + t * 4.0, 14.0 + t * 3.0, 0xE5E6ED, 0.16 * linger);
}

/// Brightening on top of the aurora, in its own colours: under the cursor while dragging (stronger
/// the nearer the bottom edge), and widening from the drop point after a drop.
fn aurora_highlight(a: &mut App, x: f32, s: f32) {
    if s <= 0.95 {
        return;
    }
    let panel = Rc::new(crate::WIN_W / 2.0 - a.sw.x / 2.0, 0.0, a.sw.x, a.sh.x);
    let f = ((x - panel.x) / panel.w).clamp(0.0, 1.0);
    let p = ((f * 525.0 - 12.5) / 40.0).clamp(0.0, (AURORA.len() - 1) as f32);
    let k = (p.floor() as usize).min(AURORA.len() - 2);
    let base = mix(AURORA[k], AURORA[k + 1], p - k as f32);
    // Same hue, more light: scale the channels instead of mixing in white.
    let lift = |c: u32, k: f32| {
        let ch = |sh: u32| ((((c >> sh) & 255) as f32 * k).min(255.0) as u32) << sh;
        ch(16) | ch(8) | ch(0)
    };
    let bright = lift(base, 2.6);
    let hot = lift(base, 3.6);
    let spread = a.ripple.filter(|_| !a.drag_over).map(|r| 1.0 + 1.6 * (r.0.elapsed().as_secs_f32() / 1.2).min(1.0)).unwrap_or(1.0);
    let bottom = panel.y + panel.h;
    // Only the part above the normal level lights up: far from the edge it looks exactly like Nusa.
    let amt = ((s - 0.95) / 0.65).clamp(0.0, 1.0);
    a.gfx.glow_ellipse(x, bottom + 6.0, panel.w * 0.30 * spread, panel.h * 0.70, bright, 0.95 * amt);
    a.gfx.glow_ellipse(x, bottom + 4.0, panel.w * 0.15 * spread, panel.h * 0.42, hot, 0.8 * amt);
}

/// Aurora strength and focus for the shelf.  A drag retains a bright baseline
/// even while the cursor is motionless; movement only adds a little energy.
fn aurora_state(a: &App) -> (f32, f32, f32) {
    let lvl = a.aurora_lvl.x.clamp(0.0, 1.6);
    match a.ripple {
        Some((_, x, y)) if !a.drag_over => (x, y, lvl),
        _ => (a.blob.0.x, a.blob.1.x, lvl),
    }
}

/// After a drop, water-like ripples spread from the drop point. Timed on Nusa's recording
/// (effectvid/new4, frames 263–269): a packet of six rings ~14 u apart leads a disc that grows to
/// ~240 u in about 0.25 s and is gone by 0.3 s; the bottom light then simply fades.
fn drop_wave(a: &mut App) {
    let Some((start, x, y)) = a.ripple else { return };
    let t = start.elapsed().as_secs_f32();
    if t >= 3.0 {
        a.ripple = None;
        return;
    }
    let q = t / 0.75;
    if q >= 1.0 {
        return;
    }
    let front = 30.0 + 210.0 * (1.0 - (1.0 - q).powf(1.4));
    let fade = (1.0 - q).powf(1.2);
    const GAP: f32 = 13.0;
    let mut r = front;
    let mut rings = 0;
    while r > 4.0 && rings < 6 {
        rings += 1;
        let k = r / front;
        let al = (0.15 + 0.15 * k) * fade;
        // Soft, wide bands (a lit band with a brighter middle), as on the recording.
        a.gfx.stroke_circle(x, y, r, 9.0, 0xC9CDE6, al * 0.75);
        a.gfx.stroke_circle(x, y, r, 4.0, 0xE2E5F3, al * 0.6);
        r -= GAP;
    }
}

// ---------------------------------------------------------------- to-do

/// A switch: the knob glides and the track warms to green. `key` names what it switches, so it
/// keeps its motion wherever a list has moved it to.
/// The lit colour of a switch: Care's green, and its brighter self under the pointer.
const SWITCH_GREEN: (u32, u32) = (0x2F8F63, 0x3FB27E);

fn toggle(a: &mut App, r: Rc, key: (i32, i32), on: bool, act: Act, tint: (u32, u32)) {
    let entry = a.switches.entry(key).or_insert((crate::Spring::new(if on { 1.0 } else { 0.0 }, 300.0, 30.0), on));
    entry.1 = on;
    let k = entry.0.x.clamp(0.0, 1.0);
    let h = btn(a, r, 0.04).clamp(0.0, 1.0);
    let off = mix(0x2C2C2F, 0x48484C, h);
    let lit = mix(tint.0, tint.1, h);
    a.gfx.fill_rrect(r, r.h / 2.0, mix(off, lit, k), 1.0);
    let knob = r.x + r.h / 2.0 + (r.w - r.h) * k;
    let radius = r.h / 2.0 - 3.0;
    let tone = mix(0xB4B4B8, 0xF2F2F6, k);
    if a.settings.glass {
        // Liquid glass (after LiquidToggle): pressed or moving, the knob grows half again and turns
        // to clear glass; it squashes with its speed, smoothed so it never flickers.
        let v = a.switches.get(&key).map_or(0.0, |e| e.0.v);
        let smooth = a.toggle_speed.entry(key).or_insert(0.0);
        *smooth += (v - *smooth) * 0.25;
        let (sx, sy) = toggle_squash(*smooth);
        let moving = smooth.abs() > 0.05;
        let press = (((h - 0.3) / 0.7).clamp(0.0, 1.0)).max(if moving { 1.0 } else { 0.0 });
        let grow = 1.0 + 0.5 * press;
        let (kw, kh) = (2.0 * radius * grow * sx * 1.3, 2.0 * radius * grow * sy);
        let thumb = Rc::new(knob - kw / 2.0, r.cy() - kh / 2.0, kw, kh);
        if press > 0.05 {
            // The track shows through the clear knob.
            let base = a.gfx.alpha;
            a.gfx.alpha = base * press;
            glass(&a.gfx, thumb, kh / 2.0, h);
            a.gfx.alpha = base;
        }
        a.gfx.fill_rrect(thumb, kh / 2.0, tone, 1.0 - 0.85 * press);
        if moving {
            a.kick();
        }
    } else {
        a.gfx.fill_circle(knob, r.cy(), radius, radius, tone, 1.0);
    }
    a.gfx.unzoom();
    hit(a, r, act);
}

fn old_notes(a: &mut App, lx: f32) {
    back_title(a, lx, "Old notes", Act::OldNotes(false));
    let top = a.reminder_top;
    let notes: Vec<String> = a.todo.tasks.iter().map(|t| t.title.clone()).collect();
    for (n, note) in notes.iter().enumerate().skip(top).take(LIST_ROWS) {
        let cy = LIST_Y + (n - top) as f32 * LIST_GAP;
        let slid = swipe_of(a, &format!("n:{n}"));
        a.gfx.ui(note, Rc::new(lx + 30.0, cy - 12.0, 270.0, 24.0), BODY, 400, 0xD8D8D8, 1.0, Align::L);
        if slid < 0.5 {
            head_button(a, Rc::new(lx + LIST_RIGHT - 24.0, cy - 12.0, 24.0, 24.0), Act::RowTrash(2, n), |g, x, y| icon::trash(g, x, y, WHITE, 1.0));
        }
        if slid > 0.01 {
            delete_button(a, lx + LIST_RIGHT, cy, 72.0, slid);
        }
    }
    scrollbar(a, lx, top, notes.len());
}

fn todo(a: &mut App, lx: f32) {
    title(a, lx, "Reminders");
    head_button(a, Rc::new(lx + SLOT_ADD.0, 7.0, 24.0, 24.0), Act::ReminderAdd, |g, x, y| icon::plus(g, x, y, 5.0, 1.8, WHITE, 1.0));
    let old = a.todo.tasks.len();
    let rows = a.reminders();
    if rows.is_empty() && old == 0 {
        pic(a, "t_todo", lx + 210.0, 70.0, 30.0, REMIND, 1.0);
        a.gfx.ui("No reminders yet · Press + to add one", Rc::new(lx, 92.0, 420.0, 20.0), BODY, 400, 0xB8B8BC, 1.0, Align::C);
        return;
    }
    let now = chrono::Local::now();
    if rows.len() == 1 && old == 0 {
        hero(a, lx, 82.0, &rows[0], now);
    } else {
        reminder_list(a, lx, &rows, old, now);
    }
}

/// What a reminder says about when it rings: "in 2 h 10 min · Mon–Fri", or "Off · Mon–Fri".
fn reminder_when(a: &App, r: &crate::luma::config::CustomReminder, now: chrono::DateTime<chrono::Local>) -> (String, String) {
    let days = days_label(&days_of(r));
    let soon = a.brain.reminder_in(&r.id, now).map(in_words);
    (if r.enabled { soon.unwrap_or_default() } else { "Off".to_string() }, days)
}

/// A lone reminder in the middle of the page: its time large, its name, and when it rings next.
/// Pointing at it brings out its switch and a trash; the trash asks first.
fn hero(a: &mut App, lx: f32, cy: f32, r: &crate::luma::config::CustomReminder, now: chrono::DateTime<chrono::Local>) {
    let band = Rc::new(lx + 24.0, cy - 29.0, 372.0, 58.0);
    let asking = swipe_of(a, &format!("r:{}", r.id));
    let near = hovering(a, band) || asking > 0.05;
    let k = flip(a, 50, near);
    if k > 0.01 {
        a.gfx.fill_rrect(band, 18.0, 0xFFFFFF, 0.06 * k);
    }
    let on = r.enabled;
    let (soon, days) = reminder_when(a, r, now);
    let when = if soon.is_empty() { days.clone() } else { format!("{soon} · {days}") };
    // As wide as its widest wording, on or off, so switching it does not shift it.
    let name_w = a.gfx.measure(&r.title, 17.0, 600);
    let widest = [format!("in 22 h 59 min · {days}"), format!("Off · {days}"), when.clone()]
        .iter()
        .map(|t| a.gfx.measure(t, SMALL, 400))
        .fold(name_w, f32::max);
    let tw = a.gfx.measure(&r.time, 40.0, 600);
    let text = widest.min(230.0 - 60.0 * k);
    let x0 = lx + 210.0 - 50.0 * k - (tw + 28.0 + text) / 2.0;
    let base = a.gfx.alpha;
    a.gfx.alpha = base * (1.0 - 0.55 * asking);
    let lit = flip(a, 51, on);
    a.gfx.ui(&r.time, Rc::new(x0, cy - 24.0, tw + 4.0, 48.0), 40.0, 600, mix(0x5A5A5E, REMIND, lit), 1.0, Align::L);
    a.gfx.line(x0 + tw + 14.0, cy - 15.0, x0 + tw + 14.0, cy + 15.0, 1.0, 0x5A5A5E, 0.9);
    let tx = x0 + tw + 28.0;
    a.gfx.ui(&r.title, Rc::new(tx, cy - 20.0, text + 2.0, 22.0), 17.0, 600, mix(0x8A8A8E, WHITE, lit), 1.0, Align::L);
    a.gfx.ui(&when, Rc::new(tx, cy + 3.0, text + 2.0, 16.0), SMALL, 400, 0x8E8E93, 1.0, Align::L);
    a.gfx.alpha = base;
    hit(a, band, Act::ReminderEdit(0));
    if k > 0.01 && asking < 0.05 {
        a.gfx.alpha = base * k;
        reminder_buttons(a, lx + 356.0, cy, 0, r);
        a.gfx.alpha = base;
    }
    delete_question(a, Rc::new(lx + 236.0, cy - 27.0, 158.0, 54.0), asking);
}

/// The switch and the trash of reminder `i`, the trash centred on `x`.
fn reminder_buttons(a: &mut App, x: f32, cy: f32, i: usize, r: &crate::luma::config::CustomReminder) {
    let id = (1, r.id.bytes().fold(7i32, |h, b| h.wrapping_mul(31).wrapping_add(b as i32)));
    toggle(a, Rc::new(x - 48.0 - SWITCH.0 / 2.0, cy - SWITCH.1 / 2.0, SWITCH.0, SWITCH.1), id, r.enabled, Act::ReminderSwitch(i), (REMIND, 0x5A92FF));
    head_button(a, Rc::new(x - 14.0, cy - 14.0, 28.0, 28.0), Act::RowTrash(0, i), |g, x, y| icon::trash(g, x, y, WHITE, 1.0));
}

/// "Delete?" with Cancel and a red Delete, in `panel`, as `k` of it has come in.
fn delete_question(a: &mut App, panel: Rc, k: f32) {
    if k <= 0.01 {
        return;
    }
    let base = a.gfx.alpha;
    a.gfx.alpha = base * k;
    a.gfx.fill_rrect(panel, panel.h.min(32.0) / 2.0, 0x1C1C1E, 1.0);
    let tall = panel.h > 40.0;
    let (cancel, del) = if tall {
        a.gfx.ui("Delete?", Rc::new(panel.x, panel.y + 3.0, panel.w, 18.0), SMALL, 600, WHITE, 1.0, Align::C);
        (Rc::new(panel.x + 8.0, panel.y + 24.0, 68.0, 24.0), Rc::new(panel.x + 82.0, panel.y + 24.0, 68.0, 24.0))
    } else {
        a.gfx.ui("Delete?", Rc::new(panel.x + 10.0, panel.y, 60.0, panel.h), SMALL, 600, WHITE, 1.0, Align::L);
        let y = panel.cy() - 11.0;
        (Rc::new(panel.x + panel.w - 136.0, y, 62.0, 22.0), Rc::new(panel.x + panel.w - 68.0, y, 62.0, 22.0))
    };
    let (hc, hd) = (btn(a, cancel, 0.06), btn(a, del, 0.06));
    a.gfx.fill_rrect(cancel, 11.0, mix(0x3A3A3E, 0x4A4A4E, hc), 1.0);
    a.gfx.ui("Cancel", cancel, SMALL, 500, WHITE, 1.0, Align::C);
    a.gfx.unzoom();
    a.gfx.fill_rrect(del, 11.0, mix(0xFF453A, 0xFF6B60, hd), 1.0);
    a.gfx.ui("Delete", del, SMALL, 600, WHITE, 1.0, Align::C);
    a.gfx.unzoom();
    a.gfx.alpha = base;
    if k > 0.5 {
        hit(a, panel, Act::None);
        hit(a, cancel, Act::RowCancel);
        hit(a, del, Act::RowDelete);
    }
}

/// Two or more reminders: a list that scrolls with the wheel, fading at an edge with more beyond.
fn reminder_list(a: &mut App, lx: f32, rows: &[crate::luma::config::CustomReminder], old: usize, now: chrono::DateTime<chrono::Local>) {
    const FIRST: f32 = 56.0;
    let total = rows.len() + (old > 0) as usize;
    let pos = a.list_pos.x;
    let top = a.reminder_top;
    let view = Rc::new(lx, 36.0, 420.0, 96.0);
    a.gfx.push_clip(view);
    for (n, r) in rows.iter().enumerate() {
        let cy = FIRST + (n as f32 - pos) * LIST_GAP;
        if !(14.0..160.0).contains(&cy) {
            continue;
        }
        let shown = (top..top + LIST_ROWS).contains(&n);
        let band = Rc::new(lx + 22.0, cy - 17.0, 376.0, 34.0);
        let asking = swipe_of(a, &format!("r:{}", r.id));
        let near = shown && (hovering(a, band) || asking > 0.05);
        let key = 70 + (r.id.bytes().fold(7i32, |h, b| h.wrapping_mul(31).wrapping_add(b as i32)) & 0xFFFF);
        let k = flip(a, key, near);
        if k > 0.01 {
            a.gfx.fill_rrect(band, 13.0, 0xFFFFFF, 0.06 * k);
        }
        let lit = flip(a, key + 0x10000, r.enabled);
        let base = a.gfx.alpha;
        a.gfx.alpha = base * (1.0 - 0.55 * asking);
        a.gfx.ui(&r.time, Rc::new(lx + 30.0, cy - 13.0, 64.0, 26.0), 20.0, 600, mix(0x5A5A5E, REMIND, lit), 1.0, Align::L);
        let (soon, days) = reminder_when(a, r, now);
        a.gfx.ui(&r.title, Rc::new(lx + 102.0, cy - 15.0, 200.0, 18.0), 14.0, 500, mix(0x8A8A8E, WHITE, lit), 1.0, Align::L);
        a.gfx.ui(&days, Rc::new(lx + 102.0, cy + 2.0, 200.0, 14.0), 11.0, 400, 0x8E8E93, 1.0, Align::L);
        a.gfx.ui(&soon, Rc::new(lx + 260.0, cy - 10.0, 132.0, 20.0), SMALL, 400, 0x8E8E93, 1.0 - k, Align::R);
        a.gfx.alpha = base;
        if shown {
            hit(a, band, Act::ReminderEdit(n));
            if k > 0.01 && asking < 0.05 {
                a.gfx.alpha = base * k;
                reminder_buttons(a, lx + 378.0, cy, n, r);
                a.gfx.alpha = base;
            }
            delete_question(a, Rc::new(lx + 196.0, cy - 15.0, 200.0, 30.0), asking);
        }
    }
    if old > 0 {
        let cy = FIRST + (rows.len() as f32 - pos) * LIST_GAP;
        let row = Rc::new(lx + 22.0, cy - 17.0, 376.0, 34.0);
        let h = btn(a, row, 0.05);
        a.gfx.fill_rrect(row, 13.0, 0xFFFFFF, 0.06 * h.min(1.0));
        a.gfx.ui(&format!("Old notes ({old})"), Rc::new(lx + 30.0, cy - 11.0, 260.0, 22.0), BODY, 400, 0x8A8A8A, 1.0, Align::L);
        a.gfx.unzoom();
        if (top..top + LIST_ROWS).contains(&rows.len()) {
            hit(a, row, Act::OldNotes(true));
        }
    }
    // More below (or above): that edge fades into the black.
    let below = (total as f32 - LIST_ROWS as f32 - pos).clamp(0.0, 1.0);
    let above = pos.clamp(0.0, 1.0);
    if below > 0.01 {
        a.gfx.fill_rrect_vgrad(Rc::new(lx, view.y + view.h - 24.0, 420.0, 24.0), 0.0, (0x000000, 0.0), (0x000000, below));
    }
    if above > 0.01 {
        a.gfx.fill_rrect_vgrad(Rc::new(lx, view.y, 420.0, 16.0), 0.0, (0x000000, above), (0x000000, 0.0));
    }
    a.gfx.pop_clip();
}

/// A reminder's weekdays, Monday first.
fn days_of(r: &crate::luma::config::CustomReminder) -> [bool; 7] {
    let mut on = [false; 7];
    for d in &r.days {
        on[*d as usize % 7] = true;
    }
    on
}

/// "in 5 min", "in 2 h 10 min", "in 1 day", "in 3 days".
fn in_words(secs: i64) -> String {
    match secs {
        s if s >= 2 * 86_400 => format!("in {} days", s / 86_400),
        s if s >= 86_400 => "in 1 day".to_string(),
        s if s >= 3600 => format!("in {} h {} min", s / 3600, s % 3600 / 60),
        s => format!("in {} min", (s / 60).max(1)),
    }
}

/// The form's three repeats: Every day, Mon–Fri, Once (no days).
pub const REPEATS: [[bool; 7]; 3] = [PRESETS[0], PRESETS[1], [false; 7]];

/// Which of the three repeats `days` is, if any (a reminder made with other days is none).
pub fn repeat_choice(days: [bool; 7]) -> Option<usize> {
    REPEATS.iter().position(|r| *r == days)
}

/// When a reminder repeats, in a few words.
fn days_label(days: &[bool; 7]) -> String {
    const NAMES: [&str; 7] = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];
    match days {
        [true, true, true, true, true, true, true] => "Daily".into(),
        [true, true, true, true, true, false, false] => "Mon–Fri".into(),
        [false, false, false, false, false, true, true] => "Sat–Sun".into(),
        _ if !days.contains(&true) => "Once".into(),
        _ => NAMES.iter().zip(days).filter(|(_, on)| **on).map(|(n, _)| *n).collect::<Vec<_>>().join(" · "),
    }
}

/// Row height of the big clock in the reminder form.
pub const CLOCK_ROW: f32 = 30.0;

/// A round button at the side of the reminder form; the last one is a green tick.
fn form_button(a: &mut App, cx: f32, cy: f32, kind: FormButton, act: Act) {
    const R: f32 = 17.0;
    let r = Rc::new(cx - R, cy - R, 2.0 * R, 2.0 * R);
    let h = btn(a, r, 0.1);
    let fill = match kind {
        FormButton::Done => mix(BUTTON, 0x48484C, h),
    };
    a.gfx.fill_circle(cx, cy, R, R, fill, 1.0);
    match kind {
        FormButton::Done => icon::tick(&a.gfx, cx, cy, ON, 1.0),
    }
    a.gfx.unzoom();
    hit(a, r, act);
}

#[derive(Clone, Copy)]
enum FormButton {
    Done,
}

/// The reminder form on one screen, done in three moves: type the name, roll the time, Save.
/// The repeat is one chip that steps through Every day, Weekdays, Weekends and Once. An
/// important day has only its name.
fn form(a: &mut App, lx: f32) {
    let (text, days, editing, date) = {
        let f = a.form.as_ref().unwrap();
        (f.title.clone(), f.days, f.id.is_some(), f.date.clone())
    };
    let heading = match &date {
        Some(d) => format!("Day: {d}"),
        None if editing => "Edit reminder".to_string(),
        None => "New reminder".to_string(),
    };
    back_title(a, lx, &heading, Act::FormBack);
    // Three columns, 20 apart: the name and its repeat, the time, Save.
    let tf = if date.is_some() { Rc::new(lx + 24.0, 62.0, 296.0, 36.0) } else { Rc::new(lx + 24.0, 42.0, 172.0, 34.0) };
    a.gfx.fill_rrect(tf, 14.0, 0x1A1A1C, 1.0);
    a.gfx.stroke_rrect(tf, 14.0, 1.0, 0x3A3A3E, 1.0);
    let inner = Rc::new(tf.x + 14.0, tf.y, tf.w - 40.0, tf.h);
    if text.is_empty() {
        a.gfx.ui(if date.is_some() { "What is this day?" } else { "What should I remind you?" }, inner, SMALL, 400, GREY, 1.0, Align::L);
    } else {
        a.gfx.ui(&text, inner, BODY, 500, WHITE, 1.0, Align::L);
        icon::x_circle(&a.gfx, tf.x + tf.w - 18.0, tf.cy(), 7.0, 0xDADADA, 0x1A1A1C, 1.0);
        hit(a, Rc::new(tf.x + tf.w - 30.0, tf.y, 28.0, tf.h), Act::FormClear);
    }
    if (a.born.elapsed().as_millis() / 530) % 2 == 0 {
        let cw = a.gfx.measure(&text, BODY, 500).min(inner.w);
        a.gfx.line(inner.x + cw + 1.0, tf.y + 9.0, inner.x + cw + 1.0, tf.y + tf.h - 9.0, 1.3, REMIND, 1.0);
    }
    if date.is_none() {
        // The repeat: three plain choices side by side, the one in use lit.
        let chosen = repeat_choice(days);
        let group = Rc::new(lx + 24.0, 88.0, 172.0, 30.0);
        if a.settings.glass {
            liquid(a, group, 15.0, 0.0, None);
        } else {
            a.gfx.fill_rrect(group, 15.0, 0x1A1A1C, 1.0);
        }
        let w = group.w / 3.0;
        for (i, name) in ["Daily", "Mon–Fri", "Once"].into_iter().enumerate() {
            let r = Rc::new(group.x + i as f32 * w + 2.0, group.y + 2.0, w - 4.0, group.h - 4.0);
            let k = flip(a, 55 + i as i32, chosen == Some(i));
            let h = btn(a, r, 0.05);
            a.gfx.fill_rrect(r, 13.0, REMIND, 0.2 * k);
            a.gfx.stroke_rrect(r, 13.0, 1.2, REMIND, k);
            a.gfx.ui(name, r, 12.0, 500, mix(mix(0x8E8E93, 0xC8C8CC, h.clamp(0.0, 1.0)), WHITE, k), 1.0, Align::C);
            a.gfx.unzoom();
            hit(a, r, Act::FormRepeat(i as u8));
        }
        // The time, always there.
        let cx = lx + 268.0;
        let hours: Vec<String> = (0..24).map(|h| format!("{h:02}")).collect();
        let minutes: Vec<String> = (0..60).map(|m| format!("{m:02}")).collect();
        pick_band(a, cx - 52.0, cx + 52.0, 81.0, 38.0);
        wheel_sized(a, WheelId::FormHour, Rc::new(cx - 52.0, 35.0, 44.0, 92.0), &hours, 26.0, REMIND);
        a.gfx.ui(":", Rc::new(cx - 10.0, 61.0, 20.0, 36.0), 26.0, 600, WHITE, 1.0, Align::C);
        wheel_sized(a, WheelId::FormMinute, Rc::new(cx + 8.0, 35.0, 44.0, 92.0), &minutes, 26.0, REMIND);
    }
    // Save: blue once there is a name.
    let ready = !text.trim().is_empty();
    let save = Rc::new(lx + 340.0, 63.0, 62.0, 36.0);
    let h = if ready { btn(a, save, 0.06) } else { 0.0 };
    if a.settings.glass {
        liquid(a, save, 18.0, h, Some(if ready { REMIND } else { 0x3A3A3E }));
    } else {
        a.gfx.fill_rrect(save, 18.0, if ready { mix(REMIND, 0x5A92FF, h) } else { 0x3A3A3E }, 1.0);
    }
    a.gfx.ui("Save", save, BODY, 600, if ready { WHITE } else { 0x8E8E93 }, 1.0, Align::C);
    a.gfx.unzoom();
    if ready {
        hit(a, save, Act::FormNext);
    }
}


#[cfg(test)]
mod tests {
    use super::sun_below;

    #[test]
    fn the_sun_sets_as_time_runs_out() {
        let r = 28.0;
        assert!(sun_below(0.0, r) - r < 0.0, "at the start most of it is up");
        assert!(sun_below(1.0, r) - r > 0.0, "at the end its top is under the line");
        assert!(sun_below(0.3, r) < sun_below(0.6, r));
    }
}
