//! Google Calendar: a calendar of Kuro's own with the next episodes.
//!
//! Desktop apps sign in with the authorization code flow, PKCE (S256) and a
//! loopback redirect on any free port. Google gives desktop clients a secret
//! it documents as not confidential, and still wants it for the code
//! exchange. The `calendar.app.created` scope only reaches calendars Kuro
//! created itself, never the user's other calendars.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use kuro_core::calendar::Episode;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use url::Url;

use crate::http::{ApiError, Http, Result, Throttle, check, decode};
use crate::oauth::{self, CANCELLED, DONE, LOGIN_TIMEOUT, READ_TIMEOUT, TokenResponse, page, same};
use crate::secrets::{SecretKey, SecretStore, Tokens};

const CALENDAR_SCOPE: &str = "https://www.googleapis.com/auth/calendar.app.created";
const SCOPES: &str = "openid email https://www.googleapis.com/auth/calendar.app.created";
/// The sign-in error when the calendar box was left unticked.
pub const NO_CALENDAR: &str = "no calendar access";
/// Refresh this long before a token expires.
const EARLY: i64 = 300;

#[derive(Debug, Clone)]
struct Urls {
    authorize: String,
    token: String,
    userinfo: String,
    api: String,
}

impl Default for Urls {
    fn default() -> Self {
        Self {
            authorize: "https://accounts.google.com/o/oauth2/v2/auth".into(),
            token: "https://oauth2.googleapis.com/token".into(),
            userinfo: "https://openidconnect.googleapis.com/v1/userinfo".into(),
            api: "https://www.googleapis.com/calendar/v3".into(),
        }
    }
}

#[derive(Clone)]
pub struct Google {
    http: Http,
    throttle: Throttle,
    secrets: Arc<dyn SecretStore>,
    /// Baked in at build time; empty when this build has no Google client.
    client_id: String,
    client_secret: String,
    urls: Urls,
    /// Whether a token is stored, so the interface can ask on every frame.
    signed: Arc<Mutex<Option<bool>>>,
    /// A refresh token is used by one refresh at a time.
    refreshing: Arc<tokio::sync::Mutex<()>>,
    clock: fn() -> i64,
}

fn now() -> i64 {
    jiff::Timestamp::now().as_second()
}

impl Google {
    pub fn new(http: Http, secrets: Arc<dyn SecretStore>) -> Self {
        let id = option_env!("KURO_GOOGLE_CLIENT_ID").unwrap_or_default();
        let secret = option_env!("KURO_GOOGLE_CLIENT_SECRET").unwrap_or_default();
        Self::with(http, secrets, id, secret, Urls::default())
    }

    fn with(http: Http, secrets: Arc<dyn SecretStore>, id: &str, secret: &str, urls: Urls) -> Self {
        Self {
            http,
            throttle: Throttle::new(Duration::from_millis(100)),
            secrets,
            client_id: id.trim().to_owned(),
            client_secret: secret.trim().to_owned(),
            urls,
            signed: Arc::default(),
            refreshing: Arc::default(),
            clock: now,
        }
    }

    /// Whether this build can sign in to Google at all.
    pub fn is_configured(&self) -> bool {
        !self.client_id.is_empty() && !self.client_secret.is_empty()
    }

    pub fn is_signed_in(&self) -> bool {
        let mut signed = self.signed.lock().unwrap_or_else(|e| e.into_inner());
        *signed.get_or_insert_with(|| matches!(self.secrets.load(SecretKey::Google), Ok(Some(_))))
    }

    fn set_signed(&self, v: bool) {
        *self.signed.lock().unwrap_or_else(|e| e.into_inner()) = Some(v);
    }

    pub async fn begin_login(&self) -> Result<Login> {
        if !self.is_configured() {
            return Err(ApiError::NotConfigured);
        }
        Login::start(&self.client_id, &self.urls.authorize).await
    }

