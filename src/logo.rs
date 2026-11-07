//! Islet's logo, drawn from shapes so it is crisp at every size: the character itself, a black
//! pill with two upright oval eyes made of glowing cyan stripes, on a soft white tile for the app
//! icon, or alone for the tray. Plain Rust with no dependencies: `build.rs` includes this file to
//! write the .ico the exe carries.

/// The sizes the app icon is made in (by build.rs).
#[allow(dead_code)]
pub const ICON_SIZES: [u32; 7] = [16, 20, 24, 32, 40, 48, 256];

const TILE: [f32; 3] = [0xF6 as f32, 0xF7 as f32, 0xFA as f32];
const BODY: [f32; 3] = [0x0B as f32, 0x0B as f32, 0x0D as f32];
const SHEEN: [f32; 3] = [0x34 as f32, 0x34 as f32, 0x3A as f32];
const RIM: [f32; 3] = [0x70 as f32, 0x70 as f32, 0x78 as f32];
const EYE: [f32; 3] = [0x66 as f32, 0xDB as f32, 0xFF as f32];

/// Signed distance from (x, y) to a rounded rectangle centred at (cx, cy).
fn rrect(x: f32, y: f32, cx: f32, cy: f32, w: f32, h: f32, r: f32) -> f32 {
    let qx = (x - cx).abs() - (w / 2.0 - r);
    let qy = (y - cy).abs() - (h / 2.0 - r);
    let out = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    out + qx.max(qy).min(0.0) - r
}

/// Roughly the signed distance to an ellipse with half-axes `a`, `b`, in pixels.
fn oval(x: f32, y: f32, cx: f32, cy: f32, a: f32, b: f32) -> f32 {
    let (u, v) = ((x - cx) / a, (y - cy) / b);
    ((u * u + v * v).sqrt() - 1.0) * a.min(b)
}

fn mix(a: [f32; 3], b: [f32; 3], k: f32) -> [f32; 3] {
    let k = k.clamp(0.0, 1.0);
    [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k, a[2] + (b[2] - a[2]) * k]
}

/// How many stripes each eye has at this eye height (px): fewer as it shrinks, none (a solid
/// oval) once a stripe would be thinner than a pixel and a half.
pub fn stripes(eye_h: f32) -> u32 {
    let n = (eye_h / 3.2).floor() as u32;
    if n < 3 {
        0
    } else {
        n.min(6)
    }
}

/// The logo `size` px square, as straight RGBA rows from the top; `tile` puts it on the white
/// rounded tile (app icon), otherwise the pill alone fills the square with a thin grey rim so it
/// shows on a dark taskbar too (tray).
pub fn logo(size: u32, tile: bool) -> Vec<[u8; 4]> {
    let s = size as f32;
    let c = s / 2.0;
    // The tile and the pill on it, after the approved sheet: the pill is 0.90 of the tile wide
    // and 0.44 tall; each eye is 0.19 of the pill wide and 0.53 of it tall, 0.18 of the pill's
    // width either side of its middle.
    let margin = if tile && size >= 32 { s * 0.04 } else { 0.0 };
    let tw = s - 2.0 * margin;
    let (pw, ph) = if tile { (tw * 0.86, tw * 0.43) } else { (s * 0.98, s * 0.60) };
    let (ew, eh) = if tile { (pw * 0.19, ph * 0.53) } else { (pw * 0.20, ph * 0.58) };
    let ex = pw * if tile { 0.18 } else { 0.19 };
    let n = stripes(eh);
    let rim = (s * 0.045).max(1.0);
    const SS: u32 = 4;
    let mut out = Vec::with_capacity((size * size) as usize);
    for py in 0..size {
        for px in 0..size {
            // Premultiplied sum over SS×SS samples.
            let mut acc = [0.0f32; 4];
            for sy in 0..SS {
                for sx in 0..SS {
                    let x = px as f32 + (sx as f32 + 0.5) / SS as f32;
                    let y = py as f32 + (sy as f32 + 0.5) / SS as f32;
                    let (mut col, mut a) = ([0.0f32; 3], 0.0f32);
                    if tile {
                        let d = rrect(x, y, c, c, tw, tw, tw * 0.23);
                        if size >= 48 {
                            // A faint shadow under the tile.
                            let sd = rrect(x, y - s * 0.012, c, c, tw, tw, tw * 0.23);
                            a = 0.10 * (-sd.max(0.0) / (s * 0.012)).exp() * (sd > 0.0) as u8 as f32;
                        }
                        if d < 0.0 {
                            // A whisper of shade toward the tile's bottom.
                            col = mix(TILE, [0xE8 as f32, 0xEA as f32, 0xF0 as f32], (y - margin) / tw * 0.6);
                            a = 1.0;
                        }
                    }
                    let d = rrect(x, y, c, c, pw, ph, ph / 2.0);
                    if d < 0.0 {
                        // Black glass, lit softly along its top.
                        let top = ((y - (c - ph / 2.0)) / ph).clamp(0.0, 1.0);
                        col = mix(SHEEN, BODY, (top / 0.35).powf(0.7));
                        if !tile && d > -rim {
                            col = mix(col, RIM, 0.7);
                        }
                        a = 1.0;
                        // The eyes and the light they throw on the pill.
                        let mut glow = 0.0f32;
                        for side in [-1.0f32, 1.0] {
                            let (cx, cy) = (c + side * ex, c);
                            let e = oval(x, y, cx, cy, ew / 2.0, eh / 2.0);
                            if e < 0.0 {
                                let lit = if n == 0 {
                                    true
                                } else {
                                    let v = (y - (cy - eh / 2.0)) / eh * n as f32;
                                    let f = v - v.floor();
                                    (0.15..0.85).contains(&f)
                                };
                                if lit {
                                    col = mix(EYE, [255.0, 255.0, 255.0], 0.12 * (1.0 - (y - cy).abs() / (eh / 2.0)));
                                } else {
                                    col = mix(col, EYE, 0.18);
                                }
                            } else {
                                glow = glow.max(0.38 * (-e / (ew * 0.22)).exp());
                            }
                        }
                        col = mix(col, EYE, glow);
                    }
                    acc[0] += col[0] * a;
                    acc[1] += col[1] * a;
                    acc[2] += col[2] * a;
                    acc[3] += a;
                }
            }
            let k = (SS * SS) as f32;
            let a = acc[3] / k;
            let un = |v: f32| if a > 0.0 { (v / k / a).round().clamp(0.0, 255.0) as u8 } else { 0 };
            out.push([un(acc[0]), un(acc[1]), un(acc[2]), (a * 255.0).round() as u8]);
        }
    }
    out
}

