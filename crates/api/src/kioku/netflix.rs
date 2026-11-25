//! Netflix: the viewing activity a profile downloads from netflix.com.
//!
//! Netflix has no API for this. Each profile can download its history from
//! Account › Profile › Viewing activity › Download all, as a CSV file with a
//! `Title,Date` header. Checked against a real download in November 2026:
//!
//! - Titles are in the account's language and separated by `": "`, as in
//!   `Series: Season 2: Episode title`. The season part comes in many forms
//!   (`Temporada 2`, `2nd Temporada`, `Primera Temporada`, `Parte 1`,
//!   `Volumen 4`, `Miniserie`, `Libro uno`, `Episodio 23`…) or not at all,
//!   and series names hold colons themselves.
//! - Episode numbers are almost never given (`Episodio 23` is the exception), only episode titles. They are
//!   counted instead, in order of viewing within each season, and marked as
//!   counted.
//! - Dates have no time and were month first (`11/25/26`) even in Spanish;
//!   day first is taken only when the file shows it.
//! - There are no ids and no progress: an id is made from the title and the
//!   date, and every row counts as watched.

use std::collections::{BTreeMap, HashMap, HashSet};

use kuro_core::kioku::matcher::normalize;
use kuro_core::kioku::{Platform, Watched, WatchedItem};

use super::{HistorySource, Mode};
use crate::http::{ApiError, Result};

/// Larger than any real history, small enough to read at once.
pub const MAX_FILE: usize = 16 * 1024 * 1024;
const MAX_ROWS: usize = 200_000;

pub struct Netflix;

impl HistorySource for Netflix {
    fn platform(&self) -> Platform {
        Platform::Netflix
    }

    fn mode(&self) -> Mode {
        Mode::File
    }

    fn parse(&self, data: &[u8]) -> Result<Vec<WatchedItem>> {
        parse(data)
    }
}

fn bad(why: &str) -> ApiError {
    ApiError::Decode(format!("not a Netflix viewing activity file: {why}"))
}

/// Rows of a CSV file: commas, quotes with `""` inside, CRLF or LF.
pub(super) fn rows(text: &str) -> Result<Vec<Vec<String>>> {
    let mut out = Vec::new();
    let (mut row, mut field) = (Vec::new(), String::new());
    let (mut quoted, mut chars) = (false, text.chars().peekable());
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '"') if chars.peek() == Some(&'"') => {
                chars.next();
                field.push('"');
            }
            (true, '"') => quoted = false,
            (true, c) => field.push(c),
            (false, '"') => quoted = true,
            (false, ',') => row.push(std::mem::take(&mut field)),
            (false, '\r') => {}
            (false, '\n') => {
                row.push(std::mem::take(&mut field));
                out.push(std::mem::take(&mut row));
                if out.len() > MAX_ROWS {
                    return Err(ApiError::TooLarge);
                }
            }
            (false, c) => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        out.push(row);
    }
    out.retain(|r| r.iter().any(|f| !f.trim().is_empty()));
    Ok(out)
}

/// `M/D/YY`, `D/M/YY` (four-digit years too) or `YYYY-MM-DD`.
fn date(text: &str, day_first: bool) -> Option<jiff::civil::Date> {
    let t = text.trim();
    if let Some((y, rest)) = t.split_once('-') {
        let (m, d) = rest.split_once('-')?;
        return jiff::civil::Date::new(y.parse().ok()?, m.parse().ok()?, d.parse().ok()?).ok();
    }
    let mut p = t.split('/');
    let (a, b, y): (i8, i8, i16) =
        (p.next()?.parse().ok()?, p.next()?.parse().ok()?, p.next()?.parse().ok()?);
    let y = if y < 100 { 2000 + y } else { y };
    let (m, d) = if day_first { (b, a) } else { (a, b) };
    jiff::civil::Date::new(y, m, d).ok()
}

fn number_word(w: &str) -> Option<u32> {
    Some(match w {
        "primera" | "primer" | "first" | "premiere" | "erste" | "prima" | "uno" | "one" | "un" | "eins" => 1,
        "segunda" | "segundo" | "second" | "deuxieme" | "zweite" | "seconda" | "dos" | "two" | "deux"
        | "zwei" => 2,
        "tercera" | "tercer" | "third" | "troisieme" | "dritte" | "terza" | "tres" | "three" | "trois"
        | "drei" => 3,
        "cuarta" | "fourth" | "quatrieme" | "vierte" | "quarta" | "cuatro" | "four" | "quatre" | "vier" => 4,
        "quinta" | "fifth" | "cinquieme" | "funfte" | "cinco" | "five" | "cinq" | "funf" => 5,
        _ => return None,
    })
}

