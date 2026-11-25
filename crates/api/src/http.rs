//! Hardened HTTP access: HTTPS only, bounded bodies, throttling and retries.

use std::sync::Arc;
use std::time::Duration;

use reqwest::{RequestBuilder, Response, StatusCode, redirect};
use serde::de::DeserializeOwned;
use thiserror::Error;
use tokio::sync::Mutex;
use tokio::time::Instant;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ApiError {
    #[error("network error: {0}")]
    Network(String),
    #[error("the service answered with status {0}")]
    Status(u16),
    #[error("too many requests, try again later")]
    RateLimited,
    #[error("sign in again")]
    Unauthorized,
    #[error("not found")]
    NotFound,
    #[error("unexpected response: {0}")]
    Decode(String),
    #[error("response is too large")]
    TooLarge,
    #[error("service is not configured")]
    NotConfigured,
    #[error("blocked address")]
    Blocked,
    #[error("sign-in failed: {0}")]
    Auth(String),
    #[error("secure storage failed: {0}")]
    Secrets(String),
    #[error("not available yet")]
    Unavailable,
    #[error("{0}")]
    Service(String),
}

impl ApiError {
    /// Whether the same request may succeed later without user action.
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Network(_) | Self::RateLimited => true,
            Self::Status(s) => *s >= 500 || *s == 403 || *s == 408,
            _ => false,
        }
    }
}

impl From<reqwest::Error> for ApiError {
    fn from(e: reqwest::Error) -> Self {
        // URLs may carry tokens in their query string, so they never reach messages or logs.
        Self::Network(e.without_url().to_string())
    }
}

pub type Result<T> = std::result::Result<T, ApiError>;

const MAX_JSON: usize = 24 * 1024 * 1024;
const MAX_WAIT: Duration = Duration::from_secs(65);

/// Spaces requests out so a service limit is not exceeded.
#[derive(Clone)]
pub struct Throttle {
    next: Arc<Mutex<Instant>>,
    gap: Duration,
}

impl Throttle {
    pub fn new(gap: Duration) -> Self {
        Self { next: Arc::new(Mutex::new(Instant::now())), gap }
    }

    /// A few requests may burst before spacing starts.
    const BURST: u32 = 8;

    pub async fn wait(&self) {
        let at = {
            let mut next = self.next.lock().await;
            let now = Instant::now();
            let floor = now.checked_sub(self.gap * Self::BURST).unwrap_or(now);
            *next = (*next).max(floor) + self.gap;
            *next
        };
        tokio::time::sleep_until(at).await;
    }

    async fn pause(&self, d: Duration) {
        let mut next = self.next.lock().await;
        *next = (*next).max(Instant::now() + d);
    }
}

#[derive(Clone)]
pub struct Http {
    client: reqwest::Client,
    https_only: bool,
}

impl Http {
    pub fn new() -> Result<Self> {
        Self::build(true, "anime-manga-tracker", true)
    }

    /// Allows plain HTTP, for mock servers only.
    pub fn insecure_for_tests() -> Result<Self> {
        Self::build(false, "anime-manga-tracker", true)
    }

    /// A client of its own: another user agent and no redirects, so every
    /// address it reaches is one the caller chose.
    pub fn strict(user_agent: &str) -> Result<Self> {
        Self::build(true, user_agent, false)
    }

    /// [`Http::strict`] over plain HTTP, for mock servers only.
    pub fn strict_for_tests(user_agent: &str) -> Result<Self> {
        Self::build(false, user_agent, false)
    }

