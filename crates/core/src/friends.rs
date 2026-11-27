//! Friends: other people's public lists on AniList or MyAnimeList.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::model::{ListEntry, ListStatus, Media, MediaKind, Service};

/// The most friends kept, so a damaged settings file cannot grow without end.
pub const MAX_FRIENDS: usize = 100;

/// Someone whose list Kuro shows: a user name on a service.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Friend {
    pub service: Service,
    pub name: String,
}

/// Whether both services could have a user called `name`. Only these names
/// are ever put in a request path.
pub fn valid_name(name: &str) -> bool {
    (2..=32).contains(&name.len()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

impl Friend {
    /// Reads a user name or a link to a profile or a list. A bare name
    /// belongs to `default`; a link names its own service.
    pub fn parse(text: &str, default: Service) -> Option<Self> {
        let text = text.trim();
        let bare = text.strip_prefix("https://").or_else(|| text.strip_prefix("http://")).unwrap_or(text);
        let bare = bare.strip_prefix("www.").unwrap_or(bare);
        let mut parts = bare.split(['/', '?', '#']);
        let first = parts.next().unwrap_or_default();
        let (service, name) = match first.to_ascii_lowercase().as_str() {
            "anilist.co" => match (parts.next(), parts.next()) {
                (Some("user"), Some(name)) => (Service::AniList, name),
                _ => return None,
            },
            "myanimelist.net" => match (parts.next(), parts.next()) {
                (Some("profile" | "animelist" | "mangalist"), Some(name)) => (Service::Mal, name),
                _ => return None,
            },
            _ if bare.contains('/') => return None,
            _ => (default, text.strip_prefix('@').unwrap_or(text)),
        };
        valid_name(name).then(|| Self { service, name: name.to_owned() })
    }

    /// Both services ignore case in user names.
    pub fn same(&self, other: &Self) -> bool {
        self.service == other.service && self.name.eq_ignore_ascii_case(&other.name)
    }

    pub fn web_url(&self) -> String {
        match self.service {
            Service::AniList => format!("https://anilist.co/user/{}/", self.name),
            Service::Mal => format!("https://myanimelist.net/profile/{}", self.name),
        }
    }
}

/// What a service tells about someone, with their whole public list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct FriendProfile {
    /// The name as the service writes it.
    pub name: String,
    pub avatar: Option<String>,
    /// Anime and manga.
    pub entries: Vec<ListEntry>,
}

impl FriendProfile {
    /// Titles they are watching or reading now, last updated first.
    pub fn current(&self, kind: Option<MediaKind>) -> Vec<&ListEntry> {
        let mut v: Vec<&ListEntry> = self
            .entries
            .iter()
            .filter(|e| e.status == ListStatus::Current && kind.is_none_or(|k| e.media.kind == k))
            .collect();
        v.sort_by_key(|e| std::cmp::Reverse(e.updated_at));
        v
    }
}

/// Titles on both lists, as `(mine, theirs)`, matched by the AniList or the
/// MyAnimeList id so lists from different services meet.
pub fn in_common<'a, 'b>(
    mine: impl IntoIterator<Item = &'a ListEntry>,
    theirs: &'b [ListEntry],
) -> Vec<(&'a ListEntry, &'b ListEntry)> {
    let mut by_id: HashMap<(Service, MediaKind, u64), &'a ListEntry> = HashMap::new();
    for e in mine {
        for s in Service::ALL {
            if let Some(id) = e.media.id_on(s) {
                by_id.insert((s, e.media.kind, id), e);
            }
        }
    }
    theirs
        .iter()
        .filter_map(|t| {
            Service::ALL
                .into_iter()
                .find_map(|s| by_id.get(&(s, t.media.kind, t.media.id_on(s)?)))
                .map(|m| (*m, t))
        })
        .collect()
}

/// Titles both scored before a taste match is worth saying.
pub const MIN_SHARED: usize = 3;
/// Scores from 0 to 100 at or above which a friend's title is a pick.
pub const PICK_SCORE: u8 = 80;

/// How alike two people score the titles they both scored.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Taste {
    /// Titles both scored.
    pub shared: usize,
    /// From 0 (as far apart as scores go) to 100 (the same scores).
    pub matching: u8,
    /// How far apart their scores are on average, out of 10.
    pub gap: f32,
    /// How much higher they score on average, out of 10; below zero, lower.
    pub bias: f32,
}

