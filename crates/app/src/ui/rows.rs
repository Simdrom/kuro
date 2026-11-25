//! Titles as cards, posters or table rows, in a grid that follows its width.

use std::rc::Rc;

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, CursorIcon, Layout, Rect, Response, Role, Sense, Stroke,
    StrokeKind, Ui, WidgetInfo, pos2, vec2,
};
use kuro_core::list::SortKey;
use kuro_core::settings::{GestureAction, ListLayout, PosterProgress, ScoreDisplay};
use kuro_core::{ListEntry, ListStatus, Media, MediaKind, ReleaseStatus};

use crate::app::{Act, App};
use crate::backend::now;
use crate::i18n::{decimal, number, t, tf};
use crate::theme;
use crate::widgets::{self, paint_text, pill};
use crate::{format, icons};

/// Room kept free on the right for the scroll bar.
const BAR: f32 = 12.0;
const GAP: f32 = 12.0;
const WHITE: Color32 = Color32::WHITE;

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
    /// Discovery: format, dates, community numbers and synopsis.
    Browse,
}

// Text

fn progress_text(e: &ListEntry) -> String {
    let total = e.media.total().map_or("?".into(), |n| n.to_string());
    format!("{} / {total}", e.progress)
}

fn volume_text(e: &ListEntry) -> String {
    let total = e.media.volumes.map_or("?".into(), |n| n.to_string());
    format!("{} / {total}", e.progress_volumes)
}

fn my_score(e: &ListEntry) -> Option<String> {
    (e.score > 0).then(|| decimal(e.score10(), usize::from(!e.score.is_multiple_of(10))))
}

fn score_text(app: &App, e: &ListEntry) -> Option<String> {
    let mine = my_score(e);
    let avg = e.media.mean_score.map(|s| decimal(s, 2));
    match app.s.score_display {
        ScoreDisplay::None => None,
        ScoreDisplay::Mine => mine,
        ScoreDisplay::Average => avg,
        ScoreDisplay::Both => match (mine, avg) {
            (Some(m), Some(a)) => Some(format!("{m} · Ø {a}")),
            (Some(m), None) => Some(m),
            (None, Some(a)) => Some(format!("Ø {a}")),
            (None, None) => None,
        },
    }
}

/// `Ep 10 · Thu 17:26`, when an episode is coming.
pub fn airing_text(m: &Media) -> Option<String> {
    let a = m.next_airing.filter(|a| a.at > now())?;
    Some(tf("Ep {} · {}", &[&a.episode, &format::airing_time(a.at, now())]))
}

/// Released episodes not watched yet.
pub fn behind(e: &ListEntry) -> Option<u32> {
    if e.media.kind != MediaKind::Anime || e.status != ListStatus::Current {
        return None;
    }
    let released = e.media.next_airing?.episode.saturating_sub(1);
    Some(released.saturating_sub(e.progress)).filter(|n| *n > 0)
}

fn when(m: &Media) -> String {
    match (m.kind, m.release) {
        (MediaKind::Manga, ReleaseStatus::Releasing) => format::release(m.release, m.kind).into(),
        (MediaKind::Anime, _) => match m.season_year() {
            Some(s) => format::season_year(s),
            None => format::release(m.release, m.kind).into(),
        },
        _ => m.start.map(format::month).unwrap_or_else(|| format::release(m.release, m.kind).into()),
    }
}

fn can_add(e: &ListEntry) -> bool {
    e.status != ListStatus::Completed || e.repeating
}

/// What the main button of a title does next.
fn next_label(e: &ListEntry) -> String {
    match e.media.kind {
        MediaKind::Anime => tf("Ep {}", &[&(e.progress + 1)]),
        MediaKind::Manga => tf("Ch {}", &[&(e.progress + 1)]),
    }
}

fn status_fill(app: &App, s: ListStatus) -> Color32 {
    if s == ListStatus::Current { app.p.fill } else { theme::for_white_text(app.p.status(s)) }
}

fn a11y_name(app: &App, item: &Item) -> String {
    let title = item.media.display_title(app.s.title_language);
    match &item.entry {
        Some(e) => format!("{title}, {}, {}", format::status(e.status, item.media.kind), progress_text(e)),
        None => title.to_owned(),
    }
}

// Grid

/// Columns of at least `min` points that fill `avail`, and their width.
pub fn columns(avail: f32, min: f32, gap: f32) -> (usize, f32) {
    let n = ((avail + gap) / (min + gap)).floor().max(1.0);
    (n as usize, ((avail - gap * (n - 1.0)) / n).max(1.0))
}

#[derive(Clone, Copy)]
struct Anchor {
    cols: usize,
    step: f32,
    first: usize,
}

/// A virtual grid: only visible cells are laid out, and the first visible
/// title stays in place when the number of columns changes.
struct Grid<'a> {
    id: &'a str,
    count: usize,
    min: f32,
    gap: f32,
}

