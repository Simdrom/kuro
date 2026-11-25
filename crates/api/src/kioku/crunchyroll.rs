//! Crunchyroll's viewing history, through the API its own website uses.
//!
//! Crunchyroll has no public API, so this is an **unofficial** integration that
//! may stop working at any time and may go against its terms of use. What it
//! relies on was checked in November 2026 against open-source clients that
//! are maintained (crunchyroll-rs, the Kodi add-on, crunchyexporter-cli) and
//! with a manual test against the real service:
//!
//! - Signing in happens on Crunchyroll's own page, in a window of the app. Its
//!   `etp_rt` session cookie is exchanged once, here, at
//!   `POST /auth/v1/token` with `grant_type=etp_rt_cookie` and the public
//!   client of crunchyroll.com, which has no secret.
//! - That answer carries a refresh token, kept in the Credential Manager with
//!   the device id it was issued to. The website's client does not accept
//!   `grant_type=refresh_token`: a session is renewed by sending the refresh
//!   token back as the `etp_rt` cookie of the same device.
//! - Access tokens last five minutes and live only in memory.
//! - `GET /content/v2/{account_id}/watch-history` pages with `meta.next_page`,
//!   a relative address; long histories reject numeric pages.
//!
//! The password never reaches this program, and the cookie is not stored.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use kuro_core::kioku::{Platform, Watched, WatchedItem};
use reqwest::{RequestBuilder, Response, StatusCode};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::Mutex;
use url::Url;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use super::{BoxFuture, HistorySource, Mode, OnPage, USER_AGENT, uuid};
use crate::http::{ApiError, Http, Result, Throttle, read_limited};
use crate::secrets::{SecretKey, SecretStore, Tokens};

pub const SITE: &str = "https://www.crunchyroll.com";
/// Hosts the sign-in window may show. The API itself is only ever asked at [`SITE`].
pub const SIGN_IN_HOSTS: [&str; 2] = ["www.crunchyroll.com", "sso.crunchyroll.com"];
/// Where the sign-in window starts.
pub const SIGN_IN_URL: &str = "https://www.crunchyroll.com/login";
/// Name of the session cookie the sign-in window waits for.
pub const COOKIE: &str = "etp_rt";

/// Base64 of `noaihdevm_6iyg0a8l0q:`, the public client of crunchyroll.com
/// (an id without a secret), as crunchyroll-rs bundles it.
const WEB_CLIENT: &str = "bm9haWhkZXZtXzZpeWcwYThsMHE6";
const DEVICE_TYPE: &str = "Kuro";
const KEY: SecretKey = SecretKey::Kioku(Platform::Crunchyroll);

const PAGE_SIZE: u32 = 100;
/// At most this many pages per read: 6000 items.
const MAX_PAGES: u32 = 60;
const MAX_BODY: usize = 8 * 1024 * 1024;
const TOKEN_BODY: usize = 64 * 1024;
/// Renew this long before the access token expires.
const EARLY: i64 = 30;
const RETRIES: u32 = 3;

#[derive(Deserialize, Zeroize, ZeroizeOnDrop)]
struct Answer {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: i64,
    #[serde(default)]
    account_id: String,
}

struct Session {
    access: Zeroizing<String>,
    account: String,
    expires_at: i64,
}

#[derive(Clone)]
pub struct Crunchyroll {
    http: Http,
    base: Url,
    secrets: Arc<dyn SecretStore>,
    throttle: Throttle,
    session: Arc<Mutex<Option<Session>>>,
    /// First wait after a refused request; doubles on each retry.
    backoff: Duration,
    clock: fn() -> i64,
}

fn now() -> i64 {
    jiff::Timestamp::now().as_second()
}

impl Crunchyroll {
    pub fn new(secrets: Arc<dyn SecretStore>) -> Result<Self> {
        Self::build(Http::strict(USER_AGENT)?, SITE, secrets)
    }

