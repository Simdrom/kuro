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
        s.backup.every = kuro_core::settings::BackupEvery::Daily;
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
