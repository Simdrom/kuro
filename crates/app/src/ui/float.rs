//! Always-on-top windows: the binge session and the desktop widget.

use std::rc::Rc;

use eframe::egui::{
    self, Align, CentralPanel, Context, Frame, Layout, RichText, Sense, Ui, ViewportBuilder, ViewportId, vec2,
};
use kuro_core::{ListStatus, MediaKind, stats};

use crate::app::{Act, App};
use crate::i18n::{t, tf};
use crate::theme;
use crate::widgets;
use crate::{format, icons};

fn window(title: &str, size: egui::Vec2) -> ViewportBuilder {
    ViewportBuilder::default()
        .with_title(title)
        .with_inner_size(size)
        .with_min_inner_size(vec2(240.0, 110.0))
        .with_always_on_top()
        .with_maximize_button(false)
        .with_taskbar(false)
}

pub fn show(app: &mut App, ctx: &Context) {
    binge(app, ctx);
    widget(app, ctx);
}

/// Progress of the title being watched right now.
fn binge(app: &mut App, ctx: &Context) {
    let Some(r) = app.binge else { return };
    let Some(e) = app.entry_at(r) else {
        app.binge = None;
        return;
    };
    let p = app.p;
    let builder = window(t("Binge session"), vec2(340.0, 138.0));
    let mut close = false;
    ctx.show_viewport_immediate(ViewportId::from_hash_of("binge"), builder, |ui, _| {
        close |= ui.input(|i| i.viewport().close_requested());
        CentralPanel::no_frame().frame(Frame::new().fill(p.card).inner_margin(12)).show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(66.0, 94.0), Sense::hover());
                app.tex.paint(ui, &p, &app.bg, e.media.cover.as_deref(), rect, 8);
                ui.vertical(|ui| {
                    let name = e.media.display_title(app.s.title_language);
                    ui.add(egui::Label::new(RichText::new(name).font(theme::semibold(14.0))).truncate());
                    let total = e.media.total().map_or("?".into(), |n| n.to_string());
                    ui.label(
                        RichText::new(format!("{} / {total}", e.progress))
                            .font(theme::bold(20.0))
                            .color(p.tint),
                    );
                    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 5.0), Sense::hover());
                    widgets::bar(ui.painter(), bar, e.fraction().unwrap_or(0.0), p.tint, p.raised);
                    ui.horizontal(|ui| {
                        let m = Rc::new(e.media.clone());
                        if e.status == ListStatus::Completed && !e.repeating {
                            ui.label(
                                RichText::new(t("Completed")).font(theme::semibold(13.0)).color(p.green),
                            );
                        } else {
                            if widgets::icon_button(ui, &p, icons::MINUS, t("Remove one")).clicked() {
                                app.act(Act::Step(m.clone(), -1));
                            }
                            if widgets::button(ui, &p, icons::PLUS, t("Next"), true).clicked() {
                                app.act(Act::Step(m, 1));
                            }
                        }
                        if widgets::button(ui, &p, "", t("End"), false).clicked() {
                            close = true;
                        }
                    });
                });
            });
        });
    });
    if close {
        app.binge = None;
    }
}

/// Current titles and counts, pinned over other windows.
fn widget(app: &mut App, ctx: &Context) {
    if !app.widget {
        return;
    }
    let p = app.p;
    let builder = window(t("Desktop widget"), vec2(300.0, 460.0));
    let mut close = false;
    ctx.show_viewport_immediate(ViewportId::from_hash_of("widget"), builder, |ui, _| {
        close |= ui.input(|i| i.viewport().close_requested());
        CentralPanel::no_frame().frame(Frame::new().fill(p.bg).inner_margin(10)).show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.set_max_width(ui.available_width() - 8.0);
                for k in MediaKind::ALL {
                    progress_group(app, ui, k);
                }
                for k in MediaKind::ALL {
                    count_group(app, ui, k);
                }
            });
        });
    });
    if close {
        app.widget = false;
    }
}

fn progress_group(app: &mut App, ui: &mut Ui, k: MediaKind) {
    let p = app.p;
    let mut list: Vec<_> =
        app.library(k).iter().filter(|e| e.status == ListStatus::Current).cloned().collect();
    list.sort_by_key(|e| std::cmp::Reverse(e.updated_at));
    if list.is_empty() {
        return;
    }
    widgets::section(ui, &p, format::kind(k));
    widgets::card(ui, &p, |ui| {
        for (i, e) in list.iter().take(5).enumerate() {
            if i > 0 {
                widgets::separator(ui, &p);
            }
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(34.0, 48.0), Sense::hover());
                app.tex.paint(ui, &p, &app.bg, e.media.cover.as_deref(), rect, 5);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let m = Rc::new(e.media.clone());
                    if widgets::icon_button(ui, &p, icons::PLUS, t("Add one")).clicked() {
                        app.act(Act::Step(m, 1));
                    }
                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                        let name = e.media.display_title(app.s.title_language);
                        ui.add(egui::Label::new(RichText::new(name).font(theme::semibold(12.5))).truncate());
                        let total = e.media.total().map_or("?".into(), |n| n.to_string());
                        widgets::dim(ui, &p, format!("{} / {total}", e.progress));
                        let (bar, _) =
                            ui.allocate_exact_size(vec2(ui.available_width(), 4.0), Sense::hover());
                        widgets::bar(ui.painter(), bar, e.fraction().unwrap_or(0.0), p.tint, p.raised);
                    });
                });
            });
        }
    });
}

fn count_group(app: &mut App, ui: &mut Ui, k: MediaKind) {
    let p = app.p;
    if app.library(k).is_empty() {
        return;
    }
    let s = stats::compute(app.library(k).iter().map(|e| &**e), k);
    widgets::section(ui, &p, &tf("{} statistics", &[&format::kind(k)]));
    widgets::card(ui, &p, |ui| {
        for st in ListStatus::ALL {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format::status_icon(st)).font(theme::icon(15.0)).color(p.status(st)));
                ui.label(RichText::new(format::status(st, k)).color(p.status(st)));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(s.get(st).count.to_string()).font(theme::semibold(13.0)));
                });
            });
        }
    });
}
