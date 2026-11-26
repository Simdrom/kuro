//! Account summary, statistics and every preference.

use eframe::egui::{self, Align2, CornerRadius, RichText, Role, Sense, Ui, WidgetInfo, vec2};
use kuro_api::oauth;
use kuro_core::backup;
use kuro_core::settings::{
    AppIcon, Every, GestureAction, MusicSearch, PosterProgress, ScoreDisplay, Theme, Tint,
};
use kuro_core::{ListStatus, MediaKind, Service, TitleLanguage, stats};

use crate::app::{Act, App, Route, SettingsPage};
use crate::i18n::{LANGUAGES, decimal, t, tf};
use crate::theme;
use crate::widgets::{self, card, note, row, section, select_row, separator, toggle_row};
use crate::{format, icon, icons};

pub fn page_label(p: SettingsPage) -> &'static str {
    match p {
        SettingsPage::General => t("General"),
        SettingsPage::Lists => t("Lists"),
        SettingsPage::Accounts => t("Accounts"),
        SettingsPage::Notifications => t("Notifications"),
        SettingsPage::Gestures => t("Mouse & keyboard"),
        SettingsPage::Appearance => t("Appearance"),
        SettingsPage::Widgets => t("Widget & automation"),
        SettingsPage::Data => t("Data & caches"),
        SettingsPage::About => t("About"),
    }
}

fn page_icon(p: SettingsPage) -> &'static str {
    match p {
        SettingsPage::General => icons::GEAR,
        SettingsPage::Lists => icons::LIST_BULLETS,
        SettingsPage::Accounts => icons::USER,
        SettingsPage::Notifications => icons::BELL,
        SettingsPage::Gestures => icons::MOUSE,
        SettingsPage::Appearance => icons::PALETTE,
        SettingsPage::Widgets => icons::APP_WINDOW,
        SettingsPage::Data => icons::DATABASE,
        SettingsPage::About => icons::INFO,
    }
}

pub fn master(app: &mut App, ui: &mut Ui) {
    let p = app.p;
    widgets::title(ui, &p, t("Settings"));
    ui.add_space(4.0);
    super::scroll(ui, "settings", |ui| {
        account_card(app, ui);
        for k in MediaKind::ALL {
            let s = stats::compute(app.library(k).iter().map(|e| &**e), k);
            section(ui, &p, &tf("{} statistics", &[&format::kind(k)]));
            card(ui, &p, |ui| {
                for (i, st) in ListStatus::ALL.into_iter().enumerate() {
                    if i > 0 {
                        separator(ui, &p);
                    }
                    let x = s.get(st);
                    let mut value = x.count.to_string();
                    if k == MediaKind::Anime && st != ListStatus::Planning {
                        value = format!("{value}  ·  {}", tf("{} days", &[&decimal(x.days, 1)]));
                    }
                    let name = format::status(st, k);
                    if widgets::nav_row(ui, &p, format::status_icon(st), p.status(st), name, &value, false)
                        .clicked()
                    {
                        app.act(Act::ShowStatus(k, st));
                    }
                }
                if let Some(m) = s.mean_score {
                    separator(ui, &p);
                    widgets::fact(ui, &p, t("My average score"), &decimal(m, 2));
                }
            });
        }
        section(ui, &p, t("Preferences"));
        card(ui, &p, |ui| {
            for (i, page) in SettingsPage::ALL.into_iter().enumerate() {
                if i > 0 {
                    separator(ui, &p);
                }
                let first = app.routes.is_empty() && app.can_split && page == SettingsPage::General;
                let on = first || app.routes.last() == Some(&Route::Settings(page));
                if widgets::nav_row(ui, &p, page_icon(page), p.green, page_label(page), "", on).clicked() {
                    app.routes.clear();
                    app.act(Act::Push(Route::Settings(page)));
                }
            }
        });
    });
}

fn account_card(app: &mut App, ui: &mut Ui) {
    let p = app.p;
    let s = app.s.primary;
    let Some(user) = app.profiles.get(&s).cloned() else {
        if !app.bg.api.is_signed_in(s) {
            super::mylist::sign_in_card(app, ui);
        }
        return;
    };
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 150.0), Sense::hover());
    widgets::gradient(ui, rect, p.tint, p.green);
    let pt = ui.painter().clone();
    let c = rect.center();
    let white = egui::Color32::WHITE;
    pt.text(c - vec2(0.0, 56.0), Align2::CENTER_CENTER, s.name(), theme::bold(17.0), white);
    let a = egui::Rect::from_center_size(c + vec2(0.0, 2.0), vec2(72.0, 72.0));
    app.tex.paint(ui, &p, &app.bg, user.avatar.as_deref(), a, 36);
    pt.text(c + vec2(0.0, 57.0), Align2::CENTER_CENTER, &user.name, theme::semibold(14.5), white);
}

