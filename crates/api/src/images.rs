//! Cover and avatar downloads from known hosts only.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use sha2::{Digest, Sha256};
use url::Url;

use crate::http::{ApiError, Http, Result, Throttle, check, read_limited};

const MAX_IMAGE: usize = 8 * 1024 * 1024;
const HOSTS: [&str; 6] =
    ["anilist.co", "myanimelist.net", "mzstatic.com", "scdn.co", "spotifycdn.com", "cdn.myanimelist.net"];

pub fn allowed(url: &str) -> Option<Url> {
    let url = Url::parse(url).ok()?;
    let host = url.host_str()?;
    let known = HOSTS.iter().any(|h| host == *h || host.strip_suffix(h).is_some_and(|p| p.ends_with('.')));
    (url.scheme() == "https" && known && url.port().is_none() && url.username().is_empty()).then_some(url)
}

#[derive(Clone)]
pub struct ImageCache {
    http: Http,
    dir: PathBuf,
    throttle: Throttle,
}

impl ImageCache {
    pub fn new(http: Http, dir: PathBuf) -> Self {
        Self { http, dir, throttle: Throttle::new(Duration::ZERO) }
    }

    /// The file name is a hash, so a URL cannot choose a path.
    fn path(&self, url: &str) -> PathBuf {
        let hash = Sha256::digest(url);
        let name: String = hash.iter().map(|b| format!("{b:02x}")).collect();
        self.dir.join(name)
    }

    pub async fn get(&self, url: &str) -> Result<Vec<u8>> {
        let parsed = allowed(url).ok_or(ApiError::Blocked)?;
        let path = self.path(url);
        if let Ok(bytes) = tokio::fs::read(&path).await {
            return Ok(bytes);
        }
        let res = self.http.send(self.http.client().get(parsed), &self.throttle).await?;
        // A redirect must not escape the allowed hosts.
        allowed(res.url().as_str()).ok_or(ApiError::Blocked)?;
        let bytes = read_limited(check(res)?, MAX_IMAGE).await?;
        if !is_image(&bytes) {
            return Err(ApiError::Decode("not an image".into()));
        }
        let _ = tokio::fs::create_dir_all(&self.dir).await;
        let temp =
            path.with_extension(format!("{:?}.tmp", std::thread::current().id()).replace(['(', ')'], ""));
        if tokio::fs::write(&temp, &bytes).await.is_ok() {
            let _ = tokio::fs::rename(&temp, &path).await;
        }
        Ok(bytes)
    }

    pub fn size(&self) -> u64 {
        files(&self.dir).iter().map(|f| f.1).sum()
    }

    pub fn clear(&self) {
        for (path, ..) in files(&self.dir) {
            let _ = std::fs::remove_file(path);
        }
    }

    /// Removes the oldest files until the cache fits `max` bytes.
    pub fn trim(&self, max: u64) {
        let mut all = files(&self.dir);
        let mut total: u64 = all.iter().map(|f| f.1).sum();
        all.sort_by_key(|f| f.2);
        for (path, size, _) in all {
            if total <= max {
                break;
            }
            if std::fs::remove_file(path).is_ok() {
                total -= size;
            }
        }
    }
}

fn files(dir: &Path) -> Vec<(PathBuf, u64, SystemTime)> {
    let Ok(read) = std::fs::read_dir(dir) else { return Vec::new() };
    read.flatten()
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            m.is_file().then(|| (e.path(), m.len(), m.modified().unwrap_or(SystemTime::UNIX_EPOCH)))
        })
        .collect()
}

fn is_image(b: &[u8]) -> bool {
    b.starts_with(&[0xFF, 0xD8, 0xFF])
        || b.starts_with(b"\x89PNG\r\n\x1a\n")
        || b.starts_with(b"GIF8")
        || (b.len() > 12 && b.starts_with(b"RIFF") && &b[8..12] == b"WEBP")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_known_https_hosts() {
        assert!(allowed("https://s4.anilist.co/file/x.jpg").is_some());
        assert!(allowed("https://cdn.myanimelist.net/i.jpg").is_some());
        for bad in [
            "http://s4.anilist.co/x.jpg",
            "https://evilanilist.co/x.jpg",
            "https://anilist.co.evil.com/x.jpg",
            "https://u:p@s4.anilist.co/x.jpg",
            "https://s4.anilist.co:8443/x.jpg",
            "https://127.0.0.1/x.jpg",
            "file:///c:/windows/win.ini",
            "",
        ] {
            assert!(allowed(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn detects_images_by_content() {
        assert!(is_image(&[0xFF, 0xD8, 0xFF, 0]));
        assert!(!is_image(b"<html>"));
        assert!(!is_image(b"RIFF"));
    }

    #[tokio::test]
    async fn blocked_urls_never_hit_the_network() {
        let dir = tempfile::tempdir().unwrap();
        let c = ImageCache::new(Http::new().unwrap(), dir.path().into());
        assert_eq!(c.get("http://localhost/x.png").await, Err(ApiError::Blocked));
    }

    #[test]
    fn trim_respects_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..5 {
            std::fs::write(dir.path().join(i.to_string()), [0u8; 100]).unwrap();
        }
        let c = ImageCache::new(Http::new().unwrap(), dir.path().into());
        c.trim(250);
        assert_eq!(c.size(), 200);
        c.clear();
        assert_eq!(c.size(), 0);
    }
}
