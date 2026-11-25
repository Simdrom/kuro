//! Prime Video: the watch history saved by a community script.
//!
//! Prime Video offers no download of the watch history. The way in used here
//! is "Watch History Exporter for Amazon Prime Video", a script the user runs
//! in the browser console on primevideo.com/settings/watch-history, which
//! saves the history as CSV (or JSON). It is not from Amazon, it reads the page
//! and makes no requests of its own (checked in November 2026), and it may
//! break whenever Amazon changes that page. Checked against a real export:
//!
//! - Columns: `Date Watched`, `Type` (`Series` or `Movie`), `Title`,
//!   `Episode Title`, `Global Title Identifier`, `Episode Global Title
//!   Identifier`, and paths and images that are not used.
//! - Dates are UTC with milliseconds (`2026-06-21 01:34:11.936`), or Unix
//!   milliseconds when the script is told not to format them.
//! - Series titles end with their season in the account's language, after
//!   `" - "` (`Show - Temporada 2`, `Show - 1.ª Temporada`, `Show - Season 2
//!   [Ultra HD]`, `Show - Temporada 3 Parte 1`, `Show - Season 12A`); some
//!   series have no season, and a few have no name, only `Temporada 1`.
//! - Every episode title starts with its number (`Episodio 3: Title`), and
//!   every episode has an id of its own.

use kuro_core::kioku::matcher::normalize;
use kuro_core::kioku::{Platform, Watched, WatchedItem};
use serde_json::Value;

use super::netflix::{Marker, marker, number_episodes, rows};
use super::{HistorySource, Mode};
use crate::http::{ApiError, Result};

/// Larger than any real history, small enough to read at once.
pub const MAX_FILE: usize = 16 * 1024 * 1024;

pub struct Amazon;

impl HistorySource for Amazon {
    fn platform(&self) -> Platform {
        Platform::Amazon
    }

    fn mode(&self) -> Mode {
        Mode::File
    }

    fn parse(&self, data: &[u8]) -> Result<Vec<WatchedItem>> {
        parse(data)
    }
}

fn bad(why: &str) -> ApiError {
    ApiError::Decode(format!("not a Prime Video watch history file: {why}"))
}

/// One row, whichever format it came in.
#[derive(Default)]
struct Row {
    /// Unix milliseconds.
    when: Option<i64>,
    movie: bool,
    title: String,
    episode_title: String,
    id: String,
    episode_id: String,
}

/// `2026-06-21 01:34:11.936` in UTC, or Unix milliseconds.
fn when(text: &str) -> Option<i64> {
    let t = text.trim();
    if !t.is_empty() && t.chars().all(|c| c.is_ascii_digit()) {
        return t.parse().ok();
    }
    let dt: jiff::civil::DateTime = t.replacen(' ', "T", 1).parse().ok()?;
    let z = dt.to_zoned_datetime(jiff::tz::TimeZone::UTC).ok()?;
    Some(z.timestamp().as_millisecond())
}

/// Quality tags Prime Video appends to titles: `[Ultra HD]`, `(4K UHD)`.
fn without_tags(title: &str) -> &str {
    let mut t = title.trim();
    loop {
        let Some(open) = t.rfind(['[', '(']) else { return t };
        let tag = t[open..].to_lowercase();
        let quality = ["4k", "uhd", "ultra hd", "hdr", "hd", "dolby", "imax"].iter().any(|q| tag.contains(q));
        if !quality || !(tag.ends_with(']') || tag.ends_with(')')) {
            return t;
        }
        t = t[..open].trim_end();
    }
}

/// The season a title suffix names, and the part when it names one too
/// (`Temporada 3 Parte 1`).
fn season_of(suffix: &str) -> Option<(u32, Option<u32>)> {
    let words: Vec<&str> = suffix.split_whitespace().collect();
    if words.len() >= 4
        && let Some(Marker::Part(part)) = marker(&words[words.len() - 2..].join(" "))
        && let Some(Marker::Season(n)) = marker(&words[..words.len() - 2].join(" "))
    {
        return Some((n, Some(part)));
    }
    match marker(suffix)? {
        Marker::Season(n) => Some((n, None)),
        Marker::Limited => Some((1, None)),
        Marker::Part(_) | Marker::Episode(_) => None,
    }
}

/// Series, season and season title of a series title.
fn split_title(title: &str) -> (String, Option<u32>, Option<String>) {
    let t = without_tags(title);
    for sep in [" - ", " – ", ": "] {
        if let Some(at) = t.rfind(sep)
            && let Some((season, part)) = season_of(&t[at + sep.len()..])
        {
            let series = t[..at].trim().to_owned();
            let season_title = part.map(|p| format!("{series} Part {p}"));
            return (series, Some(season), season_title);
        }
    }
    // A series without its name, only "Season 1".
    if let Some((season, _)) = season_of(t) {
        return (t.to_owned(), Some(season), None);
    }
    (t.to_owned(), None, None)
}

