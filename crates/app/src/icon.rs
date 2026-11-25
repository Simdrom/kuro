//! The app icon, drawn from scratch: a mango with a play mark, in five schemes.

use eframe::egui::IconData;
use mango_core::settings::AppIcon;

type Rgb = [f32; 3];

struct Scheme {
    top: Rgb,
    bottom: Rgb,
    fruit_a: Rgb,
    fruit_b: Rgb,
    leaf: Rgb,
}

fn rgb(v: u32) -> Rgb {
    [(v >> 16) as f32, (v >> 8 & 255) as f32, (v & 255) as f32].map(|c| c / 255.0)
}

fn scheme(i: AppIcon) -> Scheme {
    let s = |top, bottom, a, b, leaf| Scheme {
        top: rgb(top),
        bottom: rgb(bottom),
        fruit_a: rgb(a),
        fruit_b: rgb(b),
        leaf: rgb(leaf),
    };
    match i {
        AppIcon::Sunrise => s(0x2B2350, 0x6B2D5C, 0xFFC93C, 0xFF6B35, 0x58C27D),
        AppIcon::Night => s(0x0B1026, 0x1C2754, 0x9DB4FF, 0x5B5FEF, 0x3DD6C6),
        AppIcon::Leaf => s(0xE9F7D8, 0xB9E3A0, 0xFFD34E, 0xF2841F, 0x2E8B4F),
        AppIcon::Candy => s(0xFFE3F0, 0xFFC1DD, 0xFF8AB8, 0xE0408A, 0x62C8B0),
        AppIcon::Mono => s(0x2A2A2E, 0x131316, 0xF2F2F4, 0xB6B6BE, 0x8A8A92),
    }
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t)
}

/// Coverage of a shape whose signed distance is `d` (in unit space).
fn cover(d: f32, px: f32) -> f32 {
    (0.5 - d / px).clamp(0.0, 1.0)
}

fn rounded_box(x: f32, y: f32, half: f32, r: f32) -> f32 {
    let (qx, qy) = (x.abs() - half + r, y.abs() - half + r);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r
}

fn ellipse(x: f32, y: f32, a: f32, b: f32) -> f32 {
    let k = (x / a).hypot(y / b);
    (k - 1.0) * a.min(b)
}

fn rotate(x: f32, y: f32, deg: f32) -> (f32, f32) {
    let (s, c) = deg.to_radians().sin_cos();
    (x * c + y * s, y * c - x * s)
}

pub fn render(icon: AppIcon, size: u32) -> IconData {
    let s = scheme(icon);
    let px = 2.0 / size as f32;
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for iy in 0..size {
        for ix in 0..size {
            let x = (ix as f32 + 0.5) * px - 1.0;
            let y = (iy as f32 + 0.5) * px - 1.0;
            let mut c = mix(s.top, s.bottom, (y + 1.0) / 2.0);
            let alpha = cover(rounded_box(x, y, 0.97, 0.44), px);

            // Leaf: the overlap of two circles.
            let (lx, ly) = rotate(x - 0.30, y + 0.60, -28.0);
            let leaf = (lx.hypot(ly - 0.22) - 0.34).max(lx.hypot(ly + 0.22) - 0.34);
            c = mix(c, s.leaf, cover(leaf, px));

            // Fruit: a tilted ellipse with a slimmer tip.
            let (fx, fy) = rotate(x + 0.02, y - 0.12, 32.0);
            let fruit = ellipse(fx, fy, 0.66, 0.52 + fx * 0.12);
            c = mix(c, mix(s.fruit_a, s.fruit_b, (x * 0.35 + y + 0.5) / 1.4), cover(fruit, px));
            let (hx, hy) = rotate(x + 0.26, y + 0.12, 40.0);
            c = mix(c, [1.0; 3], 0.28 * cover(ellipse(hx, hy, 0.20, 0.08), px * 6.0));

            // Play mark.
            let (tx, ty) = (x - 0.0, y - 0.15);
            let tri = (-tx - 0.15).max(0.866 * (ty.abs()) + 0.5 * tx - 0.125) - 0.035;
            c = mix(c, [1.0; 3], 0.95 * cover(tri, px));

            rgba.extend(c.map(|v| (v * 255.0).round() as u8));
            rgba.push((alpha * 255.0).round() as u8);
        }
    }
    IconData { rgba, width: size, height: size }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_differ_and_have_transparent_corners() {
        let all: Vec<_> = AppIcon::ALL.iter().map(|i| render(*i, 64)).collect();
        for (i, a) in all.iter().enumerate() {
            assert_eq!(a.rgba.len(), 64 * 64 * 4);
            assert_eq!(a.rgba[3], 0);
            assert_eq!(a.rgba[(32 * 64 + 32) * 4 + 3], 255);
            assert!(all.iter().skip(i + 1).all(|b| a.rgba != b.rgba));
        }
    }
}
