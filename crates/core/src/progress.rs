//! Rules that turn a user action into a new list entry.

use jiff::civil::Date;

use crate::model::{FuzzyDate, ListEntry, ListStatus, MediaKind};

/// Moves progress by `delta`, applying the usual side effects.
pub fn step(entry: &ListEntry, delta: i32, today: Date) -> ListEntry {
    let target = i64::from(entry.progress).saturating_add(i64::from(delta)).clamp(0, i64::from(u32::MAX));
    set_progress(entry, target as u32, today)
}

/// Sets progress, clamped to the known total. Starting a title moves it to
/// `Current`; reaching the end completes it (and counts a finished repeat).
pub fn set_progress(entry: &ListEntry, value: u32, today: Date) -> ListEntry {
    let mut e = entry.clone();
    let total = e.media.total();
    let value = total.map_or(value, |t| value.min(t));
    let advanced = value > e.progress;
    e.progress = value;
    if !advanced {
        // Going back from a finished title means it is being watched again.
        if e.status == ListStatus::Completed && total.is_some_and(|t| value < t) {
            e.status = ListStatus::Current;
            e.finish = None;
        }
        return e;
    }
    if total == Some(value) {
        return complete(&e, today);
    }
    e.status = ListStatus::Current;
    if e.start.is_none() && !e.repeating {
        e.start = Some(FuzzyDate::from_date(today));
    }
    e
}

/// Changes volume progress without completing the title by itself.
pub fn step_volumes(entry: &ListEntry, delta: i32, today: Date) -> ListEntry {
    let mut e = entry.clone();
    let target =
        i64::from(e.progress_volumes).saturating_add(i64::from(delta)).clamp(0, i64::from(u32::MAX)) as u32;
    let target = e.media.volumes.filter(|v| *v > 0).map_or(target, |t| target.min(t));
    let advanced = target > e.progress_volumes;
    e.progress_volumes = target;
    if advanced && e.status == ListStatus::Planning {
        e.status = ListStatus::Current;
        e.start.get_or_insert(FuzzyDate::from_date(today));
    }
    e
}

pub fn complete(entry: &ListEntry, today: Date) -> ListEntry {
    let mut e = entry.clone();
    if let Some(total) = e.media.total() {
        e.progress = total;
    }
    if e.media.kind == MediaKind::Manga
        && let Some(v) = e.media.volumes.filter(|v| *v > 0)
    {
        e.progress_volumes = v;
    }
    e.status = ListStatus::Completed;
    if e.repeating {
        e.repeating = false;
        e.repeat_count = e.repeat_count.saturating_add(1);
    } else {
        let date = FuzzyDate::from_date(today);
        e.start.get_or_insert(date);
        e.finish.get_or_insert(date);
    }
    e
}

pub fn start_repeat(entry: &ListEntry) -> ListEntry {
    ListEntry {
        repeating: true,
        status: ListStatus::Current,
        progress: 0,
        progress_volumes: 0,
        ..entry.clone()
    }
}

/// Applies a status picked by the user.
pub fn set_status(entry: &ListEntry, status: ListStatus, today: Date) -> ListEntry {
    match status {
        ListStatus::Completed if entry.status != ListStatus::Completed => complete(entry, today),
        ListStatus::Current => {
            let mut e = ListEntry { status, ..entry.clone() };
            if !e.repeating {
                e.start.get_or_insert(FuzzyDate::from_date(today));
            }
            e
        }
        _ => {
            ListEntry { status, repeating: entry.repeating && status == ListStatus::Paused, ..entry.clone() }
        }
    }
}

/// Makes an arbitrary edit consistent before saving it.
pub fn sanitize(entry: &ListEntry) -> ListEntry {
    let mut e = entry.clone();
    if let Some(t) = e.media.total() {
        e.progress = e.progress.min(t);
    }
    if let Some(v) = e.media.volumes.filter(|v| *v > 0) {
        e.progress_volumes = e.progress_volumes.min(v);
    }
    e.score = e.score.min(100);
    if let (Some(s), Some(f)) = (e.start.and_then(|d| d.exact()), e.finish.and_then(|d| d.exact()))
        && f < s
    {
        e.finish = e.start;
    }
    e
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Media;

    fn day() -> Date {
        jiff::civil::date(2026, 11, 25)
    }
    fn anime(total: Option<u32>) -> ListEntry {
        ListEntry::new(Media { episodes: total, ..Media::default() })
    }

    #[test]
    fn first_episode_starts_watching() {
        let e = step(&anime(Some(12)), 1, day());
        assert_eq!((e.status, e.progress), (ListStatus::Current, 1));
        assert_eq!(e.start, Some(FuzzyDate::from_date(day())));
    }

    #[test]
    fn last_episode_completes() {
        let mut e = anime(Some(2));
        e.progress = 1;
        let e = step(&e, 1, day());
        assert_eq!(e.status, ListStatus::Completed);
        assert!(e.finish.is_some());
    }

    #[test]
    fn unknown_total_never_completes() {
        let mut e = anime(None);
        e.progress = 1100;
        assert_eq!(step(&e, 1, day()).status, ListStatus::Current);
    }

    #[test]
    fn progress_is_clamped_both_ways() {
        assert_eq!(step(&anime(Some(12)), -1, day()).progress, 0);
        assert_eq!(step(&anime(Some(12)), i32::MAX, day()).progress, 12);
        let mut e = anime(None);
        e.progress = u32::MAX;
        assert_eq!(step(&e, 5, day()).progress, u32::MAX);
    }

    #[test]
    fn finishing_a_rewatch_counts_and_keeps_dates() {
        let mut e = anime(Some(3));
        e = complete(&e, day());
        let original = e.finish;
        e = start_repeat(&e);
        assert_eq!((e.progress, e.repeating), (0, true));
        for _ in 0..3 {
            e = step(&e, 1, day().tomorrow().unwrap());
        }
        assert_eq!((e.status, e.repeating, e.repeat_count), (ListStatus::Completed, false, 1));
        assert_eq!(e.finish, original);
    }

    #[test]
    fn stepping_back_reopens() {
        let e = complete(&anime(Some(3)), day());
        let e = step(&e, -1, day());
        assert_eq!((e.status, e.finish), (ListStatus::Current, None));
    }

    #[test]
    fn completing_manga_fills_volumes() {
        let m = Media { kind: MediaKind::Manga, chapters: Some(10), volumes: Some(2), ..Media::default() };
        let e = set_status(&ListEntry::new(m), ListStatus::Completed, day());
        assert_eq!((e.progress, e.progress_volumes), (10, 2));
    }

    #[test]
    fn sanitize_fixes_dates_and_bounds() {
        let mut e = anime(Some(3));
        e.progress = 99;
        e.score = 255;
        e.start = FuzzyDate::parse("2026-05-05");
        e.finish = FuzzyDate::parse("2025-01-01");
        let e = sanitize(&e);
        assert_eq!((e.progress, e.score, e.finish), (3, 100, e.start));
    }
}