impl Grid<'_> {
    fn show(
        self,
        app: &mut App,
        ui: &mut Ui,
        height: impl Fn(f32) -> f32,
        mut cell: impl FnMut(&mut App, &mut Ui, Rect, usize),
        footer: impl FnOnce(&mut App, &mut Ui),
    ) -> bool {
        let avail = (ui.available_width() - BAR).max(1.0);
        let (cols, w) = columns(avail, self.min, self.gap);
        let h = height(w);
        let step = h + self.gap;
        let rows = self.count.div_ceil(cols);
        let key = egui::Id::unique(("grid-anchor", self.id));
        let mut area = egui::ScrollArea::vertical().id_salt(self.id).auto_shrink([false, false]);
        if let Some(a) = ui.data(|d| d.get_temp::<Anchor>(key))
            && (a.cols != cols || (a.step - step).abs() > 0.5)
        {
            area = area.vertical_scroll_offset((a.first / cols) as f32 * step);
        }
        let out = area.show_viewport(ui, |ui, view| {
            let origin = ui.max_rect().min;
            let first = (view.min.y / step).floor().max(0.0) as usize;
            let last = ((view.max.y / step).ceil().max(0.0) as usize).min(rows);
            for i in (first * cols).min(self.count)..(last * cols).min(self.count) {
                let at = origin + vec2((i % cols) as f32 * (w + self.gap), (i / cols) as f32 * step);
                cell(app, ui, Rect::from_min_size(at, vec2(w, h)), i);
            }
            let body = Rect::from_min_size(origin, vec2(avail, rows as f32 * step));
            ui.allocate_rect(body, Sense::hover());
            footer(app, ui);
            ui.add_space(16.0);
        });
        let offset = out.state.clamped_offset().y;
        let anchor =
            Anchor { cols, step, first: ((offset + self.gap) / step).floor().max(0.0) as usize * cols };
        ui.data_mut(|d| d.insert_temp(key, anchor));
        out.content_size.y - offset - out.inner_rect.height() < out.inner_rect.height().max(step * 2.0)
    }
}

/// Draws `items` in the chosen layout. Returns true when the end is near, so
/// more can be loaded.
pub fn show(
    app: &mut App,
    ui: &mut Ui,
    id: &str,
    items: &[Item],
    layout: ListLayout,
    style: Style,
    footer: impl FnOnce(&mut App, &mut Ui),
) -> bool {
    let count = items.len();
    match (layout, style) {
        (ListLayout::Posters, _) => Grid { id, count, min: app.s.poster_width, gap: GAP }.show(
            app,
            ui,
            |w| w * 1.45 + 40.0,
            |app, ui, rect, i| tile(app, ui, rect, &items[i], style),
            footer,
        ),
        (ListLayout::Cards, Style::Mine { show_status }) => Grid { id, count, min: 310.0, gap: GAP }.show(
            app,
            ui,
            |_| 164.0,
            |app, ui, rect, i| card(app, ui, rect, &items[i], show_status),
            footer,
        ),
        (ListLayout::Cards, Style::Browse) => Grid { id, count, min: 320.0, gap: GAP }.show(
            app,
            ui,
            |w| if w < 360.0 { 190.0 } else { 204.0 },
            |app, ui, rect, i| browse_card(app, ui, rect, &items[i]),
            footer,
        ),
        (ListLayout::Table, _) => {
            let avail = (ui.available_width() - BAR).max(1.0);
            let ranked = items.first().is_some_and(|i| i.rank.is_some());
            let kind = items.first().map_or(MediaKind::Anime, |i| i.media.kind);
            let cols = table_columns(avail, style, kind, ranked);
            table_header(app, ui, avail, &cols, matches!(style, Style::Mine { .. }));
            Grid { id, count, min: f32::INFINITY, gap: 0.0 }.show(
                app,
                ui,
                |_| TABLE_ROW,
                |app, ui, rect, i| table_row(app, ui, rect, &items[i], &cols, i),
                footer,
            )
        }
    }
}

// Interaction shared by cards and table rows

fn swipe_color(app: &App, g: GestureAction) -> Color32 {
    match g {
        GestureAction::Drop => app.p.red,
        GestureAction::Pause => app.p.orange,
        GestureAction::Complete => app.p.green,
        _ => app.p.tint,
    }
}

const SWIPE: f32 = 70.0;

