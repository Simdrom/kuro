//! The user's own list.

use std::collections::HashMap;
use std::rc::Rc;

use eframe::egui::{self, Align2, CornerRadius, CursorIcon, Role, Sense, Ui, WidgetInfo, pos2, vec2};
use mango_core::list::{self, Filter, SortKey};
use mango_core::{Format, ListEntry, ListStatus, MediaKind, Service};

use super::rows::{self, Item, Style};
use crate::app::{Act, App, ListCache, Route, SettingsPage, Tab};
use crate::i18n::{t, tf};
use crate::theme;
use crate::widgets;
use crate::{format, icons};

fn slot(k: MediaKind) -> usize {
    usize::from(k == MediaKind::Manga)
}

/// From this width the whole toolbar fits on one line.
const WIDE: f32 = 820.0;

pub fn show(app: &mut App, ui: &mut Ui, kind: MediaKind) {
    let p = app.p;
    let k = slot(kind);
    let wide = ui.available_width() >= WIDE;
    ui.horizontal(|ui| {
        ui.set_min_height(36.0);
        let label = if app.s.separate_tabs { super::tab_label(app.tab) } else { t("My List") };
        rows::trailing(ui, |ui| {
            let r = widgets::icon_button_colored(ui, &p, icons::SLIDERS, t("List settings"), p.tint, false);
            if r.clicked() {
                app.act(Act::Push(Route::Settings(SettingsPage::Lists)));
            }
            let name = t("Desktop widget");
            if widgets::icon_button_colored(ui, &p, icons::PUSH_PIN, name, p.tint, false).clicked() {
                app.widget = !app.widget;
            }
            if !app.s.separate_tabs {
                let other = kind.other();
                let r = widgets::icon_button_colored(ui, &p, icons::SWAP, format::kind(other), p.tint, false);
                if r.clicked() {
                    app.act(Act::Go(if other == MediaKind::Anime { Tab::Anime } else { Tab::Manga }));
                }
            }
            if wide {
                ui.add_space(6.0);
                tools(app, ui, kind);
                let w = (ui.available_width() * 0.55).clamp(180.0, 360.0);
                ui.allocate_ui(vec2(w, 32.0), |ui| {
                    widgets::search_field(ui, &p, &mut app.lists[k].filter.query, t("Search in your list"));
                });
            }
            super::heading(ui, &p, label);
        });
    });
    ui.add_space(4.0);
    if wide {
        statuses(app, ui, kind);
    } else if ui.available_width() >= 460.0 {
        ui.horizontal(|ui| {
            // The tools are three buttons and the layout switch.
            let w = (ui.available_width() - 250.0).max(120.0);
            ui.allocate_ui(vec2(w, 32.0), |ui| {
                widgets::search_field(ui, &p, &mut app.lists[k].filter.query, t("Search in your list"));
            });
            rows::trailing(ui, |ui| tools(app, ui, kind));
        });
        ui.add_space(2.0);
        statuses(app, ui, kind);
    } else {
        ui.horizontal(|ui| {
            ui.set_max_width(ui.available_width() - 8.0);
            widgets::search_field(ui, &p, &mut app.lists[k].filter.query, t("Search in your list"));
        });
        ui.add_space(2.0);
        status_menu(app, ui, kind);
        ui.horizontal(|ui| tools(app, ui, kind));
    }
    ui.add_space(6.0);

    if app.library(kind).is_empty() {
        welcome(app, ui, kind);
        return;
    }
    let mut filter = app.lists[k].filter.clone();
    filter.hide_adult = !app.s.show_adult;
    // Searching looks through every status.
    if !filter.query.trim().is_empty() {
        filter.status = None;
    }
    let style = Style::Mine { show_status: filter.status.is_none() };
    let items = rows_of(app, kind, filter);
    if items.is_empty() {
        widgets::empty(ui, &p, icons::STACK, t("Nothing here"));
        return;
    }
    let count = items.len();
    let foot = move |app: &mut App, ui: &mut Ui| {
        ui.add_space(6.0);
        let p = app.p;
        ui.vertical_centered(|ui| widgets::dim(ui, &p, tf("{} titles", &[&count])));
    };
    let layout = app.s.list_layout;
    rows::show(app, ui, &format!("mylist{k}"), &items, layout, style, foot);
}