pub fn page(app: &mut App, ui: &mut Ui, page: SettingsPage) {
    super::header(app, ui, page_label(page), |_, _| {});
    super::scroll(ui, ("page", page), |ui| {
        ui.set_max_width(ui.available_width().min(720.0));
        match page {
            SettingsPage::General => general(app, ui),
            SettingsPage::Lists => lists(app, ui),
            SettingsPage::Accounts => accounts(app, ui),
            SettingsPage::Notifications => notifications(app, ui),
            SettingsPage::Gestures => gestures(app, ui),
            SettingsPage::Appearance => appearance(app, ui),
            SettingsPage::Widgets => widgets_page(app, ui),
            SettingsPage::Data => data(app, ui),
            SettingsPage::About => about(app, ui),
        }
    });
}

fn general(app: &mut App, ui: &mut Ui) {
    let p = app.p;
    let s = &mut app.s;
    section(ui, &p, t("Language"));
    card(ui, &p, |ui| {
        let current = LANGUAGES.iter().position(|l| l.code == s.language);
        let mut v = current;
        let mut options = vec![None];
        options.extend((0..LANGUAGES.len()).map(Some));
        if select_row(ui, &p, icons::TRANSLATE, t("Interface language"), &mut v, &options, |i| {
            i.map_or(t("System"), |i| LANGUAGES[i].name).to_owned()
        }) {
            s.language = v.map(|i| LANGUAGES[i].code.to_owned()).unwrap_or_default();
        }
        separator(ui, &p);
        let all = [TitleLanguage::Romaji, TitleLanguage::English, TitleLanguage::Native];
        let label = t("Title language");
        select_row(ui, &p, icons::TEXT_AA, label, &mut s.title_language, &all, |v| {
            format::title_language(v).into()
        });
    });
    section(ui, &p, t("Navigation"));
    card(ui, &p, |ui| {
        toggle_row(ui, &p, icons::SIDEBAR_SIMPLE, t("Show sidebar"), &mut s.sidebar);
        separator(ui, &p);
        toggle_row(ui, &p, icons::BOOKS, t("Separate anime and manga tabs"), &mut s.separate_tabs);
        separator(ui, &p);
        toggle_row(ui, &p, icons::SUN, t("Show the current season as icon"), &mut s.season_icon);
        separator(ui, &p);
        let label = t("Open first");
        select_row(ui, &p, icons::HOUSE, label, &mut s.default_kind, &MediaKind::ALL, |k| {
            format::kind(k).into()
        });
    });
    section(ui, &p, t("Display"));
    card(ui, &p, |ui| {
        toggle_row(ui, &p, icons::EYE_SLASH, t("Show adult titles"), &mut s.show_adult);
        separator(ui, &p);
        toggle_row(ui, &p, icons::THUMBS_UP, t("Show recommendations"), &mut s.show_recommendations);
        separator(ui, &p);
        toggle_row(ui, &p, icons::NOTE, t("Show reviews"), &mut s.show_reviews);
        separator(ui, &p);
        toggle_row(ui, &p, icons::CLOCK, t("Show when an entry was updated"), &mut s.show_updated);
        separator(ui, &p);
        toggle_row(ui, &p, icons::WARNING, t("Ask before removing a title"), &mut s.confirm_delete);
    });
    section(ui, &p, t("Openings & endings"));
    card(ui, &p, |ui| {
        toggle_row(ui, &p, icons::MUSIC_NOTES, t("Show openings and endings"), &mut s.show_themes);
        separator(ui, &p);
        let label = t("Find songs on");
        select_row(ui, &p, icons::MAGNIFYING_GLASS, label, &mut s.music, &MusicSearch::ALL, |v| {
            format::music(v).into()
        });
    });
    section(ui, &p, t("Windows"));
    card(ui, &p, |ui| {
        toggle_row(
            ui,
            &p,
            icons::TRAY_ARROW_DOWN,
            t("Keep running in the tray when closed"),
            &mut s.close_to_tray,
        );
        separator(ui, &p);
        toggle_row(ui, &p, icons::PLAY, t("Start with Windows"), &mut s.autostart);
    });
}

