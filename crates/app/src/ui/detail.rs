//! Everything about one title.

use std::rc::Rc;

use eframe::egui::{
    self, Align, Align2, CornerRadius, CursorIcon, Layout, Rect, RichText, Role, Sense, Ui, pos2, vec2,
};
use kuro_api::music;
use kuro_core::settings::GestureAction;
use kuro_core::{ListEntry, ListStatus, Media, MediaDetail, MediaKind, MediaRef, Service, Theme, ThemeKind};

use super::rows::{self, Item};
use crate::app::{Act, App, Route};
use crate::backend::now;
use crate::i18n::{decimal, number, t, tf};
use crate::theme;
use crate::widgets;
use crate::{format, icons};

pub fn show(app: &mut App, ui: &mut Ui) {
    match app.routes.last().cloned() {
        None => {
            let p = app.p;
            ui.add_space(ui.available_height() * 0.3);
            let text = if app.tab == crate::app::Tab::Settings {
                t("Choose a setting")
            } else {
                t("Choose a title")
            };
            widgets::empty(ui, &p, icons::SPARKLE, text);
        }
        Some(Route::Media(r)) => media(app, ui, r),
        Some(Route::Overview(r)) => super::extra::overview(app, ui, r),
        Some(Route::Person(id)) => super::extra::person(app, ui, id),
        Some(Route::Review(r, id)) => super::extra::review(app, ui, r, id),
        Some(Route::Settings(page)) => super::settings::page(app, ui, page),
        Some(Route::Kioku) => super::kioku::show(app, ui),
        Some(Route::Friend(f)) => super::friends::profile(app, ui, &f),
    }
}

fn media(app: &mut App, ui: &mut Ui, r: MediaRef) {
    app.ensure_detail(r);
    let p = app.p;
    let d = &app.details[&r];
    let (data, error, loading) = (d.data.clone(), d.error.clone(), d.loading);
    let Some(d) = data else {
        super::header(app, ui, "", |_, _| {});
        match error {
            Some(e) => {
                if widgets::failure(ui, &p, &format::error(&e)) {
                    app.act(Act::Retry(r));
                }
            }
            None => widgets::loading(ui, &p),
        }
        return;
    };
    let m = Rc::new(d.media.clone());
    let title = m.display_title(app.s.title_language).to_owned();
    super::header(app, ui, &title, |app, ui| {
        share_button(app, ui, &m);
        more_button(app, ui, &m);
        if loading {
            ui.add(egui::Spinner::new().size(16.0).color(p.dim));
        }
    });
    super::scroll(ui, ("detail", r), |ui| {
        let w = ui.available_width();
        if w >= TWO_COLUMNS {
            let side = (w * 0.28).clamp(260.0, 320.0);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 20.0;
                super::column(ui, side, |ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(side, side * 1.42), Sense::hover());
                    app.tex.paint(ui, &p, &app.bg, m.cover.as_deref(), rect, 14);
                    ui.add_space(8.0);
                    my_card(app, ui, &m);
                    info(app, ui, &d);
                    titles(app, ui, &m);
                    links(app, ui, &d);
                });
                super::column(ui, w - side - 20.0, |ui| {
                    facts(app, ui, &m, 28.0);
                    ui.add_space(12.0);
                    numbers(app, ui, &m);
                    story(app, ui, &m, &d, &error);
                    if m.kind == MediaKind::Anime && app.s.show_themes {
                        themes(app, ui, &m);
                    }
                    cast(app, ui, &d);
                    shelves(app, ui, &d);
                    stats(app, ui, &d);
                    if app.s.show_reviews {
                        reviews(app, ui, r, &d);
                    }
                });
            });
            return;
        }
        hero(app, ui, &m);
        ui.add_space(8.0);
        numbers(app, ui, &m);
        ui.add_space(8.0);
        my_card(app, ui, &m);
        story(app, ui, &m, &d, &error);
        info(app, ui, &d);
        if m.kind == MediaKind::Anime && app.s.show_themes {
            themes(app, ui, &m);
        }
        titles(app, ui, &m);
        cast(app, ui, &d);
        shelves(app, ui, &d);
        stats(app, ui, &d);
        if app.s.show_reviews {
            reviews(app, ui, r, &d);
        }
        links(app, ui, &d);
    });
}

/// From this width the cover, entry and facts get a column of their own.
const TWO_COLUMNS: f32 = 860.0;

