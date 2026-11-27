//! Window layout. Everything follows the space that is available:
//!
//! - navigation is a sidebar, a rail of icons or a bottom bar;
//! - content fills the window, and an open title takes a resizable panel
//!   beside it, or the whole width when both do not fit.

pub mod browse;
pub mod calendar;
pub mod detail;
pub mod dialogs;
pub mod extra;
pub mod float;
pub mod friends;
pub mod kioku;
pub mod mylist;
pub mod ownlists;
pub mod rows;
pub mod settings;
pub mod stats;

use eframe::egui::{
    self, Align, Align2, CentralPanel, CornerRadius, CursorIcon, Frame, Layout, Margin, Panel, RichText,
    Role, Sense, Ui, WidgetInfo, pos2, vec2,
};
use kuro_core::{MediaKind, SeasonYear};

use crate::app::{Act, App, Route, SettingsPage, Tab, today};
use crate::i18n::t;
use crate::theme::{self, Palette};
use crate::{format, icons, widgets};

/// Below this width navigation moves to the bottom.
const BOTTOM: f32 = 620.0;
/// Below this width the sidebar shrinks to icons.
const RAIL: f32 = 1000.0;
/// Content and an open title fit side by side from this width.
const SPLIT: f32 = 860.0;
const CONTENT_MIN: f32 = 360.0;
const DETAIL_MIN: f32 = 400.0;

pub fn tab_label(tab: Tab) -> &'static str {
    match tab {
        Tab::Anime => t("My Anime"),
        Tab::Manga => t("My Manga"),
        Tab::Calendar => t("My Calendar"),
        Tab::Lists => t("My Lists"),
        Tab::Seasonal => t("Seasonal"),
        Tab::Ranking => t("Ranking"),
        Tab::Search => t("Search"),
        Tab::Friends => t("Friends"),
        Tab::Settings => t("Settings"),
    }
}

fn tab_icon(app: &App, tab: Tab) -> &'static str {
    match tab {
        Tab::Anime => icons::MONITOR_PLAY,
        Tab::Manga => icons::BOOK_OPEN,
        Tab::Calendar => icons::CALENDAR_CHECK,
        Tab::Lists => icons::BOOKMARK_SIMPLE,
        Tab::Seasonal if app.s.season_icon => format::season_icon(SeasonYear::of_date(today()).season),
        Tab::Seasonal => icons::CALENDAR,
        Tab::Ranking => icons::STAR,
        Tab::Search => icons::MAGNIFYING_GLASS,
        Tab::Friends => icons::USERS,
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
    let width = ui.available_width();
    if width < BOTTOM {
        bottom_bar(app, ui);
    } else if width < RAIL {
        rail(app, ui);
    } else if app.s.sidebar {
        sidebar(app, ui);
    } else {
        top_bar(app, ui);
    }
    panels(app, ui);
    let ctx = ui.ctx().clone();
    dialogs::show(app, &ctx);
    float::show(app, &ctx);
}

fn frame(p: &Palette) -> Frame {
    Frame::new().fill(p.bg).inner_margin(Margin { left: 16, right: 8, top: 12, bottom: 0 })
}

fn panels(app: &mut App, ui: &mut Ui) {
    let p = app.p;
    let avail = ui.available_width();
    app.can_split = avail >= SPLIT;
    let column = frame(&p);
    if app.tab == Tab::Settings && app.can_split {
        // Settings always show a page when there is room for it.
        Panel::left("settings")
            .resizable(false)
            .show_separator_line(false)
            .exact_size(340.0)
            .frame(column)
            .show(ui, |ui| settings::master(app, ui));
        CentralPanel::no_frame().frame(frame(&p)).show(ui, |ui| {
            if app.routes.is_empty() {
                settings::page(app, ui, SettingsPage::General);
            } else {
                detail::show(app, ui);
            }
        });
        return;
    }
    let open = !app.routes.is_empty();
    if open && (!app.can_split || app.detail_full) {
        CentralPanel::no_frame().frame(column).show(ui, |ui| detail::show(app, ui));
        return;
    }
    if open {
        let max = (avail - CONTENT_MIN).max(DETAIL_MIN);
        Panel::right("detail")
            .resizable(true)
            .default_size((avail * 0.42).clamp(DETAIL_MIN, 620.0))
            .size_range(DETAIL_MIN..=max)
            .frame(frame(&p))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                detail::show(app, ui);
            });
    }
    CentralPanel::no_frame().frame(column).show(ui, |ui| content(app, ui));
}