fn rows_of(app: &mut App, kind: MediaKind, filter: Filter) -> Rc<Vec<Item>> {
    let key = (app.lib_version, filter, app.s.sort, app.s.sort_asc, app.s.title_language);
    if let Some(c) = &app.lists[slot(kind)].cache
        && c.key == key
    {
        return c.items.clone();
    }
    let found: Vec<&Rc<ListEntry>> = app.library(kind).iter().filter(|e| key.1.accepts(e)).collect();
    let mut sorted: Vec<&ListEntry> = found.iter().map(|e| &***e).collect();
    list::sort(&mut sorted, key.2, key.3, key.4);
    let by_ptr: HashMap<*const ListEntry, &Rc<ListEntry>> =
        found.iter().map(|e| (Rc::as_ptr(e), *e)).collect();
    let items: Vec<Item> = sorted
        .iter()
        .filter_map(|e| by_ptr.get(&std::ptr::from_ref(*e)))
        .map(|e| Item { media: Rc::new(e.media.clone()), entry: Some((*e).clone()), rank: None })
        .collect();
    let items = Rc::new(items);
    app.lists[slot(kind)].cache = Some(ListCache { key, items: items.clone() });
    items
}

/// Status, name and number of titles.
fn counts(app: &App, kind: MediaKind) -> Vec<(Option<ListStatus>, &'static str, usize)> {
    let list = app.library(kind);
    let mut out = vec![(None, t("All"), list.len())];
    for s in ListStatus::ALL {
        let n = list.iter().filter(|e| e.status == s).count();
        out.push((Some(s), format::status(s, kind), n));
    }
    out
}

/// Every status at once, or a menu when they do not fit.
fn statuses(app: &mut App, ui: &mut Ui, kind: MediaKind) {
    let p = app.p;
    let k = slot(kind);
    let current = app.lists[k].filter.status;
    let chips: Vec<_> = counts(app, kind)
        .into_iter()
        .map(|(s, name, n)| {
            let g = widgets::galley(ui, name, theme::semibold(13.0), p.text, 999.0, 1);
            let c = widgets::galley(ui, &n.to_string(), theme::regular(12.5), p.dim, 999.0, 1);
            let w = g.size().x + c.size().x + if s.is_some() { 50.0 } else { 32.0 };
            (s, name, n, g, c, w)
        })
        .collect();
    let total: f32 = chips.iter().map(|c| c.5 + 8.0).sum();
    if total > ui.available_width() {
        status_menu(app, ui, kind);
        return;
    }
    ui.horizontal(|ui| {
        for (s, name, n, g, c, w) in chips {
            let (rect, r) = ui.allocate_exact_size(vec2(w, 32.0), Sense::click());
            let on = current == s;
            let label = format!("{name} {n}");
            r.widget_info(|| WidgetInfo::selected(Role::RadioButton, true, on, &label));
            let fill = if on {
                p.tint_soft()
            } else if r.hovered() {
                p.hover
            } else {
                p.card
            };
            ui.painter().rect_filled(rect, CornerRadius::same(9), fill);
            if on || r.has_focus() {
                let st = egui::Stroke::new(1.5, p.tint);
                ui.painter().rect_stroke(rect, CornerRadius::same(9), st, egui::StrokeKind::Inside);
            }
            let mut x = rect.left() + 12.0;
            if let Some(s) = s {
                let font = theme::icon_fill(14.0);
                let at = pos2(x + 6.0, rect.center().y);
                ui.painter().text(at, Align2::CENTER_CENTER, format::status_icon(s), font, p.status(s));
                x += 18.0;
            }
            let gw = g.size().x;
            ui.painter().galley(pos2(x, rect.center().y - g.size().y / 2.0), g, p.text);
            ui.painter().galley(pos2(x + gw + 8.0, rect.center().y - c.size().y / 2.0), c, p.dim);
            if r.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                app.lists[k].filter.status = s;
            }
        }
    });
}

fn status_menu(app: &mut App, ui: &mut Ui, kind: MediaKind) {
    let p = app.p;
    let k = slot(kind);
    let current = app.lists[k].filter.status;
    let label = current.map_or(t("All"), |s| format::status(s, kind));
    let icon = current.map_or(icons::LIST_BULLETS, format::status_icon);
    let r = widgets::button(ui, &p, icon, label, false);
    egui::Popup::menu(&r).show(|ui| {
        ui.set_min_width(190.0);
        for (s, name, n) in counts(app, kind) {
            if ui.selectable_label(current == s, format!("{name}  ({n})")).clicked() {
                app.lists[k].filter.status = s;
                ui.close();
            }
        }
    });
}

