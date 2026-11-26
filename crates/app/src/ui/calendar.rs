//! The coming episodes of the user's anime, day by day: the same episodes
//! Kuro sends to Google Calendar and the calendar file.

use std::rc::Rc;

use eframe::egui::Ui;
use jiff::Timestamp;
use jiff::ToSpan;
use jiff::civil::Date;
use jiff::tz::TimeZone;
use kuro_core::{MediaKind, calendar};

use super::rows;
use crate::app::{Act, App, Route, SettingsPage, today};
use crate::i18n::{t, tf};
use crate::{format, icons, widgets};

/// Days shown one by one; later episodes share one section.
const DAYS: i64 = 7;

fn start_of(d: Date) -> i64 {
    d.to_zoned_datetime(TimeZone::system()).map_or(0, |z| z.timestamp().as_second())
}

fn date_of(at: i64) -> Option<Date> {
    Some(Timestamp::from_second(at).ok()?.to_zoned_datetime(TimeZone::system()).date())
}

fn day_label(d: Date, offset: i64) -> String {
    let date = format::weekday_date(d);
    match offset {
        0 => format!("{} · {date}", t("Today")),
        1 => format!("{} · {date}", t("Tomorrow")),
        _ => date,
    }
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let p = app.p;
    ui.horizontal(|ui| {
        ui.set_min_height(36.0);
        rows::trailing(ui, |ui| {
            let label = t("Calendar settings");
            if widgets::icon_button_colored(ui, &p, icons::SLIDERS, label, p.tint, false).clicked() {
                app.act(Act::Push(Route::Settings(SettingsPage::Widgets)));
            }
            super::heading(ui, &p, super::tab_label(crate::app::Tab::Calendar));
        });
    });
    ui.add_space(4.0);

    let first = today();
    let since = start_of(first);
    let hide_adult = !app.s.show_adult;
    let due: Vec<(Rc<kuro_core::Media>, i64, u32)> = calendar::upcoming(app.library(MediaKind::Anime), since)
        .into_iter()
        .filter(|e| !(hide_adult && e.media.adult))
        .filter_map(|e| e.media.next_airing.map(|a| (Rc::new(e.media.clone()), a.at, a.episode)))
        .collect();
    if due.is_empty() {
        widgets::empty(ui, &p, icons::CALENDAR_BLANK, t("No episodes coming"));
        ui.vertical_centered(|ui| {
            widgets::note(
                ui,
                &p,
                t("Anime you watch or plan to watch show up here once their next episode has a date."),
            );
        });
        return;
    }

    super::scroll(ui, "calendar", |ui| {
        for offset in 0..=DAYS {
            let day = first.checked_add(offset.days()).unwrap_or(first);
            let last = offset == DAYS;
            let shown: Vec<_> = due
                .iter()
                .filter(|(_, at, _)| {
                    let d = date_of(*at).unwrap_or(first);
                    if last { d >= day } else { d == day }
                })
                .collect();
            if last && shown.is_empty() {
                break;
            }
            let label = if last { t("Later").to_owned() } else { day_label(day, offset) };
            widgets::section(ui, &p, &label);
            if shown.is_empty() {
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    widgets::dim(ui, &p, t("No episodes"));
                });
                continue;
            }
            ui.horizontal_wrapped(|ui| {
                ui.add_space(12.0);
                ui.spacing_mut().item_spacing = eframe::egui::vec2(12.0, 12.0);
                for (m, at, ep) in shown {
                    // Later episodes give their day; the hour is in the detail.
                    let when = if last {
                        date_of(*at).map(format::weekday_date).unwrap_or_default()
                    } else {
                        format::time(*at)
                    };
                    rows::shelf_tile(app, ui, m, &tf("Ep {} · {}", &[ep, &when]));
                }
            });
        }
    });
}