    fn build(https_only: bool, user_agent: &str, follow: bool) -> Result<Self> {
        let policy = if follow { redirect::Policy::limited(3) } else { redirect::Policy::none() };
        let client = reqwest::Client::builder()
            .https_only(https_only)
            .user_agent(user_agent)
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .redirect(policy)
            .referer(false)
            .build()?;
        Ok(Self { client, https_only })
    }

    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }

    pub fn allows_plain_http(&self) -> bool {
        !self.https_only
    }

    /// Sends a request, waiting out one rate limit answer.
    pub async fn send(&self, req: RequestBuilder, throttle: &Throttle) -> Result<Response> {
        let retry = req.try_clone();
        throttle.wait().await;
        let res = req.send().await?;
        if res.status() != StatusCode::TOO_MANY_REQUESTS {
            return Ok(res);
        }
        let wait = res
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .map_or(Duration::from_secs(5), Duration::from_secs);
        if wait > MAX_WAIT {
            return Err(ApiError::RateLimited);
        }
        throttle.pause(wait).await;
        let Some(retry) = retry else { return Err(ApiError::RateLimited) };
        throttle.wait().await;
        let res = retry.send().await?;
        if res.status() == StatusCode::TOO_MANY_REQUESTS {
            return Err(ApiError::RateLimited);
        }
        Ok(res)
    }

    pub async fn json<T: DeserializeOwned>(&self, req: RequestBuilder, throttle: &Throttle) -> Result<T> {
        let res = self.send(req, throttle).await?;
        decode(check(res)?).await
    }
}

pub fn check(res: Response) -> Result<Response> {
    match res.status() {
        s if s.is_success() => Ok(res),
        StatusCode::UNAUTHORIZED => Err(ApiError::Unauthorized),
        StatusCode::NOT_FOUND => Err(ApiError::NotFound),
        s => Err(ApiError::Status(s.as_u16())),
    }
}

/// Reads a body without trusting its declared length.
pub async fn read_limited(mut res: Response, max: usize) -> Result<Vec<u8>> {
    if res.content_length().is_some_and(|n| n > max as u64) {
        return Err(ApiError::TooLarge);
    }
    let mut body = Vec::new();
    while let Some(chunk) = res.chunk().await? {
        if body.len() + chunk.len() > max {
            return Err(ApiError::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

pub async fn decode<T: DeserializeOwned>(res: Response) -> Result<T> {
    let body = read_limited(res, MAX_JSON).await?;
    serde_json::from_slice(&body).map_err(|e| ApiError::Decode(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::any;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn throttle() -> Throttle {
        Throttle::new(Duration::ZERO)
    }

    #[tokio::test]
    async fn plain_http_is_refused_in_production() {
        let http = Http::new().unwrap();
        let err = http.client().get("http://127.0.0.1:9/").send().await.unwrap_err();
        assert!(err.is_builder() || err.is_request(), "{err:?}");
    }

    #[tokio::test]
    async fn oversized_bodies_are_rejected() {
        let s = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'x'; 5000]))
            .mount(&s)
            .await;
        let http = Http::insecure_for_tests().unwrap();
        let res = http.send(http.client().get(s.uri()), &throttle()).await.unwrap();
        assert_eq!(read_limited(res, 4999).await, Err(ApiError::TooLarge));
    }

    #[tokio::test]
    async fn retries_once_after_rate_limit() {
        let s = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "0"))
            .up_to_n_times(1)
            .mount(&s)
            .await;
        Mock::given(any()).respond_with(ResponseTemplate::new(200).set_body_json([1])).mount(&s).await;
        let http = Http::insecure_for_tests().unwrap();
        let v: Vec<i32> = http.json(http.client().get(s.uri()), &throttle()).await.unwrap();
        assert_eq!(v, [1]);
    }

    #[tokio::test]
    async fn long_rate_limits_fail_fast() {
        let s = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "9999"))
            .mount(&s)
            .await;
        let http = Http::insecure_for_tests().unwrap();
        let r = http.json::<()>(http.client().get(s.uri()), &throttle()).await;
        assert_eq!(r, Err(ApiError::RateLimited));
    }

    #[tokio::test]
    async fn error_messages_never_contain_urls() {
        let http = Http::insecure_for_tests().unwrap();
        let e: ApiError =
            http.client().get("http://127.0.0.1:1/?token=secret").send().await.unwrap_err().into();
        assert!(!e.to_string().contains("secret"), "{e}");
    }
}
