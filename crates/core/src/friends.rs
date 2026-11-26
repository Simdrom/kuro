//! Friends: other people's public lists on AniList or MyAnimeList.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::model::{ListEntry, ListStatus, MediaKind, Service};

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
