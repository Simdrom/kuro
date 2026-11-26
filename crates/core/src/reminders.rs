//! Reminders about titles left behind, and the weekly summary.

use jiff::civil::Weekday;
use jiff::tz::TimeZone;

use crate::model::{ListEntry, ListStatus, MediaKind, MediaRef};

/// Titles being watched or read with no progress for `days`. A title that
/// is only waiting for its next episode is not behind.
pub fn stale<'a>(entries: &'a [ListEntry], now: i64, days: u32, muted: &[MediaRef]) -> Vec<&'a ListEntry> {
    if days == 0 {
        return Vec::new();
    }
    let limit = now - i64::from(days) * 86_400;
    entries
        .iter()
        .filter(|e| e.status == ListStatus::Current && e.updated_at > 0 && e.updated_at <= limit)
        .filter(|e| !muted.contains(&e.media.media_ref()))
        .filter(|e| e.media.next_airing.is_none_or(|a| e.progress + 1 < a.episode))
        .collect()
}

/// Fires once for each time a title stops moving.
pub fn stale_key(e: &ListEntry) -> String {
    let m = &e.media;
    format!("stale:{}:{}:{}:{}", m.service.key(), m.kind.key(), m.id, e.updated_at)
}

/// The next episode or chapter of a stale title, when it surely exists.
pub fn next_unit(e: &ListEntry) -> Option<u32> {
    let next = e.progress + 1;
    let aired = match e.media.next_airing {
        Some(a) => next < a.episode,
        None => e.media.total().is_some_and(|t| next <= t),
    };
    aired.then_some(next)
}

/// What the week holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Week {
    /// Titles with an episode out in the next seven days.
    pub airing: usize,
    /// Episodes already out and not watched yet.
    pub behind: u32,
}

pub fn week(entries: &[ListEntry], now: i64) -> Week {
    let mut w = Week::default();
    for e in entries.iter().filter(|e| e.media.kind == MediaKind::Anime) {
        let watching = e.status == ListStatus::Current;
        if let Some(a) = e.media.next_airing {
            if matches!(e.status, ListStatus::Current | ListStatus::Planning)
                && a.at >= now
                && a.at < now + 7 * 86_400
            {
                w.airing += 1;
            }
            if watching {
                w.behind += a.episode.saturating_sub(1).saturating_sub(e.progress);
            }
        } else if watching && let Some(t) = e.media.total() {
            w.behind += t.saturating_sub(e.progress);
        }
    }
    w
}

/// The key of this week's summary once it is due: from Monday at `hour`, local time.
pub fn week_key(now: i64, hour: u32, tz: &TimeZone) -> Option<String> {
    let z = jiff::Timestamp::from_second(now).ok()?.to_zoned_datetime(tz.clone());
    let started = z.weekday() != Weekday::Monday || i64::from(z.hour()) >= i64::from(hour.min(23));
    let w = z.date().iso_week_date();
    started.then(|| format!("week:{}-W{:02}", w.year(), w.week()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Airing, Media};

    const DAY: i64 = 86_400;
    const NOW: i64 = 1_800_000_000;

    fn watching(id: u64, progress: u32, updated: i64) -> ListEntry {
        let mut e =
            ListEntry::new(Media { id, kind: MediaKind::Anime, episodes: Some(12), ..Media::default() });
        (e.status, e.progress, e.updated_at) = (ListStatus::Current, progress, updated);
        e
    }

    #[test]
    fn stale_titles_are_the_ones_left_behind() {
        let old = watching(1, 3, NOW - 20 * DAY);
        let fresh = watching(2, 3, NOW - DAY);
        let mut waiting = watching(3, 4, NOW - 20 * DAY);
        waiting.media.next_airing = Some(Airing { episode: 5, at: NOW + DAY });
        let mut behind = waiting.clone();
        behind.media.id = 4;
        behind.progress = 2;
        let mut paused = watching(5, 3, NOW - 20 * DAY);
        paused.status = ListStatus::Paused;
        let list = [old, fresh, waiting, behind, paused];
        let ids: Vec<u64> = stale(&list, NOW, 14, &[]).iter().map(|e| e.media.id).collect();
        assert_eq!(ids, [1, 4]);
        assert!(stale(&list, NOW, 0, &[]).is_empty());
        assert_eq!(stale(&list, NOW, 14, &[list[0].media.media_ref()]).len(), 1);
        assert_ne!(stale_key(&list[0]), stale_key(&watching(1, 4, NOW)), "a new stall is new");
    }

    #[test]
    fn only_episodes_that_exist_are_offered() {
        assert_eq!(next_unit(&watching(1, 3, 0)), Some(4));
        assert_eq!(next_unit(&watching(1, 12, 0)), None);
        let mut e = watching(1, 4, 0);
        e.media.next_airing = Some(Airing { episode: 5, at: NOW });
        assert_eq!(next_unit(&e), None);
    }

    #[test]
    fn the_week_counts_what_airs_and_what_is_left() {
        let mut a = watching(1, 2, NOW);
        a.media.next_airing = Some(Airing { episode: 6, at: NOW + 2 * DAY });
        let b = watching(2, 10, NOW);
        let mut later = watching(3, 0, NOW);
        later.status = ListStatus::Planning;
        later.media.next_airing = Some(Airing { episode: 1, at: NOW + 9 * DAY });
        assert_eq!(week(&[a, b, later], NOW), Week { airing: 1, behind: 3 + 2 });
    }

    #[test]
    fn the_summary_starts_on_monday_at_the_hour() {
        let tz = TimeZone::UTC;
        // 2027-01-18 is a Monday.
        let monday = jiff::civil::date(2027, 1, 18).at(9, 0, 0, 0).to_zoned_datetime(tz.clone()).unwrap();
        let at = |h: i64| monday.timestamp().as_second() + h * 3600;
        assert_eq!(week_key(at(0), 10, &tz), None);
        assert_eq!(week_key(at(1), 10, &tz).as_deref(), Some("week:2027-W03"));
        assert_eq!(week_key(at(30), 10, &tz).as_deref(), Some("week:2027-W03"), "later in the week");
        assert_eq!(week_key(at(24 * 7 - 1), 10, &tz), None, "next Monday, too early");
        assert_eq!(week_key(at(24 * 7 + 1), 10, &tz).as_deref(), Some("week:2027-W04"));
    }
}