/// Handles dragging sideways. Returns how far the content is moved.
fn swipe(app: &mut App, ui: &Ui, rect: Rect, round: CornerRadius, r: &Response, m: &Rc<Media>) -> f32 {
    if !r.dragged() && !r.drag_stopped() {
        return 0.0;
    }
    // The distance is gone on the frame the button is released.
    let key = r.id.with("dx");
    let dx = match r.total_drag_delta() {
        Some(d) => d.x,
        None => ui.data(|d| d.get_temp(key)).unwrap_or(0.0),
    };
    ui.data_mut(|d| d.insert_temp(key, dx));
    let g = if dx > 0.0 { app.s.gestures.swipe_right } else { app.s.gestures.swipe_left };
    if g == GestureAction::None {
        return 0.0;
    }
    if r.drag_stopped() {
        if dx.abs() >= SWIPE {
            app.act(Act::Gesture(g, m.clone()));
        }
        return 0.0;
    }
    let dx = dx.clamp(-130.0, 130.0);
    if dx.abs() > 4.0 {
        let on = dx.abs() >= SWIPE;
        let fill = theme::blend(app.p.card, swipe_color(app, g), if on { 0.9 } else { 0.45 });
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
    dx
}

/// Clicks and the context menu shared by every presentation.
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

/// A round icon button at a fixed place. Returns (clicked, hovered).
fn spot(
    ui: &Ui,
    p: &theme::Palette,
    id: egui::Id,
    c: egui::Pos2,
    icon: &str,
    label: &str,
    title: &str,
) -> (bool, bool) {
    let rect = Rect::from_center_size(c, vec2(30.0, 30.0));
    let b = ui.interact(rect, id.with(icon), Sense::click());
    widgets::label_a11y(&b, Role::Button, &format!("{label}: {title}"));
    if b.hovered() || b.has_focus() {
        ui.painter().circle_filled(c, 14.5, theme::blend(p.tint_soft(), p.tint, 0.2));
    }
    ui.painter().text(c, Align2::CENTER_CENTER, icon, theme::icon(16.0), p.tint);
    let b = b.on_hover_text(label).on_hover_cursor(CursorIcon::PointingHand);
    (b.clicked(), b.hovered())
}

/// A filled button with text at a fixed place, as wide as its text.
#[allow(clippy::too_many_arguments)]
fn solid(
    ui: &Ui,
    p: &theme::Palette,
    id: egui::Id,
    at: egui::Pos2,
    max: f32,
    icon: &str,
    text: &str,
    name: &str,
    strong: bool,
) -> (bool, bool, Rect) {
    let fg = if strong { WHITE } else { p.tint };
    let g = widgets::galley(ui, text, theme::semibold(12.5), fg, (max - 40.0).max(1.0), 1);
    let rect = Rect::from_min_size(at, vec2((g.size().x + 38.0).min(max), 30.0));
    let b = ui.interact(rect, id.with("solid"), Sense::click());
    widgets::label_a11y(&b, Role::Button, name);
    let base = if strong { p.fill } else { p.tint_soft() };
    let bg = if b.hovered() {
        theme::blend(base, if strong { Color32::BLACK } else { p.text }, 0.12)
    } else {
        base
    };
    ui.painter().rect_filled(rect, CornerRadius::same(8), bg);
    if b.has_focus() {
        let s = Stroke::new(2.0, p.text);
        ui.painter().rect_stroke(rect, CornerRadius::same(8), s, StrokeKind::Outside);
    }
    let c = pos2(rect.left() + 17.0, rect.center().y);
    ui.painter().text(c, Align2::CENTER_CENTER, icon, theme::icon(15.0), fg);
    ui.painter().galley(pos2(rect.left() + 28.0, rect.center().y - g.size().y / 2.0), g, fg);
    let b = b.on_hover_cursor(CursorIcon::PointingHand);
    (b.clicked(), b.hovered(), rect)
}

/// Background, border and cover that cards share.
fn card_base(app: &mut App, ui: &Ui, rect: Rect, shown: Rect, r: &Response, m: &Media, cover: f32) -> Rect {
    let p = app.p;
    let round = widgets::RADIUS;
    let open = super::is_open(app, m.media_ref());
    let fill = if open {
        theme::blend(p.card, p.tint, 0.14)
    } else if r.hovered() || ui.rect_contains_pointer(rect) {
        p.hover
    } else {
        p.card
    };
    let pt = ui.painter_at(rect);
    pt.rect_filled(shown, round, fill);
    let pic = Rect::from_min_size(shown.min, vec2(cover, shown.height()));
    let corner = CornerRadius { nw: 12, sw: 12, ne: 0, se: 0 };
    app.tex.paint_rounded(ui, &p, &app.bg, m.cover.as_deref(), pic, corner);
    if open || r.has_focus() {
        let s = Stroke::new(2.0, p.tint);
        ui.painter().rect_stroke(shown.shrink(1.0), round, s, StrokeKind::Inside);
    }
    pic
}

// Cards

fn card(app: &mut App, ui: &mut Ui, rect: Rect, item: &Item, show_status: bool) {
    let p = app.p;
    let m = &item.media;
    let Some(e) = item.entry.clone() else { return browse_card(app, ui, rect, item) };
    let id = ui.scope_id().with(("card", m.media_ref()));
    let r = ui.interact(rect, id, Sense::click_and_drag());
    let name = a11y_name(app, item);
    widgets::label_a11y(&r, Role::Button, &name);
    let dx = swipe(app, ui, rect, widgets::RADIUS, &r, m);
    let shown = rect.translate(vec2(dx, 0.0));
    let cover = (rect.width() * 0.32).clamp(84.0, 112.0);
    let pic = card_base(app, ui, rect, shown, &r, m, cover);
    let title = m.display_title(app.s.title_language).to_owned();
    let fill = status_fill(app, e.status);
    if let Some(s) = score_text(app, &e) {
        pill(&ui.painter_at(rect), pic.min + vec2(7.0, 7.0), icons::STAR, &s, p.yellow, p.on_yellow);
    }

    let x = pic.right() + 14.0;
    let right = shown.right() - 14.0;
    let w = (right - x).max(1.0);
    let mut y = shown.top() + 11.0;
    if show_status {
        let s = format::status(e.status, m.kind).to_uppercase();
        y += paint_text(ui, pos2(x, y), &s, theme::semibold(10.5), p.status(e.status), w, 1).y;
    }
    y += paint_text(ui, pos2(x, y), &title, theme::semibold(14.5), p.text, w, 2).y + 2.0;
    let airing = airing_text(m);
    let (sub, color) = match &airing {
        Some(a) => (a.clone(), p.tint),
        None => (format!("{} · {}", when(m), format::format(m.format)), p.dim),
    };
    y += paint_text(ui, pos2(x, y), &sub, theme::regular(12.0), color, w, 1).y + 1.0;

    let buttons = shown.bottom() - 40.0;
    let bar_y = buttons - 13.0;
    let line = bar_y - 19.0;
    if y + 16.0 <= line && !m.genres.is_empty() {
        paint_text(ui, pos2(x, y), &m.genres.join(", "), theme::regular(11.5), p.faint, w, 1);
    }
    // Progress: count on the left, what is left to see on the right.
    let count = if m.kind == MediaKind::Manga {
        format!("{}   {}", tf("Vol {}", &[&volume_text(&e)]), tf("Ch {}", &[&progress_text(&e)]))
    } else {
        progress_text(&e)
    };
    let note = if e.repeating {
        Some((t("Repeat").to_owned(), p.dim))
    } else {
        behind(&e).map(|n| (tf("{} behind", &[&n]), p.orange))
    };
    let mut room = w;
    if let Some((text, color)) = note {
        let g = widgets::galley(ui, &text, theme::semibold(11.5), color, w * 0.5, 1);
        room -= g.size().x + 8.0;
        ui.painter().galley(pos2(right - g.size().x, line + 1.0), g, color);
    }
    paint_text(ui, pos2(x, line), &count, theme::semibold(12.5), p.text, room, 1);
    let b = Rect::from_min_size(pos2(x, bar_y), vec2(w, 5.0));
    let color = if e.status == ListStatus::Current { p.tint } else { fill };
    widgets::bar(ui.painter(), b, e.fraction().unwrap_or(0.0), color, p.raised);

    let mut hit = false;
    if dx == 0.0 {
        let edit = app.s.show_edit_button;
        let cy = buttons + 15.0;
        let mut rx = right - 13.0;
        if edit {
            let (c, h) = spot(ui, &p, id, pos2(rx, cy), icons::PENCIL_SIMPLE, t("Edit"), &title);
            hit |= h;
            if c {
                app.act(Act::Edit(m.clone()));
            }
            rx -= 34.0;
        }
        if e.progress > 0 {
            let (c, h) = spot(ui, &p, id, pos2(rx, cy), icons::MINUS, t("Remove one"), &title);
            hit |= h;
            if c {
                app.act(Act::Step(m.clone(), -1));
            }
            rx -= 34.0;
        }
        let room = (rx + 13.0 - x).max(40.0);
        if can_add(&e) {
            let label = format!("{}: {title}", t("Add one"));
            let strong = e.status == ListStatus::Current;
            let (c, h, _) =
                solid(ui, &p, id, pos2(x, buttons), room, icons::PLUS, &next_label(&e), &label, strong);
            hit |= h;
            if c {
                app.act(Act::Step(m.clone(), 1));
            }
        } else {
            let text = format::status(e.status, m.kind);
            let pt = ui.painter();
            let font = theme::icon_fill(14.0);
            pt.text(
                pos2(x + 8.0, cy),
                Align2::CENTER_CENTER,
                format::status_icon(e.status),
                font,
                p.status(e.status),
            );
            paint_text(
                ui,
                pos2(x + 20.0, cy - 8.0),
                text,
                theme::semibold(12.5),
                p.status(e.status),
                room - 20.0,
                1,
            );
        }
    }
    finish(app, &r, item, hit);
}

fn browse_card(app: &mut App, ui: &mut Ui, rect: Rect, item: &Item) {
    let p = app.p;
    let m = &item.media;
    let id = ui.scope_id().with(("card", m.media_ref()));
    let r = ui.interact(rect, id, Sense::click());
    let name = a11y_name(app, item);
    widgets::label_a11y(&r, Role::Button, &name);
    let cover = (rect.width() * 0.33).clamp(92.0, 140.0);
    let pic = card_base(app, ui, rect, rect, &r, m, cover);
    let title = m.display_title(app.s.title_language).to_owned();
    if let Some(n) = item.rank {
        pill(ui.painter(), pic.min + vec2(7.0, 7.0), "", &format!("#{n}"), p.fill, WHITE);
    }
    let x = pic.right() + 14.0;
    let w = (rect.right() - 14.0 - x).max(1.0);
    let mut y = rect.top() + 11.0;
    y += paint_text(ui, pos2(x, y), &title, theme::semibold(14.5), p.text, w, 2).y + 1.0;
    let meta = format!("{} · {}", format::format(m.format), format::length(m));
    y += paint_text(ui, pos2(x, y), &meta, theme::regular(12.0), p.dim, w, 1).y;
    let airing = airing_text(m);
    let color = if airing.is_some() { p.tint } else { p.dim };
    let date = airing
        .or_else(|| m.start.map(format::month))
        .unwrap_or_else(|| format::release(m.release, m.kind).into());
    y += paint_text(ui, pos2(x, y), &date, theme::regular(12.0), color, w, 1).y + 3.0;

    let bottom = rect.bottom() - 40.0;
    // Community numbers sit on the button line, so the text above can grow.
    let score = m.mean_score.map_or("–".into(), |s| decimal(s, 2));
    let members = m.members.map_or("–".into(), number);
    let cy = bottom + 15.0;
    let g1 = widgets::galley(ui, &score, theme::semibold(12.5), p.text, 999.0, 1);
    let g2 = widgets::galley(ui, &members, theme::regular(12.0), p.dim, 999.0, 1);
    let mut rx = rect.right() - 14.0;
    let both = g1.size().x + g2.size().x + 54.0;
    let main = item.entry.as_ref().map_or(t("Add"), |e| format::status(e.status, m.kind));
    let main = widgets::galley(ui, main, theme::semibold(12.5), p.text, 999.0, 1).size().x + 42.0;
    if w - both > main {
        rx -= g2.size().x;
        ui.painter().galley(pos2(rx, cy - g2.size().y / 2.0), g2, p.dim);
        ui.painter().text(pos2(rx - 11.0, cy), Align2::CENTER_CENTER, icons::USERS, theme::icon(13.0), p.dim);
        rx -= 32.0;
    }
    rx -= g1.size().x;
    ui.painter().galley(pos2(rx, cy - g1.size().y / 2.0), g1, p.text);
    ui.painter().text(
        pos2(rx - 10.0, cy),
        Align2::CENTER_CENTER,
        icons::STAR,
        theme::icon_fill(13.0),
        p.yellow,
    );
    rx -= 26.0;

    let room = bottom - 8.0 - y;
    let lines = (room / 16.0).floor().max(0.0) as usize;
    let text = match &m.synopsis {
        Some(s) => s.split_whitespace().collect::<Vec<_>>().join(" "),
        None => m.genres.join(", "),
    };
    if lines > 0 && !text.is_empty() {
        paint_text(ui, pos2(x, y), &text, theme::regular(12.0), p.faint, w, lines.min(5));
    }

    let mut hit = false;
    let room = (rx - x).max(40.0);
    match &item.entry {
        Some(e) => {
            let fill = status_fill(app, e.status);
            let text = format::status(e.status, m.kind);
            let at = pos2(x, bottom + 4.0);
            let fits =
                widgets::galley(ui, text, theme::semibold(11.5), WHITE, 999.0, 1).size().x + 30.0 <= room;
            let text = if fits { text.to_owned() } else { progress_text(e) };
            pill(&ui.painter_at(rect), at, format::status_icon(e.status), &text, fill, WHITE);
        }
        None => {
            let label = format!("{}: {title}", t("Add to list"));
            let (c, h, _) = solid(ui, &p, id, pos2(x, bottom), room, icons::PLUS, t("Add"), &label, false);
            hit |= h;
            if c {
                app.act(Act::Edit(m.clone()));
            }
        }
    }
    finish(app, &r, item, hit);
}

// Posters

fn tile(app: &mut App, ui: &mut Ui, rect: Rect, item: &Item, style: Style) {
    let p = app.p;
    let m = &item.media;
    let id = ui.scope_id().with(("tile", m.media_ref()));
    let r = ui.interact(rect, id, Sense::click());
    let title = m.display_title(app.s.title_language).to_owned();
    let name = a11y_name(app, item);
    widgets::label_a11y(&r, Role::Button, &name);
    let poster = Rect::from_min_size(rect.min, vec2(rect.width(), rect.width() * 1.45));
    app.tex.paint(ui, &p, &app.bg, m.cover.as_deref(), poster, 10);
    let pt = ui.painter().clone();
    if super::is_open(app, m.media_ref()) || r.has_focus() {
        let s = Stroke::new(3.0, p.tint);
        pt.rect_stroke(poster, CornerRadius::same(10), s, StrokeKind::Inside);
    }
    if let Some(e) = &item.entry {
        let shape = app.s.poster_progress;
        let color = status_fill(app, e.status);
        let bottom = poster.bottom() - 8.0;
        let with_bar = matches!(shape, PosterProgress::Bar | PosterProgress::Both);
        if with_bar {
            let at = pos2(poster.left() + 8.0, bottom - 5.0);
            let b = Rect::from_min_size(at, vec2((poster.width() - 16.0).max(1.0), 5.0));
            pt.rect_filled(b.expand(1.5), CornerRadius::same(4), Color32::from_black_alpha(150));
            let track = Color32::from_white_alpha(90);
            widgets::bar(&pt, b, e.fraction().unwrap_or(0.0), color, track);
        }
        if matches!(shape, PosterProgress::Badge | PosterProgress::Both) {
            let y = bottom - if with_bar { 32.0 } else { 20.0 };
            let mine = matches!(style, Style::Mine { .. }) || poster.width() < 130.0;
            let text = if mine { progress_text(e) } else { format::status(e.status, m.kind).to_owned() };
            let at = pos2(poster.left() + 8.0, y);
            pill(&ui.painter_at(poster), at, format::status_icon(e.status), &text, color, WHITE);
        }
    }
    let mut top = poster.top() + 8.0;
    if let Some(n) = item.rank {
        pill(&pt, pos2(poster.left() + 8.0, top), "", &format!("#{n}"), p.fill, WHITE);
        top += 24.0;
    }
    if app.s.poster_scores {
        let own = item.entry.as_ref().filter(|_| app.s.score_display != ScoreDisplay::Average);
        let s = own.and_then(|e| my_score(e)).or_else(|| m.mean_score.map(|s| decimal(s, 2)));
        if let Some(s) = s {
            pill(&pt, pos2(poster.left() + 8.0, top), icons::STAR, &s, p.yellow, p.on_yellow);
        }
    }
    let at = pos2(poster.left() + 2.0, poster.bottom() + 5.0);
    paint_text(ui, at, &title, theme::semibold(12.0), p.text, poster.width() - 4.0, 2);

    let mut hit = false;
    let addable = item.entry.as_ref().is_none_or(|e| can_add(e));
    if ui.rect_contains_pointer(rect) && addable {
        let c = pos2(poster.right() - 22.0, poster.bottom() - 26.0);
        let b = ui.interact(Rect::from_center_size(c, vec2(32.0, 32.0)), id.with("add"), Sense::click());
        let label = if item.entry.is_some() { t("Add one") } else { t("Add to list") };
        widgets::label_a11y(&b, Role::Button, &format!("{label}: {title}"));
        pt.circle_filled(c, 15.0, p.fill);
        pt.text(c, Align2::CENTER_CENTER, icons::PLUS, theme::icon(17.0), WHITE);
        let b = b.on_hover_text(label);
        hit = b.hovered();
        if b.clicked() {
            let a = if item.entry.is_some() { Act::Step(m.clone(), 1) } else { Act::Edit(m.clone()) };
            app.act(a);
        }
    }
    finish(app, &r, item, hit);
}

// Table

const TABLE_ROW: f32 = 60.0;
const TITLE_MIN: f32 = 230.0;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Col {
    Rank,
    Title,
    Status,
    Progress,
    MyScore,
    Average,
    Next,
    Format,
    Release,
    Length,
    Members,
    Genres,
    Updated,
    Actions,
}

impl Col {
    fn width(self) -> f32 {
        match self {
            Self::Rank => 46.0,
            Self::Title => TITLE_MIN,
            Self::Status => 128.0,
            Self::Progress => 190.0,
            Self::MyScore | Self::Average => 84.0,
            Self::Next => 148.0,
            Self::Format => 100.0,
            Self::Release => 128.0,
            Self::Length => 118.0,
            Self::Members => 100.0,
            Self::Genres => 220.0,
            Self::Updated => 120.0,
            Self::Actions => 46.0,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Rank => "#",
            Self::Title => t("Title"),
            Self::Status => t("Status"),
            Self::Progress => t("Progress"),
            Self::MyScore => t("My score"),
            Self::Average => t("Average"),
            Self::Next => t("Next episode"),
            Self::Format => t("Format"),
            Self::Release => t("Release date"),
            Self::Length => t("Length"),
            Self::Members => t("Members"),
            Self::Genres => t("Genres"),
            Self::Updated => t("Last updated"),
            Self::Actions => "",
        }
    }

    fn sort(self) -> Option<SortKey> {
        Some(match self {
            Self::Title => SortKey::Title,
            Self::Progress => SortKey::Progress,
            Self::MyScore => SortKey::MyScore,
            Self::Average => SortKey::MeanScore,
            Self::Next => SortKey::NextEpisode,
            Self::Release => SortKey::ReleaseDate,
            Self::Updated => SortKey::Updated,
            _ => return None,
        })
    }
}

/// The columns that fit, most useful first, in display order.
fn table_columns(avail: f32, style: Style, kind: MediaKind, ranked: bool) -> Vec<(Col, f32)> {
    use Col as C;
    let anime = kind == MediaKind::Anime;
    // (column, priority): low numbers are kept longest.
    let wanted: Vec<(Col, u8)> = match style {
        Style::Mine { show_status } => vec![
            (C::Title, 0),
            (C::Status, if show_status { 2 } else { u8::MAX }),
            (C::Progress, 1),
            (C::MyScore, 3),
            (C::Average, 4),
            (C::Next, if anime { 5 } else { u8::MAX }),
            (C::Format, 7),
            (C::Release, 6),
            (C::Genres, 9),
            (C::Updated, 8),
            (C::Actions, 0),
        ],
        Style::Browse => vec![
            (C::Rank, if ranked { 0 } else { u8::MAX }),
            (C::Title, 0),
            (C::Average, 1),
            (C::Status, 2),
            (C::Next, if anime { 3 } else { u8::MAX }),
            (C::Format, 4),
            (C::Length, 6),
            (C::Release, 5),
            (C::Members, 7),
            (C::Genres, 8),
            (C::Actions, 0),
        ],
    };
    let mut order: Vec<(Col, u8)> = wanted.iter().copied().filter(|c| c.1 != u8::MAX).collect();
    order.sort_by_key(|c| c.1);
    let mut left = avail - 20.0;
    let mut kept = Vec::new();
    for (c, prio) in order {
        if prio == 0 || left >= c.width() {
            left -= c.width();
            kept.push(c);
        } else {
            // A less useful column must not take the place of this one.
            break;
        }
    }
    let spare = left.max(0.0);
    wanted
        .into_iter()
        .filter(|c| kept.contains(&c.0))
        .map(|(c, _)| {
            (c, if c == C::Title { (TITLE_MIN + spare).min(TITLE_MIN + left).max(60.0) } else { c.width() })
        })
        .collect()
}

fn table_header(app: &mut App, ui: &mut Ui, avail: f32, cols: &[(Col, f32)], sortable: bool) {
    let p = app.p;
    let (bar, _) = ui.allocate_exact_size(vec2(avail, 30.0), Sense::hover());
    let mut x = bar.left() + 10.0;
    for (c, w) in cols {
        let cell = Rect::from_min_size(pos2(x, bar.top()), vec2(*w, 30.0));
        x += w;
        if c.label().is_empty() {
            continue;
        }
        let sort = c.sort().filter(|_| sortable);
        let on = sort.is_some_and(|s| s == app.s.sort);
        let color = if on { p.tint } else { p.dim };
        let g = widgets::galley(ui, c.label(), theme::semibold(12.0), color, w - 22.0, 1);
        let end = cell.left() + g.size().x;
        ui.painter().galley(pos2(cell.left(), cell.center().y - g.size().y / 2.0), g, color);
        let Some(s) = sort else { continue };
        if on {
            let icon = if app.s.sort_asc { icons::CARET_UP } else { icons::CARET_DOWN };
            ui.painter().text(
                pos2(end + 10.0, cell.center().y),
                Align2::CENTER_CENTER,
                icon,
                theme::icon(12.0),
                color,
            );
        }
        let r = ui.interact(
            cell.shrink2(vec2(0.0, 2.0)),
            ui.scope_id().with(("sort", c.label())),
            Sense::click(),
        );
        widgets::label_a11y(&r, Role::Button, &format!("{}: {}", t("Sort by"), c.label()));
        if r.has_focus() {
            ui.painter().rect_stroke(cell, 4, Stroke::new(2.0, p.tint), StrokeKind::Inside);
        }
        if r.on_hover_cursor(CursorIcon::PointingHand).clicked() {
            app.sort_by(s);
        }
    }
    ui.painter().hline(bar.x_range(), bar.bottom(), Stroke::new(1.0, p.line));
}

fn table_row(app: &mut App, ui: &mut Ui, rect: Rect, item: &Item, cols: &[(Col, f32)], index: usize) {
    let p = app.p;
    let m = &item.media;
    let id = ui.scope_id().with(("row", m.media_ref()));
    let r = ui.interact(rect, id, Sense::click_and_drag());
    let name = a11y_name(app, item);
    widgets::label_a11y(&r, Role::Button, &name);
    let round = CornerRadius::same(8);
    let dx = if item.entry.is_some() { swipe(app, ui, rect, round, &r, m) } else { 0.0 };
    let shown = rect.translate(vec2(dx, 0.0));
    let open = super::is_open(app, m.media_ref());
    let hovered = ui.rect_contains_pointer(rect);
    let fill = if open {
        theme::blend(p.card, p.tint, 0.14)
    } else if hovered {
        p.hover
    } else if index.is_multiple_of(2) {
        theme::blend(p.bg, p.card, 0.6)
    } else {
        p.bg
    };
    ui.painter_at(rect).rect_filled(shown, round, fill);
    if r.has_focus() {
        ui.painter().rect_stroke(shown.shrink(1.0), round, Stroke::new(2.0, p.tint), StrokeKind::Inside);
    }
    let title = m.display_title(app.s.title_language).to_owned();
    let cy = shown.center().y;
    let mut x = shown.left() + 10.0;
    let mut hit = false;
    for (c, w) in cols {
        let at = x;
        x += w;
        let room = w - 12.0;
        let text = |ui: &Ui, s: &str, color: Color32| {
            let g = widgets::galley(ui, s, theme::regular(13.0), color, room, 1);
            let y = cy - g.size().y / 2.0;
            ui.painter().galley(pos2(at, y), g, color);
        };
        match c {
            Col::Rank => {
                let n = item.rank.map(|n| n.to_string()).unwrap_or_default();
                let g = widgets::galley(ui, &n, theme::bold(14.0), p.tint, room, 1);
                ui.painter().galley(pos2(at, cy - g.size().y / 2.0), g, p.tint);
            }
            Col::Title => {
                let pic = Rect::from_center_size(pos2(at + 18.0, cy), vec2(34.0, 48.0));
                app.tex.paint(ui, &p, &app.bg, m.cover.as_deref(), pic, 5);
                let tx = pic.right() + 10.0;
                let tw = (at + w - 12.0 - tx).max(1.0);
                let sub = match (&m.title_english, airing_text(m)) {
                    (_, Some(a)) if !cols.iter().any(|c| c.0 == Col::Next) => a,
                    (Some(en), _) if *en != title => en.clone(),
                    _ => format!("{} · {}", format::format(m.format), format::length(m)),
                };
                paint_text(ui, pos2(tx, cy - 18.0), &title, theme::semibold(13.5), p.text, tw, 1);
                paint_text(ui, pos2(tx, cy + 2.0), &sub, theme::regular(12.0), p.dim, tw, 1);
            }
            Col::Status => {
                if let Some(e) = &item.entry {
                    let s = format::status(e.status, m.kind);
                    let fill = status_fill(app, e.status);
                    let cell = Rect::from_min_size(pos2(at, shown.top()), vec2(room, TABLE_ROW));
                    let icon = format::status_icon(e.status);
                    pill(&ui.painter_at(cell), pos2(at, cy - 10.0), icon, s, fill, WHITE);
                }
            }
            Col::Progress => {
                let Some(e) = &item.entry else { continue };
                let label = if m.kind == MediaKind::Manga {
                    tf("Ch {}", &[&progress_text(e)])
                } else {
                    progress_text(e)
                };
                let tw = room - 68.0;
                paint_text(ui, pos2(at, cy - 17.0), &label, theme::semibold(12.5), p.text, tw, 1);
                let b = Rect::from_min_size(pos2(at, cy + 6.0), vec2(tw.max(1.0), 5.0));
                let color = if e.status == ListStatus::Current { p.tint } else { status_fill(app, e.status) };
                widgets::bar(ui.painter(), b, e.fraction().unwrap_or(0.0), color, p.raised);
                if dx != 0.0 {
                    continue;
                }
                if e.progress > 0 {
                    let (c, h) =
                        spot(ui, &p, id, pos2(at + tw + 20.0, cy), icons::MINUS, t("Remove one"), &title);
                    hit |= h;
                    if c {
                        app.act(Act::Step(m.clone(), -1));
                    }
                }
                if can_add(e) {
                    let (c, h) =
                        spot(ui, &p, id, pos2(at + tw + 52.0, cy), icons::PLUS, t("Add one"), &title);
                    hit |= h;
                    if c {
                        app.act(Act::Step(m.clone(), 1));
                    }
                }
            }
            Col::MyScore => {
                let s = item.entry.as_ref().and_then(|e| my_score(e));
                if let Some(s) = s {
                    pill(ui.painter(), pos2(at, cy - 10.0), icons::STAR, &s, p.yellow, p.on_yellow);
                } else {
                    text(ui, "–", p.faint);
                }
            }
            Col::Average => text(ui, &m.mean_score.map_or("–".into(), |s| decimal(s, 2)), p.text),
            Col::Next => match airing_text(m) {
                Some(a) => {
                    text(ui, &a, p.tint);
                    if let Some(n) = item.entry.as_ref().and_then(|e| behind(e)) {
                        let s = tf("{} behind", &[&n]);
                        paint_text(ui, pos2(at, cy + 9.0), &s, theme::semibold(11.0), p.orange, room, 1);
                    }
                }
                None => text(ui, "–", p.faint),
            },
            Col::Format => text(ui, format::format(m.format), p.dim),
            Col::Release => text(ui, &when(m), p.dim),
            Col::Length => text(ui, &format::length(m), p.dim),
            Col::Members => text(ui, &m.members.map_or("–".into(), number), p.dim),
            Col::Genres => text(ui, &m.genres.join(", "), p.dim),
            Col::Updated => {
                let s = item.entry.as_ref().map(|e| format::day(e.updated_at)).unwrap_or_default();
                text(ui, &s, p.dim);
            }
            Col::Actions => {
                let (icon, label) = if item.entry.is_some() {
                    (icons::PENCIL_SIMPLE, t("Edit"))
                } else {
                    (icons::PLUS, t("Add to list"))
                };
                let (c, h) = spot(ui, &p, id, pos2(at + 17.0, cy), icon, label, &title);
                hit |= h;
                if c {
                    app.act(Act::Edit(m.clone()));
                }
            }
        }
    }
    finish(app, &r, item, hit);
}

// Shelves and the layout switch

/// A small poster with a caption, for horizontal shelves.
pub fn shelf_tile(app: &mut App, ui: &mut Ui, m: &Rc<Media>, caption: &str) {
    let p = app.p;
    let (rect, r) = ui.allocate_exact_size(vec2(104.0, 206.0), Sense::click());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let title = m.display_title(app.s.title_language).to_owned();
    widgets::label_a11y(&r, Role::Button, &format!("{caption} {title}"));
    let poster = Rect::from_min_size(rect.min, vec2(104.0, 148.0));
    app.tex.paint(ui, &p, &app.bg, m.cover.as_deref(), poster, 8);
    if r.has_focus() {
        ui.painter().rect_stroke(poster, 8, Stroke::new(2.0, p.tint), StrokeKind::Inside);
    }
    let entry = app.entry(m);
    if let Some(e) = &entry {
        let c = status_fill(app, e.status);
        let at = poster.right_top() + vec2(-13.0, 13.0);
        ui.painter().circle_filled(at, 10.0, c);
        let icon = format::status_icon(e.status);
        ui.painter().text(at, Align2::CENTER_CENTER, icon, theme::icon_fill(12.0), WHITE);
    }
    let mut y = poster.bottom() + 4.0;
    if !caption.is_empty() {
        y += paint_text(ui, pos2(rect.left(), y), caption, theme::semibold(10.5), p.tint, 104.0, 1).y;
    }
    paint_text(ui, pos2(rect.left(), y), &title, theme::regular(12.0), p.text, 104.0, 2);
    let item = Item { media: m.clone(), entry, rank: None };
    finish(app, &r, &item, false);
}

/// Cards, posters or table, as one control.
pub fn layout_switch(ui: &mut Ui, p: &theme::Palette, value: &mut ListLayout) {
    let (bar, base) = ui.allocate_exact_size(vec2(34.0 * 3.0 + 4.0, 32.0), Sense::hover());
    ui.painter().rect_filled(bar, CornerRadius::same(9), p.raised);
    for (i, l) in ListLayout::ALL.into_iter().enumerate() {
        let (icon, label) = match l {
            ListLayout::Cards => (icons::CARDS, t("Cards")),
            ListLayout::Posters => (icons::SQUARES_FOUR, t("Posters")),
            ListLayout::Table => (icons::TABLE, t("Table")),
        };
        let rect = Rect::from_min_size(bar.min + vec2(2.0 + 34.0 * i as f32, 2.0), vec2(34.0, 28.0));
        let r = ui.interact(rect, base.id.with(i), Sense::click());
        let on = *value == l;
        r.widget_info(|| WidgetInfo::selected(Role::RadioButton, true, on, label));
        if on {
            let fill = if p.dark { theme::blend(p.raised, p.text, 0.2) } else { p.card };
            ui.painter().rect_filled(rect, CornerRadius::same(7), fill);
        }
        if r.has_focus() {
            ui.painter().rect_stroke(rect, 7, Stroke::new(2.0, p.tint), StrokeKind::Inside);
        }
        let color = if on { p.tint } else { p.dim };
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, icon, theme::icon(17.0), color);
        if r.on_hover_text(label).on_hover_cursor(CursorIcon::PointingHand).clicked() {
            *value = l;
        }
    }
}

