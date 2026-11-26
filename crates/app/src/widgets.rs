//! Reusable interface pieces.

use std::sync::Arc;

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, CursorIcon, FontId, Frame, Galley, Layout, Margin, Painter,
    Pos2, Rect, Response, Role, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, pos2, vec2,
};

use crate::icons;
use crate::theme::{self, Palette, blend};

pub const RADIUS: CornerRadius = CornerRadius::same(12);

pub fn label_a11y(r: &Response, kind: Role, text: &str) {
    r.widget_info(|| WidgetInfo::labeled(kind, true, text));
}

pub fn focus_ring(ui: &Ui, p: &Palette, r: &Response, radius: u8) {
    if r.has_focus() {
        ui.painter().rect_stroke(
            r.rect.expand(2.0),
            CornerRadius::same(radius.saturating_add(2)),
            Stroke::new(2.0, p.tint),
            StrokeKind::Outside,
        );
    }
}

pub fn title(ui: &mut Ui, p: &Palette, text: &str) {
    ui.label(egui::RichText::new(text).font(theme::bold(26.0)).color(p.text));
}

pub fn section(ui: &mut Ui, p: &Palette, text: &str) {
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.label(egui::RichText::new(text).font(theme::semibold(13.0)).color(p.dim));
    });
    ui.add_space(1.0);
}

pub fn dim(ui: &mut Ui, p: &Palette, text: impl Into<String>) -> Response {
    ui.label(egui::RichText::new(text).size(12.5).color(p.dim))
}

pub fn note(ui: &mut Ui, p: &Palette, text: &str) {
    Frame::new().inner_margin(Margin::symmetric(12, 2)).show(ui, |ui| {
        ui.add(egui::Label::new(egui::RichText::new(text).size(12.0).color(p.faint)).wrap());
    });
}

/// A rounded group.
pub fn card<R>(ui: &mut Ui, p: &Palette, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .fill(p.card)
        .corner_radius(RADIUS)
        .inner_margin(Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

pub fn separator(ui: &mut Ui, p: &Palette) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, Stroke::new(1.0, p.line));
}

/// `icon label ......... right`
pub fn row<R>(ui: &mut Ui, p: &Palette, icon: &str, label: &str, right: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        ui.set_min_height(32.0);
        if !icon.is_empty() {
            ui.add_sized(
                [22.0, 22.0],
                egui::Label::new(egui::RichText::new(icon).font(theme::icon(18.0)).color(p.tint)),
            );
        }
        let width = (ui.available_width() * 0.62).max(90.0);
        ui.scope(|ui| {
            ui.set_max_width(width);
            ui.add(egui::Label::new(label).wrap());
        });
        ui.with_layout(Layout::right_to_left(Align::Center), right).inner
    })
    .inner
}

/// A read-only fact.
pub fn fact(ui: &mut Ui, p: &Palette, label: &str, value: &str) {
    let text = |ui: &mut Ui, align| {
        let v = egui::RichText::new(value).color(p.dim);
        ui.add(egui::Label::new(v).wrap().selectable(true).halign(align));
    };
    // Long values get their own line when space is short.
    if ui.available_width() < 300.0 && value.chars().count() > 18 {
        ui.label(label);
        text(ui, Align::LEFT);
        return;
    }
    ui.horizontal(|ui| {
        ui.set_min_height(28.0);
        ui.label(label);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| text(ui, Align::RIGHT));
    });
}

/// A clickable row with a chevron.
pub fn nav_row(
    ui: &mut Ui,
    p: &Palette,
    icon: &str,
    icon_color: Color32,
    label: &str,
    value: &str,
    selected: bool,
) -> Response {
    let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::click());
    label_a11y(&r, Role::Button, label);
    let wide = rect.expand2(vec2(8.0, 0.0));
    if selected || r.hovered() {
        ui.painter().rect_filled(wide, CornerRadius::same(8), if selected { p.raised } else { p.hover });
    }
    let c = rect.center().y;
    let pt = ui.painter();
    pt.text(pos2(rect.left() + 11.0, c), Align2::CENTER_CENTER, icon, theme::icon(18.0), icon_color);
    pt.text(pos2(rect.right(), c), Align2::RIGHT_CENTER, icons::CARET_RIGHT, theme::icon(13.0), p.faint);
    let v = galley(ui, value, theme::regular(13.0), p.dim, rect.width() * 0.6, 1);
    let left = rect.left() + 32.0;
    let end = rect.right() - 20.0 - v.size().x;
    let l = galley(ui, label, theme::regular(14.0), p.text, end - left - 8.0, 1);
    ui.painter().galley(pos2(end, c - v.size().y / 2.0), v, p.dim);
    ui.painter().galley(pos2(left, c - l.size().y / 2.0), l, p.text);
    focus_ring(ui, p, &r, 8);
    r.on_hover_cursor(CursorIcon::PointingHand)
}

