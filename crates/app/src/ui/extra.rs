//! Series overview, voice actors and full reviews.

use std::collections::hash_map::Entry;
use std::rc::Rc;

use eframe::egui::{self, CursorIcon, Rect, RichText, Role, Sense, Ui, pos2, vec2};
use mango_core::{ListStatus, Media, MediaKind, MediaRef};

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
                    widgets::card(ui, &p, |ui| {
                        let shown: Vec<_> = list.iter().filter(|m| app.s.show_adult || !m.adult).collect();
                        for (i, m) in shown.into_iter().enumerate() {
                            if i > 0 {
                                widgets::separator(ui, &p);
                            }
                            overview_row(app, ui, m);
                        }
                    });
                }
            });
        }
    }
}

fn overview_row(app: &mut App, ui: &mut Ui, m: &Media) {
    let p = app.p;
    let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), 62.0), Sense::click());
    let title = m.display_title(app.s.title_language).to_owned();
    widgets::label_a11y(&r, Role::Button, &title);
    if r.hovered() || r.has_focus() {
        ui.painter().rect_filled(rect.expand2(vec2(8.0, 0.0)), 8, p.hover);
    }
    let pic = Rect::from_min_size(rect.min + vec2(0.0, 4.0), vec2(38.0, 54.0));
    app.tex.paint(ui, &p, &app.bg, m.cover.as_deref(), pic, 5);
    let x = pic.right() + 12.0;
    let when = match (m.kind, m.season_year(), m.start) {
        (MediaKind::Anime, Some(s), _) => format::season_year(s),
        (_, _, Some(d)) => format::month(d),
        _ => format::release(m.release, m.kind).to_owned(),
    };
    let size = widgets::paint_text(
        ui,
        pos2(x, rect.top() + 12.0),
        &when.to_uppercase(),
        theme::semibold(11.0),
        p.text,
        200.0,
        1,
    );
    if let Some(e) = app.entry(m) {
        let s = format!("·  {}", format::status(e.status, m.kind).to_uppercase());
        let at = pos2(x + size.x + 8.0, rect.top() + 12.0);
        widgets::paint_text(ui, at, &s, theme::semibold(11.0), p.status(e.status), 200.0, 1);
    }
    let w = rect.right() - x - 24.0;
    widgets::paint_text(ui, pos2(x, rect.top() + 31.0), &title, theme::regular(13.5), p.text, w, 1);
    let c = pos2(rect.right() - 6.0, rect.center().y);
    ui.painter().text(c, egui::Align2::CENTER_CENTER, icons::CARET_RIGHT, theme::icon(13.0), p.faint);
    if r.on_hover_cursor(CursorIcon::PointingHand).clicked() {
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
        widgets::card(ui, &p, |ui| {
            for (i, role) in roles.into_iter().enumerate() {
                if i > 0 {
                    widgets::separator(ui, &p);
                }
                let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), 66.0), Sense::click());
                let title = role.media.display_title(app.s.title_language).to_owned();
                widgets::label_a11y(&r, Role::Button, &format!("{}, {title}", role.character));
                if r.hovered() || r.has_focus() {
                    ui.painter().rect_filled(rect.expand2(vec2(8.0, 0.0)), 8, p.hover);
                }
                let a = Rect::from_min_size(rect.min + vec2(0.0, 4.0), vec2(42.0, 58.0));
                app.tex.paint(ui, &p, &app.bg, role.character_image.as_deref(), a, 6);
                let b = Rect::from_min_size(pos2(rect.right() - 42.0, rect.top() + 4.0), vec2(42.0, 58.0));
                app.tex.paint(ui, &p, &app.bg, role.media.cover.as_deref(), b, 6);
                let w = b.left() - a.right() - 24.0;
                let x = a.right() + 12.0;
                widgets::paint_text(
                    ui,
                    pos2(x, rect.top() + 14.0),
                    &role.character,
                    theme::semibold(13.5),
                    p.text,
                    w,
                    1,
                );
                widgets::paint_text(
                    ui,
                    pos2(x, rect.top() + 34.0),
                    &title,
                    theme::regular(12.5),
                    p.dim,
                    w,
                    1,
                );
                if r.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                    app.act(Act::Open(Rc::new(role.media.clone())));
                }
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
