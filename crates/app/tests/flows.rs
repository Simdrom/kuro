#![allow(clippy::unwrap_used)]
//! Drives the interface the way a person would, without network or GPU.

mod common;

use std::rc::Rc;

use common::*;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use kuro_api::{ApiError, Page};
use kuro_app::app::{Act, App, Feed, Load, Route, SettingsPage, Tab};
use kuro_app::backend::Event;
use kuro_core::overview::Section;
use kuro_core::settings::{GestureAction, ListLayout, Theme};
use kuro_core::*;

type H = Harness<'static, App>;

fn click(h: &mut H, label: &str) {
    h.get_by_label(label).click();
    settle(h);
}

fn r(id: u64, kind: MediaKind) -> MediaRef {
    MediaRef { service: Service::AniList, kind, id }
}

fn progress(h: &H, id: u64) -> u32 {
    h.state().entry_at(r(id, MediaKind::Anime)).unwrap().progress
}

fn act(h: &mut H, a: Act) {
    h.state_mut().act(a);
    settle(h);
}

#[test]
fn list_shows_only_the_chosen_status() {
    let mut h = harness();
    assert!(h.query_by_label_contains("Lanterns Over Saltmarsh, Watching, 9 / 24").is_some());
    assert!(h.query_by_label_contains("Iron Orchard").is_none());
    h.state_mut().lists[0].filter.status = Some(ListStatus::Completed);
    settle(&mut h);
    assert!(h.query_by_label_contains("Iron Orchard, Completed").is_some());
    assert!(h.query_by_label_contains("Lanterns").is_none());
}

#[test]
fn searching_the_list_ignores_the_status_filter() {
    let mut h = harness();
    h.state_mut().lists[0].filter.query = "orchard".into();
    settle(&mut h);
    assert!(h.query_by_label_contains("Iron Orchard, Completed").is_some());
}

#[test]
fn plus_button_adds_an_episode_and_queues_nothing_when_signed_out() {
    let mut h = harness();
    click(&mut h, "Add one: Lanterns Over Saltmarsh");
    assert_eq!(progress(&h, 1), 10);
    assert!(h.state().bg.store().pending().unwrap().is_empty());
    let stored = h.state().bg.store().entry(r(1, MediaKind::Anime)).unwrap().unwrap();
    assert_eq!(stored.progress, 10);
}

#[test]
fn changes_made_here_can_be_undone_from_the_toast() {
    let mut h = harness();
    click(&mut h, "Add one: Lanterns Over Saltmarsh");
    assert_eq!(progress(&h, 1), 10);
    assert!(h.query_by_label_contains("Lanterns Over Saltmarsh · Watching · 10/24").is_some());
    click(&mut h, "Undo");
    assert_eq!(progress(&h, 1), 9);
    assert_eq!(h.state().bg.store().entry(r(1, MediaKind::Anime)).unwrap().unwrap().progress, 9);
    assert_eq!(h.state().toast.as_ref().map(|t| t.text.as_str()), Some("Change undone"));

    // A removal comes back with everything it had.
    let before = h.state().entry_at(r(6, MediaKind::Anime)).unwrap();
    act(&mut h, Act::Delete(r(6, MediaKind::Anime)));
    assert!(h.state().entry_at(r(6, MediaKind::Anime)).is_none());
    click(&mut h, "Undo");
    let back = h.state().entry_at(r(6, MediaKind::Anime)).unwrap();
    assert_eq!((back.status, back.progress, back.score), (before.status, before.progress, before.score));

    // A title added by the change leaves the list again.
    act(&mut h, Act::SetStatus(Rc::new(media(90, MediaKind::Anime, "New Shore")), ListStatus::Planning));
    assert!(h.state().entry_at(r(90, MediaKind::Anime)).is_some());
    act(&mut h, Act::Undo);
    assert!(h.state().entry_at(r(90, MediaKind::Anime)).is_none());

    // Nothing changed, nothing to undo.
    h.state_mut().toast = None;
    act(&mut h, Act::Step(Rc::new(media(3, MediaKind::Anime, "Iron Orchard")), 1));
    assert!(h.state().toast.is_none());
}

#[test]
fn finishing_a_title_completes_it_and_fires_confetti() {
    let mut h = harness_with(store(), [1240.0, 820.0], |s| s.animations = true);
    let m = Rc::new(h.state().entry_at(r(2, MediaKind::Anime)).unwrap().media.clone());
    for _ in 0..30 {
        h.state_mut().act(Act::Step(m.clone(), 1));
    }
    h.step();
    h.step();
    let e = h.state().entry_at(r(2, MediaKind::Anime)).unwrap();
    assert_eq!((e.status, e.progress), (ListStatus::Completed, 24));
    assert!(h.state().confetti.active());
}

#[test]
fn opening_a_title_shows_its_details_and_back_closes_them() {
    let mut h = harness();
    h.state_mut().details.entry(r(1, MediaKind::Anime)).or_default().data =
        Some(Rc::new(detail(1, MediaKind::Anime, "Lanterns Over Saltmarsh")));
    h.get_by_label_contains("Lanterns Over Saltmarsh, Watching").click();
    settle(&mut h);
    assert_eq!(h.state().routes, [Route::Media(r(1, MediaKind::Anime))]);
    for label in ["Ranked", "# 12", "Complete series overview", "Synopsis"] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
    h.key_press(egui::Key::Escape);
    settle(&mut h);
    assert!(h.state().routes.is_empty());
}

#[test]
fn custom_lists_are_named_ticked_and_filtered() {
    let mut h = harness();
    let m = Rc::new(media(4, MediaKind::Anime, "Midnight Ferry"));
    act(&mut h, Act::Edit(m.clone()));
    h.state_mut().editor.as_mut().unwrap().new_list = "  Cozy,  nights ".into();
    settle(&mut h);
    click(&mut h, "Add");
    assert_eq!(h.state().editor.as_ref().unwrap().entry.lists, Some(vec!["Cozy nights".to_owned()]));
    click(&mut h, "Save");
    assert_eq!(h.state().entry_at(r(4, MediaKind::Anime)).unwrap().lists(), ["Cozy nights"]);

    // Known names are offered on other titles, and ticking one adds it.
    act(&mut h, Act::Edit(Rc::new(media(2, MediaKind::Anime, "x"))));
    click(&mut h, "Cozy nights");
    click(&mut h, "Save");
    assert_eq!(h.state().entry_at(r(2, MediaKind::Anime)).unwrap().lists(), ["Cozy nights"]);

    h.state_mut().lists[0].filter =
        kuro_core::list::Filter { list: Some("cozy NIGHTS".into()), ..Default::default() };
    settle(&mut h);
    assert!(h.query_by_label_contains("Midnight Ferry, ").is_some());
    assert!(h.query_by_label_contains("The Mapmaker and the Slowly Moving Northern Coast, ").is_some());
    assert!(h.query_by_label_contains("Lanterns Over Saltmarsh, ").is_none());

    // Signed in to AniList, new lists are made on the website.
    let tokens = kuro_api::Tokens { access: "t".into(), refresh: None, expires_at: i64::MAX, device: None };
    h.state().bg.api.accept_login(Service::AniList, &UserProfile::default(), &tokens).unwrap();
    act(&mut h, Act::Edit(m));
    assert!(h.query_by_label("Make new custom lists on AniList; they show here after a refresh.").is_some());
    assert!(h.query_by_label("New name").is_none());
}

