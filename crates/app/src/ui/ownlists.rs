//! The user's custom lists (tags on MyAnimeList), anime and manga together.

use std::collections::HashMap;
use std::rc::Rc;

use eframe::egui::{self, Ui};
use kuro_core::list;
use kuro_core::{ListEntry, ListOp, MediaKind};

use super::rows::{self, Item, Style};
use crate::app::{Act, App, Naming, OwnListsCache, Tab};
use crate::i18n::{t, tf};
use crate::{icons, widgets};

pub fn show(app: &mut App, ui: &mut Ui) {
    let p = app.p;
    ui.horizontal(|ui| {
        ui.set_min_height(36.0);
        rows::trailing(ui, |ui| {
            rows::layout_switch(ui, &p, &mut app.s.list_layout);
            let free = !app.own_lists.busy && app.own_lists.naming.is_none();
            if ui
                .add_enabled_ui(free, |ui| widgets::button(ui, &p, icons::PLUS, t("New list"), true))
                .inner
                .clicked()
            {
                app.own_lists.naming = Some(Naming { from: None, text: String::new() });
            }
            super::heading(ui, &p, super::tab_label(Tab::Lists));
        });
    });
    ui.add_space(4.0);
    if app.own_lists.busy {
        widgets::dim(ui, &p, t("Changing the list on AniList…"));
    }
    if app.own_lists.naming.is_some() {
        naming(app, ui);
    }
    let names = app.list_names();
    if names.is_empty() {
        widgets::empty(ui, &p, icons::BOOKMARK_SIMPLE, t("No custom lists yet"));
        ui.vertical_centered(|ui| {
            widgets::dim(ui, &p, t("Make one with New list, then add titles to it."));
        });
        return;
    }
    let same = |a: &str, b: &str| a.to_lowercase() == b.to_lowercase();
    let current = match &app.own_lists.selected {
        Some(s) if names.iter().any(|n| same(n, s)) => {
            names.iter().find(|n| same(n, s)).cloned().unwrap_or_default()
        }
        _ => names[0].clone(),
    };
    app.own_lists.selected = Some(current.clone());
    let items = rows_of(app, &current);
    ui.horizontal_wrapped(|ui| {
        for n in &names {
            let count = MediaKind::ALL
                .iter()
                .flat_map(|k| app.library(*k))
                .filter(|e| e.lists().iter().any(|x| same(x, n)))
                .count();
            let on = same(n, &current);
            if ui.selectable_label(on, format!("{n}  ({count})")).clicked() {
                app.own_lists.selected = Some(n.clone());
            }
        }
    });
    ui.add_space(4.0);
    tools(app, ui, &current);
    ui.add_space(6.0);
    if items.is_empty() {
        widgets::empty(ui, &p, icons::STACK, t("This list is empty"));
        ui.vertical_centered(|ui| {
            widgets::dim(ui, &p, t("Add titles with Add titles, or from a title's editor."))
        });
        return;
    }
    let count = items.len();
    let foot = move |app: &mut App, ui: &mut Ui| {
        ui.add_space(6.0);
        let p = app.p;
        ui.vertical_centered(|ui| widgets::dim(ui, &p, tf("{} titles", &[&count])));
    };
    let layout = app.s.list_layout;
    rows::show(app, ui, "ownlists", &items, layout, Style::Mine { show_status: true }, foot);
}

/// The name of a new list, or the new name of one.
fn naming(app: &mut App, ui: &mut Ui) {
    let p = app.p;
    let Some(n) = &mut app.own_lists.naming else { return };
    let (mut done, mut cancel) = (false, false);
    ui.horizontal(|ui| {
        let hint = if n.from.is_some() { t("New name") } else { t("Name of the new list") };
        let r = ui.add(
            egui::TextEdit::singleline(&mut n.text)
                .hint_text(hint)
                .char_limit(kuro_core::MAX_LIST_NAME)
                .desired_width(260.0),
        );
        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, hint));
        if !r.has_focus() && n.text.is_empty() {
            r.request_focus();
        }
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let label = if n.from.is_some() { t("Save") } else { t("Create") };
        done = widgets::button(ui, &p, icons::CHECK, label, true).clicked() || enter;
        cancel = widgets::button(ui, &p, "", t("Cancel"), false).clicked()
            || ui.input(|i| i.key_pressed(egui::Key::Escape));
    });
    ui.add_space(4.0);
    if cancel {
        app.own_lists.naming = None;
    } else if done && !n.text.trim().is_empty() {
        let text = n.text.clone();
        let op = match n.from.clone() {
            Some(from) => ListOp::Rename(from, text),
            None => ListOp::Create(text),
        };
        app.act(Act::EditLists(op));
    }
}

