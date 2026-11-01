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
const WATER: u32 = 0x35D6E8;
pub const EVENING: u32 = 0xFF8A2A;
/// The colour rising behind the reminder form.
pub const CALM: u32 = 0x3E7BFF;
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

/// Where the strip of pages comes to rest once the fingers lift: the nearest page, or the next
/// one along if it was moving fast enough. It is a ring, so there is no end to stop at.
pub fn strip_rest(at: f32, speed: f32) -> f32 {
    let lean = if speed.abs() > 1.5 { 0.4 * speed.signum() } else { 0.0 };
    (at + lean).round()
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
    PomoPlan(u32, u32),
    FormPreset(u8),
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
    FormDay(u8),
    FormDelete,
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
    let target = if pressed { -0.6 } else if hot { 1.0 } else { 0.0 };
    let e = a.hover.entry(key).or_insert((crate::Spring::new(0.0, 320.0, 24.0), 0.0));
    let changed = e.1 != target;
    e.1 = target;
    let h = e.0.x;
    if changed {
        a.kick();
    }
    a.gfx.zoom(r.cx(), r.cy(), 1.0 + grow * h);
    h
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
    a.gfx.notch(cx, w, h, radius, ear, 0x000000, 1.0);
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
        lyrics_backdrop(a, left, w, h, radius, lm);
    }
    if a.glow_mix.x > 0.01 || a.wash > 0.01 || a.focus_mix.x > 0.01 {
        let clipped = a.gfx.push_notch_clip(cx, w, h, radius, ear);
        ambient(a, left, w, h);
        if clipped {
            a.gfx.pop_layer();
        }
    }

    let exp_alpha = a.fade.x.clamp(0.0, 1.0) * ((h - 70.0) / 50.0).clamp(0.0, 1.0);
    let tall = a.toast.as_ref().or(a.last_toast.as_ref()).is_some_and(|t| t.kind.tall()) && a.toast_mix.x > 0.5;
    let fade_from = if tall { 84.0 } else { 36.0 };
    let mut small_alpha = (1.0 - (h - fade_from) / 14.0).clamp(0.0, 1.0) * (1.0 - a.fade.x).clamp(0.0, 1.0);
    // A notification grows into its detail view: the card leaves in the first half of the way, the
    // detail comes in the second, so the two are never on screen together.
    let notice = a.notice_toast() && a.fade.x < 0.01 && !a.hidden;
    let grown = if notice {
        let card_h = a.notice_card_height();
        ((h - card_h) / (crate::NOTICE_OPEN[1] - card_h)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    if notice {
        small_alpha = (1.0 - 2.0 * grown).clamp(0.0, 1.0);
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
        let urgent = a.toast.as_ref().is_some_and(|t| matches!(t.kind, ToastKind::Water | ToastKind::Focus | ToastKind::Break));
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
        let d = (2.0 * grown - 1.0).clamp(0.0, 1.0);
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

/// The focus timer: a red sun going down behind the horizon, with its light on the water below.
/// `left` is how much of the time remains; the used part stays as the reflection and the line.
fn sunset(a: &mut App, cx: f32, cy: f32, r: f32, left: f32) {
    let t = a.born.elapsed().as_secs_f32();
    let horizon = cy + r * 0.42;
    let sun = horizon - r * 0.52 + (1.0 - left) * r * 1.5;
    let height = (horizon - sun).clamp(-r, r);
    let chord = (r * r - height * height).max(0.0).sqrt();
    a.gfx.glow_ellipse(cx, horizon - r * 0.1, r * 1.5, r * 1.1, RED, 0.16 + 0.1 * left);
    a.gfx.push_clip(Rc::new(cx - r - 2.0, cy - r * 2.0, 2.0 * r + 4.0, horizon - (cy - r * 2.0)));
    a.gfx.fill_circle(cx, sun, r, r, RED, 1.0);
    a.gfx.fill_circle(cx - r * 0.12, sun - r * 0.16, r * 0.76, r * 0.76, 0xFF7468, 0.5);
    a.gfx.pop_clip();
    // The light on the water, each band narrower and drifting a little.
    let band = (r * 0.09).max(1.0);
    let gap = (r * 0.2).max(2.4);
    let width = chord.max(r * 0.34);
    for i in 0..3 {
        let k = i as f32;
        let drift = r * 0.05 * (t * 0.9 + k * 1.7).sin();
        let half = width * (1.0 - 0.24 * k) + r * 0.04 * (t * 1.3 + k).sin();
        let y = horizon + band * 0.5 + k * gap;
        a.gfx.fill_rrect(Rc::new(cx + drift - half, y, 2.0 * half, band), band / 2.0, RED, (0.95 - 0.28 * k) * (0.55 + 0.45 * left));
    }
}

/// The small sun that stays in the notch while focus runs, moving with it as it opens.
fn focus_sun(a: &mut App, left: f32, w: f32, h: f32) {
    if a.brain.focus.is_none() || a.live_alpha <= 0.01 {
        return;
    }
    let (time, total, _, _) = a.pomo_view();
    let open = ((h - 30.0) / 110.0).clamp(0.0, 1.0);
    let open = open * open * (3.0 - 2.0 * open);
    let ring = (left + w - 19.0, 14.8);
    let row = (left + (SLOT_LIVE.0 + SLOT_LIVE.1) / 2.0, 19.0);
    let at = (ring.0 + (row.0 - ring.0) * open, ring.1 + (row.1 - ring.1) * open);
    let base = a.gfx.alpha;
    // The Focus page has the big one; two suns would be one too many.
    a.gfx.alpha = a.live_alpha * if a.page == Page::Pomo && a.view == View::Page { 1.0 - open } else { 1.0 };
    sunset(a, at.0, at.1, 8.0, (time / total) as f32);
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
    for i in 0..5 {
        let fi = i as f32;
        let wave = 0.55 * (t * (2.1 + fi * 0.37) + fi * 1.9).sin() + 0.45 * (t * (3.3 - fi * 0.29) + fi * 0.7).sin();
        let bh = (3.0 + 7.4 * (0.5 + 0.5 * wave).clamp(0.0, 1.0)) * mix;
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
    let out = a.jam_x.x.clamp(0.0, 1.0);
    if out > 0.02 {
        let room = crate::JAM_ROOM;
        let x = if a.jam.side < 0.0 { bar - room / 2.0 + 6.0 } else { bar + bar_w + room / 2.0 - 6.0 };
        let base = a.gfx.alpha;
        a.gfx.alpha = base * out;
        a.gfx.push_clip(Rc::new(left, 0.0, w, h));
        mini_eyes(a, x, h / 2.0, 0.085);
        jam_notes(a, x, 31.0, h);
        a.gfx.pop_clip();
        a.gfx.alpha = base;
    }
    if line != a.pill_cur {
        let old = std::mem::replace(&mut a.pill_cur, line.clone());
        a.pill_prev = if old.is_empty() { None } else { Some((std::time::Instant::now(), old)) };
        a.kick();
    }
    // The outgoing line lifts away as the new one rises into place.
    let text = |a: &mut App, s: &str, dy: f32, al: f32| {
        let base = a.gfx.alpha;
        a.gfx.alpha = base * al;
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
        a.gfx.push_clip(Rc::new(x0, by, x1 - x0, (bottom - by).max(0.0)));
        a.gfx.text(&body, Rc::new(x0, by, x1 - x0, 200.0), 13.0, 400, 0xD8D8DC, 1.0, Align::LW, Font::Ui);
        a.gfx.pop_clip();
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
            K::Sleep => {
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
    let drink = a.drink_time();
    if let Some(age) = a.reaction.filter(|r| r.1).map(|r| r.0.elapsed().as_secs_f32() - drink).filter(|age| *age > 0.0) {
        // Little hearts of scan lines drift up from the eyes.
        for i in 0..6 {
            let k = (age - 0.08 * i as f32) / 1.2;
            if !(0.0..1.0).contains(&k) {
                continue;
            }
            let at = (eyes_at + reach * 0.3 + (i as f32 - 2.5) * 13.0 + 6.0 * (k * 9.0 + i as f32).sin(), cy + 8.0 - 42.0 * k);
            let heart: Vec<(f32, f32)> = (0..20)
                .map(|n| {
                    let t = n as f32 / 20.0 * std::f32::consts::TAU;
                    let (x, y) = (16.0 * t.sin().powi(3), 13.0 * t.cos() - 5.0 * (2.0 * t).cos() - 2.0 * (3.0 * t).cos() - (4.0 * t).cos());
                    (at.0 + 0.32 * x, at.1 - 0.32 * y)
                })
                .collect();
            a.gfx.fill_poly(&heart, crate::CHARACTER, (1.0 - k) * (k * 8.0).min(1.0));
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
                None => icon::messenger(&a.gfx, ix, h / 2.0, 19.0, 1.0),
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
fn header(a: &mut App, lx: f32) {
    let time = a.clock().time_label();
    let date = short_date();
    let (tw, dw) = (a.gfx.measure(&time, HEAD, 600), a.gfx.measure(&date, SMALL, 400));
    let x = lx + (SLOT_CLOCK.0 + SLOT_CLOCK.1) / 2.0 - (tw + 8.0 + dw) / 2.0;
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
fn head_button(a: &mut App, r: Rc, act: Act, draw: impl FnOnce(&Gfx, f32, f32)) {
    let h = btn(a, r, 0.15);
    a.gfx.fill_circle(r.cx(), r.cy(), r.w / 2.0, r.w / 2.0, 0xFFFFFF, 0.1 + 0.12 * h);
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
        "Reminders" => ("t_todo", WHITE),
        _ => ("t_shelf", WHITE),
    };
    pic(a, name, lx + 33.0, 19.0, 18.0, tint, 1.0);
    a.gfx.ui(text, Rc::new(lx + 49.0, 8.0, 100.0, 22.0), HEAD, 600, WHITE, 1.0, Align::L);
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

/// Two words in one place: as `k` goes from 0 to 1 the first drifts up and out, the second comes in.
#[allow(clippy::too_many_arguments)]
fn swap_text(a: &mut App, r: Rc, size: f32, weight: u32, hex: (u32, u32), align: Align, words: (&str, &str), k: f32) {
    if k < 0.99 {
        a.gfx.ui(words.0, Rc::new(r.x, r.y - 5.0 * k, r.w, r.h), size, weight, hex.0, 1.0 - k, align);
    }
    if k > 0.01 {
        a.gfx.ui(words.1, Rc::new(r.x, r.y + 5.0 * (1.0 - k), r.w, r.h), size, weight, hex.1, k, align);
    }
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
    if view == View::Page && lyr < 0.99 {
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
    let width = a.sw.x.max(1.0);
    let n = Page::ALL.len() as i32;
    let first = at.floor() as i32;
    let settled = (at - at.round()).abs() < 0.04;
    for i in first..=first + 1 {
        let d = i as f32 - at;
        if d.abs() >= 1.0 {
            continue;
        }
        let p = Page::ALL[i.rem_euclid(n) as usize];
        let hits = a.hits.len();
        a.gfx.offset(ox + d * width, 0.0);
        page(a, p, lx);
        // Buttons are only live on the page at rest in front.
        if !(settled && p == a.page) {
            a.hits.truncate(hits);
        }
    }
    a.gfx.offset(ox, 0.0);
}

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
        let m = a.media.lock().unwrap();
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
        a.gfx.image(&bmp, r, 6.0, 1.0);
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
    let m = a.media.lock().unwrap().clone();
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
    a.gfx.line(lx + 253.2, 22.0, lx + 253.2, 124.0, 1.0, 0x5A5A5A, 0.5);
    // Laid out at the final lyrics width so wrapping never changes while the notch resizes.
    let col = Rc::new(lx + 265.2, 10.0, crate::LYRICS[0] - 265.2 - 22.0, 124.0);
    let rise = 30.0 * (1.0 - k);
    let (status, idx, lines, times, key) = {
        let l = a.lyrics.lock().unwrap();
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
                let from = if a.lyr_anim.is_some() { shown(a.lyr_anim, 0.0) } else { a.lyr_idx.map(scroll_for).unwrap_or(scroll_for(i)) };
                a.lyr_anim = Some((now, from, scroll_for(i), a.lyr_idx));
                a.lyr_idx = Some(i);
                a.kick();
            }
            // No scroll in flight counts as one that finished long ago.
            let settled = now.checked_sub(std::time::Duration::from_secs(10)).unwrap_or(now);
            let anim = a.lyr_anim.unwrap_or((settled, scroll_for(i), scroll_for(i), None));
            let prev = anim.3;
            let since = now.duration_since(anim.0).as_secs_f32();
            // Small text: the current line sits a third of the way down with context above;
            // large text centres it.
            let big = ((size - 15.0) / 11.0).clamp(0.0, 1.0);
            let anchor = col.y + 30.0 + ((col.h - hs[i]) / 2.0 - 30.0).max(0.0) * big + rise;
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
                    hit(a, Rc::new(col.x, top, col.w, bottom - top), Act::Seek((times[j] * 1000.0) as u32));
                }
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
    fn the_wash_rises_without_a_jump_and_fades_away() {
        assert_eq!(wash_curve(0.0, 0.0), 0.0);
        assert!(wash_curve(0.05, 0.0) < 0.1);
        assert!((wash_curve(WASH_RISE, 0.0) - 1.0).abs() < 0.01);
        assert!(wash_curve(WASH_SECS, 0.0).abs() < 0.001);
        assert!((wash_curve(0.0, 0.6) - 0.6).abs() < 0.001, "a second press carries on from where it is");
    }

    #[test]
    fn the_strip_settles_on_a_page_and_runs_round() {
        assert_eq!(strip_rest(2.3, 0.0), 2.0);
        assert_eq!(strip_rest(2.3, 4.0), 3.0);
        assert_eq!(strip_rest(2.7, -4.0), 2.0);
        assert_eq!(strip_rest(5.7, 0.0).rem_euclid(6.0), 0.0);
        assert_eq!(strip_rest(-0.7, 0.0).rem_euclid(6.0), 5.0);
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
    a.gfx.fill_circle(cx, y, R, R, mix(BUTTON, hot, h.clamp(0.0, 1.0)), 1.0);
    draw(&a.gfx, cx, y);
    a.gfx.unzoom();
    a.gfx.ui(label, Rc::new(cx - 34.0, cy + R + 2.0, 68.0, 16.0), SMALL, 500, 0xE0E0E2, 1.0, Align::C);
    if live {
        hit(a, r, act);
    }
}

fn focus_idle(a: &mut App, lx: f32, live: bool) {
    let pomodoro = a.settings.pomodoro;
    // Plain focus or Pomodoro: a pill glides to the one in use.
    let mode = flip(a, 1, pomodoro);
    a.gfx.fill_rrect(Rc::new(lx + 34.0 + mode * 84.0, 42.0, 84.0, 26.0), 13.0, BUTTON, 1.0);
    for (i, (name, on)) in [("Focus", false), ("Pomodoro", true)].into_iter().enumerate() {
        let r = Rc::new(lx + 34.0 + i as f32 * 84.0, 42.0, 84.0, 26.0);
        let lit = if on { mode } else { 1.0 - mode };
        let h = if live { btn(a, r, 0.03) } else { 0.0 };
        a.gfx.ui(name, r, SMALL, 500, mix(0x8E8E93, WHITE, lit.max(h.clamp(0.0, 1.0))), 1.0, Align::C);
        a.gfx.unzoom();
        if live {
            hit(a, r, Act::FocusMode(on));
        }
    }
    let chosen = a.settings.pomo_work;
    let rest = a.settings.pomo_rest;
    const CY: f32 = 98.0;
    let plans: [(u32, u32); 3] = [(25, 5), (50, 10), (90, 15)];
    for (i, (work, pause)) in plans.into_iter().enumerate() {
        let r = Rc::new(lx + 34.0 + i as f32 * 58.0, CY - 15.0, 52.0, 30.0);
        let on = work == chosen && (!pomodoro || pause == rest);
        let k = flip(a, 10 + i as i32, on);
        let h = if live { btn(a, r, 0.06) } else { 0.0 };
        a.gfx.fill_rrect(r, 15.0, mix(BUTTON, 0x48484C, h), 1.0 - k);
        a.gfx.stroke_rrect(r, 15.0, 1.5, RED, k);
        let tone = (mix(0xE0E0E2, RED, k), mix(0xE0E0E2, RED, k));
        swap_text(a, r, 13.5, 600, tone, Align::C, (&work.to_string(), &format!("{work}/{pause}")), mode);
        a.gfx.unzoom();
        if live {
            hit(a, r, if pomodoro { Act::PomoPlan(work, pause) } else { Act::PomoMinutes(work) });
        }
    }
    divider(a, lx + 216.0, 48.0, 112.0);
    let area = Rc::new(lx + 224.0, 36.0, 96.0, 90.0);
    if pomodoro {
        let rounds: Vec<String> = (1..=8).map(|n| if n == 1 { "1 round".to_string() } else { format!("{n} rounds") }).collect();
        wheel_sized(a, WheelId::Rounds, area, &rounds, 18.0);
    } else {
        let lengths: Vec<String> = (0..FOCUS_STEPS).map(|i| format!("{} min", focus_minutes(i))).collect();
        wheel_sized(a, WheelId::FocusMinutes, area, &lengths, 20.0);
    }
    divider(a, lx + 328.0, 48.0, 112.0);
    labelled(a, lx + 368.0, 72.0, "Start", live, Act::PomoToggle, |g, x, y| icon::play(g, x + 1.0, y, 12.0, RED, 1.0));
}

fn focus_running(a: &mut App, lx: f32, live: bool) {
    let (left_s, total_s, running, phase) = a.pomo_view();
    let left = (left_s / total_s).clamp(0.0, 1.0) as f32;
    sunset(a, lx + 104.0, 70.0, 36.0, left);
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
            4..=10 => "Good morning",
            11..=17 => "Good afternoon",
            _ => "Good evening",
        };
        a.gfx.ui(hello_at, Rc::new(lx + 24.0, 100.0, 136.0, 24.0), 16.0, 500, WHITE, 1.0, Align::C);
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
        a.gfx.ui(&text, Rc::new(lx + 226.0, y - 11.0, w, 22.0), 13.0, 400, WHITE, dim, Align::L);
        a.gfx.ui(&time, Rc::new(lx + 340.0, y - 11.0, 56.0, 22.0), 12.0, 400, 0xC8C8CC, dim, Align::R);
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
    if let Some(age) = ended.filter(|e| e.1).map(|e| e.0.elapsed().as_secs_f32()) {
        for i in 0..6 {
            let k = (age - 0.08 * i as f32) / 1.2;
            if (0.0..1.0).contains(&k) {
                a.gfx.fill_circle(cx + (i as f32 - 2.5) * 14.0, cy - 8.0 - 36.0 * k, 3.0, 3.0, crate::CHARACTER, 1.0 - k);
            }
        }
    }
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
    let hot = hovering(a, strip) || hovering(a, line_box);
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
    a.gfx.ui(&line, Rc::new(lx + 54.0, CY - 11.0, 176.0, 22.0), 13.0, 400, if hot { WHITE } else { 0xD8D8DA }, 1.0, Align::L);
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
                pick_cell(a, r, &MONTHS[m as usize - 1][..3], on, Act::PickMonth(m));
            }
        }
        Cal::Years => {
            let first = year / 10 * 10 - 1;
            for i in 0..12 {
                let (cw, ch) = (col * 7.0 / 3.0, 44.0);
                let r = Rc::new(x0 + (i % 3) as f32 * cw + 3.0, 72.0 + (i / 3) as f32 * ch + 3.0, cw - 6.0, ch - 6.0);
                pick_cell(a, r, &(first + i).to_string(), first + i == today.year(), Act::PickYear(first + i));
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

fn pick_cell(a: &mut App, r: Rc, text: &str, on: bool, act: Act) {
    let h = btn(a, r, 0.05);
    a.gfx.fill_rrect(r, 12.0, if on { RED } else { mix(0x18181A, 0x303034, h.clamp(0.0, 1.0)) }, 1.0);
    a.gfx.ui(text, r, BODY, 500, WHITE, 1.0, Align::C);
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
fn care(a: &mut App, lx: f32) {
    use std::f32::consts::{FRAC_PI_2, TAU};
    title(a, lx, "Care");
    let cfg = &a.brain.cfg;
    let goal = cfg.water_goal * cfg.cup_ml;
    let (cx, cy, r) = (lx + 72.0, 84.0, 40.0 + 1.5 * a.water_pulse.x.clamp(-1.0, 2.0));
    let ring = Rc::new(cx - 44.0, cy - 44.0, 88.0, 88.0);
    // Pointing at it shows it can be set.
    let hot = btn(a, ring, 0.02).clamp(0.0, 1.0);
    a.gfx.unzoom();
    let width = 4.5 + 1.5 * hot;
    a.gfx.stroke_circle(cx, cy, r, width, WHITE, 0.1 + 0.08 * hot);
    let tint = (WATER, 1.0);
    let done = a.ring.x.clamp(0.0, 1.0);
    if done > 0.999 {
        a.gfx.stroke_circle_diag(cx, cy, r, width, tint, tint);
    } else if done > 0.001 {
        a.gfx.arc_diag(cx, cy, r, -FRAC_PI_2, -FRAC_PI_2 + TAU * done, width, tint, tint);
    }
    pic(a, "ic_water", cx, cy - 13.0, 22.0, WATER, 1.0);
    let text = format!("{:.1} / {:.1} L", a.ring.x.max(0.0) * goal as f32 / 1000.0, goal as f32 / 1000.0);
    swap_text(a, Rc::new(cx - 38.0, cy + 2.0, 76.0, 18.0), SMALL, 600, (WHITE, WHITE), Align::C, (&text, "Set goal"), hot);
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
    toggle(a, Rc::new(lx + 354.0, 46.0, SWITCH.0, SWITCH.1), (0, tile as i32), on, Act::Set(tile.key()));
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
    wheel_sized(a, WheelId::Cup, Rc::new(lx + 60.0, 36.0, 140.0, 92.0), &cups, 20.0);
    a.gfx.alpha = base * (0.28 + 0.72 * k);
    wheel_sized(a, WheelId::Goal, Rc::new(lx + 210.0, 36.0, 130.0, 92.0), &goals, 20.0);
    a.gfx.alpha = base;
    form_button(a, lx + 376.0, 82.0, FormButton::Done, Act::SaveWheels);
}

fn bed(a: &mut App, lx: f32) {
    back_title(a, lx, "Bedtime", Act::View(View::Care(Tile::Sleep)));
    let hours: Vec<String> = (0..24).map(|h| format!("{h:02}")).collect();
    let quarters: Vec<String> = (0..4).map(|q| format!("{:02}", q * 15)).collect();
    wheel_sized(a, WheelId::BedHour, Rc::new(lx + 118.0, 36.0, 76.0, 92.0), &hours, 28.0);
    a.gfx.ui(":", Rc::new(lx + 198.0, 60.0, 24.0, 40.0), 26.0, 600, WHITE, 1.0, Align::C);
    wheel_sized(a, WheelId::BedMinute, Rc::new(lx + 226.0, 36.0, 76.0, 92.0), &quarters, 28.0);
    form_button(a, lx + 376.0, 82.0, FormButton::Done, Act::SaveWheels);
}

#[allow(clippy::too_many_arguments)]
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
fn wheel_sized(a: &mut App, id: WheelId, r: Rc, labels: &[String], big: f32) {
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
        let tone = mix(0x6A6A6A, WHITE, near);
        let alpha = (1.05 - 0.5 * d.abs()).clamp(0.0, 1.0);
        a.gfx.ui(&labels[i as usize], Rc::new(r.x, y - big, r.w, big * 2.0), size, if near > 0.5 { 600 } else { 400 }, tone, alpha, Align::C);
    }
    a.gfx.pop_clip();
    hit(a, r, Act::Wheel(id));
}

// ---------------------------------------------------------------- shelf

fn shelf(a: &mut App, lx: f32) {
    title(a, lx, "Shelf");
    let area = Rc::new(lx + 43.6, 32.0, 336.8, 91.2);
    let drag = a.drag_over;
    let items = a.shelf.items.clone();
    if drag && !items.is_empty() {
        // Nusa keeps the Clear chip next to the title while files are dragged in.
        let clear = Rc::new(lx + 100.0, 10.8, 40.0, 17.6);
        a.gfx.fill_rrect(clear, 8.8, 0x2A2A2A, 1.0);
        a.gfx.ui("Clear", clear, 10.0, 600, 0xD6D6D6, 1.0, Align::C);
    }
    if items.is_empty() || drag {
        if drag {
            // Items already on the shelf stay faintly visible behind the drop frame.
            let n = items.len().min(crate::model::SHELF_MAX);
            if n > 0 {
                let step = (area.w / n as f32).min(76.0);
                let base = a.gfx.alpha;
                a.gfx.alpha = base * 0.22;
                for (i, p) in items.iter().take(n).enumerate() {
                    let tile = Rc::new(area.x + i as f32 * step, area.y, step - 6.0, area.h - 4.0);
                    if let (Some(bmp), _) = a.shelf_icon(p) {
                        a.gfx.image_fit(&bmp, Rc::new(tile.cx() - 20.0, tile.y + 8.0, 40.0, 40.0), 0.0, 1.0, false);
                    }
                    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    a.gfx.ui(&name, Rc::new(tile.x + 2.0, tile.y + 53.0, tile.w - 4.0, 14.0), 10.5, 600, WHITE, 1.0, Align::C);
                }
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
    let clear = Rc::new(lx + 84.0, 10.8, 40.0, 17.6);
    let h = btn(a, clear, 0.08);
    a.gfx.fill_rrect(clear, 8.8, mix(0x2A2A2A, 0x444444, h), 1.0);
    a.gfx.ui("Clear", clear, 10.0, 600, 0xD6D6D6, 1.0, Align::C);
    a.gfx.unzoom();
    hit(a, clear, Act::ShelfClear);

    let n = items.len().min(crate::model::SHELF_MAX);
    let step = (area.w / n as f32).min(76.0);
    for (i, p) in items.iter().take(n).enumerate() {
        let tile = Rc::new(area.x + i as f32 * step, area.y, step - 6.0, area.h - 4.0);
        // Freshly dropped items fade and rise into place.
        let enter = a.ripple.map(|r| ((r.0.elapsed().as_secs_f32() - 0.02) / 0.12).clamp(0.0, 1.0)).unwrap_or(1.0);
        let (base, off) = (a.gfx.alpha, a.gfx.off);
        a.gfx.alpha = base * enter;
        a.gfx.offset(off.0, off.1 + 8.0 * (1.0 - enter).powi(2));
        let hot = hovering(a, tile);
        let h = btn(a, tile, 0.05);
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
        hit(a, tile, Act::ShelfOpen(i));
        if hot {
            icon::x_circle(&a.gfx, tile.x + tile.w - 7.0, tile.y + 7.0, 7.0, 0x4A4A4A, WHITE, 1.0);
            hit(a, Rc::new(tile.x + tile.w - 15.0, tile.y - 1.0, 16.0, 16.0), Act::ShelfRemove(i));
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
fn toggle(a: &mut App, r: Rc, key: (i32, i32), on: bool, act: Act) {
    let entry = a.switches.entry(key).or_insert((crate::Spring::new(if on { 1.0 } else { 0.0 }, 300.0, 30.0), on));
    entry.1 = on;
    let k = entry.0.x.clamp(0.0, 1.0);
    let h = btn(a, r, 0.04).clamp(0.0, 1.0);
    let off = mix(0x2C2C2F, 0x48484C, h);
    let lit = mix(0x2F8F63, 0x3FB27E, h);
    a.gfx.fill_rrect(r, r.h / 2.0, mix(off, lit, k), 1.0);
    let knob = r.x + r.h / 2.0 + (r.w - r.h) * k;
    let radius = r.h / 2.0 - 3.0;
    a.gfx.fill_circle(knob, r.cy(), radius, radius, mix(0xB4B4B8, 0xF2F2F6, k), 1.0);
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
    let top = a.reminder_top;
    let rows = a.reminders();
    if rows.is_empty() && old == 0 {
        a.gfx.ui("No reminders yet", Rc::new(lx, 66.0, 420.0, 22.0), BODY, 400, 0x6D6D6D, 1.0, Align::C);
        return;
    }
    let pos = a.list_pos.x;
    a.gfx.push_clip(Rc::new(lx, 38.0, 420.0, 84.0));
    // Old notes come after the last reminder.
    if old > 0 {
        let cy = LIST_Y + (rows.len() as f32 - pos) * LIST_GAP;
        let row = Rc::new(lx + 24.0, cy - 16.0, 356.0, 32.0);
        a.gfx.ui(&format!("Old notes ({old})"), Rc::new(row.x + 6.0, row.y, 260.0, row.h), BODY, 400, 0x8A8A8A, 1.0, Align::L);
        if rows.len() >= top && rows.len() - top < LIST_ROWS {
            hit(a, row, Act::OldNotes(true));
        }
    }
    // The row just deleted slides off and the ones below close the gap.
    let gone = a.gone.clone().filter(|_| a.gone_mix.x > 0.01);
    let k = a.gone_mix.x.clamp(0.0, 1.0);
    if let Some((at, name, time)) = &gone {
        let cy = LIST_Y + (*at as f32 - pos) * LIST_GAP;
        let dx = -70.0 * (1.0 - k);
        a.gfx.ui(name, Rc::new(lx + 30.0 + dx, cy - 12.0, 200.0, 24.0), BODY, 500, WHITE, k, Align::L);
        a.gfx.ui(time, Rc::new(lx + 230.0 + dx, cy - 12.0, 64.0, 24.0), 13.0, 500, 0xC8C8C8, k, Align::R);
    }
    for (n, r) in rows.iter().enumerate() {
        let shift = gone.as_ref().map_or(0.0, |g| if n >= g.0 { k } else { 0.0 });
        let cy = LIST_Y + (n as f32 + shift - pos) * LIST_GAP;
        if !(20.0..150.0).contains(&cy) {
            continue;
        }
        let slid = swipe_of(a, &format!("r:{}", r.id));
        let shown = (top..top + LIST_ROWS).contains(&n) && slid < 0.05;
        let row = Rc::new(lx + 24.0, cy - 16.0, 276.0, 32.0);
        let h = btn(a, row, 0.04);
        a.gfx.fill_rrect(row, 12.0, 0xFFFFFF, 0.05 * h.min(1.0));
        let tone = if r.enabled { WHITE } else { 0x6D6D6D };
        a.gfx.ui(&r.title, Rc::new(row.x + 6.0, cy - 12.0, 200.0, 24.0), BODY, 500, tone, 1.0, Align::L);
        a.gfx.ui(&r.time, Rc::new(row.x + 206.0, cy - 12.0, 64.0, 24.0), 13.0, 500, if r.enabled { 0xC8C8C8 } else { 0x6D6D6D }, 1.0, Align::R);
        a.gfx.unzoom();
        if shown {
            hit(a, row, Act::ReminderEdit(n));
        }
        let hits = a.hits.len();
        let key = (1, r.id.bytes().fold(7i32, |h, b| h.wrapping_mul(31).wrapping_add(b as i32)));
        let base = a.gfx.alpha;
        a.gfx.alpha = base * (1.0 - slid);
        let dx = -36.0 * slid;
        toggle(a, Rc::new(lx + 308.0 + dx, cy - SWITCH.1 / 2.0, SWITCH.0, SWITCH.1), key, r.enabled, Act::ReminderSwitch(n));
        head_button(a, Rc::new(lx + LIST_RIGHT - 24.0 + dx, cy - 12.0, 24.0, 24.0), Act::RowTrash(0, n), |g, x, y| icon::trash(g, x, y, WHITE, 1.0));
        a.gfx.alpha = base;
        if !shown {
            a.hits.truncate(hits);
        }
        if slid > 0.01 {
            delete_button(a, lx + LIST_RIGHT, cy, 72.0, slid);
        }
    }
    a.gfx.pop_clip();
    scrollbar(a, lx, top, rows.len() + (old > 0) as usize);
}

/// Row height of the big clock in the reminder form.
pub const CLOCK_ROW: f32 = 30.0;

/// A round button at the side of the reminder form; the last one is a green tick.
fn form_button(a: &mut App, cx: f32, cy: f32, kind: FormButton, act: Act) {
    const R: f32 = 17.0;
    let r = Rc::new(cx - R, cy - R, 2.0 * R, 2.0 * R);
    let h = btn(a, r, 0.1);
    let fill = match kind {
        FormButton::Back => mix(0x2A2A2C, 0x48484C, h),
        FormButton::Next => mix(WHITE, 0xC8C8C8, h),
        FormButton::Done => mix(BUTTON, 0x48484C, h),
    };
    a.gfx.fill_circle(cx, cy, R, R, fill, 1.0);
    match kind {
        FormButton::Back => icon::chevron(&a.gfx, cx, cy, true, WHITE, 1.0),
        FormButton::Next => icon::chevron(&a.gfx, cx, cy, false, 0x000000, 1.0),
        FormButton::Done => icon::tick(&a.gfx, cx, cy, ON, 1.0),
    }
    a.gfx.unzoom();
    hit(a, r, act);
}

#[derive(Clone, Copy)]
enum FormButton {
    Back,
    Next,
    Done,
}

/// The reminder form, one thing at a time, between a back and a next button: the name, then a
/// big clock that shrinks up out of the way when the days come in.
fn form(a: &mut App, lx: f32) {
    let (text, days, editing, step, date) = {
        let f = a.form.as_ref().unwrap();
        (f.title.clone(), f.days, f.id.is_some(), f.step, f.date.clone())
    };
    let x = a.form_anim.x.clamp(0.0, 2.0);
    let (to_time, to_days) = (x.min(1.0), (x - 1.0).max(0.0));
    const CY: f32 = 78.0;
    let base = a.gfx.alpha;
    // Beside the title once the clock has moved up there.
    a.gfx.alpha = base * (1.0 - to_days);
    let heading = match &date {
        Some(d) => format!("Day: {d}"),
        None if editing => "Edit reminder".to_string(),
        None => "New reminder".to_string(),
    };
    a.gfx.ui(&heading, Rc::new(lx + 24.0, 8.0, 140.0, 22.0), HEAD, 600, WHITE, 1.0, Align::L);
    a.gfx.alpha = base;
    form_button(a, lx + 44.0, CY, FormButton::Back, Act::FormBack);
    if editing && step == 0 {
        head_button(a, Rc::new(lx + 340.0, 7.0, 24.0, 24.0), Act::FormDelete, |g, x, y| icon::trash(g, x, y, 0xFF6B6B, 1.0));
    }
    // The name.
    let name = 1.0 - to_time;
    if name > 0.01 {
        a.gfx.alpha = base * name;
        let tf = Rc::new(lx + 72.0 - 50.0 * to_time, CY - 18.0, 276.0, 36.0);
        a.gfx.fill_rrect(tf, 12.0, 0x1A1A1C, 1.0);
        let inner = Rc::new(tf.x + 14.0, tf.y, tf.w - 40.0, tf.h);
        if text.is_empty() {
            a.gfx.ui(if date.is_some() { "What is this day?" } else { "Reminder name" }, inner, BODY, 400, GREY, 1.0, Align::L);
        } else {
            a.gfx.ui(&text, inner, BODY, 500, WHITE, 1.0, Align::L);
            icon::x_circle(&a.gfx, tf.x + tf.w - 18.0, tf.cy(), 7.0, 0xDADADA, 0x1A1A1C, 1.0);
            if step == 0 {
                hit(a, Rc::new(tf.x + tf.w - 30.0, tf.y, 28.0, tf.h), Act::FormClear);
            }
        }
        if step == 0 && (a.born.elapsed().as_millis() / 530) % 2 == 0 {
            let cw = a.gfx.measure(&text, BODY, 500).min(inner.w);
            a.gfx.line(inner.x + cw + 1.0, tf.y + 10.0, inner.x + cw + 1.0, tf.y + tf.h - 10.0, 1.3, WHITE, 1.0);
        }
    }
    // The clock: big to choose, then small and out of the way.
    let (hour, minute) = (a.wheels[WheelId::FormHour as usize].index(), a.wheels[WheelId::FormMinute as usize].index());
    if to_time > 0.01 {
        a.gfx.alpha = base * to_time;
        let big = 30.0;
        if to_days < 0.01 {
            let cx = lx + 210.0 + 50.0 * (1.0 - to_time);
            let hours: Vec<String> = (0..24).map(|h| format!("{h:02}")).collect();
            let minutes: Vec<String> = (0..60).map(|m| format!("{m:02}")).collect();
            wheel_sized(a, WheelId::FormHour, Rc::new(cx - 92.0, 36.0, 76.0, 92.0), &hours, big);
            a.gfx.ui(":", Rc::new(cx - 12.0, CY - 22.0, 24.0, 40.0), big, 600, WHITE, 1.0, Align::C);
            wheel_sized(a, WheelId::FormMinute, Rc::new(cx + 16.0, 36.0, 76.0, 92.0), &minutes, big);
        } else {
            // The same numbers flying up to the corner.
            let k = to_days;
            let (cx, cy) = (lx + 210.0 + (-140.0) * k, CY + (19.0 - CY) * k);
            let size = big + (HEAD - big) * k;
            a.gfx.ui(&format!("{hour:02} : {minute:02}"), Rc::new(cx - 100.0, cy - size, 200.0, size * 2.0), size, 700, WHITE, 1.0, Align::C);
        }
    }
    // The days: quick choices above, each day below with a dot when it is on.
    if to_days > 0.01 {
        a.gfx.alpha = base * to_days;
        let chosen = PRESETS.iter().position(|p| *p == days);
        let live = step == 2;
        let mut x = lx + 70.0;
        for (i, (name, w)) in [("Every day", 70.0), ("Weekdays", 70.0), ("Weekends", 70.0), ("Custom", 56.0)].into_iter().enumerate() {
            let r = Rc::new(x, 44.0, w, 26.0);
            let on = chosen.map_or(i == 3, |c| c == i);
            let h = if live { btn(a, r, 0.04) } else { 0.0 };
            if on {
                a.gfx.stroke_rrect(r, 13.0, 1.3, ON, 1.0);
            } else {
                a.gfx.fill_rrect(r, 13.0, mix(0x1E1E20, 0x303034, h.clamp(0.0, 1.0)), 1.0);
            }
            a.gfx.ui(name, r, SMALL, 500, if on { ON } else { 0xB0B0B4 }, 1.0, Align::C);
            a.gfx.unzoom();
            if live && i < 3 {
                hit(a, r, Act::FormPreset(i as u8));
            }
            x += w + 4.0;
        }
        for (i, name) in ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"].into_iter().enumerate() {
            let r = Rc::new(lx + 74.0 + i as f32 * 39.0, 78.0, 36.0, 44.0);
            let on = days[i];
            let h = if live { btn(a, r, 0.06) } else { 0.0 };
            a.gfx.ui(name, Rc::new(r.x, r.y + 2.0, r.w, 22.0), BODY, if on { 600 } else { 400 }, mix(if on { WHITE } else { 0x8A8A8E }, WHITE, h.clamp(0.0, 1.0)), 1.0, Align::C);
            a.gfx.fill_circle(r.cx(), r.y + 32.0, 3.0, 3.0, if on { ON } else { 0x3A3A3E }, 1.0);
            a.gfx.unzoom();
            if live {
                hit(a, r, Act::FormDay(i as u8));
            }
        }
    }
    a.gfx.alpha = base;
    form_button(a, lx + 376.0, CY, if step == 2 || date.is_some() { FormButton::Done } else { FormButton::Next }, Act::FormNext);
}

