//! Browser sign-in through a short-lived loopback listener.
//!
//! MyAnimeList uses the authorization code flow with PKCE (it only offers the
//! `plain` method). AniList uses the implicit flow, so no client secret is
//! shipped; the token arrives in the URL fragment and the landing page posts
//! it back with a one-time nonce.

use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use kuro_core::Service;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use url::Url;

use crate::http::{ApiError, Http, Result, Throttle, check, decode};
use crate::secrets::Tokens;

/// Must match the redirect URL registered with each service.
pub const PORT: u16 = 42813;
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);
const READ_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_REQUEST: usize = 16 * 1024;

pub fn redirect_uri(service: Service) -> String {
    format!("http://127.0.0.1:{PORT}/{}", service.key())
}

pub fn random_token() -> Result<String> {
    let mut bytes = [0u8; 48];
    getrandom::fill(&mut bytes).map_err(|e| ApiError::Auth(e.to_string()))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// Compares without leaking where the strings differ.
fn same(a: &str, b: &str) -> bool {
    let (a, b) = (Sha256::digest(a), Sha256::digest(b));
    a.iter().zip(b.iter()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// One sign-in attempt.
pub struct Login {
    service: Service,
    client_id: String,
    state: String,
    verifier: String,
    nonce: String,
    listener: TcpListener,
    redirect: String,
    endpoints: Endpoints,
}

#[derive(Debug, Clone)]
pub struct Endpoints {
    pub authorize: String,
    pub token: String,
}

impl Endpoints {
    pub fn of(service: Service) -> Self {
        let (authorize, token) = match service {
            Service::AniList => {
                ("https://anilist.co/api/v2/oauth/authorize", "https://anilist.co/api/v2/oauth/token")
            }
            Service::Mal => {
                ("https://myanimelist.net/v1/oauth2/authorize", "https://myanimelist.net/v1/oauth2/token")
            }
        };
        Self { authorize: authorize.into(), token: token.into() }
    }
}

#[derive(Debug)]
pub struct LoginResult {
    pub tokens: Tokens,
    /// False when the service did not echo our state, so the user should
    /// confirm the account before it is used.
    pub verified: bool,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: i64,
}

impl TokenResponse {
    fn into_tokens(self, now: i64) -> Result<Tokens> {
        if self.access_token.is_empty() {
            return Err(ApiError::Auth("empty token".into()));
        }
        Ok(Tokens {
            access: self.access_token,
            refresh: self.refresh_token,
            expires_at: now.saturating_add(self.expires_in.max(0)),
            device: None,
        })
    }
}

impl Login {
    pub async fn start(service: Service, client_id: &str) -> Result<Self> {
        Self::start_at(service, client_id, PORT, Endpoints::of(service)).await
    }

    async fn start_at(service: Service, client_id: &str, port: u16, endpoints: Endpoints) -> Result<Self> {
        if client_id.is_empty() {
            return Err(ApiError::NotConfigured);
        }
        // Loopback only: never reachable from the network.
        let listener = TcpListener::bind(("127.0.0.1", port))
            .await
            .map_err(|e| ApiError::Auth(format!("port {port}: {e}")))?;
        let port = listener.local_addr().map_err(|e| ApiError::Auth(e.to_string()))?.port();
        Ok(Self {
            service,
            client_id: client_id.to_owned(),
            state: random_token()?,
            verifier: random_token()?,
            nonce: random_token()?,
            listener,
            redirect: format!("http://127.0.0.1:{port}/{}", service.key()),
            endpoints,
        })
    }

    /// Address to open in the user's browser.
    pub fn authorize_url(&self) -> Result<Url> {
        let mut url = Url::parse(&self.endpoints.authorize).map_err(|e| ApiError::Auth(e.to_string()))?;
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("client_id", &self.client_id).append_pair("state", &self.state);
            match self.service {
                Service::AniList => q.append_pair("response_type", "token"),
                Service::Mal => q
                    .append_pair("response_type", "code")
                    .append_pair("redirect_uri", &self.redirect)
                    .append_pair("code_challenge", &self.verifier)
                    .append_pair("code_challenge_method", "plain"),
            };
        }
        Ok(url)
    }

    /// Waits for the browser to come back.
    pub async fn finish(self, http: &Http, throttle: &Throttle, now: i64) -> Result<LoginResult> {
        let answer = tokio::time::timeout(LOGIN_TIMEOUT, self.wait())
            .await
            .map_err(|_| ApiError::Auth("timed out".into()))??;
        match answer {
            Answer::Token { response, verified } => {
                Ok(LoginResult { tokens: response.into_tokens(now)?, verified })
            }
            Answer::Code(code) => {
                let form = [
                    ("client_id", self.client_id.as_str()),
                    ("grant_type", "authorization_code"),
                    ("code", &code),
                    ("code_verifier", &self.verifier),
                    ("redirect_uri", &self.redirect),
                ];
                let res = http.send(http.client().post(&self.endpoints.token).form(&form), throttle).await?;
                let t: TokenResponse = decode(check(res)?).await?;
                Ok(LoginResult { tokens: t.into_tokens(now)?, verified: true })
            }
        }
    }

    async fn wait(&self) -> Result<Answer> {
        loop {
            let (mut sock, _) = self.listener.accept().await.map_err(|e| ApiError::Auth(e.to_string()))?;
            let Ok(Some(req)) = tokio::time::timeout(READ_TIMEOUT, read_request(&mut sock)).await else {
                continue;
            };
            let (reply, answer) = self.handle(&req);
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

    fn handle(&self, req: &Request) -> (String, Option<Result<Answer>>) {
        // A Host check stops DNS rebinding.
        let authority = self.redirect.trim_start_matches("http://").split('/').next().unwrap_or_default();
        if req.host != authority {
            return (page(421, "Misdirected request", ""), None);
        }
        let path = format!("/{}", self.service.key());
        match (self.service, req.method.as_str(), req.path.as_str()) {
            (Service::Mal, "GET", p) if p == path => {
                let get = |name: &str| req.query.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str());
                // Anything without our state was not started by us; ignore it.
                if !get("state").is_some_and(|s| same(s, &self.state)) {
                    return (page(400, "This sign-in link is not valid.", ""), None);
                }
                match get("code").filter(|c| !c.is_empty()) {
                    Some(code) => (page(200, DONE, ""), Some(Ok(Answer::Code(code.to_owned())))),
                    None => (page(200, CANCELLED, ""), Some(Err(ApiError::Auth("cancelled".into())))),
                }
            }
            (Service::AniList, "GET", p) if p == path => (page(200, "Finishing sign-in…", &self.nonce), None),
            (Service::AniList, "POST", "/anilist/token") => {
                let origin_ok = req.origin == format!("http://{authority}");
                if !origin_ok || !same(&req.nonce, &self.nonce) {
                    return (page(403, "Rejected", ""), None);
                }
                let pairs: Vec<(String, String)> = url::form_urlencoded::parse(req.body.as_bytes())
                    .map(|(k, v)| (k.into_owned(), v.into_owned()))
                    .collect();
                let get = |name: &str| pairs.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
                let state = get("state");
                if state.as_ref().is_some_and(|s| !same(s, &self.state)) {
                    return (page(400, "This sign-in link is not valid.", ""), None);
                }
                match get("access_token").filter(|t| !t.is_empty()) {
                    Some(access_token) => {
                        let response = TokenResponse {
                            access_token,
                            refresh_token: None,
                            expires_in: get("expires_in").and_then(|e| e.parse().ok()).unwrap_or(0),
                        };
                        let answer = Answer::Token { response, verified: state.is_some() };
                        (page(200, DONE, ""), Some(Ok(answer)))
                    }
                    None => (page(200, CANCELLED, ""), Some(Err(ApiError::Auth("cancelled".into())))),
                }
            }
            _ => (page(404, "Not found", ""), None),
        }
    }
}

enum Answer {
    Code(String),
    Token { response: TokenResponse, verified: bool },
}

const DONE: &str = "You are signed in. You can close this tab.";
const CANCELLED: &str = "Sign-in was cancelled. You can close this tab.";

/// `nonce` enables the script that forwards the URL fragment.
fn page(status: u16, text: &str, nonce: &str) -> String {
    let script = if nonce.is_empty() {
        String::new()
    } else {
        format!(
            "<script nonce=\"{nonce}\">const h=location.hash.slice(1);\
             history.replaceState(null,'',location.pathname);\
             fetch('/anilist/token',{{method:'POST',headers:{{'X-Nonce':'{nonce}'}},body:h}})\
             .then(r=>r.text()).then(t=>{{const d=new DOMParser().parseFromString(t,'text/html');\
             document.querySelector('p').textContent=d.querySelector('p').textContent}})</script>"
        )
    };
    let body = format!(
        "<!doctype html><meta charset=utf-8><title>Kuro</title>\
         <body style=\"font:16px system-ui;display:grid;place-items:center;height:90vh\"><p>{text}</p>{script}"
    );
    let csp = if nonce.is_empty() {
        "default-src 'none'; style-src 'unsafe-inline'".to_owned()
    } else {
        format!(
            "default-src 'none'; style-src 'unsafe-inline'; connect-src 'self'; script-src 'nonce-{nonce}'"
        )
    };
    format!(
        "HTTP/1.1 {status} X\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
         Content-Security-Policy: {csp}; frame-ancestors 'none'\r\nReferrer-Policy: no-referrer\r\n\
         Cache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

#[derive(Debug, Default)]
struct Request {
    method: String,
    path: String,
    query: Vec<(String, String)>,
    host: String,
    origin: String,
    nonce: String,
    body: String,
}

async fn read_request(sock: &mut TcpStream) -> Option<Request> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        let n = sock.read(&mut chunk).await.ok()?;
        if n == 0 || buf.len() + n > MAX_REQUEST {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = std::str::from_utf8(&buf[..head_end]).ok()?.to_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let method = first.next()?.to_owned();
    let target =
        Url::parse(&format!("http://localhost{}", first.next().filter(|t| t.starts_with('/'))?)).ok()?;
    let mut req = Request {
        method,
        path: target.path().to_owned(),
        query: target.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect(),
        ..Request::default()
    };
    let mut length = 0usize;
    for line in lines {
        let (name, value) = line.split_once(':')?;
        let value = value.trim().to_owned();
        match name.to_ascii_lowercase().as_str() {
            "host" => req.host = value,
            "origin" => req.origin = value,
            "x-nonce" => req.nonce = value,
            "content-length" => length = value.parse().ok()?,
            _ => {}
        }
    }
    let start = head_end + 4;
    if length > MAX_REQUEST {
        return None;
    }
    while buf.len() < start + length {
        let n = sock.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    req.body = String::from_utf8(buf[start..start + length].to_vec()).ok()?;
    Some(req)
}

/// Exchanges a refresh token. Only MyAnimeList issues them to public clients.
pub async fn refresh(
    http: &Http,
    throttle: &Throttle,
    endpoints: &Endpoints,
    client_id: &str,
    tokens: &Tokens,
    now: i64,
) -> Result<Tokens> {
    let Some(refresh) = tokens.refresh.as_deref() else { return Err(ApiError::Unauthorized) };
    let form = [("client_id", client_id), ("grant_type", "refresh_token"), ("refresh_token", refresh)];
    let res = http.send(http.client().post(&endpoints.token).form(&form), throttle).await?;
    if res.status().is_client_error() {
        return Err(ApiError::Unauthorized);
    }
    let mut t = decode::<TokenResponse>(check(res)?).await?.into_tokens(now)?;
    if t.refresh.is_none() {
        t.refresh = Some(refresh.to_owned());
    }
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string_contains, method};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn login(service: Service, token: &str) -> (Login, u16) {
        let e = Endpoints { authorize: "https://x.test/a".into(), token: token.into() };
        let l = Login::start_at(service, "cid", 0, e).await.unwrap();
        let port = l.listener.local_addr().unwrap().port();
        (l, port)
    }

    async fn raw(port: u16, req: String) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(req.as_bytes()).await.unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out).await;
        out
    }

    fn get(port: u16, path: &str) -> String {
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n")
    }

    fn post(port: u16, nonce: &str, origin: &str, body: &str) -> String {
        format!(
            "POST /anilist/token HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: {origin}\r\nX-Nonce: {nonce}\r\n\
             Content-Length: {}\r\n\r\n{body}",
            body.len()
        )
    }

    #[tokio::test]
    async fn mal_code_flow_ignores_forged_callbacks() {
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(body_string_contains("code=good"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "A", "refresh_token": "R", "expires_in": 100, "token_type": "Bearer"
            })))
            .mount(&s)
            .await;
        let (l, port) = login(Service::Mal, &s.uri()).await;
        let url = l.authorize_url().unwrap();
        assert!(url.as_str().contains("code_challenge_method=plain"));
        let state = l.state.clone();
        assert!(l.verifier.len() >= 43 && l.verifier != l.state);
        let http = Http::insecure_for_tests().unwrap();
        let t = Throttle::new(Duration::ZERO);
        let task = tokio::spawn(async move { l.finish(&http, &t, 1000).await });

        assert!(raw(port, get(port, "/mal?code=evil&state=wrong")).await.starts_with("HTTP/1.1 400"));
        assert!(raw(port, get(port, "/mal?code=evil")).await.starts_with("HTTP/1.1 400"));
        let rebind = "GET /mal?code=x HTTP/1.1\r\nHost: evil.com\r\n\r\n".to_owned();
        assert!(raw(port, rebind).await.starts_with("HTTP/1.1 421"));
        assert!(raw(port, get(port, "/other")).await.starts_with("HTTP/1.1 404"));
        raw(port, "garbage\r\n\r\n".into()).await;
        assert!(raw(port, get(port, &format!("/mal?code=good&state={state}"))).await.contains("signed in"));

        let r = task.await.unwrap().unwrap();
        assert_eq!((r.tokens.access.as_str(), r.tokens.expires_at, r.verified), ("A", 1100, true));
    }

    #[tokio::test]
    async fn anilist_fragment_flow_needs_nonce_origin_and_state() {
        let (l, port) = login(Service::AniList, "https://unused.test").await;
        let (state, nonce) = (l.state.clone(), l.nonce.clone());
        let origin = format!("http://127.0.0.1:{port}");
        let http = Http::insecure_for_tests().unwrap();
        let t = Throttle::new(Duration::ZERO);
        let task = tokio::spawn(async move { l.finish(&http, &t, 0).await });

        let landing = raw(port, get(port, "/anilist")).await;
        assert!(landing.contains(&format!("'nonce-{nonce}'")) && landing.contains("frame-ancestors 'none'"));
        let ok = format!("access_token=T&expires_in=50&state={state}");
        assert!(raw(port, post(port, "bad", &origin, &ok)).await.starts_with("HTTP/1.1 403"));
        assert!(raw(port, post(port, &nonce, "https://evil.com", &ok)).await.starts_with("HTTP/1.1 403"));
        let forged = "access_token=EVIL&expires_in=50&state=nope";
        assert!(raw(port, post(port, &nonce, &origin, forged)).await.starts_with("HTTP/1.1 400"));
        raw(port, post(port, &nonce, &origin, &ok)).await;

        let r = task.await.unwrap().unwrap();
        assert_eq!((r.tokens.access.as_str(), r.tokens.expires_at, r.verified), ("T", 50, true));
    }

    #[tokio::test]
    async fn anilist_without_state_is_marked_unverified() {
        let (l, port) = login(Service::AniList, "https://unused.test").await;
        let nonce = l.nonce.clone();
        let http = Http::insecure_for_tests().unwrap();
        let t = Throttle::new(Duration::ZERO);
        let task = tokio::spawn(async move { l.finish(&http, &t, 0).await });
        raw(port, post(port, &nonce, &format!("http://127.0.0.1:{port}"), "access_token=T&expires_in=1"))
            .await;
        assert!(!task.await.unwrap().unwrap().verified);
    }

    #[tokio::test]
    async fn denied_login_reports_cancelled() {
        let (l, port) = login(Service::Mal, "https://unused.test").await;
        let state = l.state.clone();
        let http = Http::insecure_for_tests().unwrap();
        let t = Throttle::new(Duration::ZERO);
        let task = tokio::spawn(async move { l.finish(&http, &t, 0).await });
        raw(port, get(port, &format!("/mal?error=access_denied&state={state}"))).await;
        assert!(matches!(task.await.unwrap(), Err(ApiError::Auth(m)) if m == "cancelled"));
    }

    #[tokio::test]
    async fn refresh_keeps_old_refresh_token_when_omitted() {
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"access_token": "N", "expires_in": 10})),
            )
            .mount(&s)
            .await;
        let http = Http::insecure_for_tests().unwrap();
        let e = Endpoints { authorize: String::new(), token: s.uri() };
        let old = Tokens { access: "O".into(), refresh: Some("R".into()), expires_at: 0, device: None };
        let t = refresh(&http, &Throttle::new(Duration::ZERO), &e, "cid", &old, 5).await.unwrap();
        assert_eq!((t.access.as_str(), t.refresh.as_deref(), t.expires_at), ("N", Some("R"), 15));
    }

    #[tokio::test]
    async fn missing_client_id_is_reported() {
        assert!(matches!(Login::start(Service::Mal, "").await, Err(ApiError::NotConfigured)));
    }
}
