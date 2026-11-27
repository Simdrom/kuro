use serde::{Deserialize, Serialize};

use crate::friends::{Friend, MAX_FRIENDS, valid_name};
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
    #[serde(alias = "mango")]
    Amber,
    Green,
    Teal,
    Indigo,
    Purple,
    Pink,
    Red,
}

impl Tint {
    pub const ALL: [Self; 8] =
        [Self::Blue, Self::Amber, Self::Green, Self::Teal, Self::Indigo, Self::Purple, Self::Pink, Self::Red];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AppIcon {
    #[default]
    #[serde(alias = "sunrise")]
    Ink,
    Night,
    Leaf,
    Candy,
    Mono,
}

impl AppIcon {
    pub const ALL: [Self; 5] = [Self::Ink, Self::Night, Self::Leaf, Self::Candy, Self::Mono];
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

/// How titles are laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ListLayout {
    #[default]
    Cards,
    Posters,
    Table,
}

impl ListLayout {
    pub const ALL: [Self; 3] = [Self::Cards, Self::Posters, Self::Table];
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
    /// Days without progress before a title being watched is mentioned; 0 never.
    pub stale_days: u32,
    /// A summary of the week on Monday.
    pub weekly: bool,
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
            stale_days: 0,
            weekly: false,
        }
    }
}

/// What the desktop widget shows. It stays open across restarts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WidgetSettings {
    pub open: bool,
    /// The next episodes to air, with a countdown.
    pub up_next: bool,
    pub watching: bool,
    pub stats: bool,
    /// Titles shown in each group.
    pub rows: u32,
}

impl Default for WidgetSettings {
    fn default() -> Self {
        Self { open: false, up_next: true, watching: true, stats: true, rows: 5 }
    }
}

/// How often something runs on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Every {
    #[default]
    Off,
    Daily,
    Weekly,
}

impl Every {
    pub const ALL: [Self; 3] = [Self::Off, Self::Daily, Self::Weekly];

    /// The day or ISO week `d` falls in, which runs once; `None` when off.
    pub fn period(self, d: jiff::civil::Date) -> Option<String> {
        match self {
            Self::Off => None,
            Self::Daily => Some(d.to_string()),
            Self::Weekly => {
                let w = d.iso_week_date();
                Some(format!("{}-W{:02}", w.year(), w.week()))
            }
        }
    }
}

/// A calendar Kuro made in the Google account.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GoogleCalendar {
    pub id: String,
    pub name: String,
}

/// The name of the first calendar Kuro makes.
pub const GOOGLE_CALENDAR_NAME: &str = "Kuro";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BackupSettings {
    pub every: Every,
    /// Empty until a folder is chosen.
    pub folder: String,
    /// Automatic copies kept in the folder.
    pub keep: u32,
}

impl Default for BackupSettings {
    fn default() -> Self {
        Self { every: Every::Off, folder: String::new(), keep: 10 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Custom lists made here, kept even while no title is in them.
    pub custom_lists: Vec<String>,
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
    pub list_layout: ListLayout,
    pub browse_layout: ListLayout,
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
    /// Handle `kuro://` links.
    pub handle_links: bool,
    /// Links may change the list, such as `kuro://step/…`.
    pub link_actions: bool,
    pub gestures: Gestures,
    pub notifications: NotificationSettings,
    // Automation
    pub widget: WidgetSettings,
    pub backup: BackupSettings,
    /// Reads the list from the account again this often; 0 never does.
    pub refresh_hours: u32,
    /// Kioku reads the Crunchyroll history on its own this often.
    pub kioku_check: Every,
    /// Kioku looks for streaming history files in the Downloads folder.
    pub watch_downloads: bool,
    /// An iCalendar file kept up to date with upcoming episodes; empty for none.
    pub calendar_file: String,
    /// The Google account the episode calendar is kept in; empty for none.
    pub google_account: String,
    /// The calendar the episodes go to: one of `google_calendars`, or empty
    /// until the first one is made.
    pub google_calendar: String,
    /// The calendars Kuro made in that account. It can only reach those.
    pub google_calendars: Vec<GoogleCalendar>,
    /// People whose lists the Friends tab shows.
    pub friends: Vec<Friend>,
    /// Asks GitHub now and then whether a newer Kuro is out.
    pub check_updates: bool,
    /// A version the user chose not to be told about again.
    pub skipped_version: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            custom_lists: Vec::new(),
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
            list_layout: ListLayout::Cards,
            browse_layout: ListLayout::Cards,
            poster_width: 130.0,
            poster_progress: PosterProgress::Both,
            poster_scores: true,
            sort: SortKey::Updated,
            sort_asc: false,
            theme: Theme::System,
            tint: Tint::Blue,
            icon: AppIcon::Ink,
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
            link_actions: false,
            gestures: Gestures::default(),
            notifications: NotificationSettings::default(),
            widget: WidgetSettings::default(),
            backup: BackupSettings::default(),
            refresh_hours: 0,
            kioku_check: Every::Off,
            watch_downloads: false,
            calendar_file: String::new(),
            google_account: String::new(),
            google_calendar: String::new(),
            google_calendars: Vec::new(),
            friends: Vec::new(),
            check_updates: true,
            skipped_version: String::new(),
        }
    }
}

