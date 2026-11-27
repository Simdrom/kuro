//! The state of each screen, and what the interface asks for.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
    Anime,
    Manga,
    /// The coming episodes of the user's anime.
    Calendar,
    Seasonal,
    Ranking,
    Search,
    /// Other people's lists.
    Friends,
    Settings,
}

impl Tab {
    pub const ALL: [Self; 8] = [
        Self::Anime,
        Self::Manga,
        Self::Calendar,
        Self::Seasonal,
        Self::Ranking,
        Self::Search,
        Self::Friends,
        Self::Settings,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingsPage {
    General,
    Lists,
    Accounts,
    Notifications,
    Gestures,
    Appearance,
    Widgets,
    Data,
    About,
}

impl SettingsPage {
    pub const ALL: [Self; 9] = [
        Self::General,
        Self::Lists,
        Self::Accounts,
        Self::Notifications,
        Self::Gestures,
        Self::Appearance,
        Self::Widgets,
        Self::Data,
        Self::About,
    ];
}

#[derive(Debug, Clone, PartialEq)]
pub enum Route {
    Media(MediaRef),
    Overview(MediaRef),
    Person(u64),
    Review(MediaRef, u64),
    Settings(SettingsPage),
    /// Kioku: viewing history from streaming platforms.
    Kioku,
    /// A friend's profile and list.
    Friend(Friend),
    /// A closer look at the statistics of one list.
    Stats(MediaKind),
}

/// Something requested by the interface, applied after drawing.
pub enum Act {
    Open(Rc<Media>),
    OpenRef(MediaRef),
    Push(Route),
    Back,
    Go(Tab),
    Gesture(GestureAction, Rc<Media>),
    Step(Rc<Media>, i32),
    StepVolumes(Rc<Media>, i32),
    SetStatus(Rc<Media>, ListStatus),
    Repeat(Rc<Media>),
    Save(Box<ListEntry>),
    Edit(Rc<Media>),
    AskDelete(Rc<Media>),
    Delete(MediaRef),
    /// Puts back the change in the toast.
    Undo,
    Url(String),
    Copy(String),
    Binge(Option<MediaRef>),
    ToggleMute(MediaRef),
    Login(Service),
    CancelLogin,
    AskSignOut(Service),
    SignOut(Service),
    AcceptLogin,
    Refresh,
    Retry(MediaRef),
    Random(MediaKind),
    ShowStatus(MediaKind, ListStatus),
    Import(String),
    KiokuConnect(Platform),
    KiokuAskDisconnect(Platform),
    KiokuDisconnect(Platform),
    KiokuSync(Platform),
    KiokuAskApply,
    KiokuApply,
    /// Leaves an unmatched series out of every later review.
    KiokuDismiss(usize),
    KiokuDismissAll,
    /// Reviews a history file exported from the platform.
    KiokuImport(Platform),
    /// Reviews the history file found in Downloads.
    KiokuImportFound,
    /// Forgets the history file found in Downloads.
    KiokuForgetFound,
    /// Opens the title picker for a series of the review.
    KiokuPick(PickFrom),
    /// Searches with the picker's text.
    KiokuLookup,
    KiokuChoose(Box<Media>),
    /// Adds the friend typed in the Friends tab.
    AddFriend,
    RemoveFriend(Friend),
    /// Adds the people followed on AniList.
    ImportFollowing,
    /// Reads one friend's list again, or everyone's.
    RefreshFriends(Option<Friend>),
}

/// The part of a review a series comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickFrom {
    Unmatched(usize),
    Change(usize),
}

/// Picks the title of a series by hand.
pub struct KiokuPicker {
    pub group: Group,
    pub text: String,
    /// Text of the last search.
    pub sent: String,
    pub busy: bool,
    pub results: Option<Result<Vec<Media>, ApiError>>,
}

/// Kioku's screen.
#[derive(Default)]
pub struct KiokuView {
    /// Whether a Crunchyroll session is stored; asked once, not every frame.
    pub connected: Option<bool>,
    pub connecting: bool,
    pub syncing: bool,
    /// How far the running sync has got.
    pub progress: Option<KiokuStep>,
    /// Interface time the running sync started at.
    pub started: Option<f64>,
    /// Seconds the last sync took.
    pub took: Option<f64>,
    pub review: Option<KiokuReview>,
    pub error: Option<ApiError>,
    pub picker: Option<KiokuPicker>,
    /// Long lists of series not found start folded.
    pub show_unmatched: bool,
    /// The running sync was started on its own, not by the user.
    pub background: bool,
    /// The period last looked at for the automatic check.
    pub checked: String,
    /// A history file in Downloads not reviewed yet, and its key.
    pub found: Option<FoundFile>,
}

/// A streaming history file found in the Downloads folder.
#[derive(Clone)]
pub struct FoundFile {
    pub platform: Platform,
    pub path: std::path::PathBuf,
    /// Remembered once the file is reviewed or forgotten.
    pub key: String,
}

impl FoundFile {
    pub fn name(&self) -> String {
        self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }
}

#[derive(Default)]
pub struct Feed {
    pub items: Vec<Rc<Media>>,
    pub page: u32,
    pub more: bool,
    pub loading: bool,
    pub error: Option<ApiError>,
    pub started: bool,
}

impl Feed {
    pub(super) fn apply(&mut self, page: u32, result: Result<Page<Media>, ApiError>) {
        self.loading = false;
        match result {
            Ok(p) => {
                self.error = None;
                // A page can arrive twice: first cached, then fresh.
                if page <= 1 {
                    self.items.clear();
                } else if page <= self.page {
                    return;
                }
                let known: std::collections::HashSet<_> = self.items.iter().map(|m| m.media_ref()).collect();
                self.items
                    .extend(p.items.into_iter().filter(|m| !known.contains(&m.media_ref())).map(Rc::new));
                self.page = page.max(1);
                self.more = p.more;
            }
            Err(e) => self.error = Some(e),
        }
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Marks the next page as requested and returns its number.
    pub fn begin_page(&mut self) -> u32 {
        self.loading = true;
        self.started = true;
        self.error = None;
        self.page + 1
    }
}

pub struct SeasonView {
    pub at: SeasonYear,
    pub continuing: bool,
    pub feed: Feed,
    pub text: String,
    pub format: Option<kuro_core::Format>,
    pub sort: BrowseSort,
    pub mine_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BrowseSort {
    #[default]
    Popularity,
    Score,
    Title,
    StartDate,
}

impl BrowseSort {
    pub const ALL: [Self; 4] = [Self::Popularity, Self::Score, Self::Title, Self::StartDate];
}

pub struct RankingView {
    pub kind: MediaKind,
    pub ranking: Ranking,
    pub genre: Option<String>,
    pub year: Option<i32>,
    pub feed: Feed,
    pub genres: Vec<String>,
    pub genres_started: bool,
}

pub struct SearchView {
    pub kind: MediaKind,
    pub text: String,
    /// Text of the last request.
    pub sent: String,
    pub typed_at: f64,
    pub feed: Feed,
    pub focus: bool,
    pub random_busy: bool,
}

/// A friend's profile as far as it has loaded.
#[derive(Default)]
pub struct FriendView {
    pub data: Option<Rc<FriendProfile>>,
    pub error: Option<ApiError>,
    pub loading: bool,
}

#[derive(Default)]
pub struct DetailView {
    pub data: Option<Rc<MediaDetail>>,
    pub error: Option<ApiError>,
    pub loading: bool,
    pub themes: Option<Result<Rc<Vec<Theme>>, ApiError>>,
    pub themes_asked: bool,
    pub expanded: bool,
}

/// Titles of a franchise by section.
pub type Overview = Vec<(Section, Vec<Media>)>;

pub enum Load<T> {
    Busy,
    Ready(Rc<T>),
    Failed(ApiError),
}

pub struct Editor {
    pub entry: ListEntry,
    pub is_new: bool,
    pub start: String,
    pub finish: String,
}

pub struct Confirm {
    pub title: String,
    pub body: String,
    pub action: String,
    pub danger: bool,
    pub act: Option<Act>,
}

pub struct Toast {
    pub text: String,
    pub until: f64,
    pub error: bool,
    /// The change the toast offers to undo.
    pub undo: Option<Undo>,
}

/// What a change replaced, so that it can be put back.
#[derive(Debug, Clone)]
pub struct Undo {
    /// The title as it was; `None` when the change added it to the list.
    pub before: Option<ListEntry>,
    pub at: MediaRef,
}

pub struct ListView {
    pub filter: Filter,
    pub cache: Option<ListCache>,
}

/// Filtered and sorted rows, kept between frames.
pub struct ListCache {
    pub key: (u64, Filter, SortKey, bool, TitleLanguage),
    pub items: Rc<Vec<crate::ui::rows::Item>>,
}
