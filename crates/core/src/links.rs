//! Recognises MyAnimeList, AniList and `mango://` links.

use url::Url;

use crate::model::{MediaKind, MediaRef, Service};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    Media(MediaRef),
    Search { kind: MediaKind, query: String },
    Tab(String),
}

const MAX_LEN: usize = 2048;
const MAX_QUERY: usize = 200;

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

fn media(service: Service, k: &str, n: &str) -> Option<Link> {
    Some(Link::Media(MediaRef { service, kind: kind(k)?, id: id(n)? }))
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
        "mango" => deep(&url),
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
        ("tab", [name @ ("anime" | "manga" | "seasonal" | "ranking" | "search" | "settings")]) => {
            Some(Link::Tab((*name).to_owned()))
        }
        _ => None,
    }
}

pub fn to_link(r: MediaRef) -> String {
    format!("mango://{}/{}/{}", r.service.key(), r.kind.key(), r.id)
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
        assert_eq!(parse("mango://mal/anime/1"), r(Service::Mal, MediaKind::Anime, 1));
        assert_eq!(
            parse("mango://open?url=https%3A%2F%2Fanilist.co%2Fmanga%2F30002"),
            r(Service::AniList, MediaKind::Manga, 30002)
        );
        assert_eq!(
            parse("mango://search/anime?q=one%20piece"),
            Some(Link::Search { kind: MediaKind::Anime, query: "one piece".into() })
        );
        assert_eq!(parse("mango://tab/ranking"), Some(Link::Tab("ranking".into())));
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
            "mango://tab/../../x",
            "mango://open?url=mango%3A%2F%2Fopen",
            "mango://search/anime?q=%00",
        ] {
            assert_eq!(parse(bad), None, "{bad}");
        }
        assert_eq!(parse(&format!("https://anilist.co/anime/1?{}", "a".repeat(4000))), None);
    }

    #[test]
    fn round_trips() {
        let m = MediaRef { service: Service::Mal, kind: MediaKind::Manga, id: 7 };
        assert_eq!(parse(&to_link(m)), Some(Link::Media(m)));
    }
}
