#![allow(dead_code, clippy::unwrap_used)]
//! An app without network, filled with invented titles.

use std::sync::Arc;

use egui_kittest::Harness;
use mango_api::{Api, ClientIds, MemorySecrets};
use mango_app::app::App;
use mango_app::backend::{Backend, now};
use mango_app::system::NoSystem;
use mango_core::*;

pub fn media(id: u64, kind: MediaKind, title: &str) -> Media {
    Media {
        id,
        kind,
        mal_id: Some(id + 1000),
        anilist_id: Some(id),
        title: title.into(),
        title_english: Some(format!("{title} (English)")),
        title_native: Some("日本語の題名".into()),
        format: if kind == MediaKind::Anime { Format::Tv } else { Format::Manga },
        release: ReleaseStatus::Finished,
        season: Some(SeasonYear { season: Season::Fall, year: 2025 }),
        start: FuzzyDate::parse("2025-10-03"),
        end: FuzzyDate::parse("2026-03-20"),
        episodes: (kind == MediaKind::Anime).then_some(24),
        chapters: (kind == MediaKind::Manga).then_some(120),
        volumes: (kind == MediaKind::Manga).then_some(14),
        duration: Some(24),
        genres: vec!["Drama".into(), "Fantasy".into()],
        studios: vec!["Studio Example".into()],
        mean_score: Some(8.42),
        members: Some(131_247),
        scored_by: Some(67_846),
        rank: Some(12),
        popularity: Some(40),
        synopsis: Some("A lighthouse keeper finds a map that redraws itself every night. ".repeat(10)),
        ..Media::default()
    }
}

pub fn entry(id: u64, kind: MediaKind, title: &str, status: ListStatus, progress: u32) -> ListEntry {
    ListEntry {
        status,
        progress,
        progress_volumes: if kind == MediaKind::Manga { progress / 9 } else { 0 },
        score: if status == ListStatus::Completed { 85 } else { 0 },
        start: FuzzyDate::parse("2026-01-02"),
        updated_at: now() - id as i64 * 1000,
        ..ListEntry::new(media(id, kind, title))
    }
}

pub fn detail(id: u64, kind: MediaKind, title: &str) -> MediaDetail {
    let person =
        |id, name: &str| Person { id, name: name.into(), image: None, language: Some("Japanese".into()) };
    MediaDetail {
        media: media(id, kind, title),
        relations: vec![Relation { kind: RelationKind::Sequel, media: media(id + 50, kind, "Second Tide") }],
        recommendations: vec![media(id + 60, kind, "Paper Harbor"), media(id + 61, kind, "Clockwork Rain")],
        characters: vec![CharacterRole {
            id: 1,
            name: "Mira Vale".into(),
            image: None,
            role: "Main".into(),
            voices: vec![person(77, "Example Actor")],
        }],
        staff: vec![StaffRole { person: person(78, "Example Director"), role: "Director".into() }],
        status_stats: vec![
            (ListStatus::Current, 900),
            (ListStatus::Completed, 2400),
            (ListStatus::Dropped, 120),
        ],
        score_stats: (1..=10).map(|i| (i * 10, u64::from(i * i * 10))).collect(),
        reviews: vec![Review {
            id: 5,
            user: "reader".into(),
            summary: "Quiet and lovely.".into(),
            body: "It takes its time, and that is the point.".into(),
            score: Some(88),
            votes_up: 10,
            votes: 12,
            created_at: 1_790_000_000,
        }],
        links: vec![ExternalLink {
            site: "Official Site".into(),
            url: "https://example.com".into(),
            streaming: false,
        }],
        languages: vec!["Japanese".into(), "English".into()],
        ..MediaDetail::default()
    }
}

pub fn store() -> Store {
    use ListStatus as L;
    use MediaKind::{Anime, Manga};
    let s = Store::in_memory().unwrap();
    let mut airing = entry(1, Anime, "Lanterns Over Saltmarsh", L::Current, 9);
    airing.media.next_airing = Some(Airing { episode: 10, at: now() + 3 * 86_400 });
    airing.media.release = ReleaseStatus::Releasing;
    airing.score = 90;
    for e in [
        airing,
        entry(2, Anime, "The Mapmaker and the Slowly Moving Northern Coast", L::Current, 3),
        entry(3, Anime, "Iron Orchard", L::Completed, 24),
        entry(4, Anime, "Midnight Ferry", L::Planning, 0),
        entry(5, Anime, "Glass Meridian", L::Paused, 6),
        entry(6, Anime, "Dustfall", L::Dropped, 2),
        entry(20, Manga, "Ink & Ember", L::Current, 57),
        entry(21, Manga, "Hollow Bell", L::Completed, 120),
    ] {
        s.put_entry(&e).unwrap();
    }
    s
}

pub fn app(ctx: &egui::Context, store: Store) -> App {
    let api = Api::new(Arc::new(MemorySecrets::default()), ClientIds::default()).unwrap();
    let dir = std::env::temp_dir().join("mango-desktop-tests");
    let bg = Backend::new(api, store, dir, ctx.clone(), false);
    App::new(ctx, bg, Box::new(NoSystem), false)
}

pub fn harness_with(
    store: Store,
    size: [f32; 2],
    setup: impl FnOnce(&mut Settings),
) -> Harness<'static, App> {
    let mut s = store.settings().unwrap();
    s.language = "en".into();
    s.theme = mango_core::settings::Theme::Black;
    s.animations = false;
    setup(&mut s);
    store.save_settings(&s).unwrap();
    let mut h = Harness::builder()
        .with_size(size)
        .with_max_steps(30)
        .allow_missing_glyphs()
        .build_eframe(|cc| app(&cc.egui_ctx, store));
    settle(&mut h);
    h
}

pub fn harness() -> Harness<'static, App> {
    harness_with(store(), [1240.0, 820.0], |_| {})
}

/// Runs some frames; timers keep the app from ever being fully idle.
pub fn settle(h: &mut Harness<'static, App>) {
    h.run_steps(8);
}
