#![allow(clippy::unwrap_used)]
//! Every screen must fit any window: nothing sticks out, nothing overlaps.

mod common;

use std::rc::Rc;

use common::*;
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use kuro_app::app::{Act, App, Feed, Route, SettingsPage, Tab};
use kuro_core::settings::ListLayout;
use kuro_core::*;

type H = Harness<'static, App>;

const WIDTHS: [f32; 13] =
    [340.0, 380.0, 460.0, 560.0, 619.0, 620.0, 760.0, 860.0, 999.0, 1000.0, 1240.0, 1700.0, 2560.0];
const ROLES: [Role; 6] =
    [Role::Button, Role::Tab, Role::CheckBox, Role::RadioButton, Role::ComboBox, Role::TextInput];

/// Names and places of every control.
fn controls(h: &H) -> Vec<(String, egui::Rect)> {
    let mut out = Vec::new();
    for role in ROLES {
        for n in h.query_all_by_role(role) {
            let label = n.accesskit_node().label().unwrap_or_default();
            out.push((format!("{role:?} {label}"), n.rect()));
        }
    }
    out
}

fn check(h: &H, w: f32, height: f32, what: &str) {
    let all = controls(h);
    assert!(!all.is_empty(), "{what}: empty screen");
    for (name, r) in &all {
        let fits = r.left() >= -1.0 && r.right() <= w + 1.0;
        assert!(fits, "{what} at {w}x{height}: {name} sticks out: {r:?}");
        assert!(r.width() > 4.0, "{what} at {w}x{height}: {name} has no room: {r:?}");
    }
    let clash =
        h.output().shapes.iter().any(
            |s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text().contains("use of widget ID")),
        );
    assert!(!clash, "{what} at {w}x{height}: widget ids clash");
    // What scrolls under the edges of the window is clipped, so it cannot collide.
    let inside: Vec<_> = all.iter().filter(|c| c.1.bottom() < height - 64.0 && c.1.top() > 0.0).collect();
    for (i, (an, a)) in inside.iter().enumerate() {
        for (bn, b) in &inside[i + 1..] {
            let hit = a.intersect(*b);
            let nested = a.expand(1.0).contains_rect(*b) || b.expand(1.0).contains_rect(*a);
            let overlap = hit.is_positive() && hit.width() > 3.0 && hit.height() > 3.0;
            assert!(!overlap || nested, "{what} at {w}x{height}: {an} {a:?} overlaps {bn} {b:?}");
        }
    }
}

fn resize(h: &mut H, w: f32, height: f32) {
    h.set_size(egui::vec2(w, height));
    settle(h);
}

fn feed(kind: MediaKind) -> Feed {
    let items = (200..230)
        .map(|i| {
            let mut m = media(i, kind, &format!("A Rather Long Title That Needs Two Lines Number {i}"));
            m.synopsis = Some("Words of a synopsis that goes on for a while. ".repeat(8));
            Rc::new(m)
        })
        .collect();
    Feed { items, started: true, page: 1, ..Feed::default() }
}

fn plain_detail(id: u64, kind: MediaKind, title: &str) -> MediaDetail {
    // Shelves scroll sideways on purpose, so they are left out here.
    MediaDetail {
        relations: Vec::new(),
        recommendations: Vec::new(),
        characters: Vec::new(),
        staff: Vec::new(),
        ..detail(id, kind, title)
    }
}

fn many() -> Store {
    let s = store();
    for i in 100..140 {
        let status = ListStatus::ALL[i as usize % 5];
        let title = format!("Another Fairly Long Title For The Grid {i}");
        s.put_entry(&entry(i, MediaKind::Anime, &title, status, (i % 20) as u32)).unwrap();
    }
    s
}

fn app_in(language: &str, w: f32) -> H {
    let language = language.to_owned();
    harness_with(many(), [w, 800.0], move |s| s.language = language)
}

#[test]
fn every_tab_fits_every_width_in_every_layout() {
    for language in ["en", "de"] {
        let mut h = app_in(language, 1240.0);
        h.state_mut().season.feed = feed(MediaKind::Anime);
        h.state_mut().ranking.feed = feed(MediaKind::Anime);
        h.state_mut().search.feed = feed(MediaKind::Anime);
        h.state_mut().search.text = "x".into();
        h.state_mut().search.sent = "x".into();
        h.state_mut().lists[0].filter.status = None;
        for layout in ListLayout::ALL {
            h.state_mut().s.list_layout = layout;
            h.state_mut().s.browse_layout = layout;
            for tab in Tab::ALL {
                h.state_mut().act(Act::Go(tab));
                for w in WIDTHS {
                    resize(&mut h, w, 800.0);
                    check(&h, w, 800.0, &format!("{language} {layout:?} {tab:?}"));
                }
            }
        }
    }
}

