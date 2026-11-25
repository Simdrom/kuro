//! The app icon, drawn from scratch: a drop of ink with a play mark, in five schemes.

use eframe::egui::IconData;
use kuro_core::settings::AppIcon;

type Rgb = [f32; 3];

struct Scheme {
    top: Rgb,
    bottom: Rgb,
    ink_a: Rgb,
    ink_b: Rgb,
    mark: Rgb,
}

fn rgb(v: u32) -> Rgb {
    [(v >> 16) as f32, (v >> 8 & 255) as f32, (v & 255) as f32].map(|c| c / 255.0)
}

fn scheme(i: AppIcon) -> Scheme {
    let s = |top, bottom, a, b, mark| Scheme {
        top: rgb(top),
        bottom: rgb(bottom),
        ink_a: rgb(a),
        ink_b: rgb(b),
        mark: rgb(mark),
    };
    match i {
        AppIcon::Ink => s(0xFBF8F1, 0xE6E0D2, 0x2C2C33, 0x08080A, 0xFBF8F1),
        AppIcon::Night => s(0x0B1026, 0x1C2754, 0x9DB4FF, 0x5B5FEF, 0xFFFFFF),
        AppIcon::Leaf => s(0xE9F7D8, 0xB9E3A0, 0x3FA463, 0x1C6B3C, 0xFFFFFF),
        AppIcon::Candy => s(0xFFE3F0, 0xFFC1DD, 0xFF8AB8, 0xD93A84, 0xFFFFFF),
        AppIcon::Mono => s(0x2A2A2E, 0x0C0C0E, 0xFFFFFF, 0xC9C9D0, 0x131316),
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

/// A drop: a circle of radius `r` that narrows to a tip `h` above its centre.
fn drop(x: f32, y: f32, r: f32, h: f32) -> f32 {
    const TIP: f32 = 0.015;
    let x = x.abs();
    let b = (r - TIP) / h;
    let a = (1.0 - b * b).sqrt();
    let k = -b * x + a * y;
    if k < 0.0 {
        x.hypot(y) - r
    } else if k > a * h {
        x.hypot(y - h) - TIP
    } else {
        a * x + b * y - r
    }
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

            // The drop leans a little, as if it had just left the brush.
            let (dx, dy) = rotate(x, y - 0.20, 14.0);
            let ink = mix(s.ink_a, s.ink_b, (x * 0.3 + y + 0.6) / 1.4);
            c = mix(c, ink, cover(drop(dx, -dy, 0.47, 0.86), px));
            // Two splashes.
            c = mix(c, s.ink_b, cover((x + 0.64).hypot(y - 0.56) - 0.085, px));
            c = mix(c, s.ink_b, cover((x - 0.62).hypot(y + 0.04) - 0.055, px));
            // Light on the edge of the drop.
            let shine = ((dx + 0.02).hypot(dy - 0.02) - 0.36).abs() - 0.022;
            let arc = if dx < -0.12 && dy > -0.16 && dy < 0.26 { 0.35 } else { 0.0 };
            c = mix(c, s.mark, arc * cover(shine, px * 2.0));

            // Play mark.
            let (tx, ty) = (x - 0.01, y - 0.22);
            let tri = (-tx - 0.13).max(0.866 * ty.abs() + 0.5 * tx - 0.11) - 0.03;
            c = mix(c, s.mark, cover(tri, px));

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