/// A number written as digits, an ordinal (`2nd`, `2.ª`) or a word.
fn number(w: &str) -> Option<u32> {
    let digits: String = w.chars().take_while(char::is_ascii_digit).collect();
    if !digits.is_empty() {
        let rest = &w[digits.len()..];
        let ordinal =
            matches!(rest, "" | "st" | "nd" | "rd" | "th" | "a" | "o" | "e" | "er" | "re" | "ª" | "º");
        return if ordinal { digits.parse().ok() } else { None };
    }
    number_word(w)
}

const SEASON: [&str; 8] =
    ["temporada", "season", "saison", "staffel", "stagione", "sesong", "sasong", "시즌"];
const PART: [&str; 9] = ["parte", "part", "partie", "teil", "volumen", "volume", "vol", "band", "cour"];
const BOOK: [&str; 5] = ["libro", "book", "livre", "buch", "curso"];
const EPISODE: [&str; 7] = ["episodio", "episode", "capitulo", "folge", "episodi", "episod", "에피소드"];
const LIMITED: [&str; 6] =
    ["miniserie", "miniseries", "limited series", "serie limitada", "mini serie", "mini-serie"];

/// What a segment of a title says about its place in a series.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Marker {
    Season(u32),
    Part(u32),
    Episode(u32),
    Limited,
}

pub(super) fn marker(segment: &str) -> Option<Marker> {
    let n = normalize(&segment.replace('\u{a0}', " "));
    if LIMITED.contains(&n.as_str()) {
        return Some(Marker::Limited);
    }
    let w: Vec<&str> = n.split(' ').collect();
    // "Temporada 2", "Season two", "시즌 2"; "2nd Temporada", "Primera Temporada".
    match w.as_slice() {
        [k, x] if SEASON.contains(k) || BOOK.contains(k) => number(x).map(Marker::Season),
        [x, k] if SEASON.contains(k) => number(x).map(Marker::Season),
        [x, "a" | "o" | "ª" | "º", k] if SEASON.contains(k) => number(x).map(Marker::Season),
        [k, x] if PART.contains(k) => number(x).map(Marker::Part),
        [k, x] if EPISODE.contains(k) => x.parse().ok().map(Marker::Episode),
        _ => None,
    }
}

/// One title of the file, taken apart.
#[derive(Debug, Clone, PartialEq, Default)]
struct Title {
    series: String,
    season: Option<u32>,
    season_title: Option<String>,
    episode: Option<u32>,
    episode_title: Option<String>,
    movie: bool,
}

fn split(title: &str, series_heads: &HashSet<String>) -> Title {
    let parts: Vec<&str> = title.split(": ").map(str::trim).filter(|p| !p.is_empty()).collect();
    let join = |s: &[&str]| s.join(": ");
    if let Some(i) = (1..parts.len()).find(|i| marker(parts[*i]).is_some()) {
        let series = join(&parts[..i]);
        let rest = (i + 1 < parts.len()).then(|| join(&parts[i + 1..]));
        let mut t = Title { series: series.clone(), episode_title: rest.clone(), ..Title::default() };
        match marker(parts[i]) {
            Some(Marker::Season(n)) => t.season = Some(n),
            Some(Marker::Part(n)) => t.season_title = Some(format!("{series} Part {n}")),
            Some(Marker::Limited) => t.season = Some(1),
            Some(Marker::Episode(n)) => t.episode = Some(n),
            None => {}
        }
        if t.episode.is_none() {
            // "Show: Season 1: Episode 3: Title". A number that only starts the
            // title ("24 hours", "5 seconds") is far too often part of the name.
            let first = rest.as_deref().and_then(|r| r.split(": ").next()).unwrap_or_default();
            if let Some(Marker::Episode(n)) = marker(first) {
                t.episode = Some(n);
            }
        }
        return t;
    }
    match parts.as_slice() {
        [] => Title::default(),
        [only] => Title { series: (*only).to_owned(), movie: true, ..Title::default() },
        // "Series: Episode title" when the series has other rows, a film's subtitle otherwise.
        [head, rest] if series_heads.contains(&normalize(head)) => {
            Title { series: (*head).to_owned(), episode_title: Some((*rest).to_owned()), ..Title::default() }
        }
        [_, _] => Title { series: title.trim().to_owned(), movie: true, ..Title::default() },
        // "Series: Arc or subtitle: Episode title".
        [head, middle @ .., last] => Title {
            series: (*head).to_owned(),
            season_title: Some(join(middle)),
            episode_title: Some((*last).to_owned()),
            ..Title::default()
        },
    }
}

/// Heads of `Head: Rest` titles, of two parts, that occur with more than one
/// rest: a series whose episodes have no season. A head that occurs once is
/// taken as a film with a subtitle.
fn series_heads(titles: &[&str]) -> HashSet<String> {
    let mut rests: HashMap<String, HashSet<&str>> = HashMap::new();
    for t in titles {
        if let Some((head, rest)) = t.split_once(": ")
            && !rest.contains(": ")
        {
            rests.entry(normalize(head)).or_default().insert(rest);
        }
    }
    rests.into_iter().filter(|(_, r)| r.len() > 1).map(|(h, _)| h).collect()
}

