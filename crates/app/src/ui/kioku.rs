//! Kioku (記憶): marks episodes watched on streaming platforms, after a review.

use eframe::egui::{self, Align, Layout, RichText, Role, Ui, WidgetInfo};
use kuro_api::ApiError;
use kuro_core::ListStatus;
use kuro_core::kioku::{Doubt, Platform, Proposal};

use crate::app::{Act, App};
use crate::backend::KiokuReview;
use crate::i18n::{t, tf};
use crate::widgets::{self, card, note, row, section, separator};
use crate::{format, icons, theme};

/// The name is the same in every language.
pub const NAME: &str = "Kioku";

pub fn show(app: &mut App, ui: &mut Ui) {
    super::header(app, ui, NAME, |_, _| {});
    super::scroll(ui, "kioku", |ui| {
        ui.set_max_width(ui.available_width().min(720.0));
        let p = app.p;
        note(ui, &p, t("kioku.intro"));
        ui.add_space(8.0);
        warning(app, ui);
        crunchyroll(app, ui);
        if app.kioku.syncing {
            widgets::loading(ui, &p);
            ui.vertical_centered(|ui| widgets::dim(ui, &p, t("kioku.syncing")));
        }
        if let Some(e) = app.kioku.error.clone() {
            ui.add_space(8.0);
            card(ui, &p, |ui| {
                row(ui, &p, icons::WARNING, &error(&e), |_| {});
            });
        }
        if app.kioku.review.is_some() {
            review(app, ui);
        }
        section(ui, &p, t("kioku.more"));
        card(ui, &p, |ui| {
            for (i, platform) in
                [Platform::Netflix, Platform::DisneyPlus, Platform::Amazon].into_iter().enumerate()
            {
                if i > 0 {
                    separator(ui, &p);
                }
                row(ui, &p, icons::MONITOR_PLAY, platform.name(), |ui| widgets::dim(ui, &p, t("kioku.soon")));
            }
        });
    });
}

fn warning(app: &App, ui: &mut Ui) {
    let p = app.p;
    egui::Frame::new()
        .fill(theme::blend(p.card, p.orange, 0.14))
        .corner_radius(widgets::RADIUS)
        .inner_margin(egui::Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.label(RichText::new(icons::WARNING).font(theme::icon(18.0)).color(p.orange));
                ui.add(egui::Label::new(RichText::new(t("kioku.unofficial")).color(p.text)).wrap());
            });
        });
}

fn error(e: &ApiError) -> String {
    match e {
        ApiError::Unauthorized => t("kioku.expired").into(),
        ApiError::Status(403) => t("kioku.blocked").into(),
        ApiError::Auth(m) if m != "cancelled" => t("kioku.signin.failed").into(),
        e => format::error(e),
    }
}

fn crunchyroll(app: &mut App, ui: &mut Ui) {
    let p = app.p;
    let platform = Platform::Crunchyroll;
    let connected = *app.kioku.connected.get_or_insert_with(|| {
        use kuro_api::kioku::HistorySource;
        app.bg.api.crunchyroll.connected()
    });
    section(ui, &p, platform.name());
    card(ui, &p, |ui| {
        if app.kioku.connecting {
            row(ui, &p, icons::SIGN_IN, t("kioku.connecting"), |ui| {
                ui.add(egui::Spinner::new().size(16.0).color(p.tint));
            });
        } else if connected {
            row(ui, &p, icons::CHECK, t("kioku.connected"), |ui| {
                let busy = app.kioku.syncing;
                ui.add_enabled_ui(!busy, |ui| {
                    if widgets::button(ui, &p, icons::ARROWS_CLOCKWISE, t("kioku.sync"), true).clicked()
                        && !busy
                    {
                        app.act(Act::KiokuSync(platform));
                    }
                });
            });
            separator(ui, &p);
            row(ui, &p, icons::SIGN_OUT, t("kioku.session"), |ui| {
                if widgets::button(ui, &p, "", t("kioku.disconnect"), false).clicked() {
                    app.act(Act::KiokuAskDisconnect(platform));
                }
            });
        } else {
            row(ui, &p, icons::USER, t("kioku.not_connected"), |ui| {
                let online = app.bg.online();
                ui.add_enabled_ui(online, |ui| {
                    if widgets::button(ui, &p, icons::SIGN_IN, t("kioku.connect"), true).clicked() && online {
                        app.act(Act::KiokuConnect(platform));
                    }
                });
            });
        }
    });
    note(ui, &p, t("kioku.signin.note"));
}

