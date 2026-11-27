//! An iCalendar file (RFC 5545) with the next episode of every airing title.

use std::fmt::Write;

use crate::model::{ListEntry, ListStatus, Media, MediaKind, Schedule, TitleLanguage};

/// Minutes an episode lasts when the service does not say.
const DEFAULT_MINUTES: u32 = 24;

/// An episode that moves this much later or more is delayed.
pub const DELAY: i64 = 3600;
/// More than this between two episodes of a weekly title is a break: a week
/// went by without one.
pub const BREAK: i64 = 10 * 86_400;

/// The schedule of `new`, just read, carrying what only `old` knew: when the
/// episode before aired and when a delayed episode was first due.
pub fn merge_schedule(old: &Media, new: &Media) -> Schedule {
    let mut s = Schedule { following: new.schedule.following, ..Schedule::default() };
    let (Some(o), Some(n)) = (old.next_airing, new.next_airing) else { return s };
    if n.episode == o.episode {
        s.previous = old.schedule.previous;
        let first = old.schedule.delayed_from.unwrap_or(o.at);
        s.delayed_from = (n.at >= first + DELAY).then_some(first);
    } else if n.episode == o.episode + 1 {
        // The episode known as next has aired.
        s.previous = Some(o.at);
    }
    s
}

/// Something unusual about when a title airs next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// The next episode was due at `was` and airs at `now`.
    Delayed { episode: u32, was: i64, now: i64 },
    /// No episode for over a week: the next one, at `back`, ends the break.
    Break { episode: u32, back: i64 },
    /// The episode after the next one comes over a week after it, at `back`.
    BreakAfter { episode: u32, back: i64 },
}

/// The changes worth saying about `m`'s next episodes.
pub fn changes(m: &Media) -> Vec<Change> {
    let Some(a) = m.next_airing else { return Vec::new() };
    let s = m.schedule;
    let mut out = Vec::new();
    if let Some(was) = s.delayed_from.filter(|w| a.at >= w + DELAY) {
        out.push(Change::Delayed { episode: a.episode, was, now: a.at });
    }
    if s.previous.is_some_and(|p| a.at - p > BREAK) {
        out.push(Change::Break { episode: a.episode, back: a.at });
    }
    if let Some(back) = s.following.filter(|f| f - a.at > BREAK) {
        out.push(Change::BreakAfter { episode: a.episode + 1, back });
    }
    out
}

/// One upcoming episode, as any calendar shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Episode {
    /// Stable across runs: the same title and episode always give the same id.
    pub uid: String,
    /// Unix seconds.
    pub start: i64,
    pub minutes: u32,
    pub title: String,
    pub url: Option<String>,
}

impl AsRef<Self> for ListEntry {
    fn as_ref(&self) -> &Self {
        self
    }
}

/// Anime watched or planned whose next episode has a date from `since` on,
/// soonest first: the titles the calendar holds.
pub fn upcoming<E: AsRef<ListEntry>>(entries: &[E], since: i64) -> Vec<&ListEntry> {
    let mut due: Vec<&ListEntry> = entries
        .iter()
        .map(AsRef::as_ref)
        .filter(|e| e.media.kind == MediaKind::Anime)
        .filter(|e| matches!(e.status, ListStatus::Current | ListStatus::Planning))
        .filter(|e| e.media.next_airing.is_some_and(|a| a.at >= since))
        .collect();
    due.sort_by_key(|e| (e.media.next_airing.map(|a| a.at), e.media.id));
    due
}

/// The next episode of every anime watched or planned, soonest first.
pub fn episodes(entries: &[ListEntry], lang: TitleLanguage, episode_word: &str) -> Vec<Episode> {
    upcoming(entries, i64::MIN)
        .into_iter()
        .filter_map(|e| {
            let (m, a) = (&e.media, e.media.next_airing?);
            Some(Episode {
                uid: format!("kuro-{}-{}-{}@kuro", m.service.key(), m.id, a.episode),
                start: a.at,
                minutes: m.duration.filter(|d| *d > 0).unwrap_or(DEFAULT_MINUTES),
                title: format!("{} · {episode_word} {}", m.display_title(lang), a.episode),
                url: m.web_url(m.service),
            })
        })
        .collect()
}