fn content(app: &mut App, ui: &mut Ui) {
    match app.tab {
        Tab::Anime => mylist::show(app, ui, MediaKind::Anime),
        Tab::Manga => mylist::show(app, ui, MediaKind::Manga),
        Tab::Calendar => calendar::show(app, ui),
        Tab::Lists => ownlists::show(app, ui),
        Tab::Seasonal => browse::seasonal(app, ui),
        Tab::Ranking => browse::ranking(app, ui),
        Tab::Search => browse::search(app, ui),
        Tab::Friends => friends::show(app, ui),
        Tab::Settings => settings::master(app, ui),
    }
}

/// A page title that gives way to the controls beside it.
pub fn heading(ui: &mut Ui, p: &Palette, text: &str) {
    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
        ui.add(egui::Label::new(RichText::new(text).font(theme::bold(26.0)).color(p.text)).truncate());
    });
}

/// A column of at most `max` points in the middle.
pub fn centered<R>(ui: &mut Ui, max: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let avail = ui.available_width();
    let w = avail.min(max);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space((avail - w) / 2.0);
        column(ui, w, add)
    })
    .inner
}

/// A top-down column of a fixed width.
pub fn column<R>(ui: &mut Ui, w: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.allocate_ui_with_layout(vec2(w, 0.0), Layout::top_down(Align::Min), |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.set_width(w);
        add(ui)
    })
    .inner
}

// Navigation

fn nav_item(ui: &mut Ui, p: &Palette, icon: &str, label: &str, on: bool, text: bool) -> egui::Response {
    let (rect, r) =
        ui.allocate_exact_size(vec2(ui.available_width(), if text { 36.0 } else { 42.0 }), Sense::click());
    r.widget_info(|| WidgetInfo::selected(Role::Tab, true, on, label));
    if on || r.hovered() || r.has_focus() {
        let fill = theme::blend(p.sidebar, p.text, if on { 0.12 } else { 0.06 });
        ui.painter().rect_filled(rect, CornerRadius::same(9), fill);
    }
    let font = if on { theme::icon_fill(19.0) } else { theme::icon(19.0) };
    let x = if text { rect.left() + 20.0 } else { rect.center().x };
    ui.painter().text(pos2(x, rect.center().y), Align2::CENTER_CENTER, icon, font, p.tint);
    if text {
        let font = if on { theme::semibold(14.0) } else { theme::regular(14.0) };
        let g = widgets::galley(ui, label, font, p.text, rect.width() - 46.0, 1);
        ui.painter().galley(pos2(rect.left() + 40.0, rect.center().y - g.size().y / 2.0), g, p.text);
    }
    let r = r.on_hover_cursor(CursorIcon::PointingHand);
    if text { r } else { r.on_hover_text(label) }
}

fn label_for(app: &App, tab: Tab) -> &'static str {
    if app.s.separate_tabs || !matches!(tab, Tab::Anime | Tab::Manga) { tab_label(tab) } else { t("My List") }
}

fn nav_list(app: &mut App, ui: &mut Ui, text: bool) {
    let p = app.p;
    for tab in tabs(app) {
        if tab == Tab::Search {
            ui.add_space(8.0);
            widgets::separator(ui, &p);
            ui.add_space(8.0);
        }
        if nav_item(ui, &p, tab_icon(app, tab), label_for(app, tab), app.tab == tab, text).clicked() {
            app.act(Act::Go(tab));
        }
    }
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
            ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                status(app, ui, true);
                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        let label = t("Hide sidebar");
                        if widgets::icon_button_colored(ui, &p, icons::SIDEBAR_SIMPLE, label, p.tint, false)
                            .clicked()
                        {
                            app.s.sidebar = false;
                        }
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.add_space(8.0);
                            ui.label(RichText::new("Kuro").font(theme::bold(24.0)));
                            ui.label(RichText::new("黒").font(theme::regular(17.0)).color(p.faint));
                        });
                        ui.add_space(8.0);
                        nav_list(app, ui, true);
                    });
                });
            });
        });
}

fn rail(app: &mut App, ui: &mut Ui) {
    let p = app.p;
    Panel::left("rail")
        .resizable(false)
        .show_separator_line(false)
        .exact_size(64.0)
        .frame(Frame::new().fill(p.sidebar).inner_margin(Margin::symmetric(8, 12)))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                status(app, ui, false);
                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| nav_list(app, ui, false));
                });
            });
        });
}

