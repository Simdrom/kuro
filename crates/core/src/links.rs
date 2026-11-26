//! Recognises MyAnimeList, AniList and `kuro://` links.

use url::Url;

use crate::model::{MediaKind, MediaRef, Service};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    Media(MediaRef),
    Search {
        kind: MediaKind,
        query: String,
    },
    Tab(String),
    /// Adds episodes or chapters (or removes them, when negative) to a title in the list.
    Step {
        media: MediaRef,
        by: i32,
    },
    /// Opens the binge session of a title in the list.
    Binge(MediaRef),
    Random(MediaKind),
    /// Reads the list from the account again.
    Refresh,
}

impl Link {
    /// Changes the list, so it only works when the user allowed it.
    pub fn changes_list(&self) -> bool {
        matches!(self, Self::Step { .. })
    }

    /// Done without bringing the window forward.
    pub fn quiet(&self) -> bool {
        matches!(self, Self::Step { .. } | Self::Refresh | Self::Binge(_))
    }
}

const MAX_LEN: usize = 2048;
const MAX_QUERY: usize = 200;
/// The most a single link may add or remove.
const MAX_STEP: i32 = 10;

fn kind(s: &str) -> Option<MediaKind> {
    match s {
        "anime" => Some(MediaKind::Anime),
        "manga" => Some(MediaKind::Manga),
        _ => None,
    }
}

fn id(s: &str) -> Option<u64> {
    s.parse().ok().filter(|n| (1..=u64::from(u32::MAX)).contains(n))
}

fn media_ref(service: Service, k: &str, n: &str) -> Option<MediaRef> {
    Some(MediaRef { service, kind: kind(k)?, id: id(n)? })
}

fn media(service: Service, k: &str, n: &str) -> Option<Link> {
    media_ref(service, k, n).map(Link::Media)
}

fn service(s: &str) -> Option<Service> {
    Service::ALL.into_iter().find(|v| v.key() == s)
}

/// Parses an untrusted link. Anything unexpected yields `None`.
pub fn parse(text: &str) -> Option<Link> {
    let text = text.trim();
    if text.len() > MAX_LEN {
        return None;
    }
    let url = Url::parse(text).ok()?;
    match url.scheme() {
        "http" | "https" => web(&url),
        "kuro" => deep(&url),
        _ => None,
    }
}

fn web(url: &Url) -> Option<Link> {
    let mut seg = url.path_segments()?;
    let (k, n) = (seg.next()?, seg.next()?);
    match url.host_str()? {
        "anilist.co" | "www.anilist.co" => media(Service::AniList, k, n),
        "myanimelist.net" | "www.myanimelist.net" => media(Service::Mal, k, n),
        _ => None,
    }
}

fn deep(url: &Url) -> Option<Link> {
    let param = |name: &str| url.query_pairs().find(|(n, _)| n == name).map(|(_, v)| v.into_owned());
    let path: Vec<&str> = url.path().split('/').filter(|s| !s.is_empty()).collect();
    match (url.host_str()?, path.as_slice()) {
        ("anilist", [k, n]) => media(Service::AniList, k, n),
        ("mal", [k, n]) => media(Service::Mal, k, n),
        ("search", [k]) => {
            let query: String = param("q")?.chars().filter(|c| !c.is_control()).take(MAX_QUERY).collect();
            (!query.trim().is_empty()).then_some(Link::Search { kind: kind(k)?, query })
        }
        // Only web links may be nested, so this cannot recurse.
        ("open", []) => web(&Url::parse(&param("url")?).ok().filter(|u| u.scheme().starts_with("http"))?),
        ("step", [s, k, n]) => {
            let by = match param("by") {
                Some(b) => b.parse().ok().filter(|b: &i32| *b != 0 && b.abs() <= MAX_STEP)?,
                None => 1,
            };
            Some(Link::Step { media: media_ref(service(s)?, k, n)?, by })
        }
        ("binge", [s, k, n]) => media_ref(service(s)?, k, n).map(Link::Binge),
        ("random", [k]) => kind(k).map(Link::Random),
        ("refresh", []) => Some(Link::Refresh),
        (
            "tab",
            [
                name @ ("anime" | "manga" | "calendar" | "seasonal" | "ranking" | "search" | "settings"
                | "kioku"),
            ],
        ) => Some(Link::Tab((*name).to_owned())),
        _ => None,
    }
}

