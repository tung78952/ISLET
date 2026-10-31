//! Thin-stroke animated icons. Each frame is computed from the clock, never swapped from images,
//! so motion has no steps. `s` is half the icon's box.

use crate::gfx::Gfx;
use std::f32::consts::{PI, TAU};

const INK: u32 = 0xFFFFFF;

fn cubic(a: (f32, f32), b: (f32, f32), c: (f32, f32), d: (f32, f32), t: f32) -> (f32, f32) {
    let u = 1.0 - t;
    let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
    (w[0] * a.0 + w[1] * b.0 + w[2] * c.0 + w[3] * d.0, w[0] * a.1 + w[1] * b.1 + w[2] * c.1 + w[3] * d.1)
}

/// A glass from the design sheet: the water tips from side to side and its level slowly rises and
/// falls. Lengths are shares of `s`.
pub fn water(g: &Gfx, cx: f32, cy: f32, s: f32, t: f32) {
    const STROKE: f32 = 0.09;
    const LIGHT: u32 = 0x74CEFF;
    const BLUE: u32 = 0x1AA6FF;
    let at = |x: f32, y: f32| (cx + x * s, cy + y * s);
    // Half the glass's inner width at a height.
    let half = |y: f32| 0.7 + (0.56 - 0.7) * (y + 0.76) / 1.5;
    let floor: Vec<(f32, f32)> = (0..=12)
        .map(|i| {
            let u = i as f32 / 6.0 - 1.0;
            (u * half(0.68), 0.68 + 0.1 * (1.0 - u * u))
        })
        .collect();
    let inside: Vec<(f32, f32)> = std::iter::once((-half(-0.76), -0.76)).chain(floor.iter().copied()).chain(std::iter::once((half(-0.76), -0.76))).map(|p| at(p.0, p.1)).collect();
    let level = -0.18 + 0.14 * (t * TAU / 7.0).sin();
    let tip = 0.07 * (t * 1.9).sin();
    let surface = |below: f32| -> Vec<(f32, f32)> {
        let mut pts: Vec<(f32, f32)> = (0..=18)
            .map(|i| {
                let u = i as f32 / 9.0 - 1.0;
                at(u, level + below + tip * u + 0.045 * (u * 3.4 - t * 2.6).sin())
            })
            .collect();
        pts.push(at(1.0, 1.1));
        pts.push(at(-1.0, 1.1));
        pts
    };
    if g.push_outline_clip(&inside) {
        g.fill_poly(&surface(0.0), LIGHT, 1.0);
        g.fill_poly(&surface(0.11), BLUE, 1.0);
        let gloss = level + 0.28;
        g.line(cx - 0.47 * s, cy + gloss * s, cx - 0.41 * s, cy + 0.42 * s, 0.11 * s, 0xFFFFFF, 0.62);
        g.pop_layer();
    }
    let rim: Vec<(f32, f32)> = (0..=36)
        .map(|i| {
            let a = TAU * i as f32 / 36.0;
            at(0.76 * a.cos(), -0.81 + 0.14 * a.sin())
        })
        .collect();
    let width = STROKE * s;
    g.polyline(&rim, width, INK, 1.0);
    let mut shell = vec![at(-0.76, -0.79), at(-0.61, 0.8)];
    shell.extend((1..=10).map(|i| {
        let p = cubic((-0.61, 0.8), (-0.58, 0.94), (-0.3, 0.96), (0.0, 0.96), i as f32 / 10.0);
        at(p.0, p.1)
    }));
    let back: Vec<(f32, f32)> = shell.iter().rev().skip(1).map(|p| (2.0 * cx - p.0, p.1)).collect();
    shell.extend(back);
    g.polyline(&shell, width, INK, 1.0);
    let lip: Vec<(f32, f32)> = floor.iter().map(|p| at(p.0, p.1 + 0.07)).collect();
    g.polyline(&lip, width * 0.8, INK, 1.0);
}