/// `Episodio 3: Title` gives 3 and `Title`.
fn split_episode(text: &str) -> (Option<u32>, Option<String>) {
    let text = text.trim();
    if text.is_empty() {
        return (None, None);
    }
    let (head, rest) = text.split_once(": ").unwrap_or((text, ""));
    match marker(head) {
        Some(Marker::Episode(n)) => (Some(n), Some(rest.trim()).filter(|r| !r.is_empty()).map(str::to_owned)),
        _ => (None, Some(text.to_owned())),
    }
}

fn item(r: Row) -> Option<WatchedItem> {
    let title = r.title.trim();
    if title.is_empty() {
        return None;
    }
    let movie = r.movie || r.episode_title.trim().is_empty();
    let (series_title, season, season_title) =
        if movie { (without_tags(title).to_owned(), None, None) } else { split_title(title) };
    let (episode, episode_title) = if movie { (None, None) } else { split_episode(&r.episode_title) };
    let own = if movie { &r.id } else { &r.episode_id };
    let external_id = if own.trim().is_empty() {
        let day = r.when.map_or(String::new(), |w| (w / 86_400_000).to_string());
        format!("amazon:{}|{}|{day}", normalize(title), normalize(&r.episode_title))
    } else {
        format!("amazon:{}", own.trim())
    };
    Some(WatchedItem {
        platform: Platform::Amazon,
        series_title,
        season,
        season_title,
        episode,
        estimated: false,
        episode_title,
        movie,
        watched_at: r.when.map(|ms| ms.div_euclid(1000)),
        // The export lists what was played, without saying how much.
        progress: Watched::Completed,
        external_id,
        series_id: Some(r.id.trim().to_owned()).filter(|id| !movie && !id.is_empty()),
    })
}

fn from_csv(text: &str) -> Result<Vec<Row>> {
    let rows = rows(text)?;
    let Some((head, body)) = rows.split_first() else { return Err(bad("empty")) };
    let col = |name: &str| head.iter().position(|h| h.trim().eq_ignore_ascii_case(name));
    let (Some(di), Some(ki), Some(ti), Some(ei)) =
        (col("Date Watched"), col("Type"), col("Title"), col("Episode Title"))
    else {
        return Err(bad("no Date Watched, Type, Title and Episode Title columns"));
    };
    let (gi, egi) = (col("Global Title Identifier"), col("Episode Global Title Identifier"));
    let field = |r: &Vec<String>, i: Option<usize>| i.and_then(|i| r.get(i)).cloned().unwrap_or_default();
    Ok(body
        .iter()
        .map(|r| Row {
            when: when(&field(r, Some(di))),
            movie: field(r, Some(ki)).trim().eq_ignore_ascii_case("movie"),
            title: field(r, Some(ti)),
            episode_title: field(r, Some(ei)),
            id: field(r, gi),
            episode_id: field(r, egi),
        })
        .collect())
}

fn from_json(text: &str) -> Result<Vec<Row>> {
    let all: Value = serde_json::from_str(text).map_err(|e| bad(&e.to_string()))?;
    let list = all.as_array().ok_or_else(|| bad("not a list"))?;
    let s = |v: &Value| v.as_str().unwrap_or_default().to_owned();
    Ok(list
        .iter()
        .map(|v| Row {
            when: v["dateWatched"].as_i64().or_else(|| v["dateWatched"].as_str().and_then(when)),
            movie: v["type"].as_str().is_some_and(|t| t.eq_ignore_ascii_case("movie")),
            title: s(&v["title"]),
            episode_title: s(&v["episodeTitle"]),
            id: s(&v["id"]),
            episode_id: s(&v["episodeId"]),
        })
        .collect())
}

