#![allow(clippy::unwrap_used)]
//! Renders every screen to PNG files for review: `cargo test --test shots -- --ignored`.

mod common;

use std::rc::Rc;

use common::*;
use mango_app::app::{Act, App, Load, Route, SettingsPage, Tab};
use mango_core::overview::Section;
use mango_core::settings::Theme;
use mango_core::{MediaKind, Store};

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
    h.state_mut().act(Act::Go(Tab::Manga));
    settle(&mut h);
    h.state_mut().s.theme = Theme::Light;
    h.state_mut().lists[0].filter.status = None;
    h.state_mut().lists[1].filter.status = None;
    h.state_mut().act(Act::Open(Rc::new(media(20, MediaKind::Manga, "Ink & Ember"))));
    shot(&mut h, "manga-light");
    h.state_mut().s.sidebar = false;
    h.state_mut().s.list_grid = true;
    h.state_mut().act(Act::Go(Tab::Anime));
    shot(&mut h, "grid-light");
    h.state_mut().command = Some("i".into());
    shot(&mut h, "command");

    let mut h = harness_with(store(), [430.0, 800.0], |_| {});
    shot(&mut h, "narrow");
    let mut h = harness_with(Store::in_memory().unwrap(), [1000.0, 700.0], |_| {});
    shot(&mut h, "welcome");
}