#[test]
fn editor_saves_and_rejects_bad_dates() {
    let mut h = harness();
    let m = Rc::new(media(4, MediaKind::Anime, "Midnight Ferry"));
    act(&mut h, Act::Edit(m));
    let ed = h.state_mut().editor.as_mut().unwrap();
    assert!(!ed.is_new);
    ed.start = "2026-02-31".into();
    ed.entry.progress = 7;
    settle(&mut h);
    assert!(h.query_by_label("Write dates as YYYY-MM-DD.").is_some());
    h.state_mut().editor.as_mut().unwrap().start = "2026-02-03".into();
    settle(&mut h);
    click(&mut h, "Save");
    let e = h.state().entry_at(r(4, MediaKind::Anime)).unwrap();
    assert!(h.state().editor.is_none());
    assert_eq!((e.progress, e.start), (7, FuzzyDate::parse("2026-02-03")));
}

#[test]
fn cancelling_the_editor_changes_nothing() {
    let mut h = harness();
    act(&mut h, Act::Edit(Rc::new(media(4, MediaKind::Anime, "Midnight Ferry"))));
    h.state_mut().editor.as_mut().unwrap().entry.progress = 9;
    click(&mut h, "Cancel");
    assert_eq!(progress(&h, 4), 0);
}

#[test]
fn removing_asks_first() {
    let mut h = harness();
    let m = Rc::new(media(6, MediaKind::Anime, "Dustfall"));
    act(&mut h, Act::AskDelete(m.clone()));
    assert!(h.query_by_label("Remove from your list?").is_some());
    click(&mut h, "Cancel");
    assert!(h.state().entry_at(r(6, MediaKind::Anime)).is_some());
    act(&mut h, Act::AskDelete(m));
    click(&mut h, "Remove");
    assert!(h.state().entry_at(r(6, MediaKind::Anime)).is_none());
    assert!(h.state().bg.store().entry(r(6, MediaKind::Anime)).unwrap().is_none());
}

#[test]
fn rewatching_round_trip() {
    let mut h = harness();
    let m = Rc::new(media(3, MediaKind::Anime, "Iron Orchard"));
    // A finished title is not advanced by accident.
    act(&mut h, Act::Step(m.clone(), 1));
    assert_eq!(progress(&h, 3), 24);
    act(&mut h, Act::Repeat(m.clone()));
    let e = h.state().entry_at(r(3, MediaKind::Anime)).unwrap();
    assert!(e.repeating && e.progress == 0);
    act(&mut h, Act::SetStatus(m, ListStatus::Completed));
    let e = h.state().entry_at(r(3, MediaKind::Anime)).unwrap();
    assert_eq!((e.repeating, e.repeat_count), (false, 1));
}

#[test]
fn every_gesture_does_what_it_says() {
    let mut h = harness();
    let m = Rc::new(media(1, MediaKind::Anime, "Lanterns Over Saltmarsh"));
    let g = |h: &mut H, g| act(h, Act::Gesture(g, m.clone()));
    g(&mut h, GestureAction::Increment);
    assert_eq!(progress(&h, 1), 10);
    g(&mut h, GestureAction::Decrement);
    assert_eq!(progress(&h, 1), 9);
    let status = |h: &H| h.state().entry_at(r(1, MediaKind::Anime)).unwrap().status;
    g(&mut h, GestureAction::Pause);
    assert_eq!(status(&h), ListStatus::Paused);
    g(&mut h, GestureAction::Drop);
    assert_eq!(status(&h), ListStatus::Dropped);
    g(&mut h, GestureAction::Complete);
    assert_eq!(status(&h), ListStatus::Completed);
    g(&mut h, GestureAction::None);
    g(&mut h, GestureAction::Edit);
    assert!(h.state().editor.is_some());
}

#[test]
fn dragging_a_row_triggers_the_swipe_action() {
    let mut h = harness();
    let at = h.get_by_label_contains("Lanterns Over Saltmarsh, Watching").rect().center();
    h.drag_at(at);
    h.step();
    for i in 1..=6 {
        h.hover_at(at + egui::vec2(i as f32 * 20.0, 0.0));
        h.step();
    }
    h.drop_at(at + egui::vec2(120.0, 0.0));
    settle(&mut h);
    assert_eq!(progress(&h, 1), 10);
    assert!(h.state().routes.is_empty(), "a swipe is not a click");
}

#[test]
fn the_calendar_shows_coming_episodes_and_opens_them() {
    let mut h = harness();
    act(&mut h, Act::Go(Tab::Calendar));
    assert!(h.query_by_label_contains("Today").is_some(), "every day of the week has a heading");
    assert!(h.query_by_label_contains("Iron Orchard").is_none(), "only titles with a coming episode");
    h.get_by_label_contains("Ep 10 · ").click();
    settle(&mut h);
    assert!(matches!(h.state().routes.last(), Some(Route::Media(r)) if r.id == 1));
}

#[test]
fn keyboard_shortcuts() {
    let mut h = harness();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Num3);
    settle(&mut h);
    assert_eq!(h.state().tab, Tab::Calendar);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Num5);
    settle(&mut h);
    assert_eq!(h.state().tab, Tab::Ranking);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Num1);
    settle(&mut h);
    act(&mut h, Act::Open(Rc::new(media(1, MediaKind::Anime, "Lanterns Over Saltmarsh"))));
    h.key_press(egui::Key::Plus);
    settle(&mut h);
    assert_eq!(progress(&h, 1), 10);
    h.key_press(egui::Key::E);
    settle(&mut h);
    assert!(h.state().editor.is_some());
}

#[test]
fn command_palette_opens_titles() {
    let mut h = harness();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::K);
    settle(&mut h);
    assert!(h.state().command.is_some());
    h.state_mut().command = Some("meridian".into());
    settle(&mut h);
    h.key_press(egui::Key::Enter);
    settle(&mut h);
    assert_eq!(h.state().routes, [Route::Media(r(5, MediaKind::Anime))]);
    assert!(h.state().command.is_none());
}

#[test]
fn episode_notices_offer_to_mark_the_new_episode() {
    let mut h = harness();
    let one = r(1, MediaKind::Anime);
    let e = (*h.state().entry_at(one).unwrap()).clone();
    let ep = e.progress + 1;
    let due = |event| kuro_core::notify::Due { key: "k".into(), at: 0, event, entry: e.clone() };
    let n = h.state().notice(&due(kuro_core::notify::Event::Episode(ep)));
    assert_eq!(n.mark, Some((one, ep)));
    assert!(n.detail.as_deref().is_some_and(|d| d.contains(&format!("{}/", e.progress))));
    assert!(n.link.is_some());
    // An episode already in the list is not offered.
    let old = h.state().notice(&due(kuro_core::notify::Event::Episode(e.progress)));
    assert_eq!(old.mark, None);

    h.state_mut().mark_episode(one, ep);
    settle(&mut h);
    assert_eq!(progress(&h, 1), ep);
    h.state_mut().mark_episode(one, ep - 1);
    assert_eq!(progress(&h, 1), ep, "never moves back");
}