pub fn toggle(ui: &mut Ui, p: &Palette, on: &mut bool, label: &str) -> Response {
    let (rect, mut r) = ui.allocate_exact_size(vec2(44.0, 26.0), Sense::click());
    if r.clicked() {
        *on = !*on;
        r.mark_changed();
    }
    let value = *on;
    r.widget_info(|| WidgetInfo::selected(Role::CheckBox, true, value, label));
    let t = ui.ctx().animate_bool(r.id, *on);
    let fill = blend(p.raised, p.green, t);
    ui.painter().rect_filled(rect, CornerRadius::same(13), fill);
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(13),
        Stroke::new(1.0, blend(p.faint, fill, t)),
        StrokeKind::Inside,
    );
    let x = egui::lerp(rect.left() + 13.0..=rect.right() - 13.0, t);
    ui.painter().circle_filled(pos2(x, rect.center().y), 10.5, Color32::WHITE);
    focus_ring(ui, p, &r, 13);
    r.on_hover_cursor(CursorIcon::PointingHand)
}

pub fn toggle_row(ui: &mut Ui, p: &Palette, icon: &str, label: &str, on: &mut bool) -> bool {
    row(ui, p, icon, label, |ui| toggle(ui, p, on, label).changed())
}

pub fn icon_button(ui: &mut Ui, p: &Palette, icon: &str, label: &str) -> Response {
    icon_button_colored(ui, p, icon, label, p.tint, true)
}

pub fn icon_button_colored(
    ui: &mut Ui,
    p: &Palette,
    icon: &str,
    label: &str,
    color: Color32,
    fill: bool,
) -> Response {
    let (rect, r) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::click());
    label_a11y(&r, Role::Button, label);
    let bg = if r.is_pointer_button_down_on() {
        blend(p.tint_soft(), p.tint, 0.2)
    } else if r.hovered() {
        blend(p.tint_soft(), p.tint, 0.1)
    } else if fill {
        p.tint_soft()
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, CornerRadius::same(9), bg);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, icon, theme::icon(17.0), color);
    focus_ring(ui, p, &r, 9);
    r.on_hover_text(label).on_hover_cursor(CursorIcon::PointingHand)
}

pub fn button(ui: &mut Ui, p: &Palette, icon: &str, label: &str, primary: bool) -> Response {
    let fg = if primary { Color32::WHITE } else { p.tint };
    let text = galley(ui, label, theme::semibold(13.5), fg, f32::INFINITY, 1);
    let w = text.size().x + 26.0 + if icon.is_empty() { 0.0 } else { 23.0 };
    let (rect, r) = ui.allocate_exact_size(vec2(w, 32.0), Sense::click());
    label_a11y(&r, Role::Button, label);
    let base = if primary { p.fill } else { p.tint_soft() };
    let bg = if r.hovered() { blend(base, if primary { Color32::BLACK } else { p.text }, 0.1) } else { base };
    ui.painter().rect_filled(rect, CornerRadius::same(9), bg);
    let mut x = rect.left() + 13.0;
    if !icon.is_empty() {
        ui.painter().text(pos2(x + 8.0, rect.center().y), Align2::CENTER_CENTER, icon, theme::icon(16.0), fg);
        x += 23.0;
    }
    ui.painter().galley(pos2(x, rect.center().y - text.size().y / 2.0), text, fg);
    focus_ring(ui, p, &r, 9);
    r.on_hover_cursor(CursorIcon::PointingHand)
}