/// Errors, franchise and synopsis.
fn story(app: &mut App, ui: &mut Ui, m: &Rc<Media>, d: &MediaDetail, error: &Option<kuro_api::ApiError>) {
    let p = app.p;
    if let Some(e) = error {
        ui.add_space(6.0);
        widgets::card(ui, &p, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format::error(e)).color(p.orange));
                if widgets::button(ui, &p, "", t("Retry"), false).clicked() {
                    app.act(Act::Retry(m.media_ref()));
                }
            });
        });
    }
    overview_button(app, ui, m, d);
    synopsis(app, ui, m);
    if let Some(b) = &m.background {
        widgets::section(ui, &p, t("Background"));
        widgets::card(ui, &p, |ui| ui.add(egui::Label::new(b).wrap().selectable(true)));
    }
}

fn share_button(app: &mut App, ui: &mut Ui, m: &Rc<Media>) {
    let p = app.p;
    let r = widgets::icon_button_colored(ui, &p, icons::EXPORT, t("Share"), p.tint, false);
    egui::Popup::menu(&r).show(|ui| {
        ui.set_min_width(200.0);
        for s in Service::ALL {
            let Some(u) = m.web_url(s) else { continue };
            if ui.button(tf("Copy {} link", &[&s.name()])).clicked() {
                app.act(Act::Copy(u.clone()));
            }
            if ui.button(tf("Open on {}", &[&s.name()])).clicked() {
                app.act(Act::Url(u));
            }
        }
    });
}

fn more_button(app: &mut App, ui: &mut Ui, m: &Rc<Media>) {
    let p = app.p;
    let r = widgets::icon_button_colored(ui, &p, icons::DOTS_THREE_CIRCLE, t("More"), p.tint, false);
    let entry = app.entry(m);
    egui::Popup::menu(&r).show(|ui| {
        rows::menu(app, ui, &Item { media: m.clone(), entry, rank: None });
        ui.separator();
        if m.kind == MediaKind::Anime {
            let r = m.media_ref();
            let muted = app.s.notifications.muted.contains(&r);
            let text = if muted { t("Turn on notifications") } else { t("Mute notifications") };
            if ui.button(text).clicked() {
                app.act(Act::ToggleMute(r));
            }
        }
        if ui.button(t("Reload")).clicked() {
            app.act(Act::Retry(m.media_ref()));
        }
    });
}

fn hero(app: &mut App, ui: &mut Ui, m: &Rc<Media>) {
    let p = app.p;
    let cover = (ui.available_width() * 0.34).clamp(96.0, 170.0);
    ui.horizontal_top(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(cover, cover * 1.42), Sense::hover());
        app.tex.paint(ui, &p, &app.bg, m.cover.as_deref(), rect, 10);
        ui.add_space(6.0);
        let size = if cover < 130.0 { 17.0 } else { 20.0 };
        ui.vertical(|ui| facts(app, ui, m, size));
    });
}

/// Titles, format, next episode and dates.
fn facts(app: &mut App, ui: &mut Ui, m: &Rc<Media>, size: f32) {
    let p = app.p;
    let title = m.display_title(app.s.title_language).to_owned();
    ui.add(egui::Label::new(RichText::new(&title).font(theme::bold(size))).wrap().selectable(true));
    let other = [Some(&m.title), m.title_english.as_ref(), m.title_native.as_ref()];
    if let Some(x) = other.into_iter().flatten().find(|x| **x != title) {
        ui.add(egui::Label::new(RichText::new(x).color(p.dim)).wrap());
    }
    ui.add_space(4.0);
    let line = format!("{} · {}", format::format(m.format), format::length(m));
    ui.add(egui::Label::new(RichText::new(line).color(p.dim)).wrap());
    if let Some(a) = m.next_airing.filter(|a| a.at > now()) {
        ui.add_space(4.0);
        let text = tf("Episode {} in {}", &[&a.episode, &format::countdown(a.at, now())]);
        let text = RichText::new(text).font(theme::semibold(13.5)).color(p.tint);
        ui.add(egui::Label::new(text).wrap()).on_hover_text(format::date_time(a.at));
    }
    ui.add_space(6.0);
    let mut parts = vec![format::release(m.release, m.kind).to_owned(), format::dates(m)];
    if let Some(n) = m.members {
        parts.push(tf("{} members", &[&number(n)]));
    }
    parts.retain(|s| !s.is_empty());
    ui.add(egui::Label::new(RichText::new(parts.join("  ·  ")).color(p.dim)).wrap());
}