/// Lays out a right-aligned group, first item outermost.
pub fn trailing(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.add_space(BAR - 8.0);
        add(ui);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_always_fill_the_width() {
        for avail in (200..4000).step_by(7) {
            let avail = avail as f32;
            let (n, w) = columns(avail, 330.0, GAP);
            assert!((w * n as f32 + GAP * (n as f32 - 1.0) - avail).abs() < 0.1);
            assert!(n == 1 || w >= 330.0);
        }
    }

    #[test]
    fn table_never_exceeds_its_width_and_keeps_the_essentials() {
        for style in [Style::Mine { show_status: true }, Style::Mine { show_status: false }, Style::Browse] {
            for kind in MediaKind::ALL {
                for avail in (280..3000).step_by(13) {
                    let avail = avail as f32;
                    let cols = table_columns(avail, style, kind, true);
                    let total: f32 = cols.iter().map(|c| c.1).sum();
                    assert!(total <= avail, "{style:?} {avail}: {total}", style = cols);
                    assert!(cols.iter().any(|c| c.0 == Col::Title && c.1 >= 60.0));
                    assert!(cols.iter().any(|c| c.0 == Col::Actions));
                }
            }
        }
    }

    #[test]
    fn wider_tables_only_gain_columns() {
        let style = Style::Mine { show_status: true };
        let mut last = 0;
        for avail in 280..2500 {
            let n = table_columns(avail as f32, style, MediaKind::Anime, false).len();
            assert!(n >= last);
            last = n;
        }
        assert_eq!(last, 11);
    }

    #[test]
    fn behind_counts_released_episodes_only() {
        let mut e = ListEntry { status: ListStatus::Current, progress: 7, ..ListEntry::default() };
        assert_eq!(behind(&e), None);
        e.media.next_airing = Some(kuro_core::Airing { episode: 10, at: 0 });
        assert_eq!(behind(&e), Some(2));
        e.progress = 9;
        assert_eq!(behind(&e), None);
    }
}
