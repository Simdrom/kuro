//! Series overview, voice actors and full reviews.

use std::collections::hash_map::Entry;
use std::rc::Rc;

use eframe::egui::{self, CursorIcon, Rect, RichText, Role, Sense, Ui, pos2, vec2};
use kuro_core::{ListStatus, Media, MediaKind, MediaRef};

use crate::app::{Act, App, Load};
use crate::i18n::t;
use crate::theme;
use crate::widgets;
use crate::{format, icons};

pub fn overview(app: &mut App, ui: &mut Ui, r: MediaRef) {
    let p = app.p;
    super::header(app, ui, t("Complete series overview"), |_, _| {});
    let Some(root) = app.details.get(&r).and_then(|d| d.data.as_ref()).map(|d| d.media.clone()) else {
        app.ensure_detail(r);
        widgets::loading(ui, &p);
        return;
    };
    if let Entry::Vacant(e) = app.overviews.entry(r) {
        e.insert(Load::Busy);
        app.bg.overview(root);
    }
    match &app.overviews[&r] {
        Load::Busy => widgets::loading(ui, &p),
        Load::Failed(e) => {
            let text = format::error(e);
            if widgets::failure(ui, &p, &text) {
                app.overviews.remove(&r);
            }
        }
        Load::Ready(v) => {
            let v = v.clone();
            super::scroll(ui, ("overview", r), |ui| {
                for (s, list) in v.iter() {
                    widgets::section(ui, &p, format::section(*s, r.kind));
                    let shown: Vec<_> = list.iter().filter(|m| app.s.show_adult || !m.adult).collect();
                    cells(app, ui, shown.len(), 300.0, 74.0, |app, ui, rect, i| {
                        overview_cell(app, ui, rect, shown[i])
                    });
                }
            });
        }
    }
}

/// A plain grid of cells, as many columns as fit.
fn cells(
    app: &mut App,
    ui: &mut Ui,
    count: usize,
    min: f32,
    height: f32,
    mut draw: impl FnMut(&mut App, &mut Ui, Rect, usize),
) {
    let gap = 10.0;
    let avail = ui.available_width();
    let (cols, w) = super::rows::columns(avail, min, gap);
    let rows = count.div_ceil(cols);
    let (area, _) = ui.allocate_exact_size(vec2(avail, rows as f32 * (height + gap)), Sense::hover());
    for i in 0..count {
        let at = area.min + vec2((i % cols) as f32 * (w + gap), (i / cols) as f32 * (height + gap));
        let rect = Rect::from_min_size(at, vec2(w, height));
        if ui.is_rect_visible(rect) {
            draw(app, ui, rect, i);
        }
    }
}

/// A clickable card background. Returns its response.
fn cell_base(
    app: &App,
    ui: &Ui,
    rect: Rect,
    id: impl std::hash::Hash + std::fmt::Debug,
    name: &str,
) -> egui::Response {
    let p = app.p;
    let r = ui.interact(rect, ui.scope_id().with(id), Sense::click());
    widgets::label_a11y(&r, Role::Button, name);
    ui.painter().rect_filled(rect, widgets::RADIUS, if r.hovered() { p.hover } else { p.card });
    if r.has_focus() {
        let s = egui::Stroke::new(2.0, p.tint);
        ui.painter().rect_stroke(rect, widgets::RADIUS, s, egui::StrokeKind::Inside);
    }
    r.on_hover_cursor(CursorIcon::PointingHand)
}

fn overview_cell(app: &mut App, ui: &mut Ui, rect: Rect, m: &Media) {
    let p = app.p;
    let title = m.display_title(app.s.title_language).to_owned();
    let r = cell_base(app, ui, rect, ("overview", m.media_ref()), &title);
    let pic = Rect::from_min_size(rect.min + vec2(10.0, 8.0), vec2(40.0, 58.0));
    app.tex.paint(ui, &p, &app.bg, m.cover.as_deref(), pic, 5);
    let x = pic.right() + 12.0;
    let w = rect.right() - x - 10.0;
    let when = match (m.kind, m.season_year(), m.start) {
        (MediaKind::Anime, Some(s), _) => format::season_year(s),
        (_, _, Some(d)) => format::month(d),
        _ => format::release(m.release, m.kind).to_owned(),
    };
    let y = rect.top() + 10.0;
    let size = widgets::paint_text(ui, pos2(x, y), &when.to_uppercase(), theme::semibold(11.0), p.dim, w, 1);
    if let Some(e) = app.entry(m) {
        let s = format!("·  {}", format::status(e.status, m.kind).to_uppercase());
        let room = (w - size.x - 8.0).max(1.0);
        widgets::paint_text(
            ui,
            pos2(x + size.x + 8.0, y),
            &s,
            theme::semibold(11.0),
            p.status(e.status),
            room,
            1,
        );
    }
    widgets::paint_text(ui, pos2(x, y + 16.0), &title, theme::semibold(13.5), p.text, w, 2);
    if r.clicked() {
        app.act(Act::Open(Rc::new(m.clone())));
    }
}