#[test]
fn only_the_title_on_screen_is_marked_open() {
    let mut h = harness();
    let (a, b) = (r(1, MediaKind::Anime), r(5, MediaKind::Anime));
    h.state_mut().routes = vec![Route::Media(a), Route::Media(b)];
    assert!(kuro_app::ui::is_open(h.state(), b));
    assert!(!kuro_app::ui::is_open(h.state(), a), "titles further back are not open");
    h.state_mut().routes.push(Route::Review(b, 9));
    assert!(kuro_app::ui::is_open(h.state(), b), "a review keeps its title marked");
}

#[test]
fn settings_survive_a_restart() {
    let mut h = harness();
    h.state_mut().s.theme = Theme::Light;
    h.state_mut().s.gestures.swipe_left = GestureAction::Drop;
    settle(&mut h);
    let s = h.state().bg.store().settings().unwrap();
    assert_eq!((s.theme, s.gestures.swipe_left), (Theme::Light, GestureAction::Drop));
    assert!(!h.state().p.dark);
}

#[test]
fn every_settings_page_renders() {
    for size in [[1240.0, 820.0], [430.0, 800.0]] {
        let mut h = harness_with(store(), size, |_| {});
        act(&mut h, Act::Go(Tab::Settings));
        for page in SettingsPage::ALL {
            h.state_mut().routes.clear();
            act(&mut h, Act::Push(Route::Settings(page)));
        }
    }
}

#[test]
fn statistics_rows_jump_to_the_list() {
    let mut h = harness();
    act(&mut h, Act::Go(Tab::Settings));
    act(&mut h, Act::ShowStatus(MediaKind::Anime, ListStatus::Dropped));
    assert!(h.query_by_label_contains("Dustfall, Dropped").is_some());
}

#[test]
fn more_statistics_show_scores_genres_and_years() {
    for size in [[1240.0, 820.0], [430.0, 800.0]] {
        let mut h = harness_with(store(), size, |_| {});
        act(&mut h, Act::Go(Tab::Settings));
        click(&mut h, "More statistics: Anime");
        assert_eq!(h.state().routes.last(), Some(&Route::Stats(MediaKind::Anime)));
        assert!(h.query_by_label_contains("Scores · ").is_some());
        assert!(h.query_by_label_contains("Drama: ").is_some());
        assert!(h.query_by_label_contains("Studio Example: ").is_some());
        act(&mut h, Act::Push(Route::Stats(MediaKind::Manga)));
        assert!(h.query_by_label_contains("Studio Example").is_none(), "no studios for manga");
    }
    let mut h = harness_with(Store::in_memory().unwrap(), [1240.0, 820.0], |_| {});
    act(&mut h, Act::Push(Route::Stats(MediaKind::Anime)));
    assert!(h.query_by_label("Nothing to count yet").is_some());
}

#[test]
fn links_open_titles_searches_and_tabs() {
    let mut h = harness();
    assert!(h.state_mut().open_link("https://anilist.co/anime/3"));
    settle(&mut h);
    assert_eq!(h.state().routes.last(), Some(&Route::Media(r(3, MediaKind::Anime))));
    assert!(h.state_mut().open_link("kuro://search/manga?q=ink"));
    settle(&mut h);
    assert_eq!((h.state().tab, h.state().search.sent.as_str()), (Tab::Search, "ink"));
    assert!(h.state_mut().open_link("kuro://tab/ranking"));
    settle(&mut h);
    assert_eq!(h.state().tab, Tab::Ranking);
    assert!(!h.state_mut().open_link("https://example.com/anime/3"));
}

#[test]
fn feeds_page_retry_and_ignore_stale_answers() {
    let mut h = harness();
    act(&mut h, Act::Go(Tab::Ranking));
    let ctx = h.ctx.clone();
    let q = h.state().ranking_query();
    let page = |ids: std::ops::Range<u64>, more| Page {
        items: ids.map(|i| media(i, MediaKind::Anime, &format!("Title {i}"))).collect(),
        more,
    };
    let send = |h: &mut H, query, n, result| {
        h.state_mut().handle(&ctx, Event::Ranking { query, page: n, result });
        settle(h);
    };
    send(&mut h, q.clone(), 1, Err(ApiError::RateLimited));
    assert!(h.query_by_label_contains("Too many requests").is_some());
    send(&mut h, q.clone(), 1, Ok(page(100..110, true)));
    assert_eq!(h.state().ranking.feed.items.len(), 10);
    assert!(h.state().ranking.feed.error.is_none());
    // Overlapping pages must not duplicate titles.
    send(&mut h, q.clone(), 2, Ok(page(105..115, false)));
    assert_eq!(h.state().ranking.feed.items.len(), 15);
    let mut other = q;
    other.genre = Some("Drama".into());
    send(&mut h, other, 1, Ok(page(500..501, false)));
    assert_eq!(h.state().ranking.feed.items.len(), 15);
}

#[test]
fn adult_titles_stay_hidden_by_default() {
    let mut h = harness();
    act(&mut h, Act::Go(Tab::Ranking));
    let mut m = media(300, MediaKind::Anime, "Hidden One");
    m.adult = true;
    h.state_mut().ranking.feed = Feed { items: vec![Rc::new(m)], started: true, ..Feed::default() };
    settle(&mut h);
    assert!(h.query_by_label_contains("Hidden One").is_none());
}

#[test]
fn a_newer_version_is_offered_until_skipped() {
    use kuro_api::updates::Release;
    let mut h = harness();
    let ctx = h.ctx.clone();
    let release = |v: &str| Release {
        version: v.into(),
        url: format!("https://github.com/Simdrom/kuro/releases/tag/v{v}"),
    };
    h.state_mut().handle(&ctx, Event::Release(Ok(release("0.0.1"))));
    assert!(h.state().update.is_none(), "an older version is not offered");
    h.state_mut().handle(&ctx, Event::Release(Ok(release("99.0.0"))));
    assert_eq!(h.state().update.as_ref().map(|u| u.version.as_str()), Some("99.0.0"));
    settle(&mut h);
    click(&mut h, "New version");
    assert_eq!(h.state().routes.last(), Some(&Route::Settings(SettingsPage::About)));
    assert!(h.query_by_label("Download").is_some());
    click(&mut h, "Skip this version");
    assert!(h.state().update.is_none());
    h.state_mut().handle(&ctx, Event::Release(Ok(release("99.0.0"))));
    assert!(h.state().update.is_none(), "a skipped version stays skipped");
    h.state_mut().handle(&ctx, Event::Release(Ok(release("99.0.1"))));
    assert!(h.state().update.is_some(), "the next one is offered again");
}