/// Layout, sorting and filters.
fn tools(app: &mut App, ui: &mut Ui, kind: MediaKind) {
    let p = app.p;
    let k = slot(kind);
    rows::layout_switch(ui, &p, &mut app.s.list_layout);
    let (icon, text) = if app.s.sort_asc {
        (icons::SORT_ASCENDING, t("Ascending"))
    } else {
        (icons::SORT_DESCENDING, t("Descending"))
    };
    if widgets::icon_button(ui, &p, icon, text).clicked() {
        app.s.sort_asc = !app.s.sort_asc;
    }
    let r = widgets::icon_button(ui, &p, icons::CLOCK_COUNTER_CLOCKWISE, t("Sort by"));
    egui::Popup::menu(&r).show(|ui| {
        ui.set_min_width(170.0);
        for s in SortKey::ALL {
            if ui.selectable_label(app.s.sort == s, format::sort(s)).clicked() {
                if app.s.sort != s {
                    app.sort_by(s);
                }
                ui.close();
            }
        }
    });
    let on = app.lists[k].filter.format.is_some() || app.lists[k].filter.genre.is_some();
    let color = if on { p.orange } else { p.tint };
    let r = widgets::icon_button_colored(ui, &p, icons::FUNNEL, t("Filter"), color, true);
    egui::Popup::menu(&r)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| filters(app, ui, kind));
}

fn filters(app: &mut App, ui: &mut Ui, kind: MediaKind) {
    ui.set_min_width(220.0);
    let k = slot(kind);
    let mut formats: Vec<Format> = app.library(kind).iter().map(|e| e.media.format).collect();
    formats.sort();
    formats.dedup();
    let mut genres: Vec<String> = app.library(kind).iter().flat_map(|e| e.media.genres.clone()).collect();
    genres.sort();
    genres.dedup();
    let f = &mut app.lists[k].filter;
    ui.label(t("Format"));
    let mut options = vec![None];
    options.extend(formats.into_iter().map(Some));
    widgets::select(ui, "fmt", t("Format"), &mut f.format, &options, |v| {
        v.map_or(t("All"), format::format).to_owned()
    });
    ui.label(t("Genre"));
    egui::ComboBox::from_id_salt("genre").selected_text(f.genre.as_deref().unwrap_or(t("All"))).show_ui(
        ui,
        |ui| {
            ui.selectable_value(&mut f.genre, None, t("All"));
            for g in genres {
                ui.selectable_value(&mut f.genre, Some(g.clone()), g);
            }
        },
    );
    if ui.button(t("Clear filters")).clicked() {
        (f.format, f.genre) = (None, None);
    }
}

fn welcome(app: &mut App, ui: &mut Ui, kind: MediaKind) {
    let p = app.p;
    if app.refreshing > 0 {
        widgets::loading(ui, &p);
        return;
    }
    super::scroll(ui, ("welcome", kind), |ui| {
        super::centered(ui, 460.0, |ui| {
            ui.add_space(8.0);
            sign_in_card(app, ui);
            ui.add_space(6.0);
            let text =
                t("Your list stays on this computer until you sign in. Nothing is shared with anyone else.");
            widgets::note(ui, &p, text);
            ui.add_space(14.0);
            ui.vertical_centered(|ui| {
                if widgets::button(ui, &p, icons::MAGNIFYING_GLASS, t("Find something to add"), false)
                    .clicked()
                {
                    app.search.kind = kind;
                    app.act(Act::Go(Tab::Search));
                }
            });
        });
    });
}

/// Gradient card with both services.
pub fn sign_in_card(app: &mut App, ui: &mut Ui) {
    let p = app.p;
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(w, 168.0), Sense::hover());
    widgets::gradient(ui, rect, p.tint, p.green);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(16.0)));
    let ui = &mut child;
    ui.vertical_centered(|ui| {
        let fg = egui::Color32::WHITE;
        ui.label(egui::RichText::new(t("Track your anime and manga")).font(theme::bold(17.0)).color(fg));
        ui.label(egui::RichText::new(t("Sign in to sync your lists.")).color(fg));
        ui.add_space(6.0);
        for s in Service::ALL {
            let signed = app.bg.api.is_signed_in(s);
            let text = if signed { tf("{} connected", &[&s.name()]) } else { s.name().to_owned() };
            let text =
                egui::RichText::new(text).font(theme::bold(15.0)).color(egui::Color32::from_rgb(22, 60, 130));
            let b =
                egui::Button::new(text).fill(fg).corner_radius(9).min_size(vec2(ui.available_width(), 34.0));
            if ui.add_enabled(!signed, b).clicked() {
                if app.bg.api.can_sign_in(s) {
                    app.act(Act::Login(s));
                } else {
                    app.act(Act::Push(Route::Settings(SettingsPage::Accounts)));
                }
            }
        }
    });
}
