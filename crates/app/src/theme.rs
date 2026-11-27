//! Colours, fonts and egui style.

use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow, Stroke, Vec2,
};
use kuro_core::ListStatus;
use kuro_core::settings::{Theme, Tint};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub dark: bool,
    pub bg: Color32,
    pub sidebar: Color32,
    pub card: Color32,
    pub raised: Color32,
    pub hover: Color32,
    pub line: Color32,
    pub text: Color32,
    pub dim: Color32,
    pub faint: Color32,
    pub tint: Color32,
    /// Tinted surface that white text is readable on.
    pub fill: Color32,
    pub yellow: Color32,
    pub on_yellow: Color32,
    pub green: Color32,
    pub orange: Color32,
    pub red: Color32,
}

const fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

pub fn tint_color(t: Tint, dark: bool) -> Color32 {
    hex(match (t, dark) {
        (Tint::Blue, true) => 0x2F95FF,
        (Tint::Blue, false) => 0x0966CC,
        (Tint::Amber, true) => 0xFFA62B,
        (Tint::Amber, false) => 0x9E5200,
        (Tint::Green, true) => 0x36C765,
        (Tint::Green, false) => 0x1B7437,
        (Tint::Teal, true) => 0x35C2CF,
        (Tint::Teal, false) => 0x0C6B74,
        (Tint::Indigo, true) => 0x8587FF,
        (Tint::Indigo, false) => 0x4F4CC9,
        (Tint::Purple, true) => 0xC77DFF,
        (Tint::Purple, false) => 0x8437B8,
        (Tint::Pink, true) => 0xFF6B93,
        (Tint::Pink, false) => 0xBE2350,
        (Tint::Red, true) => 0xFF6259,
        (Tint::Red, false) => 0xBB2B24,
    })
}