/// Adding titles to the list, renaming and deleting it.
fn tools(app: &mut App, ui: &mut Ui, name: &str) {
    let p = app.p;
    let free = !app.own_lists.busy && app.own_lists.naming.is_none();
    ui.horizontal(|ui| {
        let add = widgets::button(ui, &p, icons::PLUS, t("Add titles"), false);
        egui::Popup::menu(&add)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| picker(app, ui, name));
        ui.add_enabled_ui(free, |ui| {
            if widgets::button(ui, &p, icons::PENCIL_SIMPLE, t("Rename"), false).clicked() {
                app.own_lists.naming = Some(Naming { from: Some(name.to_owned()), text: name.to_owned() });
            }
            if widgets::button(ui, &p, icons::TRASH, t("Delete"), false).clicked() {
                app.act(Act::AskDeleteList(name.to_owned()));
            }
        });
    });
}

/// Titles of the user's list that are not in this one yet.
fn picker(app: &mut App, ui: &mut Ui, name: &str) {
    let p = app.p;
    ui.set_width(320.0);
    widgets::search_field(ui, &p, &mut app.own_lists.find, t("Find in your list"));
    ui.add_space(4.0);
    let low = name.to_lowercase();
    let lang = app.s.title_language;
    let found: Vec<(Rc<kuro_core::Media>, String)> = MediaKind::ALL
        .iter()
        .flat_map(|k| app.library(*k).iter())
        .filter(|e| app.s.show_adult || !e.media.adult)
        .filter(|e| !e.lists().iter().any(|x| x.to_lowercase() == low))
        .filter(|e| e.media.matches(&app.own_lists.find))
        .take(40)
        .map(|e| (Rc::new(e.media.clone()), e.media.display_title(lang).to_owned()))
        .collect();
    if found.is_empty() {
        widgets::dim(ui, &p, t("Nothing found"));
        return;
    }
    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
        for (m, title) in found {
            let label = format!("{}  {title}", icons::PLUS);
            if ui.add(egui::Button::new(label).frame(false)).clicked() {
                app.act(Act::ToggleList(m, name.to_owned()));
            }
        }
    });
}

/// Every title of the list, anime first, in the order the user sorts by.
fn rows_of(app: &mut App, name: &str) -> Rc<Vec<Item>> {
    let key = (app.lib_version, name.to_lowercase(), app.s.sort, app.s.sort_asc, app.s.title_language);
    if let Some(c) = &app.own_lists.cache
        && c.key == key
    {
        return c.items.clone();
    }
    let mut items = Vec::new();
    for k in MediaKind::ALL {
        let found: Vec<&Rc<ListEntry>> = app
            .library(k)
            .iter()
            .filter(|e| app.s.show_adult || !e.media.adult)
            .filter(|e| e.lists().iter().any(|x| x.to_lowercase() == key.1))
            .collect();
        let mut sorted: Vec<&ListEntry> = found.iter().map(|e| &***e).collect();
        list::sort(&mut sorted, key.2, key.3, key.4);
        let by_ptr: HashMap<*const ListEntry, &Rc<ListEntry>> =
            found.iter().map(|e| (Rc::as_ptr(e), *e)).collect();
        for e in sorted {
            if let Some(rc) = by_ptr.get(&std::ptr::from_ref(e)) {
                items.push(Item { media: Rc::new(e.media.clone()), entry: Some((*rc).clone()), rank: None });
            }
        }
    }
    let items = Rc::new(items);
    app.own_lists.cache = Some(OwnListsCache { key, items: items.clone() });
    items
}
