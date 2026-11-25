#![allow(clippy::unwrap_used)]
//! Checks against the real services. Run with `cargo test -p kuro-api --test live -- --ignored`.

use std::sync::Arc;

use kuro_api::{Api, ClientIds, MemorySecrets, Ranking, RankingQuery, SeasonQuery};
use kuro_core::{MediaKind, MediaRef, Season, SeasonYear, Service};

fn api() -> Api {
    Api::new(Arc::new(MemorySecrets::default()), ClientIds::default()).unwrap()
}

#[tokio::test]
#[ignore = "network"]
async fn anilist_queries_are_accepted() {
    let api = api();
    let s = Service::AniList;
    let found = api.search(s, MediaKind::Anime, "frieren", false, 1).await.unwrap();
    assert!(!found.items.is_empty());

    let q = SeasonQuery { season: SeasonYear { season: Season::Fall, year: 2023 }, adult: false };
    assert!(api.season(s, &q, 1).await.unwrap().items.len() > 20);
    assert!(!api.continuing(s, &q, 1).await.unwrap().items.is_empty());

    let q = RankingQuery {
        kind: MediaKind::Anime,
        ranking: Ranking::All,
        genre: Some("Drama".into()),
        year: Some(2023),
        adult: false,
    };
    let top = api.ranking(s, &q, 1).await.unwrap();
    assert!(top.items.iter().all(|m| m.start.is_some_and(|d| d.year == 2023)));
    for kind in MediaKind::ALL {
        for ranking in Ranking::options(kind, s) {
            let q = RankingQuery { kind, ranking, genre: None, year: None, adult: false };
            assert!(!api.ranking(s, &q, 1).await.unwrap().items.is_empty(), "{kind:?} {ranking:?}");
        }
    }

    let d = api.detail(MediaRef { service: s, kind: MediaKind::Anime, id: 154587 }).await.unwrap();
    assert!(d.characters.len() > 5 && !d.recommendations.is_empty() && !d.languages.is_empty());
    let va = d.characters[0].voices[0].id;
    assert!(!api.person(va).await.unwrap().roles.is_empty());
    let o = api.overview(&d.media).await.unwrap();
    assert!(o.iter().map(|s| s.1.len()).sum::<usize>() >= 2);
    assert!(api.anilist.genres().await.unwrap().len() > 10);
    assert!(api.anilist.random(MediaKind::Manga, false, 1234).await.unwrap().id > 0);
    let r = MediaRef { service: s, kind: MediaKind::Anime, id: 154587 };
    assert_eq!(api.resolve(r, Service::Mal).await.unwrap().id, 52991);
}

/// Reads one's own Crunchyroll history. Needs the `etp_rt` cookie of a signed-in
/// crunchyroll.com in `KURO_KIOKU_ETP_RT`. Prints only counts, never ids or titles.
/// The session it opens shows up as a "Kuro" device in the Crunchyroll account.
#[tokio::test]
#[ignore = "network, and a Crunchyroll session cookie"]
async fn crunchyroll_history_is_read() {
    use kuro_api::kioku::HistorySource;
    let Ok(cookie) = std::env::var("KURO_KIOKU_ETP_RT") else {
        panic!("set KURO_KIOKU_ETP_RT");
    };
    let api = api();
    api.crunchyroll.connect(cookie.trim()).await.unwrap();
    let items = api.crunchyroll.fetch(&Default::default(), &|page| println!("page {page}")).await.unwrap();
    let films = items.iter().filter(|i| i.movie).count();
    println!("items: {}  films: {films}", items.len());
    assert!(items.iter().all(|i| !i.external_id.is_empty() && !i.series_title.is_empty()));
}

/// Reads one's own Netflix viewing activity, named by `KURO_KIOKU_NETFLIX_CSV`.
/// Prints only counts, never titles.
#[test]
#[ignore = "needs a Netflix viewing activity file"]
fn netflix_file_is_read() {
    use kuro_api::kioku::HistorySource;
    let Ok(path) = std::env::var("KURO_KIOKU_NETFLIX_CSV") else {
        panic!("set KURO_KIOKU_NETFLIX_CSV");
    };
    let items = kuro_api::kioku::netflix::Netflix.parse(&std::fs::read(path).unwrap()).unwrap();
    let count = |f: &dyn Fn(&kuro_core::kioku::WatchedItem) -> bool| items.iter().filter(|i| f(i)).count();
    let groups = kuro_core::kioku::review::groups(items.clone(), &Default::default());
    println!(
        "rows: {}  films: {}  with season: {}  counted episodes: {}  given episodes: {}  undated: {}  groups: {}",
        items.len(),
        count(&|i| i.movie),
        count(&|i| i.season.is_some()),
        count(&|i| i.estimated),
        count(&|i| !i.movie && !i.estimated),
        count(&|i| i.watched_at.is_none()),
        groups.len(),
    );
    assert!(items.iter().all(|i| !i.series_title.is_empty()));
}
