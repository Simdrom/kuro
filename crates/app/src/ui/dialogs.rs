//! Editor, confirmations, command palette, sign-in progress and toasts.

use eframe::egui::{self, Align, Align2, Color32, Context, Id, Key, Layout, Modal, RichText, Ui, vec2};
use kuro_core::{FuzzyDate, ListStatus, MediaKind, progress};

use crate::app::{Act, App, Route, SettingsPage, Tab, today};
use crate::i18n::{decimal, t, tf};
use crate::theme;
use crate::widgets;
use crate::{format, icons};

pub fn show(app: &mut App, ctx: &Context) {
    editor(app, ctx);
    login(app, ctx);
    command(app, ctx);
    super::kioku::picker(app, ctx);
    confirm(app, ctx);
    toast(app, ctx);
}

fn frame(app: &App) -> egui::Frame {
    egui::Frame::new()
        .fill(app.p.bg)
        .stroke(egui::Stroke::new(1.0, app.p.line))
        .corner_radius(16)
        .inner_margin(18)
}

fn counter(ui: &mut Ui, app_p: &theme::Palette, label: &str, value: &mut u32, max: Option<u32>) {
    widgets::row(ui, app_p, "", label, |ui| {
        if widgets::icon_button(ui, app_p, icons::PLUS, &format!("{} +1", label)).clicked() {
            *value = value.saturating_add(1);
        }
        if let Some(m) = max {
            widgets::dim(ui, app_p, format!("/ {m}"));
        }
        let top = max.unwrap_or(100_000);
        let r = ui.add(egui::DragValue::new(value).range(0..=top).speed(0.2));
        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::SpinButton, true, label));
        if widgets::icon_button(ui, app_p, icons::MINUS, &format!("{} -1", label)).clicked() {
            *value = value.saturating_sub(1);
        }
    });
}

fn date_row(ui: &mut Ui, p: &theme::Palette, label: &str, text: &mut String) -> bool {
    let empty = text.trim().is_empty();
    let valid = empty || FuzzyDate::parse(text).is_some_and(|d| d.iso() == text.trim());
    widgets::row(ui, p, "", label, |ui| {
        if widgets::icon_button_colored(ui, p, icons::X, t("Clear"), p.dim, false).clicked() {
            text.clear();
        }
        if widgets::icon_button_colored(ui, p, icons::CALENDAR, t("Today"), p.tint, false).clicked() {
            *text = today().to_string();
        }
        let color = if valid { p.text } else { p.red };
        let r = ui.add(
            egui::TextEdit::singleline(text)
                .hint_text(t("YYYY-MM-DD"))
                .desired_width(124.0)
                .char_limit(10)
                .text_color(color),
        );
        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, label));
    });
    valid
}