/// The calendar for `entries`. The same list always gives the same text, so
/// the file is only rewritten when something changed.
pub fn ics(entries: &[ListEntry], lang: TitleLanguage, episode_word: &str) -> String {
    let mut out = String::new();
    for line in ["BEGIN:VCALENDAR", "VERSION:2.0", "PRODID:-//Kuro//Episodes//EN", "CALSCALE:GREGORIAN"] {
        push(&mut out, line);
    }
    push(&mut out, "X-WR-CALNAME:Kuro");
    for e in episodes(entries, lang, episode_word) {
        push(&mut out, "BEGIN:VEVENT");
        push(&mut out, &format!("UID:{}", e.uid));
        // Stamped with the airing time, not the time of writing, to keep the text stable.
        push(&mut out, &format!("DTSTAMP:{}", utc(e.start)));
        push(&mut out, &format!("DTSTART:{}", utc(e.start)));
        push(&mut out, &format!("DURATION:PT{}M", e.minutes));
        push(&mut out, &format!("SUMMARY:{}", escape(&e.title)));
        if let Some(u) = &e.url {
            push(&mut out, &format!("URL:{}", escape(u)));
        }
        push(&mut out, "TRANSP:TRANSPARENT");
        push(&mut out, "END:VEVENT");
    }
    push(&mut out, "END:VCALENDAR");
    out
}