/// A drop-down in row style. Returns true when the value changed.
pub fn select<T: PartialEq + Copy>(
    ui: &mut Ui,
    id: &str,
    label: &str,
    value: &mut T,
    options: &[T],
    name: impl Fn(T) -> String,
) -> bool {
    let before = *value;
    let r = egui::ComboBox::from_id_salt(id).selected_text(name(*value)).height(360.0).width(60.0).show_ui(
        ui,
        |ui| {
            for o in options {
                ui.selectable_value(value, *o, name(*o));
            }
        },
    );
    let shown = format!("{label}: {}", name(*value));
    r.response.widget_info(|| WidgetInfo::labeled(Role::ComboBox, true, &shown));
    before != *value
}

pub fn select_row<T: PartialEq + Copy>(
    ui: &mut Ui,
    p: &Palette,
    icon: &str,
    label: &str,
    value: &mut T,
    options: &[T],
    name: impl Fn(T) -> String,
) -> bool {
    wide_row(ui, p, icon, label, |ui| select(ui, label, label, value, options, name))
}

/// Below this width a wide control moves under its label.
pub const STACK: f32 = 420.0;

pub fn wide_row<R>(
    ui: &mut Ui,
    p: &Palette,
    icon: &str,
    label: &str,
    control: impl FnOnce(&mut Ui) -> R,
) -> R {
    if ui.available_width() >= STACK {
        return row(ui, p, icon, label, control);
    }
    row(ui, p, icon, label, |_| {});
    ui.with_layout(Layout::right_to_left(Align::Min), control).inner
}

/// Mutually exclusive choices in one capsule.
pub fn segmented<T: PartialEq + Copy>(
    ui: &mut Ui,
    p: &Palette,
    value: &mut T,
    options: &[(T, &str)],
) -> bool {
    let mut changed = false;
    let total = (ui.available_width() - 8.0).min(options.len() as f32 * 150.0).max(1.0);
    let w = total / options.len().max(1) as f32;
    let (bar, base) = ui.allocate_exact_size(vec2(total, 32.0), Sense::hover());
    ui.painter().rect_filled(bar, CornerRadius::same(9), p.raised);
    for (i, (v, text)) in options.iter().enumerate() {
        let rect = Rect::from_min_size(bar.min + vec2(w * i as f32, 0.0), vec2(w, 32.0)).shrink(2.0);
        let r = ui.interact(rect, base.id.with(i), Sense::click());
        let on = *value == *v;
        r.widget_info(|| WidgetInfo::selected(Role::RadioButton, true, on, *text));
        if on {
            ui.painter().rect_filled(
                rect,
                CornerRadius::same(7),
                if p.dark { blend(p.raised, p.text, 0.2) } else { p.card },
            );
        }
        let g = galley(ui, text, theme::semibold(13.0), if on { p.text } else { p.dim }, w - 8.0, 1);
        ui.painter().galley(rect.center() - g.size() / 2.0, g, p.text);
        focus_ring(ui, p, &r, 7);
        if r.on_hover_cursor(CursorIcon::PointingHand).clicked() && !on {
            *value = *v;
            changed = true;
        }
    }
    changed
}

pub fn search_field(ui: &mut Ui, p: &Palette, text: &mut String, hint: &str) -> Response {
    let w = ui.available_width();
    let r = ui.add(
        egui::TextEdit::singleline(text)
            .hint_text(hint)
            .desired_width(w - 12.0)
            .char_limit(200)
            .margin(Margin { left: 30, right: 28, top: 6, bottom: 6 }),
    );
    ui.painter().text(
        pos2(r.rect.left() + 16.0, r.rect.center().y),
        Align2::CENTER_CENTER,
        icons::MAGNIFYING_GLASS,
        theme::icon(15.0),
        p.dim,
    );
    if !text.is_empty() {
        let x = Rect::from_center_size(pos2(r.rect.right() - 16.0, r.rect.center().y), Vec2::splat(22.0));
        let clear = ui.interact(x, r.id.with("clear"), Sense::click());
        label_a11y(&clear, Role::Button, crate::i18n::t("Clear"));
        ui.painter().text(x.center(), Align2::CENTER_CENTER, icons::X, theme::icon(13.0), p.dim);
        if clear.on_hover_cursor(CursorIcon::PointingHand).clicked() {
            text.clear();
            r.request_focus();
        }
    }
    r
}