    fn build(http: Http, base: &str, secrets: Arc<dyn SecretStore>) -> Result<Self> {
        let base = Url::parse(base).map_err(|e| ApiError::Service(e.to_string()))?;
        Ok(Self {
            http,
            base,
            secrets,
            throttle: Throttle::new(Duration::from_millis(400)),
            session: Arc::default(),
            backoff: Duration::from_secs(5),
            clock: now,
        })
    }

    /// Whether `url` is on the one host the API is asked at.
    fn allowed(&self, url: &Url) -> bool {
        url.scheme() == self.base.scheme()
            && url.host_str() == self.base.host_str()
            && url.port_or_known_default() == self.base.port_or_known_default()
    }

    fn url(&self, path: &str) -> Result<Url> {
        let u = self.base.join(path).map_err(|e| ApiError::Service(e.to_string()))?;
        if self.allowed(&u) { Ok(u) } else { Err(ApiError::Blocked) }
    }

    /// Sends a request, waiting longer after each "too many requests".
    async fn send(&self, make: impl Fn() -> RequestBuilder) -> Result<Response> {
        let mut wait = self.backoff;
        for attempt in 0..=RETRIES {
            match self.http.send(make(), &self.throttle).await {
                Err(ApiError::RateLimited) if attempt < RETRIES => {
                    tokio::time::sleep(wait).await;
                    wait *= 2;
                }
                other => return other,
            }
        }
        Err(ApiError::RateLimited)
    }

    /// Exchanges a session cookie (or a stored refresh token, which the
    /// website's client takes the same way) for an access token.
    async fn exchange(&self, cookie: &str, device: &str) -> Result<Answer> {
        let url = self.url("/auth/v1/token")?;
        let header = Zeroizing::new(format!("device_id={device}; {COOKIE}={cookie}"));
        let form = [
            ("grant_type", "etp_rt_cookie"),
            ("scope", "offline_access"),
            ("device_id", device),
            ("device_type", DEVICE_TYPE),
        ];
        let res = self
            .send(|| {
                self.http
                    .client()
                    .post(url.clone())
                    .header("authorization", format!("Basic {WEB_CLIENT}"))
                    .header("cookie", header.as_str())
                    .form(&form)
            })
            .await?;
        let status = res.status();
        // A rejected cookie or refresh token answers 400 or 401.
        if status == StatusCode::BAD_REQUEST || status == StatusCode::UNAUTHORIZED {
            return Err(ApiError::Unauthorized);
        }
        let res = crate::http::check(res)?;
        let body = Zeroizing::new(read_limited(res, TOKEN_BODY).await?);
        let a: Answer = serde_json::from_slice(&body).map_err(|e| ApiError::Decode(e.to_string()))?;
        let id_ok = !a.account_id.is_empty()
            && a.account_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if a.access_token.is_empty() || !id_ok {
            return Err(ApiError::Auth("incomplete answer".into()));
        }
        Ok(a)
    }

    fn remember(&self, a: &Answer, device: &str) -> Result<()> {
        let refresh = a.refresh_token.clone().filter(|r| !r.is_empty());
        let Some(refresh) = refresh else { return Err(ApiError::Auth("no refresh token".into())) };
        // Only the refresh token and its device: the access token is short-lived and stays in memory.
        let stored = Tokens {
            access: String::new(),
            refresh: Some(refresh),
            expires_at: 0,
            device: Some(device.to_owned()),
        };
        self.secrets.save(KEY, &stored)
    }

    async fn start(&self, a: &Answer) {
        *self.session.lock().await = Some(Session {
            access: Zeroizing::new(a.access_token.clone()),
            account: a.account_id.clone(),
            expires_at: (self.clock)().saturating_add(a.expires_in.max(0)),
        });
    }

    /// Signs in with the `etp_rt` cookie of the sign-in window. The cookie is
    /// used once and only the refresh token that comes back is stored.
    pub async fn connect(&self, cookie: &str) -> Result<()> {
        let device = uuid()?;
        let a = self.exchange(cookie, &device).await?;
        self.remember(&a, &device)?;
        self.start(&a).await;
        Ok(())
    }

