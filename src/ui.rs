//! Layout and drawing. Coordinates are layout units measured from the reference
//! captures; panel-local x is relative to the notch's left edge.

use crate::gfx::{Align, Font, Rc};
use crate::devices::DeviceKind;
use crate::media::LyrStatus;
use crate::model::{self, Phase};
use crate::{App, ToastKind};

const WHITE: u32 = 0xFFFFFF;
const GREEN: u32 = 0x21FF54;
const RED: u32 = 0xFF4F4F;
const RED_PIE: u32 = 0xE8454D;
const CHEVRON: u32 = 0x474747;
const CHIP: u32 = 0x333333;
const GREY: u32 = 0x8C8C8C;
const LABEL: u32 = 0x999999;
const BLUE: u32 = 0x4DBDFF;
const TOGGLE: u32 = 0x2699FF;
const CYAN: u32 = 0x66DBFF;
const TODO_GREEN: u32 = 0x40B854;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Music,
    Pomo,
    Water,
    Shelf,
    Todo,
}

impl Page {
    pub const ALL: [Page; 5] = [Page::Music, Page::Pomo, Page::Water, Page::Shelf, Page::Todo];
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|p| *p == self).unwrap_or(0)
    }
    pub fn next(self) -> Page {
        Self::ALL[(self.index() + 1) % 5]
    }
    pub fn prev(self) -> Page {
        Self::ALL[(self.index() + 4) % 5]
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
    PomoPhase(Phase),
    Drank,
    WaterOn,
    WaterSub(u8),
    WaterSettings(bool),
    WaterReset,
    Bottle(i32),
    Goal(u32),
    /// Bedtime, half an hour per step (water is spread out to it).
    End(i32),
    TodoAdd,
    TodoEditMode,
    TodoToggle(usize),
    TodoDelete(usize),
    FormField(u8),
    FormClear,
    FormSave,
    FormCancel,
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
    /// Incoming call: bring up the app, or dismiss the card.
    CallAccept,
    CallDecline,
}

