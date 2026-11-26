//! An iCalendar file (RFC 5545) with the next episode of every airing title.

use std::fmt::Write;

use crate::model::{ListEntry, ListStatus, MediaKind, TitleLanguage};

/// Minutes an episode lasts when the service does not say.
const DEFAULT_MINUTES: u32 = 24;

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