    /// Waits for the browser, keeps the tokens and returns the account's address.
    pub async fn finish_login(&self, login: Login) -> Result<String> {
        let code = tokio::time::timeout(LOGIN_TIMEOUT, login.wait())
            .await
            .map_err(|_| ApiError::Auth("timed out".into()))??;
        let form = [
            ("client_id", self.client_id.as_str()),
            ("client_secret", &self.client_secret),
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("code_verifier", &login.verifier),
            ("redirect_uri", &login.redirect),
        ];
        let res =
            self.http.send(self.http.client().post(&self.urls.token).form(&form), &self.throttle).await?;
        let body = crate::http::read_limited(check(res)?, 64 * 1024).await?;
        let bad = |e: serde_json::Error| ApiError::Decode(e.to_string());
        let tokens =
            serde_json::from_slice::<TokenResponse>(&body).map_err(bad)?.into_tokens((self.clock)())?;
        #[derive(Deserialize)]
        struct Granted {
            #[serde(default)]
            scope: String,
        }
        let granted = serde_json::from_slice::<Granted>(&body).map_err(bad)?.scope;
        // Google lets people untick the calendar on the consent page.
        if !granted.split(' ').any(|s| s == CALENDAR_SCOPE) {
            return Err(ApiError::Auth(NO_CALENDAR.into()));
        }
        if tokens.refresh.is_none() {
            // Without it the calendar would stop updating within the hour.
            return Err(ApiError::Auth("no refresh token".into()));
        }
        let email = self.email(&tokens.access).await?;
        self.secrets.save(SecretKey::Google, &tokens)?;
        self.set_signed(true);
        Ok(email)
    }

    async fn email(&self, access: &str) -> Result<String> {
        #[derive(Deserialize)]
        struct Info {
            #[serde(default)]
            email: String,
        }
        let req = self.http.client().get(&self.urls.userinfo).bearer_auth(access);
        Ok(self.json::<Info>(req).await?.email)
    }

    /// Forgets the account here. Its calendar stays in Google Calendar.
    pub async fn sign_out(&self) -> Result<()> {
        if let Ok(Some(t)) = self.secrets.load(SecretKey::Google) {
            // Best effort: the grant is also dropped on Google's side.
            let token = t.refresh.clone().unwrap_or_else(|| t.access.clone());
            let revoke = self.urls.token.replace("/token", "/revoke");
            let _ = self
                .http
                .send(self.http.client().post(revoke).form(&[("token", token)]), &self.throttle)
                .await;
        }
        self.secrets.delete(SecretKey::Google)?;
        self.set_signed(false);
        Ok(())
    }

    async fn token(&self) -> Result<String> {
        let load = || self.secrets.load(SecretKey::Google)?.ok_or(ApiError::Unauthorized);
        let fresh = |t: &Tokens| t.expires_at - EARLY > (self.clock)();
        let tokens = load()?;
        if fresh(&tokens) {
            return Ok(tokens.access.clone());
        }
        let _guard = self.refreshing.lock().await;
        let tokens = load()?;
        if fresh(&tokens) {
            return Ok(tokens.access.clone());
        }
        let Some(refresh) = tokens.refresh.clone() else { return Err(ApiError::Unauthorized) };
        let form = [
            ("client_id", self.client_id.as_str()),
            ("client_secret", &self.client_secret),
            ("grant_type", "refresh_token"),
            ("refresh_token", &refresh),
        ];
        let res =
            self.http.send(self.http.client().post(&self.urls.token).form(&form), &self.throttle).await?;
        if res.status().is_client_error() {
            // Revoked, or expired as it does for apps Google has not reviewed.
            return Err(ApiError::Unauthorized);
        }
        let mut t = decode::<TokenResponse>(check(res)?).await?.into_tokens((self.clock)())?;
        t.refresh.get_or_insert(refresh);
        self.secrets.save(SecretKey::Google, &t)?;
        Ok(t.access.clone())
    }

