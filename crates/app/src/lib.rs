//! Kuro: interface and system integration.

pub mod app;
pub mod backend;
pub mod confetti;
pub mod format;
pub mod i18n;
pub mod icon;
pub mod icons;
#[cfg(windows)]
pub mod kioku_login;
pub mod system;
pub mod textures;
pub mod theme;
pub mod ui;
pub mod widgets;

/// Kuro's home page; the privacy policy is `privacy.html` beside it.
pub const WEBSITE: &str = "https://simdrom.github.io/kuro/";