pub fn parse(data: &[u8]) -> Result<Vec<WatchedItem>> {
    if data.len() > MAX_FILE {
        return Err(ApiError::TooLarge);
    }
    let text = std::str::from_utf8(data).map_err(|_| bad("not UTF-8"))?;
    let rows = rows(text.trim_start_matches('\u{feff}'))?;
    let Some((head, body)) = rows.split_first() else { return Err(bad("empty")) };
    let col = |name: &str| head.iter().position(|h| h.trim().eq_ignore_ascii_case(name));
    let (Some(ti), Some(di)) = (col("Title"), col("Date")) else {
        return Err(bad("no Title and Date columns"));
    };
    let field = |r: &Vec<String>, i: usize| r.get(i).map(|s| s.trim().to_owned()).unwrap_or_default();

    // Day first only when a first number can only be a day.
    let day_first = body.iter().any(|r| {
        let d = field(r, di);
        d.split('/').next().and_then(|a| a.parse::<u32>().ok()).is_some_and(|a| a > 12)
    });
    let titles: Vec<String> = body.iter().map(|r| field(r, ti)).collect();
    let heads = series_heads(&titles.iter().map(String::as_str).collect::<Vec<_>>());

    let mut out = Vec::new();
    for (r, title) in body.iter().zip(&titles) {
        if title.is_empty() {
            continue;
        }
        let shown = field(r, di);
        let day = date(&shown, day_first);
        let t = split(title, &heads);
        let watched_at = day
            .and_then(|d| d.at(12, 0, 0, 0).to_zoned_datetime(jiff::tz::TimeZone::UTC).ok())
            .map(|z| z.timestamp().as_second());
        out.push(WatchedItem {
            platform: Platform::Netflix,
            series_title: t.series,
            season: t.season,
            season_title: t.season_title,
            episode: t.episode,
            estimated: false,
            series_id: None,
            episode_title: t.episode_title,
            movie: t.movie,
            watched_at,
            // Netflix lists what was played, without saying how much.
            progress: Watched::Completed,
            external_id: format!("netflix:{}|{}", normalize(title), day.map_or(shown, |d| d.to_string())),
        });
    }
    number_episodes(&mut out);
    Ok(out)
}