fn numbers(app: &mut App, ui: &mut Ui, m: &Media) {
    let p = app.p;
    let cell = |ui: &mut Ui, name: &str, value: String, sub: String| {
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(name).font(theme::semibold(13.0)));
            ui.label(RichText::new(value).font(theme::bold(22.0)).color(p.tint));
            ui.label(RichText::new(sub).size(11.5).color(p.dim));
        });
    };
    let hash = |n: Option<u32>| n.map_or("–".into(), |n| format!("# {}", number(n.into())));
    ui.columns(3, |c| {
        let by = m.scored_by.map(|n| tf("{} ratings", &[&number(n)])).unwrap_or_default();
        cell(&mut c[0], t("Score"), m.mean_score.map_or("–".into(), |s| decimal(s, 2)), by);
        cell(&mut c[1], t("Ranked"), hash(m.rank), String::new());
        cell(&mut c[2], t("Popularity"), hash(m.popularity), String::new());
    });
}

fn stepper(app: &mut App, ui: &mut Ui, label: &str, value: String, minus: Act, plus: Act, can_plus: bool) {
    ui.scope(|ui| {
        ui.horizontal(|ui| {
            let b = |ui: &mut Ui, icon: &str, text: &str, enabled: bool| {
                let (rect, r) = ui.allocate_exact_size(
                    vec2(30.0, 30.0),
                    if enabled { Sense::click() } else { Sense::hover() },
                );
                widgets::label_a11y(&r, Role::Button, &format!("{text}: {label}"));
                let a = if !enabled {
                    0.12
                } else if r.hovered() {
                    0.45
                } else {
                    0.28
                };
                ui.painter().circle_filled(rect.center(), 14.0, egui::Color32::WHITE.gamma_multiply(a));
                if r.has_focus() {
                    ui.painter().circle_stroke(
                        rect.center(),
                        15.0,
                        egui::Stroke::new(2.0, egui::Color32::WHITE),
                    );
                }
                let fg = egui::Color32::WHITE.gamma_multiply(if enabled { 1.0 } else { 0.5 });
                ui.painter().text(rect.center(), Align2::CENTER_CENTER, icon, theme::icon(15.0), fg);
                r.on_hover_cursor(CursorIcon::PointingHand).clicked()
            };
            if b(ui, icons::MINUS, t("Remove one"), true) {
                app.act(minus);
            }
            let text = format!("{label}: {value}");
            ui.label(RichText::new(text).font(theme::semibold(14.0)).color(egui::Color32::WHITE));
            if b(ui, icons::PLUS, t("Add one"), can_plus) {
                app.act(plus);
            }
        });
    });
}

fn my_card(app: &mut App, ui: &mut Ui, m: &Rc<Media>) {
    let p = app.p;
    let Some(e) = app.entry(m) else {
        widgets::card(ui, &p, |ui| {
            let stack = ui.available_width() < widgets::STACK;
            if stack {
                ui.label(RichText::new(t("Not in your list")).color(p.dim));
            }
            ui.horizontal_wrapped(|ui| {
                if !stack {
                    ui.label(RichText::new(t("Not in your list")).color(p.dim));
                    ui.add_space(8.0);
                }
                let plan = format::status(ListStatus::Planning, m.kind);
                if widgets::button(ui, &p, icons::PLUS, plan, true).clicked() {
                    app.act(Act::SetStatus(m.clone(), ListStatus::Planning));
                }
                if widgets::button(ui, &p, icons::PENCIL_SIMPLE, t("Add…"), false).clicked() {
                    app.act(Act::Edit(m.clone()));
                }
            });
        });
        return;
    };
    egui::Frame::new().fill(p.fill).corner_radius(widgets::RADIUS).inner_margin(14).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let fg = egui::Color32::WHITE;
        ui.horizontal(|ui| {
            let icon = format::status_icon(e.status);
            ui.label(RichText::new(icon).font(theme::icon_fill(18.0)).color(fg));
            let mut s = format::status(e.status, m.kind).to_owned();
            if e.repeating {
                s = if m.kind == MediaKind::Anime { t("Rewatching").into() } else { t("Rereading").into() };
            }
            ui.label(RichText::new(s).font(theme::bold(16.0)).color(fg));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let (rect, r) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::click());
                widgets::label_a11y(&r, Role::Button, t("Edit"));
                let a = if r.hovered() || r.has_focus() { 0.45 } else { 0.28 };
                ui.painter().circle_filled(rect.center(), 15.0, fg.gamma_multiply(a));
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    icons::PENCIL_SIMPLE,
                    theme::icon(16.0),
                    fg,
                );
                if r.on_hover_text(t("Edit")).on_hover_cursor(CursorIcon::PointingHand).clicked() {
                    app.act(Act::Edit(m.clone()));
                }
                let score = if e.score > 0 {
                    decimal(e.score10(), usize::from(!e.score.is_multiple_of(10)))
                } else {
                    "–".into()
                };
                ui.label(RichText::new(score).font(theme::bold(15.0)).color(fg));
                ui.label(RichText::new(icons::STAR).font(theme::icon_fill(15.0)).color(fg));
            });
        });
        ui.add_space(6.0);
        let of = |n: Option<u32>| n.map_or("?".into(), |n| n.to_string());
        let can = e.status != ListStatus::Completed || e.repeating;
        let side_by_side = ui.available_width() >= 440.0;
        let steppers = |ui: &mut Ui| {
            if m.kind == MediaKind::Manga {
                let v = format!("{} / {}", e.progress_volumes, of(m.volumes));
                let (a, b) = (Act::StepVolumes(m.clone(), -1), Act::StepVolumes(m.clone(), 1));
                stepper(app, ui, t("Volumes"), v, a, b, can);
                if side_by_side {
                    ui.add_space(14.0);
                }
            }
            let label = if m.kind == MediaKind::Anime { t("Episodes") } else { t("Chapters") };
            let v = format!("{} / {}", e.progress, of(e.media.total().or(m.total())));
            stepper(app, ui, label, v, Act::Step(m.clone(), -1), Act::Step(m.clone(), 1), can);
        };
        if side_by_side {
            ui.horizontal(steppers);
        } else {
            ui.vertical(steppers);
        }
        if let Some(f) = e.fraction() {
            ui.add_space(4.0);
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 5.0), Sense::hover());
            widgets::bar(ui.painter(), rect, f, fg, fg.gamma_multiply(0.3));
        }
        dates_line(app, ui, &e);
        if !e.notes.trim().is_empty() {
            ui.add_space(2.0);
            ui.add(egui::Label::new(RichText::new(e.notes.trim()).color(fg).italics()).wrap());
        }
        if e.status == ListStatus::Current {
            ui.add_space(6.0);
            let b = egui::Button::new(
                RichText::new(t("Start binge session")).font(theme::semibold(13.0)).color(fg),
            )
            .fill(fg.gamma_multiply(0.2));
            if ui.add(b).clicked() {
                app.act(Act::Gesture(GestureAction::Binge, m.clone()));
            }
        }
    });
}