fn editor(app: &mut App, ctx: &Context) {
    let Some(mut ed) = app.editor.take() else { return };
    let p = app.p;
    let mut names = kuro_core::list_names(app.library(ed.entry.media.kind).iter().map(|e| &**e));
    names.extend(ed.entry.lists().iter().filter(|n| !names.contains(n)).cloned().collect::<Vec<_>>());
    let (lists_label, can_name) = (app.lists_label(), app.can_name_lists());
    let (mut close, mut save, mut delete) = (false, false, false);
    let mut valid = true;
    let modal = Modal::new(Id::unique("editor")).frame(frame(app)).show(ctx, |ui| {
        ui.set_width(430.0_f32.min((ctx.content_rect().width() - 64.0).max(200.0)));
        let m = ed.entry.media.clone();
        let kind = m.kind;
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(44.0, 62.0), egui::Sense::hover());
            app.tex.paint(ui, &p, &app.bg, m.cover.as_deref(), rect, 6);
            ui.vertical(|ui| {
                let head = if ed.is_new { t("Add to list") } else { t("Edit") };
                ui.label(RichText::new(head).size(12.5).color(p.dim));
                let name = m.display_title(app.s.title_language);
                ui.add(egui::Label::new(RichText::new(name).font(theme::bold(17.0))).wrap());
            });
        });
        ui.add_space(8.0);
        egui::ScrollArea::vertical().max_height((ctx.content_rect().height() - 230.0).max(120.0)).show(
            ui,
            |ui| {
                let e = &mut ed.entry;
                widgets::card(ui, &p, |ui| {
                    let before = e.status;
                    let mut status = e.status;
                    let label = t("Status");
                    widgets::select_row(ui, &p, "", label, &mut status, &ListStatus::ALL, |s| {
                        format::status(s, kind).to_owned()
                    });
                    if status != before {
                        *e = progress::set_status(e, status, today());
                        let text = |d: Option<FuzzyDate>| d.map(|d| d.iso()).unwrap_or_default();
                        ed.start = text(e.start);
                        ed.finish = text(e.finish);
                    }
                    widgets::separator(ui, &p);
                    let total = e.media.total();
                    if kind == MediaKind::Manga {
                        counter(ui, &p, t("Volumes"), &mut e.progress_volumes, e.media.volumes);
                        widgets::separator(ui, &p);
                    }
                    let label = if kind == MediaKind::Anime { t("Episodes") } else { t("Chapters") };
                    counter(ui, &p, label, &mut e.progress, total);
                });
                ui.add_space(8.0);
                widgets::card(ui, &p, |ui| {
                    widgets::row(ui, &p, icons::STAR, t("Score"), |ui| {
                        let mut v = e.score10();
                        let shown = if e.score == 0 { "–".into() } else { decimal(v, 1) };
                        ui.add_sized(
                            [34.0, 20.0],
                            egui::Label::new(RichText::new(shown).font(theme::bold(15.0))),
                        );
                        ui.spacing_mut().slider_width = (ui.available_width() - 60.0).clamp(60.0, 150.0);
                        let r = ui.add(egui::Slider::new(&mut v, 0.0..=10.0).step_by(0.5).show_value(false));
                        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, true, t("Score")));
                        if r.changed() {
                            e.score = (v * 10.0).round() as u8;
                        }
                    });
                    if app.s.save_both || app.s.primary == kuro_core::Service::Mal {
                        widgets::note(ui, &p, t("MyAnimeList rounds scores to whole numbers."));
                    }
                });
                ui.add_space(8.0);
                widgets::card(ui, &p, |ui| {
                    valid &= date_row(ui, &p, t("Started"), &mut ed.start);
                    widgets::separator(ui, &p);
                    valid &= date_row(ui, &p, t("Finished"), &mut ed.finish);
                });
                ui.add_space(8.0);
                widgets::card(ui, &p, |ui| {
                    let label = if kind == MediaKind::Anime { t("Rewatching") } else { t("Rereading") };
                    if widgets::toggle_row(ui, &p, icons::REPEAT, label, &mut e.repeating) && e.repeating {
                        *e = progress::start_repeat(e);
                    }
                    widgets::separator(ui, &p);
                    let label =
                        if kind == MediaKind::Anime { t("Times rewatched") } else { t("Times reread") };
                    counter(ui, &p, label, &mut e.repeat_count, None);
                });
                ui.add_space(8.0);
                widgets::card(ui, &p, |ui| {
                    lists_card(ui, &p, e, &names, &mut ed.new_list, lists_label, can_name);
                });
                ui.add_space(8.0);
                widgets::card(ui, &p, |ui| {
                    ui.label(t("Notes"));
                    let w = ui.available_width();
                    let r = ui.add(
                        egui::TextEdit::multiline(&mut e.notes)
                            .desired_rows(3)
                            .desired_width(w)
                            .char_limit(5000),
                    );
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, t("Notes")));
                });
            },
        );
        ui.add_space(12.0);
        if !valid {
            ui.label(RichText::new(t("Write dates as YYYY-MM-DD.")).color(p.red));
        }
        ui.horizontal(|ui| {
            if !ed.is_new
                && widgets::icon_button_colored(ui, &p, icons::TRASH, t("Remove from list…"), p.red, true)
                    .clicked()
            {
                delete = true;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_enabled_ui(valid, |ui| {
                    save = widgets::button(ui, &p, "", t("Save"), true).clicked() && valid;
                });
                close = widgets::button(ui, &p, "", t("Cancel"), false).clicked();
            });
        });
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::Enter)) {
            save = valid;
        }
    });
    if save {
        ed.entry.start = FuzzyDate::parse(&ed.start);
        ed.entry.finish = FuzzyDate::parse(&ed.finish);
        app.act(Act::Save(Box::new(ed.entry)));
    } else if delete {
        app.act(Act::AskDelete(std::rc::Rc::new(ed.entry.media)));
    } else if !close && !modal.should_close() {
        app.editor = Some(ed);
    }
}

fn confirm(app: &mut App, ctx: &Context) {
    let Some(mut c) = app.confirm.take() else { return };
    let p = app.p;
    let (mut yes, mut no) = (false, false);
    let modal = Modal::new(Id::unique("confirm")).frame(frame(app)).show(ctx, |ui| {
        ui.set_width(360.0_f32.min((ctx.content_rect().width() - 64.0).max(200.0)));
        ui.label(RichText::new(&c.title).font(theme::bold(18.0)));
        ui.add_space(4.0);
        ui.add(egui::Label::new(RichText::new(&c.body).color(p.dim)).wrap());
        ui.add_space(14.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let fill = if c.danger { p.red } else { p.tint };
            let b = egui::Button::new(
                RichText::new(&c.action).font(theme::semibold(13.5)).color(theme::on(fill)),
            );
            yes = ui.add(b.fill(fill).min_size(vec2(80.0, 32.0)).corner_radius(9)).clicked();
            no = widgets::button(ui, &p, "", t("Cancel"), false).clicked();
        });
    });
    if yes {
        if let Some(a) = c.act.take() {
            app.act(a);
        }
    } else if no || modal.should_close() {
        if matches!(c.act, Some(Act::AcceptLogin)) {
            app.act(Act::CancelLogin);
        }
    } else {
        app.confirm = Some(c);
    }
}

