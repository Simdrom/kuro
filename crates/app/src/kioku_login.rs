//! Kioku's Crunchyroll sign-in, in a process of its own: `kuro --kioku-login crunchyroll`.
//!
//! The window shows Crunchyroll's own sign-in page in a private WebView2, so
//! the password is typed into Crunchyroll and never reaches Kuro. Once the
//! page has set its session cookie, the cookie is exchanged once for a refresh
//! token, which goes to the Credential Manager; the cookie itself is dropped
//! and, being private, the browser keeps nothing on disk either.
//!
//! It runs apart from the main window because both need an event loop of
//! their own, and a process can only have one. The result is its exit code.

use std::sync::Arc;
use std::time::{Duration, Instant};

use kuro_api::Keyring;
use kuro_api::kioku::crunchyroll::{COOKIE, Crunchyroll, SIGN_IN_HOSTS, SIGN_IN_URL, SITE};
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::platform::run_return::EventLoopExtRunReturn;
use tao::window::WindowBuilder;
use wry::WebViewBuilder;
use zeroize::Zeroizing;

pub const ARG: &str = "--kioku-login";

/// Exit codes the main window reads.
pub const SIGNED_IN: i32 = 0;
pub const CANCELLED: i32 = 2;
pub const FAILED: i32 = 3;
pub const EXPIRED: i32 = 4;

const TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// Only Crunchyroll's own pages, over HTTPS.
fn allowed(address: &str) -> bool {
    url::Url::parse(address)
        .is_ok_and(|u| u.scheme() == "https" && u.host_str().is_some_and(|h| SIGN_IN_HOSTS.contains(&h)))
        || address == "about:blank"
}

fn cookie(secrets: &str) -> Option<Zeroizing<String>> {
    let mut event_loop = EventLoopBuilder::new().build();
    let window = WindowBuilder::new()
        .with_title("Kuro · Crunchyroll")
        .with_inner_size(LogicalSize::new(480.0, 760.0))
        .build(&event_loop)
        .ok()?;
    let webview = match WebViewBuilder::new()
        .with_url(SIGN_IN_URL)
        .with_incognito(true)
        .with_navigation_handler(|u| allowed(&u))
        .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
        .build(&window)
    {
        Ok(w) => w,
        Err(e) => {
            tracing::error!("the sign-in window could not open (is the WebView2 runtime installed?): {e}");
            return None;
        }
    };
    tracing::debug!("signing in for {secrets}");
    let started = Instant::now();
    let mut next = Instant::now();
    let mut found = None;
    event_loop.run_return(|event, _, flow| {
        *flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(500));
        if let Event::WindowEvent { event: WindowEvent::CloseRequested, .. } = event {
            *flow = ControlFlow::Exit;
            return;
        }
        if Instant::now() < next {
            return;
        }
        next = Instant::now() + Duration::from_secs(1);
        if started.elapsed() > TIMEOUT {
            *flow = ControlFlow::Exit;
            return;
        }
        if let Ok(cookies) = webview.cookies_for_url(SITE)
            && let Some(c) = cookies.iter().find(|c| c.name() == COOKIE && !c.value().is_empty())
        {
            found = Some(Zeroizing::new(c.value().to_owned()));
            *flow = ControlFlow::Exit;
        }
    });
    drop(webview);
    drop(window);
    found
}

/// Runs the sign-in and returns the exit code.
pub fn run(secrets: &str) -> i32 {
    let Some(cookie) = cookie(secrets) else { return CANCELLED };
    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(_) => return FAILED,
    };
    let source = match Crunchyroll::new(Arc::new(Keyring::new(secrets))) {
        Ok(s) => s,
        Err(_) => return FAILED,
    };
    match rt.block_on(source.connect(&cookie)) {
        Ok(()) => SIGNED_IN,
        Err(kuro_api::ApiError::Unauthorized) => EXPIRED,
        Err(e) => {
            // Error messages never carry addresses, cookies or tokens.
            tracing::error!("Crunchyroll sign-in failed: {e}");
            FAILED
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_only_goes_to_crunchyroll() {
        assert!(allowed("https://www.crunchyroll.com/login"));
        assert!(allowed("https://sso.crunchyroll.com/authorize?x=1"));
        assert!(!allowed("http://www.crunchyroll.com/login"));
        assert!(!allowed("https://www.crunchyroll.com.evil.example/"));
        assert!(!allowed("https://evil.example/?https://www.crunchyroll.com"));
        assert!(!allowed("file:///c:/windows"));
    }
}