fn dates_line(app: &App, ui: &mut Ui, e: &ListEntry) {
    let fg = egui::Color32::WHITE;
    let mut parts = Vec::new();
    if let Some(d) = e.start {
        parts.push(tf("Started {}", &[&format::date(d)]));
    }
    if let Some(d) = e.finish {
        parts.push(tf("Finished {}", &[&format::date(d)]));
    }
    if let (Some(a), Some(b)) = (e.start.and_then(|d| d.exact()), e.finish.and_then(|d| d.exact())) {
        parts.push(tf("{} days", &[&(b.duration_since(a).as_hours() / 24 + 1)]));
    }
    if e.repeat_count > 0 {
        parts.push(tf("Repeated {} times", &[&e.repeat_count]));
    }
    if app.s.show_updated && e.updated_at > 0 {
        parts.push(tf("Updated {}", &[&format::date_time(e.updated_at)]));
    }
    if !parts.is_empty() {
        ui.add_space(4.0);
        ui.add(egui::Label::new(RichText::new(parts.join("  ·  ")).size(12.0).color(fg)).wrap());
    }
}

fn overview_button(app: &mut App, ui: &mut Ui, m: &Rc<Media>, d: &MediaDetail) {
    use kuro_core::RelationKind as R;
    let p = app.p;
    let related = d.relations.iter().any(|r| {
        r.media.kind == m.kind && !matches!(r.kind, R::Other | R::Character | R::Adaptation | R::Source)
    });
    if !related {
        return;
    }
    ui.add_space(8.0);
    widgets::card(ui, &p, |ui| {
        if widgets::nav_row(ui, &p, icons::STACK, p.tint, t("Complete series overview"), "", false).clicked()
        {
            app.act(Act::Push(Route::Overview(m.media_ref())));
        }
    });
}

fn synopsis(app: &mut App, ui: &mut Ui, m: &Media) {
    let p = app.p;
    let Some(text) = &m.synopsis else { return };
    widgets::section(ui, &p, t("Synopsis"));
    let d = app.details.entry(m.media_ref()).or_default();
    widgets::card(ui, &p, |ui| {
        let long = text.chars().count() > 520;
        let shown = if long && !d.expanded {
            let cut: String = text.chars().take(460).collect();
            let cut = cut.rsplit_once(' ').map_or(cut.as_str(), |c| c.0).to_owned();
            format!("{cut}…")
        } else {
            text.clone()
        };
        ui.add(egui::Label::new(RichText::new(shown).line_height(Some(20.0))).wrap().selectable(true));
        if long {
            let label = if d.expanded { t("Show less") } else { t("Show more") };
            if ui.link(label).clicked() {
                d.expanded = !d.expanded;
            }
        }
    });
}