/// Where a change comes from, as the platform names it.
fn source(r: &KiokuReview, p: &Proposal) -> Option<String> {
    let i = r.held.iter().find(|i| p.items.contains(&i.external_id))?;
    Some(match (&i.season_title, i.season) {
        (Some(s), _) => s.clone(),
        (None, Some(n)) if !i.movie => format!("{} · {}", i.series_title, tf("kioku.season", &[&n])),
        _ => i.series_title.clone(),
    })
}

fn describe(p: &Proposal) -> String {
    let to = p.after.progress;
    let mut s = match &p.before {
        Some(b) => tf("kioku.change.progress", &[&b.progress, &to]),
        None => tf("kioku.change.new", &[&to]),
    };
    if p.after.status == ListStatus::Completed {
        s.push_str(" · ");
        s.push_str(t("kioku.change.complete"));
    }
    s
}

fn doubt(d: &Doubt) -> String {
    match d {
        Doubt::Unsure => t("kioku.doubt.unsure").into(),
        Doubt::Gap { missing } => tf("kioku.doubt.gap", &[missing]),
        Doubt::Status(ListStatus::Dropped) => t("kioku.doubt.dropped").into(),
        Doubt::Status(_) => t("kioku.doubt.paused").into(),
        Doubt::Outside { count } => tf("kioku.doubt.outside", &[count]),
    }
}

fn review(app: &mut App, ui: &mut Ui) {
    let p = app.p;
    let lang = app.s.title_language;
    let Some(r) = app.kioku.review.as_mut() else { return };
    if r.changes.is_empty() && r.unmatched.is_empty() {
        ui.add_space(8.0);
        card(ui, &p, |ui| row(ui, &p, icons::CHECK, t("kioku.nothing"), |_| {}));
        return;
    }
    let mut acts = Vec::new();
    if !r.changes.is_empty() {
        section(ui, &p, t("kioku.to_mark"));
        let sources: Vec<Option<String>> = r.changes.iter().map(|c| source(r, c)).collect();
        card(ui, &p, |ui| {
            for (i, c) in r.changes.iter_mut().enumerate() {
                if i > 0 {
                    separator(ui, &p);
                }
                let title = c.media.display_title(lang).to_owned();
                ui.horizontal_top(|ui| {
                    let mut on = c.selected;
                    let resp = ui.add(egui::Checkbox::without_text(&mut on));
                    resp.widget_info(|| WidgetInfo::selected(Role::CheckBox, true, on, &title));
                    c.selected = on;
                    ui.vertical(|ui| {
                        ui.add(egui::Label::new(RichText::new(&title).font(theme::semibold(14.0))).wrap());
                        let mut line = describe(c);
                        if let Some(s) = &sources[i] {
                            line = format!("{line}\n{}: {s}", Platform::Crunchyroll.name());
                        }
                        ui.add(egui::Label::new(RichText::new(line).size(12.5).color(p.dim)).wrap());
                        for d in &c.doubts {
                            ui.add(
                                egui::Label::new(RichText::new(doubt(d)).size(12.5).color(p.orange)).wrap(),
                            );
                        }
                        if c.confidence < 0.995 {
                            let pct = (c.confidence * 100.0).round() as u32;
                            widgets::dim(ui, &p, tf("kioku.confidence", &[&pct]));
                        }
                    });
                });
            }
        });
        note(ui, &p, t("kioku.review.note"));
    }
    if !r.unmatched.is_empty() {
        section(ui, &p, t("kioku.unmatched"));
        card(ui, &p, |ui| {
            for (i, g) in r.unmatched.iter().enumerate() {
                if i > 0 {
                    separator(ui, &p);
                }
                let label = tf("kioku.unmatched.row", &[&g.query(), &g.items.len()]);
                row(ui, &p, icons::MAGNIFYING_GLASS, &label, |ui| {
                    if widgets::button(ui, &p, "", t("kioku.ignore"), false).clicked() {
                        acts.push(Act::KiokuDismiss(i));
                    }
                });
            }
        });
        note(ui, &p, t("kioku.unmatched.note"));
    }
    let n = r.selected();
    let dismissed = !r.dismissed.is_empty() || !r.settled.is_empty();
    ui.add_space(12.0);
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let ready = n > 0 || dismissed;
        ui.add_enabled_ui(ready, |ui| {
            if widgets::button(ui, &p, icons::CHECK, &tf("kioku.apply", &[&n]), true).clicked() && ready {
                acts.push(Act::KiokuAskApply);
            }
        });
        if widgets::button(ui, &p, "", t("kioku.discard"), false).clicked() {
            app.kioku.review = None;
        }
    });
    for a in acts {
        app.act(a);
    }
}