pub fn to_link(r: MediaRef) -> String {
    format!("kuro://{}/{}/{}", r.service.key(), r.kind.key(), r.id)
}

/// A link that adds `by` episodes or chapters to `r`.
pub fn step_link(r: MediaRef, by: i32) -> String {
    let base = format!("kuro://step/{}/{}/{}", r.service.key(), r.kind.key(), r.id);
    if by == 1 { base } else { format!("{base}?by={by}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(service: Service, kind: MediaKind, id: u64) -> Option<Link> {
        Some(Link::Media(MediaRef { service, kind, id }))
    }

    #[test]
    fn accepts_known_links() {
        assert_eq!(
            parse("https://anilist.co/anime/154587/Frieren/"),
            r(Service::AniList, MediaKind::Anime, 154587)
        );
        assert_eq!(parse(" https://myanimelist.net/manga/2 "), r(Service::Mal, MediaKind::Manga, 2));
        assert_eq!(parse("kuro://mal/anime/1"), r(Service::Mal, MediaKind::Anime, 1));
        assert_eq!(
            parse("kuro://open?url=https%3A%2F%2Fanilist.co%2Fmanga%2F30002"),
            r(Service::AniList, MediaKind::Manga, 30002)
        );
        assert_eq!(
            parse("kuro://search/anime?q=one%20piece"),
            Some(Link::Search { kind: MediaKind::Anime, query: "one piece".into() })
        );
        assert_eq!(parse("kuro://tab/ranking"), Some(Link::Tab("ranking".into())));
    }

    #[test]
    fn rejects_everything_else() {
        for bad in [
            "",
            "anilist.co/anime/1",
            "https://anilist.co.evil.com/anime/1",
            "https://evil.com/?https://anilist.co/anime/1",
            "https://anilist.co@evil.com/anime/1",
            "https://anilist.co/anime/-1",
            "https://anilist.co/anime/0",
            "https://anilist.co/anime/99999999999999999999",
            "https://anilist.co/user/1",
            "file:///c:/anime/1",
            "javascript:alert(1)",
            "kuro://tab/../../x",
            "kuro://open?url=kuro%3A%2F%2Fopen",
            "kuro://search/anime?q=%00",
        ] {
            assert_eq!(parse(bad), None, "{bad}");
        }
        assert_eq!(parse(&format!("https://anilist.co/anime/1?{}", "a".repeat(4000))), None);
    }

    #[test]
    fn action_links() {
        let a = MediaRef { service: Service::AniList, kind: MediaKind::Anime, id: 9 };
        assert_eq!(parse("kuro://step/anilist/anime/9"), Some(Link::Step { media: a, by: 1 }));
        assert_eq!(parse("kuro://step/anilist/anime/9?by=-2"), Some(Link::Step { media: a, by: -2 }));
        assert_eq!(parse("kuro://binge/anilist/anime/9"), Some(Link::Binge(a)));
        assert_eq!(parse("kuro://random/manga"), Some(Link::Random(MediaKind::Manga)));
        assert_eq!(parse("kuro://refresh"), Some(Link::Refresh));
        for bad in [
            "kuro://step/anilist/anime/9?by=0",
            "kuro://step/anilist/anime/9?by=11",
            "kuro://step/anilist/anime/9?by=x",
            "kuro://step/kitsu/anime/9",
            "kuro://step/anime/9",
            "kuro://random/novel",
            "kuro://refresh/now",
        ] {
            assert_eq!(parse(bad), None, "{bad}");
        }
        for by in [1, 3, -1] {
            assert_eq!(parse(&step_link(a, by)), Some(Link::Step { media: a, by }));
        }
        assert!(Link::Step { media: a, by: 1 }.changes_list());
        assert!(!Link::Binge(a).changes_list() && Link::Binge(a).quiet());
        assert!(!Link::Media(a).quiet());
    }

    #[test]
    fn round_trips() {
        let m = MediaRef { service: Service::Mal, kind: MediaKind::Manga, id: 7 };
        assert_eq!(parse(&to_link(m)), Some(Link::Media(m)));
    }
}
