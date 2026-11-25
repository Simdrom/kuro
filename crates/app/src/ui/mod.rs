//! Window layout: navigation, list column and detail column.

pub mod browse;
pub mod detail;
pub mod dialogs;
pub mod extra;
pub mod float;
pub mod mylist;
pub mod rows;
pub mod settings;

use eframe::egui::{
    self, Align2, CentralPanel, CornerRadius, CursorIcon, Frame, Margin, Panel, Role, Sense, Ui, WidgetInfo,
    pos2, vec2,
};
use mango_core::{MediaKind, SeasonYear};

use crate::app::{Act, App, Route, Tab, today};
use crate::i18n::t;
use crate::theme::{self, Palette};
use crate::{format, icons, widgets};

/// Below this width only one column fits.
const NARROW: f32 = 820.0;

pub fn tab_label(tab: Tab) -> &'static str {
    match tab {
        Tab::Anime => t("My Anime"),
        Tab::Manga => t("My Manga"),
        Tab::Seasonal => t("Seasonal"),
        Tab::Ranking => t("Ranking"),
        Tab::Search => t("Search"),
        Tab::Settings => t("Settings"),
    }
}

fn tab_icon(app: &App, tab: Tab) -> &'static str {
    match tab {
        Tab::Anime => icons::MONITOR_PLAY,
        Tab::Manga => icons::BOOK_OPEN,
        Tab::Seasonal if app.s.season_icon => format::season_icon(SeasonYear::of_date(today()).season),
        Tab::Seasonal => icons::CALENDAR,
        Tab::Ranking => icons::STAR,
        Tab::Search => icons::MAGNIFYING_GLASS,
        Tab::Settings => icons::GEAR,
    }
}

/// One "My List" entry stands for both kinds when the tabs are combined.
fn tabs(app: &App) -> Vec<Tab> {
    Tab::ALL
        .into_iter()
        .filter(|tab| {
            app.s.separate_tabs || !matches!((tab, app.tab), (Tab::Manga, _) if app.tab != Tab::Manga)
        })
        .filter(|tab| app.s.separate_tabs || *tab != Tab::Anime || app.tab != Tab::Manga)
        .collect()
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let p = app.p;
    let width = ui.available_width();
    let narrow = width < NARROW;
    if narrow {
        bottom_bar(app, ui);
    } else if app.s.sidebar {
        sidebar(app, ui);
    } else {
        top_bar(app, ui);
    }
    let column = Frame::new().fill(p.bg).inner_margin(Margin { left: 14, right: 8, top: 12, bottom: 0 });
    if narrow {
        CentralPanel::no_frame()
            .frame(column)
            .show(ui, |ui| if app.routes.is_empty() { master(app, ui) } else { detail::show(app, ui, true) });
    } else {
        let max = (width * 0.45).max(320.0);
        Panel::left("master")
            .resizable(true)
            .show_separator_line(false)
            .default_size(370.0)
            .size_range(300.0..=max)
            .frame(column)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                master(app, ui);
            });
        let frame = Frame::new().fill(p.bg).inner_margin(Margin { left: 14, right: 6, top: 10, bottom: 0 });
        CentralPanel::no_frame().frame(frame).show(ui, |ui| detail::show(app, ui, false));
    }
    let ctx = ui.ctx().clone();
    dialogs::show(app, &ctx);
    float::show(app, &ctx);
}

fn master(app: &mut App, ui: &mut Ui) {
    match app.tab {
        Tab::Anime => mylist::show(app, ui, MediaKind::Anime),
        Tab::Manga => mylist::show(app, ui, MediaKind::Manga),
        Tab::Seasonal => browse::seasonal(app, ui),
        Tab::Ranking => browse::ranking(app, ui),
        Tab::Search => browse::search(app, ui),
        Tab::Settings => settings::master(app, ui),
    }
}