/// An .ico file (for build.rs) holding each (size, RGBA rows) image as a 32-bit bitmap.
#[allow(dead_code)]
pub fn ico(images: &[(u32, Vec<[u8; 4]>)]) -> Vec<u8> {
    let mut head = Vec::new();
    let mut body = Vec::new();
    head.extend_from_slice(&[0, 0, 1, 0]);
    head.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (size, px) in images {
        let s = *size;
        let mask_row = s.div_ceil(32) * 4;
        let mut bmp = Vec::new();
        for v in [40u32, s, s * 2] {
            bmp.extend_from_slice(&v.to_le_bytes());
        }
        bmp.extend_from_slice(&1u16.to_le_bytes());
        bmp.extend_from_slice(&32u16.to_le_bytes());
        for v in [0u32, s * s * 4 + mask_row * s, 0, 0, 0, 0] {
            bmp.extend_from_slice(&v.to_le_bytes());
        }
        for y in (0..s).rev() {
            for x in 0..s {
                let [r, g, b, a] = px[(y * s + x) as usize];
                bmp.extend_from_slice(&[b, g, r, a]);
            }
        }
        bmp.resize(bmp.len() + (mask_row * s) as usize, 0);
        let dim = if s >= 256 { 0 } else { s as u8 };
        head.extend_from_slice(&[dim, dim, 0, 0]);
        head.extend_from_slice(&1u16.to_le_bytes());
        head.extend_from_slice(&32u16.to_le_bytes());
        head.extend_from_slice(&(bmp.len() as u32).to_le_bytes());
        head.extend_from_slice(&offset.to_le_bytes());
        offset += bmp.len() as u32;
        body.extend_from_slice(&bmp);
    }
    head.extend_from_slice(&body);
    head
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_eyes_are_solid_and_big_ones_striped() {
        assert_eq!(stripes(4.0), 0);
        assert_eq!(stripes(12.0), 3);
        assert_eq!(stripes(120.0), 6);
    }

    #[test]
    fn the_tile_is_white_at_its_corner_and_the_pill_black_between_the_eyes() {
        let s = 64;
        let px = logo(s, true);
        let at = |x: u32, y: u32| px[(y * s + x) as usize];
        assert!(at(8, 8)[0] > 200 && at(8, 8)[3] == 255);
        let mid = at(32, 32);
        assert!(mid[0] < 60 && mid[3] == 255, "{mid:?}");
        assert_eq!(at(0, 0)[3], 0);
    }

    #[test]
    fn an_ico_lists_every_size() {
        let images: Vec<_> = [16, 256].iter().map(|&s| (s, logo(s, true))).collect();
        let f = ico(&images);
        assert_eq!(u16::from_le_bytes([f[4], f[5]]), 2);
        assert_eq!(f[6], 16);
        assert_eq!(f[22], 0);
    }
}