/// Gives every episode without a number its place in its season, by the order
/// it was first watched. A season with some numbers given keeps those.
pub(super) fn number_episodes(items: &mut [WatchedItem]) {
    let mut seasons: BTreeMap<(String, Option<u32>, String), Vec<usize>> = BTreeMap::new();
    for (i, it) in items.iter().enumerate().filter(|(_, it)| !it.movie) {
        let key = (
            normalize(&it.series_title),
            it.season,
            it.season_title.as_deref().map(normalize).unwrap_or_default(),
        );
        seasons.entry(key).or_default().push(i);
    }
    for rows in seasons.values() {
        if rows.iter().all(|i| items[*i].episode.is_some()) {
            continue;
        }
        // The file lists the newest first; ties keep that order reversed.
        let mut order: Vec<usize> = rows.clone();
        order.sort_by_key(|i| (items[*i].watched_at.unwrap_or(i64::MAX), std::cmp::Reverse(*i)));
        let mut place: HashMap<String, u32> = HashMap::new();
        for i in order {
            let name = items[i].episode_title.as_deref().map(normalize).unwrap_or_default();
            let next = u32::try_from(place.len()).unwrap_or(u32::MAX).saturating_add(1);
            let n = *place.entry(name).or_insert(next);
            items[i].episode = Some(n);
            items[i].estimated = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/kioku/netflix.csv");

    fn find<'a>(items: &'a [WatchedItem], episode_title: &str) -> &'a WatchedItem {
        items.iter().find(|i| i.episode_title.as_deref() == Some(episode_title)).unwrap()
    }

    #[test]
    fn reads_the_invented_viewing_activity() {
        let items = Netflix.parse(FIXTURE).unwrap();
        assert_eq!(items.len(), 15);
        let e = find(&items, "Marea baja");
        assert_eq!(
            (e.series_title.as_str(), e.season, e.episode, e.estimated),
            ("Lanterns Over Saltmarsh", Some(2), Some(2), true)
        );
        assert_eq!(e.watched_at, Some(noon(jiff::civil::date(2026, 11, 18))));
        // Counted in the order they were watched, oldest first.
        assert_eq!(find(&items, "La primera lámpara").episode, Some(1));
        assert_eq!(find(&items, "La tercera lámpara").episode, Some(3));
    }

    #[test]
    fn every_way_of_writing_a_season() {
        let items = Netflix.parse(FIXTURE).unwrap();
        let s = |t: &str| {
            let i = find(&items, t);
            (i.series_title.clone(), i.season, i.season_title.clone(), i.episode)
        };
        assert_eq!(s("El huerto despierta"), ("Iron Orchard".into(), Some(2), None, Some(1)));
        assert_eq!(s("Raíces"), ("Iron Orchard".into(), Some(1), None, Some(1)));
        assert_eq!(
            s("Paso a paso"),
            ("Glass Meridian".into(), None, Some("Glass Meridian Part 1".into()), Some(1))
        );
        assert_eq!(s("Sopa de niebla"), ("Midnight Ferry: Kitchen".into(), None, None, Some(23)));
        // A number at the start of a title is not taken as its episode.
        assert_eq!(s("24 horas"), ("Dustfall".into(), Some(1), None, Some(1)));
        // An arc instead of a season.
        assert_eq!(s("Primer puerto"), ("Paper Harbor".into(), None, Some("East Reach".into()), Some(1)));
        // Series of two parts only, known because the head repeats.
        assert_eq!(s("Llegada"), ("Clockwork Rain".into(), None, None, Some(1)));
    }

    #[test]
    fn films_have_no_episode() {
        let items = Netflix.parse(FIXTURE).unwrap();
        let films: Vec<&str> = items.iter().filter(|i| i.movie).map(|i| i.series_title.as_str()).collect();
        assert_eq!(films, ["Lanterns Over Saltmarsh: Night Star", "Hollow Bell"]);
        assert!(items.iter().filter(|i| i.movie).all(|i| i.episode.is_none()));
    }

    #[test]
    fn quotes_commas_and_repeats() {
        let items = Netflix.parse(FIXTURE).unwrap();
        let q = find(&items, "Dijo \"basta\", y se fue");
        assert_eq!(q.series_title, "Iron Orchard");
        // The same episode twice the same day is one id; on another day, another.
        let ids: HashSet<&str> = items.iter().map(|i| i.external_id.as_str()).collect();
        assert_eq!(ids.len(), items.len() - 1);
    }

    #[test]
    fn day_first_only_when_the_file_shows_it() {
        let us = "Title,Date\n\"A: Temporada 1: B\",\"3/4/26\"\n";
        assert_eq!(parse(us.as_bytes()).unwrap()[0].watched_at, date("3/4/26", false).map(noon));
        let eu = "Title,Date\n\"A: Temporada 1: B\",\"3/4/26\"\n\"A: Temporada 1: C\",\"25/4/26\"\n";
        let items = parse(eu.as_bytes()).unwrap();
        assert_eq!(items[0].watched_at, Some(noon(jiff::civil::date(2026, 4, 3))));
        assert_eq!(date("2026-11-25", false), Some(jiff::civil::date(2026, 11, 25)));
        assert_eq!(date("13/40/26", true), None);
    }

    fn noon(d: jiff::civil::Date) -> i64 {
        d.at(12, 0, 0, 0).to_zoned_datetime(jiff::tz::TimeZone::UTC).unwrap().timestamp().as_second()
    }

    #[test]
    fn other_files_are_refused() {
        assert!(matches!(parse(b"Name,When\nx,y\n"), Err(ApiError::Decode(_))));
        assert!(matches!(parse(b""), Err(ApiError::Decode(_))));
        assert!(matches!(parse(&[0xff, 0xfe, 0x00]), Err(ApiError::Decode(_))));
        assert_eq!(parse(&vec![b'a'; MAX_FILE + 1]), Err(ApiError::TooLarge));
        // A BOM and a trailing line without a newline are fine.
        assert_eq!(parse("\u{feff}Title,Date\n\"A\",\"1/2/26\"".as_bytes()).unwrap().len(), 1);
    }

    #[test]
    fn markers_in_many_languages() {
        for (s, m) in [
            ("Temporada 2", Marker::Season(2)),
            ("2nd Temporada", Marker::Season(2)),
            ("Primera Temporada", Marker::Season(1)),
            ("1.ª Temporada", Marker::Season(1)),
            ("Season 3", Marker::Season(3)),
            ("Staffel 2", Marker::Season(2)),
            ("시즌 2", Marker::Season(2)),
            ("Libro uno", Marker::Season(1)),
            ("Curso 2", Marker::Season(2)),
            ("Parte 1", Marker::Part(1)),
            ("Vol. 1", Marker::Part(1)),
            ("Volumen 4", Marker::Part(4)),
            ("Episodio\u{a0}23", Marker::Episode(23)),
            ("Miniserie", Marker::Limited),
        ] {
            assert_eq!(marker(s), Some(m), "{s}");
        }
        for s in ["Edgerunners", "1984", "Nocturno", "Stranger Things 5", "2025"] {
            assert_eq!(marker(s), None, "{s}");
        }
    }
}
