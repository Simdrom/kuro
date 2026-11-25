//! Images on the GPU, loaded on demand.

use std::collections::HashMap;

use eframe::egui::{
    self, Color32, ColorImage, CornerRadius, Rect, TextureHandle, TextureOptions, Ui, pos2, vec2,
};

use crate::backend::Backend;
use crate::theme::Palette;

enum Slot {
    Loading,
    Ready(TextureHandle),
    Failed,
}

#[derive(Default)]
pub struct Textures {
    slots: HashMap<String, (Slot, u64)>,
    frame: u64,
}

const MAX: usize = 350;

impl Textures {
    pub fn begin_frame(&mut self) {
        self.frame += 1;
        if self.slots.len() > MAX {
            let keep = self.frame.saturating_sub(120);
            self.slots.retain(|_, (s, used)| *used >= keep || matches!(s, Slot::Loading));
        }
    }

    pub fn insert(&mut self, ctx: &egui::Context, url: String, image: Option<ColorImage>) {
        let slot = match image {
            Some(i) => Slot::Ready(ctx.load_texture(
                &url,
                i,
                TextureOptions::LINEAR.with_mipmap_mode(Some(egui::TextureFilter::Linear)),
            )),
            None => Slot::Failed,
        };
        self.slots.insert(url, (slot, self.frame));
    }

    /// Lets failed downloads be tried again.
    pub fn forget_failures(&mut self) {
        self.slots.retain(|_, (s, _)| !matches!(s, Slot::Failed));
    }

    pub fn get(&mut self, bg: &Backend, url: &str) -> Option<&TextureHandle> {
        let frame = self.frame;
        if !self.slots.contains_key(url) {
            bg.image(url.to_owned());
            self.slots.insert(url.to_owned(), (Slot::Loading, frame));
        }
        let (slot, used) = self.slots.get_mut(url)?;
        *used = frame;
        match slot {
            Slot::Ready(t) => Some(t),
            _ => None,
        }
    }

    /// Paints an image that fills `rect`, cropping what does not fit.
    pub fn paint(&mut self, ui: &Ui, p: &Palette, bg: &Backend, url: Option<&str>, rect: Rect, radius: u8) {
        self.paint_rounded(ui, p, bg, url, rect, CornerRadius::same(radius));
    }

    pub fn paint_rounded(
        &mut self,
        ui: &Ui,
        p: &Palette,
        bg: &Backend,
        url: Option<&str>,
        rect: Rect,
        cr: CornerRadius,
    ) {
        if !ui.is_rect_visible(rect) {
            return;
        }
        match url.filter(|u| !u.is_empty()).and_then(|u| self.get(bg, u)) {
            Some(t) => {
                let size = t.size_vec2();
                let scale = (rect.width() / size.x).max(rect.height() / size.y);
                let shown = vec2(rect.width() / (size.x * scale), rect.height() / (size.y * scale));
                let uv = Rect::from_center_size(pos2(0.5, 0.5), shown);
                egui::Image::from_texture(t).uv(uv).corner_radius(cr).paint_at(ui, rect);
            }
            None => {
                ui.painter().rect_filled(rect, cr, p.raised);
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    crate::icons::IMAGE,
                    crate::theme::icon((rect.width() * 0.3).clamp(10.0, 32.0)),
                    Color32::from_gray(if p.dark { 90 } else { 170 }),
                );
            }
        }
    }
}