fn lists(app: &mut App, ui: &mut Ui) {
    let p = app.p;
    let s = &mut app.s;
    section(ui, &p, t("Lists"));
    card(ui, &p, |ui| {
        row(ui, &p, icons::BOOKS, t("My List"), |ui| super::rows::layout_switch(ui, &p, &mut s.list_layout));
        separator(ui, &p);
        row(ui, &p, icons::STAR, t("Seasonal, ranking and search"), |ui| {
            super::rows::layout_switch(ui, &p, &mut s.browse_layout);
        });
        separator(ui, &p);
        let label = t("Scores");
        select_row(ui, &p, icons::STAR, label, &mut s.score_display, &ScoreDisplay::ALL, |v| {
            format::score_display(v).into()
        });
        separator(ui, &p);
        toggle_row(ui, &p, icons::PENCIL_SIMPLE, t("Show edit button"), &mut s.show_edit_button);
    });
    section(ui, &p, t("Posters"));
    card(ui, &p, |ui| {
        widgets::wide_row(ui, &p, icons::IMAGE, t("Poster size"), |ui| {
            let r =
                ui.add(egui::Slider::new(&mut s.poster_width, 90.0..=240.0).step_by(10.0).show_value(false));
            r.widget_info(|| WidgetInfo::labeled(Role::Slider, true, t("Poster size")));
        });
        separator(ui, &p);
        let label = t("Progress");
        select_row(ui, &p, icons::CHART_BAR, label, &mut s.poster_progress, &PosterProgress::ALL, |v| {
            format::poster_progress(v).into()
        });
        separator(ui, &p);
        toggle_row(ui, &p, icons::STAR, t("Show scores"), &mut s.poster_scores);
    });
}

fn accounts(app: &mut App, ui: &mut Ui) {
    let p = app.p;
    section(ui, &p, t("Data source"));
    card(ui, &p, |ui| {
        let label = t("Primary service");
        select_row(ui, &p, icons::STAR, label, &mut app.s.primary, &Service::ALL, |s| s.name().into());
    });
    note(ui, &p, t("Lists, search, rankings and details come from your primary service."));
    section(ui, &p, t("Save changes to"));
    card(ui, &p, |ui| {
        for (both, name, body) in [
            (false, t("Only the primary service"), t("Changes are saved to your primary service.")),
            (true, t("Both services"), t("Every change is saved to AniList and MyAnimeList.")),
        ] {
            if both {
                separator(ui, &p);
            }
            let on = app.s.save_both == both;
            let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::click());
            r.widget_info(|| WidgetInfo::selected(Role::RadioButton, true, on, name));
            if r.hovered() || r.has_focus() {
                ui.painter().rect_filled(rect.expand2(vec2(8.0, 0.0)), CornerRadius::same(8), p.hover);
            }
            let w = rect.width() - 30.0;
            widgets::paint_text(ui, rect.min + vec2(0.0, 5.0), name, theme::regular(14.0), p.text, w, 1);
            widgets::paint_text(ui, rect.min + vec2(0.0, 26.0), body, theme::regular(12.0), p.dim, w, 1);
            if on {
                let c = egui::pos2(rect.right() - 10.0, rect.center().y);
                ui.painter().text(c, Align2::CENTER_CENTER, icons::CHECK, theme::icon(17.0), p.tint);
            }
            if r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                app.s.save_both = both;
            }
        }
    });
    for s in Service::ALL {
        service(app, ui, s);
    }
    section(ui, &p, super::kioku::NAME);
    card(ui, &p, |ui| {
        let name = super::kioku::NAME;
        if widgets::nav_row(ui, &p, icons::CLOCK_COUNTER_CLOCKWISE, p.tint, name, "Crunchyroll", false)
            .clicked()
        {
            app.act(Act::Push(Route::Kioku));
        }
    });
    note(ui, &p, t("kioku.settings.note"));
}

