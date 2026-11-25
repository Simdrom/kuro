//! Text for domain values, in the interface language.

use jiff::tz::TimeZone;
use jiff::{Timestamp, ZonedDateTime};
use mango_api::{ApiError, Ranking};
use mango_core::list::SortKey;
use mango_core::overview::Section;
use mango_core::settings::{AppIcon, GestureAction, MusicSearch, PosterProgress, ScoreDisplay, Theme, Tint};
use mango_core::{
    Format, FuzzyDate, ListStatus, Media, MediaKind, RelationKind, ReleaseStatus, Season, SeasonYear,
    TitleLanguage,
};

use crate::i18n::{current, number, t, tf};
use crate::icons;

pub fn status(s: ListStatus, k: MediaKind) -> &'static str {
    match (s, k) {
        (ListStatus::Current, MediaKind::Anime) => t("Watching"),
        (ListStatus::Current, MediaKind::Manga) => t("Reading"),
        (ListStatus::Completed, _) => t("Completed"),
        (ListStatus::Paused, _) => t("On hold"),
        (ListStatus::Dropped, _) => t("Dropped"),
        (ListStatus::Planning, MediaKind::Anime) => t("Plan to watch"),
        (ListStatus::Planning, MediaKind::Manga) => t("Plan to read"),
    }
}

pub fn status_icon(s: ListStatus) -> &'static str {
    match s {
        ListStatus::Current => icons::PLAY,
        ListStatus::Completed => icons::CHECK,
        ListStatus::Paused => icons::HAND_PALM,
        ListStatus::Dropped => icons::X,
        ListStatus::Planning => icons::CALENDAR_BLANK,
    }
}

pub fn kind(k: MediaKind) -> &'static str {
    match k {
        MediaKind::Anime => t("Anime"),
        MediaKind::Manga => t("Manga"),
    }
}

pub fn season(s: Season) -> &'static str {
    match s {
        Season::Winter => t("Winter"),
        Season::Spring => t("Spring"),
        Season::Summer => t("Summer"),
        Season::Fall => t("Fall"),
    }
}

pub fn season_icon(s: Season) -> &'static str {
    match s {
        Season::Winter => icons::SNOWFLAKE,
        Season::Spring => icons::FLOWER,
        Season::Summer => icons::SUN,
        Season::Fall => icons::LEAF,
    }
}

pub fn season_year(s: SeasonYear) -> String {
    format!("{} {}", season(s.season), s.year)
}

pub fn release(r: ReleaseStatus, k: MediaKind) -> &'static str {
    match (r, k) {
        (ReleaseStatus::Finished, _) => t("Finished"),
        (ReleaseStatus::Releasing, MediaKind::Anime) => t("Airing"),
        (ReleaseStatus::Releasing, MediaKind::Manga) => t("Publishing"),
        (ReleaseStatus::NotYetReleased, _) => t("Not yet released"),
        (ReleaseStatus::Cancelled, _) => t("Cancelled"),
        (ReleaseStatus::Hiatus, _) => t("On hiatus"),
        (ReleaseStatus::Unknown, _) => t("Unknown"),
    }
}

pub fn sort(k: SortKey) -> &'static str {
    match k {
        SortKey::Updated => t("Last updated"),
        SortKey::Title => t("Title"),
        SortKey::MyScore => t("My score"),
        SortKey::MeanScore => t("Average score"),
        SortKey::Progress => t("Progress"),
        SortKey::StartDate => t("Date started"),
        SortKey::NextEpisode => t("Next episode"),
        SortKey::ReleaseDate => t("Release date"),
    }
}

pub fn ranking(r: Ranking) -> &'static str {
    match r {
        Ranking::All => t("Top rated"),
        Ranking::Airing => t("Airing"),
        Ranking::Upcoming => t("Upcoming"),
        Ranking::Popularity => t("Most popular"),
        Ranking::Favorites => t("Most favorited"),
        Ranking::Trending => t("Trending"),
        Ranking::Format(f) => format(f),
    }
}

pub fn format(f: Format) -> &'static str {
    match f {
        Format::Movie => t("Movie"),
        Format::Special => t("Special"),
        Format::Music => t("Music"),
        Format::Novel => t("Light novel"),
        Format::Manga => t("Manga"),
        Format::Unknown => t("Unknown"),
        other => other.label(),
    }
}

