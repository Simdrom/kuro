//! Kioku (記憶): marks episodes watched on streaming platforms, after a review.

use eframe::egui::{self, Align, Context, Id, Key, Layout, Modal, RichText, Role, Ui, WidgetInfo};
use kuro_api::ApiError;
use kuro_core::ListStatus;
use kuro_core::kioku::{Doubt, Platform, Proposal};

use crate::app::{Act, App, PickFrom};
use crate::backend::{KiokuReview, KiokuStep};
use crate::i18n::{t, tf};
use crate::widgets::{self, card, note, row, section, separator};
use crate::{format, icons, theme};

/// The name is the same in every language.
pub const NAME: &str = "Kioku";

/// Longer lists of series not found start folded.
const UNMATCHED_SHOWN: usize = 3;

pub fn show(app: &mut App, ui: &mut Ui) {
    super::header(app, ui, NAME, |_, _| {});
    super::scroll(ui, "kioku", |ui| {
        ui.set_max_width(ui.available_width().min(720.0));
        let p = app.p;
        note(ui, &p, t("kioku.intro"));
        ui.add_space(8.0);
        warning(app, ui);
        crunchyroll(app, ui);
        netflix(app, ui);
        if app.kioku.syncing {
            progress(app, ui);
        }
        if let Some(e) = app.kioku.error.clone() {
            ui.add_space(8.0);
            card(ui, &p, |ui| {
                row(ui, &p, icons::WARNING, &error(&e), |_| {});
            });
        }
        if app.kioku.review.is_some() {
            if let Some(took) = app.kioku.took {
                ui.add_space(8.0);
                note(ui, &p, &tf("kioku.took", &[&format::duration(took)]));
            }
            review(app, ui);
        }
        section(ui, &p, t("kioku.more"));
        card(ui, &p, |ui| {
            for (i, platform) in [Platform::DisneyPlus, Platform::Amazon].into_iter().enumerate() {
                if i > 0 {
                    separator(ui, &p);
                }
                row(ui, &p, icons::MONITOR_PLAY, platform.name(), |ui| widgets::dim(ui, &p, t("kioku.soon")));
            }
        });
    });
}

fn progress(app: &App, ui: &mut Ui) {
    let p = app.p;
    widgets::loading(ui, &p);
    let text = match app.kioku.progress {
        None => t("kioku.syncing").to_owned(),
        Some(KiokuStep::Reading(n)) => tf("kioku.progress.reading", &[&n]),
        Some(KiokuStep::Searching { done, total }) => tf("kioku.progress.searching", &[&done, &total]),
    };
    ui.vertical_centered(|ui| widgets::dim(ui, &p, text));
    if let Some(started) = app.kioku.started {
        let elapsed = ui.input(|i| i.time) - started;
        ui.vertical_centered(|ui| widgets::dim(ui, &p, tf("kioku.elapsed", &[&format::duration(elapsed)])));
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
    }
    if let Some(KiokuStep::Searching { done, total }) = app.kioku.progress
        && total > 0
    {
        ui.add_space(6.0);
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width().min(320.0), 6.0), egui::Sense::hover());
        let rect = rect.translate(egui::vec2((ui.available_width() - rect.width()).max(0.0) / 2.0, 0.0));
        widgets::bar(ui.painter(), rect, done as f32 / total as f32, p.tint, p.raised);
    }
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
        ApiError::Decode(m) => tf("kioku.file.failed", &[m]),
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