#[test]
fn the_calendar_says_when_an_episode_is_delayed_or_on_a_break() {
    // Tall enough to show the later episodes too.
    let mut h = harness_with(store(), [1240.0, 2600.0], |_| {});
    let ctx = h.ctx.clone();
    let day = 86_400;
    let first = h.state().entry_at(r(1, MediaKind::Anime)).unwrap().media.next_airing.unwrap();
    let mut m = media(1, MediaKind::Anime, "Lanterns Over Saltmarsh");
    m.release = ReleaseStatus::Releasing;
    m.next_airing = Some(Airing { episode: 10, at: first.at + 7 * day });
    h.state_mut().handle(&ctx, Event::Airing(Ok(vec![m.clone()])));
    let e = h.state().entry_at(r(1, MediaKind::Anime)).unwrap();
    assert_eq!(e.media.schedule.delayed_from, Some(first.at));
    act(&mut h, Act::Go(Tab::Calendar));
    assert!(h.query_by_label("Schedule changes").is_some());
    assert!(h.query_by_label_contains("Lanterns Over Saltmarsh: Episode 10 moved from").is_some());
    assert!(h.query_by_label_contains("· Delayed Lanterns Over Saltmarsh").is_some());

    // Episode 10 aired; 11 comes two weeks later.
    m.next_airing = Some(Airing { episode: 11, at: first.at + 21 * day });
    h.state_mut().handle(&ctx, Event::Airing(Ok(vec![m])));
    let s = h.state().entry_at(r(1, MediaKind::Anime)).unwrap().media.schedule;
    assert_eq!((s.previous, s.delayed_from), (Some(first.at + 7 * day), None));
    settle(&mut h);
    assert!(h.query_by_label_contains("On a break: episode 11 airs").is_some());
    assert!(h.query_by_label_contains("moved from").is_none());
}

#[test]
fn airing_updates_do_not_count_as_edits() {
    let mut h = harness();
    let before = h.state().entry_at(r(1, MediaKind::Anime)).unwrap().updated_at;
    let mut m = media(1, MediaKind::Anime, "Lanterns Over Saltmarsh");
    m.next_airing = Some(Airing { episode: 11, at: 4_000_000_000 });
    let ctx = h.ctx.clone();
    h.state_mut().handle(&ctx, Event::Airing(Ok(vec![m])));
    let e = h.state().entry_at(r(1, MediaKind::Anime)).unwrap();
    assert_eq!((e.media.next_airing.unwrap().episode, e.updated_at), (11, before));
    assert!(h.state().bg.store().pending().unwrap().is_empty());
}

#[test]
fn failed_details_offer_a_retry() {
    let mut h = harness();
    let id = r(999, MediaKind::Anime);
    act(&mut h, Act::OpenRef(id));
    let ctx = h.ctx.clone();
    h.state_mut()
        .handle(&ctx, Event::Detail { media: id, result: Err(ApiError::Network("x".into())), fresh: true });
    settle(&mut h);
    click(&mut h, "Retry");
    assert!(h.state().details[&id].loading);
}

#[test]
fn unverified_sign_in_needs_confirmation() {
    let mut h = harness();
    let ctx = h.ctx.clone();
    let login = |h: &mut H| {
        h.state_mut().login = Some(Service::AniList);
        let profile = UserProfile { id: 1, name: "someone".into(), avatar: None };
        let tokens =
            kuro_api::Tokens { access: "t".into(), refresh: None, expires_at: i64::MAX, device: None };
        h.state_mut()
            .handle(&ctx, Event::Login { service: Service::AniList, result: Ok((profile, tokens, false)) });
        settle(h);
    };
    login(&mut h);
    assert!(h.query_by_label("Sign in as someone?").is_some());
    assert!(!h.state().bg.api.is_signed_in(Service::AniList));
    click(&mut h, "Cancel");
    assert!(!h.state().bg.api.is_signed_in(Service::AniList));
    login(&mut h);
    h.get_all_by_label("Sign in").next().unwrap().click();
    settle(&mut h);
    assert!(h.state().bg.api.is_signed_in(Service::AniList));

    // Edits are queued once signed in, and signing out removes the account's data.
    act(&mut h, Act::Step(Rc::new(media(1, MediaKind::Anime, "x")), 1));
    assert_eq!(h.state().bg.store().pending().unwrap().len(), 1);
    act(&mut h, Act::AskSignOut(Service::AniList));
    assert!(h.query_by_label_contains("unsent changes will be lost").is_some());
    click(&mut h, "Sign out");
    assert!(!h.state().bg.api.is_signed_in(Service::AniList));
    assert!(h.state().library(MediaKind::Anime).is_empty());
    assert!(h.state().bg.store().pending().unwrap().is_empty());
}

#[test]
fn a_cancelled_sign_in_is_ignored_when_it_arrives_late() {
    let mut h = harness();
    let ctx = h.ctx.clone();
    let profile = UserProfile { id: 1, name: "late".into(), avatar: None };
    let tokens = kuro_api::Tokens { access: "t".into(), refresh: None, expires_at: i64::MAX, device: None };
    h.state_mut().handle(&ctx, Event::Login { service: Service::Mal, result: Ok((profile, tokens, true)) });
    settle(&mut h);
    assert!(!h.state().bg.api.is_signed_in(Service::Mal));
}

#[test]
fn overview_person_and_review_pages_render() {
    let mut h = harness();
    let id = r(1, MediaKind::Anime);
    h.state_mut().details.entry(id).or_default().data =
        Some(Rc::new(detail(1, MediaKind::Anime, "Lanterns Over Saltmarsh")));
    let titles = vec![media(1, MediaKind::Anime, "Lanterns Over Saltmarsh")];
    h.state_mut().overviews.insert(id, Load::Ready(Rc::new(vec![(Section::Main, titles)])));
    act(&mut h, Act::Push(Route::Overview(id)));
    assert!(h.query_by_label("Seasons").is_some());

    let role = |id, title: &str| VoiceRole {
        character: format!("Hero of {title}"),
        character_image: None,
        role: "MAIN".into(),
        media: media(id, MediaKind::Anime, title),
    };
    let person = PersonDetail {
        person: Person { id: 7, name: "Example Actor".into(), ..Person::default() },
        roles: vec![role(3, "Iron Orchard"), role(1, "Lanterns Over Saltmarsh")],
        ..PersonDetail::default()
    };
    h.state_mut().people.insert(7, Load::Ready(Rc::new(person)));
    act(&mut h, Act::Push(Route::Person(7)));
    assert!(h.query_by_label_contains("Hero of Lanterns").is_some());
    h.state_mut().roles_completed_only = true;
    settle(&mut h);
    assert!(h.query_by_label_contains("Hero of Lanterns").is_none());
    assert!(h.query_by_label_contains("Hero of Iron Orchard").is_some());

    act(&mut h, Act::Push(Route::Review(id, 5)));
    assert!(h.query_by_label("Quiet and lovely.").is_some());
}

#[test]
fn unknown_review_goes_back_instead_of_breaking() {
    let mut h = harness();
    act(&mut h, Act::Push(Route::Review(r(1, MediaKind::Anime), 12345)));
    assert!(h.state().routes.is_empty());
}