/// A bell from the design sheet: it rocks and settles, the clapper trails behind, and the sound
/// waves spread from the inner ring to the outer one.
pub fn bell(g: &Gfx, cx: f32, cy: f32, s: f32, t: f32, accent: u32) {
    const CYCLE: f32 = 2.6;
    let swing = |t: f32| {
        let q = (t.rem_euclid(CYCLE) / 1.7).clamp(0.0, 1.0);
        0.36 * (PI * q).sin().max(0.0).powf(0.7) * (1.0 - q) * (q * TAU * 3.5).sin()
    };
    let pivot = (cx, cy - 0.85 * s);
    let at = |p: (f32, f32)| (cx + p.0 * s, cy + p.1 * s);
    let parts: [[(f32, f32); 4]; 3] = [
        [(0.157, -0.77), (0.4, -0.72), (0.6, -0.5), (0.6, -0.22)],
        [(0.6, -0.22), (0.6, 0.1), (0.62, 0.3), (0.84, 0.5)],
        [(0.84, 0.5), (0.91, 0.56), (0.9, 0.68), (0.78, 0.68)],
    ];
    let right: Vec<(f32, f32)> = parts.iter().flat_map(|c| (0..=10).map(move |i| cubic(c[0], c[1], c[2], c[3], i as f32 / 10.0))).collect();
    let body: Vec<(f32, f32)> = right.iter().map(|p| at(*p)).chain(right.iter().rev().map(|p| at((-p.0, p.1)))).collect();
    let clapper: Vec<(f32, f32)> = (0..=14)
        .map(|i| {
            let a = PI * i as f32 / 14.0;
            at((0.25 * a.cos(), 0.75 + 0.245 * a.sin()))
        })
        .collect();
    let q = t.rem_euclid(CYCLE) / CYCLE;
    for (r, delay) in [(0.82f32, 0.0f32), (1.07, 0.09)] {
        let k = ((q - delay) / 0.62).clamp(0.0, 1.0);
        let wave = (PI * k).sin().max(0.0);
        if wave > 0.01 {
            for dir in [-1.0f32, 1.0] {
                let arc: Vec<(f32, f32)> = (0..=10)
                    .map(|i| {
                        let a = (i as f32 / 10.0 - 0.5) * 0.92;
                        at((dir * (r + 0.05 * k) * a.cos(), -0.35 + (r + 0.05 * k) * a.sin()))
                    })
                    .collect();
                g.polyline(&arc, 0.12 * s, accent, wave);
            }
        }
    }
    g.rotate(pivot.0, pivot.1, swing(t - 0.07) * 1.5);
    g.fill_poly(&clapper, INK, 1.0);
    g.unzoom();
    g.rotate(pivot.0, pivot.1, swing(t));
    g.fill_poly(&body, INK, 1.0);
    g.fill_circle(cx, cy - 0.853 * s, 0.152 * s, 0.152 * s, INK, 1.0);
    g.fill_rrect(crate::gfx::Rc::new(cx - 0.152 * s, cy - 0.86 * s, 0.304 * s, 0.14 * s), 0.0, INK, 1.0);
    g.unzoom();
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

/// Eases 0..1 in and out.
fn ease(k: f32) -> f32 {
    let k = k.clamp(0.0, 1.0);
    k * k * (3.0 - 2.0 * k)
}

fn star(g: &Gfx, x: f32, y: f32, r: f32, hex: u32, a: f32) {
    let n = r * 0.22;
    g.fill_poly(&[(x, y - r), (x + n, y - n), (x + r, y), (x + n, y + n), (x, y + r), (x - n, y + n), (x - r, y), (x - n, y - n)], hex, a);
}

/// A hand waving from the wrist, with sound-like arcs leaving it.
pub fn hello(g: &Gfx, cx: f32, cy: f32, s: f32, t: f32) {
    const AMBER: u32 = 0xFDB825;
    let at = |x: f32, y: f32| (cx + x * s, cy + y * s);
    let swing = 0.3 * (t * 7.0).sin() * (0.5 + 0.5 * (t * TAU / 2.6).cos()).powf(0.5);
    let reach = swing.abs() / 0.3;
    for (dir, r) in [(-1.0f32, 0.86f32), (1.0, 0.86), (1.0, 1.06)] {
        let arc: Vec<(f32, f32)> = (0..=8).map(|i| { let a = (i as f32 / 8.0 - 0.5) * 0.9; at(dir * r * a.cos(), 0.1 + r * a.sin()) }).collect();
        g.polyline(&arc, 0.13 * s, AMBER, reach);
    }
    let wrist = at(0.0, 0.72);
    g.rotate(wrist.0, wrist.1, swing);
    let (px, py) = at(0.03, 0.25);
    g.fill_circle(px, py, 0.52 * s, 0.48 * s, INK, 1.0);
    for (b, e) in [((-0.32, 0.2), (-0.66, -0.12)), ((-0.14, 0.1), (-0.5, -0.4)), ((0.04, 0.05), (-0.27, -0.58)), ((0.2, 0.08), (-0.02, -0.62)), ((0.42, 0.2), (0.36, -0.44))] {
        let (a, c) = (at(b.0, b.1), at(e.0, e.1));
        g.line(a.0, a.1, c.0, c.1, 0.22 * s, INK, 1.0);
    }
    g.unzoom();
}

/// A crescent rocking gently under two twinkling stars.
pub fn night(g: &Gfx, cx: f32, cy: f32, s: f32, t: f32) {
    const GOLD: u32 = 0xFFD23C;
    let (mx, my) = (cx - 0.09 * s, cy + 0.01 * s);
    g.rotate(mx, my, 0.1 * (t * 1.3).sin());
    g.fill_circle(mx, my, 0.76 * s, 0.76 * s, INK, 1.0);
    g.fill_circle(cx + 0.38 * s, cy - 0.18 * s, 0.6 * s, 0.6 * s, 0x000000, 1.0);
    g.unzoom();
    let glow = |phase: f32| 0.72 + 0.28 * (t * 2.2 + phase).sin();
    star(g, cx + 0.44 * s, cy - 0.47 * s, 0.32 * s * glow(0.0), GOLD, 1.0);
    star(g, cx + 0.73 * s, cy - 0.05 * s, 0.17 * s * glow(2.0), GOLD, 1.0);
}

/// A fist pumping up, with sparks flying off at the top of each pump.
pub fn cheer(g: &Gfx, cx: f32, cy: f32, s: f32, t: f32) {
    const AMBER: u32 = 0xFFC02E;
    let p = (t % 1.2) / 1.2;
    let up = (p * PI).sin().powi(2);
    let cy = cy + (0.1 - 0.22 * up) * s;
    let at = |x: f32, y: f32| (cx + x * s, cy + y * s);
    let spark = ((p - 0.3) / 0.45).clamp(0.0, 1.0);
    if spark > 0.0 && spark < 1.0 {
        for a in [-0.55f32, 0.0, 0.55] {
            let (r0, r1) = (0.86 + 0.14 * spark, 1.1 + 0.2 * spark);
            let (dx, dy) = (a.sin(), -a.cos());
            let (b, e) = (at(dx * r0, dy * r0 - 0.1), at(dx * r1, dy * r1 - 0.1));
            g.line(b.0, b.1, e.0, e.1, 0.12 * s, AMBER, (PI * spark).sin());
        }
    }
    let wrist = at(-0.27, 0.46);
    g.fill_rrect(crate::gfx::Rc::new(wrist.0, wrist.1, 0.6 * s, 0.4 * s), 0.1 * s, INK, 1.0);
    let body = at(-0.56, -0.24);
    g.fill_rrect(crate::gfx::Rc::new(body.0, body.1, 1.12 * s, 0.86 * s), 0.34 * s, INK, 1.0);
    for (b, e) in [((-0.32, -0.1), (-0.22, -0.46)), ((-0.08, -0.1), (0.02, -0.52)), ((0.16, -0.08), (0.28, -0.46)), ((0.38, 0.0), (0.52, -0.3))] {
        let (a, c) = (at(b.0, b.1), at(e.0, e.1));
        g.line(a.0, a.1, c.0, c.1, 0.27 * s, INK, 1.0);
    }
    let crease: Vec<(f32, f32)> = [(-0.42, 0.12), (-0.1, 0.1), (0.1, 0.04), (0.2, 0.26)].iter().map(|p| at(p.0, p.1)).collect();
    g.polyline(&crease, 0.05 * s, 0x000000, 1.0);
}

/// A person walking: legs and arms swing in opposition and the knees bend on the way through.
pub fn walk(g: &Gfx, cx: f32, cy: f32, s: f32, t: f32) {
    let phase = t * 4.6;
    let bob = 0.03 * (phase * 2.0).cos();
    let cy = cy + bob * s;
    let at = |x: f32, y: f32| (cx + x * s, cy + y * s);
    let (neck, hip) = (at(0.0, -0.42), at(-0.06, 0.12));
    let limb = |root: (f32, f32), a: f32, bend: f32, len: f32, width: f32| {
        let knee = (root.0 + a.sin() * len * s, root.1 + a.cos() * len * s);
        let b = a + bend;
        let foot = (knee.0 + b.sin() * len * s, knee.1 + b.cos() * len * s);
        g.polyline(&[root, knee, foot], width * s, INK, 1.0);
    };
    let swing = phase.sin();
    limb(hip, 0.5 * swing, -(0.5 * (phase - 0.7).cos().max(0.0)), 0.39, 0.25);
    limb(hip, -0.5 * swing, -(0.5 * (-(phase - 0.7).cos()).max(0.0)), 0.39, 0.25);
    limb(neck, -0.7 * swing, 0.4, 0.32, 0.2);
    limb(neck, 0.7 * swing, 0.4, 0.32, 0.2);
    g.line(neck.0, neck.1, hip.0, hip.1, 0.36 * s, INK, 1.0);
    let head = at(0.03, -0.76);
    g.fill_circle(head.0, head.1, 0.23 * s, 0.23 * s, INK, 1.0);
}

/// An open book with a page turning over.
pub fn study(g: &Gfx, cx: f32, cy: f32, s: f32, t: f32) {
    const PAGE: u32 = 0x9B8CE6;
    let at = |x: f32, y: f32| (cx + x * s, cy + y * s);
    let leaf = |side: f32, w: f32, hex: u32, a: f32| {
        let edge: Vec<(f32, f32)> = (0..=8).map(|i| { let u = i as f32 / 8.0; at(side * (0.04 + w * u), -0.56 + 0.1 * (1.0 - (2.0 * u - 1.0).powi(2))) }).collect();
        let foot: Vec<(f32, f32)> = (0..=8).rev().map(|i| { let u = i as f32 / 8.0; at(side * (0.04 + w * u), 0.5 + 0.08 * (2.0 * u - 1.0).powi(2) + 0.1 * (1.0 - u)) }).collect();
        let pts: Vec<(f32, f32)> = edge.into_iter().chain(foot).collect();
        g.fill_poly(&pts, hex, a);
    };
    leaf(-1.0, 0.64, PAGE, 1.0);
    leaf(1.0, 0.64, PAGE, 1.0);
    // The turning page's width follows the cosine of its angle, crossing the spine edge-on.
    let p = ease(((t % 3.2) / 3.2 - 0.25) / 0.5);
    if p > 0.0 && p < 1.0 {
        let c = (p * PI).cos();
        leaf(c.signum(), 0.64 * c.abs(), 0xFFFFFF, 0.92);
    }
    let shell = [(-0.9, -0.5), (-0.9, 0.62), (-0.08, 0.66), (0.0, 0.78), (0.08, 0.66), (0.9, 0.62), (0.9, -0.5)].map(|p| at(p.0, p.1));
    g.polyline(&shell, 0.1 * s, INK, 1.0);
    for side in [-1.0f32, 1.0] {
        let top: Vec<(f32, f32)> = (0..=8).map(|i| { let u = i as f32 / 8.0; at(side * (0.02 + 0.72 * u), -0.62 + 0.1 * (1.0 - (2.0 * u - 1.0).powi(2)) - 0.03) }).collect();
        g.polyline(&top, 0.09 * s, INK, 1.0);
    }
    let (a, b) = (at(0.0, -0.56), at(0.0, 0.7));
    g.line(a.0, a.1, b.0, b.1, 0.1 * s, INK, 1.0);
}

/// A person raising both arms into a stretch and letting them down.
pub fn stretch(g: &Gfx, cx: f32, cy: f32, s: f32, t: f32) {
    const AMBER: u32 = 0xFFC02E;
    let k = ease(0.5 - 0.65 * (t * TAU / 3.0).cos());
    let at = |x: f32, y: f32| (cx + x * s, cy + y * s);
    let rise = 0.06 * k;
    let (neck, hip) = (at(0.0, -0.22 - rise), at(0.0, 0.22));
    for side in [-1.0f32, 1.0] {
        let hand = (0.62 + 0.1 * (k * PI).sin(), 0.28 - 1.08 * k);
        let (sh, el, ha) = (at(side * 0.16, -0.22 - rise), at(side * (0.42 + 0.06 * (1.0 - k)), 0.08 - 0.52 * k), at(side * hand.0, hand.1));
        g.polyline(&[sh, el, ha], 0.22 * s, INK, 1.0);
        let foot = at(side * 0.3, 0.92);
        g.polyline(&[(hip.0 + side * 0.1 * s, hip.1), foot], 0.26 * s, INK, 1.0);
        for dy in [-0.14f32, 0.14] {
            let (a, b) = (at(side * (0.72 + 0.08 * k), 0.1 + dy * 1.4 - 0.1), at(side * (1.02 + 0.08 * k), 0.1 + dy * 2.4 - 0.1));
            g.line(a.0, a.1, b.0, b.1, 0.13 * s, AMBER, k);
        }
    }
    g.line(neck.0, neck.1, hip.0, hip.1, 0.5 * s, INK, 1.0);
    let head = at(0.0, -0.6 - rise);
    g.fill_circle(head.0, head.1, 0.26 * s, 0.26 * s, INK, 1.0);
}

/// An eye that looks far away, side to side, and blinks.
pub fn look(g: &Gfx, cx: f32, cy: f32, s: f32, t: f32) {
    const BLUE: u32 = 0x4A9CF2;
    let cycle = t % 4.0;
    let side = 0.28 * (ease((cycle - 1.1) / 0.2) - 2.0 * ease((cycle - 2.1) / 0.2) + ease((cycle - 3.1) / 0.2));
    let blink = (t % 2.9 - 2.75).abs();
    let open = 1.0 - ease(1.0 - blink / 0.13);
    let (top, low) = (0.92 * open.max(0.04), 0.82 * open.max(0.04));
    let y0 = cy + 0.14 * s;
    let curve = |sign: f32, h: f32, k: f32| cubic((-0.94, 0.0), (-0.5, sign * h), (0.5, sign * h), (0.94, 0.0), k);
    let mut lid: Vec<(f32, f32)> = (0..=16).map(|i| curve(-1.0, top, i as f32 / 16.0)).collect();
    lid.extend((1..16).rev().map(|i| curve(1.0, low, i as f32 / 16.0)));
    let lid: Vec<(f32, f32)> = lid.iter().map(|p| (cx + p.0 * s, y0 + p.1 * s)).collect();
    g.fill_poly(&lid, INK, 1.0);
    if g.push_outline_clip(&lid) {
        let ix = cx + side * s;
        g.fill_circle(ix, y0, 0.44 * s, 0.44 * s, BLUE, 1.0);
        g.fill_circle(ix, y0, 0.24 * s, 0.24 * s, 0x07152E, 1.0);
        g.fill_circle(ix + 0.13 * s, y0 - 0.14 * s, 0.1 * s, 0.1 * s, INK, 1.0);
        g.pop_layer();
    }
    for a in [-0.5f32, 0.0, 0.5] {
        let (dx, dy) = (a.sin(), -a.cos());
        g.line(cx + dx * 0.62 * s, y0 + dy * 0.72 * s, cx + dx * 0.84 * s, y0 + dy * 0.94 * s, 0.13 * s, BLUE, open);
    }
}

/// Wind curling past with a leaf drifting on it, the lines rising and falling like a breath.
pub fn breathe(g: &Gfx, cx: f32, cy: f32, s: f32, t: f32) {
    const TEAL: u32 = 0x62DCC8;
    const LEAF: u32 = 0x7BE8A8;
    let at = |x: f32, y: f32| (cx + x * s, cy + y * s);
    let breath = (t * TAU / 4.0).sin();
    // A stroke that waves along and ends in a curl.
    let curl = |y: f32, reach: f32, turn: f32, r: f32, phase: f32, hex: u32, width: f32| {
        let mut pts: Vec<(f32, f32)> = (0..=16)
            .map(|i| {
                let k = i as f32 / 16.0;
                (-0.92 + (reach + 0.92) * k, y + 0.07 * (k * 6.0 + t * 1.8 + phase).sin())
            })
            .collect();
        let end = *pts.last().unwrap();
        let (ox, oy) = (end.0, end.1 + turn * r);
        pts.extend((1..=14).map(|i| {
            let k = i as f32 / 14.0;
            let a = -turn * PI / 2.0 + turn * k * 4.4;
            let radius = r * (1.0 - 0.72 * k);
            (ox + radius * a.cos(), oy + radius * a.sin())
        }));
        let pts: Vec<(f32, f32)> = pts.iter().map(|p| at(p.0, p.1)).collect();
        g.polyline(&pts, width * s, hex, 1.0);
    };
    curl(-0.22 - 0.06 * breath, 0.42, -1.0, 0.3, 0.0, TEAL, 0.13);
    curl(0.14 + 0.06 * breath, 0.1, 1.0, 0.28, 1.6, INK, 0.12);
    let drift = (t * 0.5) % 1.0;
    let leaf = at(-0.5 + 0.8 * drift, -0.66 + 0.1 * (t * 2.0).sin());
    g.rotate(leaf.0, leaf.1, 0.5 + 0.5 * (t * 2.0).sin());
    g.fill_circle(leaf.0, leaf.1, 0.26 * s, 0.13 * s, LEAF, (PI * drift).sin());
    g.unzoom();
}