pub fn parse(data: &[u8]) -> Result<Vec<WatchedItem>> {
    if data.len() > MAX_FILE {
        return Err(ApiError::TooLarge);
    }
    let text = std::str::from_utf8(data).map_err(|_| bad("not UTF-8"))?.trim_start_matches('\u{feff}');
    let rows = if text.trim_start().starts_with('[') { from_json(text)? } else { from_csv(text)? };
    let mut out: Vec<WatchedItem> = rows.into_iter().filter_map(item).collect();
    // Only seasons whose titles carry no numbers are counted.
    number_episodes(&mut out);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/kioku/amazon.csv");

    fn find<'a>(items: &'a [WatchedItem], episode_title: &str) -> &'a WatchedItem {
        items.iter().find(|i| i.episode_title.as_deref() == Some(episode_title)).unwrap()
    }

    #[test]
    fn reads_the_invented_watch_history() {
        let items = Amazon.parse(FIXTURE).unwrap();
        assert_eq!(items.len(), 12);
        let e = find(&items, "La tercera lámpara");
        assert_eq!(
            (e.series_title.as_str(), e.season, e.episode, e.estimated),
            ("Lanterns Over Saltmarsh", Some(2), Some(3), false)
        );
        assert_eq!(e.external_id, "amazon:amzn1.dv.gti.00000000-0000-0000-0000-0000000000e3");
        assert_eq!(e.series_id.as_deref(), Some("amzn1.dv.gti.00000000-0000-0000-0000-0000000000a1"));
        assert_eq!(e.watched_at, Some(when("2026-11-20 21:10:05.120").unwrap() / 1000));
        assert_eq!(e.watched_at, Some(1_795_209_005));
    }

    #[test]
    fn every_way_of_writing_a_season() {
        let items = Amazon.parse(FIXTURE).unwrap();
        let s = |t: &str| {
            let i = find(&items, t);
            (i.series_title.clone(), i.season, i.season_title.clone(), i.episode)
        };
        assert_eq!(s("Raíces"), ("Iron Orchard".into(), Some(1), None, Some(1)));
        assert_eq!(s("Cristal"), ("Glass Meridian".into(), Some(2), None, Some(5)));
        assert_eq!(s("Polvo"), ("Dustfall".into(), Some(3), Some("Dustfall Part 1".into()), Some(1)));
        assert_eq!(s("Ecos"), ("Paper Harbor: Ecos del puerto".into(), None, None, Some(2)));
        assert_eq!(s("Lluvia"), ("Clockwork Rain".into(), Some(12), None, Some(1)));
        assert_eq!(s("Niebla"), ("Midnight Ferry".into(), Some(2), None, Some(7)));
        assert_eq!(s("Partida"), ("Midnight Ferry".into(), Some(1), None, Some(1)));
    }

    #[test]
    fn nameless_series_stay_apart() {
        let items = Amazon.parse(FIXTURE).unwrap();
        let (a, b) = (find(&items, "Sin nombre"), find(&items, "Otro"));
        assert_eq!((a.series_title.as_str(), a.season), ("Temporada 1", Some(1)));
        assert_ne!(a.series_id, b.series_id);
        let groups = kuro_core::kioku::review::groups(vec![a.clone(), b.clone()], &Default::default());
        assert_eq!(groups.len(), 2);
    }

    #[test]
    fn films_have_no_episode_and_no_series_id() {
        let items = Amazon.parse(FIXTURE).unwrap();
        let films: Vec<&WatchedItem> = items.iter().filter(|i| i.movie).collect();
        assert_eq!(films.len(), 1);
        assert_eq!(films[0].series_title, "Hollow Bell");
        assert_eq!((films[0].episode, films[0].series_id.as_deref()), (None, None));
    }

    #[test]
    fn json_export_reads_the_same() {
        let json = r#"[{"dateWatched": 1795209005120, "type": "Series", "title": "Lanterns Over Saltmarsh - Temporada 2",
            "episodeTitle": "Episodio 3: La tercera lámpara", "id": "S", "episodeId": "E"},
            {"dateWatched": "2026-01-02 03:04:05.000", "type": "Movie", "title": "Hollow Bell (4K UHD)", "episodeTitle": "",
            "id": "M", "episodeId": ""}]"#;
        let items = parse(json.as_bytes()).unwrap();
        assert_eq!((items[0].episode, items[0].watched_at), (Some(3), Some(1_795_209_005)));
        assert_eq!(
            (items[1].series_title.as_str(), items[1].external_id.as_str()),
            ("Hollow Bell", "amazon:M")
        );
    }

    #[test]
    fn titles_lose_quality_tags_only() {
        assert_eq!(without_tags("Show - Season 2 [Ultra HD]"), "Show - Season 2");
        assert_eq!(without_tags("Film (4K UHD)"), "Film");
        assert_eq!(without_tags("Mob Psycho 100 (Dub)"), "Mob Psycho 100 (Dub)");
        assert_eq!(split_episode("Episodio 12: El final"), (Some(12), Some("El final".into())));
        assert_eq!(split_episode("Un piloto"), (None, Some("Un piloto".into())));
    }

    #[test]
    fn other_files_are_refused() {
        assert!(matches!(parse(b"Title,Date\nx,1/2/26\n"), Err(ApiError::Decode(_))));
        assert!(matches!(parse(b"[1, 2"), Err(ApiError::Decode(_))));
        assert!(matches!(parse(&[0xff, 0xfe]), Err(ApiError::Decode(_))));
        assert_eq!(parse(&vec![b'a'; MAX_FILE + 1]), Err(ApiError::TooLarge));
    }
}