    /// Makes Kuro's calendar show exactly `episodes` from now on: missing
    /// ones are added, changed ones updated and dropped ones removed. Past
    /// events stay as a record. Returns the calendar's id, which is new when
    /// `calendar` is empty or was deleted.
    pub async fn sync(&self, calendar: &str, name: &str, episodes: &[Episode]) -> Result<String> {
        let token = self.token().await?;
        let calendar = match calendar {
            "" => self.create_calendar(&token, name).await?,
            id => match self.json(self.get(&token, &format!("/calendars/{}", enc(id)))).await {
                Ok(serde_json::Value::Object(_)) => id.to_owned(),
                Err(ApiError::NotFound) => self.create_calendar(&token, name).await?,
                Ok(_) => return Err(ApiError::Decode("calendar".into())),
                Err(e) => return Err(e),
            },
        };
        let events = format!("/calendars/{}/events", enc(&calendar));
        let existing = self.upcoming(&token, &events).await?;
        let mut wanted = HashSet::new();
        for e in episodes {
            let id = event_id(&e.uid);
            if !wanted.insert(id.clone()) {
                continue;
            }
            let body = event_body(e, &id);
            match existing.get(&id) {
                Some(r) if r.shows(e) => {}
                Some(_) => self.put(&token, &format!("{events}/{id}"), &body).await?,
                None => {
                    let req = self.authed(self.http.client().post(self.url(&events)), &token).json(&body);
                    match self.send(req).await {
                        // Deleted earlier: Google keeps the id, so it is brought back.
                        Err(ApiError::Status(409)) => {
                            self.put(&token, &format!("{events}/{id}"), &body).await?
                        }
                        r => r?,
                    }
                }
            }
        }
        self.remove(&token, &events, existing.keys().filter(|id| !wanted.contains(*id))).await?;
        Ok(calendar)
    }

    /// Makes a calendar of Kuro's own called `name` and returns its id.
    pub async fn create(&self, name: &str) -> Result<String> {
        let token = self.token().await?;
        self.create_calendar(&token, name).await
    }

    /// Removes the events that have not ended yet from `calendar`, when
    /// Kuro moves to another one. A calendar already deleted is fine.
    pub async fn clear(&self, calendar: &str) -> Result<()> {
        let token = self.token().await?;
        let events = format!("/calendars/{}/events", enc(calendar));
        match self.upcoming(&token, &events).await {
            Ok(existing) => self.remove(&token, &events, existing.keys()).await,
            Err(ApiError::NotFound) => Ok(()),
            Err(e) => Err(e),
        }
    }

