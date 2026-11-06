//! Liquid glass refraction, after Kyant0/AndroidLiquidGlass: near the rim of a rounded
//! rectangle the picture under it bends inward along a circular profile, so its edge looks like
//! the thick rim of a lens; the middle is left as it is. This builds the displacement map that
//! Direct2D's DisplacementMap effect reads (red: across, green: down, 0.5 = no move).

/// Signed distance from `p` (relative to the centre) to a rounded rectangle of half size `half`
/// and corner `radius`: negative inside.
fn sd_round_rect(p: (f32, f32), half: (f32, f32), radius: f32) -> f32 {
    let (cx, cy) = (p.0.abs() - (half.0 - radius), p.1.abs() - (half.1 - radius));
    let outside = (cx.max(0.0).powi(2) + cy.max(0.0).powi(2)).sqrt() - radius;
    let inside = cx.max(cy).min(0.0);
    outside + inside
}

/// Which way the rim faces at `p`: outward, unit length.
fn rim_normal(p: (f32, f32), half: (f32, f32), radius: f32) -> (f32, f32) {
    let (cx, cy) = (p.0.abs() - (half.0 - radius), p.1.abs() - (half.1 - radius));
    let (sx, sy) = (p.0.signum(), p.1.signum());
    if cx > 0.0 && cy > 0.0 {
        let l = (cx * cx + cy * cy).sqrt().max(1e-6);
        (sx * cx / l, sy * cy / l)
    } else if cx > cy {
        (sx, 0.0)
    } else {
        (0.0, sy)
    }
}

/// The circular height of the rim: 1 at the very edge, 0 at `depth` inside.
fn circle_map(x: f32) -> f32 {
    1.0 - (1.0 - x * x).max(0.0).sqrt()
}

/// How far (px) the picture is pulled in at `p` inside a `w`×`h` rounded rectangle: the rim is
/// `depth` px wide and bends by up to `amount` px.
pub fn offset_at(p: (f32, f32), w: f32, h: f32, radius: f32, depth: f32, amount: f32) -> (f32, f32) {
    let half = (w / 2.0, h / 2.0);
    let radius = radius.min(half.0).min(half.1);
    let sd = sd_round_rect(p, half, radius);
    if sd > 0.0 || -sd >= depth {
        return (0.0, 0.0);
    }
    let d = circle_map(1.0 - -sd / depth) * amount;
    let n = rim_normal(p, half, radius.max(depth.min(half.0.min(half.1))));
    (-n.0 * d, -n.1 * d)
}

/// The displacement map for a `w`×`h` px picture, as premultiplied BGRA: red carries the shift
/// across, green the shift down, both centred on 128 for `amount` px either way.
pub fn map(w: u32, h: u32, radius: f32, depth: f32, amount: f32) -> Vec<u8> {
    map_in(w, h, (0.0, 0.0, w as f32, h as f32), radius, depth, amount)
}

/// The same for a glass shape `shape` (x, y, w, h in px) inside a `w`×`h` px picture: outside the
/// shape nothing moves. The shape may reach past the picture (the notch's top is off screen).
pub fn map_in(w: u32, h: u32, shape: (f32, f32, f32, f32), radius: f32, depth: f32, amount: f32) -> Vec<u8> {
    let (sx, sy, sw, sh) = shape;
    let mut px = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let p = (x as f32 + 0.5 - (sx + sw / 2.0), y as f32 + 0.5 - (sy + sh / 2.0));
            let (ox, oy) = offset_at(p, sw, sh, radius, depth, amount);
            let enc = |v: f32| (128.0 + 127.0 * (v / amount.max(1e-6)).clamp(-1.0, 1.0)).round() as u8;
            let i = ((y * w + x) * 4) as usize;
            px[i] = 128; // blue: unused
            px[i + 1] = enc(oy);
            px[i + 2] = enc(ox);
            px[i + 3] = 255;
        }
    }
    px
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_middle_stays_and_the_rim_bends_inward_most_at_the_edge() {
        let (w, h, r, depth, amount) = (100.0, 100.0, 12.0, 14.0, 8.0);
        assert_eq!(offset_at((0.0, 0.0), w, h, r, depth, amount), (0.0, 0.0), "the middle is untouched");
        assert_eq!(offset_at((0.0, 30.0), w, h, r, depth, amount), (0.0, 0.0), "deeper than the rim");
        let near = offset_at((0.0, 49.5), w, h, r, depth, amount);
        let mid = offset_at((0.0, 42.0), w, h, r, depth, amount);
        assert!(near.1 < 0.0, "at the bottom edge it pulls upward, inward: {near:?}");
        assert!(near.1.abs() > mid.1.abs(), "strongest at the very edge");
        assert!(near.1.abs() <= amount + 1e-3, "never more than the amount");
        let left = offset_at((-49.5, 0.0), w, h, r, depth, amount);
        assert!(left.0 > 0.0 && left.1.abs() < 1e-4, "at the left edge it pulls rightward");
    }

    #[test]
    fn the_map_is_neutral_in_the_middle() {
        let px = map(40, 40, 6.0, 8.0, 5.0);
        let i = ((20 * 40 + 20) * 4) as usize;
        assert_eq!((px[i + 2], px[i + 1], px[i + 3]), (128, 128, 255));
    }
}