fn info(app: &mut App, ui: &mut Ui, d: &MediaDetail) {
    let p = app.p;
    let m = &d.media;
    let mut facts: Vec<(&str, String)> = Vec::new();
    let mut add = |k, v: Option<String>| {
        if let Some(v) = v.filter(|v| !v.is_empty()) {
            facts.push((k, v));
        }
    };
    add(t("Season"), m.season_year().filter(|_| m.kind == MediaKind::Anime).map(format::season_year));
    add(t("Broadcast"), m.broadcast.clone());
    add(t("Studios"), Some(m.studios.join(", ")));
    add(t("Source"), m.source.clone());
    add(t("Genres"), Some(m.genres.join(", ")));
    add(t("Tags"), Some(m.tags.iter().take(8).cloned().collect::<Vec<_>>().join(", ")));
    add(t("Duration"), m.duration.map(|n| tf("{} min per episode", &[&n])));
    add(t("Rating"), m.rating.clone());
    add(t("Languages"), Some(d.languages.join(", ")));
    let platforms = kuro_core::streaming_platforms(&d.links);
    if facts.is_empty() && platforms.is_empty() {
        return;
    }
    widgets::section(ui, &p, t("Information"));
    widgets::card(ui, &p, |ui| {
        for (i, (k, v)) in facts.iter().enumerate() {
            if i > 0 {
                widgets::separator(ui, &p);
            }
            widgets::fact(ui, &p, k, v);
        }
        if !platforms.is_empty() {
            if !facts.is_empty() {
                widgets::separator(ui, &p);
            }
            streaming(app, ui, &platforms);
        }
    });
}

/// A fold with the platforms that stream the title; open or closed for the whole session.
fn streaming(app: &mut App, ui: &mut Ui, platforms: &[kuro_core::ExternalLink]) {
    let p = app.p;
    let id = egui::Id::unique("detail_streaming_open");
    let mut open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
    widgets::label_a11y(&r, Role::Button, t("Streaming"));
    if r.clicked() {
        open = !open;
        ui.data_mut(|d| d.insert_temp(id, open));
    }
    let c = rect.center().y;
    let caret = if open { icons::CARET_DOWN } else { icons::CARET_RIGHT };
    let pt = ui.painter();
    pt.text(pos2(rect.right(), c), Align2::RIGHT_CENTER, caret, theme::icon(13.0), p.faint);
    let names = platforms.iter().map(|l| l.site.as_str()).collect::<Vec<_>>().join(", ");
    let summary = if open { String::new() } else { names };
    let v = widgets::galley(ui, &summary, theme::regular(13.0), p.dim, rect.width() * 0.65, 1);
    let end = rect.right() - 20.0 - v.size().x;
    let l = widgets::galley(ui, t("Streaming"), theme::regular(14.0), p.text, end - rect.left() - 8.0, 1);
    ui.painter().galley(pos2(end, c - v.size().y / 2.0), v, p.dim);
    ui.painter().galley(pos2(rect.left(), c - l.size().y / 2.0), l, p.text);
    widgets::focus_ring(ui, &p, &r, 8);
    r.on_hover_cursor(CursorIcon::PointingHand);
    if !open {
        return;
    }
    for l in platforms {
        let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
        widgets::label_a11y(&r, Role::Link, &l.site);
        if r.hovered() {
            ui.painter().rect_filled(rect.expand2(vec2(8.0, 0.0)), CornerRadius::same(8), p.hover);
        }
        let c = rect.center().y;
        // AniList's logos are white, drawn on the brand colour.
        let badge = Rect::from_center_size(pos2(rect.left() + 11.0, c), vec2(22.0, 22.0));
        let logo = l.icon.as_deref().and_then(|u| app.tex.get(&app.bg, u));
        match logo {
            Some(tex) => {
                let fill =
                    l.color.as_deref().and_then(|c| egui::Color32::from_hex(c).ok()).unwrap_or(p.raised);
                ui.painter().rect_filled(badge, CornerRadius::same(6), fill);
                egui::Image::from_texture(tex).paint_at(ui, badge.shrink(3.0));
            }
            None => {
                let pt = ui.painter();
                pt.text(
                    badge.center(),
                    Align2::CENTER_CENTER,
                    icons::MONITOR_PLAY,
                    theme::icon(16.0),
                    p.tint,
                );
            }
        }
        let pt = ui.painter();
        pt.text(
            pos2(rect.right(), c),
            Align2::RIGHT_CENTER,
            icons::ARROW_SQUARE_OUT,
            theme::icon(14.0),
            p.faint,
        );
        let name = widgets::galley(ui, &l.site, theme::regular(14.0), p.dim, rect.width() - 60.0, 1);
        ui.painter().galley(pos2(rect.left() + 30.0, c - name.size().y / 2.0), name, p.dim);
        widgets::focus_ring(ui, &p, &r, 8);
        if r.on_hover_text(&l.url).on_hover_cursor(CursorIcon::PointingHand).clicked() {
            app.act(Act::Url(l.url.clone()));
        }
    }
}

