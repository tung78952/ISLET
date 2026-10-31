//! Thin-stroke animated icons. Each frame is computed from the clock, never swapped from images,
//! so motion has no steps. `s` is half the icon's box.

use crate::gfx::Gfx;
use std::f32::consts::PI;

const INK: u32 = 0xFFFFFF;

/// Points along an ellipse tipped by `tip`, from one angle to another.
fn oval_arc(c: (f32, f32), r: (f32, f32), tip: f32, from: f32, to: f32) -> Vec<(f32, f32)> {
    (0..=12)
        .map(|i| {
            let a = from + (to - from) * i as f32 / 12.0;
            let (x, y) = (r.0 * a.cos(), r.1 * a.sin());
            (c.0 + x * tip.cos() - y * tip.sin(), c.1 + x * tip.sin() + y * tip.cos())
        })
        .collect()
}

/// A music note from the design sheet: `pair` picks the beamed one.
pub fn note(g: &Gfx, cx: f32, cy: f32, s: f32, pair: bool, rim: u32, alpha: f32) {
    const TIP: f32 = -0.25;
    let mut pts: Vec<(f32, f32)> = if pair {
        let mut p = vec![(-0.31, -0.67), (0.97, -0.95)];
        p.extend(oval_arc((0.56, 0.59), (0.41, 0.33), TIP, 0.1, 5.1));
        p.extend([(0.85, -0.53), (-0.15, -0.31)]);
        p.extend(oval_arc((-0.54, 0.61), (0.41, 0.33), TIP, 0.1, 5.1));
        p
    } else {
        let mut p = vec![(-0.03, -0.97), (0.1, -0.95), (0.31, -0.71), (0.62, -0.5), (0.77, -0.25), (0.72, 0.03), (0.54, 0.23), (0.56, -0.03), (0.46, -0.21), (0.31, -0.33), (0.13, -0.39)];
        p.extend(oval_arc((-0.31, 0.61), (0.42, 0.34), TIP, 0.1, 5.1));
        p
    };
    for p in pts.iter_mut() {
        *p = (cx + p.0 * s, cy + p.1 * s);
    }
    g.fill_poly(&pts, INK, alpha);
    pts.push(pts[0]);
    g.polyline(&pts, 0.9, rim, alpha);
}

/// A nearly flat battery from the design sheet. `dent` pushes its left end in, as when it is hit.
pub fn battery(g: &Gfx, cx: f32, cy: f32, w: f32, h: f32, dent: f32) {
    const RED: u32 = 0xFF0A14;
    const STROKE: f32 = 1.8;
    let (l, r, t, b, rad) = (cx - w / 2.0, cx + w / 2.0, cy - h / 2.0, cy + h / 2.0, h * 0.24);
    let corner = |c: (f32, f32), from: f32| -> Vec<(f32, f32)> { (0..=5).map(|i| { let a = from + i as f32 / 5.0 * PI / 2.0; (c.0 + rad * a.cos(), c.1 + rad * a.sin()) }).collect() };
    let mut body = corner((l + rad, t + rad), PI);
    body.extend(corner((r - rad, t + rad), 1.5 * PI));
    body.extend(corner((r - rad, b - rad), 0.0));
    body.extend(corner((l + rad, b - rad), 0.5 * PI));
    // The left side bows inwards.
    body.extend((1..=8).map(|i| {
        let k = i as f32 / 9.0;
        (l + dent * h * 0.3 * (PI * k).sin(), b - rad - (h - 2.0 * rad) * k)
    }));
    body.push(body[0]);
    g.polyline(&body, STROKE, INK, 1.0);
    let tab = h * 0.46;
    g.polyline(&[(r + 0.5, cy - tab / 2.0), (r + h * 0.14, cy - tab / 2.0), (r + h * 0.14, cy + tab / 2.0), (r + 0.5, cy + tab / 2.0)], STROKE, INK, 1.0);
    let bar = crate::gfx::Rc::new(l + h * 0.16 + dent * h * 0.22, t + h * 0.17, w * 0.14, h * 0.66);
    g.fill_rrect(bar, h * 0.08, RED, 1.0);
}
