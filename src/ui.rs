//! Layout and drawing. Coordinates are layout units measured from the reference
//! captures; panel-local x is relative to the notch's left edge.

use crate::gfx::{Align, Font, Rc};
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
    /// Water reminder toast buttons.
    ToastDrank,
    ToastSnooze,
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
    Start(i32),
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
    pub fn speaker(g: &Gfx, cx: f32, cy: f32, muted: bool, hex: u32, a: f32) {
        // (cx, cy) is the mouth of the cone; waves are centered on it.
        g.polygon(&[(cx - 10.0, cy - 4.4), (cx - 4.8, cy - 4.4), (cx, cy - 8.0), (cx, cy + 8.0), (cx - 4.8, cy + 4.4), (cx - 10.0, cy + 4.4)], hex, a);
        if muted {
            g.line(cx + 3.2, cy - 3.2, cx + 9.2, cy + 2.8, 2.2, hex, a);
            g.line(cx + 3.2, cy + 2.8, cx + 9.2, cy - 3.2, 2.2, hex, a);
        } else {
            g.arc_between(cx, cy, 4.4, -1.2, 1.2, 2.2, hex, a);
            g.arc_between(cx, cy, 8.0, -0.95, 0.95, 2.2, hex, a);
        }
    }
    pub fn bluetooth(g: &Gfx, cx: f32, cy: f32, hex: u32, a: f32) {
        // The rune: a vertical stem with two closed loops on the right and two crossing arms.
        let (h, d) = (6.4, 3.4);
        g.polyline(&[(cx - d, cy - h * 0.5), (cx + d, cy + h * 0.5), (cx, cy + h), (cx, cy - h), (cx + d, cy - h * 0.5), (cx - d, cy + h * 0.5)], 1.6, hex, a);
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
    let (visual_cx, visual_w) = a.notch_geometry();
    let (w, h) = (visual_w.max(1.0), a.sh.x.max(1.0));
    let cx = visual_cx;
    let t = ((h - 30.0) / 110.0).clamp(0.0, 1.0);
    // Corner and ear radii fitted to the reference silhouette (compact 30 tall → expanded 140 tall).
    let radius = 12.4 + 19.2 * t;
    let ear = 10.0 + 12.2 * t;
    if a.minimal() && a.mode == crate::Mode::Compact {
        // A near-invisible pad keeps the tiny minimal notch easy to hover.
        a.gfx.fill_rrect(Rc::new(cx - 70.0, 0.0, 140.0, 18.0), 0.0, 0x000000, 0.005);
    }
    let na = 1.0;
    a.gfx.alpha = na;
    a.gfx.notch(cx, w, h, radius, ear, 0x000000, 1.0);
    let left = cx - w / 2.0;

    // The drop light sits behind everything on the panel, header included.
    if a.page == Page::Shelf && a.fade.x > 0.01 && (a.drag_over || a.ripple.is_some()) {
        // Light effects live inside the notch's own outline, never past its rim.
        let clipped = a.gfx.push_notch_clip(cx, w, h, radius, ear);
        a.gfx.alpha = a.fade.x.clamp(0.0, 1.0);
        let (fx, fy, st) = aurora_state(a);
        aurora(a, fx, fy, st);
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
    let fade_from = if tall { 58.0 } else { 36.0 };
    let small_alpha = (1.0 - (h - fade_from) / 14.0).clamp(0.0, 1.0) * (1.0 - a.fade.x).clamp(0.0, 1.0);

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
        if base > 0.01 && h > 18.0 && !a.minimal() {
            a.gfx.alpha = na * small_alpha * base;
            compact(a, left, w);
        }
        if pm > 0.01 && a.lyric_line().is_some() {
            a.gfx.alpha = na * small_alpha * pm;
            lyric_pill(a, left, w);
        }
        if tm > 0.01 {
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
    a.gfx.alpha = 1.0;
}

// ---------------------------------------------------------------- compact

fn compact(a: &mut App, left: f32, w: f32) {
    let cy = 14.8;
    let t = a.born.elapsed().as_secs_f32();
    let charging = a.power.is_some_and(|p| p.battery && p.ac);
    if charging {
        icon::charging(&a.gfx, left + 20.0, cy, 8.2, GREEN, 1.0);
    } else if a.settings.pet {
        let look = ((a.mouse.0 - (left + 20.0)) / 80.0).clamp(-1.0, 1.0) * 1.0;
        mascot(a, left + 20.0, cy, 20.0, t, look);
    }
    let time = a.clock().time_label();
    a.gfx.ui(&time, Rc::new(left, 5.0, w, 20.0), 16.0, 700, WHITE, 1.0, Align::C);
    right_indicator(a, left + w - 28.0, cy);
}

/// Equalizer bars while music plays, or the focus-timer pie while it runs.
fn right_indicator(a: &mut App, x: f32, cy: f32) {
    if a.pomo.running() {
        let frac = (a.pomo.left() / a.pomo.phase.secs()) as f32;
        let cx = x + 9.2;
        a.gfx.fill_circle(cx, cy, 9.6, 9.6, RED_PIE, 0.35);
        a.gfx.pie(cx, cy, 9.6, frac, RED_PIE, 1.0);
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

fn mascot(a: &mut App, cx: f32, cy: f32, s: f32, t: f32, look: f32) {
    const FUR: u32 = 0xF7C873;
    let g = &a.gfx;
    g.triangle([(cx - s * 0.42, cy - s * 0.08), (cx - s * 0.34, cy - s * 0.52), (cx - s * 0.08, cy - s * 0.34)], FUR, 1.0);
    g.triangle([(cx + s * 0.42, cy - s * 0.08), (cx + s * 0.34, cy - s * 0.52), (cx + s * 0.08, cy - s * 0.34)], FUR, 1.0);
    g.fill_circle(cx, cy + s * 0.04, s * 0.46, s * 0.4, FUR, 1.0);
    let blink = if t % 3.7 < 0.13 { 0.15 } else { 1.0 };
    for side in [-1.0f32, 1.0] {
        g.fill_circle(cx + side * s * 0.16 + look, cy + s * 0.02, s * 0.06, s * 0.09 * blink, 0x2B1D0E, 1.0);
        g.fill_circle(cx + side * s * 0.29, cy + s * 0.17, s * 0.07, s * 0.04, 0xFF8A80, 0.7);
    }
    g.fill_circle(cx + look, cy + s * 0.14, s * 0.035, s * 0.025, 0x2B1D0E, 1.0);
}

fn lyric_pill(a: &mut App, left: f32, w: f32) {
    let line = a.lyric_line().unwrap_or_default();
    // Art-tinted glow rising from the bottom edge, as on the expanded lyrics view.
    let h = a.sh.x;
    let radius = 12.4 + 19.2 * ((h - 30.0) / 110.0).clamp(0.0, 1.0);
    a.gfx.fill_rrect_vgrad(Rc::new(left, 0.0, w, h), radius, (art_tint(a), 0.0), (art_tint(a), 0.85));
    icon::note(&a.gfx, left + 22.0, 16.0, 0xBDBDBD, 1.0);
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
            icon::speaker(&a.gfx, ix + 1.2, cy, muted, WHITE, 1.0);
            // The endpoint impact belongs to the toast silhouette.  Keeping
            // the meter itself stable avoids a second, conflicting stretch.
            let bar = Rc::new(left + 49.6, cy - 2.8, w - 49.6 - 24.0, 6.4);
            a.gfx.fill_rrect(bar, 3.2, 0x474747, 1.0);
            let lv = if live { a.vol_level.x } else { level.unwrap_or(0.0) }.clamp(0.0, 1.0);
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

/// Two-line toasts. Bluetooth: badge, name, "Connected · N%" counting up while a battery ring
/// fills around the headphones and a ripple runs out, then the ring dims. Water: a pulsing droplet
/// with Snooze / Drank buttons. App notices: initial badge, app name and message.
fn tall_toast(a: &mut App, left: f32, w: f32, t: &crate::Toast, live: bool) {
    let (kind, title, value) = (t.kind, t.title.as_str(), t.value.as_str());
    let age = t.born.elapsed().as_secs_f32();
    let cy = 29.6;
    let bx = left + 32.0;
    let tx = left + 64.0;
    match kind {
        ToastKind::Bluetooth(on) => {
            a.gfx.fill_circle(bx, cy, 14.4, 14.4, if on { 0x56C1F5 } else { 0x6B6B6B }, 1.0);
            a.gfx.fill_circle(bx, cy, 10.8, 10.8, if on { 0x0D2C44 } else { 0x222222 }, 1.0);
            icon::bluetooth(&a.gfx, bx, cy, WHITE, 1.0);
            let ex = left + w - 40.0;
            // Battery count-up over ~0.9 s, eased out.
            let k = (age / 0.9).clamp(0.0, 1.0);
            let ease = 1.0 - (1.0 - k).powi(3);
            let lv = t.level.unwrap_or(0.0) * ease;
            let sub = match t.level {
                Some(_) => format!("{value} · {}%", (lv * 100.0).round()),
                None => value.to_string(),
            };
            a.gfx.ui(title, Rc::new(tx, 13.6, w - 64.0 - 72.0, 19.0), 15.0, 700, WHITE, 1.0, Align::L);
            a.gfx.ui(&sub, Rc::new(tx, 33.6, w - 64.0 - 72.0, 15.0), 11.5, 400, 0x9A9A9A, 1.0, Align::L);
            // Ripple rings while counting, then the ring settles to a dim track.
            for d in [0.0f32, 0.35] {
                let q = ((age - d) / 0.8).clamp(0.0, 1.0);
                if on && q > 0.0 && q < 1.0 {
                    a.gfx.stroke_circle(ex, cy, 16.0 + 9.0 * q, 1.4, 0x46D9CE, 0.5 * (1.0 - q));
                }
            }
            let settle = ((age - 1.0) / 0.5).clamp(0.0, 1.0);
            a.gfx.fill_circle(ex, cy, 16.0, 16.0, 0x0C1414, 1.0);
            a.gfx.stroke_circle(ex, cy, 16.0, 2.0, 0x2A3A3A, 1.0);
            if on && lv > 0.0 {
                a.gfx.arc(ex, cy, 16.0, lv, 2.0, mix(0x46D9CE, 0x3A4A4A, settle), 1.0);
            }
            icon::headphones(&a.gfx, ex, cy, if on { WHITE } else { 0x9A9A9A }, 1.0);
        }
        ToastKind::Water => {
            // Droplet in a dark disc with a breathing cyan ring.
            let pulse = 0.5 + 0.5 * (age * 5.2).sin();
            a.gfx.glow(bx, cy, 20.0, 0x2FD6E0, 0.10 + 0.14 * pulse);
            a.gfx.fill_circle(bx, cy, 15.2, 15.2, 0x0B1A1E, 1.0);
            a.gfx.stroke_circle(bx, cy, 15.2, 1.6, 0x2FD6E0, 0.25 + 0.6 * pulse);
            icon::droplet(&a.gfx, bx, cy + 2.0, 9.6, 0x35E0E8, 1.0);
            let bw = 128.0;
            a.gfx.ui(title, Rc::new(tx, 13.6, w - 64.0 - bw - 8.0, 19.0), 15.0, 700, WHITE, 1.0, Align::L);
            // The original keeps this supporting line readable against the
            // transparent toast; #9A9A9A became visibly soft at fractional
            // DPI with grayscale antialiasing.
            a.gfx.ui(value, Rc::new(tx, 33.6, w - 64.0 - bw - 8.0, 15.0), 11.5, 400, 0xB2B2B2, 1.0, Align::L);
            let snooze = Rc::new(left + w - bw - 14.0, cy - 13.0, 58.0, 26.0);
            let drank = Rc::new(left + w - 66.0, cy - 13.0, 54.0, 26.0);
            let hs = if live { btn(a, snooze, 0.07) } else { 0.0 };
            a.gfx.fill_rrect(snooze, 13.0, mix(0x343434, 0x4A4A4A, hs), 1.0);
            a.gfx.ui("Snooze", snooze, 11.5, 600, 0xE6E6E6, 1.0, Align::C);
            a.gfx.unzoom();
            let hd = if live { btn(a, drank, 0.07) } else { 0.0 };
            a.gfx.fill_rrect(drank, 13.0, mix(0x1E50C8, 0x2A66E8, hd), 1.0);
            a.gfx.ui("Drank", drank, 11.5, 600, 0xBFD6FF, 1.0, Align::C);
            a.gfx.unzoom();
            if live {
                hit(a, snooze, Act::ToastSnooze);
                hit(a, drank, Act::ToastDrank);
            }
        }
        _ => {
            a.gfx.fill_circle(bx, cy, 14.4, 14.4, 0x2E2E2E, 1.0);
            let initial: String = value.chars().next().map(String::from).unwrap_or_default();
            a.gfx.ui(&initial, Rc::new(bx - 14.4, cy - 14.4, 28.8, 28.8), 13.0, 700, WHITE, 1.0, Align::C);
            a.gfx.ui(value, Rc::new(tx, 13.6, w - 88.0, 19.0), 15.0, 700, WHITE, 1.0, Align::L);
            a.gfx.ui(title, Rc::new(tx, 33.6, w - 88.0, 15.0), 11.5, 400, 0x9A9A9A, 1.0, Align::L);
        }
    }
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

fn expanded(a: &mut App, lx: f32) {
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
    // Glow halo then the ring itself, lit from the top-left.
    let (r, tl, br) = (31.8, 0xFD685D, 0xF33548);
    for (w, alpha) in [(18.4, 0.07), (12.8, 0.11), (8.8, 0.18)] {
        a.gfx.stroke_circle_diag(cx, cy, r, w, (tl, alpha), (br, alpha));
    }
    a.gfx.stroke_circle_diag(cx, cy, r, 6.2, (tl, 1.0), (br, 1.0));
    icon::stopwatch(&a.gfx, cx, cy + 2.5, 16.2, WHITE, 1.0);

    let label = a.pomo.label();
    a.gfx.ui(&label, Rc::new(lx + 205.0, 32.4, 120.0, 34.0), 30.0, 700, WHITE, 1.0, Align::C);
    let rr = Rc::new(lx + 318.0, 38.0, 24.0, 24.0);
    let h = btn(a, rr, 0.15);
    a.gfx.fill_circle(lx + 329.6, 50.0, 11.2, 11.2, mix(0x191919, 0x333333, h), 1.0);
    icon::reset(&a.gfx, lx + 330.0, 50.4, 5.4, WHITE, 1.0);
    a.gfx.unzoom();
    hit(a, rr, Act::PomoReset);
    a.gfx.ui(a.pomo.phase.label(), Rc::new(lx + 206.6, 67.4, 120.0, 16.0), 13.0, 400, LABEL, 1.0, Align::C);

    let short = Rc::new(lx + 146.0, 92.0, 84.0, 28.0);
    let focus = Rc::new(lx + 302.0, 92.0, 83.2, 28.0);
    for (r, p, text) in [(short, Phase::Short, "Short Break"), (focus, Phase::Focus, "Focus Time")] {
        let on = a.pomo.phase == p;
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
    if a.pomo.running() {
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
    let wd = &a.water;
    let (taken, goal, on, pct, next) = (wd.taken, wd.goal, wd.on, wd.percent(), wd.next_minute(clock.minutes()));
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
    let w = &a.water;
    let (start, end) = (w.start, w.end);
    let slots = w.slots().max(1);
    let bw = (x1 - x0) / slots as f32;
    let max = 500.0;
    for k in 0..slots {
        let hour = ((start + k * model::Water::INTERVAL) / 60) as usize;
        let per = w.hours[hour.min(23)] as f32 / 4.0;
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

    for (label, y) in [("Bottle", 50.4f32), ("Goal", 80.8), ("Hours", 110.8)] {
        a.gfx.ui(label, Rc::new(lx + 22.0, y - 9.8, 60.0, 18.0), 13.0, 600, 0xD9D9D9, 1.0, Align::L);
    }
    let (bottle, each, per, goal, start, end, slots, refills) = {
        let w = &a.water;
        (w.bottle, w.each(), w.per_bottle(), w.goal, w.start, w.end, w.slots(), w.refills())
    };
    stepper(a, lx, 50.4, &format!("{bottle} ml"), Act::Bottle(-1), Act::Bottle(1), 86.0, 162.0);
    a.gfx.ui(&format!("{each} ml each · {per}/bottle"), Rc::new(lx + 179.6, 43.2, 130.0, 13.0), 9.5, 400, 0x808080, 1.0, Align::L);

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
    stepper(a, lx, 110.8, &hm_pad(start), Act::Start(-1), Act::Start(1), 86.0, 162.0);
    a.gfx.ui("to", Rc::new(lx + 174.0, 102.2, 20.0, 15.0), 11.5, 400, GREY, 1.0, Align::C);
    stepper(a, lx, 110.8, &hm_pad(end), Act::End(-1), Act::End(1), 206.0, 282.0);

    let card = Rc::new(lx + 306.0, 36.0, 92.0, 62.4);
    a.gfx.fill_rrect(card, 12.0, 0x0F1926, 1.0);
    a.gfx.ui(&slots.to_string(), Rc::new(card.x, 38.4, card.w, 26.0), 22.0, 700, CYAN, 1.0, Align::C);
    a.gfx.ui("reminders", Rc::new(card.x, 59.6, card.w, 13.0), 10.0, 400, 0x93979D, 1.0, Align::C);
    a.gfx.fill_rrect(Rc::new(lx + 331.6, 75.2, 40.8, 1.6), 0.8, 0x212A36, 1.0);
    let rs = refills.to_string();
    let rw = a.gfx.measure(&rs, 15.0, 700) + 3.0 + a.gfx.measure("refills", 10.0, 400);
    let rx = card.cx() - rw / 2.0 + 0.8;
    a.gfx.ui(&rs, Rc::new(rx, 80.8, 20.0, 16.0), 15.0, 700, CYAN, 1.0, Align::L);
    let nw = a.gfx.measure(&rs, 15.0, 700);
    a.gfx.ui("refills", Rc::new(rx + nw + 3.0, 83.8, 40.0, 13.0), 10.0, 400, 0x93979D, 1.0, Align::L);
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
                        a.gfx.image(&bmp, Rc::new(tile.cx() - 20.0, tile.y + 8.0, 40.0, 40.0), 0.0, 1.0);
                    }
                    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    a.gfx.ui(&name, Rc::new(tile.x + 2.0, tile.y + 53.0, tile.w - 4.0, 14.0), 10.5, 600, WHITE, 1.0, Align::C);
                }
                a.gfx.alpha = base;
            }
            // Nusa keeps the target itself near-neutral charcoal.  The colour
            // comes from the broad violet aurora behind it, not from a blue
            // fill laid over every shelf item.
            a.gfx.fill_rrect(area, 12.0, 0x25262A, 0.42);
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
        let k = (1.0 - start.elapsed().as_secs_f32() / 0.6).clamp(0.0, 1.0);
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
        let enter = a.ripple.map(|r| ((r.0.elapsed().as_secs_f32() - 0.08) / 0.3).clamp(0.0, 1.0)).unwrap_or(1.0);
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
            a.gfx.image(&bmp, ib, 0.0, 1.0);
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

/// The panel's light while files are dragged in and after a drop, matched to Nusa screenshots:
/// Nusa's drag backdrop has three separate lights: a teal spill under the
/// left ear, a nearly colourless slate haze behind the tray, and a violet-blue
/// spill under the right ear.  Keeping them separate matters: averaging them
/// produces the flat blue/purple wash that the reference does not have.
fn aurora(a: &mut App, _focus_x: f32, _focus_y: f32, s: f32) {
    if s <= 0.01 {
        return;
    }
    let panel = Rc::new(crate::WIN_W / 2.0 - a.sw.x / 2.0, 0.0, a.sw.x, a.sh.x);
    let (w, h) = (panel.w, panel.h);
    let bottom = panel.y + h;
    let mid = panel.x + w * 0.53;
    // Broad, desaturated underpainting. Its centre is just inside the lower
    // rim, so it reads as a soft blur rather than a coloured rectangle.
    a.gfx.glow_ellipse(panel.x + w * 0.08, bottom + 9.0, w * 0.34, h * 0.66, 0x07516B, 0.62 * s);
    a.gfx.glow_ellipse(mid, bottom + 7.0, w * 0.46, h * 0.67, 0x727787, 0.27 * s);
    a.gfx.glow_ellipse(panel.x + w * 0.89, bottom - 2.0, w * 0.48, h * 0.82, 0x482878, 0.88 * s);
    // A small indigo bridge prevents a hard seam between the two corner
    // colours, but intentionally never dominates either lobe.
    a.gfx.glow_ellipse(panel.x + w * 0.68, bottom + 3.0, w * 0.30, h * 0.52, 0x2C4394, 0.38 * s);
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
    a.gfx.glow_ellipse(x, y, 52.0, 35.0, 0xECECF1, 0.26 * strength);
    a.gfx.glow_ellipse(x, y + 4.0, 79.0, 50.0, 0xAEB2C1, 0.14 * strength);
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

/// Aurora strength and focus for the shelf.  A drag retains a bright baseline
/// even while the cursor is motionless; movement only adds a little energy.
fn aurora_state(a: &App) -> (f32, f32, f32) {
    if a.drag_over {
        return (a.blob.0.x, a.blob.1.x, 0.78 + 0.22 * a.drag_energy);
    }
    match a.ripple {
        Some((start, x, y)) => {
            let t = start.elapsed().as_secs_f32();
            (x, y, a.drop_energy * (1.0 - (t - 0.5).max(0.0) / 2.5).clamp(0.0, 1.0).powf(1.3))
        }
        None => (0.0, 0.0, 0.0),
    }
}

/// After a drop, a packet of flat concentric bands runs out from the drop point across the
/// panel. Measured on the Nusa screenshot: six ~10 u bands, brightness stepping up to a peak one
/// third in from the leading edge and down again (alphas below, over a lavender-grey).
fn drop_wave(a: &mut App) {
    let Some((start, x, y)) = a.ripple else { return };
    let t = start.elapsed().as_secs_f32();
    if t >= 3.0 {
        a.ripple = None;
        return;
    }
    let panel = Rc::new(crate::WIN_W / 2.0 - a.sw.x / 2.0, 0.0, a.sw.x, a.sh.x);
    let reach = ((x - panel.x).max(panel.x + panel.w - x).powi(2) + (y.max(panel.h - y)).powi(2)).sqrt() + 70.0;
    // A coloured afterglow remains when the wave reaches the shelf edge.
    // Nusa visibly charges the drop location; it does not leave only rings.
    let linger = (1.0 - t / 2.65).clamp(0.0, 1.0).powf(1.35);
    let pos = ((x - panel.x) / panel.w).clamp(0.0, 1.0);
    let tint = if pos < 0.5 {
        mix(0x0B6179, 0x727787, pos * 2.0)
    } else {
        mix(0x727787, 0x5A348B, (pos - 0.5) * 2.0)
    };
    let spread = (1.0 - linger) * 76.0;
    // Coloured afterglow is deliberately broader than the rings and survives
    // after the leading wave has passed the panel edge.
    a.gfx.glow_ellipse(x, y, 132.0 + t * 16.0, 42.0 + t * 7.0, tint, 0.20 * linger);
    a.gfx.glow_ellipse(x - spread * 0.5, y + 2.0, 100.0, 26.0, 0x0B6179, 0.13 * linger);
    a.gfx.glow_ellipse(x + spread * 0.5, y - 2.0, 102.0, 29.0, 0x5A348B, 0.16 * linger);
    a.gfx.glow_ellipse(x, y, 58.0 + t * 7.0, 27.0 + t * 5.0, tint, 0.34 * linger);
    a.gfx.glow_ellipse(x, y, 28.0 + t * 4.0, 15.0 + t * 3.0, 0xE2E3EB, 0.10 * linger);

    let q = (t / 1.6).clamp(0.0, 1.0);
    if q < 1.0 {
        // Inner to outer, subordinate to the coloured afterglow.
        const BANDS: [f32; 6] = [0.03, 0.08, 0.15, 0.10, 0.045, 0.02];
        const BW: f32 = 10.0;
        let front = 20.0 + (reach + BW * 6.0) * (1.0 - (1.0 - q).powf(1.6));
        let fade = 1.0 - q.powf(2.2);
        for (k, al) in BANDS.iter().enumerate() {
            let r = front - (BANDS.len() - 1 - k) as f32 * BW - BW / 2.0;
            if r <= BW / 2.0 {
                continue;
            }
            a.gfx.stroke_circle(x, y, r, BW, 0xC8CCEB, al * fade);
        }
    }
}

// ---------------------------------------------------------------- to-do

fn todo(a: &mut App, lx: f32) {
    title(a, lx, "Todo", Some(("List", TODO_GREEN)));
    let done = a.todo.done_today();
    let total = a.todo.tasks.len();
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
    let order = a.todo.order();
    a.gfx.push_clip(card);
    for (row, &i) in order.iter().take(3).enumerate() {
        let y = card.y + 4.0 + row as f32 * 29.0;
        let rr = Rc::new(card.x + 4.0, y, card.w - 8.0, 27.0);
        let (t, time, is_done) = {
            let task = &a.todo.tasks[i];
            (task.title.clone(), task.time.clone(), task.done_day != 0)
        };
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
            a.gfx.ui(&t, Rc::new(card.x + 29.6, y + 1.0, card.w - 60.0, 15.0), 13.0, 400, tcol, 1.0, Align::L);
            a.gfx.ui(&time, Rc::new(card.x + 29.6, y + 14.6, card.w - 60.0, 11.0), 9.5, 400, GREY, 1.0, Align::L);
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

