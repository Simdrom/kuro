#![allow(clippy::unwrap_used)]
//! Renders every screen to PNG files for review: `cargo test --test shots -- --ignored`.

mod common;

use std::rc::Rc;

use common::*;
use kuro_app::app::{Act, App, Load, Route, SettingsPage, Tab};
use kuro_core::overview::Section;
use kuro_core::settings::{ListLayout, Theme};
use kuro_core::{MediaKind, Store};

fn shot(h: &mut egui_kittest::Harness<'static, App>, name: &str) {
    settle(h);
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/shots");
    std::fs::create_dir_all(&dir).unwrap();
    h.render().unwrap().save(dir.join(format!("{name}.png"))).unwrap();
}

#[test]
#[ignore = "needs a GPU"]
fn screens() {
    let mut h = harness();
    let m = Rc::new(media(1, MediaKind::Anime, "Lanterns Over Saltmarsh"));
    let r = m.media_ref();
    h.state_mut().details.entry(r).or_default().data =
        Some(Rc::new(detail(1, MediaKind::Anime, "Lanterns Over Saltmarsh")));
    h.state_mut().act(Act::Open(m.clone()));
    shot(&mut h, "anime");
    h.state_mut().act(Act::Edit(m.clone()));
    shot(&mut h, "editor");
    h.state_mut().editor = None;
    let titles = vec![m.as_ref().clone(), media(3, MediaKind::Anime, "Iron Orchard")];
    h.state_mut().overviews.insert(r, Load::Ready(Rc::new(vec![(Section::Main, titles)])));
    h.state_mut().act(Act::Push(Route::Overview(r)));
    shot(&mut h, "overview");
    h.state_mut().act(Act::Go(Tab::Settings));
    for page in SettingsPage::ALL {
        h.state_mut().routes.clear();
        h.state_mut().act(Act::Push(Route::Settings(page)));
        shot(&mut h, &format!("settings-{page:?}"));
    }
    h.state_mut().routes.clear();
    h.state_mut().act(Act::Push(Route::Stats(MediaKind::Anime)));
    shot(&mut h, "stats");
    h.state_mut().routes.clear();
    h.state_mut().act(Act::Push(Route::Kioku));
    shot(&mut h, "kioku");
    let found = review(h.state(), &history());
    h.state_mut().kioku.review = Some(found);
    shot(&mut h, "kioku-review");
    h.state_mut().act(Act::KiokuPick(kuro_app::app::PickFrom::Unmatched(0)));
    let titles =
        vec![media(4, MediaKind::Anime, "Midnight Ferry"), media(5, MediaKind::Anime, "Glass Meridian")];
    settle(&mut h);
    h.state_mut().kioku.picker.as_mut().unwrap().results = Some(Ok(titles));
    shot(&mut h, "kioku-picker");
    h.state_mut().kioku.picker = None;
    h.state_mut().routes.clear();
    h.state_mut().act(Act::Go(Tab::Calendar));
    shot(&mut h, "calendar");
    // The same episode, a day later: a delay.
    let mut late = media(1, MediaKind::Anime, "Lanterns Over Saltmarsh");
    let was = h.state().entry_at(r).unwrap().media.next_airing.unwrap();
    late.release = kuro_core::ReleaseStatus::Releasing;
    late.next_airing = Some(kuro_core::Airing { episode: was.episode, at: was.at + 86_400 });
    late.schedule.following = Some(was.at + 15 * 86_400);
    let ctx = h.ctx.clone();
    h.state_mut().handle(&ctx, kuro_app::backend::Event::Airing(Ok(vec![late])));
    shot(&mut h, "calendar-changes");
    friends(&mut h);
    h.state_mut().act(Act::Open(m.clone()));
    shot(&mut h, "calendar-open");
    h.state_mut().routes.clear();
    h.state_mut().act(Act::Go(Tab::Manga));
    settle(&mut h);
    h.state_mut().s.theme = Theme::Light;
    h.state_mut().lists[0].filter.status = None;
    h.state_mut().lists[1].filter.status = None;
    h.state_mut().act(Act::Open(Rc::new(media(20, MediaKind::Manga, "Ink & Ember"))));
    shot(&mut h, "manga-light");
    h.state_mut().s.sidebar = false;
    h.state_mut().s.list_layout = ListLayout::Posters;
    h.state_mut().act(Act::Go(Tab::Anime));
    shot(&mut h, "grid-light");
    h.state_mut().command = Some("i".into());
    shot(&mut h, "command");

    let mut h = harness_with(store(), [430.0, 800.0], |_| {});
    shot(&mut h, "narrow");
    let mut h = harness_with(Store::in_memory().unwrap(), [1000.0, 700.0], |_| {});
    shot(&mut h, "welcome");
}