fn login(app: &mut App, ctx: &Context) {
    let Some(s) = app.login else { return };
    if app.confirm.is_some() {
        return;
    }
    let p = app.p;
    let modal = Modal::new(Id::unique("login")).frame(frame(app)).show(ctx, |ui| {
        ui.set_width(340.0_f32.min((ctx.content_rect().width() - 64.0).max(200.0)));
        ui.vertical_centered(|ui| {
            ui.add(egui::Spinner::new().size(28.0).color(p.tint));
            ui.add_space(8.0);
            ui.label(RichText::new(tf("Signing in to {}", &[&s.name()])).font(theme::bold(18.0)));
            ui.label(RichText::new(t("Finish in your browser. This window updates by itself.")).color(p.dim));
            ui.add_space(12.0);
            widgets::button(ui, &p, "", t("Cancel"), false).clicked()
        })
        .inner
    });
    if modal.inner || modal.should_close() {
        app.act(Act::CancelLogin);
    }
}

/// Ctrl+K: jump anywhere, the desktop stand-in for voice shortcuts.
fn command(app: &mut App, ctx: &Context) {
    let Some(mut text) = app.command.take() else { return };
    let p = app.p;
    let mut picked: Option<Act> = None;
    let modal = Modal::new(Id::unique("command")).frame(frame(app)).show(ctx, |ui| {
        ui.set_width(480.0_f32.min((ctx.content_rect().width() - 64.0).max(200.0)));
        let r = widgets::search_field(ui, &p, &mut text, t("Go to a title, page or link"));
        if !r.has_focus() {
            r.request_focus();
        }
        ui.add_space(6.0);
        let q = text.trim().to_lowercase();
        let mut rows: Vec<(String, &'static str, Act)> = Vec::new();
        if kuro_core::links::parse(text.trim()).is_some() {
            rows.push((t("Open link").into(), icons::LINK, Act::Back));
        }
        if !q.is_empty() {
            for k in MediaKind::ALL {
                let found = app.library(k).iter().filter(|e| e.media.matches(&q)).take(6);
                for e in found {
                    let name = e.media.display_title(app.s.title_language).to_owned();
                    let icon = format::status_icon(e.status);
                    rows.push((name, icon, Act::Open(std::rc::Rc::new(e.media.clone()))));
                }
            }
        }
        for tab in Tab::ALL {
            let name = super::tab_label(tab);
            if name.to_lowercase().contains(&q) {
                rows.push((name.into(), icons::CARET_RIGHT, Act::Go(tab)));
            }
        }
        for page in SettingsPage::ALL {
            let name = super::settings::page_label(page);
            if !q.is_empty() && name.to_lowercase().contains(&q) {
                rows.push((name.into(), icons::GEAR, Act::Push(Route::Settings(page))));
            }
        }
        for k in MediaKind::ALL {
            let name = tf("{} statistics", &[&format::kind(k)]);
            if !q.is_empty() && name.to_lowercase().contains(&q) {
                rows.push((name, icons::CHART_BAR, Act::Push(Route::Stats(k))));
            }
        }
        let kioku = super::kioku::NAME;
        let about = t("kioku.palette");
        if !q.is_empty() && [kioku, about, "crunchyroll"].iter().any(|n| n.to_lowercase().contains(&q)) {
            rows.push((
                format!("{kioku} · {about}"),
                icons::CLOCK_COUNTER_CLOCKWISE,
                Act::Push(Route::Kioku),
            ));
        }
        if !q.is_empty() {
            for k in MediaKind::ALL {
                let label = tf("Search {}: {}", &[&format::kind(k), &text.trim()]);
                rows.push((label, icons::MAGNIFYING_GLASS, Act::Random(k)));
            }
        }
        let id = Id::unique("command-index");
        let mut at: usize = ui.data(|d| d.get_temp(id)).unwrap_or(0);
        if r.changed() {
            at = 0;
        }
        ui.input_mut(|i| {
            if i.consume_key(egui::Modifiers::NONE, Key::ArrowDown) {
                at += 1;
            }
            if i.consume_key(egui::Modifiers::NONE, Key::ArrowUp) {
                at = at.saturating_sub(1);
            }
        });
        at = at.min(rows.len().saturating_sub(1));
        ui.data_mut(|d| d.insert_temp(id, at));
        let enter = ui.input(|i| i.key_pressed(Key::Enter));
        egui::ScrollArea::vertical()
            .max_height(340.0_f32.min((ctx.content_rect().height() - 140.0).max(100.0)))
            .show(ui, |ui| {
                for (i, (name, icon, act)) in rows.into_iter().enumerate() {
                    let hit = widgets::nav_row(ui, &p, icon, p.tint, &name, "", i == at).clicked();
                    if hit || (enter && i == at) {
                        picked = Some(act);
                        if icon == icons::LINK {
                            picked = None;
                            let link = text.clone();
                            app.open_link(&link);
                            text.clear();
                            ui.close();
                        } else if icon == icons::MAGNIFYING_GLASS
                            && let Some(Act::Random(k)) = picked
                        {
                            app.search.kind = k;
                            app.search.text = text.trim().to_owned();
                            app.start_search();
                            picked = Some(Act::Go(Tab::Search));
                        }
                    }
                }
            });
    });
    match picked {
        Some(a) => app.act(a),
        None if modal.should_close() => {}
        None => app.command = Some(text),
    }
}

/// The user's own lists or tags of a title: known names to tick, and a new one.
fn lists_card(
    ui: &mut Ui,
    p: &crate::theme::Palette,
    e: &mut kuro_core::ListEntry,
    names: &[String],
    new: &mut String,
    label: &str,
    can_name: bool,
) {
    ui.label(label);
    if !names.is_empty() {
        ui.horizontal_wrapped(|ui| {
            for n in names {
                let on = e.lists().iter().any(|x| x.to_lowercase() == n.to_lowercase());
                if ui.selectable_label(on, n).clicked() {
                    let mut next: Vec<String> = e.lists().to_vec();
                    if on {
                        next.retain(|x| x.to_lowercase() != n.to_lowercase());
                    } else {
                        next.push(n.clone());
                    }
                    e.lists = Some(kuro_core::clean_lists(next));
                }
            }
        });
    }
    if !can_name {
        widgets::note(ui, p, t("Make new custom lists on AniList; they show here after a refresh."));
        return;
    }
    ui.horizontal(|ui| {
        let r = ui.add(
            egui::TextEdit::singleline(new)
                .hint_text(t("New name"))
                .char_limit(kuro_core::MAX_LIST_NAME)
                .desired_width((ui.available_width() - 90.0).max(80.0)),
        );
        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, t("New name")));
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        if (widgets::button(ui, p, icons::PLUS, t("Add"), false).clicked() || enter) && !new.trim().is_empty()
        {
            let next: Vec<String> = e.lists().iter().cloned().chain([new.trim().to_owned()]).collect();
            e.lists = Some(kuro_core::clean_lists(next));
            new.clear();
        }
    });
}