/// Sync state at the bottom of the navigation.
fn status(app: &mut App, ui: &mut Ui, text: bool) {
    let p = app.p;
    if app.refreshing > 0 {
        ui.horizontal(|ui| {
            ui.add_space(if text { 0.0 } else { 14.0 });
            ui.add(egui::Spinner::new().size(16.0).color(p.dim));
            if text {
                widgets::dim(ui, &p, t("Updating…"));
            }
        });
        return;
    }
    let mut hint = "F5".to_owned();
    let problem = app.sync_error.as_ref().map(|(s, e)| format!("{}: {}", s.name(), format::error(e)));
    if let Some(s) = &problem {
        hint = s.clone();
    } else if app.unsynced > 0 {
        hint = crate::i18n::tf("{} changes waiting to sync", &[&app.unsynced]);
    }
    // Bottom-up: the button comes first, its explanation above it.
    if nav_item(ui, &p, icons::ARROWS_CLOCKWISE, t("Refresh"), false, text).on_hover_text(&hint).clicked() {
        app.act(Act::Refresh);
    }
    if text && hint != "F5" {
        let color = if problem.is_some() { p.orange } else { p.dim };
        ui.add(egui::Label::new(RichText::new(hint).size(12.0).color(color)).wrap());
    }
    if let Some(u) = &app.update {
        // Short enough for the sidebar; the whole sentence on hover.
        let hint = crate::i18n::tf("Kuro {} is available", &[&u.version]);
        if nav_item(ui, &p, icons::SPARKLE, t("New version"), false, text).on_hover_text(&hint).clicked() {
            app.act(Act::Go(Tab::Settings));
            app.act(Act::Push(Route::Settings(SettingsPage::About)));
        }
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
                    if on || r.hovered() || r.has_focus() {
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
                    let icon_at = c - vec2(0.0, 9.0);
                    ui.painter().text(icon_at, Align2::CENTER_CENTER, tab_icon(app, tab), font, color);
                    let g = widgets::galley(ui, label_for(app, tab), theme::regular(10.5), color, w - 4.0, 1);
                    ui.painter().galley(c + vec2(-g.size().x / 2.0, 6.0), g, color);
                    if r.has_focus() {
                        let s = egui::Stroke::new(2.0, p.tint);
                        ui.painter().rect_stroke(rect.shrink(1.0), 8, s, egui::StrokeKind::Inside);
                    }
                    if r.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                        app.act(Act::Go(tab));
                    }
                }
            });
        });
}

/// Header of an open title or page, with trailing buttons.
pub fn header(app: &mut App, ui: &mut Ui, title: &str, right: impl FnOnce(&mut App, &mut Ui)) {
    let p = app.p;
    ui.horizontal(|ui| {
        ui.set_min_height(36.0);
        if !app.routes.is_empty() {
            let deeper = app.routes.len() > 1 || !app.can_split;
            let (icon, label) = if deeper { (icons::CARET_LEFT, t("Back")) } else { (icons::X, t("Close")) };
            if widgets::icon_button(ui, &p, icon, label).clicked() {
                app.act(Act::Back);
            }
        }
        rows::trailing(ui, |ui| {
            if app.can_split && app.tab != Tab::Settings {
                let (icon, label) = if app.detail_full {
                    (icons::ARROWS_IN_SIMPLE, t("Show beside the list"))
                } else {
                    (icons::ARROWS_OUT_SIMPLE, t("Use the whole window"))
                };
                if widgets::icon_button_colored(ui, &p, icon, label, p.tint, false).clicked() {
                    app.detail_full = !app.detail_full;
                }
            }
            right(app, ui);
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.add_space(6.0);
                ui.add(egui::Label::new(RichText::new(title).font(theme::semibold(15.0))).truncate());
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
            ui.set_max_width((ui.available_width() - 10.0).max(1.0));
            let r = add(ui);
            ui.add_space(24.0);
            r
        })
        .inner
}

/// Whether `r` is the title shown in the detail pane, or the one a page on top of it came from.
/// Titles further back in the history do not count.
pub fn is_open(app: &App, r: kuro_core::MediaRef) -> bool {
    app.routes.iter().rev().find_map(|x| if let Route::Media(m) = x { Some(*m) } else { None }) == Some(r)
}
