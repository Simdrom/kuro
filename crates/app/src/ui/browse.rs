//! Seasonal, ranking and search columns.

use std::rc::Rc;

use chrono::Datelike;
use eframe::egui::{self, Ui};
use mango_api::Ranking;
use mango_core::links;
use mango_core::{Format, Media, MediaKind, Season, SeasonYear, Service, season};

use super::rows::{self, Item, Style};
use crate::app::{Act, App, BrowseSort, Feed, today};
use crate::i18n::t;
use crate::widgets;
use crate::{format, icons};

fn items(app: &App, list: &[Rc<Media>], ranked: bool) -> Vec<Item> {
    list.iter()
        .enumerate()
        .filter(|(_, m)| app.s.show_adult || !m.adult)
        .map(|(i, m)| Item { media: m.clone(), entry: app.entry(m), rank: ranked.then_some(i + 1) })
        .collect()
}

/// Draws a feed. Returns true when its next page is needed.
fn feed(app: &mut App, ui: &mut Ui, id: &str, f: &Feed, list: &[Item], grid: bool) -> bool {
    let p = app.p;
    if list.is_empty() {
        if f.loading || !f.started {
            widgets::loading(ui, &p);
        } else if let Some(e) = &f.error {
            return widgets::failure(ui, &p, &format::error(e));
        } else if f.more {
            return true;
        } else {
            widgets::empty(ui, &p, icons::STACK, t("Nothing found"));
        }
        return false;
    }
    let (loading, error) = (f.loading, f.error.clone());
    let mut retry = false;
    let foot = |_: &mut App, ui: &mut Ui| {
        if loading {
            widgets::loading(ui, &p);
        } else if let Some(e) = &error {
            retry = widgets::failure(ui, &p, &format::error(e));
        }
    };
    let near = if grid {
        rows::grid(app, ui, id, list, false, foot)
    } else {
        rows::list(app, ui, id, list, Style::Browse, foot)
    };
    retry || (near && f.more && !f.loading && f.error.is_none())
}

fn browse_sort(s: BrowseSort) -> &'static str {
    match s {
        BrowseSort::Popularity => t("Most popular"),
        BrowseSort::Score => t("Average score"),
        BrowseSort::Title => t("Title"),
        BrowseSort::StartDate => t("Release date"),
    }
}

pub fn seasonal(app: &mut App, ui: &mut Ui) {
    let p = app.p;
    widgets::title(ui, &p, &format::season_year(app.season.at));
    ui.horizontal(|ui| {
        ui.set_max_width(ui.available_width() - 8.0);
        widgets::search_field(ui, &p, &mut app.season.text, t("Filter by title"));
    });
    let before = (app.season.at, app.season.continuing);
    ui.horizontal(|ui| {
        if widgets::icon_button(ui, &p, icons::CARET_LEFT, t("Previous season")).clicked() {
            app.season.at = app.season.at.prev();
        }
        let r = widgets::icon_button(ui, &p, format::season_icon(app.season.at.season), t("Season"));
        egui::Popup::menu(&r).show(|ui| {
            for s in Season::ALL {
                if ui.selectable_label(app.season.at.season == s, format::season(s)).clicked() {
                    app.season.at.season = s;
                    ui.close();
                }
            }
        });
        let r = widgets::icon_button(ui, &p, icons::CALENDAR, t("Year"));
        egui::Popup::menu(&r).show(|ui| {
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                for y in season::years(today()) {
                    if ui.selectable_label(app.season.at.year == y, y.to_string()).clicked() {
                        app.season.at.year = y;
                        ui.close();
                    }
                }
            });
        });
        if widgets::icon_button(ui, &p, icons::CARET_RIGHT, t("Next season")).clicked() {
            app.season.at = app.season.at.next();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(8.0);
            let r = widgets::icon_button(ui, &p, icons::SORT_ASCENDING, t("Sort by"));
            egui::Popup::menu(&r).show(|ui| {
                for s in BrowseSort::ALL {
                    if ui.selectable_label(app.season.sort == s, browse_sort(s)).clicked() {
                        app.season.sort = s;
                        ui.close();
                    }
                }
            });
            let on = app.season.format.is_some() || app.season.mine_only;
            let color = if on { p.orange } else { p.tint };
            let r = widgets::icon_button_colored(ui, &p, icons::FUNNEL, t("Filter"), color, true);
            egui::Popup::menu(&r).show(|ui| {
                ui.set_min_width(180.0);
                let v = &mut app.season;
                ui.checkbox(&mut v.mine_only, t("Only titles in my list"));
                ui.separator();
                ui.selectable_value(&mut v.format, None, t("All formats"));
                for f in
                    [Format::Tv, Format::TvShort, Format::Movie, Format::Ona, Format::Ova, Format::Special]
                {
                    ui.selectable_value(&mut v.format, Some(f), format::format(f));
                }
            });
            let (icon, text) = if app.s.seasonal_grid {
                (icons::LIST_BULLETS, t("Show as list"))
            } else {
                (icons::SQUARES_FOUR, t("Show as posters"))
            };
            if widgets::icon_button(ui, &p, icon, text).clicked() {
                app.s.seasonal_grid = !app.s.seasonal_grid;
            }
        });
    });
    widgets::segmented(ui, &p, &mut app.season.continuing, &[(false, t("New")), (true, t("Continuing"))]);
    ui.add_space(4.0);
    if before != (app.season.at, app.season.continuing) {
        app.season.feed.reset();
    }
    if !app.season.feed.started {
        app.load_season();
    }
    let mut list: Vec<Item> = items(app, &app.season.feed.items, false);
    let v = &app.season;
    list.retain(|i| {
        v.format.is_none_or(|f| f == i.media.format)
            && (!v.mine_only || i.entry.is_some())
            && i.media.matches(&v.text)
    });
    let lang = app.s.title_language;
    match v.sort {
        BrowseSort::Popularity => {}
        BrowseSort::Score => list
            .sort_by(|a, b| b.media.mean_score.unwrap_or(0.0).total_cmp(&a.media.mean_score.unwrap_or(0.0))),
        BrowseSort::Title => list.sort_by_key(|i| i.media.display_title(lang).to_lowercase()),
        BrowseSort::StartDate => list.sort_by_key(|i| (i.media.start.is_none(), i.media.start)),
    }
    // Sorting and filtering need the whole season, which is a few pages.
    let whole = v.sort != BrowseSort::Popularity || v.format.is_some() || v.mine_only || !v.text.is_empty();
    let f = std::mem::take(&mut app.season.feed);
    let more = feed(app, ui, "season", &f, &list, app.s.seasonal_grid);
    app.season.feed = f;
    let f = &app.season.feed;
    if more || (whole && f.more && !f.loading && f.error.is_none()) {
        app.load_season();
    }
}