/// Formats up to 8 typed digits into the "HH:MM-HH:MM" mask.
pub fn time_mask(digits: &str) -> String {
    let mut d = digits.chars();
    let mut out = String::new();
    for (i, slot) in "__:__-__:__".chars().enumerate() {
        let _ = i;
        if slot == '_' {
            out.push(d.next().unwrap_or('_'));
        } else {
            out.push(slot);
        }
    }
    out
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
    pub fn charging(g: &Gfx, cx: f32, cy: f32, r: f32, hex: u32, a: f32) {
        // Green ring with a white bolt.
        g.stroke_circle(cx, cy, r - 0.6, 3.0, hex, a);
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
        // Clockwise from the top-right round to the top-left, where the arrowhead points onward.
        let (from, to) = (-0.28 * PI, 1.25 * PI);
        g.arc_between(cx, cy, r, from, to, 2.0, hex, a);
        let (ex, ey) = (cx + r * to.cos(), cy + r * to.sin());
        let (tx, ty) = (-to.sin(), to.cos());
        let (h, len) = (2.6, 3.4);
        g.polygon(&[(ex - ty * h, ey + tx * h), (ex + ty * h, ey - tx * h), (ex + tx * len, ey + ty * len)], hex, a);
    }
    pub fn cup(g: &Gfx, cx: f32, cy: f32, hex: u32, a: f32) {
        g.stroke_rrect(crate::gfx::Rc::new(cx - 4.4, cy - 2.0, 7.2, 6.4), 1.4, 1.3, hex, a);
        g.arc_between(cx + 3.4, cy + 1.0, 1.8, -1.6, 1.6, 1.2, hex, a);
        for dx in [-2.2f32, 0.4] {
            g.line(cx + dx, cy - 5.2, cx + dx, cy - 3.6, 1.2, hex, a);
        }
    }
    pub fn target(g: &Gfx, cx: f32, cy: f32, hex: u32, a: f32) {
        g.stroke_circle(cx, cy, 5.2, 1.3, hex, a);
        g.stroke_circle(cx, cy, 2.4, 1.3, hex, a);
        g.line(cx, cy, cx + 5.6, cy - 5.6, 1.3, hex, a);
    }
    pub fn droplet(g: &Gfx, cx: f32, cy: f32, s: f32, hex: u32, a: f32) {
        g.droplet(cx, cy, s, hex, a);
    }
    pub fn sliders(g: &Gfx, cx: f32, cy: f32, hex: u32, a: f32) {
        g.line(cx - 5.0, cy - 2.4, cx + 5.0, cy - 2.4, 1.3, hex, a);
        g.line(cx - 5.0, cy + 2.4, cx + 5.0, cy + 2.4, 1.3, hex, a);
        g.fill_circle(cx + 1.6, cy - 2.4, 1.7, 1.7, hex, a);
        g.fill_circle(cx - 1.8, cy + 2.4, 1.7, 1.7, hex, a);
    }
    pub fn bell(g: &Gfx, cx: f32, cy: f32, hex: u32, a: f32) {
        // Dome with a flared rim and a clapper below.
        let half = [(4.4f32, 2.6f32), (3.8, 1.8), (3.4, 0.0), (3.2, -2.4), (2.6, -3.8), (1.6, -4.6), (0.0, -4.9)];
        let mut pts: Vec<(f32, f32)> = half.iter().map(|&(x, y)| (cx + x, cy + y)).collect();
        pts.extend(half.iter().rev().skip(1).map(|&(x, y)| (cx - x, cy + y)));
        g.polygon(&pts, hex, a);
        g.fill_circle(cx, cy + 4.6, 1.3, 1.3, hex, a);
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
    pub fn checklist(g: &Gfx, cx: f32, cy: f32, hex: u32, a: f32) {
        g.fill_rrect(crate::gfx::Rc::new(cx - 16.8, cy - 17.2, 33.6, 34.4), 7.0, 0x404245, a);
        for (i, dy) in [-6.0f32, 5.0].iter().enumerate() {
            let y = cy + dy;
            let _ = i;
            g.polyline(&[(cx - 9.6, y), (cx - 7.2, y + 2.4), (cx - 3.0, y - 2.6)], 1.8, hex, a);
            g.line(cx + 0.6, y, cx + 9.6, y, 1.8, hex, a);
        }
    }
    pub fn plus_circle(g: &Gfx, cx: f32, cy: f32, hex: u32, a: f32) {
        g.fill_circle(cx, cy, 6.8, 6.8, hex, a);
        g.line(cx - 3.2, cy, cx + 3.2, cy, 1.5, 0x000000, a);
        g.line(cx, cy - 3.2, cx, cy + 3.2, 1.5, 0x000000, a);
    }
    pub fn pen(g: &Gfx, cx: f32, cy: f32, hex: u32, a: f32) {
        let k = 0.89;
        let p = |x: f32, y: f32| (cx + x * k, cy + y * k);
        g.polygon(&[p(-5.4, 5.4), p(-5.0, 2.6), p(3.2, -5.6), p(5.6, -3.2), p(-2.6, 5.0)], hex, a);
        g.line(cx - 6.4 * k, cy + 6.6 * k, cx + 6.4 * k, cy + 6.6 * k, 1.2, hex, a);
    }
    pub fn clock(g: &Gfx, cx: f32, cy: f32, hex: u32, a: f32) {
        g.stroke_circle(cx, cy, 6.4, 1.6, hex, a);
        g.polyline(&[(cx, cy - 3.6), (cx, cy), (cx + 2.6, cy + 1.6)], 1.5, hex, a);
    }
    pub fn trash(g: &Gfx, cx: f32, cy: f32, hex: u32, a: f32) {
        g.line(cx - 3.8, cy - 3.2, cx + 3.8, cy - 3.2, 1.2, hex, a);
        g.stroke_rrect(crate::gfx::Rc::new(cx - 2.8, cy - 2.2, 5.6, 6.6), 1.0, 1.1, hex, a);
        g.line(cx - 1.2, cy - 4.6, cx + 1.2, cy - 4.6, 1.2, hex, a);
    }
    pub fn x_circle(g: &Gfx, cx: f32, cy: f32, r: f32, bg: u32, fg: u32, a: f32) {
        g.fill_circle(cx, cy, r, r, bg, a);
        let s = r * 0.42;
        g.line(cx - s, cy - s, cx + s, cy + s, 1.5, fg, a);
        g.line(cx - s, cy + s, cx + s, cy - s, 1.5, fg, a);
    }
    pub fn minus_plus(g: &Gfx, cx: f32, cy: f32, plus: bool, a: f32) {
        g.fill_circle(cx, cy, 10.4, 10.4, 0x1F1F1F, a);
        g.line(cx - 4.6, cy, cx + 4.6, cy, 2.0, 0xFFFFFF, a);
        if plus {
            g.line(cx, cy - 4.6, cx, cy + 4.6, 2.0, 0xFFFFFF, a);
        }
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
    pub fn liquid(g: &Gfx, cx: f32, cy: f32, r: f32, level: f32, t: f32, a: f32) {
        // Dark glass, darker at the top.
        g.fill_circle(cx, cy, r, r, 0x1E0306, a);
        g.glow(cx, cy + r * 0.4, r * 1.1, 0x3A070C, 0.8 * a);
        // Surface height: 0.10 empty … 0.58 full (a full timer reads as a half glass, as in Nusa),
        // slowly tilting with a gentle travelling wave.
        let h = cy + r - (0.1 + 0.48 * level.clamp(0.0, 1.0)) * 2.0 * r;
        let tilt = 0.5 * (t * 0.7).sin() + 0.12 * (t * 1.9).sin();
        let mut surf = Vec::with_capacity(34);
        for i in 0..=32 {
            let u = i as f32 / 32.0;
            let x = cx - r + 2.0 * r * u;
            let wave = 0.7 * (u * 5.5 + t * 2.0).sin() + 0.35 * (u * 9.0 - t * 2.7).sin();
            surf.push((x, h + (x - cx) * tilt.tan() + wave));
        }
        let mut body = surf.clone();
        body.push((cx + r, cy + r + 2.0));
        body.push((cx - r, cy + r + 2.0));
        g.clip_circle(cx, cy, r - 0.5, || {
            g.polygon(&body, super::RED_PIE, a);
            // Light from the upper left inside the liquid, deeper red at the bottom right.
            g.glow(cx - r * 0.4, h + r * 0.35, r * 0.95, 0xFF7C80, 0.45 * a);
            g.glow(cx + r * 0.5, cy + r * 0.8, r * 0.9, 0xB81F2C, 0.5 * a);
            // A bright meniscus along the surface.
            g.polyline(&surf, 1.1, 0xFFB0B3, 0.55 * a);
        });
        // Glass rim and a small specular highlight.
        g.stroke_circle(cx, cy, r - 0.6, 1.4, 0xF4A3A7, 0.8 * a);
        g.arc_between(cx, cy, r - 2.6, 3.6, 4.4, 1.2, 0xFFFFFF, 0.35 * a);
    }
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
    pub fn phone(g: &Gfx, cx: f32, cy: f32, down: bool, hex: u32, a: f32) {
        use std::f32::consts::PI;
        let r = 7.4;
        // The handle is an arc with a thick round earpiece at each end; hanging up it lies flat.
        let (hx, hy, from, to) = if down { (cx, cy + 5.0, PI + 0.5, 2.0 * PI - 0.5) } else { (cx + 4.2, cy - 4.2, PI * 0.5 + 0.2, PI - 0.2) };
        g.arc_between(hx, hy, r, from, to, 2.2, hex, a);
        for t in [from, to] {
            g.fill_circle(hx + r * t.cos(), hy + r * t.sin(), 2.3, 2.3, hex, a);
        }
    }
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
    let lm = a.lyr_mix.x.clamp(0.0, 1.0);
    if a.page == Page::Music && lm > 0.01 && a.fade.x > 0.01 {
        lyrics_backdrop(a, left, w, h, radius, lm);
    }

    let exp_alpha = a.fade.x.clamp(0.0, 1.0) * ((h - 70.0) / 50.0).clamp(0.0, 1.0);
    let tall = a.toast.as_ref().or(a.last_toast.as_ref()).is_some_and(|t| t.kind.tall()) && a.toast_mix.x > 0.5;
    let fade_from = if tall { 84.0 } else { 36.0 };
    let mut small_alpha = (1.0 - (h - fade_from) / 14.0).clamp(0.0, 1.0) * (1.0 - a.fade.x).clamp(0.0, 1.0);
    // A notification grows into its detail view: the card leaves in the first half of the way, the
    // detail comes in the second, so the two are never on screen together.
    let notice = a.notice_toast() && a.fade.x < 0.01;
    let grown = if notice {
        let card_h = a.notice_card_height();
        ((h - card_h) / (crate::NOTICE_OPEN[1] - card_h)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    if notice {
        small_alpha = (1.0 - 2.0 * grown).clamp(0.0, 1.0);
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
        } else if a.yielded && a.eyes.show.x > 0.3 {
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
            if show > 0.3 {
                eyes(a, left + w / 2.0, h / 2.0, (show - 0.3) / 0.7, (w / 2.0 - 61.0).max(14.0));
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
    if notice {
        let d = (2.0 * grown - 1.0).clamp(0.0, 1.0);
        if d > 0.01 {
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
    let show = a.eyes.show.x.clamp(0.0, 1.0);
    // The eyes have the notch to themselves: everything else steps back while they are open.
    let rest = 1.0 - (show / 0.3).min(1.0);
    if charging {
        icon::charging(&a.gfx, left + 20.0, cy, 8.2, GREEN, rest);
    }
    let time = a.clock().time_label();
    a.gfx.ui(&time, Rc::new(left, 5.0, w, 20.0), 16.0, 700, WHITE, rest, Align::C);
    a.gfx.alpha = rest;
    right_indicator(a, left + w - 28.0, cy);
    a.gfx.alpha = 1.0;
    if show > 0.3 {
        eyes(a, left + w / 2.0, cy, (show - 0.3) / 0.7, (w / 2.0 - 61.0).max(14.0));
    }
    // The battery reaches past the compact notch, so that scene redraws everything.
    if a.eyes.scene.x > 0.01 || a.eye_feel_wide() {
        a.loop_rect = None;
    } else if show > 0.01 || a.sample_running() {
        a.loop_rect = Some(Rc::new(left + 2.0, 0.0, w - 4.0, 30.0));
    }
}

/// Equalizer bars while music plays, or the focus-timer pie while it runs.
fn right_indicator(a: &mut App, x: f32, cy: f32) {
    // The only part that moves when nothing else does: frames can redraw just this box.
    a.loop_rect = Some(Rc::new(x - 3.0, cy - 12.0, 27.0, 24.0));
    if a.brain.focus.is_some() {
        let (left, total, _, _) = a.pomo_view();
        let frac = (left / total) as f32;
        let t = a.born.elapsed().as_secs_f32();
        icon::liquid(&a.gfx, x + 9.2, cy, 9.2, frac, t, 1.0);
        return;
    }
    if !a.playing() {
        return;
    }
    let t = a.born.elapsed().as_secs_f32();
    for i in 0..5 {
        let fi = i as f32;
        let wave = 0.55 * (t * (2.1 + fi * 0.37) + fi * 1.9).sin() + 0.45 * (t * (3.3 - fi * 0.29) + fi * 0.7).sin();
        let bh = 3.0 + 7.4 * (0.5 + 0.5 * wave).clamp(0.0, 1.0);
        a.gfx.fill_rrect(Rc::new(x + i as f32 * 4.05, cy - bh / 2.0, 2.2, bh), 1.1, WHITE, 1.0);
    }
}

/// White mask lenses with a thin red rim, as on the design sheet.
fn eyes(a: &mut App, cx: f32, cy: f32, show: f32, reach: f32) {
    const RED: u32 = 0xFF1F1F;
    /// Notch units per pixel of the design sheet.
    const SCALE: f32 = 0.175;
    let e = &a.eyes;
    // In the battery scene the eyes stay on their side and lunge at it.
    let scene = e.scene.x.clamp(0.0, 1.0);
    let centre = cx;
    let cx = cx - 46.0 * scene + 32.0 * e.lunge.x * scene;
    let reach = reach * (1.0 - scene);
    let lean = e.lean.x;
    let t = a.born.elapsed().as_secs_f32();
    let breath = 0.35 * (t * 1.7).sin();
    let g = &a.gfx;
    for (i, side) in [-1.0f32, 1.0].into_iter().enumerate() {
        let outline: Vec<(f32, f32)> = e.pair[i].points.iter().map(|p| (cx + lean * reach + p.0 * SCALE, cy + breath + e.hop.x + p.1 * SCALE)).collect();
        g.glow_ellipse(cx + lean * reach + side * 32.0, cy, 28.0, 16.0, RED, 0.3 * show);
        let lens = show * (1.0 - e.dizzy.x.clamp(0.0, 1.0));
        g.fill_poly(&outline, WHITE, lens);
        let mut rim = outline;
        rim.push(rim[0]);
        g.polyline(&rim, 1.6, RED, lens);
        let (ex, ey) = (cx + lean * reach + side * 29.0, cy + breath + e.hop.x);
        let dizzy = e.dizzy.x.clamp(0.0, 1.0);
        if dizzy > 0.01 {
            let turn = t * 7.0 * side;
            let spiral: Vec<(f32, f32)> = (0..=44)
                .map(|n| {
                    let k = n as f32 / 44.0;
                    let a = turn + k * 15.0;
                    (ex + 15.0 * k * a.cos(), ey + 10.5 * k * a.sin())
                })
                .collect();
            g.polyline(&spiral, 3.6, RED, show * dizzy);
            g.polyline(&spiral, 1.7, WHITE, show * dizzy);
        }
        let tear = e.tear.x.clamp(0.0, 1.0);
        if tear > 0.01 {
            let k = (t % 1.5) / 1.5;
            let dy = 9.0 + 5.0 * k * k;
            g.droplet(cx + side * 52.0, cy + dy, 4.2, WHITE, show * tear * (1.0 - k * k));
        }
    }
    // The battery never leaves the notch as it narrows again.
    let half = a.sw.x / 2.0;
    g.push_clip(Rc::new(centre - half, 0.0, half * 2.0, a.sh.x));
    if scene > 0.01 {
        // The battery slides in from the right, takes the blows and shows what is left.
        let (bx, by) = (centre + 66.0 + 60.0 * (1.0 - scene) + e.shove.x, cy);
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

/// The small eyes at the head of the lyrics.
fn mini_eyes(a: &mut App, cx: f32, cy: f32) {
    /// Notch units per pixel of the design sheet.
    const SCALE: f32 = 0.085;
    let hop = a.mini.hop.x;
    for eye in &a.mini.pair {
        let outline: Vec<(f32, f32)> = eye.points.iter().map(|p| (cx + p.0 * SCALE, cy + hop + p.1 * SCALE)).collect();
        a.gfx.fill_poly(&outline, WHITE, 1.0);
        let mut rim = outline;
        rim.push(rim[0]);
        a.gfx.polyline(&rim, 1.25, 0xFF1F1F, 1.0);
    }
}

fn lyric_pill(a: &mut App, left: f32, w: f32) {
    let line = a.lyric_line().unwrap_or_default();
    // Art-tinted glow rising from the bottom edge, as on the expanded lyrics view.
    let h = a.sh.x;
    let radius = 12.4 + 19.2 * ((h - 30.0) / 110.0).clamp(0.0, 1.0);
    a.gfx.fill_rrect_vgrad(Rc::new(left, 0.0, w, h), radius, (art_tint(a), 0.0), (art_tint(a), 0.85));
    mini_eyes(a, left + 25.0, h / 2.0);
    // Between lines only the small eyes move, so frames can redraw just their box.
    a.loop_rect = Some(Rc::new(left + 4.0, 0.0, 44.0, h));
    if line != a.pill_cur {
        let old = std::mem::replace(&mut a.pill_cur, line.clone());
        let t = a.born.elapsed().as_secs_f32();
        a.mini.new_line(t);
        a.pill_prev = if old.is_empty() { None } else { Some((std::time::Instant::now(), old)) };
        a.kick();
    }
    // The outgoing line lifts away as the new one rises into place.
    let text = |a: &mut App, s: &str, dy: f32, al: f32| {
        let base = a.gfx.alpha;
        a.gfx.alpha = base * al;
        a.gfx.push_clip(Rc::new(left, 0.0, w, h));
        a.gfx.ui(s, Rc::new(left + 44.0, 6.0 + dy, w - 88.0, 22.0), 13.0, 600, WHITE, 1.0, Align::C);
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
        ToastKind::Charging(_) | ToastKind::Battery => icon::charging(&a.gfx, ix, cy, 8.2, GREEN, 1.0),
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
    Stopwatch,
    /// Nothing: the caller draws over the badge.
    Blank,
    /// What a connected device is (headphones, mouse, drive...).
    Device(DeviceKind),
    Usb,
    /// Luma's breaks.
    Stretch,
    Eye,
    Breath,
    Moon,
}

fn glyph(a: &mut App, g: Glyph, cx: f32, cy: f32, hex: u32) {
    match g {
        Glyph::Device(k) => icon::device(&a.gfx, k, cx, cy, hex, 1.0),
        Glyph::Usb => icon::usb(&a.gfx, cx, cy, hex, 1.0),
        Glyph::Blank => {}
        Glyph::Stretch => {
            // A figure with both arms raised.
            a.gfx.fill_circle(cx, cy - 5.6, 1.9, 1.9, hex, 1.0);
            a.gfx.polyline(&[(cx - 5.2, cy - 6.6), (cx - 2.2, cy - 2.4), (cx + 2.2, cy - 2.4), (cx + 5.2, cy - 6.6)], 1.6, hex, 1.0);
            a.gfx.line(cx, cy - 2.4, cx, cy + 2.6, 1.6, hex, 1.0);
            a.gfx.polyline(&[(cx - 3.2, cy + 7.4), (cx, cy + 2.6), (cx + 3.2, cy + 7.4)], 1.6, hex, 1.0);
        }
        Glyph::Eye => {
            let top: Vec<(f32, f32)> = (0..=12).map(|i| { let u = i as f32 / 12.0; (cx - 7.6 + 15.2 * u, cy - 4.6 * (std::f32::consts::PI * u).sin()) }).collect();
            let bot: Vec<(f32, f32)> = (0..=12).map(|i| { let u = i as f32 / 12.0; (cx - 7.6 + 15.2 * u, cy + 4.6 * (std::f32::consts::PI * u).sin()) }).collect();
            a.gfx.polyline(&top, 1.5, hex, 1.0);
            a.gfx.polyline(&bot, 1.5, hex, 1.0);
            a.gfx.fill_circle(cx, cy, 2.2, 2.2, hex, 1.0);
        }
        Glyph::Breath => {
            // Three wind strokes curling at the end.
            for (dy, len) in [(-4.0f32, 11.0f32), (0.0, 14.0), (4.0, 9.0)] {
                a.gfx.line(cx - 7.0, cy + dy, cx - 7.0 + len - 2.0, cy + dy, 1.5, hex, 1.0);
                a.gfx.arc_between(cx - 7.0 + len - 2.0, cy + dy - 1.6, 1.6, std::f32::consts::FRAC_PI_2, std::f32::consts::PI * 2.2, 1.5, hex, 1.0);
            }
        }
        Glyph::Moon => {
            a.gfx.fill_circle(cx, cy, 6.8, 6.8, hex, 1.0);
            a.gfx.fill_circle(cx + 3.4, cy - 2.6, 5.8, 5.8, 0x000000, 1.0);
        }
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
        Glyph::Stopwatch => icon::stopwatch(&a.gfx, cx, cy + 1.2, 7.4, hex, 1.0),
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
        // The bell is its own badge and keeps ringing while the card waits.
        crate::anim::bell(&a.gfx, bx, cy, 16.0, age, c.accent);
        a.loop_rect = Some(Rc::new(bx - 24.0, 0.0, 48.0, h));
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
    a.gfx.ui(title, Rc::new(tx, cy - 17.6, tw, 20.0), 15.0, 700, WHITE, 1.0, Align::L);
    a.gfx.ui(sub, Rc::new(tx, cy + 2.4, tw, 16.0), 11.5, 400, 0xA0A0A0, 1.0, Align::L);
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

    // Content.
    let top = 46.0;
    let bottom = h - 14.0 - 28.0 - 10.0;
    if kind == crate::NoticeKind::Screenshot {
        let tr = Rc::new(x0, top, 128.0, 72.0);
        match &thumb {
            Some(b) => a.gfx.image_fit(b, tr, 8.0, 1.0, true),
            None => a.gfx.fill_rrect(tr, 8.0, 0x1C1C1E, 1.0),
        }
        a.gfx.stroke_rrect(tr, 8.0, 1.0, 0x3A3A3C, 1.0);
        let tx = tr.x + tr.w + 14.0;
        let tw = x1 - tx;
        a.gfx.ui("Ảnh chụp màn hình", Rc::new(tx, top + 2.0, tw, 20.0), 15.0, 700, WHITE, 1.0, Align::L);
        let line = match &file {
            Some(f) => f.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
            None => "Đã chép vào bộ nhớ tạm".to_string(),
        };
        a.gfx.ui(&line, Rc::new(tx, top + 26.0, tw, 17.0), 12.0, 400, 0xC7C7CC, 1.0, Align::L);
        if !size.is_empty() {
            a.gfx.ui(&size, Rc::new(tx, top + 45.0, tw, 16.0), 11.5, 400, 0x8E8E93, 1.0, Align::L);
        }
    } else {
        a.gfx.ui(&title, Rc::new(x0, top, x1 - x0, 20.0), 15.0, 700, WHITE, 1.0, Align::L);
        let by = top + 24.0;
        a.gfx.push_clip(Rc::new(x0, by, x1 - x0, (bottom - by).max(0.0)));
        a.gfx.text(&body, Rc::new(x0, by, x1 - x0, 200.0), 13.0, 400, 0xE5E5EA, 1.0, Align::LW, Font::Ui);
        a.gfx.pop_clip();
    }

    // Buttons: Close always; Open when it leads somewhere. Clicking the content opens too.
    let by = h - 14.0 - 28.0;
    let close = Rc::new(x1 - 28.0, by, 28.0, 28.0);
    let hc = if live { btn(a, close, 0.07) } else { 0.0 };
    a.gfx.fill_circle(close.cx(), close.cy(), 14.0, 14.0, mix(0x2C2C2E, 0x3A3A3C, hc), 1.0);
    a.gfx.line(close.cx() - 4.0, close.cy() - 4.0, close.cx() + 4.0, close.cy() + 4.0, 1.8, 0xD1D1D6, 1.0);
    a.gfx.line(close.cx() - 4.0, close.cy() + 4.0, close.cx() + 4.0, close.cy() - 4.0, 1.8, 0xD1D1D6, 1.0);
    a.gfx.unzoom();
    if live {
        hit(a, Rc::new(left, top - 4.0, w, by - top), if openable { Act::NoticeOpen } else { Act::None });
        hit(a, close, Act::NoticeClose);
    }
    if openable {
        let label = match kind {
            crate::NoticeKind::Screenshot => "Mở ảnh",
            crate::NoticeKind::Chat => "Trả lời",
            _ => "Mở",
        };
        let open = Rc::new(close.x - 8.0 - 84.0, by, 84.0, 28.0);
        card_button(a, open, label, 0x1E50C8, 0xDCE8FF, live, Act::NoticeOpen);
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
        ToastKind::Song => {
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
        ToastKind::Sample(i) => {
            // Nothing on the right, so the icon and text sit together in the middle of the card.
            const ICON: f32 = 54.0;
            const GAP: f32 = 14.0;
            let cy = h / 2.0;
            let clock = a.born.elapsed().as_secs_f32();
            let text_w = a.gfx.measure(title, 15.0, 700).max(a.gfx.measure(value, 11.5, 400));
            let x0 = left + (w - (ICON + GAP + text_w)) / 2.0;
            let ix = x0 + ICON / 2.0;
            match i {
                0 => crate::anim::water(&a.gfx, ix, cy, 22.0, clock),
                _ => crate::anim::bell(&a.gfx, ix, cy, 22.0, clock, 0xFDB825),
            }
            let tx = x0 + ICON + GAP;
            a.gfx.ui(title, Rc::new(tx, cy - 17.6, text_w + 8.0, 20.0), 15.0, 700, WHITE, 1.0, Align::L);
            a.gfx.ui(value, Rc::new(tx, cy + 2.4, text_w + 8.0, 16.0), 11.5, 400, 0xA0A0A0, 1.0, Align::L);
            a.loop_rect = Some(Rc::new(x0 - 2.0, 0.0, ICON + 4.0, h));
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
        ToastKind::Call => {
            // Apple's call banner: who, from which app, red and green round buttons.
            let info = a.notice.clone();
            let accent = info.as_ref().map(|n| a.notice_accent(n)).unwrap_or(GREEN);
            let ix = left + 34.0;
            let cy = h / 2.0;
            // Rings spread from the icon in the app's colour, in time with the ringing.
            for d in [0.0f32, 0.5] {
                let q = ((age + d) % 1.4) / 1.4;
                a.gfx.stroke_circle(ix, cy, 16.0 + 12.0 * q, 1.4, accent, 0.45 * (1.0 - q));
            }
            match &info {
                Some(n) => notice_badge(a, n, ix, cy, 16.0),
                None => a.gfx.fill_circle(ix, cy, 16.0, 16.0, accent, 1.0),
            }
            let tx = left + 60.0;
            let tw = w - 60.0 - 96.0;
            a.gfx.ui(value, Rc::new(tx, cy - 17.6, tw, 20.0), 15.0, 700, WHITE, 1.0, Align::L);
            a.gfx.ui(&format!("{title} · Incoming call"), Rc::new(tx, cy + 2.4, tw, 16.0), 11.5, 400, 0xA0A0A0, 1.0, Align::L);
            for (i, (fill, act)) in [(0xFF3B30u32, Act::CallDecline), (0x34C759, Act::CallAccept)].into_iter().enumerate() {
                let r = Rc::new(left + w - 88.0 + i as f32 * 40.0, cy - 16.0, 32.0, 32.0);
                let hov = if live { btn(a, r, 0.08) } else { 0.0 };
                a.gfx.fill_circle(r.cx(), r.cy(), 16.0, 16.0, mix(fill, WHITE, 0.12 * hov), 1.0);
                icon::phone(&a.gfx, r.cx(), r.cy(), i == 0, WHITE, 1.0);
                a.gfx.unzoom();
                if live {
                    hit(a, r, act);
                }
            }
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
            let (b1, b2) = buttons(32.0, 64.0);
            let c = Card { accent: RED, dark: 0x2A0A0C, badge: Glyph::Stopwatch, shake: 0.2, ring: None, reserve: b2.x + b2.w - b1.x + 14.0 };
            card(a, left, w, h, t, title, value, &c);
            // Close: a round ✕ the same height as the action button.
            let x_btn = Rc::new(b1.x + 2.0, b1.y, 28.0, 28.0);
            let hx = if live { btn(a, x_btn, 0.08) } else { 0.0 };
            a.gfx.fill_circle(x_btn.cx(), x_btn.cy(), 14.0, 14.0, mix(0x4A1517, 0x6A1E21, hx), 1.0);
            let (qx, qy, d) = (x_btn.cx(), x_btn.cy(), 5.0);
            a.gfx.line(qx - d, qy - d, qx + d, qy + d, 2.2, 0xF0484F, 1.0);
            a.gfx.line(qx - d, qy + d, qx + d, qy - d, 2.2, 0xF0484F, 1.0);
            a.gfx.unzoom();
            if live {
                hit(a, x_btn, Act::ToastDismiss);
            }
            let (bg, fg) = if focus { (0x0E3A1B, 0x35D766) } else { (0x0D2F44, 0x4DBDFF) };
            // A time's-up card offers a few more minutes; a phase notice skips the break or
            // pauses the round.
            let label = if a.focus_end_card() {
                "+5 min"
            } else if focus {
                "Pause"
            } else {
                "Skip"
            };
            card_button(a, b2, label, bg, fg, live, Act::ToastStartPhase);
        }
        ToastKind::Task => {
            let (b1, b2) = buttons(56.0, 56.0);
            let c = Card { accent: TODO_GREEN, dark: 0x0B2414, badge: Glyph::Bell, shake: 0.0, ring: None, reserve: b2.x + b2.w - b1.x + 14.0 };
            card(a, left, w, h, t, title, value, &c);
            later_button(a, b1, t, live);
            card_button(a, b2, "Start", 0x0E3A1B, 0x35D766, live, Act::LumaDone);
        }
        ToastKind::Remind(k) => {
            let (b1, b2) = buttons(56.0, 64.0);
            use crate::luma::config::Kind as K;
            let (glyph, accent, dark, done) = match k {
                K::Stretch => (Glyph::Stretch, 0xFF9F43, 0x2E1D08, "Done"),
                K::Eyes => (Glyph::Eye, 0x7BD88F, 0x0E2A14, "Done"),
                K::Breathe => (Glyph::Breath, 0x66DBFF, 0x0B2A33, "Done"),
                K::Sleep | K::Water => (Glyph::Moon, 0xA78BFA, 0x1F1636, "Sleep"),
            };
            let c = Card { accent, dark, badge: glyph, shake: 0.14, ring: None, reserve: b2.x + b2.w - b1.x + 14.0 };
            card(a, left, w, h, t, title, value, &c);
            later_button(a, b1, t, live);
            card_button(a, b2, done, mix(dark, accent, 0.25), accent, live, Act::LumaDone);
        }
        ToastKind::HeadsUp => {
            let (b1, b2) = buttons(56.0, 56.0);
            // Counts down to the card, the ring emptying second by second.
            let secs_left = a.brain.showing.as_ref().and_then(|s| s.heads_up_until).map(|u| (u - chrono::Local::now()).num_milliseconds().max(0) as f32 / 1000.0).unwrap_or(0.0);
            let c = Card { accent: 0xF5B833, dark: 0x2A2310, badge: Glyph::Bell, shake: 0.0, ring: None, reserve: b2.x + b2.w - b1.x + 14.0 };
            card(a, left, w, h, t, title, &format!("Coming up in {}s", secs_left.ceil() as u32), &c);
            card_button(a, b1, "Later", 0x2E2E2E, 0xE6E6E6, live, Act::LumaLater);
            card_button(a, b2, "Now", 0x2A2310, 0xF5B833, live, Act::LumaNow);
        }
        ToastKind::LumaApp => {
            let (b1, b2) = buttons(56.0, 72.0);
            let c = Card { accent: 0xA78BFA, dark: 0x1F1636, badge: Glyph::Moon, shake: 0.0, ring: None, reserve: b2.x + b2.w - b1.x + 14.0 };
            card(a, left, w, h, t, title, value, &c);
            card_button(a, b1, "Later", 0x2E2E2E, 0xE6E6E6, live, Act::ToastDismiss);
            card_button(a, b2, "Turn off", 0x1F1636, 0xC9B8FF, live, Act::LumaAppOff);
        }
        ToastKind::Water => {
            let (b1, b2) = buttons(62.0, 56.0);
            let (ix, cy, tx) = (left + 38.0, h / 2.0, left + 70.0);
            crate::anim::water(&a.gfx, ix, cy, 22.0, a.born.elapsed().as_secs_f32());
            let tw = b1.x - tx - 8.0;
            a.gfx.ui(title, Rc::new(tx, cy - 17.6, tw, 20.0), 15.0, 700, WHITE, 1.0, Align::L);
            a.gfx.ui(value, Rc::new(tx, cy + 2.4, tw, 16.0), 11.5, 400, 0xA0A0A0, 1.0, Align::L);
            a.loop_rect = Some(Rc::new(ix - 24.0, 0.0, 48.0, h));
            later_button(a, b1, t, live);
            card_button(a, b2, "Drank", 0x1E50C8, 0xDCE8FF, live, Act::LumaDone);
        }
        _ => {
            // App notification: the app's icon on its own colour, sender over message.
            let info = a.notice.clone().filter(|_| live);
            let accent = info.as_ref().map(|n| a.notice_accent(n)).unwrap_or(0xF5B833);
            let has_icon = info.as_ref().and_then(|n| n.icon.as_ref()).is_some_and(|p| a.shelf_icon(p).0.is_some());
            let c = Card { accent, dark: mix(0x000000, accent, 0.18), badge: if has_icon { Glyph::Blank } else { Glyph::Bell }, shake: if has_icon { 0.0 } else { 0.32 }, ring: None, reserve: 14.0 };
            card(a, left, w, h, t, value, title, &c);
            if let Some(n) = info.filter(|_| has_icon) {
                notice_badge(a, &n, left + 32.0, h / 2.0, 10.0);
            }
        }
    }
}

/// "Later" on Luma's cards; on the strict care level it only appears after a few seconds.
fn later_button(a: &mut App, r: Rc, t: &crate::Toast, live: bool) {
    let wait = a.brain.skip_after() as f32;
    let age = t.born.elapsed().as_secs_f32();
    if age < wait {
        let left = (wait - age).ceil() as u32;
        a.gfx.fill_rrect(r, r.h / 2.0, 0x1E1E1E, 1.0);
        a.gfx.ui(&format!("{left}s"), r, 12.0, 600, 0x7A7A7A, 1.0, Align::C);
        return;
    }
    card_button(a, r, "Later", 0x2E2E2E, 0xE6E6E6, live, Act::LumaLater);
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

fn header(a: &mut App, lx: f32) {
    let charging = a.power.is_some_and(|p| p.battery && p.ac);
    if charging {
        icon::charging(&a.gfx, lx + 146.0, 18.0, 8.2, GREEN, 1.0);
    }
    let date = a.date_label();
    let time = a.clock().time_label();
    a.gfx.ui(&date, Rc::new(lx + 163.0, 8.0, 160.0, 20.0), 13.0, 400, WHITE, 1.0, Align::L);
    let dw = a.gfx.measure(&date, 13.0, 400);
    a.gfx.ui(&time, Rc::new(lx + 163.0 + dw + 7.0, 8.0, 60.0, 20.0), 13.0, 700, WHITE, 1.0, Align::L);
    // The bell appears once there is something to look back at.
    if !a.history.is_empty() {
        let bell = Rc::new(lx + 346.0, 7.0, 24.0, 22.0);
        let h = btn(a, bell, 0.1);
        icon::bell(&a.gfx, bell.cx(), bell.cy(), mix(0xD6D6D6, WHITE, h), 1.0);
        if a.history_unread > 0 {
            let n = a.history_unread.min(99).to_string();
            let w = (a.gfx.measure(&n, 8.5, 700) + 6.0).max(11.0);
            let dot = Rc::new(bell.cx() + 2.0, bell.y, w, 11.0);
            a.gfx.fill_rrect(dot, 5.5, RED, 1.0);
            a.gfx.ui(&n, dot, 8.5, 700, WHITE, 1.0, Align::C);
        }
        a.gfx.unzoom();
        hit(a, bell, Act::History(true));
    }
    right_indicator(a, lx + 390.8, 18.0);
}

fn chevrons(a: &mut App, lx: f32) {
    for (x, left, act) in [(lx + 22.0, true, Act::PagePrev), (lx + 395.0, false, Act::PageNext)] {
        let r = Rc::new(x - 11.0, 59.0, 22.0, 22.0);
        let h = btn(a, r, 0.18);
        a.gfx.fill_circle(x, 70.0, 8.4, 8.4, mix(CHEVRON, 0x8A8A8A, h), 1.0);
        icon::chevron(&a.gfx, x, 70.0, left, 0x000000, 1.0);
        a.gfx.unzoom();
        hit(a, r, act);
    }
}

fn dots(a: &mut App, lx: f32) {
    let n = Page::ALL.len();
    // The active pill glides between dots: each dot's width and brightness follow its distance
    // from the eased page position.
    let pos = a.dot_pos.x.rem_euclid(n as f32);
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

fn title(a: &mut App, lx: f32, first: &str, second: Option<(&str, u32)>) {
    a.gfx.ui(first, Rc::new(lx + 15.0, 8.2, 200.0, 22.0), 15.0, 600, WHITE, 1.0, Align::L);
    if let Some((s, col)) = second {
        let fw = a.gfx.measure(first, 15.0, 600);
        a.gfx.ui(s, Rc::new(lx + 15.0 + fw, 8.2, 200.0, 22.0), 15.0, 600, col, 1.0, Align::L);
    }
}

/// Notification history: newest first, three rows at a time, the wheel scrolls.
fn history(a: &mut App, lx: f32) {
    let back = Rc::new(lx + 19.0, 8.0, 22.0, 22.0);
    let h = btn(a, back, 0.18);
    a.gfx.fill_circle(lx + 30.0, 19.2, 8.8, 8.8, mix(CHEVRON, 0x8A8A8A, h), 1.0);
    icon::chevron(&a.gfx, lx + 30.0, 19.2, true, 0x000000, 1.0);
    a.gfx.unzoom();
    hit(a, back, Act::History(false));
    a.gfx.ui("Notifications", Rc::new(lx + 46.0, 8.2, 160.0, 22.0), 15.0, 600, WHITE, 1.0, Align::L);
    let clear = Rc::new(lx + 346.0, 10.0, 46.0, 18.0);
    let h = btn(a, clear, 0.05);
    a.gfx.fill_rrect(clear, 9.0, mix(0x222222, 0x3A3A3A, h), 1.0);
    a.gfx.ui("Clear", clear, 10.0, 600, 0xD6D6D6, 1.0, Align::C);
    a.gfx.unzoom();
    hit(a, clear, Act::HistoryClear);
    let rows: Vec<_> = a.history.iter().cloned().enumerate().skip(a.history_top).take(3).collect();
    for (row, (i, n)) in rows.into_iter().enumerate() {
        let y = 34.0 + row as f32 * 33.0;
        let r = Rc::new(lx + 19.0, y, 374.0, 31.0);
        let h = btn(a, r, 0.01);
        if h > 0.0 {
            a.gfx.fill_rrect(r, 9.0, 0x222222, h.min(1.0));
        }
        notice_badge(a, &n, lx + 38.0, y + 15.5, 11.0);
        let (tx, tw) = (lx + 58.0, 276.0);
        let secs = n.born.elapsed().as_secs();
        let age = if secs < 60 { "now".to_string() } else if secs < 3600 { format!("{} min", secs / 60) } else { format!("{} h", secs / 3600) };
        a.gfx.ui(&age, Rc::new(lx + 330.0, y + 2.0, 56.0, 14.0), 10.0, 400, 0x8E8E93, 1.0, Align::R);
        a.gfx.ui(&n.title, Rc::new(tx, y + 1.0, tw, 15.0), 12.0, 600, WHITE, 1.0, Align::L);
        let sub = if n.body.is_empty() { n.source_name() } else { format!("{} · {}", n.source_name(), n.body) };
        a.gfx.ui(&sub, Rc::new(tx, y + 16.0, 328.0, 14.0), 10.5, 400, 0xA0A0A5, 1.0, Align::L);
        a.gfx.unzoom();
        hit(a, r, Act::HistoryOpen(i));
    }
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
    if a.history_open {
        history(a, lx);
        return;
    }
    let settings = a.page == Page::Water && a.water_settings;
    let lyr = if a.page == Page::Music { a.lyr_mix.x.clamp(0.0, 1.0) } else { 0.0 };
    let base = a.gfx.alpha;
    a.gfx.alpha = base * (1.0 - lyr);
    if a.gfx.alpha > 0.01 {
        header(a, lx);
    }
    a.gfx.alpha = base;
    let s = a.slide.x.clamp(0.0, 1.2);
    let dir = a.slide_dir;
    if s > 0.01 {
        let n = a.hits.len();
        let prev = a.prev_page;
        let base = a.gfx.alpha;
        a.gfx.alpha = base * s.min(1.0);
        a.gfx.offset(-dir * 30.0 * (1.0 - s), 0.0);
        page(a, prev, lx);
        a.gfx.offset(0.0, 0.0);
        a.gfx.alpha = base;
        a.hits.truncate(n);
    }
    let base = a.gfx.alpha;
    a.gfx.alpha = base * (1.0 - s).clamp(0.0, 1.0);
    a.gfx.offset(dir * 30.0 * s, 0.0);
    let p = a.page;
    page(a, p, lx);
    a.gfx.offset(0.0, 0.0);
    a.gfx.alpha = base;
    if !settings && lyr < 0.99 {
        let base = a.gfx.alpha;
        a.gfx.alpha = base * (1.0 - lyr);
        chevrons(a, lx);
        dots(a, lx);
        a.gfx.alpha = base;
    }
}

fn page(a: &mut App, p: Page, lx: f32) {
    match p {
        Page::Music => music(a, lx),
        Page::Pomo => pomo(a, lx),
        Page::Water => {
            if a.water_settings {
                water_settings(a, lx)
            } else {
                water(a, lx)
            }
        }
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

// ---------------------------------------------------------------- pomodoro

fn pomo(a: &mut App, lx: f32) {
    title(a, lx, "Pomodoro", None);
    let (cx, cy) = (lx + 80.0, 74.0);
    // Glow halo then the ring itself, lit from the top-left. The ring shows the time left: the
    // elapsed part opens clockwise from 12 o'clock.
    use std::f32::consts::{FRAC_PI_2, TAU};
    let (r, tl, br) = (31.8, 0xFD685D, 0xF33548);
    let (left_s, total_s, running, phase_label) = a.pomo_view();
    let left = (left_s / total_s).clamp(0.0, 1.0) as f32;
    let (from, to) = (-FRAC_PI_2 + TAU * (1.0 - left), -FRAC_PI_2 + TAU);
    for (w, alpha) in [(18.4, 0.07), (12.8, 0.11), (8.8, 0.18)] {
        if left > 0.999 {
            a.gfx.stroke_circle_diag(cx, cy, r, w, (tl, alpha), (br, alpha));
        } else {
            a.gfx.arc_diag(cx, cy, r, from, to, w, (tl, alpha), (br, alpha));
        }
    }
    if left > 0.999 {
        a.gfx.stroke_circle_diag(cx, cy, r, 6.2, (tl, 1.0), (br, 1.0));
    } else {
        a.gfx.arc_diag(cx, cy, r, from, to, 6.2, (tl, 1.0), (br, 1.0));
    }
    icon::stopwatch(&a.gfx, cx, cy + 2.5, 16.2, WHITE, 1.0);

    let secs = left_s.ceil() as u64;
    let label = format!("{:02}:{:02}", secs / 60, secs % 60);
    a.gfx.ui(&label, Rc::new(lx + 205.0, 32.4, 120.0, 34.0), 30.0, 700, WHITE, 1.0, Align::C);
    let rr = Rc::new(lx + 318.0, 38.0, 24.0, 24.0);
    let h = btn(a, rr, 0.15);
    a.gfx.fill_circle(lx + 329.6, 50.0, 11.2, 11.2, mix(0x191919, 0x333333, h), 1.0);
    icon::reset(&a.gfx, lx + 330.0, 50.4, 5.4, WHITE, 1.0);
    a.gfx.unzoom();
    hit(a, rr, Act::PomoReset);
    a.gfx.ui(&phase_label, Rc::new(lx + 206.6, 67.4, 120.0, 16.0), 13.0, 400, LABEL, 1.0, Align::C);

    let short = Rc::new(lx + 146.0, 92.0, 84.0, 28.0);
    let focus = Rc::new(lx + 302.0, 92.0, 83.2, 28.0);
    for (r, p, text) in [(short, Phase::Short, "Short Break"), (focus, Phase::Focus, "Focus Time")] {
        let on = match a.brain.focus.as_ref() {
            Some(f) => f.resting() == (p != Phase::Focus),
            None => a.pomo_mode == p,
        };
        let h = btn(a, r, 0.06);
        a.gfx.fill_rrect(r, 14.0, mix(if on { CHIP } else { 0x171717 }, 0x444444, h * 0.5), 1.0);
        let ix = r.x + 14.0;
        if p == Phase::Short {
            icon::cup(&a.gfx, ix, r.cy() + 0.4, 0xEDEDED, 1.0);
        } else {
            icon::target(&a.gfx, ix, r.cy(), 0xEDEDED, 1.0);
        }
        a.gfx.ui(text, Rc::new(r.x + 24.8, r.y, r.w - 24.8, r.h), 10.0, 400, 0xEDEDED, 1.0, Align::L);
        a.gfx.unzoom();
        hit(a, r, Act::PomoPhase(p));
    }
    let play = Rc::new(lx + 238.0, 92.0, 54.4, 28.0);
    let h = btn(a, play, 0.08);
    a.gfx.fill_rrect(play, 14.0, mix(RED, 0xFF7A7A, h * 0.6), 1.0);
    if running {
        icon::pause(&a.gfx, play.cx(), play.cy(), 11.0, WHITE, 1.0);
    } else {
        icon::play(&a.gfx, play.cx() + 1.0, play.cy(), 11.0, WHITE, 1.0);
    }
    a.gfx.unzoom();
    hit(a, play, Act::PomoToggle);
}

// ---------------------------------------------------------------- water

fn water(a: &mut App, lx: f32) {
    title(a, lx, "Water", Some((" Reminder", BLUE)));
    let clock = a.clock();
    let cfg = &a.brain.cfg;
    let (taken, goal, on) = (a.brain.stats.water * cfg.cup_ml, cfg.water_goal * cfg.cup_ml, cfg.reminders.water.enabled);
    let pct = (taken as f32 * 100.0 / goal.max(1) as f32).round() as u32;
    // The next reminder, as a clock time (demo captures keep their fixed clock).
    let next = match a.brain.water_in(chrono::Local::now()) {
        Some(s) if a.demo.is_none() => (clock.minutes() + (s as u32).div_ceil(60)) % (24 * 60),
        _ => clock.minutes() + 60,
    };
    let big = format!("{:.1}", taken as f32 / 1000.0);
    a.gfx.ui(&big, Rc::new(lx + 43.8, 30.4, 80.0, 32.0), 30.0, 700, WHITE, 1.0, Align::L);
    let bw = a.gfx.measure(&big, 30.0, 700);
    let of = format!("/ {:.1} L", goal as f32 / 1000.0);
    a.gfx.ui(&of, Rc::new(lx + 43.8 + bw + 5.0, 45.2, 70.0, 16.0), 13.0, 600, GREY, 1.0, Align::L);

    // The knob slides and the track color blends with the eased switch position.
    let tg = Rc::new(lx + 162.0, 36.8, 34.4, 20.8);
    let _ = on;
    let k = a.toggle_x.x;
    let h = btn(a, tg, 0.06);
    a.gfx.fill_rrect(tg, 10.4, mix(0x3A3A3A, TOGGLE, k), 1.0);
    let kx = tg.x + 10.0 + (tg.w - 20.0) * k;
    let kr = 8.0 + 0.6 * h.max(0.0);
    a.gfx.fill_circle(kx, tg.cy() - 0.4, kr, kr, WHITE, 1.0);
    icon::bell(&a.gfx, kx, tg.cy() - 0.4, mix(0x7A7A7A, TOGGLE, k), 1.0);
    a.gfx.unzoom();
    hit(a, tg, Act::WaterOn);

    a.gfx.ui("Today ·", Rc::new(lx + 44.0, 63.4, 60.0, 16.0), 11.5, 400, GREY, 1.0, Align::L);
    let pct_s = format!("{pct}%");
    a.gfx.ui(&pct_s, Rc::new(lx + 84.1, 63.4, 60.0, 16.0), 11.5, 700, BLUE, 1.0, Align::L);
    let nx = lx + 84.1 + a.gfx.measure(&pct_s, 11.5, 700) + 4.9;
    a.gfx.ui(&format!("· Next {}", hm_pad(next)), Rc::new(nx, 63.4, 120.0, 16.0), 11.5, 400, GREY, 1.0, Align::L);
    let bar = Rc::new(lx + 43.2, 82.4, 153.2, 4.8);
    a.gfx.fill_rrect(bar, 2.4, 0x212121, 1.0);
    if taken > 0 {
        a.gfx.fill_rrect(Rc::new(bar.x, bar.y, (bar.w * pct as f32 / 100.0).clamp(bar.h, bar.w), bar.h), 2.4, 0x64DFFF, 1.0);
    }

    let drink = Rc::new(lx + 43.6, 97.6, 59.2, 26.4);
    let h = btn(a, drink, 0.07);
    a.gfx.fill_rrect(drink, 13.2, mix(0x0D457F, 0x1663B0, h), 1.0);
    icon::droplet(&a.gfx, drink.x + 14.4, drink.cy() + 2.1, 8.1, CYAN, 1.0);
    a.gfx.ui("Drink", Rc::new(drink.x + 26.4, drink.y, 40.0, drink.h), 10.0, 400, WHITE, 1.0, Align::L);
    a.gfx.unzoom();
    hit(a, drink, Act::Drank);
    let set = Rc::new(lx + 108.8, 97.6, 70.4, 26.4);
    let h = btn(a, set, 0.07);
    a.gfx.fill_rrect(set, 13.2, mix(0x19191C, 0x34343A, h), 1.0);
    icon::sliders(&a.gfx, set.x + 13.6, set.cy(), 0xE3E3E3, 1.0);
    a.gfx.ui("Settings", Rc::new(set.x + 26.0, set.y, 44.0, set.h), 10.0, 400, 0xE3E3E3, 1.0, Align::L);
    a.gfx.unzoom();
    hit(a, set, Act::WaterSettings(true));

    a.gfx.fill_rrect(Rc::new(lx + 205.2, 36.0, 1.6, 84.0), 0.8, 0x131313, 1.0);
    for (i, dx) in [359.6f32, 366.8].iter().enumerate() {
        let active = a.water_sub as usize == i;
        let r = Rc::new(lx + dx - 4.0, 35.0, 8.0, 12.0);
        let h = btn(a, r, 0.4);
        a.gfx.fill_circle(lx + dx, 40.8, 2.0, 2.0, if active { 0xE5E5E5 } else { mix(0x555555, 0x9A9A9A, h) }, 1.0);
        a.gfx.unzoom();
        hit(a, r, Act::WaterSub(i as u8));
    }
    if a.water_sub == 0 {
        gauge(a, lx + 293.2, 78.4, pct);
    } else {
        chart(a, lx);
    }
}

fn hm_pad(min: u32) -> String {
    format!("{:02}:{:02}", min / 60, min % 60)
}

fn gauge(a: &mut App, cx: f32, cy: f32, pct: u32) {
    use std::f32::consts::PI;
    let r = 32.8;
    let (from, to) = (PI * 0.75, PI * 2.25);
    // Drinking produces a restrained pulse in the gauge, rather than a
    // static number jump. It is drawn before the dial so text stays crisp.
    let splash = a.water_pulse.x.max(0.0).min(0.22) / 0.22;
    if splash > 0.0 {
        for (radius, alpha) in [(r + 5.0, 0.30), (r + 12.0, 0.16)] {
            a.gfx.stroke_circle(cx, cy, radius + (1.0 - splash) * 7.0, 1.4, CYAN, alpha * splash);
        }
    }
    a.gfx.arc_between(cx, cy, r, from, to, 6.4, 0x0F2640, 1.0);
    let f = (pct as f32 / 100.0).clamp(0.0, 1.0);
    if f > 0.0 {
        a.gfx.arc_between(cx, cy, r, from, from + (to - from) * f, 6.4, 0x2AA8FF, 1.0);
    }
    let end = from + (to - from) * f;
    let ex = cx + r * end.cos();
    let ey = cy + r * end.sin();
    if splash > 0.0 {
        a.gfx.glow(ex, ey, 14.0, CYAN, 0.32 * splash);
    }
    a.gfx.fill_circle(ex, ey, 3.6 + splash * 1.3, 3.6 + splash * 1.3, BLUE, 1.0);
    a.gfx.ui(&pct.to_string(), Rc::new(cx - 30.0, cy - 19.4, 60.0, 30.0), 27.0, 700, WHITE, 1.0, Align::C);
    icon::droplet(&a.gfx, cx - 6.0, cy + 17.6, 5.3, CYAN, 1.0);
    a.gfx.ui("%", Rc::new(cx + 0.7, cy + 9.9, 14.0, 14.0), 11.5, 400, 0xD9D9D9, 1.0, Align::L);
}

fn chart(a: &mut App, lx: f32) {
    let x0 = lx + 219.6;
    let x1 = lx + 336.4;
    for (y, label) in [(56.4f32, "500"), (81.2, "250"), (106.0, "0 ml")] {
        a.gfx.line(x0, y, x1, y, 0.8, if label == "0 ml" { 0x1B4259 } else { 0x373737 }, 1.0);
        a.gfx.ui(label, Rc::new(lx + 344.4, y - 6.0, 40.0, 12.0), 9.5, 400, 0x808080, 1.0, Align::L);
    }
    // One bar per hour of the user's day, from when it usually starts to bedtime.
    let (day, bed) = a.brain.water_day();
    let start = (chrono::Timelike::hour(&day) * 60).min(20 * 60);
    let mut end = chrono::Timelike::hour(&bed) * 60;
    if end <= start {
        end = (end + 24 * 60).min(24 * 60);
    }
    let cups = a.brain.cups_by_hour(chrono::Local::now());
    let cup = a.brain.cfg.cup_ml as f32;
    let slots = ((end - start) / 60).max(1);
    let bw = (x1 - x0) / slots as f32;
    let max = 500.0;
    for k in 0..slots {
        let hour = ((start / 60 + k) % 24) as usize;
        let per = cups[hour] as f32 * cup;
        let x = x0 + k as f32 * bw + bw * 0.25;
        a.gfx.fill_rrect(Rc::new(x, 58.0, bw * 0.5, 48.0), 0.8, 0x0F2640, 0.8);
        if per > 0.0 {
            let hgt = (per / max * 49.6).min(49.6);
            a.gfx.fill_rrect(Rc::new(x, 106.0 - hgt, bw * 0.5, hgt), 0.8, 0x2AA8FF, 1.0);
        }
    }
    let span = (end - start) as f32;
    for hour in [9u32, 12, 15] {
        let m = hour * 60;
        if m >= start && m <= end {
            let x = x0 + (m - start) as f32 / span * (x1 - x0);
            a.gfx.ui(&format!("{hour:02}"), Rc::new(x - 10.0, 110.0, 20.0, 12.0), 9.5, 400, 0x808080, 1.0, Align::C);
        }
    }
}

fn water_settings(a: &mut App, lx: f32) {
    let back = Rc::new(lx + 19.0, 8.0, 22.0, 22.0);
    let h = btn(a, back, 0.18);
    a.gfx.fill_circle(lx + 30.0, 19.2, 8.8, 8.8, mix(CHEVRON, 0x8A8A8A, h), 1.0);
    icon::chevron(&a.gfx, lx + 30.0, 19.2, true, 0x000000, 1.0);
    a.gfx.unzoom();
    hit(a, back, Act::WaterSettings(false));
    a.gfx.ui("Settings", Rc::new(lx + 46.0, 8.2, 100.0, 22.0), 15.0, 600, WHITE, 1.0, Align::L);

    for (label, y) in [("Cup", 50.4f32), ("Goal", 80.8), ("Day", 110.8)] {
        a.gfx.ui(label, Rc::new(lx + 22.0, y - 9.8, 60.0, 18.0), 13.0, 600, 0xD9D9D9, 1.0, Align::L);
    }
    let (cup, cups, every, goal, start, end) = {
        let cfg = &a.brain.cfg;
        let (day, bed) = a.brain.water_day();
        let hm = |t: chrono::NaiveTime| chrono::Timelike::hour(&t) * 60 + chrono::Timelike::minute(&t);
        (cfg.cup_ml, cfg.water_goal, cfg.reminders.water.minutes, cfg.water_goal * cfg.cup_ml, hm(day), hm(bed))
    };
    stepper(a, lx, 50.4, &format!("{cup} ml"), Act::Bottle(-1), Act::Bottle(1), 86.0, 162.0);
    a.gfx.ui(&format!("{cups} cups a day"), Rc::new(lx + 179.6, 43.2, 130.0, 13.0), 9.5, 400, 0x808080, 1.0, Align::L);

    for (i, g) in model::GOALS.iter().enumerate() {
        let r = Rc::new(lx + 74.8 + i as f32 * 56.0, 68.0, 50.4, 24.0);
        let on = goal == *g;
        let h = btn(a, r, 0.07);
        if on {
            a.gfx.fill_rrect_hgrad(r, 12.0, (0x66E1FF, 1.0), (0x0E74FF, 1.0));
        } else {
            a.gfx.fill_rrect(r, 12.0, mix(0x191919, 0x333333, h), 1.0);
        }
        a.gfx.ui(&format!("{:.1} L", *g as f32 / 1000.0), r, 11.5, 400, if on { 0x0B2236 } else { 0xE8E8E8 }, 1.0, Align::C);
        a.gfx.unzoom();
        hit(a, r, Act::Goal(*g));
    }
    // The day starts when the user usually starts (learned); it ends at bedtime, which can be set.
    a.gfx.ui(&hm_pad(start), Rc::new(lx + 75.0, 100.6, 98.0, 20.0), 14.0, 600, 0x9A9A9A, 1.0, Align::C);
    a.gfx.ui("to", Rc::new(lx + 174.0, 102.2, 20.0, 15.0), 11.5, 400, GREY, 1.0, Align::C);
    stepper(a, lx, 110.8, &hm_pad(end), Act::End(-1), Act::End(1), 206.0, 282.0);

    let card = Rc::new(lx + 306.0, 36.0, 92.0, 62.4);
    a.gfx.fill_rrect(card, 12.0, 0x0F1926, 1.0);
    a.gfx.ui(&cups.to_string(), Rc::new(card.x, 38.4, card.w, 26.0), 22.0, 700, CYAN, 1.0, Align::C);
    a.gfx.ui("cups", Rc::new(card.x, 59.6, card.w, 13.0), 10.0, 400, 0x93979D, 1.0, Align::C);
    a.gfx.fill_rrect(Rc::new(lx + 331.6, 75.2, 40.8, 1.6), 0.8, 0x212A36, 1.0);
    let rs = format!("~{every}");
    let rw = a.gfx.measure(&rs, 15.0, 700) + 3.0 + a.gfx.measure("min", 10.0, 400);
    let rx = card.cx() - rw / 2.0 + 0.8;
    a.gfx.ui(&rs, Rc::new(rx, 80.8, 40.0, 16.0), 15.0, 700, CYAN, 1.0, Align::L);
    let nw = a.gfx.measure(&rs, 15.0, 700);
    a.gfx.ui("min", Rc::new(rx + nw + 3.0, 83.8, 40.0, 13.0), 10.0, 400, 0x93979D, 1.0, Align::L);
    let reset = Rc::new(lx + 306.0, 104.0, 92.0, 18.4);
    let h = btn(a, reset, 0.05);
    a.gfx.fill_rrect(reset, 9.2, mix(0x19191C, 0x3A2020, h), 1.0);
    icon::trash(&a.gfx, lx + 318.0, reset.cy(), 0xC5C5C6, 1.0);
    a.gfx.ui("Reset", Rc::new(lx + 328.4, reset.y, 50.0, reset.h), 9.5, 400, 0xC5C5C6, 1.0, Align::L);
    a.gfx.unzoom();
    hit(a, reset, Act::WaterReset);
}

#[allow(clippy::too_many_arguments)]
fn stepper(a: &mut App, lx: f32, cy: f32, value: &str, dec: Act, inc: Act, mx: f32, px: f32) {
    for (x, plus, act) in [(mx, false, dec), (px, true, inc)] {
        let r = Rc::new(lx + x - 10.4, cy - 10.4, 20.8, 20.8);
        btn(a, r, 0.15);
        icon::minus_plus(&a.gfx, lx + x, cy, plus, 1.0);
        a.gfx.unzoom();
        hit(a, r, act);
    }
    a.gfx.ui(value, Rc::new(lx + mx + 11.0, cy - 10.2, px - mx - 22.0, 20.0), 14.0, 600, WHITE, 1.0, Align::C);
}

// ---------------------------------------------------------------- shelf

fn shelf(a: &mut App, lx: f32) {
    title(a, lx, "Shelf", None);
    let area = Rc::new(lx + 43.6, 32.0, 336.8, 91.2);
    let drag = a.drag_over;
    let items = a.shelf.items.clone();
    if drag && !items.is_empty() {
        // Nusa keeps the Clear chip next to the title while files are dragged in.
        let clear = Rc::new(lx + 84.0, 10.8, 40.0, 17.6);
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

fn todo(a: &mut App, lx: f32) {
    title(a, lx, "Todo", Some(("List", TODO_GREEN)));
    let items = a.todo_items();
    let done = items.iter().filter(|t| t.done).count();
    let total = items.len();
    a.gfx.ui(&format!("{done}/{total}"), Rc::new(lx + 44.6, 26.4, 140.0, 40.0), 36.0, 700, WHITE, 1.0, Align::L);
    a.gfx.ui("Task Done Today", Rc::new(lx + 43.8, 72.0, 140.0, 16.0), 12.0, 400, LABEL, 1.0, Align::L);

    let form_open = a.form.is_some();
    let add = Rc::new(lx + 40.4, 93.6, 76.8, 26.4);
    let h = btn(a, add, 0.07);
    a.gfx.fill_rrect(add, 13.2, mix(if form_open { 0x0F2B17 } else { 0x1E1E1E }, 0x383838, h * 0.7), 1.0);
    icon::plus_circle(&a.gfx, add.x + 16.4, add.cy(), if form_open { TODO_GREEN } else { WHITE }, 1.0);
    a.gfx.ui("Add Task", Rc::new(add.x + 27.2, add.y, 50.0, add.h), 10.0, 400, if form_open { TODO_GREEN } else { 0xD6D6D6 }, 1.0, Align::L);
    a.gfx.unzoom();
    hit(a, add, Act::TodoAdd);
    let edit = Rc::new(lx + 121.2, 93.6, 70.4, 26.4);
    let enabled = total > 0;
    let editing = a.todo_edit;
    let h = if enabled { btn(a, edit, 0.07) } else { 0.0 };
    a.gfx.fill_rrect(edit, 13.2, mix(if editing { 0x0F2B17 } else { 0x1E1E1E }, 0x383838, h * 0.7), 1.0);
    let ec = if editing {
        TODO_GREEN
    } else if enabled {
        0xD6D6D6
    } else {
        0x565656
    };
    icon::pen(&a.gfx, edit.x + 13.8, edit.cy(), if enabled || editing { ec } else { 0x676767 }, 1.0);
    a.gfx.ui("Edit Task", Rc::new(edit.x + 24.0, edit.y, 46.0, edit.h), 10.0, 400, ec, 1.0, Align::L);
    a.gfx.unzoom();
    hit(a, edit, Act::TodoEditMode);

    let card = Rc::new(lx + 196.4, 32.0, 179.2, 91.2);
    if form_open {
        form(a, lx);
        return;
    }
    a.gfx.fill_rrect(card, 12.0, 0x1A1A1A, 1.0);
    if total == 0 {
        icon::checklist(&a.gfx, card.cx(), 66.0, 0x8A8C90, 1.0);
        a.gfx.ui("No tasks.", Rc::new(card.x + 0.4, 91.0, card.w, 16.0), 13.0, 400, 0x6D6D6D, 1.0, Align::C);
        return;
    }
    a.gfx.push_clip(card);
    for (i, item) in items.into_iter().enumerate().take(3) {
        let y = card.y + 4.0 + i as f32 * 29.0;
        let rr = Rc::new(card.x + 4.0, y, card.w - 8.0, 27.0);
        let (t, time, is_done) = (item.title, item.time, item.done);
        let h = btn(a, rr, 0.03);
        if h > 0.0 {
            a.gfx.fill_rrect(rr, 9.0, 0x2A2A2A, 0.7 * h.min(1.0));
        }
        if editing {
            a.gfx.stroke_rrect(rr, 9.0, 1.0, 0x3A3A3A, 1.0);
        }
        let ccx = card.x + 17.0;
        let cy = y + 13.0;
        if is_done {
            a.gfx.fill_circle(ccx, cy, 6.8, 6.8, TODO_GREEN, 1.0);
            a.gfx.polyline(&[(ccx - 3.0, cy), (ccx - 0.8, cy + 2.4), (ccx + 3.2, cy - 2.4)], 1.5, 0x000000, 1.0);
        } else {
            a.gfx.stroke_circle(ccx, cy, 6.8, 1.4, 0xDDDDDD, 1.0);
        }
        let tcol = if is_done { 0x8C8C8C } else { WHITE };
        if time.is_empty() {
            a.gfx.ui(&t, Rc::new(card.x + 29.6, y, card.w - 60.0, 26.0), 13.0, 400, tcol, 1.0, Align::L);
        } else {
            // Two lines with clear space between them: title, then the time span.
            let span = match time.split_once('-') {
                Some((start, end)) if !end.is_empty() && end != "00:00" => format!("{start} – {end}"),
                Some((start, _)) => start.to_string(),
                None => time.clone(),
            };
            a.gfx.ui(&t, Rc::new(card.x + 29.6, y + 0.5, card.w - 60.0, 14.0), 12.5, 400, tcol, 1.0, Align::L);
            a.gfx.ui(&span, Rc::new(card.x + 29.6, y + 17.0, card.w - 60.0, 10.0), 9.5, 400, GREY, 1.0, Align::L);
        }
        a.gfx.unzoom();
        hit(a, rr, Act::TodoToggle(i));
        if editing {
            icon::x_circle(&a.gfx, card.x + card.w - 16.0, cy, 8.0, 0xE53833, WHITE, 1.0);
            hit(a, Rc::new(card.x + card.w - 26.0, y + 2.0, 20.0, 22.0), Act::TodoDelete(i));
        }
    }
    a.gfx.pop_clip();
}

fn form(a: &mut App, lx: f32) {
    let (text, digits, field) = {
        let f = a.form.as_ref().unwrap();
        (f.title.clone(), f.digits.clone(), f.field)
    };
    let tf = Rc::new(lx + 201.2, 39.2, 169.6, 27.2);
    a.gfx.fill_rrect(tf, 11.0, 0x121212, 1.0);
    a.gfx.stroke_rrect(tf, 11.0, 1.4, if field == 0 { 0xDADADA } else { 0x3A3A3A }, 1.0);
    let inner = Rc::new(tf.x + 11.0, tf.y, tf.w - 34.0, tf.h);
    if text.is_empty() {
        a.gfx.ui("Task title", inner, 13.0, 400, GREY, 1.0, Align::L);
    } else {
        a.gfx.ui(&text, inner, 13.0, 400, WHITE, 1.0, Align::L);
        icon::x_circle(&a.gfx, tf.x + tf.w - 15.2, tf.cy(), 6.4, 0xDADADA, 0x121212, 1.0);
        hit(a, Rc::new(tf.x + tf.w - 24.0, tf.y, 22.0, tf.h), Act::FormClear);
    }
    let blink = (a.born.elapsed().as_millis() / 530) % 2 == 0;
    if field == 0 && blink {
        let cw = a.gfx.measure(&text, 13.0, 400).min(inner.w);
        a.gfx.line(inner.x + cw + 0.8, tf.y + 7.0, inner.x + cw + 0.8, tf.y + tf.h - 7.0, 1.2, WHITE, 1.0);
    }
    hit(a, Rc::new(tf.x, tf.y, tf.w - 26.0, tf.h), Act::FormField(0));

    let tm = Rc::new(lx + 201.2, 70.4, 169.6, 22.4);
    a.gfx.fill_rrect(tm, 11.0, 0x1E1E1E, 1.0);
    if field == 1 {
        a.gfx.stroke_rrect(tm, 11.0, 1.4, 0xDADADA, 1.0);
    }
    icon::clock(&a.gfx, tm.x + 13.6, tm.cy(), WHITE, 1.0);
    let mask = time_mask(&digits);
    let typed: String = mask.chars().take(typed_len(&digits)).collect();
    let rest: String = mask.chars().skip(typed_len(&digits)).collect();
    a.gfx.ui(&typed, Rc::new(tm.x + 26.4, tm.y, 120.0, tm.h), 13.0, 400, WHITE, 1.0, Align::L);
    let tw = a.gfx.measure(&typed, 13.0, 400);
    a.gfx.ui(&rest, Rc::new(tm.x + 26.4 + tw, tm.y, 120.0, tm.h), 13.0, 400, 0x5F5F61, 1.0, Align::L);
    hit(a, tm, Act::FormField(1));

    let cancel = Rc::new(lx + 201.2, 98.4, 82.4, 20.8);
    let h = btn(a, cancel, 0.07);
    a.gfx.fill_rrect(cancel, 10.4, mix(0x2A2A2A, 0x444444, h), 1.0);
    a.gfx.ui("Cancel", cancel, 11.5, 400, WHITE, 1.0, Align::C);
    a.gfx.unzoom();
    hit(a, cancel, Act::FormCancel);
    let save = Rc::new(lx + 288.4, 98.4, 81.6, 20.8);
    let h = btn(a, save, 0.07);
    a.gfx.fill_rrect(save, 10.4, mix(TODO_GREEN, 0x5FD873, h), 1.0);
    a.gfx.ui("Save", save, 11.5, 400, WHITE, 1.0, Align::C);
    a.gfx.unzoom();
    hit(a, save, Act::FormSave);
}

/// Number of mask characters covered by the typed digits (separators included).
fn typed_len(digits: &str) -> usize {
    match digits.len() {
        0 => 0,
        1 => 1,
        2 => 2,
        3 => 4,
        4 => 5,
        5 => 7,
        6 => 8,
        7 => 10,
        _ => 11,
    }
}