fn toast(app: &mut App, ctx: &Context) {
    let Some(toast) = &app.toast else { return };
    let now = ctx.input(|i| i.time);
    if now > toast.until {
        app.toast = None;
        return;
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(200));
    let p = app.p;
    let (text, can_undo) = (toast.text.clone(), toast.undo.is_some());
    let fill = if toast.error { p.red } else { theme::blend(p.card, p.text, 0.12) };
    let mut undo = false;
    let r = egui::Area::new(Id::unique("toast"))
        .anchor(Align2::CENTER_BOTTOM, vec2(0.0, -26.0))
        .order(egui::Order::Tooltip)
        // The area takes the click itself: interacting with its response
        // again would use its id twice.
        .sense(egui::Sense::click())
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(fill)
                .corner_radius(12)
                .inner_margin(egui::Margin::symmetric(16, 10))
                .show(ui, |ui| {
                    ui.set_max_width(460.0_f32.min(ctx.content_rect().width() - 70.0));
                    let fg: Color32 = theme::on(fill);
                    ui.horizontal(|ui| {
                        let room = ui.available_width() - if can_undo { 80.0 } else { 0.0 };
                        ui.scope(|ui| {
                            ui.set_max_width(room.max(60.0));
                            ui.add(egui::Label::new(RichText::new(&text).color(fg)).wrap());
                        });
                        if can_undo {
                            let label = RichText::new(t("Undo")).color(fg).strong();
                            undo = ui.add(egui::Button::new(label).frame(false)).clicked();
                        }
                    });
                });
        });
    if undo {
        app.act(Act::Undo);
    } else if r.response.clicked() {
        app.toast = None;
    } else if r.response.contains_pointer()
        && let Some(t) = app.toast.as_mut()
    {
        // Kept up while the pointer is on it, so it can still be undone.
        t.until = t.until.max(now + 1.5);
    }
}