fn themes(app: &mut App, ui: &mut Ui, m: &Rc<Media>) {
    let p = app.p;
    let r = m.media_ref();
    let d = app.details.entry(r).or_default();
    if !d.themes_asked && !d.loading {
        d.themes_asked = true;
        app.bg.themes(m);
    }
    let themes = match app.details.get(&r).and_then(|d| d.themes.clone()) {
        Some(Ok(v)) if v.is_empty() => return,
        None => return,
        Some(x) => x,
    };
    widgets::section(ui, &p, t("Openings & endings"));
    widgets::card(ui, &p, |ui| match themes {
        Err(e) => {
            ui.horizontal_wrapped(|ui| {
                let text = t("Openings and endings are not available right now.");
                widgets::dim(ui, &p, text).on_hover_text(format::error(&e));
                if ui.link(t("Retry")).clicked() {
                    let d = app.details.entry(r).or_default();
                    d.themes = None;
                    d.themes_asked = false;
                }
            });
        }
        Ok(list) => {
            for (i, th) in list.iter().enumerate() {
                if i > 0 {
                    widgets::separator(ui, &p);
                }
                theme_row(app, ui, th);
            }
        }
    });
}

fn theme_row(app: &mut App, ui: &mut Ui, th: &Theme) {
    let p = app.p;
    let key = (th.title.clone(), th.artist.clone());
    ui.horizontal(|ui| {
        ui.set_min_height(54.0);
        let (rect, _) = ui.allocate_exact_size(vec2(46.0, 46.0), Sense::hover());
        // Only look up songs that are actually on screen.
        if app.s.music.apple() && ui.is_rect_visible(rect) && !app.tracks.contains_key(&key) {
            app.tracks.insert(key.clone(), None);
            app.bg.track(th.clone());
        }
        let track = app.tracks.get(&key).cloned().flatten();
        match track.as_ref().and_then(|x| x.artwork.as_deref()) {
            Some(a) => app.tex.paint(ui, &p, &app.bg, Some(a), rect, 7),
            None => {
                ui.painter().rect_filled(rect, CornerRadius::same(7), p.raised);
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    icons::MUSIC_NOTES,
                    theme::icon(20.0),
                    p.dim,
                );
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if app.s.music.apple() {
                let url = track.as_ref().map(|x| x.url.clone()).or_else(|| music::apple_search_url(th));
                if widgets::icon_button_colored(
                    ui,
                    &p,
                    icons::MUSIC_NOTES,
                    t("Play on Apple Music"),
                    p.red,
                    false,
                )
                .clicked()
                    && let Some(u) = url
                {
                    app.act(Act::Url(u));
                }
            }
            if app.s.music.spotify()
                && widgets::icon_button_colored(
                    ui,
                    &p,
                    icons::SPOTIFY_LOGO,
                    t("Find on Spotify"),
                    p.green,
                    false,
                )
                .clicked()
                && let Some(u) = music::spotify_search_url(th)
            {
                app.act(Act::Url(u));
            }
            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                let mut kind = match th.kind {
                    ThemeKind::Opening => t("Opening").to_uppercase(),
                    ThemeKind::Ending => t("Ending").to_uppercase(),
                };
                if let Some(e) = &th.episodes {
                    kind.push_str(&format!(" · {e}"));
                }
                ui.label(RichText::new(kind).font(theme::semibold(10.5)).color(p.tint));
                ui.add(egui::Label::new(RichText::new(&th.title).font(theme::semibold(14.0))).truncate());
                ui.add(egui::Label::new(RichText::new(&th.artist).size(12.5).color(p.dim)).truncate());
            });
        });
    });
}