fn service(app: &mut App, ui: &mut Ui, s: Service) {
    let p = app.p;
    section(ui, &p, s.name());
    let signed = app.bg.api.is_signed_in(s);
    card(ui, &p, |ui| {
        if signed {
            let name = app.profiles.get(&s).map_or("…".to_owned(), |u| u.name.clone());
            row(ui, &p, icons::USER, &name, |ui| {
                if widgets::button(ui, &p, icons::SIGN_OUT, t("Sign out"), false).clicked() {
                    app.act(Act::AskSignOut(s));
                }
            });
            if app.sync_error.as_ref().is_some_and(|(x, e)| *x == s && *e == kuro_api::ApiError::Unauthorized)
            {
                separator(ui, &p);
                widgets::wide_row(
                    ui,
                    &p,
                    icons::WARNING,
                    t("Your session has expired. Sign in again."),
                    |ui| {
                        if widgets::button(ui, &p, icons::SIGN_IN, t("Sign in"), true).clicked() {
                            app.act(Act::Login(s));
                        }
                    },
                );
            }
            return;
        }
        let ready = app.bg.api.can_sign_in(s);
        widgets::wide_row(ui, &p, icons::USER, t("Not signed in"), |ui| {
            ui.add_enabled_ui(ready, |ui| {
                if widgets::button(ui, &p, icons::SIGN_IN, t("Sign in"), true).clicked() && ready {
                    app.act(Act::Login(s));
                }
            });
        });
        separator(ui, &p);
        let id = match s {
            Service::AniList => &mut app.s.anilist_client_id,
            Service::Mal => &mut app.s.mal_client_id,
        };
        widgets::wide_row(ui, &p, icons::KEY, t("Client ID"), |ui| {
            let w = (ui.available_width() - 40.0).clamp(80.0, 220.0);
            let r = ui.add(egui::TextEdit::singleline(id).desired_width(w).char_limit(128));
            let label = format!("{} {}", s.name(), t("Client ID"));
            r.widget_info(|| WidgetInfo::labeled(Role::TextInput, true, &label));
            // A pasted id may carry spaces or a line break.
            id.retain(|c| c.is_ascii_alphanumeric());
        });
        separator(ui, &p);
        let redirect = oauth::redirect_uri(s);
        widgets::wide_row(ui, &p, icons::LINK, t("Redirect URL"), |ui| {
            if widgets::icon_button(ui, &p, icons::COPY, t("Copy")).clicked() {
                app.act(Act::Copy(redirect.clone()));
            }
            ui.add(egui::Label::new(RichText::new(&redirect).size(12.5).color(p.dim)).selectable(true));
        });
    });
    if !signed {
        let (text, url) = match s {
            Service::AniList => (
                t("Create a client on AniList with this redirect URL, then paste its ID here. It is free."),
                "https://anilist.co/settings/developer",
            ),
            Service::Mal => (
                t(
                    "Create a client on MyAnimeList (app type “other”) with this redirect URL, then paste its ID here. It is free.",
                ),
                "https://myanimelist.net/apiconfig",
            ),
        };
        note(ui, &p, text);
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            if ui.link(t("Open the developer page")).on_hover_text(url).clicked() {
                app.act(Act::Url(url.into()));
            }
        });
    }
}

fn notifications(app: &mut App, ui: &mut Ui) {
    let p = app.p;
    let n = &mut app.s.notifications;
    section(ui, &p, t("Notifications"));
    card(ui, &p, |ui| {
        toggle_row(ui, &p, icons::BELL, t("Notifications"), &mut n.enabled);
        ui.add_enabled_ui(n.enabled, |ui| {
            separator(ui, &p);
            toggle_row(ui, &p, icons::PLAY, t("Every new episode"), &mut n.episodes);
            separator(ui, &p);
            toggle_row(ui, &p, icons::SPARKLE, t("Premieres"), &mut n.premieres);
            separator(ui, &p);
            toggle_row(ui, &p, icons::FLAG, t("Finales"), &mut n.finished);
            separator(ui, &p);
            toggle_row(
                ui,
                &p,
                icons::CALENDAR_BLANK,
                t("Include titles I plan to watch"),
                &mut n.include_planning,
            );
        });
    });
    section(ui, &p, t("Time"));
    card(ui, &p, |ui| {
        let delays = [0, 15, 30, 60, 120, 180, 360];
        let label = t("Delay after airing");
        select_row(ui, &p, icons::CLOCK, label, &mut n.delay_minutes, &delays, |m| match m {
            0 => t("None").to_owned(),
            m => tf("{} min", &[&m]),
        });
        separator(ui, &p);
        let hours: Vec<u32> = (0..24).collect();
        let label = t("Preferred hour when only the date is known");
        select_row(ui, &p, icons::CLOCK, label, &mut n.date_hour, &hours, |h| format!("{h:02}:00"));
    });
    note(ui, &p, t("Notifications arrive while the app is open or in the tray."));
    let muted = n.muted.clone();
    if !muted.is_empty() {
        section(ui, &p, t("Muted"));
        card(ui, &p, |ui| {
            for r in muted {
                let name = app.entry_at(r).map_or_else(
                    || format!("{} {}", r.service.name(), r.id),
                    |e| e.media.display_title(app.s.title_language).to_owned(),
                );
                row(ui, &p, icons::BELL_SLASH, &name, |ui| {
                    if widgets::icon_button(ui, &p, icons::BELL, t("Turn on notifications")).clicked() {
                        app.act(Act::ToggleMute(r));
                    }
                });
            }
        });
    }
    ui.add_space(10.0);
    if widgets::button(ui, &p, icons::BELL, t("Send a test notification"), false).clicked() {
        app.sys.notify(crate::system::Notice::new("Kuro", t("Notifications are working.")));
    }
}

