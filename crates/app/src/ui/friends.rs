//! Friends: what other people on AniList or MyAnimeList are watching, and
//! their profiles.

use std::rc::Rc;

use eframe::egui::{self, Align, Align2, CursorIcon, Key, Layout, Rect, RichText, Sense, Ui, pos2, vec2};
use kuro_api::ApiError;
use kuro_core::friends::{Friend, FriendProfile, in_common};
use kuro_core::{ListEntry, ListStatus, MediaKind, Service, stats};

use super::extra::{cell_base, cells};
use super::rows;
use crate::app::{Act, App, Route, Tab};
use crate::i18n::{decimal, t, tf};
use crate::theme::{self, Palette};
use crate::{format, icons, widgets};

/// Width of a poster on a shelf, and the room between two.
const TILE: f32 = 104.0;
const GAP: f32 = 12.0;

pub fn show(app: &mut App, ui: &mut Ui) {
    let p = app.p;
    ui.horizontal(|ui| {
        ui.set_min_height(36.0);
        rows::trailing(ui, |ui| {
            if !app.s.friends.is_empty() {
                let label = t("Refresh friends");
                if widgets::icon_button_colored(ui, &p, icons::ARROWS_CLOCKWISE, label, p.tint, false)
                    .clicked()
                {
                    app.act(Act::RefreshFriends(None));
                }
            }
            if app.bg.api.is_signed_in(Service::AniList) {
                if app.following_busy {
                    ui.add(egui::Spinner::new().size(18.0).color(p.dim));
                } else {
                    let label = t("Add the people I follow on AniList");
                    if widgets::icon_button_colored(ui, &p, icons::USERS, label, p.tint, false).clicked() {
                        app.act(Act::ImportFollowing);
                    }
                }
            }
            super::heading(ui, &p, super::tab_label(Tab::Friends));
        });
    });
    ui.add_space(4.0);
    add_box(app, ui);
    if app.s.friends.is_empty() {
        widgets::empty(ui, &p, icons::USERS, t("No friends yet"));
        ui.vertical_centered(|ui| {
            widgets::note(
                ui,
                &p,
                t("Add someone by their AniList or MyAnimeList user name, or paste a link to their profile."),
            );
        });
        return;
    }
    super::scroll(ui, "friends", |ui| {
        for f in app.s.friends.clone() {
            ui.add_space(10.0);
            friend_card(app, ui, &f);
        }
    });
}

/// The service, the name and the button that adds them.
fn add_box(app: &mut App, ui: &mut Ui) {
    let p = app.p;
    widgets::card(ui, &p, |ui| {
        let options = [(Service::AniList, Service::AniList.name()), (Service::Mal, Service::Mal.name())];
        widgets::segmented(ui, &p, &mut app.friend_service, &options);
        ui.add_space(6.0);
        // The button goes first so a long translation never pushes it out.
        let row = vec2(ui.available_width(), 32.0);
        ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
            let empty = app.friend_text.trim().is_empty();
            let add =
                ui.add_enabled_ui(!empty, |ui| widgets::button(ui, &p, icons::PLUS, t("Add"), true)).inner;
            let r = ui.add(
                egui::TextEdit::singleline(&mut app.friend_text)
                    .hint_text(t("User name or profile link"))
                    .desired_width(ui.available_width())
                    .char_limit(200),
            );
            let sent = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if (sent || add.clicked()) && !app.friend_text.trim().is_empty() {
                app.act(Act::AddFriend);
            }
        });
    });
}

