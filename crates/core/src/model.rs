use jiff::civil::Date;
use serde::{Deserialize, Serialize};

use crate::season::{Season, SeasonYear};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    #[default]
    Anime,
    Manga,
}

impl MediaKind {
    pub const ALL: [Self; 2] = [Self::Anime, Self::Manga];
    pub fn key(self) -> &'static str {
        match self {
            Self::Anime => "anime",
            Self::Manga => "manga",
        }
    }
    pub fn other(self) -> Self {
        match self {
            Self::Anime => Self::Manga,
            Self::Manga => Self::Anime,
        }
    }
}

/// A tracking service, which is also the namespace of a media id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Service {
    #[default]
    AniList,
    Mal,
}

impl Service {
    pub const ALL: [Self; 2] = [Self::AniList, Self::Mal];
    pub fn key(self) -> &'static str {
        match self {
            Self::AniList => "anilist",
            Self::Mal => "mal",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::AniList => "AniList",
            Self::Mal => "MyAnimeList",
        }
    }
    pub fn other(self) -> Self {
        match self {
            Self::AniList => Self::Mal,
            Self::Mal => Self::AniList,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MediaRef {
    pub service: Service,
    pub kind: MediaKind,
    pub id: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ListStatus {
    Current,
    Completed,
    Paused,
    Dropped,
    #[default]
    Planning,
}

impl ListStatus {
    pub const ALL: [Self; 5] = [Self::Current, Self::Completed, Self::Paused, Self::Dropped, Self::Planning];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Tv,
    TvShort,
    Movie,
    Special,
    Ova,
    Ona,
    Music,
    Manga,
    Novel,
    OneShot,
    Doujin,
    Manhwa,
    Manhua,
    #[default]
    Unknown,
}

impl Format {
    pub fn label(self) -> &'static str {
        match self {
            Self::Tv => "TV",
            Self::TvShort => "TV Short",
            Self::Movie => "Movie",
            Self::Special => "Special",
            Self::Ova => "OVA",
            Self::Ona => "ONA",
            Self::Music => "Music",
            Self::Manga => "Manga",
            Self::Novel => "Light Novel",
            Self::OneShot => "One-shot",
            Self::Doujin => "Doujinshi",
            Self::Manhwa => "Manhwa",
            Self::Manhua => "Manhua",
            Self::Unknown => "?",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseStatus {
    Finished,
    Releasing,
    NotYetReleased,
    Cancelled,
    Hiatus,
    #[default]
    Unknown,
}

/// A date whose month or day may be unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FuzzyDate {
    pub year: i32,
    pub month: Option<u32>,
    pub day: Option<u32>,
}

fn civil(year: i32, month: u32, day: u32) -> Option<Date> {
    Date::new(year.try_into().ok()?, month.try_into().ok()?, day.try_into().ok()?).ok()
}

impl FuzzyDate {
    /// Builds a date, dropping components that are out of range.
    pub fn new(year: i32, month: Option<u32>, day: Option<u32>) -> Option<Self> {
        if !(1..=9999).contains(&year) {
            return None;
        }
        let month = month.filter(|m| (1..=12).contains(m));
        let day = month.and_then(|m| day.filter(|d| civil(year, m, *d).is_some()));
        Some(Self { year, month, day })
    }

    /// Parses `YYYY`, `YYYY-MM` or `YYYY-MM-DD`.
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.trim().split('-');
        let year = parts.next()?.parse().ok()?;
        let month = parts.next().and_then(|p| p.parse().ok());
        let day = parts.next().and_then(|p| p.parse().ok());
        if parts.next().is_some() {
            return None;
        }
        Self::new(year, month, day)
    }

    pub fn from_date(d: Date) -> Self {
        Self { year: d.year().into(), month: Some(d.month() as u32), day: Some(d.day() as u32) }
    }

    /// The exact calendar date, when all components are known.
    pub fn exact(&self) -> Option<Date> {
        civil(self.year, self.month?, self.day?)
    }

    pub fn season(&self) -> Option<SeasonYear> {
        Some(SeasonYear { season: Season::of_month(self.month?), year: self.year })
    }

    pub fn iso(&self) -> String {
        match (self.month, self.day) {
            (Some(m), Some(d)) => format!("{:04}-{m:02}-{d:02}", self.year),
            (Some(m), None) => format!("{:04}-{m:02}", self.year),
            _ => format!("{:04}", self.year),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Airing {
    pub episode: u32,
    /// Unix seconds.
    pub at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Media {
    pub service: Service,
    pub kind: MediaKind,
    pub id: u64,
    pub mal_id: Option<u64>,
    pub anilist_id: Option<u64>,
    pub title: String,
    pub title_english: Option<String>,
    pub title_native: Option<String>,
    pub synonyms: Vec<String>,
    pub cover: Option<String>,
    pub format: Format,
    pub release: ReleaseStatus,
    pub season: Option<SeasonYear>,
    pub start: Option<FuzzyDate>,
    pub end: Option<FuzzyDate>,
    pub episodes: Option<u32>,
    pub chapters: Option<u32>,
    pub volumes: Option<u32>,
    /// Minutes per episode.
    pub duration: Option<u32>,
    pub source: Option<String>,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub studios: Vec<String>,
    /// Community average on a 0-10 scale.
    pub mean_score: Option<f32>,
    pub scored_by: Option<u64>,
    pub members: Option<u64>,
    pub rank: Option<u32>,
    pub popularity: Option<u32>,
    pub synopsis: Option<String>,
    pub background: Option<String>,
    pub rating: Option<String>,
    pub broadcast: Option<String>,
    pub adult: bool,
    pub next_airing: Option<Airing>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TitleLanguage {
    #[default]
    Romaji,
    English,
    Native,
}

impl Media {
    pub fn media_ref(&self) -> MediaRef {
        MediaRef { service: self.service, kind: self.kind, id: self.id }
    }

    /// Total units (episodes or chapters), `None` while unknown.
    pub fn total(&self) -> Option<u32> {
        match self.kind {
            MediaKind::Anime => self.episodes,
            MediaKind::Manga => self.chapters,
        }
        .filter(|n| *n > 0)
    }

    pub fn display_title(&self, lang: TitleLanguage) -> &str {
        let preferred = match lang {
            TitleLanguage::Romaji => None,
            TitleLanguage::English => self.title_english.as_deref(),
            TitleLanguage::Native => self.title_native.as_deref(),
        };
        preferred.filter(|t| !t.is_empty()).unwrap_or(&self.title)
    }

    /// Season of the title, derived from the start date when missing.
    pub fn season_year(&self) -> Option<SeasonYear> {
        self.season.or_else(|| self.start.and_then(|d| d.season()))
    }

    pub fn id_on(&self, service: Service) -> Option<u64> {
        if service == self.service {
            return Some(self.id);
        }
        match service {
            Service::AniList => self.anilist_id,
            Service::Mal => self.mal_id,
        }
    }

    pub fn web_url(&self, service: Service) -> Option<String> {
        let id = self.id_on(service)?;
        let kind = self.kind.key();
        Some(match service {
            Service::AniList => format!("https://anilist.co/{kind}/{id}"),
            Service::Mal => format!("https://myanimelist.net/{kind}/{id}"),
        })
    }

    pub fn matches(&self, query: &str) -> bool {
        let q = query.trim().to_lowercase();
        q.is_empty()
            || std::iter::once(&self.title)
                .chain(self.title_english.iter())
                .chain(self.title_native.iter())
                .chain(self.synonyms.iter())
                .any(|t| t.to_lowercase().contains(&q))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    Sequel,
    Prequel,
    Parent,
    SideStory,
    SpinOff,
    Alternative,
    Adaptation,
    Source,
    Summary,
    Character,
    Compilation,
    Contains,
    #[default]
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Relation {
    pub kind: RelationKind,
    pub media: Media,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Person {
    pub id: u64,
    pub name: String,
    pub image: Option<String>,
    pub language: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CharacterRole {
    pub id: u64,
    pub name: String,
    pub image: Option<String>,
    pub role: String,
    pub voices: Vec<Person>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StaffRole {
    pub person: Person,
    pub role: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Review {
    pub id: u64,
    pub user: String,
    pub summary: String,
    pub body: String,
    pub score: Option<u32>,
    pub votes_up: u32,
    pub votes: u32,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ExternalLink {
    pub site: String,
    pub url: String,
    pub streaming: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeKind {
    Opening,
    Ending,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Theme {
    pub kind: ThemeKind,
    pub title: String,
    pub artist: String,
    pub episodes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct MediaDetail {
    pub media: Media,
    pub relations: Vec<Relation>,
    pub recommendations: Vec<Media>,
    pub characters: Vec<CharacterRole>,
    pub staff: Vec<StaffRole>,
    pub status_stats: Vec<(ListStatus, u64)>,
    /// `(score 10..=100, users)`.
    pub score_stats: Vec<(u32, u64)>,
    pub reviews: Vec<Review>,
    pub links: Vec<ExternalLink>,
    pub themes: Vec<Theme>,
    pub trailer: Option<String>,
    /// Voice languages, native first.
    pub languages: Vec<String>,
}

/// One role in a voice actor's filmography.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct VoiceRole {
    pub character: String,
    pub character_image: Option<String>,
    pub role: String,
    pub media: Media,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PersonDetail {
    pub person: Person,
    pub native_name: Option<String>,
    pub description: Option<String>,
    pub roles: Vec<VoiceRole>,
}

/// The user's state for one title.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ListEntry {
    pub media: Media,
    pub status: ListStatus,
    pub progress: u32,
    pub progress_volumes: u32,
    /// 0-100, where 0 means unscored.
    pub score: u8,
    pub start: Option<FuzzyDate>,
    pub finish: Option<FuzzyDate>,
    pub repeating: bool,
    pub repeat_count: u32,
    pub notes: String,
    /// Unix seconds.
    pub updated_at: i64,
}

impl ListEntry {
    pub fn new(media: Media) -> Self {
        Self { media, ..Self::default() }
    }

    pub fn score10(&self) -> f32 {
        f32::from(self.score) / 10.0
    }

    /// Completed fraction, or `None` when the total is unknown.
    pub fn fraction(&self) -> Option<f32> {
        let total = self.media.total()?;
        Some((self.progress as f32 / total as f32).clamp(0.0, 1.0))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UserProfile {
    pub id: u64,
    pub name: String,
    pub avatar: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_date_rejects_nonsense() {
        assert_eq!(FuzzyDate::parse("2023-09-29").and_then(|d| d.exact()), civil(2023, 9, 29));
        assert_eq!(FuzzyDate::parse("2023-09").map(|d| d.iso()).as_deref(), Some("2023-09"));
        assert_eq!(FuzzyDate::parse("2023").map(|d| d.iso()).as_deref(), Some("2023"));
        assert_eq!(FuzzyDate::parse("2023-13-40").map(|d| d.iso()).as_deref(), Some("2023"));
        assert_eq!(FuzzyDate::parse("2023-02-30").map(|d| d.iso()).as_deref(), Some("2023-02"));
        for bad in ["", "abc", "0", "2023-01-01-01", "99999"] {
            assert_eq!(FuzzyDate::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn title_falls_back_to_romaji() {
        let m = Media { title: "Romaji".into(), title_english: Some(String::new()), ..Media::default() };
        assert_eq!(m.display_title(TitleLanguage::English), "Romaji");
        assert_eq!(m.display_title(TitleLanguage::Native), "Romaji");
    }

    #[test]
    fn zero_totals_are_unknown() {
        let m = Media { episodes: Some(0), ..Media::default() };
        assert_eq!(m.total(), None);
    }

    #[test]
    fn old_cached_json_still_loads() {
        let m: Media = serde_json::from_str(r#"{"id":1,"title":"x"}"#).unwrap();
        assert_eq!(m.id, 1);
    }

    #[test]
    fn cross_service_ids() {
        let m = Media { service: Service::AniList, id: 5, mal_id: Some(9), ..Media::default() };
        assert_eq!(m.id_on(Service::AniList), Some(5));
        assert_eq!(m.id_on(Service::Mal), Some(9));
        assert_eq!(m.web_url(Service::Mal).as_deref(), Some("https://myanimelist.net/anime/9"));
    }
}
