//! Turns matched history into proposed list changes, which the user reviews
//! before anything is written.
//!
//! Progress never moves back: an episode below the current one changes
//! nothing, a completed title is never touched and a title being watched again
//! is left alone. Applying the same history twice changes nothing either,
//! because imported items are remembered by their platform id.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use jiff::civil::Date;

use crate::kioku::item::WatchedItem;
use crate::kioku::matcher::{self, Match};
use crate::model::{FuzzyDate, ListEntry, ListStatus, Media};
use crate::progress;

/// Items of one season of one series, matched together.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub items: Vec<WatchedItem>,
}

impl Group {
    /// Text to search the tracker for.
    pub fn query(&self) -> &str {
        self.items.first().map_or("", |i| i.series_title.as_str())
    }

    /// Stable name of the series and season, to remember a title chosen for it.
    pub fn key(&self) -> String {
        self.items
            .first()
            .map(|i| {
                let k = group_key(i);
                let mut key = format!(
                    "{}|{}|{}|{}|{}",
                    k.0,
                    k.1.map_or(String::new(), |s| s.to_string()),
                    k.2,
                    k.3,
                    k.4
                );
                // Only platforms that give a series id add it, so older keys stay valid.
                if !k.5.is_empty() {
                    key = format!("{key}|{}", k.5);
                }
                key
            })
            .unwrap_or_default()
    }
}

/// Series, season number, season title, film, film title and series id, normalised.
type GroupKey = (String, Option<u32>, String, bool, String, String);

fn group_key(i: &WatchedItem) -> GroupKey {
    let film = if i.movie {
        i.episode_title.as_deref().map(matcher::normalize).unwrap_or_default()
    } else {
        String::new()
    };
    (
        matcher::normalize(&i.series_title),
        i.season,
        i.season_title.as_deref().map(matcher::normalize).unwrap_or_default(),
        i.movie,
        film,
        i.series_id.clone().unwrap_or_default(),
    )
}

/// Played far enough, not imported before and once per platform id, grouped by
/// series and season.
pub fn groups(items: Vec<WatchedItem>, known: &HashSet<String>) -> Vec<Group> {
    let mut seen = HashSet::new();
    let mut by: BTreeMap<GroupKey, Vec<WatchedItem>> = BTreeMap::new();
    for i in items {
        if !i.progress.counts() || known.contains(&i.external_id) || !seen.insert(i.external_id.clone()) {
            continue;
        }
        by.entry(group_key(&i)).or_default().push(i);
    }
    by.into_values().map(|items| Group { items }).collect()
}

#[derive(Debug, Clone, PartialEq)]
pub enum Doubt {
    /// The match is not certain enough.
    Unsure,
    /// Episodes between the current progress and the newest one are missing
    /// from the history.
    Gap { missing: u32 },
    /// The title is paused or dropped in the list.
    Status(ListStatus),
    /// Some episodes do not belong to the matched entry.
    Outside { count: u32 },
}

/// Why a matched group proposes nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    Completed,
    Rewatching,
    /// The list is already at or past the history.
    Behind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Proposal {
    pub media: Media,
    /// Platform ids this change covers.
    pub items: Vec<String>,
    pub before: Option<ListEntry>,
    pub after: ListEntry,
    pub confidence: f32,
    pub doubts: Vec<Doubt>,
    /// Ticked in the review: certain and without doubts.
    pub selected: bool,
    /// Episode numbers were counted, not given by the platform.
    pub estimated: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Change(Box<Proposal>),
    /// Matched, but nothing to change; the items still count as imported.
    Nothing {
        media: Box<Media>,
        items: Vec<String>,
        why: Skip,
    },
    /// No title found: left for the user, never applied.
    Unmatched(Group),
}

fn date(at: Option<i64>) -> Option<FuzzyDate> {
    let ts = jiff::Timestamp::from_second(at?).ok()?;
    Some(FuzzyDate::from_date(ts.to_zoned_datetime(jiff::tz::TimeZone::system()).date()))
}