fn friend_card(app: &mut App, ui: &mut Ui, f: &Friend) {
    let p = app.p;
    app.ensure_friend(f);
    let (data, error, loading) = state(app, f);
    let name = data.as_ref().map_or(f.name.as_str(), |d| d.name.as_str()).to_owned();
    widgets::card(ui, &p, |ui| {
        ui.horizontal(|ui| {
            let (rect, r) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::click());
            avatar(app, ui, data.as_deref(), &name, rect);
            let mut open = r.on_hover_cursor(CursorIcon::PointingHand).clicked();
            ui.vertical(|ui| {
                let title =
                    egui::Label::new(RichText::new(&name).font(theme::semibold(15.0))).sense(Sense::click());
                open |= ui.add(title).on_hover_cursor(CursorIcon::PointingHand).clicked();
                widgets::dim(ui, &p, summary(f, data.as_deref()));
            });
            rows::trailing(ui, |ui| {
                if widgets::icon_button(ui, &p, icons::TRASH, &tf("Remove {}", &[&name])).clicked() {
                    app.act(Act::RemoveFriend(f.clone()));
                }
                if widgets::icon_button(ui, &p, icons::CARET_RIGHT, &tf("Open {}’s profile", &[&name]))
                    .clicked()
                {
                    open = true;
                }
                if loading {
                    ui.add(egui::Spinner::new().size(16.0).color(p.dim));
                }
            });
            if open {
                app.act(Act::Push(Route::Friend(f.clone())));
            }
        });
        if let Some(e) = &error
            && data.is_none()
        {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(icons::WARNING).font(theme::icon(15.0)).color(p.orange));
                ui.add(egui::Label::new(RichText::new(error_text(e)).size(12.5).color(p.dim)).wrap());
            });
            if widgets::button(ui, &p, icons::ARROWS_CLOCKWISE, t("Retry"), false).clicked() {
                app.act(Act::RefreshFriends(Some(f.clone())));
            }
            return;
        }
        let Some(d) = data else { return };
        // As many posters as fit in one row; the profile shows them all.
        let fit = ((ui.available_width() + GAP) / (TILE + GAP)).floor().max(1.0) as usize;
        let current: Vec<ListEntry> = d
            .current(None)
            .into_iter()
            .filter(|e| app.s.show_adult || !e.media.adult)
            .take(fit)
            .cloned()
            .collect();
        ui.add_space(6.0);
        if current.is_empty() {
            widgets::dim(ui, &p, t("Not watching or reading anything right now"));
            return;
        }
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for e in &current {
                rows::shelf_tile(app, ui, &Rc::new(e.media.clone()), &progress(e));
            }
        });
    });
}

/// "AniList · Watching 3 · Reading 1".
fn summary(f: &Friend, data: Option<&FriendProfile>) -> String {
    let mut parts = vec![f.service.name().to_owned()];
    if let Some(d) = data {
        for k in MediaKind::ALL {
            let n = d.current(Some(k)).len();
            if n > 0 {
                parts.push(format!("{} {n}", format::status(ListStatus::Current, k)));
            }
        }
    }
    parts.join(" · ")
}

fn progress(e: &ListEntry) -> String {
    match e.media.kind {
        MediaKind::Anime => tf("Ep {}", &[&e.progress]),
        MediaKind::Manga => tf("Ch {}", &[&e.progress]),
    }
}

fn state(app: &App, f: &Friend) -> (Option<Rc<FriendProfile>>, Option<ApiError>, bool) {
    app.friends.get(f).map_or((None, None, true), |v| (v.data.clone(), v.error.clone(), v.loading))
}

fn error_text(e: &ApiError) -> String {
    match e {
        // MyAnimeList answers 403 for a list that is not public.
        ApiError::NotFound | ApiError::Status(403) => {
            t("No user with this name, or their list is private.").into()
        }
        e => format::error(e),
    }
}

/// A round picture, or the first letter of the name.
fn avatar(app: &mut App, ui: &Ui, data: Option<&FriendProfile>, name: &str, rect: Rect) {
    let p = app.p;
    let radius = (rect.width() / 2.0) as u8;
    if let Some(url) = data.and_then(|d| d.avatar.as_deref()) {
        app.tex.paint(ui, &p, &app.bg, Some(url), rect, radius);
        return;
    }
    ui.painter().circle_filled(rect.center(), rect.width() / 2.0, theme::blend(p.card, p.tint, 0.25));
    let letter: String = name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
    let font = theme::bold(rect.width() * 0.45);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, letter, font, p.tint);
}

/// A friend's page: statistics, what they are on now and the titles you share.
pub fn profile(app: &mut App, ui: &mut Ui, f: &Friend) {
    let p = app.p;
    app.ensure_friend(f);
    let (data, error, loading) = state(app, f);
    let name = data.as_ref().map_or(f.name.as_str(), |d| d.name.as_str()).to_owned();
    super::header(app, ui, &name, |app, ui| {
        if widgets::icon_button_colored(ui, &p, icons::ARROW_SQUARE_OUT, t("Open in browser"), p.tint, false)
            .clicked()
        {
            app.act(Act::Url(f.web_url()));
        }
        if loading {
            ui.add(egui::Spinner::new().size(16.0).color(p.dim));
        } else if widgets::icon_button_colored(ui, &p, icons::ARROWS_CLOCKWISE, t("Refresh"), p.tint, false)
            .clicked()
        {
            app.act(Act::RefreshFriends(Some(f.clone())));
        }
    });
    let Some(d) = data else {
        match error {
            Some(e) => {
                if widgets::failure(ui, &p, &error_text(&e)) {
                    app.act(Act::RefreshFriends(Some(f.clone())));
                }
            }
            None => widgets::loading(ui, &p),
        }
        return;
    };
    super::scroll(ui, ("friend", f), |ui| {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(72.0, 72.0), Sense::hover());
            avatar(app, ui, Some(&d), &name, rect);
            ui.add_space(6.0);
            ui.vertical(|ui| {
                ui.label(RichText::new(&name).font(theme::bold(20.0)));
                widgets::dim(ui, &p, f.service.name());
            });
        });
        stats_cards(app, ui, &d);
        for k in MediaKind::ALL {
            let list: Vec<&ListEntry> =
                d.current(Some(k)).into_iter().filter(|e| app.s.show_adult || !e.media.adult).collect();
            if list.is_empty() {
                continue;
            }
            widgets::section(ui, &p, format::status(ListStatus::Current, k));
            ui.horizontal_wrapped(|ui| {
                ui.add_space(12.0);
                ui.spacing_mut().item_spacing = vec2(12.0, 12.0);
                for e in list {
                    rows::shelf_tile(app, ui, &Rc::new(e.media.clone()), &progress(e));
                }
            });
        }
        shared(app, ui, &p, &d);
    });
}