#[test]
fn every_tab_layout_and_view_renders() {
    for (sidebar, separate, grid, size) in [
        (true, true, ListLayout::Cards, [1240.0, 820.0]),
        (false, true, ListLayout::Posters, [1240.0, 820.0]),
        (true, false, ListLayout::Table, [900.0, 600.0]),
        (true, true, ListLayout::Table, [430.0, 700.0]),
    ] {
        let mut h = harness_with(store(), size, |s| {
            s.sidebar = sidebar;
            s.separate_tabs = separate;
            s.list_layout = grid;
            s.browse_layout = grid;
            s.season_icon = true;
        });
        for tab in Tab::ALL {
            act(&mut h, Act::Go(tab));
            assert_eq!(h.state().tab, tab);
        }
    }
}

#[test]
fn import_replaces_and_reports_bad_files() {
    let mut h = harness();
    let json = h.state().bg.store().export(Service::AniList).unwrap();
    act(&mut h, Act::Delete(r(1, MediaKind::Anime)));
    act(&mut h, Act::Import(json));
    assert!(h.state().entry_at(r(1, MediaKind::Anime)).is_some());
    act(&mut h, Act::Import("{}".into()));
    assert!(h.state().toast.as_ref().unwrap().error);
}

#[test]
fn muting_a_title_is_remembered() {
    let mut h = harness();
    let id = r(1, MediaKind::Anime);
    act(&mut h, Act::ToggleMute(id));
    assert_eq!(h.state().bg.store().settings().unwrap().notifications.muted, [id]);
    act(&mut h, Act::ToggleMute(id));
    assert!(h.state().s.notifications.muted.is_empty());
}

#[test]
fn large_lists_only_lay_out_visible_rows() {
    let s = Store::in_memory().unwrap();
    for i in 1..=3000 {
        s.put_entry(&entry(i, MediaKind::Anime, &format!("Title {i:04}"), ListStatus::Current, 1)).unwrap();
    }
    let h = harness_with(s, [1240.0, 820.0], |_| {});
    let rows = h.get_all_by_label_contains("Title ").count();
    assert!(rows > 4 && rows < 200, "{rows}");
}

#[test]
fn interactive_elements_have_names() {
    let mut h = harness();
    h.state_mut().details.entry(r(1, MediaKind::Anime)).or_default().data =
        Some(Rc::new(detail(1, MediaKind::Anime, "Lanterns Over Saltmarsh")));
    act(&mut h, Act::Open(Rc::new(media(1, MediaKind::Anime, "Lanterns Over Saltmarsh"))));
    for role in [egui::accesskit::Role::Button, egui::accesskit::Role::CheckBox, egui::accesskit::Role::Tab] {
        for n in h.query_all_by_role(role) {
            let label = n.accesskit_node().label().unwrap_or_default();
            assert!(!label.trim().is_empty(), "{n:?}");
        }
    }
}

#[test]
fn kioku_opens_from_accounts_and_the_palette() {
    // Tall enough for the end of the accounts page.
    let mut h = harness_with(store(), [1240.0, 2400.0], |_| {});
    act(&mut h, Act::Go(Tab::Settings));
    act(&mut h, Act::Push(Route::Settings(SettingsPage::Accounts)));
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Kioku").click();
    settle(&mut h);
    assert_eq!(h.state().routes.last(), Some(&Route::Kioku));
    assert!(h.query_by_label_contains("unofficial integration with Crunchyroll").is_some());

    let mut h = harness();
    h.state_mut().command = Some("kiok".into());
    settle(&mut h);
    click(&mut h, "Kioku · Watch history");
    assert_eq!(h.state().routes.last(), Some(&Route::Kioku));
}

#[test]
fn kioku_reviews_before_writing_and_never_imports_twice() {
    // Tall enough for the whole review.
    let mut h = harness_with(store(), [1240.0, 2400.0], |_| {});
    let source = history();
    let found = review(h.state(), &source);
    act(&mut h, Act::Push(Route::Kioku));
    let ctx = h.ctx.clone();
    h.state_mut().handle(&ctx, Event::KiokuReview(Ok(found)));
    settle(&mut h);

    // Nothing is written until the change is confirmed.
    assert_eq!(progress(&h, 1), 9);
    let shown = h.state().kioku.review.clone().unwrap();
    let ticked: Vec<(u64, bool)> = shown.changes.iter().map(|c| (c.media.id, c.selected)).collect();
    // The paused title with a gap is offered, but not ticked; the finished one is not offered.
    assert_eq!(ticked, [(1, true), (5, false)]);
    assert_eq!(shown.settled, ["I24"]);
    assert_eq!(shown.unmatched.len(), 1);
    let boxes: Vec<String> = h
        .query_all_by_role(egui::accesskit::Role::CheckBox)
        .map(|n| n.accesskit_node().label().unwrap_or_default())
        .collect();
    assert_eq!(boxes, ["Lanterns Over Saltmarsh", "Glass Meridian"]);
    assert!(h.query_by_label_contains("Paused in your list").is_some());
    assert!(h.query_by_label_contains("2 episodes in between are not in the history").is_some());

    click(&mut h, "Ignore");
    click(&mut h, "Mark 1 titles");
    assert!(h.query_by_label("Mark 1 titles?").is_some());
    assert_eq!(progress(&h, 1), 9);
    click(&mut h, "Mark");

    let e = h.state().entry_at(r(1, MediaKind::Anime)).unwrap();
    assert_eq!((e.status, e.progress), (ListStatus::Current, 11));
    let done = h.state().entry_at(r(3, MediaKind::Anime)).unwrap();
    assert_eq!((done.status, done.progress), (ListStatus::Completed, 24));
    let paused = h.state().entry_at(r(5, MediaKind::Anime)).unwrap();
    assert_eq!((paused.status, paused.progress), (ListStatus::Paused, 6));
    assert!(h.state().kioku.review.is_none());

    // The same history again: only what was neither applied nor ignored comes back.
    let again = review(h.state(), &source);
    let left: Vec<u64> = again.changes.iter().map(|c| c.media.id).collect();
    assert_eq!((left, again.unmatched.len(), again.settled.len()), (vec![5], 0, 0));
    assert_eq!(progress(&h, 1), 11);
}

#[test]
fn kioku_shows_how_far_a_sync_has_got() {
    use kuro_app::backend::KiokuStep;
    let mut h = harness();
    act(&mut h, Act::Push(Route::Kioku));
    h.state_mut().kioku.syncing = true;
    h.state_mut().kioku.started = Some(h.ctx.input(|i| i.time) - 75.0);
    let ctx = h.ctx.clone();
    h.state_mut().handle(&ctx, Event::KiokuProgress(KiokuStep::Reading(4)));
    settle(&mut h);
    assert!(h.query_by_label("Reading the history: page 4").is_some());
    assert!(h.query_by_label_contains("Elapsed: 1:1").is_some());
    h.state_mut().handle(&ctx, Event::KiokuProgress(KiokuStep::Searching { done: 3, total: 10 }));
    settle(&mut h);
    assert!(h.query_by_label("Looking up titles: 3 of 10").is_some());
    // The result ends the progress, whatever it is.
    h.state_mut().handle(&ctx, Event::KiokuReview(Err(kuro_api::ApiError::Unauthorized)));
    settle(&mut h);
    assert!(h.state().kioku.progress.is_none() && !h.state().kioku.syncing);
    assert!(h.state().kioku.took.is_some_and(|t| t >= 75.0));
    assert!(h.query_by_label_contains("session has expired").is_some());
}