fn titles(app: &mut App, ui: &mut Ui, m: &Media) {
    let p = app.p;
    let mut facts: Vec<(&str, String)> = Vec::new();
    if app.s.title_language != kuro_core::TitleLanguage::Romaji {
        facts.push((t("Romaji"), m.title.clone()));
    }
    for (k, v) in [(t("English"), &m.title_english), (t("Japanese"), &m.title_native)] {
        if let Some(v) = v {
            facts.push((k, v.clone()));
        }
    }
    if !m.synonyms.is_empty() {
        facts.push((t("Synonyms"), m.synonyms.join("\n")));
    }
    if facts.is_empty() {
        return;
    }
    widgets::section(ui, &p, t("Alternative titles"));
    widgets::card(ui, &p, |ui| {
        for (i, (k, v)) in facts.iter().enumerate() {
            if i > 0 {
                widgets::separator(ui, &p);
            }
            widgets::fact(ui, &p, k, v);
        }
    });
}

fn portrait(app: &mut App, ui: &mut Ui, image: Option<&str>, name: &str, sub: &str, clickable: bool) -> bool {
    let p = app.p;
    let (rect, r) =
        ui.allocate_exact_size(vec2(84.0, 160.0), if clickable { Sense::click() } else { Sense::hover() });
    if !ui.is_rect_visible(rect) {
        return false;
    }
    widgets::label_a11y(&r, if clickable { Role::Button } else { Role::Label }, &format!("{name}, {sub}"));
    let pic = Rect::from_min_size(rect.min, vec2(84.0, 112.0));
    app.tex.paint(ui, &p, &app.bg, image, pic, 8);
    if r.has_focus() {
        ui.painter().rect_stroke(pic, 8, egui::Stroke::new(2.0, p.tint), egui::StrokeKind::Outside);
    }
    let y = pic.bottom() + 4.0;
    let h = widgets::paint_text(ui, pos2(rect.left(), y), name, theme::semibold(11.5), p.text, 84.0, 2).y;
    widgets::paint_text(ui, pos2(rect.left(), y + h), sub, theme::regular(11.0), p.dim, 84.0, 1);
    if clickable { r.on_hover_cursor(CursorIcon::PointingHand).clicked() } else { false }
}

fn shelf(ui: &mut Ui, id: &str, add: impl FnOnce(&mut Ui)) {
    egui::ScrollArea::horizontal().id_salt(id).auto_shrink([false, true]).show(ui, |ui| {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            add(ui);
        });
        ui.add_space(8.0);
    });
}

fn cast(app: &mut App, ui: &mut Ui, d: &MediaDetail) {
    let p = app.p;
    if !d.characters.is_empty() {
        widgets::section(ui, &p, t("Characters & voice actors"));
        widgets::card(ui, &p, |ui| {
            shelf(ui, "cast", |ui| {
                for c in &d.characters {
                    portrait(app, ui, c.image.as_deref(), &c.name, &c.role, false);
                    let voice = c.voices.iter().find(|v| v.language.as_deref() == Some("Japanese"));
                    if let Some(v) = voice.or(c.voices.first()) {
                        let lang = v.language.clone().unwrap_or_default();
                        if portrait(app, ui, v.image.as_deref(), &v.name, &lang, true) {
                            app.act(Act::Push(Route::Person(v.id)));
                        }
                    }
                    ui.add_space(6.0);
                }
            });
        });
    }
    if !d.staff.is_empty() {
        widgets::section(ui, &p, t("Staff"));
        widgets::card(ui, &p, |ui| {
            shelf(ui, "staff", |ui| {
                for s in &d.staff {
                    portrait(app, ui, s.person.image.as_deref(), &s.person.name, &s.role, false);
                }
            });
        });
    }
}

fn shelves(app: &mut App, ui: &mut Ui, d: &MediaDetail) {
    let p = app.p;
    let adult = app.s.show_adult;
    let visible = |m: &Media| adult || !m.adult;
    if !d.relations.is_empty() {
        widgets::section(ui, &p, t("Related"));
        widgets::card(ui, &p, |ui| {
            shelf(ui, "related", |ui| {
                for r in d.relations.iter().filter(|r| visible(&r.media)) {
                    rows::shelf_tile(app, ui, &Rc::new(r.media.clone()), format::relation(r.kind));
                }
            });
        });
    }
    if app.s.show_recommendations && !d.recommendations.is_empty() {
        widgets::section(ui, &p, t("Recommendations"));
        widgets::card(ui, &p, |ui| {
            shelf(ui, "recs", |ui| {
                for m in d.recommendations.iter().filter(|m| visible(m)) {
                    rows::shelf_tile(app, ui, &Rc::new(m.clone()), "");
                }
            });
        });
    }
}