pub fn ranking(app: &mut App, ui: &mut Ui) {
    let p = app.p;
    widgets::title(ui, &p, t("Ranking"));
    let before = app.ranking_query();
    let mut kind = app.ranking.kind;
    let kinds = MediaKind::ALL.map(|k| (k, format::kind(k)));
    if widgets::segmented(ui, &p, &mut kind, &kinds) {
        app.ranking.kind = kind;
        app.ranking.ranking = Ranking::All;
    }
    ui.add_space(2.0);
    let anilist = app.s.primary == Service::AniList;
    ui.horizontal_wrapped(|ui| {
        let options = Ranking::options(kind, app.s.primary);
        let r = &mut app.ranking;
        widgets::select(ui, "ranking", t("Ranking"), &mut r.ranking, &options, |v| {
            format::ranking(v).to_owned()
        });
        if !anilist {
            return;
        }
        if !r.genres_started {
            r.genres_started = true;
            app.bg.genres();
        }
        let r = &mut app.ranking;
        egui::ComboBox::from_id_salt("genre")
            .width(110.0)
            .selected_text(r.genre.as_deref().unwrap_or(t("All genres")))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut r.genre, None, t("All genres"));
                for g in &r.genres {
                    // Only shown on request, like the rest of the adult catalogue.
                    if app.s.show_adult || g != "Hentai" {
                        ui.selectable_value(&mut r.genre, Some(g.clone()), g);
                    }
                }
            });
        let mut years = vec![None];
        years.extend((1960..=today().year() + 1).rev().map(Some));
        widgets::select(ui, "year", t("Year"), &mut r.year, &years, |y| {
            y.map_or(t("All years").to_owned(), |y| y.to_string())
        });
    });
    ui.add_space(4.0);
    if before != app.ranking_query() {
        app.ranking.feed.reset();
    }
    if !app.ranking.feed.started {
        app.load_ranking();
    }
    let list = items(app, &app.ranking.feed.items, true);
    let f = std::mem::take(&mut app.ranking.feed);
    let more = feed(app, ui, "ranking", &f, &list, false);
    app.ranking.feed = f;
    if more {
        app.load_ranking();
    }
}

pub fn search(app: &mut App, ui: &mut Ui) {
    let p = app.p;
    ui.horizontal(|ui| {
        widgets::title(ui, &p, t("Search"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(8.0);
            let label = t("Random title");
            let busy = app.search.random_busy;
            if busy {
                ui.add(egui::Spinner::new().size(18.0).color(p.tint));
            } else if widgets::icon_button(ui, &p, icons::SHUFFLE, label).clicked() {
                let k = app.search.kind;
                app.act(Act::Random(k));
            }
        });
    });
    let now = ui.input(|i| i.time);
    let r = ui
        .horizontal(|ui| {
            ui.set_max_width(ui.available_width() - 8.0);
            widgets::search_field(ui, &p, &mut app.search.text, t("Title, or an AniList or MyAnimeList link"))
        })
        .inner;
    if std::mem::take(&mut app.search.focus) {
        r.request_focus();
    }
    if r.changed() {
        app.search.typed_at = now;
    }
    let mut kind = app.search.kind;
    let kinds = MediaKind::ALL.map(|k| (k, format::kind(k)));
    if widgets::segmented(ui, &p, &mut kind, &kinds) {
        app.search.kind = kind;
        app.start_search();
    }
    ui.add_space(4.0);

    let text = app.search.text.trim().to_owned();
    if text != app.search.sent {
        if links::parse(&text).is_some() {
            let link = std::mem::take(&mut app.search.text);
            app.open_link(&link);
        } else if text.is_empty()
            || now - app.search.typed_at > 0.45
            || ui.input(|i| i.key_pressed(egui::Key::Enter))
        {
            app.start_search();
        } else {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(120));
        }
    }
    if app.search.sent.is_empty() {
        let hint = t("Search for a title, or try your luck with a random one.");
        widgets::empty(ui, &p, icons::MAGNIFYING_GLASS, hint);
        return;
    }
    let list = items(app, &app.search.feed.items, false);
    let f = std::mem::take(&mut app.search.feed);
    let more = feed(app, ui, "search", &f, &list, false);
    app.search.feed = f;
    if more {
        app.load_search();
    }
}

/// Used by tests and the season header.
pub fn current_season() -> SeasonYear {
    SeasonYear::of_date(today())
}