/// Text limited to `rows` lines, elided at the end.
pub fn galley(ui: &Ui, text: &str, font: FontId, color: Color32, width: f32, rows: usize) -> Arc<Galley> {
    let mut job = egui::text::LayoutJob::simple(text.to_owned(), font, color, width.max(1.0));
    job.wrap.max_rows = rows;
    job.wrap.break_anywhere = false;
    ui.ctx().fonts_mut(|f| f.layout_job(job))
}

pub fn paint_text(
    ui: &Ui,
    pos: Pos2,
    text: &str,
    font: FontId,
    color: Color32,
    width: f32,
    rows: usize,
) -> Vec2 {
    let g = galley(ui, text, font, color, width, rows);
    let size = g.size();
    ui.painter().galley(pos, g, color);
    size
}

/// A filled capsule with an icon. Returns its rectangle.
pub fn pill(pt: &Painter, pos: Pos2, icon: &str, text: &str, fill: Color32, fg: Color32) -> Rect {
    let g = pt.layout_no_wrap(text.to_owned(), theme::semibold(11.5), fg);
    let iw = if icon.is_empty() { 0.0 } else { 16.0 };
    let rect = Rect::from_min_size(pos, vec2(g.size().x + iw + 14.0, 20.0));
    pt.rect_filled(rect, CornerRadius::same(7), fill);
    if !icon.is_empty() {
        pt.text(
            pos2(rect.left() + 13.0, rect.center().y),
            Align2::CENTER_CENTER,
            icon,
            theme::icon_fill(12.0),
            fg,
        );
    }
    pt.galley(pos2(rect.left() + 7.0 + iw, rect.center().y - g.size().y / 2.0), g, fg);
    rect
}

pub fn bar(pt: &Painter, rect: Rect, fraction: f32, fill: Color32, track: Color32) {
    let r = CornerRadius::same((rect.height() / 2.0) as u8);
    pt.rect_filled(rect, r, track);
    let w = rect.width() * fraction.clamp(0.0, 1.0);
    if w > 0.5 {
        pt.rect_filled(Rect::from_min_size(rect.min, vec2(w, rect.height())), r, fill);
    }
}

pub fn empty(ui: &mut Ui, p: &Palette, icon: &str, text: &str) {
    ui.add_space(40.0);
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new(icon).font(theme::icon(44.0)).color(p.faint));
        ui.add_space(4.0);
        ui.label(egui::RichText::new(text).size(14.0).color(p.dim));
    });
}

/// An error with a retry button. Returns true when pressed.
pub fn failure(ui: &mut Ui, p: &Palette, text: &str) -> bool {
    empty(ui, p, icons::WARNING, text);
    ui.add_space(8.0);
    ui.vertical_centered(|ui| {
        button(ui, p, icons::ARROWS_CLOCKWISE, crate::i18n::t("Retry"), false).clicked()
    })
    .inner
}

pub fn loading(ui: &mut Ui, p: &Palette) {
    ui.add_space(40.0);
    ui.vertical_centered(|ui| ui.add(egui::Spinner::new().size(26.0).color(p.tint)));
}

/// A rounded diagonal gradient, darkened so white text stays readable.
pub fn gradient(ui: &Ui, rect: Rect, a: Color32, b: Color32) {
    let (a, b) = (theme::for_white_text(a), theme::for_white_text(b));
    let r = 14.0;
    let mut mesh = egui::Mesh::default();
    let color = |pos: Pos2| {
        let t = ((pos.x - rect.left()) / rect.width() + (pos.y - rect.top()) / rect.height()) / 2.0;
        blend(a, b, t.clamp(0.0, 1.0))
    };
    mesh.colored_vertex(rect.center(), color(rect.center()));
    let corners = [
        (pos2(rect.right() - r, rect.top() + r), -90.0_f32),
        (pos2(rect.right() - r, rect.bottom() - r), 0.0),
        (pos2(rect.left() + r, rect.bottom() - r), 90.0),
        (pos2(rect.left() + r, rect.top() + r), 180.0),
    ];
    for (c, start) in corners {
        for i in 0..=8 {
            let ang = (start + 90.0 * i as f32 / 8.0).to_radians();
            let v = c + vec2(ang.cos(), ang.sin()) * r;
            mesh.colored_vertex(v, color(v));
        }
    }
    let n = mesh.vertices.len() as u32 - 1;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    ui.painter().add(mesh);
}
