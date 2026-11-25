//! Openings and endings, and where to play them.

use std::time::Duration;

use kuro_core::{Theme, ThemeKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use crate::http::{ApiError, Http, Result, Throttle};
use crate::mal::parse_theme;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Track {
    pub title: String,
    pub artist: String,
    pub artwork: Option<String>,
    pub url: String,
}

#[derive(Clone)]
pub struct Music {
    http: Http,
    itunes: String,
    jikan: String,
    animethemes: String,
    slow: Throttle,
    themes: Throttle,
}

impl Music {
    pub fn new(http: Http) -> Self {
        Self::with_endpoints(
            http,
            "https://itunes.apple.com",
            "https://api.jikan.moe/v4",
            "https://api.animethemes.moe",
            Duration::from_secs(3),
        )
    }

    pub fn with_endpoints(http: Http, itunes: &str, jikan: &str, animethemes: &str, gap: Duration) -> Self {
        Self {
            http,
            itunes: itunes.into(),
            jikan: jikan.into(),
            animethemes: animethemes.into(),
            // Apple allows about 20 searches per minute.
            slow: Throttle::new(gap),
            themes: Throttle::new(gap / 3),
        }
    }

    /// Themes by MyAnimeList id, from whichever source answers.
    pub async fn themes(&self, mal_id: Option<u64>, anilist_id: Option<u64>) -> Result<Vec<Theme>> {
        let mut last = ApiError::NotFound;
        if let Some(id) = anilist_id {
            match self.animethemes_themes(id).await {
                Ok(t) if !t.is_empty() => return Ok(t),
                Ok(_) => {}
                Err(e) => last = e,
            }
        }
        if let Some(id) = mal_id {
            return self.jikan_themes(id).await;
        }
        if last == ApiError::NotFound { Ok(Vec::new()) } else { Err(last) }
    }

    async fn jikan_themes(&self, id: u64) -> Result<Vec<Theme>> {
        let url = format!("{}/anime/{id}/themes", self.jikan);
        let v: Value = match self.http.json(self.http.client().get(url), &self.themes).await {
            Err(ApiError::NotFound) => return Ok(Vec::new()),
            other => other?,
        };
        let mut out = Vec::new();
        for (key, kind) in [("openings", ThemeKind::Opening), ("endings", ThemeKind::Ending)] {
            for s in v["data"][key].as_array().into_iter().flatten().filter_map(Value::as_str) {
                out.extend(parse_theme(kind, s));
            }
        }
        Ok(out)
    }

    async fn animethemes_themes(&self, anilist_id: u64) -> Result<Vec<Theme>> {
        let query = [
            ("filter[has]", "resources".to_owned()),
            ("filter[site]", "AniList".into()),
            ("filter[external_id]", anilist_id.to_string()),
            ("include", "animethemes.song.artists".into()),
        ];
        let req = self.http.client().get(format!("{}/anime", self.animethemes)).query(&query);
        let v: Value = self.http.json(req, &self.themes).await?;
        let mut out = Vec::new();
        for t in v["anime"][0]["animethemes"].as_array().into_iter().flatten() {
            let Some(title) = t["song"]["title"].as_str().filter(|s| !s.is_empty()) else { continue };
            let artists: Vec<&str> = t["song"]["artists"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|a| a["name"].as_str())
                .collect();
            let kind = if t["type"].as_str() == Some("ED") { ThemeKind::Ending } else { ThemeKind::Opening };
            let theme = Theme { kind, title: title.to_owned(), artist: artists.join(", "), episodes: None };
            if !out.contains(&theme) {
                out.push(theme);
            }
        }
        out.sort_by_key(|t| t.kind == ThemeKind::Ending);
        Ok(out)
    }

    /// Best match in the Apple Music catalogue.
    pub async fn apple(&self, theme: &Theme) -> Result<Option<Track>> {
        let query = [
            ("term", search_term(theme)),
            ("media", "music".into()),
            ("entity", "song".into()),
            ("limit", "10".into()),
        ];
        let req = self.http.client().get(format!("{}/search", self.itunes)).query(&query);
        let v: Value = self.http.json(req, &self.slow).await?;
        let found = v["results"].as_array().into_iter().flatten().filter_map(|r| {
            let url = r["trackViewUrl"].as_str().filter(|u| is_apple(u))?;
            Some(Track {
                title: r["trackName"].as_str()?.to_owned(),
                artist: r["artistName"].as_str().unwrap_or_default().to_owned(),
                artwork: r["artworkUrl100"].as_str().map(str::to_owned),
                url: url.to_owned(),
            })
        });
        Ok(found.max_by_key(|t| score(theme, t)).filter(|t| score(theme, t) > 0))
    }
}

fn is_apple(url: &str) -> bool {
    Url::parse(url).is_ok_and(|u| {
        u.scheme() == "https" && matches!(u.host_str(), Some("music.apple.com" | "itunes.apple.com"))
    })
}

/// `Yuusha (勇者)` → `Yuusha`.
fn base_title(t: &str) -> &str {
    t.split(" (").next().unwrap_or(t).trim()
}

pub fn search_term(t: &Theme) -> String {
    let artist = t.artist.split([',', ';']).next().unwrap_or_default();
    let artist = artist.split(" feat").next().unwrap_or_default();
    format!("{} {}", base_title(&t.title), base_title(artist)).trim().to_owned()
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

fn score(t: &Theme, track: &Track) -> u32 {
    let has = |a: &str, b: &str| {
        let (a, b) = (norm(a), norm(b));
        !a.is_empty() && !b.is_empty() && (a.contains(&b) || b.contains(&a))
    };
    let artist = t.artist.split([',', ';']).any(|a| has(base_title(a), &track.artist));
    let title = has(base_title(&t.title), &track.title)
        || t.title.split(['(', ')']).skip(1).any(|n| !n.trim().is_empty() && has(n, &track.title));
    // A matching title alone is common for unrelated songs.
    match (title, artist) {
        (true, true) => 2,
        (false, true) => 1,
        _ => 0,
    }
}

/// Opens the desktop app or web player; needs no account or key.
pub fn spotify_search_url(t: &Theme) -> Option<String> {
    let mut url = Url::parse("https://open.spotify.com/search").ok()?;
    url.path_segments_mut().ok()?.push(&search_term(t));
    Some(url.into())
}

pub fn apple_search_url(t: &Theme) -> Option<String> {
    Url::parse_with_params("https://music.apple.com/search", [("term", search_term(t))]).ok().map(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::path;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn theme(title: &str, artist: &str) -> Theme {
        Theme { kind: ThemeKind::Opening, title: title.into(), artist: artist.into(), episodes: None }
    }
    fn music(s: &MockServer) -> Music {
        let u = s.uri();
        Music::with_endpoints(Http::insecure_for_tests().unwrap(), &u, &u, &u, Duration::ZERO)
    }

    #[tokio::test]
    async fn falls_back_to_the_second_source() {
        let s = MockServer::start().await;
        Mock::given(path("/anime")).respond_with(ResponseTemplate::new(503)).mount(&s).await;
        Mock::given(path("/anime/5/themes"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {
                "openings": ["1: \"A\" by B (eps 1-2)"], "endings": ["\"C\" by D"]}})))
            .mount(&s)
            .await;
        let t = music(&s).themes(Some(5), Some(6)).await.unwrap();
        assert_eq!(t.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(), ["A", "C"]);
    }

    #[tokio::test]
    async fn picks_the_right_artist_and_rejects_foreign_urls() {
        let s = MockServer::start().await;
        Mock::given(path("/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"results": [
                {"trackName": "Yuusha", "artistName": "Cover Band", "trackViewUrl": "https://music.apple.com/1"},
                {"trackName": "Yuusha", "artistName": "YOASOBI", "trackViewUrl": "https://evil.test/2"},
                {"trackName": "Yuusha", "artistName": "YOASOBI", "trackViewUrl": "https://music.apple.com/3"}]})))
            .mount(&s)
            .await;
        let t = music(&s).apple(&theme("Yuusha (勇者)", "YOASOBI")).await.unwrap().unwrap();
        assert_eq!(t.url, "https://music.apple.com/3");
        assert_eq!(music(&s).apple(&theme("Other", "Nobody")).await.unwrap(), None);
    }

    #[test]
    fn search_urls_are_escaped() {
        let u = spotify_search_url(&theme("A/B?&", "C#")).unwrap();
        assert_eq!(u, "https://open.spotify.com/search/A%2FB%3F&%20C%23");
        assert!(apple_search_url(&theme("a&b", "c")).unwrap().contains("term=a%26b+c"));
    }
}