#[test]
fn kioku_titles_can_be_chosen_by_hand_and_are_remembered() {
    use kuro_app::app::PickFrom;
    let mut h = harness_with(store(), [1240.0, 2400.0], |_| {});
    let found = review(h.state(), &history());
    act(&mut h, Act::Push(Route::Kioku));
    let ctx = h.ctx.clone();
    h.state_mut().handle(&ctx, Event::KiokuReview(Ok(found)));
    settle(&mut h);
    click(&mut h, "Choose title");
    let picker = h.state().kioku.picker.as_ref().unwrap();
    assert_eq!(picker.text, "A Show Nobody Has Heard Of");
    assert!(h.query_by_label("Which title is “A Show Nobody Has Heard Of”?").is_some());
    // Offline, the search answers through an event, as it would from the network.
    let text = picker.text.clone();
    h.state_mut().kioku.picker.as_mut().unwrap().sent = text.clone();
    let result = Ok(vec![media(4, MediaKind::Anime, "Midnight Ferry")]);
    h.state_mut().handle(&ctx, Event::KiokuFound { text, result });
    settle(&mut h);
    click(&mut h, "Midnight Ferry");

    let r = h.state().kioku.review.clone().unwrap();
    assert!(h.state().kioku.picker.is_none());
    assert!(r.unmatched.is_empty());
    let c = r.changes.iter().find(|c| c.media.id == 4).unwrap();
    assert_eq!(
        (c.before.as_ref().map(|b| b.status), c.after.progress, c.selected),
        (Some(ListStatus::Planning), 1, true)
    );

    // The next sync knows the choice.
    let again = review(h.state(), &history());
    assert!(again.unmatched.is_empty() && again.changes.iter().any(|c| c.media.id == 4));

    // A matched title can be changed the same way.
    let i = h.state().kioku.review.as_ref().unwrap().changes.iter().position(|c| c.media.id == 1).unwrap();
    act(&mut h, Act::KiokuPick(PickFrom::Change(i)));
    assert_eq!(h.state().kioku.picker.as_ref().unwrap().group.items.len(), 3);
}

#[test]
fn kioku_counted_episodes_are_marked() {
    let mut h = harness_with(store(), [1240.0, 2400.0], |_| {});
    let mut items: Vec<_> = (1..=3).map(|n| watched(&format!("M{n}"), "Midnight Ferry", 1, n)).collect();
    for i in &mut items {
        i.estimated = true;
    }
    let found = review(h.state(), &FakeSource(items));
    act(&mut h, Act::Push(Route::Kioku));
    let ctx = h.ctx.clone();
    h.state_mut().handle(&ctx, Event::KiokuReview(Ok(found)));
    settle(&mut h);
    let c = &h.state().kioku.review.as_ref().unwrap().changes[0];
    assert!(c.estimated && c.selected && c.after.progress == 3);
    assert!(h.query_by_label("Episode numbers counted from the history").is_some());
}

#[test]
fn kioku_folds_long_lists_of_series_not_found() {
    let mut h = harness_with(store(), [1240.0, 2400.0], |_| {});
    let items = (1..=5).map(|n| watched(&format!("U{n}"), &format!("Unknown Show {n}"), 1, 1)).collect();
    let found = review(h.state(), &FakeSource(items));
    act(&mut h, Act::Push(Route::Kioku));
    let ctx = h.ctx.clone();
    h.state_mut().handle(&ctx, Event::KiokuReview(Ok(found)));
    settle(&mut h);
    assert!(h.query_by_label_contains("5 series not found").is_some());
    assert!(h.query_by_label_contains("Unknown Show 1").is_none());
    click(&mut h, "Show");
    assert!(h.query_by_label_contains("Unknown Show 1").is_some());
    click(&mut h, "Ignore all");
    let r = h.state().kioku.review.as_ref().unwrap();
    assert!(r.unmatched.is_empty());
    assert_eq!(r.dismissed.len(), 5);
}

#[test]
fn kioku_prime_video_series_without_a_name_show_an_episode() {
    use kuro_api::kioku::HistorySource;
    let file = include_bytes!("../../api/tests/fixtures/kioku/amazon.csv");
    let items = kuro_api::kioku::amazon::Amazon.parse(file).unwrap();
    let mut h = harness_with(store(), [1240.0, 2400.0], |_| {});
    let found = review(h.state(), &FakeSource(items));
    act(&mut h, Act::Push(Route::Kioku));
    assert_eq!(h.get_all_by_label("Import file").count(), 2);
    let ctx = h.ctx.clone();
    h.state_mut().handle(&ctx, Event::KiokuReview(Ok(found)));
    settle(&mut h);
    let r = h.state().kioku.review.as_ref().unwrap();
    let nameless: Vec<_> = r.unmatched.iter().filter(|g| g.query() == "Temporada 1").collect();
    assert_eq!(nameless.len(), 2);
    if r.unmatched.len() > 3 {
        click(&mut h, "Show");
    }
    assert!(h.query_by_label("For example: Sin nombre").is_some());
    assert!(h.query_by_label("For example: Otro").is_some());
}