/// The taste match of `(mine, theirs)` pairs, from [`in_common`]; `None`
/// until [`MIN_SHARED`] titles were scored by both.
pub fn taste(both: &[(&ListEntry, &ListEntry)]) -> Option<Taste> {
    let scored: Vec<(f32, f32)> = both
        .iter()
        .filter(|(m, t)| m.score > 0 && t.score > 0)
        .map(|(m, t)| (f32::from(m.score), f32::from(t.score)))
        .collect();
    if scored.len() < MIN_SHARED {
        return None;
    }
    let n = scored.len() as f32;
    let gap = scored.iter().map(|(m, t)| (m - t).abs()).sum::<f32>() / n;
    let bias = scored.iter().map(|(m, t)| t - m).sum::<f32>() / n;
    Some(Taste {
        shared: scored.len(),
        matching: (100.0 - gap).round().clamp(0.0, 100.0) as u8,
        gap: gap / 10.0,
        bias: bias / 10.0,
    })
}

/// A title friends scored well that is not in the user's list.
#[derive(Debug, Clone, PartialEq)]
pub struct Pick {
    pub media: Media,
    /// Who scored it, with their score from 0 to 100, highest first.
    pub by: Vec<(String, u8)>,
}

impl Pick {
    /// The friends' average score, from 0 to 100.
    pub fn score(&self) -> u8 {
        let sum: u32 = self.by.iter().map(|(_, s)| u32::from(*s)).sum();
        u8::try_from(sum / self.by.len().max(1) as u32).unwrap_or(100)
    }
}