fn gestures(app: &mut App, ui: &mut Ui) {
    let p = app.p;
    let g = &mut app.s.gestures;
    section(ui, &p, t("On a title"));
    card(ui, &p, |ui| {
        let all = &GestureAction::ALL;
        let name = |v| format::gesture(v).to_owned();
        select_row(ui, &p, icons::MOUSE_LEFT_CLICK, t("Double-click"), &mut g.double_click, all, name);
        separator(ui, &p);
        select_row(ui, &p, icons::MOUSE_MIDDLE_CLICK, t("Middle-click"), &mut g.middle_click, all, name);
        separator(ui, &p);
        select_row(ui, &p, icons::HAND_SWIPE_RIGHT, t("Drag right"), &mut g.swipe_right, all, name);
        separator(ui, &p);
        select_row(ui, &p, icons::HAND_SWIPE_LEFT, t("Drag left"), &mut g.swipe_left, all, name);
    });
    note(
        ui,
        &p,
        t("Right-click always opens the full menu. Dragging a row sideways also works on touch screens."),
    );
    section(ui, &p, t("Keyboard"));
    card(ui, &p, |ui| {
        let keys = [
            ("Ctrl+K", t("Go to anything")),
            ("Ctrl+1 … 6", t("Switch tabs")),
            ("Ctrl+F", t("Search")),
            ("Ctrl+,", t("Settings")),
            ("F5", t("Refresh")),
            ("+ / -", t("Add or remove one on the open title")),
            ("E", t("Edit the open title")),
            ("Esc", t("Back")),
        ];
        for (i, (k, v)) in keys.into_iter().enumerate() {
            if i > 0 {
                separator(ui, &p);
            }
            widgets::fact(ui, &p, v, k);
        }
    });
}