impl Settings {
    /// Whether the list should be read from the account again, last read at `last`.
    pub fn refresh_due(&self, last: i64, now: i64) -> bool {
        self.refresh_hours > 0 && now - last >= i64::from(self.refresh_hours) * 3600
    }

    /// Clamps values that could come from a damaged or edited file.
    pub fn normalized(mut self) -> Self {
        let fin = |v: f32, d: f32| if v.is_finite() { v } else { d };
        self.poster_width = fin(self.poster_width, 130.0).clamp(90.0, 240.0);
        self.zoom = fin(self.zoom, 1.0).clamp(0.75, 1.75);
        self.notifications.date_hour = self.notifications.date_hour.min(23);
        self.notifications.delay_minutes = self.notifications.delay_minutes.min(24 * 60);
        self.notifications.stale_days = self.notifications.stale_days.min(365);
        self.refresh_hours = self.refresh_hours.min(24 * 7);
        self.widget.rows = self.widget.rows.clamp(1, 20);
        self.backup.keep = self.backup.keep.clamp(1, 100);
        for path in [
            &mut self.backup.folder,
            &mut self.calendar_file,
            &mut self.google_account,
            &mut self.google_calendar,
        ] {
            if path.len() > 1024 || path.chars().any(char::is_control) {
                path.clear();
            }
        }
        self.google_calendars
            .retain(|c| !c.id.is_empty() && c.id.len() <= 1024 && !c.id.contains(char::is_control));
        self.google_calendars.truncate(50);
        let mut friends: Vec<Friend> = Vec::new();
        for f in std::mem::take(&mut self.friends) {
            if valid_name(&f.name) && !friends.iter().any(|x| x.same(&f)) {
                friends.push(f);
            }
        }
        friends.truncate(MAX_FRIENDS);
        self.friends = friends;
        for c in &mut self.google_calendars {
            c.name =
                c.name.chars().filter(|c| !c.is_control()).take(100).collect::<String>().trim().to_owned();
            if c.name.is_empty() {
                c.name = GOOGLE_CALENDAR_NAME.into();
            }
        }
        // The calendar made before Kuro could keep more than one.
        if !self.google_calendar.is_empty()
            && !self.google_calendars.iter().any(|c| c.id == self.google_calendar)
        {
            let id = self.google_calendar.clone();
            self.google_calendars.push(GoogleCalendar { id, name: GOOGLE_CALENDAR_NAME.into() });
        }
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
    fn the_first_google_calendar_joins_the_list() {
        let s = Settings { google_calendar: "abc@group".into(), ..Settings::default() }.normalized();
        assert_eq!(s.google_calendars, [GoogleCalendar { id: "abc@group".into(), name: "Kuro".into() }]);
        assert_eq!(s.clone().normalized(), s, "stable");
        let named = GoogleCalendar {
            id: "x".into(),
            name: " 
Anime "
                .into(),
        };
        let s = Settings { google_calendars: vec![named], ..Settings::default() }.normalized();
        assert_eq!(s.google_calendars[0].name, "Anime");
    }

    #[test]
    fn partial_json_uses_defaults() {
        let s: Settings = serde_json::from_str(r#"{"tint":"pink"}"#).unwrap();
        assert_eq!(s.tint, Tint::Pink);
        assert!(s.save_both);
    }

    #[test]
    fn settings_saved_under_the_old_name_still_load() {
        let s: Settings = serde_json::from_str(r#"{"tint":"mango","icon":"sunrise","zoom":1.25}"#).unwrap();
        assert_eq!((s.tint, s.icon, s.zoom), (Tint::Amber, AppIcon::Ink, 1.25));
    }

    #[test]
    fn the_list_is_read_again_only_when_asked_and_due() {
        let mut s = Settings::default();
        assert!(!s.refresh_due(0, 1_000_000));
        s.refresh_hours = 3;
        assert!(!s.refresh_due(1_000_000, 1_000_000 + 3 * 3600 - 1));
        assert!(s.refresh_due(1_000_000, 1_000_000 + 3 * 3600));
    }

    #[test]
    fn periods_run_once_a_day_or_a_week() {
        let d = jiff::civil::date(2027, 1, 20);
        assert_eq!(Every::Off.period(d), None);
        assert_eq!(Every::Daily.period(d).as_deref(), Some("2027-01-20"));
        assert_eq!(Every::Weekly.period(d).as_deref(), Some("2027-W03"));
        assert_eq!(Every::Weekly.period(jiff::civil::date(2027, 1, 18)), Every::Weekly.period(d));
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