fn nav_item(ui: &mut Ui, p: &Palette, icon: &str, label: &str, on: bool) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
    r.widget_info(|| WidgetInfo::selected(Role::Tab, true, on, label));
    if on || r.hovered() || r.has_focus() {
        let fill =
            if on { theme::blend(p.sidebar, p.text, 0.12) } else { theme::blend(p.sidebar, p.text, 0.06) };
        ui.painter().rect_filled(rect, CornerRadius::same(9), fill);
    }
    let font = if on { theme::icon_fill(19.0) } else { theme::icon(19.0) };
    ui.painter().text(pos2(rect.left() + 20.0, rect.center().y), Align2::CENTER_CENTER, icon, font, p.tint);
    let font = if on { theme::semibold(14.0) } else { theme::regular(14.0) };
    ui.painter().text(pos2(rect.left() + 40.0, rect.center().y), Align2::LEFT_CENTER, label, font, p.text);
    r.on_hover_cursor(CursorIcon::PointingHand)
}

fn label_for(app: &App, tab: Tab) -> &'static str {
    if app.s.separate_tabs || !matches!(tab, Tab::Anime | Tab::Manga) { tab_label(tab) } else { t("My List") }
}

fn sidebar(app: &mut App, ui: &mut Ui) {
    let p = app.p;
    Panel::left("sidebar")
        .resizable(false)
        .show_separator_line(false)
        .exact_size(196.0)
        .frame(Frame::new().fill(p.sidebar).inner_margin(Margin::symmetric(10, 12)))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.horizontal(|ui| {
                if widgets::icon_button_colored(
                    ui,
                    &p,
                    icons::SIDEBAR_SIMPLE,
                    t("Hide sidebar"),
                    p.tint,
                    false,
                )
                .clicked()
                {
                    app.s.sidebar = false;
                }
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                ui.label(egui::RichText::new("Mango").font(theme::bold(24.0)));
            });
            ui.add_space(8.0);
            for tab in tabs(app) {
                if tab == Tab::Search {
                    ui.add_space(8.0);
                    widgets::separator(ui, &p);
                    ui.add_space(8.0);
                }
                if nav_item(ui, &p, tab_icon(app, tab), label_for(app, tab), app.tab == tab).clicked() {
                    app.act(Act::Go(tab));
                }
            }
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| status(app, ui));
        });
}

/// Sync state at the bottom of the sidebar.
fn status(app: &mut App, ui: &mut Ui) {
    let p = app.p;
    if app.refreshing > 0 {
        ui.horizontal(|ui| {
            ui.add(egui::Spinner::new().size(14.0).color(p.dim));
            widgets::dim(ui, &p, t("Updating…"));
        });
        return;
    }
    if let Some((s, e)) = &app.sync_error {
        let text = format!("{}: {}", s.name(), format::error(e));
        ui.add(egui::Label::new(egui::RichText::new(text).size(12.0).color(p.orange)).wrap());
    } else if app.unsynced > 0 {
        widgets::dim(ui, &p, crate::i18n::tf("{} changes waiting to sync", &[&app.unsynced]));
    }
    if nav_item(ui, &p, icons::ARROWS_CLOCKWISE, t("Refresh"), false).on_hover_text("F5").clicked() {
        app.act(Act::Refresh);
    }
}