fn appearance(app: &mut App, ui: &mut Ui) {
    let p = app.p;
    let s = &mut app.s;
    section(ui, &p, t("Theme"));
    let themes = Theme::ALL.map(|v| (v, format::theme(v)));
    widgets::segmented(ui, &p, &mut s.theme, &themes);
    section(ui, &p, t("Tint"));
    card(ui, &p, |ui| {
        ui.horizontal_wrapped(|ui| {
            for v in Tint::ALL {
                let on = s.tint == v;
                let c = theme::tint_color(v, p.dark);
                let (rect, r) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::click());
                r.widget_info(|| WidgetInfo::selected(Role::RadioButton, true, on, format::tint(v)));
                ui.painter().circle_filled(rect.center(), 14.0, c);
                if on || r.hovered() || r.has_focus() {
                    let w = if on { 2.5 } else { 1.0 };
                    ui.painter().circle_stroke(rect.center(), 18.0, egui::Stroke::new(w, c));
                }
                if on {
                    let font = theme::icon(15.0);
                    ui.painter().text(rect.center(), Align2::CENTER_CENTER, icons::CHECK, font, theme::on(c));
                }
                if r.on_hover_text(format::tint(v)).clicked() {
                    s.tint = v;
                }
            }
        });
    });
    section(ui, &p, t("App icon"));
    card(ui, &p, |ui| {
        ui.horizontal_wrapped(|ui| {
            for v in AppIcon::ALL {
                let on = s.icon == v;
                let (rect, r) = ui.allocate_exact_size(vec2(78.0, 92.0), Sense::click());
                r.widget_info(|| WidgetInfo::selected(Role::RadioButton, true, on, format::app_icon(v)));
                let id = egui::Id::unique(("app-icon", v as u8));
                let tex = ui.data(|d| d.get_temp::<egui::TextureHandle>(id)).unwrap_or_else(|| {
                    let i = icon::render(v, 128);
                    let img = egui::ColorImage::from_rgba_unmultiplied([128, 128], &i.rgba);
                    let h = ui.ctx().load_texture("app-icon", img, egui::TextureOptions::LINEAR);
                    ui.data_mut(|d| d.insert_temp(id, h.clone()));
                    h
                });
                let pic = egui::Rect::from_center_size(rect.center() - vec2(0.0, 10.0), vec2(60.0, 60.0));
                egui::Image::from_texture(&tex).paint_at(ui, pic);
                if on || r.hovered() || r.has_focus() {
                    let st = egui::Stroke::new(if on { 2.5 } else { 1.0 }, p.tint);
                    ui.painter().rect_stroke(pic.expand(4.0), 18, st, egui::StrokeKind::Outside);
                }
                let at = rect.center_bottom() - vec2(0.0, 8.0);
                ui.painter().text(
                    at,
                    Align2::CENTER_CENTER,
                    format::app_icon(v),
                    theme::regular(12.0),
                    p.text,
                );
                if r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    s.icon = v;
                }
            }
        });
    });
    section(ui, &p, t("Interface"));
    card(ui, &p, |ui| {
        let zooms = [0.8, 0.9, 1.0, 1.1, 1.25, 1.5];
        let label = t("Interface size");
        select_row(ui, &p, icons::TEXT_AA, label, &mut s.zoom, &zooms, |z| {
            format!("{} %", (z * 100.0).round())
        });
        separator(ui, &p);
        toggle_row(ui, &p, icons::SPARKLE, t("Animations"), &mut s.animations);
        separator(ui, &p);
        toggle_row(ui, &p, icons::CONFETTI, t("Confetti when I complete a title"), &mut s.confetti);
    });
}

fn widgets_page(app: &mut App, ui: &mut Ui) {
    let p = app.p;
    section(ui, &p, t("Desktop widget"));
    card(ui, &p, |ui| {
        let w = &mut app.s.widget;
        toggle_row(ui, &p, icons::PUSH_PIN, t("Show the desktop widget"), &mut w.open);
        separator(ui, &p);
        toggle_row(ui, &p, icons::CALENDAR_BLANK, t("Next episodes"), &mut w.up_next);
        separator(ui, &p);
        toggle_row(ui, &p, icons::PLAY, t("Watching and reading"), &mut w.watching);
        separator(ui, &p);
        toggle_row(ui, &p, icons::CHART_BAR, t("Statistics"), &mut w.stats);
        separator(ui, &p);
        let rows = [3, 5, 8, 10];
        select_row(ui, &p, icons::LIST_BULLETS, t("Titles per group"), &mut w.rows, &rows, |n| n.to_string());
    });
    note(
        ui,
        &p,
        t(
            "A small window above the others with your progress and statistics. It opens again with Kuro if you leave it open.",
        ),
    );
    backup_section(app, ui);
    section(ui, &p, t("Sync"));
    card(ui, &p, |ui| {
        let hours = [0, 1, 3, 6, 12, 24];
        let label = t("Bring in changes made elsewhere");
        select_row(ui, &p, icons::CLOUD_ARROW_UP, label, &mut app.s.refresh_hours, &hours, |h| match h {
            0 => t("Off").to_owned(),
            1 => t("Every hour").to_owned(),
            h => tf("Every {} hours", &[&h]),
        });
    });
    note(
        ui,
        &p,
        t(
            "Your list is read again from your account, so changes made on the website or another device show up without refreshing. Your own changes are always sent right away.",
        ),
    );
    calendar_section(app, ui);
    reminders_section(app, ui);
    section(ui, &p, super::kioku::NAME);
    card(ui, &p, |ui| {
        let label = t("kioku.auto");
        select_row(ui, &p, icons::CLOCK_COUNTER_CLOCKWISE, label, &mut app.s.kioku_check, &Every::ALL, |v| {
            format::every(v).into()
        });
        separator(ui, &p);
        toggle_row(ui, &p, icons::TRAY_ARROW_DOWN, t("kioku.downloads"), &mut app.s.watch_downloads);
    });
    note(ui, &p, t("kioku.auto.note"));
    links_section(app, ui);
}

