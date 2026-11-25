#![allow(clippy::unwrap_used)]
//! Drives the interface the way a person would, without network or GPU.

mod common;

use std::rc::Rc;

use common::*;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use mango_api::{ApiError, Page};
use mango_app::app::{Act, App, Feed, Load, Route, SettingsPage, Tab};
use mango_app::backend::Event;
use mango_core::overview::Section;
use mango_core::settings::{GestureAction, ListLayout, Theme};
use mango_core::*;

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
fn keyboard_shortcuts() {
    let mut h = harness();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Num4);
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
fn links_open_titles_searches_and_tabs() {
    let mut h = harness();
    assert!(h.state_mut().open_link("https://anilist.co/anime/3"));
    settle(&mut h);
    assert_eq!(h.state().routes.last(), Some(&Route::Media(r(3, MediaKind::Anime))));
    assert!(h.state_mut().open_link("mango://search/manga?q=ink"));
    settle(&mut h);
    assert_eq!((h.state().tab, h.state().search.sent.as_str()), (Tab::Search, "ink"));
    assert!(h.state_mut().open_link("mango://tab/ranking"));
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
        let tokens = mango_api::Tokens { access: "t".into(), refresh: None, expires_at: i64::MAX };
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
    let tokens = mango_api::Tokens { access: "t".into(), refresh: None, expires_at: i64::MAX };
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
