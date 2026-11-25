//! MyAnimeList API v2 client.

use std::time::Duration;

use kuro_core::{
    Format, FuzzyDate, ListEntry, ListStatus, Media, MediaDetail, MediaKind, Relation, RelationKind,
    ReleaseStatus, Season, SeasonYear, Service, Theme, ThemeKind, UserProfile,
};
use serde_json::Value;
use url::Url;

use crate::http::{ApiError, Http, Result, Throttle, check};
use crate::{Page, Ranking, RankingQuery, SeasonQuery};

const ENDPOINT: &str = "https://api.myanimelist.net/v2";
const FIELDS: &str = "id,title,main_picture,alternative_titles,start_date,end_date,synopsis,mean,rank,popularity,\
num_list_users,num_scoring_users,nsfw,media_type,status,genres,num_episodes,start_season,broadcast,source,\
average_episode_duration,rating,studios,num_volumes,num_chapters";
const DETAIL: &str =
    ",background,related_anime,related_manga,recommendations,statistics,opening_themes,ending_themes";
const PER_PAGE: u32 = 50;

#[derive(Clone)]
pub struct Mal {
    http: Http,
    throttle: Throttle,
    endpoint: String,
}

/// How a request identifies itself.
#[derive(Clone, Copy)]
pub enum Access<'a> {
    Token(&'a str),
    Client(&'a str),
}

impl Mal {
    pub fn new(http: Http) -> Self {
        Self { http, throttle: Throttle::new(Duration::from_millis(350)), endpoint: ENDPOINT.into() }
    }

    pub fn with_endpoint(http: Http, endpoint: &str) -> Self {
        Self { http, throttle: Throttle::new(Duration::ZERO), endpoint: endpoint.into() }
    }

    pub fn throttle(&self) -> &Throttle {
        &self.throttle
    }

    async fn get(&self, access: Access<'_>, path: &str, query: &[(&str, String)]) -> Result<Value> {
        let req = self.http.client().get(format!("{}{path}", self.endpoint)).query(query);
        self.http.json(authorize(req, access), &self.throttle).await
    }

    async fn page(
        &self,
        access: Access<'_>,
        kind: MediaKind,
        path: &str,
        mut query: Vec<(&str, String)>,
        page: u32,
    ) -> Result<Page<Media>> {
        query.push(("fields", FIELDS.into()));
        query.push(("limit", PER_PAGE.to_string()));
        query.push(("offset", (page.saturating_sub(1) * PER_PAGE).to_string()));
        let v = self.get(access, path, &query).await?;
        Ok(Page {
            items: list(&v["data"])
                .iter()
                .filter_map(|d| {
                    let mut m = media(kind, &d["node"])?;
                    if let Some(r) = num(&d["ranking"]["rank"]) {
                        m.rank = Some(r);
                    }
                    Some(m)
                })
                .collect(),
            more: v["paging"]["next"].is_string(),
        })
    }

    pub async fn search(
        &self,
        access: Access<'_>,
        kind: MediaKind,
        text: &str,
        adult: bool,
        page: u32,
    ) -> Result<Page<Media>> {
        // The service rejects very short or long queries.
        let text: String = text.trim().chars().take(64).collect();
        if text.chars().count() < 3 {
            return Ok(Page::default());
        }
        let q = vec![("q", text), ("nsfw", adult.to_string())];
        self.page(access, kind, &format!("/{}", kind.key()), q, page).await
    }

    pub async fn season(&self, access: Access<'_>, q: &SeasonQuery, page: u32) -> Result<Page<Media>> {
        let path = format!("/anime/season/{}/{}", q.season.year, q.season.season.key());
        let params = vec![("sort", "anime_num_list_users".into()), ("nsfw", q.adult.to_string())];
        self.page(access, MediaKind::Anime, &path, params, page).await
    }

    pub async fn ranking(&self, access: Access<'_>, q: &RankingQuery, page: u32) -> Result<Page<Media>> {
        let kind = match (q.ranking, q.kind) {
            (Ranking::Airing, MediaKind::Anime) => "airing",
            (Ranking::Upcoming, MediaKind::Anime) => "upcoming",
            (Ranking::Popularity | Ranking::Trending, _) => "bypopularity",
            (Ranking::Favorites, _) => "favorite",
            (Ranking::Format(Format::Tv), _) => "tv",
            (Ranking::Format(Format::Movie), _) => "movie",
            (Ranking::Format(Format::Ova), _) => "ova",
            (Ranking::Format(Format::Special), _) => "special",
            (Ranking::Format(Format::Manga), _) => "manga",
            (Ranking::Format(Format::Novel), _) => "novels",
            (Ranking::Format(Format::OneShot), _) => "oneshots",
            (Ranking::Format(Format::Doujin), _) => "doujin",
            (Ranking::Format(Format::Manhwa), _) => "manhwa",
            (Ranking::Format(Format::Manhua), _) => "manhua",
            _ => "all",
        };
        let params = vec![("ranking_type", kind.to_owned()), ("nsfw", q.adult.to_string())];
        self.page(access, q.kind, &format!("/{}/ranking", q.kind.key()), params, page).await
    }

    pub async fn detail(&self, access: Access<'_>, kind: MediaKind, id: u64) -> Result<MediaDetail> {
        let fields = format!("{FIELDS}{DETAIL}");
        let v = self.get(access, &format!("/{}/{id}", kind.key()), &[("fields", fields)]).await?;
        detail(kind, &v).ok_or_else(|| ApiError::Decode("media".into()))
    }

    // Account

    pub async fn viewer(&self, token: &str) -> Result<UserProfile> {
        let v = self.get(Access::Token(token), "/users/@me", &[]).await?;
        Ok(UserProfile {
            id: v["id"].as_u64().ok_or_else(|| ApiError::Decode("user".into()))?,
            name: text(&v["name"]),
            avatar: opt_text(&v["picture"]),
        })
    }

    pub async fn library(&self, token: &str, kind: MediaKind) -> Result<Vec<ListEntry>> {
        let status = match kind {
            MediaKind::Anime => {
                "list_status{status,score,num_episodes_watched,is_rewatching,start_date,finish_date,\
                 num_times_rewatched,comments,updated_at}"
            }
            MediaKind::Manga => {
                "list_status{status,score,num_volumes_read,num_chapters_read,is_rereading,start_date,finish_date,\
                 num_times_reread,comments,updated_at}"
            }
        };
        let first = format!("{}/users/@me/{}list", self.endpoint, kind.key());
        let query =
            [("fields", format!("{FIELDS},{status}")), ("limit", "500".into()), ("nsfw", "true".into())];
        let mut req = self.http.client().get(first).query(&query);
        let mut out = Vec::new();
        for _ in 0..100 {
            let v: Value = self.http.json(authorize(req, Access::Token(token)), &self.throttle).await?;
            out.extend(list(&v["data"]).iter().filter_map(|d| entry(kind, d)));
            let Some(next) = v["paging"]["next"].as_str() else { break };
            // The token must only ever be sent to the service itself.
            req = self.http.client().get(self.same_origin(next)?);
        }
        Ok(out)
    }

    fn same_origin(&self, next: &str) -> Result<Url> {
        let url = Url::parse(next).map_err(|_| ApiError::Blocked)?;
        let base = Url::parse(&self.endpoint).map_err(|_| ApiError::Blocked)?;
        if url.origin() == base.origin() { Ok(url) } else { Err(ApiError::Blocked) }
    }

    pub async fn save(&self, token: &str, kind: MediaKind, id: u64, e: &ListEntry) -> Result<()> {
        let status = match (kind, e.status) {
            (MediaKind::Anime, ListStatus::Current) => "watching",
            (MediaKind::Manga, ListStatus::Current) => "reading",
            (_, ListStatus::Completed) => "completed",
            (_, ListStatus::Paused) => "on_hold",
            (_, ListStatus::Dropped) => "dropped",
            (MediaKind::Anime, ListStatus::Planning) => "plan_to_watch",
            (MediaKind::Manga, ListStatus::Planning) => "plan_to_read",
        };
        let date =
            |d: Option<FuzzyDate>| d.and_then(|d| d.exact()).map(|d| d.to_string()).unwrap_or_default();
        let mut form = vec![
            ("status", status.to_owned()),
            ("score", ((u32::from(e.score) + 5) / 10).to_string()),
            ("comments", e.notes.clone()),
            ("start_date", date(e.start)),
            ("finish_date", date(e.finish)),
        ];
        match kind {
            MediaKind::Anime => form.extend([
                ("num_watched_episodes", e.progress.to_string()),
                ("is_rewatching", e.repeating.to_string()),
                ("num_times_rewatched", e.repeat_count.to_string()),
            ]),
            MediaKind::Manga => form.extend([
                ("num_chapters_read", e.progress.to_string()),
                ("num_volumes_read", e.progress_volumes.to_string()),
                ("is_rereading", e.repeating.to_string()),
                ("num_times_reread", e.repeat_count.to_string()),
            ]),
        }
        let url = format!("{}/{}/{id}/my_list_status", self.endpoint, kind.key());
        let req = authorize(self.http.client().patch(url).form(&form), Access::Token(token));
        check(self.http.send(req, &self.throttle).await?).map(drop)
    }

    pub async fn delete(&self, token: &str, kind: MediaKind, id: u64) -> Result<()> {
        let url = format!("{}/{}/{id}/my_list_status", self.endpoint, kind.key());
        let req = authorize(self.http.client().delete(url), Access::Token(token));
        match check(self.http.send(req, &self.throttle).await?) {
            Err(ApiError::NotFound) => Ok(()),
            other => other.map(drop),
        }
    }
}

fn authorize(req: reqwest::RequestBuilder, access: Access<'_>) -> reqwest::RequestBuilder {
    match access {
        Access::Token(t) => req.bearer_auth(t),
        Access::Client(id) => req.header("X-MAL-CLIENT-ID", id),
    }
}

// Conversion

fn list(v: &Value) -> &[Value] {
    v.as_array().map_or(&[], Vec::as_slice)
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().trim().to_owned()
}

fn opt_text(v: &Value) -> Option<String> {
    Some(text(v)).filter(|s| !s.is_empty())
}

fn num(v: &Value) -> Option<u32> {
    v.as_u64().and_then(|n| u32::try_from(n).ok())
}

fn date(v: &Value) -> Option<FuzzyDate> {
    FuzzyDate::parse(v.as_str()?)
}

fn names(v: &Value) -> Vec<String> {
    list(v).iter().filter_map(|g| opt_text(&g["name"])).collect()
}

fn title_case(s: &str) -> String {
    s.split('_')
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn media(kind: MediaKind, v: &Value) -> Option<Media> {
    let id = v["id"].as_u64()?;
    let alt = &v["alternative_titles"];
    let season = match v["start_season"]["season"].as_str() {
        Some("winter") => Some(Season::Winter),
        Some("spring") => Some(Season::Spring),
        Some("summer") => Some(Season::Summer),
        Some("fall") => Some(Season::Fall),
        _ => None,
    };
    let day = opt_text(&v["broadcast"]["day_of_the_week"]);
    let time = opt_text(&v["broadcast"]["start_time"]);
    Some(Media {
        service: Service::Mal,
        kind,
        id,
        mal_id: Some(id),
        title: text(&v["title"]),
        title_english: opt_text(&alt["en"]),
        title_native: opt_text(&alt["ja"]),
        synonyms: list(&alt["synonyms"]).iter().filter_map(opt_text).collect(),
        cover: opt_text(&v["main_picture"]["large"]).or_else(|| opt_text(&v["main_picture"]["medium"])),
        format: match v["media_type"].as_str().unwrap_or_default() {
            "tv" => Format::Tv,
            "movie" => Format::Movie,
            "special" | "tv_special" => Format::Special,
            "ova" => Format::Ova,
            "ona" => Format::Ona,
            "music" => Format::Music,
            "manga" => Format::Manga,
            "novel" | "light_novel" => Format::Novel,
            "one_shot" => Format::OneShot,
            "doujinshi" => Format::Doujin,
            "manhwa" => Format::Manhwa,
            "manhua" => Format::Manhua,
            _ => Format::Unknown,
        },
        release: match v["status"].as_str().unwrap_or_default() {
            "finished_airing" | "finished" => ReleaseStatus::Finished,
            "currently_airing" | "currently_publishing" => ReleaseStatus::Releasing,
            "not_yet_aired" | "not_yet_published" => ReleaseStatus::NotYetReleased,
            "on_hiatus" => ReleaseStatus::Hiatus,
            "discontinued" => ReleaseStatus::Cancelled,
            _ => ReleaseStatus::Unknown,
        },
        season: season.and_then(|season| {
            Some(SeasonYear { season, year: i32::try_from(v["start_season"]["year"].as_i64()?).ok()? })
        }),
        start: date(&v["start_date"]),
        end: date(&v["end_date"]),
        episodes: num(&v["num_episodes"]).filter(|n| *n > 0),
        chapters: num(&v["num_chapters"]).filter(|n| *n > 0),
        volumes: num(&v["num_volumes"]).filter(|n| *n > 0),
        duration: num(&v["average_episode_duration"]).map(|s| s / 60).filter(|n| *n > 0),
        source: opt_text(&v["source"]).map(|s| title_case(&s)),
        genres: names(&v["genres"]),
        studios: names(&v["studios"]),
        mean_score: v["mean"].as_f64().map(|s| s as f32),
        scored_by: v["num_scoring_users"].as_u64(),
        members: v["num_list_users"].as_u64(),
        rank: num(&v["rank"]),
        popularity: num(&v["popularity"]),
        synopsis: opt_text(&v["synopsis"]),
        background: opt_text(&v["background"]),
        rating: opt_text(&v["rating"]).map(|r| r.to_uppercase().replace('_', "-")),
        broadcast: day.map(|d| match time {
            Some(t) => format!("{} {t} JST", title_case(&d)),
            None => title_case(&d),
        }),
        adult: v["nsfw"].as_str() == Some("black"),
        ..Media::default()
    })
}

fn status_of(s: &str) -> Option<ListStatus> {
    Some(match s {
        "watching" | "reading" => ListStatus::Current,
        "completed" => ListStatus::Completed,
        "on_hold" => ListStatus::Paused,
        "dropped" => ListStatus::Dropped,
        "plan_to_watch" | "plan_to_read" => ListStatus::Planning,
        _ => return None,
    })
}

fn entry(kind: MediaKind, v: &Value) -> Option<ListEntry> {
    let s = &v["list_status"];
    let anime = kind == MediaKind::Anime;
    let n = |a: &str, m: &str| num(&s[if anime { a } else { m }]).unwrap_or(0);
    let repeating = s[if anime { "is_rewatching" } else { "is_rereading" }].as_bool().unwrap_or(false);
    let status = status_of(s["status"].as_str()?)?;
    Some(ListEntry {
        media: media(kind, &v["node"])?,
        // The service keeps a rewatched title under its old status.
        status: if repeating { ListStatus::Current } else { status },
        progress: n("num_episodes_watched", "num_chapters_read"),
        progress_volumes: num(&s["num_volumes_read"]).unwrap_or(0),
        score: (num(&s["score"]).unwrap_or(0).min(10) * 10) as u8,
        start: date(&s["start_date"]),
        finish: date(&s["finish_date"]),
        repeating,
        repeat_count: n("num_times_rewatched", "num_times_reread"),
        notes: text(&s["comments"]),
        updated_at: s["updated_at"]
            .as_str()
            .and_then(|t| t.parse::<jiff::Timestamp>().ok())
            .map_or(0, |t| t.as_second()),
    })
}

fn detail(kind: MediaKind, v: &Value) -> Option<MediaDetail> {
    let mut relations = Vec::new();
    for (k, field) in [(MediaKind::Anime, "related_anime"), (MediaKind::Manga, "related_manga")] {
        for r in list(&v[field]) {
            let Some(media) = media(k, &r["node"]) else { continue };
            let kind = match r["relation_type"].as_str().unwrap_or_default() {
                "sequel" => RelationKind::Sequel,
                "prequel" => RelationKind::Prequel,
                "parent_story" | "full_story" => RelationKind::Parent,
                "side_story" => RelationKind::SideStory,
                "spin_off" => RelationKind::SpinOff,
                "alternative_version" | "alternative_setting" => RelationKind::Alternative,
                "adaptation" => RelationKind::Adaptation,
                "summary" => RelationKind::Summary,
                "character" => RelationKind::Character,
                _ => RelationKind::Other,
            };
            relations.push(Relation { kind, media });
        }
    }
    let count = |key: &str| {
        let s = &v["statistics"]["status"][key];
        s.as_u64().or_else(|| s.as_str().and_then(|t| t.parse().ok()))
    };
    let status_stats = [
        ("watching", ListStatus::Current),
        ("completed", ListStatus::Completed),
        ("on_hold", ListStatus::Paused),
        ("dropped", ListStatus::Dropped),
        ("plan_to_watch", ListStatus::Planning),
    ]
    .into_iter()
    .filter_map(|(k, s)| Some((s, count(k)?)))
    .collect();
    let mut themes = Vec::new();
    for (field, k) in [("opening_themes", ThemeKind::Opening), ("ending_themes", ThemeKind::Ending)] {
        themes.extend(list(&v[field]).iter().filter_map(|t| parse_theme(k, t["text"].as_str()?)));
    }
    Some(MediaDetail {
        media: media(kind, v)?,
        relations,
        recommendations: list(&v["recommendations"]).iter().filter_map(|r| media(kind, &r["node"])).collect(),
        status_stats,
        themes,
        ..MediaDetail::default()
    })
}

/// Parses `#1: "Title (Native)" by Artist (eps 1-12)`.
pub fn parse_theme(kind: ThemeKind, s: &str) -> Option<Theme> {
    let (open, close) = (s.find('"')?, s.rfind('"')?);
    if close <= open + 1 {
        return None;
    }
    let title = s[open + 1..close].trim().to_owned();
    let mut rest = s[close + 1..].trim();
    let mut episodes = None;
    if let Some(i) = rest.rfind("(ep") {
        let inner = rest[i + 1..].trim_end_matches(')');
        episodes = Some(inner.trim_start_matches("eps").trim_start_matches("ep").trim().to_owned());
        rest = rest[..i].trim();
    }
    let artist = rest.strip_prefix("by ").unwrap_or(rest).trim().to_owned();
    Some(Theme { kind, title, artist, episodes: episodes.filter(|e| !e.is_empty()) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{any, body_string_contains, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client(s: &MockServer) -> Mal {
        Mal::with_endpoint(Http::insecure_for_tests().unwrap(), &s.uri())
    }

    #[tokio::test]
    async fn public_requests_use_the_client_id_header() {
        let s = MockServer::start().await;
        Mock::given(path("/anime"))
            .and(header("X-MAL-CLIENT-ID", "cid"))
            .and(query_param("q", "one piece"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"node": {"id": 21, "title": "One Piece", "num_episodes": 0, "mean": 8.7,
                    "status": "currently_airing", "media_type": "tv", "nsfw": "white",
                    "broadcast": {"day_of_the_week": "sunday", "start_time": "23:15"}}}],
                "paging": {"next": "x"}
            })))
            .mount(&s)
            .await;
        let p =
            client(&s).search(Access::Client("cid"), MediaKind::Anime, "one piece", false, 1).await.unwrap();
        assert!(p.more);
        let m = &p.items[0];
        assert_eq!((m.id, m.episodes, m.release), (21, None, ReleaseStatus::Releasing));
        assert_eq!(m.broadcast.as_deref(), Some("Sunday 23:15 JST"));
    }

    #[tokio::test]
    async fn short_searches_are_not_sent() {
        let s = MockServer::start().await;
        Mock::given(any()).respond_with(ResponseTemplate::new(400)).expect(0).mount(&s).await;
        assert!(
            client(&s)
                .search(Access::Client("c"), MediaKind::Anime, "ab", false, 1)
                .await
                .unwrap()
                .items
                .is_empty()
        );
    }

    #[tokio::test]
    async fn library_follows_pages_on_the_same_origin_only() {
        let s = MockServer::start().await;
        Mock::given(path("/users/@me/animelist"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"node": {"id": 1, "title": "A"}, "list_status": {"status": "completed", "score": 9,
                    "num_episodes_watched": 12, "is_rewatching": true, "num_times_rewatched": 1,
                    "updated_at": "2026-01-01T00:00:00+00:00"}}],
                "paging": {"next": format!("{}/page2", s.uri())}
            })))
            .mount(&s)
            .await;
        Mock::given(path("/page2"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"data": [], "paging": {"next": "https://evil.test/steal"}})),
            )
            .mount(&s)
            .await;
        assert_eq!(client(&s).library("tok", MediaKind::Anime).await, Err(ApiError::Blocked));
    }

    #[test]
    fn entries_convert_rewatching_and_scores() {
        let e = entry(
            MediaKind::Anime,
            &json!({"node": {"id": 1, "title": "A"}, "list_status": {"status": "completed", "score": 9,
                "num_episodes_watched": 3, "is_rewatching": true, "updated_at": "2026-01-01T00:00:00+00:00"}}),
        )
        .unwrap();
        assert_eq!(
            (e.status, e.repeating, e.score, e.updated_at),
            (ListStatus::Current, true, 90, 1_767_225_600)
        );
    }

    #[tokio::test]
    async fn save_rounds_scores_and_uses_kind_specific_fields() {
        let s = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path("/manga/2/my_list_status"))
            .and(header("authorization", "Bearer tok"))
            .and(body_string_contains("status=reading"))
            .and(body_string_contains("score=8"))
            .and(body_string_contains("num_chapters_read=5"))
            .and(body_string_contains("start_date=2026-01-02"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&s)
            .await;
        let e = ListEntry {
            status: ListStatus::Current,
            progress: 5,
            score: 75,
            start: FuzzyDate::parse("2026-01-02"),
            ..ListEntry::default()
        };
        client(&s).save("tok", MediaKind::Manga, 2, &e).await.unwrap();
    }

    #[tokio::test]
    async fn delete_is_idempotent() {
        let s = MockServer::start().await;
        Mock::given(method("DELETE")).respond_with(ResponseTemplate::new(404)).mount(&s).await;
        assert_eq!(client(&s).delete("t", MediaKind::Anime, 1).await, Ok(()));
    }

    #[test]
    fn themes() {
        let t = parse_theme(ThemeKind::Opening, r#"#1: "Yuusha (勇者)" by YOASOBI (eps 2-16)"#).unwrap();
        assert_eq!((t.title.as_str(), t.artist.as_str()), ("Yuusha (勇者)", "YOASOBI"));
        assert_eq!(t.episodes.as_deref(), Some("2-16"));
        let t = parse_theme(ThemeKind::Ending, r#""Anytime Anywhere" by milet"#).unwrap();
        assert_eq!((t.artist.as_str(), t.episodes), ("milet", None));
        assert!(parse_theme(ThemeKind::Ending, "no quotes").is_none());
        assert!(parse_theme(ThemeKind::Ending, "\"").is_none());
    }
}
