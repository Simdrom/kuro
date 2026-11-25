//! Finds the title on AniList or MyAnimeList that a platform's series stands for.
//!
//! Platforms count seasons inside one series, while AniList and MyAnimeList
//! keep one entry per season (and sometimes per part). Titles also come in
//! romaji, English or with different punctuation, so every comparison works on
//! normalised text and every result carries a confidence.

use crate::kioku::item::WatchedItem;
use crate::model::{Format, Media, MediaKind};

/// Below this a match is only ever suggested, never applied by itself.
pub const THRESHOLD: f32 = 0.75;

/// Titles at least this similar are taken as the same name.
const SAME: f32 = 0.86;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Basis {
    /// The season's own title names the entry.
    SeasonTitle,
    /// The series title and the season number written in the entry's title.
    SeasonNumber,
    /// The n-th entry of the franchise by date: seasons are often split differently.
    SeasonOrder,
    /// Similar title, nothing else agrees.
    TitleOnly,
    Movie,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Match {
    pub media: Media,
    /// Subtracted from the platform's episode numbers, when it keeps counting
    /// across seasons that the tracker splits.
    pub offset: u32,
    /// 0 to 1.
    pub confidence: f32,
    pub basis: Basis,
}

impl Match {
    pub fn sure(&self) -> bool {
        self.confidence >= THRESHOLD
    }

    /// The tracker's episode for a platform episode, if it belongs to this entry.
    pub fn episode(&self, platform_episode: u32) -> Option<u32> {
        let e = platform_episode.checked_sub(self.offset).filter(|e| *e > 0)?;
        self.media.total().is_none_or(|t| e <= t).then_some(e)
    }
}

/// Lower case, without accents, macrons or punctuation, words separated by one space.
pub fn normalize(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    for c in title.chars().flat_map(char::to_lowercase) {
        let c = match c {
            'ā' | 'â' | 'á' | 'à' | 'ä' => 'a',
            'ē' | 'ê' | 'é' | 'è' | 'ë' => 'e',
            'ī' | 'î' | 'í' | 'ì' | 'ï' => 'i',
            'ō' | 'ô' | 'ó' | 'ò' | 'ö' => 'o',
            'ū' | 'û' | 'ú' | 'ù' | 'ü' => 'u',
            // As in "SPY×FAMILY", written "Spy x Family" elsewhere.
            '×' => {
                out.push_str(" x ");
                continue;
            }
            '\'' | '’' | '`' => continue,
            c if c.is_alphanumeric() => c,
            _ => ' ',
        };
        out.push(c);
    }
    out.split_whitespace()
        .filter(|w| !matches!(*w, "dub" | "dubbed" | "sub" | "subbed" | "tv"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn ordinal(w: &str) -> Option<u32> {
    let digits = w.trim_end_matches(|c: char| c.is_ascii_alphabetic());
    let suffix = &w[digits.len()..];
    if matches!(suffix, "st" | "nd" | "rd" | "th") { digits.parse().ok() } else { None }
}

fn roman(w: &str) -> Option<u32> {
    match w {
        "ii" => Some(2),
        "iii" => Some(3),
        "iv" => Some(4),
        "v" => Some(5),
        "vi" => Some(6),
        _ => None,
    }
}

/// The season a normalised title names, and the title without it.
pub fn split_season(norm: &str) -> (String, Option<u32>) {
    let words: Vec<&str> = norm.split(' ').filter(|w| !w.is_empty()).collect();
    let mut season = None;
    let mut keep = Vec::with_capacity(words.len());
    let mut i = 0;
    while i < words.len() {
        let w = words[i];
        let next = words.get(i + 1).copied();
        if w == "season"
            && let Some(n) = next.and_then(|n| n.parse().ok())
        {
            season = Some(n);
            i += 2;
            continue;
        }
        if let Some(n) = ordinal(w).filter(|_| next == Some("season")) {
            season = Some(n);
            i += 2;
            continue;
        }
        if w.len() > 1
            && let Some(n) = w.strip_prefix('s').and_then(|n| n.parse().ok())
        {
            season = Some(n);
            i += 1;
            continue;
        }
        // A trailing roman numeral, as in "Title II"; a trailing digit is too often part of the name.
        if i + 1 == words.len()
            && i > 0
            && let Some(n) = roman(w)
        {
            season = Some(n);
            i += 1;
            continue;
        }
        keep.push(w);
        i += 1;
    }
    (keep.join(" "), season)
}

fn bigrams(s: &str) -> Vec<(char, char)> {
    let chars: Vec<char> = s.chars().filter(|c| *c != ' ').collect();
    let mut v: Vec<(char, char)> = chars.windows(2).map(|w| (w[0], w[1])).collect();
    v.sort_unstable();
    v
}

/// Dice coefficient on character pairs, 0 to 1.
pub fn similarity(a: &str, b: &str) -> f32 {
    if a == b {
        return if a.is_empty() { 0.0 } else { 1.0 };
    }
    let (x, y) = (bigrams(a), bigrams(b));
    if x.is_empty() || y.is_empty() {
        return 0.0;
    }
    let (mut i, mut j, mut common) = (0, 0, 0);
    while i < x.len() && j < y.len() {
        match x[i].cmp(&y[j]) {
            std::cmp::Ordering::Equal => {
                common += 1;
                i += 1;
                j += 1;
            }
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
        }
    }
    2.0 * common as f32 / (x.len() + y.len()) as f32
}

fn titles(m: &Media) -> impl Iterator<Item = String> + '_ {
    std::iter::once(&m.title)
        .chain(m.title_english.iter())
        .chain(m.synonyms.iter())
        .filter(|t| !t.trim().is_empty())
        .map(|t| normalize(t))
}

/// Best similarity of `text` to any title of `m`.
fn closeness(m: &Media, text: &str) -> f32 {
    titles(m).map(|t| similarity(&t, text)).fold(0.0, f32::max)
}

/// Best similarity of the season-free base of `text` to any base of `m`, and
/// the season `m` names in that title.
fn base_closeness(m: &Media, base: &str) -> (f32, Option<u32>) {
    titles(m)
        .map(|t| {
            let (b, s) = split_season(&t);
            // "Title: Subtitle" is still the same series.
            let sub = !base.is_empty() && b.starts_with(base) && b[base.len()..].starts_with(' ');
            (if sub { similarity(&b, base).max(SAME) } else { similarity(&b, base) }, s)
        })
        .fold((0.0, None), |best, x| if x.0 > best.0 { x } else { best })
}

fn serial(f: Format) -> bool {
    matches!(f, Format::Tv | Format::TvShort | Format::Ona | Format::Unknown)
}

fn started(m: &Media) -> (i32, u32, u32) {
    m.start.map_or((i32::MAX, 0, 0), |d| (d.year, d.month.unwrap_or(0), d.day.unwrap_or(0)))
}

/// The entry for a group of items from one season of one series.
pub fn best(items: &[WatchedItem], candidates: &[Media]) -> Option<Match> {
    let first = items.first()?;
    let anime: Vec<&Media> = candidates.iter().filter(|m| m.kind == MediaKind::Anime).collect();
    if first.movie {
        return movie(first, &anime);
    }
    let series = normalize(&first.series_title);
    let (series_base, series_hint) = split_season(&series);
    let season_title = first.season_title.as_deref().map(normalize).filter(|t| !t.is_empty());
    let hint = season_title.as_deref().and_then(|t| split_season(t).1);
    let wanted = hint.or(first.season).or(series_hint).unwrap_or(1);
    let top = items.iter().filter_map(|i| i.episode).max().unwrap_or(1);

    let pick =
        |m: &Media, confidence: f32, basis: Basis| Match { media: m.clone(), offset: 0, confidence, basis };
    let mut found: Option<Match> = None;

    // A season title with its own subtitle names the entry directly.
    if let Some(st) = season_title.as_deref().filter(|t| *t != series)
        && let Some((m, s)) = anime
            .iter()
            .filter(|m| serial(m.format))
            .map(|m| (*m, closeness(m, st)))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .filter(|(_, s)| *s >= SAME)
    {
        found = Some(pick(m, 0.8 + 0.15 * s, Basis::SeasonTitle));
    }

    // Same series, and the entry's title names the wanted season (or none, for the first).
    let franchise: Vec<(&Media, Option<u32>)> = {
        let mut f: Vec<(&Media, f32, Option<u32>)> = anime
            .iter()
            .filter(|m| serial(m.format))
            .map(|m| {
                let (s, season) = base_closeness(m, &series_base);
                (*m, s, season)
            })
            .filter(|(_, s, _)| *s >= SAME)
            .collect();
        f.sort_by_key(|(m, _, _)| started(m));
        f.into_iter().map(|(m, _, s)| (m, s)).collect()
    };
    if found.is_none() {
        let named = franchise.iter().find(|(_, s)| s.unwrap_or(1) == wanted && (s.is_some() || wanted == 1));
        if let Some((m, _)) = named {
            let exact = closeness(m, &series).max(closeness(m, &series_base));
            found = Some(pick(m, 0.75 + 0.2 * exact.min(1.0), Basis::SeasonNumber));
        }
    }
    // The n-th season by date: plausible, never certain.
    if found.is_none()
        && let Some((m, _)) = franchise.get(wanted as usize - 1)
    {
        found = Some(pick(m, 0.6, Basis::SeasonOrder));
    }
    // Anything with a similar name.
    if found.is_none() {
        let (m, s) = anime.iter().map(|m| (*m, closeness(m, &series))).max_by(|a, b| a.1.total_cmp(&b.1))?;
        if s < 0.5 {
            return None;
        }
        found = Some(pick(m, s * 0.7, Basis::TitleOnly));
    }
    let mut found = found?;

    // Numbering carried over from earlier seasons.
    if let Some(total) = found.media.total()
        && top > total
    {
        let at =
            franchise.iter().position(|(m, _)| m.id == found.media.id && m.service == found.media.service);
        let earlier: Option<u32> =
            at.map(|i| franchise[..i].iter().map(|(m, _)| m.total().unwrap_or(0)).sum());
        match earlier {
            Some(off) if off > 0 && top > off && top - off <= total => {
                found.offset = off;
                found.confidence *= 0.85;
            }
            _ => found.confidence = found.confidence.min(0.4),
        }
    }
    Some(found)
}

fn movie(item: &WatchedItem, anime: &[&Media]) -> Option<Match> {
    let names: Vec<String> = [Some(item.series_title.as_str()), item.episode_title.as_deref()]
        .into_iter()
        .flatten()
        .map(normalize)
        .filter(|t| !t.is_empty())
        .collect();
    let score = |m: &Media| names.iter().map(|n| closeness(m, n)).fold(0.0, f32::max);
    let (m, s) = anime
        .iter()
        .filter(|m| m.format == Format::Movie)
        .map(|m| (*m, score(m)))
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    if s < 0.5 {
        return None;
    }
    let confidence = if s >= SAME { 0.8 + 0.15 * s } else { s * 0.7 };
    Some(Match { media: m.clone(), offset: 0, confidence, basis: Basis::Movie })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kioku::item::{Platform, Watched};
    use crate::model::FuzzyDate;

    fn anime(id: u64, romaji: &str, english: Option<&str>, format: Format, start: &str, eps: u32) -> Media {
        Media {
            id,
            kind: MediaKind::Anime,
            title: romaji.into(),
            title_english: english.map(Into::into),
            format,
            start: FuzzyDate::parse(start),
            episodes: Some(eps),
            ..Media::default()
        }
    }

    fn item(series: &str, season: Option<u32>, season_title: Option<&str>, episode: u32) -> WatchedItem {
        WatchedItem {
            platform: Platform::Crunchyroll,
            series_title: series.into(),
            season,
            season_title: season_title.map(Into::into),
            episode: Some(episode),
            episode_title: None,
            movie: false,
            watched_at: None,
            progress: Watched::Completed,
            external_id: format!("{series}-{season:?}-{episode}"),
        }
    }

    /// An invented franchise: three seasons and a film.
    fn franchise() -> Vec<Media> {
        vec![
            anime(10, "Shiokaze no Tōdai", Some("Lanterns Over Saltmarsh"), Format::Tv, "2021-04-02", 12),
            anime(
                11,
                "Shiokaze no Tōdai 2nd Season",
                Some("Lanterns Over Saltmarsh Season 2"),
                Format::Tv,
                "2022-10-01",
                12,
            ),
            anime(
                12,
                "Shiokaze no Tōdai: Kaimeiki",
                Some("Lanterns Over Saltmarsh: Tidebreak"),
                Format::Tv,
                "2024-01-05",
                13,
            ),
            anime(
                13,
                "Shiokaze no Tōdai Movie: Yoru no Hoshi",
                Some("Lanterns Over Saltmarsh: Night Star"),
                Format::Movie,
                "2023-06-01",
                1,
            ),
            anime(20, "Iron Orchard", None, Format::Tv, "2020-01-01", 24),
        ]
    }

    #[test]
    fn normalises_punctuation_accents_and_dubs() {
        assert_eq!(normalize("Shiokaze no Tōdai (English Dub)"), "shiokaze no todai english");
        assert_eq!(normalize("SPY×FAMILY"), "spy x family");
        assert_eq!(normalize("Spy x Family"), "spy x family");
        assert_eq!(normalize("Iron  Orchard: Ōkami's Tale!"), "iron orchard okamis tale");
    }

    #[test]
    fn finds_seasons_written_in_many_ways() {
        for (t, base, n) in [
            ("lanterns season 2", "lanterns", Some(2)),
            ("lanterns 2nd season", "lanterns", Some(2)),
            ("lanterns s3", "lanterns", Some(3)),
            ("lanterns ii", "lanterns", Some(2)),
            ("mob psycho 100", "mob psycho 100", None),
            ("v", "v", None),
        ] {
            assert_eq!(split_season(t), (base.to_owned(), n), "{t}");
        }
    }

    #[test]
    fn first_season_by_english_or_romaji_title() {
        let c = franchise();
        for name in ["Lanterns Over Saltmarsh", "Shiokaze no Todai"] {
            let m = best(&[item(name, Some(1), None, 3)], &c).unwrap();
            assert_eq!(m.media.id, 10, "{name}");
            assert!(m.sure(), "{name}: {m:?}");
        }
    }

    #[test]
    fn second_season_is_its_own_entry() {
        let m = best(&[item("Lanterns Over Saltmarsh", Some(2), None, 4)], &franchise()).unwrap();
        assert_eq!((m.media.id, m.basis), (11, Basis::SeasonNumber));
        assert!(m.sure());
        assert_eq!(m.episode(4), Some(4));
    }

    #[test]
    fn season_subtitle_names_the_entry() {
        let it = item("Lanterns Over Saltmarsh", Some(3), Some("Lanterns Over Saltmarsh: Tidebreak"), 1);
        let m = best(&[it], &franchise()).unwrap();
        assert_eq!((m.media.id, m.basis), (12, Basis::SeasonTitle));
        assert!(m.sure());
    }

    #[test]
    fn season_by_order_is_only_a_suggestion() {
        // Season 3 has no number in its title, so only the order points to it.
        let m = best(&[item("Lanterns Over Saltmarsh", Some(3), None, 2)], &franchise()).unwrap();
        assert_eq!((m.media.id, m.basis), (12, Basis::SeasonOrder));
        assert!(!m.sure());
    }

    #[test]
    fn numbering_carried_across_seasons_is_shifted() {
        let items = [
            item("Lanterns Over Saltmarsh", Some(2), None, 13),
            item("Lanterns Over Saltmarsh", Some(2), None, 15),
        ];
        let m = best(&items, &franchise()).unwrap();
        assert_eq!((m.media.id, m.offset), (11, 12));
        assert_eq!((m.episode(13), m.episode(15), m.episode(30)), (Some(1), Some(3), None));
        assert!(m.confidence < 0.95);
    }

    #[test]
    fn episodes_beyond_any_entry_are_doubtful() {
        let m = best(&[item("Iron Orchard", Some(1), None, 80)], &franchise()).unwrap();
        assert!(!m.sure());
    }

    #[test]
    fn films_match_only_films() {
        let mut it = item("Lanterns Over Saltmarsh: Night Star", None, None, 1);
        (it.episode, it.movie) = (None, true);
        let m = best(&[it], &franchise()).unwrap();
        assert_eq!((m.media.id, m.basis), (13, Basis::Movie));
        assert!(m.sure());
    }

    #[test]
    fn unknown_titles_have_no_match() {
        assert!(best(&[item("Completely Different Show", Some(1), None, 1)], &franchise()).is_none());
        assert!(best(&[], &franchise()).is_none());
    }

    #[test]
    fn similarity_bounds() {
        assert_eq!(similarity("abc", "abc"), 1.0);
        assert_eq!(similarity("", ""), 0.0);
        assert!(similarity("lanterns over saltmarsh", "lanterns over the saltmarsh") > SAME);
        assert!(similarity("iron orchard", "glass meridian") < 0.3);
    }
}
