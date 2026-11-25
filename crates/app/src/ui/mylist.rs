//! The user's own list.

use std::rc::Rc;

use eframe::egui::{self, Ui};
use mango_core::list::{self, SortKey};
use mango_core::{Format, ListEntry, ListStatus, MediaKind, Service};

use super::rows::{self, Item, Style};
use crate::app::{Act, App, Route, SettingsPage, Tab};
use crate::i18n::{t, tf};
use crate::widgets;
use crate::{format, icons};

fn slot(k: MediaKind) -> usize {
    usize::from(k == MediaKind::Manga)
}

pub fn show(app: &mut App, ui: &mut Ui, kind: MediaKind) {
    let p = app.p;
    ui.horizontal(|ui| {
        let label = if app.s.separate_tabs { super::tab_label(app.tab) } else { t("My List") };
        widgets::title(ui, &p, label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(8.0);
            let r = widgets::icon_button_colored(ui, &p, icons::SLIDERS, t("List settings"), p.tint, false);
            if r.clicked() {
                app.act(Act::Push(Route::Settings(SettingsPage::Lists)));
            }
            let widget_label = t("Desktop widget");
            if widgets::icon_button_colored(ui, &p, icons::PUSH_PIN, widget_label, p.tint, false).clicked() {
                app.widget = !app.widget;
            }
            if !app.s.separate_tabs {
                let other = kind.other();
                let r = widgets::icon_button_colored(ui, &p, icons::SWAP, format::kind(other), p.tint, false);
                if r.clicked() {
                    app.act(Act::Go(if other == MediaKind::Anime { Tab::Anime } else { Tab::Manga }));
                }
            }
        });
    });
    if !app.s.separate_tabs {
        widgets::dim(ui, &p, format::kind(kind));
    }
    let k = slot(kind);
    ui.horizontal(|ui| {
        ui.set_max_width(ui.available_width() - 8.0);
        widgets::search_field(ui, &p, &mut app.lists[k].filter.query, t("Search in your list"));
    });
    ui.add_space(2.0);
    toolbar(app, ui, kind);
    ui.add_space(4.0);

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
    let found: Vec<Rc<ListEntry>> = app.library(kind).iter().filter(|e| filter.accepts(e)).cloned().collect();
    let mut sorted: Vec<&ListEntry> = found.iter().map(|e| &**e).collect();
    list::sort(&mut sorted, app.s.sort, app.s.sort_asc, app.s.title_language);
    let by_ptr: std::collections::HashMap<*const ListEntry, &Rc<ListEntry>> =
        found.iter().map(|e| (Rc::as_ptr(e), e)).collect();
    let items: Vec<Item> = sorted
        .iter()
        .filter_map(|e| by_ptr.get(&std::ptr::from_ref(*e)))
        .map(|e| Item { media: Rc::new(e.media.clone()), entry: Some((*e).clone()), rank: None })
        .collect();
    if items.is_empty() {
        widgets::empty(ui, &p, icons::STACK, t("Nothing here"));
        return;
    }
    let style = Style::Mine { show_status: filter.status.is_none() };
    let count = items.len();
    let foot = move |app: &mut App, ui: &mut Ui| {
        ui.add_space(6.0);
        let p = app.p;
        ui.vertical_centered(|ui| widgets::dim(ui, &p, tf("{} titles", &[&count])));
    };
    let id = format!("mylist{k}");
    if app.s.list_grid {
        rows::grid(app, ui, &id, &items, true, foot);
    } else {
        rows::list(app, ui, &id, &items, style, foot);
    }
}

fn toolbar(app: &mut App, ui: &mut Ui, kind: MediaKind) {
    let p = app.p;
    let k = slot(kind);
    ui.horizontal(|ui| {
        let current = app.lists[k].filter.status;
        let label = current.map_or(t("All"), |s| format::status(s, kind));
        let icon = current.map_or(icons::LIST_BULLETS, format::status_icon);
        let r = widgets::button(ui, &p, icon, label, false);
        egui::Popup::menu(&r).show(|ui| {
            ui.set_min_width(170.0);
            let mut pick = |ui: &mut Ui, s: Option<ListStatus>, text: &str| {
                if ui.selectable_label(current == s, text).clicked() {
                    app.lists[k].filter.status = s;
                    ui.close();
                }
            };
            pick(ui, None, t("All"));
            for s in ListStatus::ALL {
                pick(ui, Some(s), format::status(s, kind));
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(8.0);
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
                        // Start with the direction people expect.
                        app.s.sort_asc = matches!(s, SortKey::Title | SortKey::NextEpisode);
                        app.s.sort = s;
                        ui.close();
                    }
                }
            });
            let on = app.lists[k].filter.format.is_some() || app.lists[k].filter.genre.is_some();
            let r = widgets::icon_button_colored(
                ui,
                &p,
                icons::FUNNEL,
                t("Filter"),
                if on { p.orange } else { p.tint },
                true,
            );
            egui::Popup::menu(&r)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| filters(app, ui, kind));
            let (icon, text) = if app.s.list_grid {
                (icons::LIST_BULLETS, t("Show as list"))
            } else {
                (icons::SQUARES_FOUR, t("Show as posters"))
            };
            if widgets::icon_button(ui, &p, icon, text).clicked() {
                app.s.list_grid = !app.s.list_grid;
            }
        });
    });
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
        ui.add_space(8.0);
        sign_in_card(app, ui);
        ui.add_space(6.0);
        widgets::note(
            ui,
            &p,
            t("Your list stays on this computer until you sign in. Nothing is shared with anyone else."),
        );
        ui.add_space(14.0);
        ui.vertical_centered(|ui| {
            if widgets::button(ui, &p, icons::MAGNIFYING_GLASS, t("Find something to add"), false).clicked() {
                app.search.kind = kind;
                app.act(Act::Go(Tab::Search));
            }
        });
    });
}

/// Gradient card with both services.
pub fn sign_in_card(app: &mut App, ui: &mut Ui) {
    let p = app.p;
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 168.0), egui::Sense::hover());
    widgets::gradient(ui, rect, p.tint, p.green);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(16.0)));
    let ui = &mut child;
    ui.vertical_centered(|ui| {
        let fg = egui::Color32::WHITE;
        ui.label(
            egui::RichText::new(t("Track your anime and manga")).font(crate::theme::bold(17.0)).color(fg),
        );
        ui.label(egui::RichText::new(t("Sign in to sync your lists.")).color(fg));
        ui.add_space(6.0);
        for s in Service::ALL {
            let signed = app.bg.api.is_signed_in(s);
            let text = if signed { tf("{} connected", &[&s.name()]) } else { s.name().to_owned() };
            let b = egui::Button::new(
                egui::RichText::new(text)
                    .font(crate::theme::bold(15.0))
                    .color(egui::Color32::from_rgb(22, 60, 130)),
            )
            .fill(fg)
            .corner_radius(9)
            .min_size(egui::vec2(ui.available_width(), 34.0));
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