    /// A valid access token and the account id, renewed when needed.
    async fn session(&self) -> Result<(Zeroizing<String>, String)> {
        let mut s = self.session.lock().await;
        if let Some(x) = s.as_ref().filter(|x| x.expires_at - EARLY > (self.clock)()) {
            return Ok((x.access.clone(), x.account.clone()));
        }
        let stored = self.secrets.load(KEY)?.ok_or(ApiError::Unauthorized)?;
        let (Some(refresh), Some(device)) = (stored.refresh.as_deref(), stored.device.as_deref()) else {
            return Err(ApiError::Unauthorized);
        };
        let a = self.exchange(refresh, device).await?;
        if a.refresh_token.as_deref().is_some_and(|r| !r.is_empty() && r != refresh) {
            self.remember(&a, device)?;
        }
        let x = Session {
            access: Zeroizing::new(a.access_token.clone()),
            account: a.account_id.clone(),
            expires_at: (self.clock)().saturating_add(a.expires_in.max(0)),
        };
        let out = (x.access.clone(), x.account.clone());
        *s = Some(x);
        Ok(out)
    }

    async fn page(&self, url: &Url) -> Result<Value> {
        for renewed in [false, true] {
            let (access, _) = self.session().await?;
            let res = self.send(|| self.http.client().get(url.clone()).bearer_auth(access.as_str())).await?;
            if res.status() == StatusCode::UNAUTHORIZED && !renewed {
                *self.session.lock().await = None;
                continue;
            }
            let body = read_limited(crate::http::check(res)?, MAX_BODY).await?;
            return serde_json::from_slice(&body).map_err(|e| ApiError::Decode(e.to_string()));
        }
        Err(ApiError::Unauthorized)
    }

    /// The history, newest first, until a page holds nothing new.
    pub async fn history(&self, known: &HashSet<String>) -> Result<Vec<WatchedItem>> {
        self.history_with(known, &|_| {}).await
    }

    /// [`Crunchyroll::history`], telling `on_page` after every page.
    pub async fn history_with(&self, known: &HashSet<String>, on_page: &OnPage) -> Result<Vec<WatchedItem>> {
        let (_, account) = self.session().await?;
        let mut url = self.url(&format!("/content/v2/{account}/watch-history"))?;
        url.query_pairs_mut().append_pair("page_size", &PAGE_SIZE.to_string()).append_pair("locale", "en-US");
        let mut out = Vec::new();
        for n in 1..=MAX_PAGES {
            let page = self.page(&url).await?;
            let rows = page["data"].as_array().map_or(0, Vec::len);
            let items = parse_page(&page);
            let fresh = items.iter().any(|i| !known.contains(&i.external_id));
            // Counts only: titles and ids never reach the log.
            tracing::info!(page = n, rows, kept = items.len(), "kioku: Crunchyroll history page read");
            on_page(n);
            out.extend(items);
            let next = page["meta"]["next_page"].as_str().filter(|n| !n.is_empty());
            match next {
                Some(n) if rows > 0 && fresh => url = self.url(n)?,
                _ => break,
            }
        }
        Ok(out)
    }
}

impl HistorySource for Crunchyroll {
    fn platform(&self) -> Platform {
        Platform::Crunchyroll
    }

    fn mode(&self) -> Mode {
        Mode::Account
    }

    fn connected(&self) -> bool {
        matches!(self.secrets.load(KEY), Ok(Some(_)))
    }

    fn fetch<'a>(
        &'a self,
        known: &'a HashSet<String>,
        on_page: &'a OnPage,
    ) -> BoxFuture<'a, Result<Vec<WatchedItem>>> {
        Box::pin(self.history_with(known, on_page))
    }

    fn disconnect(&self) -> Result<()> {
        if let Ok(mut s) = self.session.try_lock() {
            *s = None;
        }
        self.secrets.delete(KEY)
    }
}