/// What applying `episodes` (tracker numbering, with when each was watched)
/// would do to the current entry.
pub fn propose(
    m: &Match,
    before: Option<&ListEntry>,
    episodes: &[(u32, Option<i64>)],
    items: Vec<String>,
    today: Date,
) -> Result<Proposal, Skip> {
    if let Some(b) = before {
        if b.repeating {
            return Err(Skip::Rewatching);
        }
        if b.status == ListStatus::Completed {
            return Err(Skip::Completed);
        }
    }
    let current = before.map_or(0, |b| b.progress);
    let top = episodes.iter().map(|(e, _)| *e).max().unwrap_or(0);
    if top <= current {
        return Err(Skip::Behind);
    }
    let ahead: Vec<&(u32, Option<i64>)> = episodes.iter().filter(|(e, _)| *e > current).collect();
    let first = ahead.iter().filter_map(|(_, at)| *at).min();
    let last = ahead.iter().filter_map(|(_, at)| *at).max();

    let mut doubts = Vec::new();
    if !m.sure() {
        doubts.push(Doubt::Unsure);
    }
    let watched: BTreeSet<u32> = ahead.iter().map(|(e, _)| *e).collect();
    // Every watched episode lies in current+1..=top, so this counts the rest
    // without walking a range that a bogus episode number could make huge.
    let missing = (top - current).saturating_sub(u32::try_from(watched.len()).unwrap_or(u32::MAX));
    if missing > 0 {
        doubts.push(Doubt::Gap { missing });
    }
    if let Some(s) =
        before.map(|b| b.status).filter(|s| matches!(s, ListStatus::Paused | ListStatus::Dropped))
    {
        doubts.push(Doubt::Status(s));
    }

    let mut after = before.cloned().unwrap_or_else(|| ListEntry::new(m.media.clone()));
    if after.start.is_none() {
        after.start = date(first);
    }
    let had_finish = after.finish.is_some();
    after = progress::set_progress(&after, top, today);
    if after.status == ListStatus::Completed && !had_finish {
        after.finish = date(last).or(after.finish);
    }
    let after = progress::sanitize(&after);
    debug_assert!(after.progress >= current);
    let selected = doubts.is_empty();
    Ok(Proposal {
        media: m.media.clone(),
        items,
        before: before.cloned(),
        after,
        confidence: m.confidence,
        doubts,
        selected,
        estimated: false,
    })
}

/// Matches a group against the tracker's candidates and proposes a change.
/// `entry` gives the current list entry of a matched title.
pub fn review(
    group: Group,
    candidates: &[Media],
    entry: impl Fn(&Media) -> Option<ListEntry>,
    today: Date,
) -> Outcome {
    let found = matcher::best(&group.items, candidates);
    review_with(group, found, entry, today)
}

