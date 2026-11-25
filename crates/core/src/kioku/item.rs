//! What a streaming platform says was watched, before it is matched to a title.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Crunchyroll,
    Netflix,
    DisneyPlus,
    Amazon,
}

impl Platform {
    pub const ALL: [Self; 4] = [Self::Crunchyroll, Self::Netflix, Self::DisneyPlus, Self::Amazon];

    pub fn key(self) -> &'static str {
        match self {
            Self::Crunchyroll => "crunchyroll",
            Self::Netflix => "netflix",
            Self::DisneyPlus => "disney_plus",
            Self::Amazon => "amazon",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Crunchyroll => "Crunchyroll",
            Self::Netflix => "Netflix",
            Self::DisneyPlus => "Disney+",
            Self::Amazon => "Prime Video",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.key() == key)
    }
}

/// How much of an episode was played.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Watched {
    /// The platform says it was finished.
    Completed,
    /// Share played, from 0 to 1.
    Fraction(f32),
}

impl Watched {
    /// Played to the end, or nearly: credits are often skipped.
    pub const ENOUGH: f32 = 0.9;

    pub fn counts(self) -> bool {
        match self {
            Self::Completed => true,
            Self::Fraction(f) => f >= Self::ENOUGH,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WatchedItem {
    pub platform: Platform,
    pub series_title: String,
    /// Season number as the platform counts it, which may differ from the
    /// separate entries AniList and MyAnimeList keep per season.
    pub season: Option<u32>,
    /// Title the platform gives the season, often with its own subtitle.
    pub season_title: Option<String>,
    /// `None` for films and for specials without a number.
    pub episode: Option<u32>,
    pub episode_title: Option<String>,
    pub movie: bool,
    /// Unix seconds.
    pub watched_at: Option<i64>,
    pub progress: Watched,
    /// Stable id on the platform, the same for every audio version.
    pub external_id: String,
}
