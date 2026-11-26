//! The app icon: 黒 (kuro) in ink on paper, in five schemes.

use eframe::egui::IconData;
use kuro_core::settings::AppIcon;

type Rgb = [f32; 3];

struct Scheme {
    top: Rgb,
    bottom: Rgb,
    ink_a: Rgb,
    ink_b: Rgb,
}

fn rgb(v: u32) -> Rgb {
    [(v >> 16) as f32, (v >> 8 & 255) as f32, (v & 255) as f32].map(|c| c / 255.0)
}

fn scheme(i: AppIcon) -> Scheme {
    let s = |top, bottom, a, b| Scheme { top: rgb(top), bottom: rgb(bottom), ink_a: rgb(a), ink_b: rgb(b) };
    match i {
        AppIcon::Ink => s(0xFBF8F1, 0xE6E0D2, 0x2C2C33, 0x08080A),
        AppIcon::Night => s(0x0B1026, 0x1C2754, 0x9DB4FF, 0x5B5FEF),
        AppIcon::Leaf => s(0xE9F7D8, 0xB9E3A0, 0x3FA463, 0x1C6B3C),
        AppIcon::Candy => s(0xFFE3F0, 0xFFC1DD, 0xFF8AB8, 0xD93A84),
        AppIcon::Mono => s(0x2A2A2E, 0x0C0C0E, 0xFFFFFF, 0xC9C9D0),
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

/// Kuro's mark, 黒, drawn by assets/make_icon.py: how much of each pixel it covers.
fn mark() -> &'static (Vec<f32>, usize) {
    static MARK: std::sync::OnceLock<(Vec<f32>, usize)> = std::sync::OnceLock::new();
    MARK.get_or_init(|| {
        let png = include_bytes!("../assets/kuro-mark.png");
        let m = eframe::icon_data::from_png_bytes(png).unwrap_or_default();
        (m.rgba.as_chunks::<4>().0.iter().map(|p| f32::from(p[3]) / 255.0).collect(), m.width as usize)
    })
}

/// Average coverage of the mark over one pixel of an icon `size` wide.
fn mark_at(ix: u32, iy: u32, size: u32) -> f32 {
    let (m, n) = mark();
    if *n == 0 {
        return 0.0;
    }
    let span = |i: u32| {
        let a = (i as usize * n) / size as usize;
        a..((i as usize + 1) * n).div_ceil(size as usize).max(a + 1).min(*n)
    };
    let (xs, ys) = (span(ix), span(iy));
    let count = (xs.len() * ys.len()) as f32;
    ys.flat_map(|y| xs.clone().map(move |x| m[y * n + x])).sum::<f32>() / count
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

            // The character in ink, darker towards the bottom right.
            let ink = mix(s.ink_a, s.ink_b, (x * 0.3 + y + 0.6) / 1.4);
            c = mix(c, ink, mark_at(ix, iy, size));

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
        // The character is there: its strokes cross the middle row.
        assert_eq!(mark().1, 512);
        let row = &all[0].rgba[32 * 64 * 4..33 * 64 * 4];
        assert!(row.as_chunks::<4>().0.iter().any(|p| p[0] < 100), "no ink in the middle");
    }
}
