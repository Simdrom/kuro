//! Whether a newer Kuro has been published, from its releases on GitHub.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::http::{ApiError, Http, Result, Throttle};

/// Where Kuro is published.
pub const REPOSITORY: &str = "Simdrom/kuro";

/// A published version of Kuro.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Release {
    /// `x.y.z`, without the `v` of the tag.
    pub version: String,
    /// The release page, where the setup is downloaded.
    pub url: String,
}

#[derive(Clone)]
pub struct Updates {
    http: Http,
    base: String,
    throttle: Throttle,
}

impl Updates {
    /// Asks GitHub with a client of its own, which follows no redirects.
    pub fn new() -> Result<Self> {
        let agent = format!("Kuro/{}", env!("CARGO_PKG_VERSION"));
        Ok(Self::with_endpoint(Http::strict(&agent)?, "https://api.github.com"))
    }

    pub fn with_endpoint(http: Http, base: &str) -> Self {
        Self {
            http,
            base: base.trim_end_matches('/').into(),
            throttle: Throttle::new(std::time::Duration::ZERO),
        }
    }

    /// The newest release: GitHub never names a draft or a pre-release the latest.
    pub async fn latest(&self) -> Result<Release> {
        let url = format!("{}/repos/{REPOSITORY}/releases/latest", self.base);
        let req = self
            .http
            .client()
            .get(url)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28");
        let v: Value = self.http.json(req, &self.throttle).await?;
        parse(&v)
    }
}

fn parse(v: &Value) -> Result<Release> {
    let tag = v["tag_name"].as_str().ok_or_else(|| ApiError::Decode("no tag".into()))?;
    let version = tag.trim_start_matches('v');
    if parse_version(version).is_none() {
        return Err(ApiError::Decode(format!("not a version: {tag}")));
    }
    // Only Kuro's own release pages are ever opened.
    let page = format!("https://github.com/{REPOSITORY}/releases/");
    let url = v["html_url"].as_str().filter(|u| u.starts_with(&page)).ok_or(ApiError::Blocked)?;
    Ok(Release { version: version.to_owned(), url: url.to_owned() })
}

fn parse_version(v: &str) -> Option<[u64; 3]> {
    let mut parts = v.split('.').map(|p| p.parse::<u64>().ok());
    let out = [parts.next()??, parts.next()??, parts.next()??];
    parts.next().is_none().then_some(out)
}

/// Whether `latest` is a later version than `current`; false when either is not `x.y.z`.
pub fn is_newer(latest: &str, current: &str) -> bool {
    matches!((parse_version(latest), parse_version(current)), (Some(l), Some(c)) if l > c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{header, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn versions_compare_by_number() {
        assert!(is_newer("0.10.0", "0.9.3"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(!is_newer("0.6.0", "0.6.0"));
        assert!(!is_newer("0.5.1", "0.6.0"));
        assert!(!is_newer("0.7", "0.6.0"));
        assert!(!is_newer("0.7.0-beta", "0.6.0"));
    }

    #[tokio::test]
    async fn reads_the_latest_release() {
        let s = MockServer::start().await;
        Mock::given(path("/repos/Simdrom/kuro/releases/latest"))
            .and(header("Accept", "application/vnd.github+json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "tag_name": "v0.7.0",
                "html_url": "https://github.com/Simdrom/kuro/releases/tag/v0.7.0",
                "draft": false,
            })))
            .mount(&s)
            .await;
        let u = Updates::with_endpoint(Http::strict_for_tests("Kuro/test").unwrap(), &s.uri());
        let r = u.latest().await.unwrap();
        assert_eq!(r.version, "0.7.0");
        assert_eq!(r.url, "https://github.com/Simdrom/kuro/releases/tag/v0.7.0");
    }

    #[test]
    fn refuses_other_pages_and_odd_tags() {
        let page = "https://github.com/Simdrom/kuro/releases/tag/v1.0.0";
        assert_eq!(
            parse(&json!({"tag_name": "v1.0.0", "html_url": "https://evil.example/x"})),
            Err(ApiError::Blocked)
        );
        assert!(parse(&json!({"tag_name": "nightly", "html_url": page})).is_err());
        assert!(parse(&json!({"html_url": page})).is_err());
    }
}