    async fn remove(&self, token: &str, events: &str, ids: impl Iterator<Item = &String>) -> Result<()> {
        for id in ids {
            let req = self.authed(self.http.client().delete(self.url(&format!("{events}/{id}"))), token);
            match self.send(req).await {
                Ok(()) | Err(ApiError::NotFound | ApiError::Status(410)) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    async fn create_calendar(&self, token: &str, name: &str) -> Result<String> {
        #[derive(Deserialize)]
        struct Created {
            id: String,
        }
        let req = self.authed(self.http.client().post(self.url("/calendars")), token).json(&json!({
            "summary": name,
            "description": "Upcoming episodes from Kuro. Kuro keeps this calendar up to date.",
        }));
        let c: Created = self.json(req).await?;
        if c.id.is_empty() {
            return Err(ApiError::Decode("calendar id".into()));
        }
        Ok(c.id)
    }

    /// Events that have not ended yet, by id.
    async fn upcoming(&self, token: &str, events: &str) -> Result<HashMap<String, Remote>> {
        #[derive(Deserialize)]
        struct List {
            #[serde(default)]
            items: Vec<Remote>,
            #[serde(rename = "nextPageToken")]
            next: Option<String>,
        }
        let since = jiff::Timestamp::from_second((self.clock)()).unwrap_or_default().to_string();
        let mut out = HashMap::new();
        let mut page: Option<String> = None;
        // Bounded, in case the service keeps handing out pages.
        for _ in 0..20 {
            let mut req =
                self.get(token, events).query(&[("timeMin", since.as_str()), ("maxResults", "2500")]);
            if let Some(p) = &page {
                req = req.query(&[("pageToken", p)]);
            }
            let list: List = self.json(req).await?;
            out.extend(list.items.into_iter().map(|r| (r.id.clone(), r)));
            match list.next {
                Some(p) if !p.is_empty() => page = Some(p),
                _ => break,
            }
        }
        Ok(out)
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.urls.api)
    }

    fn authed(&self, req: reqwest::RequestBuilder, token: &str) -> reqwest::RequestBuilder {
        req.bearer_auth(token)
    }

    fn get(&self, token: &str, path: &str) -> reqwest::RequestBuilder {
        self.authed(self.http.client().get(self.url(path)), token)
    }

    async fn put(&self, token: &str, path: &str, body: &serde_json::Value) -> Result<()> {
        self.send(self.authed(self.http.client().put(self.url(path)), token).json(body)).await
    }

    async fn json<T: serde::de::DeserializeOwned>(&self, req: reqwest::RequestBuilder) -> Result<T> {
        decode(answer(self.http.send(req, &self.throttle).await?).await?).await
    }

    async fn send(&self, req: reqwest::RequestBuilder) -> Result<()> {
        answer(self.http.send(req, &self.throttle).await?).await.map(drop)
    }
}

/// Like [`check`], but a refusal carries Google's own reason, such as the
/// Calendar API not being enabled for the client's project.
async fn answer(res: reqwest::Response) -> Result<reqwest::Response> {
    #[derive(Deserialize)]
    struct Body {
        error: Detail,
    }
    #[derive(Deserialize)]
    struct Detail {
        #[serde(default)]
        message: String,
    }
    let status = res.status().as_u16();
    if !matches!(status, 400 | 403) {
        return check(res);
    }
    let body = crate::http::read_limited(res, 64 * 1024).await.unwrap_or_default();
    match serde_json::from_slice::<Body>(&body) {
        Ok(b) if !b.error.message.is_empty() => {
            Err(ApiError::Service(format!("{} ({status})", b.error.message)))
        }
        _ => Err(ApiError::Status(status)),
    }
}

/// Percent-encodes a path segment: calendar ids contain `@` and `#`.
fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>().replace('+', "%20")
}

/// Google wants 5 to 1024 characters from `a-v` and `0-9`; hex fits.
fn event_id(uid: &str) -> String {
    uid.bytes().fold(String::with_capacity(uid.len() * 2), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn rfc3339(at: i64) -> String {
    jiff::Timestamp::from_second(at).unwrap_or_default().to_string()
}

fn event_body(e: &Episode, id: &str) -> serde_json::Value {
    json!({
        "id": id,
        "status": "confirmed",
        "summary": e.title,
        "description": e.url.clone().unwrap_or_default(),
        "start": { "dateTime": rfc3339(e.start) },
        "end": { "dateTime": rfc3339(e.start + i64::from(e.minutes) * 60) },
        "transparency": "transparent",
    })
}

#[derive(Debug, Deserialize)]
struct Remote {
    id: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    description: String,
    start: Option<When>,
    end: Option<When>,
}

#[derive(Debug, Deserialize)]
struct When {
    #[serde(rename = "dateTime")]
    date_time: Option<String>,
}

impl Remote {
    /// Whether it already shows `e`. Times come back in the calendar's zone.
    fn shows(&self, e: &Episode) -> bool {
        let at = |w: &Option<When>| {
            w.as_ref()?.date_time.as_deref()?.parse::<jiff::Timestamp>().ok().map(|t| t.as_second())
        };
        self.summary == e.title
            && self.description == e.url.as_deref().unwrap_or_default()
            && at(&self.start) == Some(e.start)
            && at(&self.end) == Some(e.start + i64::from(e.minutes) * 60)
    }
}

/// One sign-in attempt, listening on a free loopback port.
pub struct Login {
    client_id: String,
    authorize: String,
    state: String,
    verifier: String,
    listener: TcpListener,
    redirect: String,
}

impl Login {
    async fn start(client_id: &str, authorize: &str) -> Result<Self> {
        // Loopback only: never reachable from the network.
        let listener =
            TcpListener::bind(("127.0.0.1", 0)).await.map_err(|e| ApiError::Auth(format!("listen: {e}")))?;
        let port = listener.local_addr().map_err(|e| ApiError::Auth(e.to_string()))?.port();
        Ok(Self {
            client_id: client_id.to_owned(),
            authorize: authorize.to_owned(),
            state: oauth::random_token()?,
            verifier: oauth::random_token()?,
            listener,
            redirect: format!("http://127.0.0.1:{port}"),
        })
    }

    /// Address to open in the user's browser.
    pub fn authorize_url(&self) -> Result<Url> {
        let mut url = Url::parse(&self.authorize).map_err(|e| ApiError::Auth(e.to_string()))?;
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(self.verifier.as_bytes()));
        url.query_pairs_mut()
            .append_pair("client_id", &self.client_id)
            .append_pair("redirect_uri", &self.redirect)
            .append_pair("response_type", "code")
            .append_pair("scope", SCOPES)
            .append_pair("state", &self.state)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            // A refresh token every time, so the calendar keeps updating.
            .append_pair("access_type", "offline")
            .append_pair("prompt", "consent");
        Ok(url)
    }

    /// The authorization code, once the browser comes back with our state.
    async fn wait(&self) -> Result<String> {
        let authority = self.redirect.trim_start_matches("http://");
        loop {
            let (mut sock, _) = self.listener.accept().await.map_err(|e| ApiError::Auth(e.to_string()))?;
            let Ok(Some(req)) = tokio::time::timeout(READ_TIMEOUT, oauth::read_request(&mut sock)).await
            else {
                continue;
            };
            let get = |name: &str| req.query.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str());
            let (reply, answer) = if req.host != authority {
                // A Host check stops DNS rebinding.
                (page(421, "Misdirected request", ""), None)
            } else if req.method != "GET" || req.path != "/" {
                (page(404, "Not found", ""), None)
            } else if !get("state").is_some_and(|s| same(s, &self.state)) {
                // Anything without our state was not started by us.
                (page(400, "This sign-in link is not valid.", ""), None)
            } else {
                match get("code").filter(|c| !c.is_empty()) {
                    Some(code) => (page(200, DONE, ""), Some(Ok(code.to_owned()))),
                    None => (page(200, CANCELLED, ""), Some(Err(ApiError::Auth("cancelled".into())))),
                }
            };
            let _ = tokio::time::timeout(READ_TIMEOUT, async {
                let _ = sock.write_all(reply.as_bytes()).await;
                let _ = sock.shutdown().await;
            })
            .await;
            if let Some(a) = answer {
                return a;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::MemorySecrets;
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpStream;
    use wiremock::matchers::{body_string_contains, method, path, path_regex};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn google(server: &MockServer, secrets: Arc<dyn SecretStore>) -> Google {
        let u = server.uri();
        let urls = Urls {
            authorize: "https://accounts.test/auth".into(),
            token: format!("{u}/token"),
            userinfo: format!("{u}/userinfo"),
            api: u,
        };
        let mut g = Google::with(Http::insecure_for_tests().unwrap(), secrets, "cid", "csecret", urls);
        g.throttle = Throttle::new(Duration::ZERO);
        g.clock = || 1_800_000_000;
        g
    }

    async fn raw(port: u16, req: String) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(req.as_bytes()).await.unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out).await;
        out
    }

    #[tokio::test]
    async fn signs_in_with_pkce_and_ignores_forged_callbacks() {
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_string_contains("code=good"))
            .and(body_string_contains("client_secret=csecret"))
            .and(body_string_contains("code_verifier="))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "access_token": "A", "refresh_token": "R", "expires_in": 3600,
                "scope": format!("openid {CALENDAR_SCOPE} email")
            })))
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/userinfo"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"email": "me@example.com"})))
            .mount(&s)
            .await;
        let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecrets::default());
        let g = google(&s, secrets.clone());
        assert!(!g.is_signed_in());
        let login = g.begin_login().await.unwrap();
        let url = login.authorize_url().unwrap().to_string();
        assert!(url.contains("code_challenge_method=S256") && url.contains("access_type=offline"));
        assert!(url.contains("calendar.app.created"));
        let port = login.listener.local_addr().unwrap().port();
        let state = login.state.clone();
        let g2 = g.clone();
        let task = tokio::spawn(async move { g2.finish_login(login).await });

        let get = |p: &str| format!("GET {p} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n");
        assert!(raw(port, get("/?code=evil&state=wrong")).await.starts_with("HTTP/1.1 400"));
        let rebind = "GET /?code=x HTTP/1.1\r\nHost: evil.com\r\n\r\n".to_owned();
        assert!(raw(port, rebind).await.starts_with("HTTP/1.1 421"));
        assert!(raw(port, get(&format!("/?code=good&state={state}"))).await.contains("signed in"));

        assert_eq!(task.await.unwrap().unwrap(), "me@example.com");
        assert!(g.is_signed_in());
        assert_eq!(secrets.load(SecretKey::Google).unwrap().unwrap().refresh.as_deref(), Some("R"));
    }

    #[tokio::test]
    async fn a_sign_in_without_the_calendar_is_refused() {
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "access_token": "A", "refresh_token": "R", "expires_in": 3600, "scope": "openid email"
            })))
            .mount(&s)
            .await;
        let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecrets::default());
        let g = google(&s, secrets.clone());
        let login = g.begin_login().await.unwrap();
        let port = login.listener.local_addr().unwrap().port();
        let state = login.state.clone();
        let g2 = g.clone();
        let task = tokio::spawn(async move { g2.finish_login(login).await });
        raw(port, format!("GET /?code=c&state={state} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n")).await;
        assert!(matches!(task.await.unwrap(), Err(ApiError::Auth(m)) if m == NO_CALENDAR));
        assert!(!g.is_signed_in() && secrets.load(SecretKey::Google).unwrap().is_none());
    }

    #[tokio::test]
    async fn not_configured_without_a_client() {
        let s = MockServer::start().await;
        let mut g = google(&s, Arc::new(MemorySecrets::default()));
        g.client_secret.clear();
        assert!(matches!(g.begin_login().await, Err(ApiError::NotConfigured)));
    }

    fn episode(uid: &str, start: i64, title: &str) -> Episode {
        Episode { uid: uid.into(), start, minutes: 24, title: title.into(), url: None }
    }

    #[tokio::test]
    async fn sync_adds_updates_and_removes_only_what_changed() {
        let s = MockServer::start().await;
        let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecrets::default());
        let t =
            Tokens { access: "A".into(), refresh: Some("R".into()), expires_at: 1_900_000_000, device: None };
        secrets.save(SecretKey::Google, &t).unwrap();
        let g = google(&s, secrets);

        let same = episode("same", 1_800_100_000, "Same · Ep. 1");
        let moved = episode("moved", 1_800_200_000, "Moved · Ep. 2");
        let new = episode("new", 1_800_300_000, "New · Ep. 3");
        let remote = |e: &Episode, start: i64| {
            json!({
                "id": event_id(&e.uid), "summary": e.title, "description": "",
                // Given back in another zone, as Google does.
                "start": {"dateTime": jiff::Timestamp::from_second(start).unwrap()
                    .display_with_offset(jiff::tz::offset(9)).to_string()},
                "end": {"dateTime": rfc3339(start + 24 * 60)},
            })
        };
        Mock::given(method("POST"))
            .and(path("/calendars"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "cal@group"})))
            .expect(1)
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/calendars/cal%40group/events"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [
                remote(&same, same.start),
                remote(&moved, moved.start - 3600),
                {"id": "dropped", "summary": "Gone", "start": {"dateTime": rfc3339(1_800_050_000)}},
            ]})))
            .mount(&s)
            .await;
        Mock::given(method("PUT"))
            .and(path(format!("/calendars/cal%40group/events/{}", event_id("moved"))))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&s)
            .await;
        Mock::given(method("POST"))
            .and(path("/calendars/cal%40group/events"))
            .and(body_string_contains(event_id("new")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&s)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/calendars/cal%40group/events/dropped"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&s)
            .await;
        Mock::given(path_regex(".*"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .with_priority(10)
            .mount(&s)
            .await;

        let id = g.sync("", "Kuro", &[same, moved, new]).await.unwrap();
        assert_eq!(id, "cal@group");
    }

    #[tokio::test]
    async fn a_deleted_calendar_is_made_again() {
        let s = MockServer::start().await;
        let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecrets::default());
        let t =
            Tokens { access: "A".into(), refresh: Some("R".into()), expires_at: 1_900_000_000, device: None };
        secrets.save(SecretKey::Google, &t).unwrap();
        let g = google(&s, secrets);
        Mock::given(method("GET"))
            .and(path("/calendars/old"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&s)
            .await;
        Mock::given(method("POST"))
            .and(path("/calendars"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "fresh"})))
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/calendars/fresh/events"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": []})))
            .mount(&s)
            .await;
        assert_eq!(g.sync("old", "Kuro", &[]).await.unwrap(), "fresh");
    }

    #[tokio::test]
    async fn an_expired_token_is_refreshed_with_the_secret() {
        let s = MockServer::start().await;
        let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecrets::default());
        let t = Tokens { access: "old".into(), refresh: Some("R".into()), expires_at: 0, device: None };
        secrets.save(SecretKey::Google, &t).unwrap();
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_string_contains("grant_type=refresh_token"))
            .and(body_string_contains("client_secret=csecret"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"access_token": "N", "expires_in": 3600})),
            )
            .mount(&s)
            .await;
        let g = google(&s, secrets.clone());
        assert_eq!(g.token().await.unwrap(), "N");
        let kept = secrets.load(SecretKey::Google).unwrap().unwrap();
        assert_eq!((kept.access.as_str(), kept.refresh.as_deref()), ("N", Some("R")));
    }

    #[tokio::test]
    async fn a_refusal_carries_googles_reason() {
        let s = MockServer::start().await;
        let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecrets::default());
        let t =
            Tokens { access: "A".into(), refresh: Some("R".into()), expires_at: 1_900_000_000, device: None };
        secrets.save(SecretKey::Google, &t).unwrap();
        Mock::given(method("POST"))
            .and(path("/calendars"))
            .respond_with(ResponseTemplate::new(403).set_body_json(json!({"error": {
                "code": 403, "message": "Google Calendar API has not been used in project 1 before or it is disabled."
            }})))
            .mount(&s)
            .await;
        let e = google(&s, secrets).sync("", "Kuro", &[]).await.unwrap_err();
        assert!(
            matches!(&e, ApiError::Service(m) if m.contains("disabled") && m.ends_with("(403)")),
            "{e:?}"
        );
    }

    #[tokio::test]
    async fn moving_away_clears_what_is_still_to_come() {
        let s = MockServer::start().await;
        let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecrets::default());
        let t =
            Tokens { access: "A".into(), refresh: Some("R".into()), expires_at: 1_900_000_000, device: None };
        secrets.save(SecretKey::Google, &t).unwrap();
        let g = google(&s, secrets);
        Mock::given(method("GET"))
            .and(path("/calendars/old/events"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"items": [{"id": "e1"}, {"id": "e2"}]})),
            )
            .mount(&s)
            .await;
        Mock::given(method("DELETE"))
            .and(path_regex("^/calendars/old/events/e[12]$"))
            .respond_with(ResponseTemplate::new(204))
            .expect(2)
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/calendars/gone/events"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&s)
            .await;
        g.clear("old").await.unwrap();
        g.clear("gone").await.unwrap();
    }

    #[test]
    fn event_ids_use_only_allowed_characters() {
        let id = event_id("kuro-anilist-12345-3@kuro");
        assert!(id.len() >= 5 && id.chars().all(|c| matches!(c, '0'..='9' | 'a'..='v')));
    }
}