/// [`review`] with the match already decided, such as a title the user chose.
pub fn review_with(
    group: Group,
    found: Option<Match>,
    entry: impl Fn(&Media) -> Option<ListEntry>,
    today: Date,
) -> Outcome {
    let Some(m) = found else { return Outcome::Unmatched(group) };
    let mut episodes = Vec::new();
    let mut outside = 0u32;
    for i in &group.items {
        let tracker = if i.movie { Some(1) } else { i.episode.and_then(|e| m.episode(e)) };
        match tracker {
            Some(e) => episodes.push((e, i.watched_at)),
            None => outside += 1,
        }
    }
    let items: Vec<String> = group.items.iter().map(|i| i.external_id.clone()).collect();
    if episodes.is_empty() {
        return Outcome::Unmatched(group);
    }
    let before = entry(&m.media);
    match propose(&m, before.as_ref(), &episodes, items.clone(), today) {
        Ok(mut p) => {
            p.estimated = group.items.iter().any(|i| i.estimated);
            if outside > 0 {
                p.doubts.push(Doubt::Outside { count: outside });
                p.selected = false;
            }
            Outcome::Change(Box::new(p))
        }
        Err(why) => Outcome::Nothing { media: Box::new(m.media), items, why },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kioku::item::{Platform, Watched};
    use crate::kioku::matcher::Basis;
    use crate::model::{Format, MediaKind};

    fn day() -> Date {
        jiff::civil::date(2026, 11, 25)
    }

    fn media(total: u32) -> Media {
        Media {
            id: 1,
            kind: MediaKind::Anime,
            title: "Iron Orchard".into(),
            format: Format::Tv,
            episodes: Some(total),
            ..Media::default()
        }
    }

    fn sure(total: u32) -> Match {
        Match { media: media(total), offset: 0, confidence: 0.95, basis: Basis::SeasonNumber }
    }

    fn eps(list: &[u32]) -> Vec<(u32, Option<i64>)> {
        list.iter().map(|e| (*e, Some(1_790_000_000 + i64::from(*e) * 86_400))).collect()
    }

    fn entry(status: ListStatus, progress: u32) -> ListEntry {
        ListEntry { status, progress, ..ListEntry::new(media(12)) }
    }

    fn item(id: &str, ep: u32, progress: Watched) -> WatchedItem {
        WatchedItem {
            platform: Platform::Crunchyroll,
            series_title: "Iron Orchard".into(),
            season: Some(1),
            season_title: None,
            episode: Some(ep),
            estimated: false,
            series_id: None,
            episode_title: None,
            movie: false,
            watched_at: Some(1_790_000_000),
            progress,
            external_id: id.into(),
        }
    }

    #[test]
    fn new_title_starts_on_the_first_watched_day() {
        let p = propose(&sure(12), None, &eps(&[1, 2, 3]), vec![], day()).unwrap();
        assert_eq!((p.after.status, p.after.progress), (ListStatus::Current, 3));
        assert_eq!(p.after.start, date(Some(1_790_000_000 + 86_400)));
        assert!(p.selected);
    }

    #[test]
    fn never_moves_progress_back() {
        let before = entry(ListStatus::Current, 8);
        assert_eq!(propose(&sure(12), Some(&before), &eps(&[3, 4, 8]), vec![], day()), Err(Skip::Behind));
        let p = propose(&sure(12), Some(&before), &eps(&[2, 9]), vec![], day()).unwrap();
        assert_eq!(p.after.progress, 9);
    }

    #[test]
    fn never_touches_completed_or_rewatched_titles() {
        let done = entry(ListStatus::Completed, 12);
        assert_eq!(propose(&sure(24), Some(&done), &eps(&[13]), vec![], day()), Err(Skip::Completed));
        let again = ListEntry { repeating: true, ..entry(ListStatus::Current, 2) };
        assert_eq!(propose(&sure(12), Some(&again), &eps(&[3]), vec![], day()), Err(Skip::Rewatching));
    }

    #[test]
    fn last_episode_completes_with_the_watched_date() {
        let p = propose(&sure(3), None, &eps(&[1, 2, 3]), vec![], day()).unwrap();
        assert_eq!(p.after.status, ListStatus::Completed);
        assert_eq!(p.after.finish, date(Some(1_790_000_000 + 3 * 86_400)));
    }

    #[test]
    fn gaps_paused_titles_and_unsure_matches_are_unticked() {
        let p =
            propose(&sure(12), Some(&entry(ListStatus::Current, 2)), &eps(&[3, 6]), vec![], day()).unwrap();
        assert!(!p.selected);
        assert!(p.doubts.contains(&Doubt::Gap { missing: 2 }));
        let p = propose(&sure(12), Some(&entry(ListStatus::Paused, 2)), &eps(&[3]), vec![], day()).unwrap();
        assert_eq!((p.selected, p.doubts.clone()), (false, vec![Doubt::Status(ListStatus::Paused)]));
        let unsure = Match { confidence: 0.6, ..sure(12) };
        let p = propose(&unsure, None, &eps(&[1]), vec![], day()).unwrap();
        assert_eq!((p.selected, p.doubts), (false, vec![Doubt::Unsure]));
    }

    #[test]
    fn importing_twice_changes_nothing() {
        let items = vec![item("a", 1, Watched::Completed), item("b", 2, Watched::Completed)];
        let candidates = [media(12)];
        let g = groups(items.clone(), &HashSet::new());
        let Outcome::Change(p) = review(g[0].clone(), &candidates, |_| None, day()) else { panic!() };
        assert_eq!(p.after.progress, 2);
        // Once applied, its ids are known and the same history yields nothing.
        let known: HashSet<String> = p.items.into_iter().collect();
        assert!(groups(items, &known).is_empty());
        // Even if those ids were forgotten, the list is already there.
        let g = groups(vec![item("a", 1, Watched::Completed)], &HashSet::new());
        let now = p.after;
        assert!(matches!(
            review(g[0].clone(), &candidates, |_| Some(now.clone()), day()),
            Outcome::Nothing { why: Skip::Behind, .. }
        ));
    }

    #[test]
    fn partly_watched_and_repeated_items_are_dropped() {
        let items = vec![
            item("a", 1, Watched::Completed),
            item("a", 1, Watched::Completed),
            item("b", 2, Watched::Fraction(0.95)),
            item("c", 3, Watched::Fraction(0.4)),
        ];
        let g = groups(items, &HashSet::new());
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].items.iter().map(|i| i.external_id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    }

    #[test]
    fn odd_numbers_from_a_platform_never_panic_or_hang() {
        let open = Media { episodes: None, ..media(12) };
        let candidates = [open, Media { id: 2, format: Format::Movie, ..media(1) }];
        for (season, ep) in [(Some(0), 0), (Some(0), 1), (None, u32::MAX), (Some(u32::MAX), 1), (Some(1), 0)]
        {
            let mut i = item("x", ep, Watched::Completed);
            i.season = season;
            let g = groups(vec![i], &HashSet::new());
            let started = std::time::Instant::now();
            let _ = review(g[0].clone(), &candidates, |_| None, day());
            assert!(started.elapsed().as_secs() < 1, "{season:?} {ep}");
        }
    }

    #[test]
    fn a_chosen_title_matches_what_the_names_do_not() {
        let renamed = |id: &str, ep| WatchedItem {
            series_title: "Huerto de hierro".into(),
            ..item(id, ep, Watched::Completed)
        };
        let g = groups(vec![renamed("a", 1), renamed("b", 2)], &HashSet::new());
        assert!(matches!(review(g[0].clone(), &[media(12)], |_| None, day()), Outcome::Unmatched(_)));
        let chosen = Some(Match::chosen(media(12)));
        let Outcome::Change(p) = review_with(g[0].clone(), chosen, |_| None, day()) else { panic!() };
        assert!(p.selected && p.after.progress == 2);
        assert_eq!(
            g[0].key(),
            groups(vec![item("b", 9, Watched::Completed)], &HashSet::new())[0]
                .key()
                .replace("iron orchard", "huerto de hierro")
        );
    }

    #[test]
    fn series_with_the_same_name_stay_apart_by_id() {
        let named = |id: &str, series: &str| WatchedItem {
            series_title: "Temporada 1".into(),
            series_id: Some(series.into()),
            ..item(id, 1, Watched::Completed)
        };
        let g = groups(vec![named("a", "S1"), named("b", "S2")], &HashSet::new());
        assert_eq!(g.len(), 2);
        assert_ne!(g[0].key(), g[1].key());
        // Without an id the key is the one it always was.
        let plain = groups(vec![item("c", 1, Watched::Completed)], &HashSet::new());
        assert_eq!(plain[0].key(), "iron orchard|1||false|");
    }

    #[test]
    fn counted_episodes_are_marked_but_still_ticked() {
        let mut items = vec![item("a", 1, Watched::Completed), item("b", 2, Watched::Completed)];
        for i in &mut items {
            i.estimated = true;
        }
        let g = groups(items, &HashSet::new());
        let Outcome::Change(p) = review(g[0].clone(), &[media(12)], |_| None, day()) else { panic!() };
        assert!(p.estimated && p.selected);
        // More counted episodes than the title has: not ticked.
        let many: Vec<WatchedItem> = (1..=14)
            .map(|n| WatchedItem { estimated: true, ..item(&n.to_string(), n, Watched::Completed) })
            .collect();
        let g = groups(many, &HashSet::new());
        let Outcome::Change(p) = review(g[0].clone(), &[media(12)], |_| None, day()) else { panic!() };
        assert!(!p.selected);
    }

    #[test]
    fn unknown_series_stay_for_review() {
        let g = groups(vec![item("a", 1, Watched::Completed)], &HashSet::new());
        assert!(matches!(review(g[0].clone(), &[], |_| None, day()), Outcome::Unmatched(_)));
    }
}