fn channel(c: u8) -> f32 {
    let c = f32::from(c) / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

pub fn luminance(c: Color32) -> f32 {
    0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
}

pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// Black or white, whichever reads better on `c`.
pub fn on(c: Color32) -> Color32 {
    let dark = hex(0x0B0B0D);
    if contrast(c, Color32::WHITE) >= contrast(c, dark) { Color32::WHITE } else { dark }
}

/// Darkens `c` just enough for white text.
pub fn for_white_text(mut c: Color32) -> Color32 {
    for _ in 0..40 {
        if contrast(c, Color32::WHITE) >= 4.5 {
            break;
        }
        c = blend(c, Color32::BLACK, 0.04);
    }
    c
}

impl Palette {
    pub fn new(theme: Theme, system_dark: bool, tint: Tint) -> Self {
        let dark = match theme {
            Theme::System => system_dark,
            Theme::Light => false,
            Theme::Dark | Theme::Black => true,
        };
        let tint = tint_color(tint, dark);
        if dark {
            let black = theme == Theme::Black;
            Self {
                dark,
                bg: hex(if black { 0x000000 } else { 0x0E0E10 }),
                sidebar: hex(if black { 0x0C0C0E } else { 0x161619 }),
                card: hex(0x1C1C1F),
                raised: hex(0x2A2A2E),
                hover: hex(0x27272B),
                line: hex(0x343439),
                text: hex(0xF4F4F6),
                dim: hex(0xA3A3AB),
                faint: hex(0x8A8A92),
                tint,
                fill: for_white_text(tint),
                yellow: hex(0xF5C211),
                on_yellow: hex(0x2A2000),
                green: hex(0x3CCB68),
                orange: hex(0xFFA62B),
                red: hex(0xFF6259),
            }
        } else {
            Self {
                dark,
                bg: hex(0xF3F3F7),
                sidebar: hex(0xE9E9EF),
                card: hex(0xFFFFFF),
                raised: hex(0xE4E4EA),
                hover: hex(0xEFEFF4),
                line: hex(0xDADAE0),
                text: hex(0x151518),
                dim: hex(0x5D5D66),
                faint: hex(0x66666F),
                tint,
                fill: for_white_text(tint),
                yellow: hex(0xF2BB05),
                on_yellow: hex(0x2A2000),
                green: hex(0x1B7437),
                orange: hex(0x9A5100),
                red: hex(0xBB2B24),
            }
        }
    }

    pub fn status(&self, s: ListStatus) -> Color32 {
        match s {
            ListStatus::Current => self.tint,
            ListStatus::Completed => self.green,
            ListStatus::Paused => self.orange,
            ListStatus::Dropped => self.red,
            ListStatus::Planning => self.dim,
        }
    }

    pub fn tint_soft(&self) -> Color32 {
        blend(self.card, self.tint, if self.dark { 0.22 } else { 0.14 })
    }
}

/// Colours for the browser page shown after signing in.
pub fn login_page(p: &Palette, animations: bool) -> kuro_api::oauth::PageLook {
    let rgb = |c: Color32| [c.r(), c.g(), c.b()];
    kuro_api::oauth::PageLook {
        dark: p.dark,
        animations,
        bg: rgb(p.bg),
        card: rgb(p.card),
        line: rgb(p.line),
        text: rgb(p.text),
        dim: rgb(p.dim),
        tint: rgb(p.tint),
        ok: rgb(p.green),
        on_ok: rgb(on(p.green)),
        bad: rgb(p.red),
        on_bad: rgb(on(p.red)),
    }
}

pub fn blend(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

fn family(name: &str) -> FontFamily {
    FontFamily::Name(name.into())
}

pub fn regular(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}
pub fn semibold(size: f32) -> FontId {
    FontId::new(size, family("semibold"))
}
pub fn bold(size: f32) -> FontId {
    FontId::new(size, family("bold"))
}
pub fn icon(size: f32) -> FontId {
    FontId::new(size, family("icons"))
}
pub fn icon_fill(size: f32) -> FontId {
    FontId::new(size, family("icons-fill"))
}

fn system_font(files: &[&str]) -> Option<FontData> {
    let dir = std::path::PathBuf::from(std::env::var_os("WINDIR")?).join("Fonts");
    files.iter().find_map(|f| std::fs::read(dir.join(f)).ok()).map(FontData::from_owned)
}

pub fn install_fonts(ctx: &egui::Context) {
    let mut f = FontDefinitions::default();
    let mut base = f.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    // Symbols such as stars and notes that appear in titles.
    if let Some(d) = system_font(&["seguisym.ttf"]) {
        f.font_data.insert("symbols".into(), Arc::new(d));
        base.push("symbols".into());
    }
    // Japanese (and most Chinese characters), then Hangul: the mark 黒, native titles and the
    // Korean interface. Segoe UI and egui's own fonts have none of them.
    let cjk: [(&str, &[&str]); 2] =
        [("cjk", &["YuGothM.ttc", "meiryo.ttc", "msgothic.ttc"]), ("hangul", &["malgun.ttf"])];
    for (name, files) in cjk {
        if let Some(d) = system_font(files) {
            f.font_data.insert(name.into(), Arc::new(d));
            base.push(name.into());
        }
    }
    let add = |name: &str, key: FontFamily, data: Option<FontData>, f: &mut FontDefinitions| {
        let mut list = base.clone();
        if let Some(d) = data {
            f.font_data.insert(name.to_owned(), Arc::new(d));
            list.insert(0, name.to_owned());
        }
        f.families.insert(key, list);
    };
    add("ui", FontFamily::Proportional, system_font(&["segoeui.ttf"]), &mut f);
    add("ui-semibold", family("semibold"), system_font(&["seguisb.ttf", "segoeuib.ttf"]), &mut f);
    add("ui-bold", family("bold"), system_font(&["segoeuib.ttf"]), &mut f);
    for (name, bytes) in [
        ("icons", include_bytes!("../assets/fonts/Phosphor.ttf").as_slice()),
        ("icons-fill", include_bytes!("../assets/fonts/Phosphor-Fill.ttf").as_slice()),
    ] {
        f.font_data.insert(name.into(), Arc::new(FontData::from_static(bytes)));
        f.families.insert(family(name), vec![name.into()]);
    }
    ctx.set_fonts(f);
}

pub fn apply(ctx: &egui::Context, p: &Palette, animations: bool) {
    let p = *p;
    ctx.set_theme(if p.dark { egui::Theme::Dark } else { egui::Theme::Light });
    ctx.global_style_mut(|s| {
        s.animation_time = if animations { 0.14 } else { 0.0 };
        s.spacing.item_spacing = Vec2::new(8.0, 6.0);
        s.spacing.button_padding = Vec2::new(12.0, 6.0);
        s.spacing.interact_size = Vec2::new(40.0, 30.0);
        s.spacing.menu_margin = Margin::same(6);
        s.spacing.scroll.floating = true;
        s.interaction.selectable_labels = false;
        s.url_in_tooltip = true;
        for (k, f) in &mut s.text_styles {
            *f = match k {
                egui::TextStyle::Small => regular(11.5),
                egui::TextStyle::Heading => bold(26.0),
                egui::TextStyle::Monospace => FontId::monospace(13.0),
                _ => regular(14.0),
            };
        }
        let v = &mut s.visuals;
        v.dark_mode = p.dark;
        v.override_text_color = Some(p.text);
        v.weak_text_color = Some(p.dim);
        v.panel_fill = p.bg;
        v.window_fill = p.card;
        v.window_stroke = Stroke::new(1.0, p.line);
        v.window_corner_radius = CornerRadius::same(14);
        v.menu_corner_radius = CornerRadius::same(10);
        v.extreme_bg_color = p.raised;
        v.text_edit_bg_color = Some(p.raised);
        v.faint_bg_color = p.hover;
        v.hyperlink_color = p.tint;
        v.selection.bg_fill = blend(p.card, p.tint, 0.45);
        v.selection.stroke = Stroke::new(1.0, p.text);
        v.error_fg_color = p.red;
        v.warn_fg_color = p.orange;
        v.popup_shadow = Shadow {
            offset: [0, 6],
            blur: 18,
            spread: 0,
            color: Color32::from_black_alpha(if p.dark { 120 } else { 40 }),
        };
        v.window_shadow = v.popup_shadow;
        let r = CornerRadius::same(8);
        for (w, fill) in [
            (&mut v.widgets.noninteractive, p.card),
            (&mut v.widgets.inactive, p.raised),
            (&mut v.widgets.hovered, blend(p.raised, p.text, 0.08)),
            (&mut v.widgets.active, blend(p.raised, p.text, 0.16)),
            (&mut v.widgets.open, p.raised),
        ] {
            w.corner_radius = r;
            w.bg_fill = fill;
            w.weak_bg_fill = fill;
            w.bg_stroke = Stroke::NONE;
            w.fg_stroke = Stroke::new(1.0, p.text);
            w.expansion = 0.0;
        }
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.line);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_theme_and_tint_is_readable() {
        for theme in [Theme::Light, Theme::Dark, Theme::Black] {
            for tint in Tint::ALL {
                let p = Palette::new(theme, true, tint);
                for bg in [p.bg, p.card] {
                    assert!(contrast(p.text, bg) >= 7.0);
                    for c in [p.dim, p.faint, p.tint, p.green, p.orange, p.red] {
                        assert!(
                            contrast(c, bg) >= 4.5,
                            "{theme:?} {tint:?} {c:?} on {bg:?}: {}",
                            contrast(c, bg)
                        );
                    }
                }
                assert!(contrast(Color32::WHITE, p.fill) >= 4.5, "{theme:?} {tint:?}");
                assert!(contrast(p.on_yellow, p.yellow) >= 4.5);
            }
        }
    }
}
