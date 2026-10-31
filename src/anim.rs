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
