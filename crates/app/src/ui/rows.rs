//! Title rows and poster tiles, with virtual scrolling, swipes and menus.

use std::rc::Rc;

use eframe::egui::{
    self, Align2, CornerRadius, CursorIcon, Rect, Response, Role, Sense, Stroke, Ui, pos2, vec2,
};
use mango_core::settings::{GestureAction, PosterProgress, ScoreDisplay};
use mango_core::{ListEntry, ListStatus, Media, MediaKind, ReleaseStatus};

use crate::app::{Act, App};
use crate::backend::now;
use crate::i18n::{decimal, number, t, tf};
use crate::theme;
use crate::widgets::{self, paint_text, pill};
use crate::{format, icons};

pub const ROW: f32 = 112.0;
const POSTER: egui::Vec2 = vec2(66.0, 94.0);

#[derive(Clone)]
pub struct Item {
    pub media: Rc<Media>,
    pub entry: Option<Rc<ListEntry>>,
    /// Position shown in rankings.
    pub rank: Option<usize>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Style {
    /// Personal list: progress and scores.
    Mine { show_status: bool },
    /// Discovery: format, dates and community numbers.
    Browse,
}

fn progress_text(e: &ListEntry) -> String {
    let total = e.media.total().map_or("?".into(), |n| n.to_string());
    format!("{} / {total}", e.progress)
}

fn volume_text(e: &ListEntry) -> String {
    let total = e.media.volumes.map_or("?".into(), |n| n.to_string());
    format!("{} / {total}", e.progress_volumes)
}

fn score_text(app: &App, e: &ListEntry) -> Option<String> {
    let mine = (e.score > 0).then(|| decimal(e.score10(), usize::from(!e.score.is_multiple_of(10))));
    let avg = e.media.mean_score.map(|s| decimal(s, 2));
    match app.s.score_display {
        ScoreDisplay::None => None,
        ScoreDisplay::Mine => mine,
        ScoreDisplay::Average => avg,
        ScoreDisplay::Both => match (mine, avg) {
            (Some(m), Some(a)) => Some(format!("{m}  ·  Ø {a}")),
            (Some(m), None) => Some(m),
            (None, Some(a)) => Some(format!("Ø {a}")),
            (None, None) => None,
        },
    }
}

/// `E10 · Thu 17:26`, when an episode is coming.
pub fn airing_text(m: &Media) -> Option<String> {
    let a = m.next_airing.filter(|a| a.at > now())?;
    Some(tf("Ep {} · {}", &[&a.episode, &format::airing_time(a.at, now())]))
}

fn subtitle(m: &Media) -> String {
    if let Some(a) = airing_text(m) {
        return a;
    }
    match (m.kind, m.release) {
        (MediaKind::Manga, ReleaseStatus::Releasing) => format::release(m.release, m.kind).into(),
        (MediaKind::Anime, _) => match m.season_year() {
            Some(s) => format::season_year(s),
            None => format::release(m.release, m.kind).into(),
        },
        _ => m.start.map(format::month).unwrap_or_else(|| format::release(m.release, m.kind).into()),
    }
}

/// Draws all `items` inside one rounded card, only laying out visible rows.
/// Returns true when the end is near, so more can be loaded.
pub fn list(
    app: &mut App,
    ui: &mut Ui,
    id: &str,
    items: &[Item],
    style: Style,
    footer: impl FnOnce(&mut App, &mut Ui),
) -> bool {
    let p = app.p;
    let total = items.len() as f32 * ROW;
    let out =
        egui::ScrollArea::vertical().id_salt(id).auto_shrink([false, false]).show_viewport(ui, |ui, view| {
            let w = ui.available_width() - 8.0;
            let origin = ui.max_rect().min;
            if !items.is_empty() {
                let card = Rect::from_min_size(origin, vec2(w, total));
                ui.painter().rect_filled(card, widgets::RADIUS, p.card);
            }
            let first = (view.min.y / ROW).floor().max(0.0) as usize;
            let last = ((view.max.y / ROW).ceil().max(0.0) as usize).min(items.len());
            for (i, item) in items.iter().enumerate().take(last).skip(first) {
                let rect = Rect::from_min_size(origin + vec2(0.0, i as f32 * ROW), vec2(w, ROW));
                let round = CornerRadius {
                    nw: if i == 0 { 12 } else { 0 },
                    ne: if i == 0 { 12 } else { 0 },
                    sw: if i + 1 == items.len() { 12 } else { 0 },
                    se: if i + 1 == items.len() { 12 } else { 0 },
                };
                row(app, ui, rect, round, item, style);
                if i + 1 < items.len() {
                    let y = rect.bottom();
                    ui.painter().hline(
                        rect.left() + POSTER.x + 24.0..=rect.right(),
                        y,
                        Stroke::new(1.0, p.line),
                    );
                }
            }
            ui.allocate_rect(Rect::from_min_size(origin, vec2(w, total)), Sense::hover());
            footer(app, ui);
            ui.add_space(16.0);
        });
    let end = out.content_size.y - out.state.clamped_offset().y - out.inner_rect.height();
    end < ROW * 6.0
}

fn swipe_color(app: &App, g: GestureAction) -> egui::Color32 {
    match g {
        GestureAction::Drop => app.p.red,
        GestureAction::Pause => app.p.orange,
        GestureAction::Complete => app.p.green,
        _ => app.p.tint,
    }
}

const SWIPE: f32 = 70.0;
const PILL: f32 = 108.0;

fn row(app: &mut App, ui: &mut Ui, rect: Rect, round: CornerRadius, item: &Item, style: Style) {
    let p = app.p;
    let m = &item.media;
    let id = ui.scope_id().with(("row", m.media_ref()));
    let r = ui.interact(rect, id, Sense::click_and_drag());
    let title = m.display_title(app.s.title_language).to_owned();
    let name = match &item.entry {
        Some(e) => format!("{title}, {}, {}", format::status(e.status, m.kind), progress_text(e)),
        None => title.clone(),
    };
    widgets::label_a11y(&r, Role::Button, &name);

    // Swipe: the row follows the pointer and reveals the action behind it.
    let mut dx = 0.0;
    if r.dragged() || r.drag_stopped() {
        // The distance is gone on the frame the button is released.
        let key = id.with("dx");
        dx = match r.total_drag_delta() {
            Some(d) => d.x,
            None => ui.data(|d| d.get_temp(key)).unwrap_or(0.0),
        };
        ui.data_mut(|d| d.insert_temp(key, dx));
        let g = if dx > 0.0 { app.s.gestures.swipe_right } else { app.s.gestures.swipe_left };
        if g == GestureAction::None {
            dx = 0.0;
        }
        dx = dx.clamp(-130.0, 130.0);
        if r.drag_stopped() {
            if dx.abs() >= SWIPE {
                app.act(Act::Gesture(g, m.clone()));
            }
            dx = 0.0;
        } else if dx.abs() > 4.0 {
            let on = dx.abs() >= SWIPE;
            let fill = theme::blend(p.card, swipe_color(app, g), if on { 0.9 } else { 0.45 });
            ui.painter().rect_filled(rect, round, fill);
            let x = if dx > 0.0 { rect.left() + 34.0 } else { rect.right() - 34.0 };
            let c = pos2(x, rect.center().y);
            ui.painter().text(
                c,
                Align2::CENTER_CENTER,
                format::gesture_icon(g),
                theme::icon(24.0),
                theme::on(fill),
            );
        }
    }
    let painter = ui.painter_at(rect);
    let shown = rect.translate(vec2(dx, 0.0));
    let open = super::is_open(app, m.media_ref());
    if dx != 0.0 || open || r.hovered() || r.has_focus() {
        painter.rect_filled(
            shown,
            round,
            if open {
                theme::blend(p.card, p.tint, 0.12)
            } else if dx != 0.0 {
                p.card
            } else {
                p.hover
            },
        );
    }
    if r.has_focus() {
        painter.rect_stroke(shown.shrink(1.0), round, Stroke::new(2.0, p.tint), egui::StrokeKind::Inside);
    }

    let poster = Rect::from_min_size(shown.min + vec2(10.0, 9.0), POSTER);
    app.tex.paint(ui, &p, &app.bg, m.cover.as_deref(), poster, 7);
    let x = poster.right() + 12.0;
    let hovered = ui.rect_contains_pointer(rect);
    let buttons = item.entry.is_some() || hovered;
    let right = shown.right() - if buttons { 46.0 } else { 14.0 };
    let width = right - x;
    let mut y = shown.top() + 9.0;

    match (style, &item.entry) {
        (Style::Mine { show_status }, Some(e)) => {
            if show_status {
                let s = format::status(e.status, m.kind).to_uppercase();
                y += paint_text(ui, pos2(x, y), &s, theme::semibold(10.5), p.status(e.status), width, 1).y;
            }
            let score = score_text(app, e);
            // Two title lines leave room for the score only beside the progress.
            let beside = score.as_ref().is_some_and(|s| {
                let w = widgets::galley(ui, s, theme::semibold(11.5), p.text, 999.0, 1).size().x + 30.0;
                PILL + 6.0 + w <= width
            });
            let rows =
                if show_status || m.kind == MediaKind::Manga || (score.is_some() && !beside) { 1 } else { 2 };
            y += paint_text(ui, pos2(x, y), &title, theme::semibold(14.0), p.text, width, rows).y;
            let sub = subtitle(m);
            paint_text(ui, pos2(x, y + 1.0), &sub, theme::regular(12.0), p.dim, width, 1);
            let pr = progress_pill(app, ui, e, pos2(x, shown.bottom() - 10.0), width);
            if let Some(s) = score {
                let top = pr.top() - 23.0;
                if top > y + 14.0 {
                    pill(&painter, pos2(x, top), icons::STAR, &s, p.yellow, p.on_yellow);
                } else if beside {
                    let at = pos2(pr.right() + 6.0, pr.center().y - 10.0);
                    pill(&painter, at, icons::STAR, &s, p.yellow, p.on_yellow);
                }
            }
        }
        _ => {
            if let Some(n) = item.rank {
                let s = format!("#{n}");
                y += paint_text(ui, pos2(x, y), &s, theme::semibold(11.5), p.tint, width, 1).y;
            }
            if let Some(e) = &item.entry {
                let s = format::status(e.status, m.kind).to_uppercase();
                y += paint_text(ui, pos2(x, y), &s, theme::semibold(10.5), p.status(e.status), width, 1).y;
            }
            let rows = if item.rank.is_some() || item.entry.is_some() { 1 } else { 2 };
            y += paint_text(ui, pos2(x, y), &title, theme::semibold(14.0), p.text, width, rows).y;
            if let Some(en) = m.title_english.as_ref().filter(|e| **e != title) {
                paint_text(ui, pos2(x, y), en, theme::regular(11.5), p.dim, width, 1);
            }
            let line = |ui: &Ui, bottom: f32, s: &str| {
                paint_text(ui, pos2(x, shown.bottom() - bottom), s, theme::regular(11.5), p.dim, width, 1);
            };
            line(ui, 46.0, &format!("{} ({})", format::format(m.format), format::length(m)));
            let when = airing_text(m).or_else(|| m.start.map(format::month));
            line(ui, 32.0, &when.unwrap_or_else(|| format::release(m.release, m.kind).into()));
            let score = m.mean_score.map_or("–".into(), |s| decimal(s, 2));
            let members = m.members.map_or("–".into(), number);
            let bottom = shown.bottom() - 17.0;
            painter.text(
                pos2(x, bottom + 7.0),
                Align2::LEFT_CENTER,
                icons::STAR,
                theme::icon_fill(11.0),
                p.yellow,
            );
            let s = paint_text(ui, pos2(x + 15.0, bottom), &score, theme::regular(11.5), p.dim, width, 1);
            let ux = x + 15.0 + s.x + 12.0;
            painter.text(pos2(ux, bottom + 7.0), Align2::LEFT_CENTER, icons::USERS, theme::icon(11.0), p.dim);
            paint_text(ui, pos2(ux + 15.0, bottom), &members, theme::regular(11.5), p.dim, width, 1);
        }
    }

    let mut hit_button = false;
    if dx == 0.0 {
        let mut spot = |ui: &Ui, n: usize, icon: &str, label: &str, strong: bool| {
            let c = pos2(rect.right() - 26.0, rect.top() + 26.0 + n as f32 * 32.0);
            let rr = Rect::from_center_size(c, vec2(28.0, 28.0));
            let b = ui.interact(rr, id.with(label), Sense::click());
            widgets::label_a11y(&b, Role::Button, &format!("{label}: {title}"));
            let bg = if b.hovered() { theme::blend(p.tint_soft(), p.tint, 0.25) } else { p.tint_soft() };
            if strong || b.hovered() || b.has_focus() {
                ui.painter().circle_filled(c, 13.5, bg);
            }
            ui.painter().text(c, Align2::CENTER_CENTER, icon, theme::icon(15.0), p.tint);
            let b = b.on_hover_text(label).on_hover_cursor(CursorIcon::PointingHand);
            hit_button |= b.hovered();
            b.clicked()
        };
        if let Some(e) = &item.entry {
            let can_add = e.status != ListStatus::Completed || e.repeating;
            if can_add && spot(ui, 0, icons::PLUS, t("Add one"), true) {
                app.act(Act::Step(m.clone(), 1));
            }
            if app.s.show_edit_button && spot(ui, 1, icons::PENCIL_SIMPLE, t("Edit"), false) {
                app.act(Act::Edit(m.clone()));
            }
            if hovered && e.progress > 0 && spot(ui, 2, icons::MINUS, t("Remove one"), false) {
                app.act(Act::Step(m.clone(), -1));
            }
        } else if hovered && spot(ui, 0, icons::PLUS, t("Add to list"), true) {
            app.act(Act::Edit(m.clone()));
        }
    }
    finish(app, &r, item, hit_button);
}

/// Clicks and the context menu shared by rows and tiles.
fn finish(app: &mut App, r: &Response, item: &Item, hit_button: bool) {
    let g = app.s.gestures;
    if r.middle_clicked() {
        app.act(Act::Gesture(g.middle_click, item.media.clone()));
    }
    if r.double_clicked() && g.double_click != GestureAction::None {
        app.act(Act::Gesture(g.double_click, item.media.clone()));
    } else if r.clicked() && !hit_button {
        app.act(Act::Open(item.media.clone()));
    }
    let r = if hit_button { r.clone() } else { r.clone().on_hover_cursor(CursorIcon::PointingHand) };
    r.context_menu(|ui| menu(app, ui, item));
}

pub fn menu(app: &mut App, ui: &mut Ui, item: &Item) {
    ui.set_min_width(210.0);
    let m = item.media.clone();
    let entry = |app: &mut App, ui: &mut Ui, text: &str, a: Act| {
        if ui.button(text).clicked() {
            app.act(a);
            ui.close();
        }
    };
    if let Some(e) = &item.entry {
        entry(app, ui, t("Add one"), Act::Step(m.clone(), 1));
        if m.kind == MediaKind::Manga {
            entry(app, ui, t("Add one volume"), Act::StepVolumes(m.clone(), 1));
        }
        entry(app, ui, t("Remove one"), Act::Step(m.clone(), -1));
        if e.status == ListStatus::Completed && !e.repeating {
            let label = if m.kind == MediaKind::Anime { t("Rewatch") } else { t("Reread") };
            entry(app, ui, label, Act::Repeat(m.clone()));
        }
        ui.separator();
    }
    ui.menu_button(t("Status"), |ui| {
        for s in ListStatus::ALL {
            let on = item.entry.as_ref().is_some_and(|e| e.status == s);
            if ui.selectable_label(on, format::status(s, m.kind)).clicked() {
                app.act(Act::SetStatus(m.clone(), s));
                ui.close();
            }
        }
    });
    entry(app, ui, if item.entry.is_some() { t("Edit…") } else { t("Add to list…") }, Act::Edit(m.clone()));
    if item.entry.as_ref().is_some_and(|e| e.status == ListStatus::Current) {
        entry(app, ui, t("Start binge session"), Act::Gesture(GestureAction::Binge, m.clone()));
    }
    ui.separator();
    if let Some(u) = m.web_url(m.service) {
        entry(app, ui, t("Open in browser"), Act::Url(u.clone()));
        entry(app, ui, t("Copy link"), Act::Copy(u));
    }
    if item.entry.is_some() {
        ui.separator();
        entry(app, ui, t("Remove from list…"), Act::AskDelete(m));
    }
}

fn status_fill(app: &App, s: ListStatus) -> egui::Color32 {
    if s == ListStatus::Current { app.p.fill } else { theme::for_white_text(app.p.status(s)) }
}

/// The tinted capsule with progress. `bottom_left` is its lower left corner.
fn progress_pill(app: &App, ui: &Ui, e: &ListEntry, bottom_left: egui::Pos2, width: f32) -> Rect {
    let manga = e.media.kind == MediaKind::Manga;
    let fill = status_fill(app, e.status);
    let fg = egui::Color32::WHITE;
    let w = PILL.min(width);
    let h = if manga { 42.0 } else { 27.0 };
    let rect = Rect::from_min_size(bottom_left - vec2(0.0, h), vec2(w, h));
    let pt = ui.painter();
    pt.rect_filled(rect, CornerRadius::same(7), fill);
    let line = |y: f32, icon: &str, text: &str| {
        pt.text(pos2(rect.left() + 14.0, y), Align2::CENTER_CENTER, icon, theme::icon_fill(13.0), fg);
        pt.text(pos2(rect.left() + 26.0, y), Align2::LEFT_CENTER, text, theme::semibold(11.5), fg);
    };
    if manga {
        line(rect.top() + 11.0, icons::BOOK, &volume_text(e));
        line(rect.top() + 26.0, icons::BOOKMARK_SIMPLE, &progress_text(e));
    } else {
        line(rect.top() + 11.0, icons::STACK, &progress_text(e));
    }
    if e.repeating {
        pt.text(
            pos2(rect.right() - 12.0, rect.top() + 11.0),
            Align2::CENTER_CENTER,
            icons::REPEAT,
            theme::icon(13.0),
            fg,
        );
    }
    let b = Rect::from_min_size(pos2(rect.left() + 8.0, rect.bottom() - 6.5), vec2(w - 16.0, 2.5));
    widgets::bar(pt, b, e.fraction().unwrap_or(0.0), fg, fg.gamma_multiply(0.3));
    rect
}

/// Poster tiles. Returns true when the end is near.
pub fn grid(
    app: &mut App,
    ui: &mut Ui,
    id: &str,
    items: &[Item],
    mine: bool,
    footer: impl FnOnce(&mut App, &mut Ui),
) -> bool {
    let gap = 10.0;
    let avail = ui.available_width() - 8.0;
    let cols = ((avail + gap) / (app.s.poster_width + gap)).floor().max(1.0);
    let w = (avail - gap * (cols - 1.0)) / cols;
    let h = w * 1.45 + 38.0;
    let step = h + gap;
    let cols = cols as usize;
    let rows = items.len().div_ceil(cols);
    let out =
        egui::ScrollArea::vertical().id_salt(id).auto_shrink([false, false]).show_viewport(ui, |ui, view| {
            let origin = ui.max_rect().min;
            let first = (view.min.y / step).floor().max(0.0) as usize;
            let last = ((view.max.y / step).ceil().max(0.0) as usize).min(rows);
            for (i, item) in items.iter().enumerate().take(last * cols).skip(first * cols) {
                let at = origin + vec2((i % cols) as f32 * (w + gap), (i / cols) as f32 * step);
                tile(app, ui, Rect::from_min_size(at, vec2(w, h)), item, mine);
            }
            ui.allocate_rect(Rect::from_min_size(origin, vec2(avail, rows as f32 * step)), Sense::hover());
            footer(app, ui);
            ui.add_space(16.0);
        });
    out.content_size.y - out.state.clamped_offset().y - out.inner_rect.height() < step * 2.0
}

fn tile(app: &mut App, ui: &mut Ui, rect: Rect, item: &Item, mine: bool) {
    let p = app.p;
    let m = &item.media;
    let id = ui.scope_id().with(("tile", m.media_ref()));
    let r = ui.interact(rect, id, Sense::click());
    let title = m.display_title(app.s.title_language).to_owned();
    widgets::label_a11y(&r, Role::Button, &title);
    let poster = Rect::from_min_size(rect.min, vec2(rect.width(), rect.width() * 1.45));
    app.tex.paint(ui, &p, &app.bg, m.cover.as_deref(), poster, 10);
    let pt = ui.painter().clone();
    if super::is_open(app, m.media_ref()) || r.has_focus() {
        let s = Stroke::new(3.0, p.tint);
        pt.rect_stroke(poster, CornerRadius::same(10), s, egui::StrokeKind::Outside);
    }
    if let Some(e) = &item.entry {
        let style = app.s.poster_progress;
        let color = status_fill(app, e.status);
        let bottom = poster.bottom() - 8.0;
        let with_bar = matches!(style, PosterProgress::Bar | PosterProgress::Both);
        if with_bar {
            let b = Rect::from_min_size(
                pos2(poster.left() + 8.0, bottom - 5.0),
                vec2(poster.width() - 16.0, 5.0),
            );
            pt.rect_filled(b.expand(1.5), CornerRadius::same(4), egui::Color32::from_black_alpha(150));
            let track = egui::Color32::from_white_alpha(90);
            widgets::bar(&pt, b, e.fraction().unwrap_or(0.0), color, track);
        }
        if matches!(style, PosterProgress::Badge | PosterProgress::Both) {
            let y = bottom - if with_bar { 32.0 } else { 20.0 };
            let text = if mine { progress_text(e) } else { format::status(e.status, m.kind).to_owned() };
            pill(
                &pt,
                pos2(poster.left() + 8.0, y),
                format::status_icon(e.status),
                &text,
                color,
                egui::Color32::WHITE,
            );
        }
    }
    if app.s.poster_scores {
        let own = item.entry.as_ref().filter(|e| e.score > 0 && app.s.score_display != ScoreDisplay::Average);
        let s = own.map(|e| decimal(e.score10(), usize::from(!e.score.is_multiple_of(10))));
        if let Some(s) = s.or_else(|| m.mean_score.map(|s| decimal(s, 2))) {
            pill(&pt, poster.min + vec2(8.0, 8.0), icons::STAR, &s, p.yellow, p.on_yellow);
        }
    }
    if let Some(n) = item.rank {
        let g = format!("#{n}");
        pill(
            &pt,
            pos2(poster.right() - 14.0 - 9.0 * g.len() as f32, poster.top() + 8.0),
            "",
            &g,
            p.card,
            p.text,
        );
    }
    let at = pos2(poster.left() + 2.0, poster.bottom() + 5.0);
    paint_text(ui, at, &title, theme::semibold(12.0), p.text, poster.width() - 4.0, 2);

    let mut hit = false;
    let can_add = item.entry.as_ref().is_none_or(|e| e.status != ListStatus::Completed || e.repeating);
    if ui.rect_contains_pointer(rect) && can_add {
        let c = pos2(poster.right() - 22.0, poster.bottom() - 26.0);
        let b = ui.interact(Rect::from_center_size(c, vec2(32.0, 32.0)), id.with("add"), Sense::click());
        let label = if item.entry.is_some() { t("Add one") } else { t("Add to list") };
        widgets::label_a11y(&b, Role::Button, &format!("{label}: {title}"));
        pt.circle_filled(c, 15.0, p.fill);
        pt.text(c, Align2::CENTER_CENTER, icons::PLUS, theme::icon(17.0), egui::Color32::WHITE);
        let b = b.on_hover_text(label);
        hit = b.hovered();
        if b.clicked() {
            let a = if item.entry.is_some() { Act::Step(m.clone(), 1) } else { Act::Edit(m.clone()) };
            app.act(a);
        }
    }
    finish(app, &r, item, hit);
}

/// A small poster with a caption, for horizontal shelves.
pub fn shelf_tile(app: &mut App, ui: &mut Ui, m: &Rc<Media>, caption: &str) {
    let p = app.p;
    let (rect, r) = ui.allocate_exact_size(vec2(96.0, 190.0), Sense::click());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let title = m.display_title(app.s.title_language).to_owned();
    widgets::label_a11y(&r, Role::Button, &format!("{caption} {title}"));
    let poster = Rect::from_min_size(rect.min, vec2(96.0, 136.0));
    app.tex.paint(ui, &p, &app.bg, m.cover.as_deref(), poster, 8);
    let entry = app.entry(m);
    if let Some(e) = &entry {
        let c = p.status(e.status);
        ui.painter().circle_filled(poster.right_top() + vec2(-12.0, 12.0), 9.0, c);
        ui.painter().text(
            poster.right_top() + vec2(-12.0, 12.0),
            Align2::CENTER_CENTER,
            format::status_icon(e.status),
            theme::icon_fill(11.0),
            theme::on(c),
        );
    }
    let mut y = poster.bottom() + 4.0;
    if !caption.is_empty() {
        y += paint_text(ui, pos2(rect.left(), y), caption, theme::semibold(10.5), p.tint, 96.0, 1).y;
    }
    paint_text(ui, pos2(rect.left(), y), &title, theme::regular(11.5), p.text, 96.0, 2);
    let item = Item { media: m.clone(), entry, rank: None };
    finish(app, &r, &item, false);
}