fn netflix(app: &mut App, ui: &mut Ui) {
    let p = app.p;
    let platform = Platform::Netflix;
    section(ui, &p, platform.name());
    card(ui, &p, |ui| {
        row(ui, &p, icons::TRAY_ARROW_DOWN, t("kioku.netflix.file"), |ui| {
            let ready = app.bg.online() && !app.kioku.syncing;
            ui.add_enabled_ui(ready, |ui| {
                if widgets::button(ui, &p, icons::TRAY_ARROW_DOWN, t("kioku.import"), true).clicked() && ready
                {
                    app.act(Act::KiokuImport(platform));
                }
            });
        });
    });
    note(ui, &p, t("kioku.netflix.note"));
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
    let show_unmatched = app.kioku.show_unmatched;
    let Some(r) = app.kioku.review.as_mut() else { return };
    if r.changes.is_empty() && r.unmatched.is_empty() {
        ui.add_space(8.0);
        card(ui, &p, |ui| row(ui, &p, icons::CHECK, t("kioku.nothing"), |_| {}));
        return;
    }
    let platform = r.platform.map_or("", Platform::name);
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
                            line = format!("{line}\n{platform}: {s}");
                        }
                        ui.add(egui::Label::new(RichText::new(line).size(12.5).color(p.dim)).wrap());
                        if c.estimated {
                            widgets::dim(ui, &p, t("kioku.estimated"));
                        }
                        for d in &c.doubts {
                            ui.add(
                                egui::Label::new(RichText::new(doubt(d)).size(12.5).color(p.orange)).wrap(),
                            );
                        }
                        if c.confidence < 0.995 {
                            let pct = (c.confidence * 100.0).round() as u32;
                            widgets::dim(ui, &p, tf("kioku.confidence", &[&pct]));
                        }
                        let label = tf("kioku.change_title", &[&title]);
                        if ui.link(t("kioku.change_title.short")).on_hover_text(&label).clicked() {
                            acts.push(Act::KiokuPick(PickFrom::Change(i)));
                        }
                    });
                });
            }
        });
        note(ui, &p, t("kioku.review.note"));
    }
    let mut toggle = false;
    if !r.unmatched.is_empty() {
        section(ui, &p, t("kioku.unmatched"));
        let folded = r.unmatched.len() > UNMATCHED_SHOWN;
        card(ui, &p, |ui| {
            if folded {
                let label = tf("kioku.unmatched.many", &[&r.unmatched.len()]);
                row(ui, &p, icons::MAGNIFYING_GLASS, &label, |ui| {
                    if widgets::button(ui, &p, "", t("kioku.ignore_all"), false).clicked() {
                        acts.push(Act::KiokuDismissAll);
                    }
                    let name = if show_unmatched { t("kioku.hide") } else { t("kioku.show") };
                    toggle = widgets::button(ui, &p, "", name, false).clicked();
                });
            }
            if folded && !show_unmatched {
                return;
            }
            for (i, g) in r.unmatched.iter().enumerate() {
                if i > 0 || folded {
                    separator(ui, &p);
                }
                let label = tf("kioku.unmatched.row", &[&g.query(), &g.items.len()]);
                row(ui, &p, "", &label, |ui| {
                    if widgets::button(ui, &p, "", t("kioku.ignore"), false).clicked() {
                        acts.push(Act::KiokuDismiss(i));
                    }
                    if widgets::button(ui, &p, "", t("kioku.choose"), false).clicked() {
                        acts.push(Act::KiokuPick(PickFrom::Unmatched(i)));
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
    if toggle {
        app.kioku.show_unmatched = !app.kioku.show_unmatched;
    }
    for a in acts {
        app.act(a);
    }
}

/// The title picker, over everything else.
pub fn picker(app: &mut App, ctx: &Context) {
    let Some(mut pk) = app.kioku.picker.take() else { return };
    let p = app.p;
    let lang = app.s.title_language;
    let (mut close, mut search, mut chosen) = (false, false, None);
    let frame = egui::Frame::new()
        .fill(p.bg)
        .stroke(egui::Stroke::new(1.0, p.line))
        .corner_radius(16)
        .inner_margin(18);
    let modal = Modal::new(Id::unique("kioku-picker")).frame(frame).show(ctx, |ui| {
        ui.set_width(460.0_f32.min((ctx.content_rect().width() - 64.0).max(200.0)));
        let head = tf("kioku.pick.title", &[&pk.group.query()]);
        ui.add(egui::Label::new(RichText::new(head).font(theme::bold(17.0))).wrap());
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 110.0).max(80.0);
            let r = ui.add(egui::TextEdit::singleline(&mut pk.text).desired_width(w).char_limit(200));
            r.widget_info(|| WidgetInfo::labeled(Role::TextInput, true, t("kioku.pick.search")));
            search = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            search |=
                widgets::button(ui, &p, icons::MAGNIFYING_GLASS, t("kioku.pick.search"), true).clicked();
        });
        ui.add_space(8.0);
        egui::ScrollArea::vertical().max_height((ctx.content_rect().height() - 220.0).max(100.0)).show(
            ui,
            |ui| match &pk.results {
                _ if pk.busy => widgets::loading(ui, &p),
                None => {}
                Some(Err(e)) => {
                    widgets::dim(ui, &p, error(e));
                }
                Some(Ok(list)) if list.is_empty() => {
                    widgets::dim(ui, &p, t("kioku.pick.none"));
                }
                Some(Ok(list)) => {
                    for m in list {
                        let year = m.start.map(|d| d.year.to_string()).unwrap_or_default();
                        let what = format!("{} {year}", format::format(m.format));
                        let name = m.display_title(lang);
                        if widgets::nav_row(ui, &p, icons::PLUS, p.tint, name, what.trim(), false).clicked() {
                            chosen = Some(m.clone());
                        }
                    }
                }
            },
        );
        ui.add_space(10.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            close = widgets::button(ui, &p, "", t("Cancel"), false).clicked();
        });
    });
    if let Some(m) = chosen {
        app.kioku.picker = Some(pk);
        app.act(Act::KiokuChoose(Box::new(m)));
    } else if !close && !modal.should_close() {
        app.kioku.picker = Some(pk);
        if search {
            app.act(Act::KiokuLookup);
        }
    }
}