/// The items of one history page that are episodes or films.
pub fn parse_page(page: &Value) -> Vec<WatchedItem> {
    page["data"].as_array().map(|rows| rows.iter().filter_map(parse_item).collect()).unwrap_or_default()
}

fn text(v: &Value) -> Option<String> {
    v.as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned)
}

fn number(v: &Value) -> Option<u32> {
    v.as_u64().and_then(|n| u32::try_from(n).ok())
}

fn parse_item(row: &Value) -> Option<WatchedItem> {
    let panel = &row["panel"];
    let (meta, movie) = match panel["type"].as_str()? {
        "episode" => (&panel["episode_metadata"], false),
        // Not seen in a real history yet; read like crunchyroll-rs reads films.
        "movie" => (&panel["movie_metadata"], true),
        // Music videos, concerts and anything else are not anime episodes.
        _ => return None,
    };
    if meta["is_clip"].as_bool() == Some(true) {
        return None;
    }
    let series_title = if movie {
        text(&meta["movie_listing_title"]).or_else(|| text(&panel["title"]))?
    } else {
        text(&meta["series_title"])?
    };
    // Every audio version has its own id: the original one counts for all.
    let original = meta["versions"]
        .as_array()
        .and_then(|v| v.iter().find(|x| x["original"].as_bool() == Some(true)))
        .and_then(|x| text(&x["guid"]));
    let external_id = original.or_else(|| text(&panel["id"])).or_else(|| text(&row["id"]))?;
    let progress = if row["fully_watched"].as_bool() == Some(true) {
        Watched::Completed
    } else {
        let played = row["playhead"].as_f64().unwrap_or(0.0);
        let length = meta["duration_ms"].as_f64().unwrap_or(0.0) / 1000.0;
        Watched::Fraction(if length > 0.0 { (played / length).clamp(0.0, 1.0) as f32 } else { 0.0 })
    };
    let watched_at =
        row["date_played"].as_str().and_then(|d| d.parse::<jiff::Timestamp>().ok()).map(|t| t.as_second());
    Some(WatchedItem {
        platform: Platform::Crunchyroll,
        series_title,
        season: if movie { None } else { number(&meta["season_number"]) },
        season_title: if movie { None } else { text(&meta["season_title"]) },
        episode: if movie { None } else { number(&meta["episode_number"]) },
        estimated: false,
        series_id: None,
        episode_title: text(&panel["title"]),
        movie,
        watched_at,
        progress,
        external_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemorySecrets;
    use serde_json::json;
    use wiremock::matchers::{body_string_contains, header, header_regex, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const PAGE1: &str = include_str!("../../tests/fixtures/kioku/crunchyroll_page1.json");
    const PAGE2: &str = include_str!("../../tests/fixtures/kioku/crunchyroll_page2.json");

    fn page(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    fn client(uri: &str, secrets: Arc<dyn SecretStore>) -> Crunchyroll {
        let mut c = Crunchyroll::build(Http::strict_for_tests(USER_AGENT).unwrap(), uri, secrets).unwrap();
        c.throttle = Throttle::new(Duration::ZERO);
        c.backoff = Duration::ZERO;
        c.clock = || 1000;
        c
    }

    fn stored(secrets: &MemorySecrets) -> Tokens {
        secrets.load(KEY).unwrap().unwrap()
    }

    fn token(access: &str, refresh: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "access_token": access, "refresh_token": refresh, "expires_in": 300,
            "account_id": "acc-1", "profile_id": "p", "token_type": "Bearer", "scope": "offline_access",
            "country": "ES"
        }))
    }

    #[test]
    fn reads_episodes_films_and_partial_plays() {
        let items = parse_page(&page(PAGE1));
        let ids: Vec<&str> = items.iter().map(|i| i.external_id.as_str()).collect();
        // The clip and the music video are dropped; the dub counts as its original.
        assert_eq!(ids, ["EP-ORIG-3", "EP-ORIG-2", "EP-B-1", "MV-1"]);
        let e = &items[0];
        assert_eq!(e.series_title, "Lanterns Over Saltmarsh");
        assert_eq!((e.season, e.episode, e.movie), (Some(2), Some(3), false));
        assert_eq!(e.season_title.as_deref(), Some("Lanterns Over Saltmarsh Season 2"));
        assert_eq!(e.progress, Watched::Completed);
        assert_eq!(e.watched_at, Some(1_791_028_800));
        assert!(matches!(items[2].progress, Watched::Fraction(f) if (f - 0.5).abs() < 0.01));
        let film = &items[3];
        assert!(film.movie && film.episode.is_none());
        assert_eq!(film.series_title, "Lanterns Over Saltmarsh: Night Star");
    }

    #[test]
    fn rows_without_a_series_are_skipped() {
        assert!(parse_page(&json!({"data": [{"panel": {}}, {"panel": {"type": "episode"}}, 3]})).is_empty());
        assert!(parse_page(&json!({})).is_empty());
    }

    #[tokio::test]
    async fn connect_stores_only_the_refresh_token_and_device() {
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/v1/token"))
            .and(header("authorization", format!("Basic {WEB_CLIENT}").as_str()))
            .and(header_regex("cookie", "^device_id=[0-9a-f-]{36}; etp_rt=COOKIE$"))
            .and(header_regex("user-agent", "^Kuro-Kioku/"))
            .and(body_string_contains("grant_type=etp_rt_cookie"))
            .respond_with(token("A1", "R1"))
            .expect(1)
            .mount(&s)
            .await;
        let secrets = Arc::new(MemorySecrets::default());
        let c = client(&s.uri(), secrets.clone());
        c.connect("COOKIE").await.unwrap();
        let t = stored(&secrets);
        assert_eq!((t.access.as_str(), t.refresh.as_deref()), ("", Some("R1")));
        assert_eq!(t.device.as_deref().map(str::len), Some(36));
        assert!(c.connected());
    }

    #[tokio::test]
    async fn a_rejected_cookie_asks_to_sign_in_again() {
        let s = MockServer::start().await;
        Mock::given(any_token())
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({"error": "invalid_grant"})))
            .mount(&s)
            .await;
        let secrets = Arc::new(MemorySecrets::default());
        assert_eq!(client(&s.uri(), secrets.clone()).connect("bad").await, Err(ApiError::Unauthorized));
        assert!(secrets.load(KEY).unwrap().is_none());
    }

    fn any_token() -> wiremock::matchers::PathExactMatcher {
        path("/auth/v1/token")
    }

    #[tokio::test]
    async fn sessions_renew_by_sending_the_refresh_token_as_the_cookie() {
        let s = MockServer::start().await;
        Mock::given(any_token())
            .and(header("cookie", "device_id=dev-1; etp_rt=R1"))
            .respond_with(token("A2", "R2"))
            .expect(1)
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/content/v2/acc-1/watch-history"))
            .and(header("authorization", "Bearer A2"))
            .and(query_param("page_size", "100"))
            .respond_with(ResponseTemplate::new(200).set_body_string(PAGE2))
            .expect(1)
            .mount(&s)
            .await;
        let secrets = Arc::new(MemorySecrets::default());
        let old = Tokens {
            access: String::new(),
            refresh: Some("R1".into()),
            expires_at: 0,
            device: Some("dev-1".into()),
        };
        secrets.save(KEY, &old).unwrap();
        let items = client(&s.uri(), secrets.clone()).history(&HashSet::new()).await.unwrap();
        assert_eq!(items.len(), 1);
        // A new refresh token replaces the old one, on the same device.
        let t = stored(&secrets);
        assert_eq!((t.refresh.as_deref(), t.device.as_deref()), (Some("R2"), Some("dev-1")));
    }

    async fn signed_in(s: &MockServer) -> Crunchyroll {
        Mock::given(any_token()).respond_with(token("A1", "R1")).mount(s).await;
        let c = client(&s.uri(), Arc::new(MemorySecrets::default()));
        c.connect("COOKIE").await.unwrap();
        c
    }

    #[tokio::test]
    async fn follows_next_page_and_stops_at_known_items() {
        let s = MockServer::start().await;
        let c = signed_in(&s).await;
        let mut first = page(PAGE1);
        first["meta"]["next_page"] =
            json!("/content/v2/acc-1/watch-history?page=2&page_size=100&locale=en-US");
        Mock::given(path("/content/v2/acc-1/watch-history"))
            .and(query_param("page", "2"))
            .respond_with(ResponseTemplate::new(200).set_body_string(PAGE2))
            .expect(1)
            .mount(&s)
            .await;
        Mock::given(path("/content/v2/acc-1/watch-history"))
            .and(query_param("locale", "en-US"))
            .respond_with(ResponseTemplate::new(200).set_body_json(first.clone()))
            .mount(&s)
            .await;
        let all = c.history(&HashSet::new()).await.unwrap();
        assert_eq!(all.len(), 5);
        // A first page with nothing new ends the read there.
        let known: HashSet<String> = parse_page(&first).into_iter().map(|i| i.external_id).collect();
        assert_eq!(c.history(&known).await.unwrap().len(), 4);
    }

    #[tokio::test]
    async fn next_pages_on_other_hosts_are_refused() {
        let s = MockServer::start().await;
        let c = signed_in(&s).await;
        let mut first = page(PAGE1);
        first["meta"]["next_page"] = json!("https://evil.example/steal");
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(first))
            .mount(&s)
            .await;
        assert_eq!(c.history(&HashSet::new()).await, Err(ApiError::Blocked));
    }

    #[tokio::test]
    async fn reading_stops_after_the_page_limit() {
        let s = MockServer::start().await;
        let c = signed_in(&s).await;
        let mut looping = page(PAGE1);
        looping["meta"]["next_page"] = json!("/content/v2/acc-1/watch-history?locale=en-US&page=again");
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(looping))
            .expect(u64::from(MAX_PAGES))
            .mount(&s)
            .await;
        // Ids repeat, so only an empty `known` keeps it going until the limit.
        assert_eq!(c.history(&HashSet::new()).await.unwrap().len(), 4 * MAX_PAGES as usize);
    }

    #[tokio::test]
    async fn waits_and_retries_when_rate_limited() {
        let s = MockServer::start().await;
        let c = signed_in(&s).await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "0"))
            .up_to_n_times(4)
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string(PAGE2))
            .mount(&s)
            .await;
        assert_eq!(c.history(&HashSet::new()).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn oversized_pages_are_rejected() {
        let s = MockServer::start().await;
        let c = signed_in(&s).await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b' '; MAX_BODY + 1]))
            .mount(&s)
            .await;
        assert_eq!(c.history(&HashSet::new()).await, Err(ApiError::TooLarge));
    }

    #[tokio::test]
    async fn nothing_stored_means_not_connected() {
        let c = client("http://127.0.0.1:9", Arc::new(MemorySecrets::default()));
        assert!(!c.connected());
        assert_eq!(c.history(&HashSet::new()).await, Err(ApiError::Unauthorized));
    }

    #[tokio::test]
    async fn disconnect_forgets_the_session() {
        let s = MockServer::start().await;
        let c = signed_in(&s).await;
        c.disconnect().unwrap();
        assert!(!c.connected());
        assert_eq!(c.history(&HashSet::new()).await, Err(ApiError::Unauthorized));
    }

    #[test]
    fn production_only_talks_to_crunchyroll() {
        let c = Crunchyroll::new(Arc::new(MemorySecrets::default())).unwrap();
        assert!(c.url("/auth/v1/token").is_ok());
        assert_eq!(c.url("https://beta-api.crunchyroll.com/x"), Err(ApiError::Blocked));
        assert_eq!(c.url("//evil.example/x"), Err(ApiError::Blocked));
        assert!(!c.http.allows_plain_http());
    }
}