fn reminders_section(app: &mut App, ui: &mut Ui) {
    let p = app.p;
    let n = &mut app.s.notifications;
    section(ui, &p, t("Reminders"));
    card(ui, &p, |ui| {
        ui.add_enabled_ui(n.enabled, |ui| {
            let days = [0, 7, 14, 30];
            let label = t("Titles I stopped watching or reading");
            select_row(
                ui,
                &p,
                icons::CLOCK_COUNTER_CLOCKWISE,
                label,
                &mut n.stale_days,
                &days,
                |d| match d {
                    0 => t("Off").to_owned(),
                    d => tf("After {} days", &[&d]),
                },
            );
            separator(ui, &p);
            toggle_row(ui, &p, icons::CALENDAR, t("A summary of the week on Monday"), &mut n.weekly);
        });
    });
    if !n.enabled {
        note(ui, &p, t("Reminders are notifications: turn notifications on to get them."));
    }
}

fn calendar_section(app: &mut App, ui: &mut Ui) {
    let p = app.p;
    section(ui, &p, t("Calendar"));
    card(ui, &p, |ui| {
        widgets::wide_row(ui, &p, icons::CALENDAR, t("Episode calendar"), |ui| {
            let file = app.s.calendar_file.clone();
            if !file.is_empty() && widgets::button(ui, &p, "", t("Stop"), false).clicked() {
                app.s.calendar_file.clear();
            }
            if widgets::button(ui, &p, "", t("Choose…"), false).clicked() {
                app.bg.pick_calendar_file();
            }
            let shown = if file.is_empty() { t("Off") } else { file.as_str() };
            widgets::dim(ui, &p, shown).on_hover_text(shown);
        });
    });
    if let Some(e) = &app.calendar_error {
        note(ui, &p, &tf("The calendar could not be saved: {}", &[e]));
    }
    note(
        ui,
        &p,
        t(
            "The next episode of everything you watch or plan to watch is written to this .ics file and kept up to date. Import it into your calendar, or subscribe to it where your calendar app allows.",
        ),
    );
}

fn backup_section(app: &mut App, ui: &mut Ui) {
    let p = app.p;
    section(ui, &p, t("Automatic backup"));
    card(ui, &p, |ui| {
        let b = &mut app.s.backup;
        let label = t("Back up my list");
        select_row(ui, &p, icons::ARROWS_CLOCKWISE, label, &mut b.every, &Every::ALL, |v| {
            format::every(v).into()
        });
        separator(ui, &p);
        widgets::wide_row(ui, &p, icons::EXPORT, t("Folder"), |ui| {
            if widgets::button(ui, &p, "", t("Choose…"), false).clicked() {
                app.bg.pick_folder();
            }
            let folder = if b.folder.is_empty() { t("No folder chosen") } else { b.folder.as_str() };
            widgets::dim(ui, &p, folder).on_hover_text(folder);
        });
        separator(ui, &p);
        let keep = [3, 5, 10, 30];
        select_row(ui, &p, icons::STACK, t("Copies to keep"), &mut b.keep, &keep, |n| n.to_string());
    });
    if let Some(e) = &app.backup_error {
        note(ui, &p, &tf("The automatic backup failed: {}", &[e]));
    } else if let Some(d) = app.last_backup.as_deref().and_then(backup::date_of) {
        note(ui, &p, &tf("Last backup: {}", &[&crate::i18n::current().date(d, true)]));
    }
    note(ui, &p, t("Each copy is named by its date. Older copies beyond the limit are deleted."));
}