/// What `friends` (a name and a list each) scored [`PICK_SCORE`] or more,
/// finished or are still on, and is not in `mine` under any service: the
/// titles more of them liked first, then the best scored.
pub fn picks<'a>(
    mine: impl IntoIterator<Item = &'a ListEntry>,
    friends: &[(&str, &[ListEntry])],
) -> Vec<Pick> {
    let mut have: std::collections::HashSet<(Service, MediaKind, u64)> = std::collections::HashSet::new();
    for e in mine {
        for s in Service::ALL {
            if let Some(id) = e.media.id_on(s) {
                have.insert((s, e.media.kind, id));
            }
        }
    }
    let known = |m: &Media| {
        Service::ALL.into_iter().any(|s| m.id_on(s).is_some_and(|id| have.contains(&(s, m.kind, id))))
    };
    // One key per title, whichever service the friend is on.
    let key = |m: &Media| {
        let s = if m.id_on(Service::Mal).is_some() { Service::Mal } else { Service::AniList };
        (s, m.kind, m.id_on(s).unwrap_or(m.id))
    };
    let mut found: HashMap<(Service, MediaKind, u64), Pick> = HashMap::new();
    let mut order: Vec<(Service, MediaKind, u64)> = Vec::new();
    for (name, list) in friends {
        for e in list.iter() {
            let liked =
                e.score >= PICK_SCORE && matches!(e.status, ListStatus::Completed | ListStatus::Current);
            if !liked || known(&e.media) {
                continue;
            }
            let k = key(&e.media);
            let p = found.entry(k).or_insert_with(|| {
                order.push(k);
                Pick { media: e.media.clone(), by: Vec::new() }
            });
            if !p.by.iter().any(|(n, _)| n == name) {
                p.by.push(((*name).to_owned(), e.score));
            }
        }
    }
    let mut out: Vec<Pick> = order.into_iter().filter_map(|k| found.remove(&k)).collect();
    for p in &mut out {
        p.by.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    }
    out.sort_by(|a, b| {
        b.by.len()
            .cmp(&a.by.len())
            .then(b.score().cmp(&a.score()))
            .then_with(|| {
                b.media.mean_score.partial_cmp(&a.media.mean_score).unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| a.media.title.cmp(&b.media.title))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Media;

    fn f(service: Service, name: &str) -> Option<Friend> {
        Some(Friend { service, name: name.into() })
    }

    #[test]
    fn reads_names_and_profile_links() {
        let p = |t| Friend::parse(t, Service::AniList);
        assert_eq!(p("  Kuro_fan "), f(Service::AniList, "Kuro_fan"));
        assert_eq!(p("@ink-reader"), f(Service::AniList, "ink-reader"));
        assert_eq!(Friend::parse("ink", Service::Mal), f(Service::Mal, "ink"));
        assert_eq!(p("https://anilist.co/user/Someone/animelist"), f(Service::AniList, "Someone"));
        assert_eq!(p("anilist.co/user/Someone/"), f(Service::AniList, "Someone"));
        assert_eq!(p("https://myanimelist.net/profile/Reader"), f(Service::Mal, "Reader"));
        assert_eq!(p("https://myanimelist.net/animelist/Reader?status=1"), f(Service::Mal, "Reader"));
        assert_eq!(p("http://www.myanimelist.net/mangalist/Reader"), f(Service::Mal, "Reader"));
    }

    #[test]
    fn refuses_what_could_not_be_a_user() {
        let p = |t| Friend::parse(t, Service::Mal);
        for bad in ["", "a", "two words", "../users/@me", "name?x=1", "https://example.com/user/x"] {
            assert_eq!(p(bad), None, "{bad}");
        }
        assert_eq!(p("https://anilist.co/anime/1"), None);
        assert_eq!(p("https://myanimelist.net/profile/../x"), None);
        assert_eq!(p(&"x".repeat(33)), None);
    }

    #[test]
    fn names_ignore_case() {
        let a = Friend { service: Service::Mal, name: "Reader".into() };
        assert!(a.same(&Friend { service: Service::Mal, name: "reader".into() }));
        assert!(!a.same(&Friend { service: Service::AniList, name: "Reader".into() }));
    }

    fn entry(service: Service, id: u64, anilist: Option<u64>, mal: Option<u64>) -> ListEntry {
        let mut e =
            ListEntry::new(Media { service, id, anilist_id: anilist, mal_id: mal, ..Media::default() });
        e.status = ListStatus::Current;
        e
    }

    #[test]
    fn titles_in_common_meet_across_services() {
        let mine =
            [entry(Service::AniList, 10, Some(10), Some(110)), entry(Service::AniList, 20, Some(20), None)];
        let theirs = [
            entry(Service::Mal, 110, None, Some(110)),
            entry(Service::Mal, 120, None, Some(120)),
            entry(Service::AniList, 20, Some(20), None),
        ];
        let both = in_common(&mine, &theirs);
        let ids: Vec<_> = both.iter().map(|(m, t)| (m.media.id, t.media.id)).collect();
        assert_eq!(ids, [(10, 110), (20, 20)]);
    }

    fn scored(service: Service, id: u64, mal: Option<u64>, status: ListStatus, score: u8) -> ListEntry {
        let mut e = entry(service, id, (service == Service::AniList).then_some(id), mal);
        (e.status, e.score) = (status, score);
        e
    }

    #[test]
    fn taste_needs_enough_titles_scored_by_both() {
        let a = |s| scored(Service::AniList, 1, None, ListStatus::Completed, s);
        let (m1, m2, m3, t1, t2, t3) = (a(80), a(60), a(0), a(90), a(40), a(70));
        assert_eq!(taste(&[(&m1, &t1), (&m2, &t2), (&m3, &t3)]), None, "an unscored title does not count");
        let m3 = a(100);
        let t = taste(&[(&m1, &t1), (&m2, &t2), (&m3, &t3)]).unwrap();
        // Gaps of 10, 20 and 30: 20 on average.
        assert_eq!((t.shared, t.matching), (3, 80));
        assert!((t.gap - 2.0).abs() < 1e-4);
        assert!((t.bias + 4.0 / 3.0).abs() < 1e-4, "they score lower");
    }

    #[test]
    fn picks_are_what_friends_loved_and_the_user_lacks() {
        use ListStatus as L;
        let mine = [scored(Service::AniList, 1, Some(101), L::Planning, 0)];
        let ana = [
            scored(Service::AniList, 1, Some(101), L::Completed, 100), // already mine
            scored(Service::AniList, 2, Some(102), L::Completed, 90),
            scored(Service::AniList, 3, Some(103), L::Dropped, 95), // dropped
            scored(Service::AniList, 4, Some(104), L::Planning, 95), // not seen
            scored(Service::AniList, 5, Some(105), L::Current, 70), // too low
            scored(Service::AniList, 6, None, L::Current, 85),
        ];
        let ben = [
            scored(Service::Mal, 101, Some(101), L::Completed, 100), // mine, on the other service
            scored(Service::Mal, 102, Some(102), L::Completed, 80),
        ];
        let p = picks(&mine, &[("Ana", &ana), ("Ben", &ben)]);
        let ids: Vec<_> = p.iter().map(|p| p.media.id).collect();
        assert_eq!(ids, [2, 6], "liked by two first");
        assert_eq!(p[0].by, [("Ana".to_owned(), 90), ("Ben".to_owned(), 80)]);
        assert_eq!(p[0].score(), 85);
    }

    #[test]
    fn current_titles_come_last_updated_first() {
        let mut a = entry(Service::AniList, 1, Some(1), None);
        a.updated_at = 5;
        let mut b = entry(Service::AniList, 2, Some(2), None);
        b.updated_at = 9;
        let mut c = entry(Service::AniList, 3, Some(3), None);
        c.status = ListStatus::Completed;
        let p = FriendProfile { entries: vec![a, b, c], ..FriendProfile::default() };
        let ids: Vec<_> = p.current(None).iter().map(|e| e.media.id).collect();
        assert_eq!(ids, [2, 1]);
        assert!(p.current(Some(MediaKind::Manga)).is_empty());
    }
}