fn top_bar(app: &mut App, ui: &mut Ui) {
    let p = app.p;
    Panel::top("topbar")
        .show_separator_line(false)
        .exact_size(50.0)
        .frame(Frame::new().fill(p.bg).inner_margin(Margin::symmetric(12, 8)))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if widgets::icon_button_colored(
                    ui,
                    &p,
                    icons::SIDEBAR_SIMPLE,
                    t("Show sidebar"),
                    p.tint,
                    false,
                )
                .clicked()
                {
                    app.s.sidebar = true;
                }
                let list = tabs(app);
                let widths: Vec<f32> = list
                    .iter()
                    .map(|tab| {
                        widgets::galley(ui, label_for(app, *tab), theme::semibold(13.5), p.text, 999.0, 1)
                            .size()
                            .x
                            + 28.0
                    })
                    .collect();
                let total: f32 = widths.iter().sum::<f32>() + 8.0;
                ui.add_space(((ui.available_width() - total) / 2.0 - 20.0).max(0.0));
                let (bar, _) = ui.allocate_exact_size(vec2(total, 36.0), Sense::hover());
                ui.painter().rect_filled(bar, CornerRadius::same(18), p.card);
                let mut x = bar.left() + 4.0;
                for (tab, w) in list.into_iter().zip(widths) {
                    let rect = egui::Rect::from_min_size(pos2(x, bar.top() + 4.0), vec2(w, 28.0));
                    let r = ui.interact(rect, ui.scope_id().with(tab), Sense::click());
                    let on = app.tab == tab;
                    r.widget_info(|| WidgetInfo::selected(Role::Tab, true, on, tab_label(tab)));
                    if on || r.hovered() {
                        let fill = theme::blend(p.card, p.text, if on { 0.16 } else { 0.06 });
                        ui.painter().rect_filled(rect, CornerRadius::same(14), fill);
                    }
                    ui.painter().text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        label_for(app, tab),
                        theme::semibold(13.5),
                        if on { p.text } else { p.dim },
                    );
                    if r.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                        app.act(Act::Go(tab));
                    }
                    x += w;
                }
            });
        });
}

fn bottom_bar(app: &mut App, ui: &mut Ui) {
    let p = app.p;
    Panel::bottom("bottombar")
        .show_separator_line(false)
        .exact_size(58.0)
        .frame(Frame::new().fill(p.sidebar).inner_margin(Margin::symmetric(6, 4)))
        .show(ui, |ui| {
            let list = tabs(app);
            let w = ui.available_width() / list.len() as f32;
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.horizontal(|ui| {
                for tab in list {
                    let (rect, r) = ui.allocate_exact_size(vec2(w, 50.0), Sense::click());
                    let on = app.tab == tab;
                    r.widget_info(|| WidgetInfo::selected(Role::Tab, true, on, tab_label(tab)));
                    let color = if on { p.tint } else { p.dim };
                    let font = if on { theme::icon_fill(21.0) } else { theme::icon(21.0) };
                    let c = rect.center();
                    ui.painter().text(
                        c - vec2(0.0, 9.0),
                        Align2::CENTER_CENTER,
                        tab_icon(app, tab),
                        font,
                        color,
                    );
                    ui.painter().text(
                        c + vec2(0.0, 13.0),
                        Align2::CENTER_CENTER,
                        label_for(app, tab),
                        theme::regular(10.5),
                        color,
                    );
                    if r.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                        app.act(Act::Go(tab));
                    }
                }
            });
        });
}

/// Header of the detail column. Returns the area for trailing buttons.
pub fn header(app: &mut App, ui: &mut Ui, title: &str, right: impl FnOnce(&mut App, &mut Ui)) {
    let p = app.p;
    ui.horizontal(|ui| {
        ui.set_min_height(36.0);
        let label = if app.routes.len() > 1 { t("Back") } else { t("Close") };
        let icon = if app.routes.len() > 1 { icons::CARET_LEFT } else { icons::X };
        if widgets::icon_button(ui, &p, icon, label).clicked() {
            app.act(Act::Back);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(8.0);
            right(app, ui);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add_space(6.0);
                ui.add(egui::Label::new(egui::RichText::new(title).font(theme::semibold(15.0))).truncate());
            });
        });
    });
    ui.add_space(4.0);
}

pub fn scroll<R>(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    let id = egui::Id::unique(id);
    egui::ScrollArea::vertical()
        .id_salt(id)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_max_width(ui.available_width() - 8.0);
            let r = add(ui);
            ui.add_space(24.0);
            r
        })
        .inner
}

pub fn is_open(app: &App, r: mango_core::MediaRef) -> bool {
    app.routes.contains(&Route::Media(r))
}