#[test]
fn an_open_title_fits_beside_or_instead_of_the_list() {
    for (id, kind, title) in
        [(1, MediaKind::Anime, "Lanterns Over Saltmarsh"), (20, MediaKind::Manga, "Ink & Ember")]
    {
        for language in ["en", "de", "fr"] {
            let mut h = app_in(language, 1240.0);
            let r = MediaRef { service: Service::AniList, kind, id };
            h.state_mut().details.entry(r).or_default().data = Some(Rc::new(plain_detail(id, kind, title)));
            if kind == MediaKind::Manga {
                h.state_mut().act(Act::Go(Tab::Manga));
                settle(&mut h);
            }
            h.state_mut().act(Act::Open(Rc::new(media(id, kind, title))));
            for full in [false, true] {
                h.state_mut().detail_full = full;
                for w in WIDTHS {
                    resize(&mut h, w, 800.0);
                    check(&h, w, 800.0, &format!("{language} detail full={full}"));
                }
            }
        }
    }
}

#[test]
fn the_entry_card_stays_compact() {
    for (id, kind, label) in [(1, MediaKind::Anime, "Episodes"), (20, MediaKind::Manga, "Chapters")] {
        for w in WIDTHS {
            let mut h = app_in("en", w);
            let r = MediaRef { service: Service::AniList, kind, id };
            h.state_mut().details.entry(r).or_default().data = Some(Rc::new(plain_detail(id, kind, "Title")));
            h.state_mut().act(Act::Open(Rc::new(media(id, kind, "Title"))));
            settle(&mut h);
            let edit = h.get_by_label("Edit").rect().bottom();
            let plus = h.get_by_label(&format!("Add one: {label}")).rect().top();
            assert!(plus - edit < 70.0, "{w}: {} between the edit button and the stepper", plus - edit);
        }
    }
}

#[test]
fn equal_titles_do_not_share_controls() {
    let s = Store::in_memory().unwrap();
    for i in 1..=6 {
        s.put_entry(&entry(i, MediaKind::Anime, "Same Name", ListStatus::Current, i as u32)).unwrap();
    }
    for layout in ListLayout::ALL {
        let mut ids = std::collections::HashSet::new();
        let copy = Store::in_memory().unwrap();
        for e in s.entries(Service::AniList, MediaKind::Anime).unwrap() {
            copy.put_entry(&e).unwrap();
        }
        let mut h = harness_with(copy, [1400.0, 900.0], |s| s.list_layout = layout);
        settle(&mut h);
        for n in h.query_all_by_role(Role::Button) {
            assert!(
                ids.insert(n.accesskit_node().id()),
                "{layout:?}: {:?} appears twice",
                n.accesskit_node().label()
            );
        }
        let warned = h.output().shapes.iter().any(
            |s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text().contains("use of widget ID")),
        );
        assert!(!warned, "{layout:?}: widget ids clash");
    }
}

#[test]
fn every_settings_page_fits() {
    for language in ["en", "de", "fr"] {
        let mut h = app_in(language, 1240.0);
        h.state_mut().act(Act::Go(Tab::Settings));
        settle(&mut h);
        for page in SettingsPage::ALL {
            h.state_mut().routes.clear();
            h.state_mut().act(Act::Push(Route::Settings(page)));
            for w in WIDTHS {
                resize(&mut h, w, 800.0);
                check(&h, w, 800.0, &format!("{language} {page:?}"));
            }
        }
    }
}

#[test]
fn dialogs_fit_the_smallest_window() {
    for (w, height) in [(340.0, 400.0), (340.0, 800.0), (900.0, 400.0)] {
        let mut h = app_in("de", w);
        resize(&mut h, w, height);
        h.state_mut().act(Act::Edit(Rc::new(media(20, MediaKind::Manga, "Ink & Ember"))));
        settle(&mut h);
        settle(&mut h);
        let save = h.get_by_label("Speichern").rect();
        let window = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, height));
        assert!(window.contains_rect(save), "save button at {save:?} in {w}x{height}");
        h.state_mut().editor = None;
        h.state_mut().act(Act::AskDelete(Rc::new(media(1, MediaKind::Anime, "Lanterns Over Saltmarsh"))));
        settle(&mut h);
        let yes = h.get_by_label("Entfernen").rect();
        assert!(window.contains_rect(yes), "confirm button at {yes:?} in {w}x{height}");
    }
}

