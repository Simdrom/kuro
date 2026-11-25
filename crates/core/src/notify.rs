//! Decides which notifications are due.

use jiff::tz::TimeZone;

use crate::model::{FuzzyDate, ListEntry, ListStatus, MediaKind, ReleaseStatus};
use crate::settings::NotificationSettings;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Event {
    Episode(u32),
    Premiere,
    Finished,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Due {
    /// Stable id, so each notification fires once.
    pub key: String,
    pub at: i64,
    pub event: Event,
    pub entry: ListEntry,
}

/// Alerts older than this are dropped rather than shown late.
const MAX_AGE: i64 = 12 * 3600;

fn local_at(d: FuzzyDate, hour: i8, tz: &TimeZone) -> Option<i64> {
    let at = tz.to_timestamp(d.exact()?.at(hour, 0, 0, 0)).ok()?;
    Some(at.as_second())
}

/// Every notification for `entries`, past or future, that settings allow.
pub fn plan(entries: &[ListEntry], s: &NotificationSettings, tz: &TimeZone) -> Vec<Due> {
    let mut out = Vec::new();
    if !s.enabled {
        return out;
    }
    let hour = s.date_hour.min(23) as i8;
    for e in entries.iter().filter(|e| e.media.kind == MediaKind::Anime) {
        let m = &e.media;
        let wanted = match e.status {
            ListStatus::Current => true,
            ListStatus::Planning => s.include_planning,
            _ => false,
        };
        if !wanted || s.muted.contains(&m.media_ref()) {
            continue;
        }
        let mut push = |suffix: String, at, event| {
            let key = format!("{}:{}:{suffix}", m.service.key(), m.id);
            out.push(Due { key, at, event, entry: e.clone() });
        };
        if let Some(n) = m.next_airing {
            let at = n.at + i64::from(s.delay_minutes) * 60;
            if n.episode == 1 && s.premieres {
                push("premiere".into(), at, Event::Premiere);
            } else if s.episodes && e.status == ListStatus::Current {
                push(format!("ep{}", n.episode), at, Event::Episode(n.episode));
            }
            if s.finished && m.episodes == Some(n.episode) && n.episode > 1 {
                push("finished".into(), at, Event::Finished);
            }
        } else {
            if s.premieres
                && m.release == ReleaseStatus::NotYetReleased
                && let Some(at) = m.start.and_then(|d| local_at(d, hour, tz))
            {
                push("premiere".into(), at, Event::Premiere);
            }
            if s.finished
                && m.release == ReleaseStatus::Releasing
                && let Some(at) = m.end.and_then(|d| local_at(d, hour, tz))
            {
                push("finished".into(), at, Event::Finished);
            }
        }
    }
    out.sort_by_key(|d| d.at);
    out
}

/// Planned notifications that should fire right now.
pub fn due(plan: Vec<Due>, now: i64, already: impl Fn(&str) -> bool) -> Vec<Due> {
    plan.into_iter().filter(|d| d.at <= now && now - d.at <= MAX_AGE && !already(&d.key)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Airing, Media};

    fn airing(id: u64, ep: u32, at: i64, total: Option<u32>) -> ListEntry {
        ListEntry {
            status: ListStatus::Current,
            ..ListEntry::new(Media {
                id,
                episodes: total,
                next_airing: Some(Airing { episode: ep, at }),
                ..Media::default()
            })
        }
    }
    fn on() -> NotificationSettings {
        NotificationSettings { enabled: true, ..NotificationSettings::default() }
    }

    #[test]
    fn fires_once_inside_the_window() {
        let p = plan(&[airing(1, 5, 1000, Some(12))], &on(), &TimeZone::UTC);
        assert_eq!(p[0].event, Event::Episode(5));
        assert!(due(p.clone(), 999, |_| false).is_empty());
        assert_eq!(due(p.clone(), 1000, |_| false).len(), 1);
        assert!(due(p.clone(), 1000, |k| k == "anilist:1:ep5").is_empty());
        assert!(due(p, 1000 + MAX_AGE + 1, |_| false).is_empty());
    }

    #[test]
    fn long_running_without_total_still_alerts() {
        assert_eq!(plan(&[airing(1, 1150, 10, None)], &on(), &TimeZone::UTC)[0].event, Event::Episode(1150));
    }

    #[test]
    fn premiere_and_finale() {
        assert_eq!(plan(&[airing(1, 1, 10, Some(12))], &on(), &TimeZone::UTC)[0].event, Event::Premiere);
        let p = plan(&[airing(1, 12, 10, Some(12))], &on(), &TimeZone::UTC);
        assert!(p.iter().any(|d| d.event == Event::Finished));
    }

    #[test]
    fn muted_disabled_and_dropped_are_silent() {
        let e = airing(1, 2, 10, None);
        let mut s = on();
        s.muted.push(e.media.media_ref());
        assert!(plan(std::slice::from_ref(&e), &s, &TimeZone::UTC).is_empty());
        let off = NotificationSettings { enabled: false, ..on() };
        assert!(plan(std::slice::from_ref(&e), &off, &TimeZone::UTC).is_empty());
        let dropped = ListEntry { status: ListStatus::Dropped, ..e };
        assert!(plan(&[dropped], &on(), &TimeZone::UTC).is_empty());
    }

    #[test]
    fn date_only_premiere_uses_preferred_hour() {
        let e = ListEntry {
            status: ListStatus::Current,
            ..ListEntry::new(Media {
                release: ReleaseStatus::NotYetReleased,
                start: FuzzyDate::parse("2027-01-02"),
                ..Media::default()
            })
        };
        let s = NotificationSettings { date_hour: 9, ..on() };
        let at = "2027-01-02T09:00:00Z".parse::<jiff::Timestamp>().unwrap().as_second();
        assert_eq!(plan(&[e], &s, &TimeZone::UTC)[0].at, at);
    }
}