#[test]
#[ignore = "needs a GPU"]
fn sizes() {
    let titles =
        ["Lanterns Over Saltmarsh", "Iron Orchard", "The Slowly Moving Northern Coast", "Glass Meridian"];
    let s = store();
    for i in 100..130u64 {
        let status = kuro_core::ListStatus::ALL[i as usize % 5];
        let mut e = entry(i, MediaKind::Anime, titles[i as usize % 4], status, (i % 23) as u32);
        if i % 3 == 0 {
            let at = kuro_app::backend::now() + 90_000;
            e.media.next_airing = Some(kuro_core::Airing { episode: 22, at });
        }
        s.put_entry(&e).unwrap();
    }
    let mut h = harness_with(s, [1426.0, 1000.0], |s| s.theme = Theme::Dark);
    let items: Vec<_> = (200..230u64)
        .map(|i| {
            let mut m = media(i, MediaKind::Anime, titles[i as usize % 4]);
            m.synopsis = Some("A lighthouse keeper finds a map that redraws itself every night. ".repeat(6));
            Rc::new(m)
        })
        .collect();
    let feed = || kuro_app::app::Feed { items: items.clone(), started: true, page: 1, ..Default::default() };
    h.state_mut().season.feed = feed();
    h.state_mut().ranking.feed = feed();
    let m = Rc::new(media(1, MediaKind::Anime, "Lanterns Over Saltmarsh"));
    let r = m.media_ref();
    h.state_mut().details.entry(r).or_default().data =
        Some(Rc::new(detail(1, MediaKind::Anime, "Lanterns Over Saltmarsh")));
    shot(&mut h, "s-1426-cards");
    h.state_mut().lists[0].filter.status = None;
    h.state_mut().act(Act::Open(m.clone()));
    shot(&mut h, "s-1426-open");
    h.state_mut().detail_full = true;
    shot(&mut h, "s-1426-full");
    h.state_mut().detail_full = false;
    h.state_mut().routes.clear();
    h.state_mut().s.list_layout = ListLayout::Table;
    shot(&mut h, "s-1426-table");
    h.state_mut().s.list_layout = ListLayout::Posters;
    h.set_size(egui::vec2(2200.0, 1100.0));
    shot(&mut h, "s-2200-posters");
    h.state_mut().act(Act::Go(Tab::Seasonal));
    shot(&mut h, "s-2200-season");
    h.set_size(egui::vec2(760.0, 800.0));
    shot(&mut h, "s-760-season");
    h.state_mut().act(Act::Go(Tab::Anime));
    h.state_mut().s.list_layout = ListLayout::Cards;
    shot(&mut h, "s-760-cards");
    h.set_size(egui::vec2(360.0, 700.0));
    shot(&mut h, "s-360-cards");
    h.state_mut().act(Act::Open(m));
    shot(&mut h, "s-360-open");
}

/// The Friends tab with one friend, then their profile.
fn friends(h: &mut egui_kittest::Harness<'static, App>) {
    use kuro_app::app::FriendView;
    use kuro_core::friends::{Friend, FriendProfile};
    use kuro_core::{ListEntry, ListStatus, Service};
    let f = Friend { service: Service::AniList, name: "InkReader".into() };
    let titles = [
        (1, "Lanterns Over Saltmarsh", 85),
        (3, "Iron Orchard", 70),
        (40, "Paper Comet", 0),
        (41, "Quiet Signal", 95),
        (42, "Salt and Static", 90),
    ];
    let entries = titles
        .into_iter()
        .map(|(id, title, score)| {
            let mut e = ListEntry::new(media(id, MediaKind::Anime, title));
            (e.status, e.progress, e.score) = (ListStatus::Current, 4, score);
            e
        })
        .collect();
    let data = FriendProfile { name: f.name.clone(), avatar: None, entries };
    h.state_mut().s.friends = vec![f.clone()];
    h.state_mut()
        .friends
        .insert(f.clone(), FriendView { data: Some(Rc::new(data)), error: None, loading: false });
    h.state_mut().routes.clear();
    h.state_mut().act(Act::Go(Tab::Friends));
    shot(h, "friends");
    h.state_mut().act(Act::Push(Route::Friend(f)));
    shot(h, "friend-profile");
    h.state_mut().routes.clear();
}