#[test]
fn wide_windows_show_more_columns() {
    let rows = |w: f32| {
        let mut h = app_in("en", w);
        h.state_mut().lists[0].filter.status = None;
        settle(&mut h);
        let tops: Vec<i32> = h
            .get_all_by_label_contains(" / 24")
            .filter(|n| n.accesskit_node().role() == Role::Button)
            .map(|n| n.rect().top() as i32)
            .collect();
        tops.iter().filter(|t| **t == tops[0]).count()
    };
    assert_eq!(rows(400.0), 1);
    assert!(rows(1240.0) >= 2);
    assert!(rows(1920.0) >= 4);
}

#[test]
fn opening_a_title_keeps_the_list_visible_when_there_is_room() {
    let mut h = app_in("en", 1400.0);
    h.state_mut().act(Act::Open(Rc::new(media(1, MediaKind::Anime, "Lanterns Over Saltmarsh"))));
    settle(&mut h);
    assert!(
        h.query_by_label_contains("The Mapmaker and the Slowly Moving Northern Coast, Watching").is_some()
    );
    h.get_by_label("Use the whole window").click();
    settle(&mut h);
    assert!(
        h.query_by_label_contains("The Mapmaker and the Slowly Moving Northern Coast, Watching").is_none()
    );
    h.get_by_label("Show beside the list").click();
    resize(&mut h, 700.0, 800.0);
    assert!(
        h.query_by_label_contains("The Mapmaker and the Slowly Moving Northern Coast, Watching").is_none()
    );
    h.get_by_label("Back").click();
    settle(&mut h);
    assert!(
        h.query_by_label_contains("The Mapmaker and the Slowly Moving Northern Coast, Watching").is_some()
    );
}

#[test]
fn resizing_keeps_the_place_in_the_list() {
    let s = Store::in_memory().unwrap();
    for i in 1..=400 {
        s.put_entry(&entry(i, MediaKind::Anime, &format!("Title {i:04}"), ListStatus::Current, 1)).unwrap();
    }
    let mut h = harness_with(s, [1600.0, 800.0], |_| {});
    let first = |h: &H| {
        h.get_all_by_label_contains(", Watching, 1 / 24")
            .filter(|n| n.rect().top() > 90.0)
            .min_by_key(|n| (n.rect().top() as i32, n.rect().left() as i32))
            .and_then(|n| n.accesskit_node().label())
            .unwrap()
    };
    h.hover_at(egui::pos2(700.0, 500.0));
    for _ in 0..12 {
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -400.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
        });
        h.run_steps(6);
    }
    h.run_steps(40);
    let number = |s: &str| s[6..10].parse::<i32>().unwrap();
    let before = number(&first(&h));
    assert!(before > 20, "did not scroll: {before}");
    resize(&mut h, 800.0, 800.0);
    h.run_steps(10);
    let after = number(&first(&h));
    assert!((after - before).abs() <= 6, "was at {before}, now at {after}");
}

#[test]
fn table_headers_sort() {
    let mut h = harness_with(store(), [1500.0, 800.0], |s| s.list_layout = ListLayout::Table);
    h.state_mut().lists[0].filter.status = None;
    settle(&mut h);
    h.get_by_label("Sort by: Title").click();
    settle(&mut h);
    assert_eq!(h.state().s.sort, kuro_core::list::SortKey::Title);
    assert!(h.state().s.sort_asc);
    let y = |h: &H, s: &str| h.get_by_label_contains(&format!("{s}, ")).rect().top();
    assert!(y(&h, "Dustfall") < y(&h, "Midnight Ferry"));
    h.get_by_label("Sort by: Title").click();
    settle(&mut h);
    assert!(y(&h, "Dustfall") > y(&h, "Midnight Ferry"));
}

#[test]
fn status_chips_filter_the_list() {
    let mut h = harness_with(store(), [1500.0, 800.0], |_| {});
    h.get_by_label("Dropped 1").click();
    settle(&mut h);
    assert!(h.query_by_label_contains("Dustfall, Dropped").is_some());
    assert!(h.query_by_label_contains("Lanterns Over Saltmarsh, Watching").is_none());
}