pub fn section(s: Section, k: MediaKind) -> &'static str {
    match (s, k) {
        (Section::Main, MediaKind::Anime) => t("Seasons"),
        (Section::Main, MediaKind::Manga) => t("Main series"),
        (Section::Movies, _) => t("Movies"),
        (Section::SideStories, _) => t("Side stories & spin-offs"),
        (Section::Other, _) => t("Recaps & alternatives"),
    }
}

pub fn relation(r: RelationKind) -> &'static str {
    match r {
        RelationKind::Sequel => t("Sequel"),
        RelationKind::Prequel => t("Prequel"),
        RelationKind::Parent => t("Main story"),
        RelationKind::SideStory => t("Side story"),
        RelationKind::SpinOff => t("Spin-off"),
        RelationKind::Alternative => t("Alternative"),
        RelationKind::Adaptation => t("Adaptation"),
        RelationKind::Source => t("Original work"),
        RelationKind::Summary => t("Recap"),
        RelationKind::Character => t("Shared characters"),
        RelationKind::Compilation => t("Compilation"),
        RelationKind::Contains => t("Contains"),
        RelationKind::Other => t("Other"),
    }
}

pub fn gesture(g: GestureAction) -> &'static str {
    match g {
        GestureAction::None => t("Nothing"),
        GestureAction::Increment => t("Add one episode or chapter"),
        GestureAction::Decrement => t("Remove one episode or chapter"),
        GestureAction::IncrementVolume => t("Add one volume"),
        GestureAction::Edit => t("Edit"),
        GestureAction::Complete => t("Mark as completed"),
        GestureAction::Drop => t("Mark as dropped"),
        GestureAction::Pause => t("Put on hold"),
        GestureAction::OpenWeb => t("Open in browser"),
        GestureAction::CopyLink => t("Copy link"),
        GestureAction::Binge => t("Start binge session"),
    }
}

pub fn gesture_icon(g: GestureAction) -> &'static str {
    match g {
        GestureAction::None => "",
        GestureAction::Increment => icons::PLUS,
        GestureAction::Decrement => icons::MINUS,
        GestureAction::IncrementVolume => icons::BOOK,
        GestureAction::Edit => icons::PENCIL_SIMPLE,
        GestureAction::Complete => icons::CHECK,
        GestureAction::Drop => icons::X,
        GestureAction::Pause => icons::HAND_PALM,
        GestureAction::OpenWeb => icons::ARROW_SQUARE_OUT,
        GestureAction::CopyLink => icons::COPY,
        GestureAction::Binge => icons::LIGHTNING,
    }
}

pub fn theme(v: Theme) -> &'static str {
    match v {
        Theme::System => t("System"),
        Theme::Light => t("Light"),
        Theme::Dark => t("Dark"),
        Theme::Black => t("Black"),
    }
}

pub fn tint(v: Tint) -> &'static str {
    match v {
        Tint::Blue => t("Blue"),
        Tint::Mango => t("Mango"),
        Tint::Green => t("Green"),
        Tint::Teal => t("Teal"),
        Tint::Indigo => t("Indigo"),
        Tint::Purple => t("Purple"),
        Tint::Pink => t("Pink"),
        Tint::Red => t("Red"),
    }
}

pub fn app_icon(v: AppIcon) -> &'static str {
    match v {
        AppIcon::Sunrise => t("Sunrise"),
        AppIcon::Night => t("Night"),
        AppIcon::Leaf => t("Leaf"),
        AppIcon::Candy => t("Candy"),
        AppIcon::Mono => t("Mono"),
    }
}

pub fn score_display(v: ScoreDisplay) -> &'static str {
    match v {
        ScoreDisplay::None => t("None"),
        ScoreDisplay::Mine => t("My score"),
        ScoreDisplay::Average => t("Average score"),
        ScoreDisplay::Both => t("Both"),
    }
}

pub fn poster_progress(v: PosterProgress) -> &'static str {
    match v {
        PosterProgress::None => t("None"),
        PosterProgress::Badge => t("Badge"),
        PosterProgress::Bar => t("Progress bar"),
        PosterProgress::Both => t("Both"),
    }
}

pub fn music(v: MusicSearch) -> &'static str {
    match v {
        MusicSearch::Off => t("Off"),
        MusicSearch::Spotify => "Spotify",
        MusicSearch::AppleMusic => "Apple Music",
        MusicSearch::Both => t("Both"),
    }
}