fn stats(app: &mut App, ui: &mut Ui, d: &MediaDetail) {
    let p = app.p;
    let kind = d.media.kind;
    let total: u64 = d.status_stats.iter().map(|s| s.1).sum();
    if total > 0 {
        widgets::section(ui, &p, t("Community"));
        widgets::card(ui, &p, |ui| {
            for s in ListStatus::ALL {
                let n: u64 = d.status_stats.iter().filter(|x| x.0 == s).map(|x| x.1).sum();
                ui.horizontal(|ui| {
                    ui.set_min_height(24.0);
                    ui.add_sized(
                        [120.0, 18.0],
                        egui::Label::new(format::status(s, kind)).halign(Align::LEFT),
                    );
                    let w = (ui.available_width() - 90.0).max(40.0);
                    let (rect, _) = ui.allocate_exact_size(vec2(w, 8.0), Sense::hover());
                    widgets::bar(ui.painter(), rect, n as f32 / total as f32, p.status(s), p.raised);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        widgets::dim(ui, &p, number(n))
                    });
                });
            }
        });
    }
    let max = d.score_stats.iter().map(|s| s.1).max().unwrap_or(0);
    if max > 0 {
        widgets::section(ui, &p, t("Score distribution"));
        widgets::card(ui, &p, |ui| {
            let w = ui.available_width();
            let (rect, _) = ui.allocate_exact_size(vec2(w, 112.0), Sense::hover());
            let n = d.score_stats.len() as f32;
            let step = w / n;
            for (i, (score, count)) in d.score_stats.iter().enumerate() {
                let h = 84.0 * (*count as f32 / max as f32);
                let cx = rect.left() + step * (i as f32 + 0.5);
                let half = (step * 0.34).min(22.0);
                let base = rect.bottom() - 20.0;
                let bar = Rect::from_min_max(pos2(cx - half, base - h.max(2.0)), pos2(cx + half, base));
                ui.painter().rect_filled(bar, CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 }, p.tint);
                ui.painter().text(
                    pos2(cx, rect.bottom() - 8.0),
                    Align2::CENTER_CENTER,
                    score / 10,
                    theme::regular(11.5),
                    p.dim,
                );
                let hover = Rect::from_min_max(
                    pos2(cx - step / 2.0, rect.top()),
                    pos2(cx + step / 2.0, rect.bottom()),
                );
                let r = ui.interact(hover, ui.scope_id().with(("score", i)), Sense::hover());
                let text = format!("{}: {}", score / 10, number(*count));
                widgets::label_a11y(&r, Role::Label, &text);
                r.on_hover_text(text);
            }
        });
    }
}

fn reviews(app: &mut App, ui: &mut Ui, r: MediaRef, d: &MediaDetail) {
    let p = app.p;
    if d.reviews.is_empty() {
        return;
    }
    widgets::section(ui, &p, t("Reviews"));
    widgets::card(ui, &p, |ui| {
        for (i, v) in d.reviews.iter().enumerate() {
            if i > 0 {
                widgets::separator(ui, &p);
            }
            let resp = ui
                .scope_builder(egui::UiBuilder::new().sense(Sense::click()).id_salt(v.id), |ui| {
                    ui.set_width(ui.available_width());
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&v.user).font(theme::semibold(13.5)));
                        widgets::dim(ui, &p, format::date_time(v.created_at));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            widgets::dim(ui, &p, format!("{} / {}", v.votes_up, v.votes));
                            ui.label(RichText::new(icons::THUMBS_UP).font(theme::icon(13.0)).color(p.dim));
                            if let Some(s) = v.score {
                                ui.label(RichText::new(s.to_string()).font(theme::bold(14.0)).color(p.tint));
                            }
                        });
                    });
                    ui.add(egui::Label::new(&v.summary).wrap());
                    ui.add_space(4.0);
                })
                .response;
            widgets::label_a11y(&resp, Role::Button, &tf("Review by {}", &[&v.user]));
            if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                app.act(Act::Push(Route::Review(r, v.id)));
            }
        }
    });
}

fn links(app: &mut App, ui: &mut Ui, d: &MediaDetail) {
    let p = app.p;
    // Streaming platforms live in Information.
    let others: Vec<_> = d.links.iter().filter(|l| !l.streaming).collect();
    if others.is_empty() && d.trailer.is_none() {
        return;
    }
    widgets::section(ui, &p, t("Links"));
    widgets::card(ui, &p, |ui| {
        ui.horizontal_wrapped(|ui| {
            if let Some(u) = &d.trailer
                && widgets::button(ui, &p, icons::PLAY, t("Trailer"), true).on_hover_text(u).clicked()
            {
                app.act(Act::Url(u.clone()));
            }
            for l in others {
                if widgets::button(ui, &p, icons::GLOBE, &l.site, false).on_hover_text(&l.url).clicked() {
                    app.act(Act::Url(l.url.clone()));
                }
            }
        });
    });
}