fn links_section(app: &mut App, ui: &mut Ui) {
    let p = app.p;
    let s = &mut app.s;
    section(ui, &p, t("Links"));
    card(ui, &p, |ui| {
        toggle_row(ui, &p, icons::LINK, t("Open kuro:// links in this app"), &mut s.handle_links);
        separator(ui, &p);
        toggle_row(
            ui,
            &p,
            icons::COPY,
            t("Open copied AniList and MyAnimeList links"),
            &mut s.watch_clipboard,
        );
        separator(ui, &p);
        toggle_row(ui, &p, icons::LIGHTNING, t("Allow links that change my list"), &mut s.link_actions);
    });
    note(ui, &p, t("Copied links open when you return to this window. Other clipboard content is ignored."));
    if app.s.handle_links {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            if ui.link(t("Copy a bookmarklet that opens the current page here")).clicked() {
                let code = "javascript:location.href='kuro://open?url='+encodeURIComponent(location.href)";
                app.act(Act::Copy(code.into()));
            }
        });
    }
    section(ui, &p, t("Automation"));
    card(ui, &p, |ui| {
        for (i, link) in [
            "kuro://anilist/anime/<id>",
            "kuro://mal/manga/<id>",
            "kuro://search/anime?q=<text>",
            "kuro://tab/seasonal",
            "kuro://random/anime",
            "kuro://refresh",
            "kuro://binge/anilist/anime/<id>",
            "kuro://step/anilist/anime/<id>?by=1",
        ]
        .into_iter()
        .enumerate()
        {
            if i > 0 {
                separator(ui, &p);
            }
            ui.add(egui::Label::new(RichText::new(link).monospace()).selectable(true));
        }
    });
    note(
        ui,
        &p,
        t("Pass one of these links to the program, or open it from anywhere once link handling is on."),
    );
    note(
        ui,
        &p,
        t(
            "Step, binge and refresh links work without bringing the window forward, so a shortcut, a hotkey tool or a Stream Deck can use them. Step links need the switch above; a title's menu copies one.",
        ),
    );
}

fn data(app: &mut App, ui: &mut Ui) {
    let p = app.p;
    if app.image_bytes.is_none() {
        app.image_bytes = Some(0);
        app.bg.image_cache_size();
    }
    section(ui, &p, t("Caches"));
    card(ui, &p, |ui| {
        let n = app.image_bytes.unwrap_or(0);
        widgets::wide_row(ui, &p, icons::IMAGE, t("Images"), |ui| {
            if widgets::button(ui, &p, "", t("Clear"), false).clicked() {
                app.bg.images.clear();
                app.image_bytes = None;
            }
            widgets::dim(ui, &p, format::bytes(n));
        });
        separator(ui, &p);
        let n = *app.cache_bytes.get_or_insert_with(|| app.bg.store().cache_bytes().unwrap_or(0));
        widgets::wide_row(ui, &p, icons::DATABASE, t("Titles and rankings"), |ui| {
            if widgets::button(ui, &p, "", t("Clear"), false).clicked() {
                if let Err(e) = app.bg.store().clear_cache() {
                    tracing::error!("cache could not be cleared: {e}");
                }
                app.cache_bytes = None;
                app.details.clear();
                app.overviews.clear();
                app.people.clear();
                app.tracks.clear();
            }
            widgets::dim(ui, &p, format::bytes(n));
        });
    });
    section(ui, &p, t("Backup"));
    card(ui, &p, |ui| {
        widgets::wide_row(ui, &p, icons::EXPORT, t("Save your list to a file"), |ui| {
            if widgets::button(ui, &p, "", t("Export…"), false).clicked() {
                app.bg.export(app.s.primary);
            }
        });
        separator(ui, &p);
        widgets::wide_row(ui, &p, icons::TRAY_ARROW_DOWN, t("Restore a list from a file"), |ui| {
            if widgets::button(ui, &p, "", t("Import…"), false).clicked() {
                app.bg.import();
            }
        });
    });
    section(ui, &p, t("Sync"));
    card(ui, &p, |ui| {
        widgets::wide_row(ui, &p, icons::CLOUD_ARROW_UP, t("Changes waiting to sync"), |ui| {
            if widgets::button(ui, &p, "", t("Sync now"), false).clicked() {
                app.bg.sync();
            }
            widgets::dim(ui, &p, app.unsynced.to_string());
        });
    });
}

fn about(app: &mut App, ui: &mut Ui) {
    let p = app.p;
    section(ui, &p, "Kuro");
    card(ui, &p, |ui| {
        widgets::fact(ui, &p, t("Version"), env!("CARGO_PKG_VERSION"));
    });
    note(ui, &p, t("Kuro is Japanese for black, like the ink of manga."));
    note(
        ui,
        &p,
        t(
            "An independent, unofficial app. Not affiliated with ManGo, RocketScience IT, AniList or MyAnimeList.",
        ),
    );
    section(ui, &p, t("Privacy"));
    card(ui, &p, |ui| {
        ui.add(
            egui::Label::new(t(
                "No analytics and no ads. The app talks only to AniList, MyAnimeList, Apple Music search, AnimeThemes and Jikan. Sign-in tokens are kept in the Windows Credential Manager.",
            ))
            .wrap(),
        );
    });
    section(ui, &p, t("Credits"));
    card(ui, &p, |ui| {
        ui.label(t("Icons: Phosphor Icons (MIT)"));
    });
}
