//! Kioku: reads what was watched on streaming platforms.
//!
//! A platform either lets Kuro read the history with the user's account
//! ([`Mode::Account`]) or hands the user a file to import ([`Mode::File`]).
//! Sources only read: matching and writing to the list happen elsewhere.

pub mod amazon;
pub mod crunchyroll;
pub mod disney;
pub mod netflix;

use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;

use kuro_core::kioku::{Platform, WatchedItem};

use crate::http::{ApiError, Result};

/// Sent with every request to a platform.
pub const USER_AGENT: &str = concat!("Kuro-Kioku/", env!("CARGO_PKG_VERSION"));

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Told how many pages have been read so far.
pub type OnPage = dyn Fn(u32) + Send + Sync;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Signs in to the platform and reads the history.
    Account,
    /// Imports a file the platform gives out, such as a personal data export.
    File,
}

pub trait HistorySource: Send + Sync {
    fn platform(&self) -> Platform;

    fn mode(&self) -> Mode;

    /// Account mode: whether a session is stored.
    fn connected(&self) -> bool {
        false
    }

    /// Account mode: the history, newest first. Reading stops at the first
    /// page whose items are all in `known`. `on_page` follows the reading.
    fn fetch<'a>(
        &'a self,
        known: &'a HashSet<String>,
        on_page: &'a OnPage,
    ) -> BoxFuture<'a, Result<Vec<WatchedItem>>> {
        let _ = (known, on_page);
        Box::pin(async { Err(ApiError::Unavailable) })
    }

    /// Account mode: forgets the stored session.
    fn disconnect(&self) -> Result<()> {
        Ok(())
    }

    /// File mode: the items in an export.
    fn parse(&self, data: &[u8]) -> Result<Vec<WatchedItem>> {
        let _ = data;
        Err(ApiError::Unavailable)
    }
}

/// Random identifier in UUID v4 form.
pub(crate) fn uuid() -> Result<String> {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).map_err(|e| ApiError::Service(e.to_string()))?;
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    Ok(format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_agent_names_kuro_and_its_version() {
        assert!(USER_AGENT.starts_with("Kuro-Kioku/") && USER_AGENT.len() > "Kuro-Kioku/".len());
    }

    #[test]
    fn uuids_are_random_v4() {
        let (a, b) = (uuid().unwrap(), uuid().unwrap());
        assert_ne!(a, b);
        assert_eq!((a.len(), &a[14..15]), (36, "4"));
    }
}