pub fn title_language(v: TitleLanguage) -> &'static str {
    match v {
        TitleLanguage::Romaji => t("Romaji"),
        TitleLanguage::English => t("English"),
        TitleLanguage::Native => t("Japanese"),
    }
}

pub fn error(e: &ApiError) -> String {
    match e {
        ApiError::Network(_) => t("No connection. Check your network and try again.").into(),
        ApiError::RateLimited => t("Too many requests. Wait a minute and try again.").into(),
        ApiError::Unauthorized => t("Your session has expired. Sign in again.").into(),
        ApiError::NotFound => t("Not found").into(),
        ApiError::NotConfigured => t("Add a client ID in Settings › Accounts to use this service.").into(),
        ApiError::Status(s) => tf("The service is not available right now ({}).", &[s]),
        ApiError::Auth(m) if m == "cancelled" => t("Sign-in was cancelled.").into(),
        other => tf("Something went wrong: {}", &[other]),
    }
}

pub fn date(d: FuzzyDate) -> String {
    let full = FuzzyDate::new(d.year, d.month, Some(d.day.unwrap_or(1))).and_then(|f| f.exact());
    match full {
        Some(n) => current().date(n, d.day.is_some()),
        None => d.year.to_string(),
    }
}

pub fn month(d: FuzzyDate) -> String {
    date(FuzzyDate { day: None, ..d })
}

fn local(at: i64) -> Option<ZonedDateTime> {
    Some(Timestamp::from_second(at).ok()?.to_zoned_datetime(TimeZone::system()))
}

fn clock(z: &ZonedDateTime) -> String {
    format!("{:02}:{:02}", z.hour(), z.minute())
}

pub fn day(at: i64) -> String {
    local(at).map(|z| current().date(z.date(), true)).unwrap_or_default()
}

pub fn date_time(at: i64) -> String {
    local(at).map(|z| format!("{} {}", current().date(z.date(), true), clock(&z))).unwrap_or_default()
}

/// `Thu 17:26`, or the date when more than a week away.
pub fn airing_time(at: i64, now: i64) -> String {
    let Some(z) = local(at) else { return String::new() };
    let l = current();
    if at - now < 7 * 86_400 {
        return format!("{} {}", l.weekday(z.weekday()), clock(&z));
    }
    match l.code {
        "ko" => format!("{} {}일 {}", l.month(z.month()), z.day(), clock(&z)),
        _ => format!("{} {} {}", z.day(), l.month(z.month()), clock(&z)),
    }
}

pub fn countdown(at: i64, now: i64) -> String {
    let s = (at - now).max(0);
    let (d, h, m) = (s / 86_400, s / 3600 % 24, s / 60 % 60);
    match (d, h) {
        (0, 0) => tf("{} min", &[&m]),
        (0, _) => tf("{} h {} min", &[&h, &m]),
        _ => tf("{} d {} h", &[&d, &h]),
    }
}

pub fn length(m: &Media) -> String {
    let n = |v: Option<u32>| v.map_or_else(|| "?".to_owned(), |v| number(v.into()));
    match m.kind {
        MediaKind::Anime => tf("{} eps", &[&n(m.episodes)]),
        MediaKind::Manga => tf("{} vol, {} ch", &[&n(m.volumes), &n(m.chapters)]),
    }
}

pub fn dates(m: &Media) -> String {
    match (m.start, m.end) {
        (Some(a), Some(b)) if month(a) == month(b) => month(a),
        (Some(a), Some(b)) => format!("{} – {}", month(a), month(b)),
        (Some(a), None) => month(a),
        _ => String::new(),
    }
}

pub fn bytes(n: u64) -> String {
    let mb = n as f32 / (1024.0 * 1024.0);
    format!("{} MB", crate::i18n::decimal(mb, 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn countdowns() {
        assert_eq!(countdown(100, 200), "0 min");
        assert_eq!(countdown(3_700, 0), "1 h 1 min");
        assert_eq!(countdown(90_000, 0), "1 d 1 h");
    }

    #[test]
    fn date_ranges_collapse() {
        let m = Media {
            start: FuzzyDate::parse("2023-09-29"),
            end: FuzzyDate::parse("2024-03-22"),
            ..Media::default()
        };
        assert_eq!(dates(&m), "Sep 2023 – Mar 2024");
        let m = Media { end: m.start, ..m };
        assert_eq!(dates(&m), "Sep 2023");
        assert_eq!(dates(&Media { start: FuzzyDate::parse("2023"), ..Media::default() }), "2023");
    }
}