pub fn person(app: &mut App, ui: &mut Ui, id: u64) {
    let p = app.p;
    if let Entry::Vacant(e) = app.people.entry(id) {
        e.insert(Load::Busy);
        app.bg.person(id);
    }
    let data = match &app.people[&id] {
        Load::Ready(v) => Some(v.clone()),
        _ => None,
    };
    let name = data.as_ref().map(|d| d.person.name.clone()).unwrap_or_default();
    super::header(app, ui, &name, |_, _| {});
    let Some(d) = data else {
        if let Load::Failed(e) = &app.people[&id] {
            let text = format::error(e);
            if widgets::failure(ui, &p, &text) {
                app.people.remove(&id);
            }
        } else {
            widgets::loading(ui, &p);
        }
        return;
    };
    super::scroll(ui, ("person", id), |ui| {
        ui.horizontal_top(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(120.0, 160.0), Sense::hover());
            app.tex.paint(ui, &p, &app.bg, d.person.image.as_deref(), rect, 10);
            ui.add_space(6.0);
            ui.vertical(|ui| {
                ui.label(RichText::new(&d.person.name).font(theme::bold(20.0)));
                if let Some(n) = &d.native_name {
                    ui.label(RichText::new(n).color(p.dim));
                }
                if let Some(l) = &d.person.language {
                    ui.label(RichText::new(l).color(p.dim));
                }
            });
        });
        if let Some(text) = &d.description {
            ui.add_space(6.0);
            widgets::card(ui, &p, |ui| ui.add(egui::Label::new(text).wrap().selectable(true)));
        }
        widgets::section(ui, &p, t("Roles"));
        widgets::card(ui, &p, |ui| {
            widgets::toggle_row(
                ui,
                &p,
                icons::CHECK,
                t("Only anime I have completed"),
                &mut app.roles_completed_only,
            );
        });
        ui.add_space(6.0);
        let roles: Vec<_> = d
            .roles
            .iter()
            .filter(|r| app.s.show_adult || !r.media.adult)
            .filter(|r| {
                !app.roles_completed_only
                    || app.entry(&r.media).is_some_and(|e| e.status == ListStatus::Completed)
            })
            .collect();
        if roles.is_empty() {
            widgets::empty(ui, &p, icons::MICROPHONE, t("Nothing here"));
            return;
        }
        cells(app, ui, roles.len(), 320.0, 78.0, |app, ui, rect, i| {
            let role = roles[i];
            let title = role.media.display_title(app.s.title_language).to_owned();
            let r = cell_base(app, ui, rect, ("role", i), &format!("{}, {title}", role.character));
            let a = Rect::from_min_size(rect.min + vec2(10.0, 9.0), vec2(43.0, 60.0));
            app.tex.paint(ui, &p, &app.bg, role.character_image.as_deref(), a, 6);
            let b = Rect::from_min_size(pos2(rect.right() - 53.0, rect.top() + 9.0), vec2(43.0, 60.0));
            app.tex.paint(ui, &p, &app.bg, role.media.cover.as_deref(), b, 6);
            let x = a.right() + 12.0;
            let w = (b.left() - x - 12.0).max(1.0);
            let top = rect.top();
            widgets::paint_text(
                ui,
                pos2(x, top + 18.0),
                &role.character,
                theme::semibold(13.5),
                p.text,
                w,
                1,
            );
            widgets::paint_text(ui, pos2(x, top + 40.0), &title, theme::regular(12.5), p.dim, w, 1);
            if r.clicked() {
                app.act(Act::Open(Rc::new(role.media.clone())));
            }
        });
    });
}

pub fn review(app: &mut App, ui: &mut Ui, r: MediaRef, id: u64) {
    let p = app.p;
    let data = app.details.get(&r).and_then(|d| d.data.clone());
    let Some(v) = data.as_ref().and_then(|d| d.reviews.iter().find(|v| v.id == id)) else {
        app.act(Act::Back);
        return;
    };
    super::header(app, ui, &v.user, |_, _| {});
    super::scroll(ui, ("review", id), |ui| {
        widgets::card(ui, &p, |ui| {
            ui.label(RichText::new(&v.summary).font(theme::semibold(16.0)));
            widgets::dim(ui, &p, format::date_time(v.created_at));
            ui.add_space(6.0);
            ui.add(egui::Label::new(RichText::new(&v.body).line_height(Some(20.0))).wrap().selectable(true));
        });
    });
}