fn stats_cards(app: &App, ui: &mut Ui, d: &FriendProfile) {
    let p = app.p;
    for k in MediaKind::ALL {
        let s = stats::compute(&d.entries, k);
        if ListStatus::ALL.iter().all(|st| s.get(*st).count == 0) {
            continue;
        }
        widgets::section(ui, &p, &tf("{} statistics", &[&format::kind(k)]));
        widgets::card(ui, &p, |ui| {
            let mut first = true;
            for st in ListStatus::ALL {
                let x = s.get(st);
                if x.count == 0 {
                    continue;
                }
                if !first {
                    widgets::separator(ui, &p);
                }
                first = false;
                let mut value = x.count.to_string();
                if k == MediaKind::Anime && st != ListStatus::Planning && x.days > 0.0 {
                    value = format!("{value}  ·  {}", tf("{} days", &[&decimal(x.days, 1)]));
                }
                widgets::fact(ui, &p, format::status(st, k), &value);
            }
            if let Some(m) = s.mean_score {
                widgets::separator(ui, &p);
                widgets::fact(ui, &p, t("Average score"), &decimal(m, 2));
            }
        });
    }
}

/// Titles on both lists, with both scores.
fn shared(app: &mut App, ui: &mut Ui, p: &Palette, d: &FriendProfile) {
    let mine: Vec<Rc<ListEntry>> = MediaKind::ALL.into_iter().flat_map(|k| app.library(k).to_vec()).collect();
    let mut both: Vec<(Rc<ListEntry>, ListEntry)> = in_common(mine.iter().map(|e| &**e), &d.entries)
        .into_iter()
        .filter(|(m, _)| app.s.show_adult || !m.media.adult)
        .map(|(m, t)| (Rc::new(m.clone()), t.clone()))
        .collect();
    let lang = app.s.title_language;
    both.sort_by(|(a, x), (b, y)| {
        (y.score.min(1), y.score)
            .cmp(&(x.score.min(1), x.score))
            .then_with(|| a.media.display_title(lang).cmp(b.media.display_title(lang)))
    });
    widgets::section(ui, p, &tf("In common · {}", &[&both.len()]));
    if both.is_empty() {
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            widgets::dim(ui, p, t("You have no titles in common yet"));
        });
        return;
    }
    cells(app, ui, both.len(), 300.0, 74.0, |app, ui, rect, i| {
        let (mine, theirs) = &both[i];
        let m = &mine.media;
        let title = m.display_title(app.s.title_language).to_owned();
        let scores = tf("Them {} · You {}", &[&score(theirs.score), &score(mine.score)]);
        let r = cell_base(app, ui, rect, ("common", m.media_ref()), &format!("{title}, {scores}"));
        let pic = Rect::from_min_size(rect.min + vec2(10.0, 8.0), vec2(40.0, 58.0));
        app.tex.paint(ui, &app.p, &app.bg, m.cover.as_deref(), pic, 5);
        let x = pic.right() + 12.0;
        let w = (rect.right() - x - 10.0).max(1.0);
        let y = rect.top() + 10.0;
        let status = format::status(theirs.status, m.kind).to_uppercase();
        let color = app.p.status(theirs.status);
        widgets::paint_text(ui, pos2(x, y), &status, theme::semibold(11.0), color, w, 1);
        widgets::paint_text(ui, pos2(x, y + 16.0), &title, theme::semibold(13.5), app.p.text, w, 1);
        widgets::paint_text(ui, pos2(x, y + 36.0), &scores, theme::regular(12.5), app.p.dim, w, 1);
        if r.clicked() {
            app.act(Act::Open(Rc::new(m.clone())));
        }
    });
}

/// A 0–100 score out of ten, or a dash when there is none.
fn score(s: u8) -> String {
    match s {
        0 => "–".into(),
        s if s % 10 == 0 => (s / 10).to_string(),
        s => decimal(f32::from(s) / 10.0, 1),
    }
}