#[test]
fn automatic_backups_are_dated_and_old_ones_are_removed() {
    let dir = std::env::temp_dir().join(format!("kuro-backups-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for old in ["kuro-backup-2020-01-01.json", "kuro-backup-2020-01-02.json", "notes.json"] {
        std::fs::write(dir.join(old), "{}").unwrap();
    }
    let folder = dir.display().to_string();
    let mut h = harness_with(store(), [1240.0, 820.0], |s| {
        s.backup.every = kuro_core::settings::Every::Daily;
        s.backup.folder = folder;
        s.backup.keep = 2;
    });
    for _ in 0..100 {
        if h.state().last_backup.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        settle(&mut h);
    }
    let today = kuro_core::backup::file_name(kuro_app::app::today());
    assert_eq!(h.state().last_backup.as_deref(), Some(today.as_str()));
    let mut names: Vec<_> =
        std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    names.sort();
    assert_eq!(names, ["kuro-backup-2020-01-02.json", today.as_str(), "notes.json"]);
    let text = std::fs::read_to_string(dir.join(&today)).unwrap();
    assert!(text.contains("Lanterns Over Saltmarsh"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn action_links_change_the_list_only_when_allowed() {
    let mut h = harness();
    assert!(h.state_mut().open_link("kuro://step/anilist/anime/1?by=2"));
    settle(&mut h);
    assert_eq!(progress(&h, 1), 9, "off by default");

    h.state_mut().s.link_actions = true;
    assert!(h.state_mut().open_link("kuro://step/anilist/anime/1?by=2"));
    settle(&mut h);
    assert_eq!(progress(&h, 1), 11);
    assert!(h.state_mut().open_link("kuro://step/anilist/anime/1?by=-1"));
    settle(&mut h);
    assert_eq!(progress(&h, 1), 10);

    // A finished title stays finished, and titles outside the list are left alone.
    assert!(h.state_mut().open_link("kuro://step/anilist/anime/3"));
    assert!(h.state_mut().open_link("kuro://step/anilist/anime/999"));
    settle(&mut h);
    assert_eq!(progress(&h, 3), 24);
    assert!(h.state().entry_at(r(999, MediaKind::Anime)).is_none());

    assert!(h.state_mut().open_link("kuro://binge/anilist/anime/2"));
    settle(&mut h);
    assert_eq!(h.state().binge, Some(r(2, MediaKind::Anime)));
}

#[test]
fn the_episode_calendar_is_written() {
    let path = std::env::temp_dir().join(format!("kuro-calendar-{}.ics", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let file = path.display().to_string();
    let mut h = harness_with(store(), [1240.0, 820.0], |s| s.calendar_file = file);
    let read = |h: &mut H| {
        for _ in 0..100 {
            if let Ok(t) = std::fs::read_to_string(&path)
                && h.state().calendar_error.is_none()
            {
                return t;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
            settle(h);
        }
        panic!("the calendar was not written");
    };
    let text = read(&mut h);
    assert!(text.contains("SUMMARY:Lanterns Over Saltmarsh · Episode 10"), "{text}");
    assert_eq!(text.matches("BEGIN:VEVENT").count(), 1);
    assert!(h.state().calendar_error.is_none());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn titles_left_behind_are_mentioned_once() {
    let s = store();
    let mut old = entry(7, MediaKind::Anime, "Quiet Harbor", ListStatus::Current, 4);
    old.updated_at = kuro_app::backend::now() - 30 * 86_400;
    s.put_entry(&old).unwrap();
    let key = kuro_core::reminders::stale_key(&old);
    let mut h = harness_with(s, [1240.0, 820.0], |s| s.notifications.stale_days = 14);
    settle(&mut h);
    assert!(h.state().bg.store().was_notified(&key).unwrap());
    let fresh = kuro_core::reminders::stale_key(&h.state().entry_at(r(1, MediaKind::Anime)).unwrap());
    assert!(!h.state().bg.store().was_notified(&fresh).unwrap(), "recent titles are left alone");
}

#[test]
fn the_widget_shows_what_was_chosen_and_stays_open() {
    let mut h = harness();
    h.state_mut().s.widget.open = true;
    for (up_next, watching, stats) in [(true, true, true), (false, false, false), (true, false, false)] {
        let w = &mut h.state_mut().s.widget;
        (w.up_next, w.watching, w.stats, w.rows) = (up_next, watching, stats, 3);
        settle(&mut h);
    }
    assert!(h.state().bg.store().settings().unwrap().widget.open, "remembered for the next start");
}

#[test]
fn an_automatic_kioku_check_waits_for_review_and_leaves_nothing_when_empty() {
    let mut h = harness();
    let ctx = h.ctx.clone();
    let found = review(h.state(), &history());
    assert!(!found.changes.is_empty());
    h.state_mut().kioku.background = true;
    h.state_mut().handle(&ctx, Event::KiokuReview(Ok(found)));
    settle(&mut h);
    assert!(h.state().kioku.review.is_some(), "kept for the user to review");
    assert_eq!(progress(&h, 1), 9, "nothing written");

    let mut empty = review(h.state(), &FakeSource(Vec::new()));
    empty.changes.clear();
    h.state_mut().kioku.review = None;
    h.state_mut().kioku.background = true;
    h.state_mut().handle(&ctx, Event::KiokuReview(Ok(empty)));
    assert!(h.state().kioku.review.is_none() && !h.state().kioku.syncing);

    assert!(h.state_mut().open_link("kuro://tab/kioku"));
    settle(&mut h);
    assert_eq!(h.state().routes.last(), Some(&Route::Kioku));
}

#[test]
fn history_files_in_downloads_are_offered_until_handled() {
    let dir = std::env::temp_dir().join(format!("kuro-downloads-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let csv = "Title,Date\n\"Lanterns Over Saltmarsh: Season 1: Ten\",\"11/20/26\"\n";
    std::fs::write(dir.join("NetflixViewingHistory.csv"), csv).unwrap();
    std::fs::write(dir.join("netflix-notes.csv"), "not a history").unwrap();
    let mut h = harness();
    let wait = |h: &mut H| {
        for _ in 0..100 {
            if h.state().kioku.found.is_some() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
            settle(h);
        }
    };
    h.state().bg.scan_downloads(dir.clone(), 0);
    wait(&mut h);
    let found = h.state().kioku.found.clone().expect("found");
    assert_eq!(
        (found.platform, found.name().as_str()),
        (kuro_core::kioku::Platform::Netflix, "NetflixViewingHistory.csv")
    );

    act(&mut h, Act::Push(Route::Kioku));
    assert!(h.query_by_label_contains("NetflixViewingHistory.csv").is_some());
    click(&mut h, "Not this file");
    assert!(h.state().kioku.found.is_none());

    // Forgotten files are not offered again.
    h.state().bg.scan_downloads(dir.clone(), 0);
    wait(&mut h);
    assert!(h.state().kioku.found.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn toasts_use_their_id_once_and_close_on_click() {
    let mut h = harness();
    let clash = |h: &H| {
        h.output()
            .shapes
            .iter()
            .any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text().contains("use of widget ID")))
    };
    for error in [false, true] {
        let text = "Something long enough to wrap onto a second line of the toast, surely.";
        if error {
            h.state_mut().say_error(text);
        } else {
            h.state_mut().say(text);
        }
        settle(&mut h);
        assert!(!clash(&h), "error: {error}");
        click(&mut h, text);
        assert!(h.state().toast.is_none(), "a click closes it (error: {error})");
    }
}

#[test]
fn a_title_menu_works_from_the_keyboard() {
    use egui::{Key, Modifiers};
    let mut h = harness();
    let row = "Lanterns Over Saltmarsh, Watching";
    h.get_by_label_contains(row).focus();
    settle(&mut h);
    assert!(h.query_by_label("Add one +").is_none(), "closed");

    // The Menu key opens it with the focus on its first entry; Enter runs it.
    h.key_press(Key::Menu);
    settle(&mut h);
    assert!(h.query_by_label("Add one +").is_some(), "opened by keyboard");
    h.key_press(Key::Enter);
    settle(&mut h);
    assert_eq!(progress(&h, 1), 10);
    assert!(h.query_by_label("Add one +").is_none(), "closed after the action");

    // The focus is back on the title, so Shift+F10 opens it again; arrows reach the
    // status submenu (Add one, Remove one, Status), Right enters it.
    h.key_press_modifiers(Modifiers::SHIFT, Key::F10);
    settle(&mut h);
    assert!(h.query_by_label("Add one +").is_some(), "the title kept the focus");
    for k in [Key::ArrowDown, Key::ArrowDown, Key::ArrowRight, Key::ArrowDown, Key::ArrowDown, Key::Enter] {
        h.key_press(k);
        settle(&mut h);
    }
    let e = h.state().entry_at(r(1, MediaKind::Anime)).unwrap();
    assert_eq!(e.status, ListStatus::Paused, "Watching → Completed → Paused");

    // Escape closes it without doing anything, and leaves the list where it was.
    h.key_press(Key::Menu);
    settle(&mut h);
    h.key_press(Key::Escape);
    settle(&mut h);
    assert!(h.query_by_label("Add one +").is_none());
    assert!(h.state().routes.is_empty());
}

#[test]
fn a_title_menu_answers_to_the_key_each_entry_shows() {
    use egui::Key;
    let mut h = harness();
    // Another title is open beside the list: its own + must not fire.
    h.state_mut().details.entry(r(2, MediaKind::Anime)).or_default().data =
        Some(Rc::new(detail(2, MediaKind::Anime, "The Mapmaker")));
    act(&mut h, Act::Push(Route::Media(r(2, MediaKind::Anime))));
    let open = progress(&h, 2);
    let row = "Lanterns Over Saltmarsh, Watching";
    let menu = |h: &mut H| {
        h.get_by_label_contains(row).focus();
        settle(h);
        h.key_press(Key::Menu);
        settle(h);
        assert!(h.query_by_label("Add one +").is_some(), "menu open");
    };
    menu(&mut h);
    h.key_press(Key::Plus);
    settle(&mut h);
    assert_eq!((progress(&h, 1), progress(&h, 2)), (10, open), "only the menu's title moves");

    menu(&mut h);
    h.key_press(Key::E);
    settle(&mut h);
    assert!(h.state().editor.as_ref().is_some_and(|e| e.entry.media.id == 1), "E edits the menu's title");
    h.state_mut().editor = None;
    settle(&mut h);

    menu(&mut h);
    h.key_press(Key::Num3);
    settle(&mut h);
    assert_eq!(h.state().entry_at(r(1, MediaKind::Anime)).unwrap().status, ListStatus::Paused);
}

#[test]
fn friends_show_what_they_watch_and_the_titles_in_common() {
    use kuro_core::friends::{Friend, FriendProfile};
    let mut h = harness();
    h.state_mut().friend_text = "https://myanimelist.net/profile/Reader".into();
    act(&mut h, Act::AddFriend);
    let friend = Friend { service: Service::Mal, name: "Reader".into() };
    assert_eq!(h.state().s.friends, std::slice::from_ref(&friend));
    assert!(h.state().friend_text.is_empty());
    h.state_mut().friend_text = "@reader".into();
    h.state_mut().friend_service = Service::Mal;
    act(&mut h, Act::AddFriend);
    assert_eq!(h.state().s.friends.len(), 1, "names ignore case");

    act(&mut h, Act::Go(Tab::Friends));
    assert!(h.query_by_label_contains("Open Reader’s profile").is_some());
    // Their list is on MyAnimeList; one title is also on the user's AniList list.
    let theirs = |id: u64, title: &str, score| {
        let mut m = media(id, MediaKind::Anime, title);
        (m.service, m.anilist_id, m.mal_id) = (Service::Mal, None, Some(id));
        let mut e = ListEntry::new(m);
        (e.status, e.progress, e.score) = (ListStatus::Current, 3, score);
        e
    };
    let profile = FriendProfile {
        name: "Reader".into(),
        avatar: None,
        entries: vec![theirs(1001, "Lanterns Over Saltmarsh", 80), theirs(5555, "Paper Comet", 0)],
    };
    let ctx = h.ctx.clone();
    h.state_mut().handle(&ctx, Event::Friend { friend: friend.clone(), result: Ok(profile), fresh: true });
    settle(&mut h);
    assert!(h.query_by_label_contains("Ep 3 Paper Comet").is_some());

    click(&mut h, "Open Reader’s profile");
    assert_eq!(h.state().routes.last(), Some(&Route::Friend(friend.clone())));
    assert!(h.query_by_label_contains("In common · 1").is_some());
    assert!(h.query_by_label_contains("Lanterns Over Saltmarsh, Them 8 · You").is_some());

    act(&mut h, Act::RemoveFriend(friend));
    assert!(h.state().s.friends.is_empty());
    assert!(h.state().routes.is_empty());
}

#[test]
fn friends_compare_tastes_and_recommend_what_they_loved() {
    use kuro_core::friends::{Friend, FriendProfile};
    let mut h = harness();
    let mut mine = (*h.state().entry_at(r(2, MediaKind::Anime)).unwrap()).clone();
    mine.score = 70;
    h.state_mut().save(&mine);
    let friend = Friend { service: Service::Mal, name: "Reader".into() };
    h.state_mut().s.friends.push(friend.clone());
    let theirs = |id: u64, title: &str, status, score| {
        let mut m = media(id, MediaKind::Anime, title);
        (m.service, m.anilist_id, m.mal_id) = (Service::Mal, None, Some(id));
        let mut e = ListEntry::new(m);
        (e.status, e.score) = (status, score);
        e
    };
    // Scored by both: 90/80, 85/85 and 70/60.
    let entries = vec![
        theirs(1001, "Lanterns Over Saltmarsh", ListStatus::Completed, 80),
        theirs(1003, "Iron Orchard", ListStatus::Completed, 85),
        theirs(1002, "The Mapmaker", ListStatus::Current, 60),
        theirs(7777, "Quiet Signal", ListStatus::Completed, 95),
        theirs(8888, "Loud Noise", ListStatus::Completed, 60),
    ];
    let profile = FriendProfile { name: "Reader".into(), avatar: None, entries };
    let ctx = h.ctx.clone();
    h.state_mut().handle(&ctx, Event::Friend { friend: friend.clone(), result: Ok(profile), fresh: true });
    act(&mut h, Act::Go(Tab::Friends));
    assert!(h.query_by_label_contains("93% taste match").is_some());
    assert!(h.query_by_label("Your friends recommend").is_some());
    assert!(h.query_by_label_contains("★ 9.5 · Reader Quiet Signal").is_some());
    assert!(h.query_by_label_contains("Loud Noise").is_none(), "only what they scored well");

    act(&mut h, Act::Push(Route::Friend(friend)));
    assert!(h.query_by_label("Loved by Reader, not in your list").is_some());
    assert!(h.query_by_label_contains("★ 9.5 Quiet Signal").is_some());
    assert!(h.query_by_label("0.7 lower than you on average").is_some());
    assert!(h.query_by_label("93%").is_some());
}

#[test]
fn a_missing_friend_says_so_and_can_be_retried() {
    use kuro_core::friends::Friend;
    let mut h = harness();
    h.state_mut().friend_text = "nobody".into();
    act(&mut h, Act::AddFriend);
    act(&mut h, Act::Go(Tab::Friends));
    let friend = Friend { service: h.state().s.primary, name: "nobody".into() };
    let ctx = h.ctx.clone();
    h.state_mut()
        .handle(&ctx, Event::Friend { friend: friend.clone(), result: Err(ApiError::NotFound), fresh: true });
    settle(&mut h);
    assert!(h.query_by_label_contains("No user with this name").is_some());
    click(&mut h, "Retry");
    assert!(h.state().friends[&friend].loading);
}
