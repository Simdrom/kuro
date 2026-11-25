use serde::{Deserialize, Serialize};

use crate::list::SortKey;
use crate::model::{MediaKind, MediaRef, Service, TitleLanguage};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
    /// Pure black background.
    Black,
}

impl Theme {
    pub const ALL: [Self; 4] = [Self::System, Self::Light, Self::Dark, Self::Black];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Tint {
    #[default]
    Blue,
    Mango,
    Green,
    Teal,
    Indigo,
    Purple,
    Pink,
    Red,
}

impl Tint {
    pub const ALL: [Self; 8] =
        [Self::Blue, Self::Mango, Self::Green, Self::Teal, Self::Indigo, Self::Purple, Self::Pink, Self::Red];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AppIcon {
    #[default]
    Sunrise,
    Night,
    Leaf,
    Candy,
    Mono,
}

impl AppIcon {
    pub const ALL: [Self; 5] = [Self::Sunrise, Self::Night, Self::Leaf, Self::Candy, Self::Mono];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ScoreDisplay {
    None,
    Mine,
    #[default]
    Average,
    Both,
}

impl ScoreDisplay {
    pub const ALL: [Self; 4] = [Self::None, Self::Mine, Self::Average, Self::Both];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PosterProgress {
    None,
    Badge,
    Bar,
    #[default]
    Both,
}

impl PosterProgress {
    pub const ALL: [Self; 4] = [Self::None, Self::Badge, Self::Bar, Self::Both];
}

/// What a swipe, double-click or middle-click on a title does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GestureAction {
    #[default]
    None,
    Increment,
    Decrement,
    IncrementVolume,
    Edit,
    Complete,
    Drop,
    Pause,
    OpenWeb,
    CopyLink,
    Binge,
}

impl GestureAction {
    pub const ALL: [Self; 11] = [
        Self::None,
        Self::Increment,
        Self::Decrement,
        Self::IncrementVolume,
        Self::Edit,
        Self::Complete,
        Self::Drop,
        Self::Pause,
        Self::OpenWeb,
        Self::CopyLink,
        Self::Binge,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Gestures {
    pub swipe_right: GestureAction,
    pub swipe_left: GestureAction,
    pub double_click: GestureAction,
    pub middle_click: GestureAction,
}

impl Default for Gestures {
    fn default() -> Self {
        Self {
            swipe_right: GestureAction::Increment,
            swipe_left: GestureAction::Edit,
            double_click: GestureAction::None,
            middle_click: GestureAction::Increment,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MusicSearch {
    Off,
    Spotify,
    AppleMusic,
    #[default]
    Both,
}

impl MusicSearch {
    pub const ALL: [Self; 4] = [Self::Off, Self::Spotify, Self::AppleMusic, Self::Both];
    pub fn apple(self) -> bool {
        matches!(self, Self::AppleMusic | Self::Both)
    }
    pub fn spotify(self) -> bool {
        matches!(self, Self::Spotify | Self::Both)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationSettings {
    pub enabled: bool,
    pub episodes: bool,
    pub premieres: bool,
    pub finished: bool,
    pub include_planning: bool,
    pub delay_minutes: u32,
    /// Hour used when only the date is known.
    pub date_hour: u32,
    pub muted: Vec<MediaRef>,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            episodes: true,
            premieres: true,
            finished: true,
            include_planning: true,
            delay_minutes: 0,
            date_hour: 10,
            muted: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    // General
    /// UI language code; empty follows the system.
    pub language: String,
    pub default_kind: MediaKind,
    pub sidebar: bool,
    pub separate_tabs: bool,
    pub season_icon: bool,
    pub show_adult: bool,
    pub show_recommendations: bool,
    pub show_reviews: bool,
    pub show_updated: bool,
    pub show_edit_button: bool,
    pub show_themes: bool,
    pub music: MusicSearch,
    pub title_language: TitleLanguage,
    pub confirm_delete: bool,
    pub watch_clipboard: bool,
    // My List
    pub score_display: ScoreDisplay,
    pub list_grid: bool,
    pub seasonal_grid: bool,
    pub poster_width: f32,
    pub poster_progress: PosterProgress,
    pub poster_scores: bool,
    pub sort: SortKey,
    pub sort_asc: bool,
    // Appearance
    pub theme: Theme,
    pub tint: Tint,
    pub icon: AppIcon,
    pub zoom: f32,
    pub confetti: bool,
    pub animations: bool,
    // Accounts
    pub primary: Service,
    pub save_both: bool,
    pub mal_client_id: String,
    pub anilist_client_id: String,
    // System
    pub close_to_tray: bool,
    pub autostart: bool,
    /// Handle `mango://` links.
    pub handle_links: bool,
    pub gestures: Gestures,
    pub notifications: NotificationSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: String::new(),
            default_kind: MediaKind::Anime,
            sidebar: true,
            separate_tabs: true,
            season_icon: false,
            show_adult: false,
            show_recommendations: true,
            show_reviews: true,
            show_updated: false,
            show_edit_button: true,
            show_themes: true,
            music: MusicSearch::Both,
            title_language: TitleLanguage::Romaji,
            confirm_delete: true,
            watch_clipboard: false,
            score_display: ScoreDisplay::Both,
            list_grid: false,
            seasonal_grid: false,
            poster_width: 130.0,
            poster_progress: PosterProgress::Both,
            poster_scores: true,
            sort: SortKey::Updated,
            sort_asc: false,
            theme: Theme::System,
            tint: Tint::Blue,
            icon: AppIcon::Sunrise,
            zoom: 1.0,
            confetti: true,
            animations: true,
            primary: Service::AniList,
            save_both: true,
            mal_client_id: String::new(),
            anilist_client_id: String::new(),
            close_to_tray: false,
            autostart: false,
            handle_links: false,
            gestures: Gestures::default(),
            notifications: NotificationSettings::default(),
        }
    }
}

impl Settings {
    /// Clamps values that could come from a damaged or edited file.
    pub fn normalized(mut self) -> Self {
        let fin = |v: f32, d: f32| if v.is_finite() { v } else { d };
        self.poster_width = fin(self.poster_width, 130.0).clamp(90.0, 240.0);
        self.zoom = fin(self.zoom, 1.0).clamp(0.75, 1.75);
        self.notifications.date_hour = self.notifications.date_hour.min(23);
        self.notifications.delay_minutes = self.notifications.delay_minutes.min(24 * 60);
        for id in [&mut self.mal_client_id, &mut self.anilist_client_id] {
            *id = id.trim().to_owned();
            if id.len() > 128 || !id.chars().all(|c| c.is_ascii_alphanumeric()) {
                id.clear();
            }
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_json_uses_defaults() {
        let s: Settings = serde_json::from_str(r#"{"tint":"pink"}"#).unwrap();
        assert_eq!(s.tint, Tint::Pink);
        assert!(s.save_both);
    }

    #[test]
    fn normalizes_hostile_values() {
        let s = Settings {
            zoom: f32::NAN,
            poster_width: 1e9,
            mal_client_id: "a b&c=1".into(),
            anilist_client_id: " 123 ".into(),
            ..Settings::default()
        }
        .normalized();
        assert_eq!((s.zoom, s.poster_width), (1.0, 240.0));
        assert_eq!((s.mal_client_id.as_str(), s.anilist_client_id.as_str()), ("", "123"));
    }
}