/// `20261126T153000Z`
fn utc(at: i64) -> String {
    jiff::Timestamp::from_second(at)
        .map_or_else(|_| "19700101T000000Z".into(), |t| t.strftime("%Y%m%dT%H%M%SZ").to_string())
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' | ';' | ',' => {
                out.push('\\');
                out.push(c);
            }
            '\n' => out.push_str("\\n"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// Adds a content line, folded at 75 octets without splitting a character.
fn push(out: &mut String, line: &str) {
    let mut used = 0;
    for c in line.chars() {
        if used + c.len_utf8() > 75 {
            out.push_str("\r\n ");
            used = 1;
        }
        out.push(c);
        used += c.len_utf8();
    }
    let _ = out.write_str("\r\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Airing, Media};

    fn airing(id: u64, title: &str, status: ListStatus, ep: u32, at: i64) -> ListEntry {
        let mut e =
            ListEntry::new(Media { id, kind: MediaKind::Anime, title: title.into(), ..Media::default() });
        e.status = status;
        e.media.next_airing = Some(Airing { episode: ep, at });
        e
    }

    const DAY: i64 = 86_400;

    fn next(ep: u32, at: i64, schedule: Schedule) -> Media {
        Media { next_airing: Some(Airing { episode: ep, at }), schedule, ..Media::default() }
    }

    #[test]
    fn a_later_time_for_the_same_episode_is_a_delay() {
        let first = next(5, 1000 * DAY, Schedule::default());
        let moved = next(5, 1007 * DAY, Schedule { following: Some(1014 * DAY), ..Schedule::default() });
        let s = merge_schedule(&first, &moved);
        assert_eq!(
            s,
            Schedule { following: Some(1014 * DAY), previous: None, delayed_from: Some(1000 * DAY) }
        );
        // Moving again keeps the first time; moving back ends the delay.
        let again = merge_schedule(&next(5, 1007 * DAY, s), &next(5, 1009 * DAY, Schedule::default()));
        assert_eq!(again.delayed_from, Some(1000 * DAY));
        let back = merge_schedule(&next(5, 1009 * DAY, again), &next(5, 1000 * DAY, Schedule::default()));
        assert_eq!(back.delayed_from, None);
        // Minutes are not a delay.
        let late = merge_schedule(&first, &next(5, 1000 * DAY + 600, Schedule::default()));
        assert_eq!(late.delayed_from, None);
    }

    #[test]
    fn an_aired_episode_becomes_the_previous_one() {
        let s = merge_schedule(
            &next(5, 1000 * DAY, Schedule::default()),
            &next(6, 1014 * DAY, Schedule::default()),
        );
        assert_eq!((s.previous, s.delayed_from), (Some(1000 * DAY), None));
        let m = next(6, 1014 * DAY, s);
        assert_eq!(changes(&m), [Change::Break { episode: 6, back: 1014 * DAY }]);
        // Several episodes at once say nothing about the gap.
        let s = merge_schedule(
            &next(5, 1000 * DAY, Schedule::default()),
            &next(8, 1020 * DAY, Schedule::default()),
        );
        assert_eq!(s.previous, None);
    }

    #[test]
    fn changes_name_delays_and_weeks_without_an_episode() {
        let weekly = next(
            6,
            1007 * DAY,
            Schedule { previous: Some(1000 * DAY), following: Some(1014 * DAY), delayed_from: None },
        );
        assert!(changes(&weekly).is_empty());
        let gap = next(6, 1007 * DAY, Schedule { following: Some(1021 * DAY), ..Schedule::default() });
        assert_eq!(changes(&gap), [Change::BreakAfter { episode: 7, back: 1021 * DAY }]);
        let late = next(6, 1008 * DAY, Schedule { delayed_from: Some(1007 * DAY), ..Schedule::default() });
        assert_eq!(changes(&late), [Change::Delayed { episode: 6, was: 1007 * DAY, now: 1008 * DAY }]);
        assert!(changes(&Media::default()).is_empty());
    }

    #[test]
    fn lists_the_next_episode_of_what_is_watched_or_planned() {
        let mut done = airing(3, "Done", ListStatus::Completed, 5, 1_800_000_000);
        done.media.next_airing = Some(Airing { episode: 5, at: 1_800_000_000 });
        let list = [
            airing(2, "Later", ListStatus::Planning, 1, 1_800_090_000),
            airing(1, "Soon, really; now", ListStatus::Current, 10, 1_800_000_000),
            done,
            ListEntry::new(Media {
                id: 4,
                kind: MediaKind::Anime,
                title: "No date".into(),
                ..Media::default()
            }),
        ];
        let text = ics(&list, TitleLanguage::Romaji, "Ep.");
        assert!(text.starts_with("BEGIN:VCALENDAR\r\n") && text.ends_with("END:VCALENDAR\r\n"));
        assert_eq!(text.matches("BEGIN:VEVENT").count(), 2);
        assert!(text.contains("SUMMARY:Soon\\, really\\; now · Ep. 10\r\n"));
        assert!(text.contains("DTSTART:20270115T080000Z\r\n"));
        assert!(text.contains("DURATION:PT24M"));
        assert!(text.find("Soon").unwrap() < text.find("Later").unwrap(), "sorted by airing time");
        assert_eq!(text, ics(&list, TitleLanguage::Romaji, "Ep."), "stable");
    }

    #[test]
    fn upcoming_keeps_what_airs_from_the_given_time() {
        let list = [
            airing(1, "Later", ListStatus::Current, 2, 300),
            airing(2, "Aired", ListStatus::Current, 9, 100),
            airing(3, "Soon", ListStatus::Planning, 1, 200),
            airing(4, "Dropped", ListStatus::Dropped, 4, 250),
        ];
        let ids: Vec<u64> = upcoming(&list, 150).iter().map(|e| e.media.id).collect();
        assert_eq!(ids, [3, 1]);
    }

    #[test]
    fn long_lines_fold_between_characters() {
        let mut out = String::new();
        push(&mut out, &format!("SUMMARY:{}", "黒".repeat(40)));
        for line in out.split("\r\n") {
            assert!(line.len() <= 75, "{line}");
        }
        assert_eq!(out.replace("\r\n ", ""), format!("SUMMARY:{}\r\n", "黒".repeat(40)));
    }
}
