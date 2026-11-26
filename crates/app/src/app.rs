//! Application state, actions and event handling.

use std::collections::HashMap;
use std::rc::Rc;

use eframe::egui::{self, Key, Modifiers, Vec2};
use jiff::civil::Date;
use jiff::tz::TimeZone;
use kuro_api::music::Track;
use kuro_api::{ApiError, Page, Ranking, RankingQuery, SeasonQuery, Tokens};
use kuro_core::kioku::{Group, Platform};
use kuro_core::links::{self, Link};
use kuro_core::list::{Filter, SortKey};
use kuro_core::notify;
use kuro_core::overview::Section;
use kuro_core::settings::{BackupEvery, GestureAction, Settings};
use kuro_core::{
    ListEntry, ListStatus, Media, MediaDetail, MediaKind, MediaRef, PersonDetail, SeasonYear, Service, Theme,
    TitleLanguage, UserProfile, progress,
};

use crate::backend::{Backend, Event, KiokuReview, KiokuStep, now};
use crate::confetti::Confetti;
use crate::i18n::{self, t, tf};
use crate::system::{Notice, System};
use crate::textures::Textures;
use crate::theme::{self, Palette};
use crate::{format, icon};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
    Anime,
    Manga,
    Seasonal,
    Ranking,
    Search,
    Settings,
}

impl Tab {
    pub const ALL: [Self; 6] =
        [Self::Anime, Self::Manga, Self::Seasonal, Self::Ranking, Self::Search, Self::Settings];
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
    /// Opens the title picker for a series of the review.
    KiokuPick(PickFrom),
    /// Searches with the picker's text.
    KiokuLookup,
    KiokuChoose(Box<Media>),
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
    fn apply(&mut self, page: u32, result: Result<Page<Media>, ApiError>) {
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

pub struct App {
    pub bg: Backend,
    pub sys: Box<dyn System>,
    pub s: Settings,
    saved: Settings,
    pub p: Palette,
    pub tex: Textures,
    pub tab: Tab,
    pub routes: Vec<Route>,
    pub lists: [ListView; 2],
    lib: [Vec<Rc<ListEntry>>; 2],
    /// Grows with every change to `lib`.
    pub lib_version: u64,
    index: HashMap<MediaRef, usize>,
    pub season: SeasonView,
    pub ranking: RankingView,
    pub search: SearchView,
    pub details: HashMap<MediaRef, DetailView>,
    pub people: HashMap<u64, Load<PersonDetail>>,
    pub overviews: HashMap<MediaRef, Load<Overview>>,
    pub tracks: HashMap<(String, String), Option<Track>>,
    pub roles_completed_only: bool,
    pub editor: Option<Editor>,
    pub confirm: Option<Confirm>,
    pub toast: Option<Toast>,
    pub confetti: Confetti,
    pub command: Option<String>,
    pub binge: Option<MediaRef>,
    /// An open title uses the whole window.
    pub detail_full: bool,
    /// Content and an open title fit side by side.
    pub can_split: bool,
    pub profiles: HashMap<Service, UserProfile>,
    pub login: Option<Service>,
    pending_login: Option<(Service, UserProfile, Tokens)>,
    pub refreshing: u8,
    pub unsynced: usize,
    pub sync_error: Option<(Service, ApiError)>,
    pub image_bytes: Option<u64>,
    pub cache_bytes: Option<u64>,
    pub kioku: KiokuView,
    /// Newest automatic backup in the chosen folder, once known.
    pub last_backup: Option<String>,
    pub backup_error: Option<String>,
    /// When the backup folder is looked at next.
    backup_at: f64,
    /// When the list was last read from the account.
    library_at: i64,
    /// The calendar text last sent to the file.
    calendar_text: String,
    pub calendar_error: Option<String>,
    acts: Vec<Act>,
    next_tick: f64,
    airing_at: i64,
    clipboard_seen: String,
    really_quit: bool,
    hidden: bool,
    /// The main window has the keyboard focus.
    focused: bool,
    /// Frames left in which a hidden start is enforced.
    start_hidden: u8,
    applied: Option<(Palette, bool, f32)>,
    icon_applied: Option<kuro_core::settings::AppIcon>,
    tray_applied: bool,
    time: f64,
    screen: egui::Rect,
}

pub fn today() -> Date {
    jiff::ZonedDateTime::now().date()
}

fn slot(k: MediaKind) -> usize {
    match k {
        MediaKind::Anime => 0,
        MediaKind::Manga => 1,
    }
}

impl App {
    pub fn new(ctx: &egui::Context, bg: Backend, sys: Box<dyn System>, hidden: bool) -> Self {
        let s = bg.store().settings().unwrap_or_default();
        i18n::set(&s.language);
        theme::install_fonts(ctx);
        let p = Palette::new(s.theme, true, s.tint);
        let list = || ListView {
            filter: Filter { status: Some(ListStatus::Current), ..Filter::default() },
            cache: None,
        };
        let mut app = Self {
            tab: if s.default_kind == MediaKind::Anime { Tab::Anime } else { Tab::Manga },
            saved: s.clone(),
            p,
            s,
            bg,
            sys,
            tex: Textures::default(),
            routes: Vec::new(),
            lists: [list(), list()],
            lib: Default::default(),
            lib_version: 0,
            index: HashMap::new(),
            season: SeasonView {
                at: SeasonYear::of_date(today()),
                continuing: false,
                feed: Feed::default(),
                text: String::new(),
                format: None,
                sort: BrowseSort::default(),
                mine_only: false,
            },
            ranking: RankingView {
                kind: MediaKind::Anime,
                ranking: Ranking::All,
                genre: None,
                year: None,
                feed: Feed::default(),
                genres: Vec::new(),
                genres_started: false,
            },
            search: SearchView {
                kind: MediaKind::Anime,
                text: String::new(),
                sent: String::new(),
                typed_at: 0.0,
                feed: Feed::default(),
                focus: false,
                random_busy: false,
            },
            details: HashMap::new(),
            people: HashMap::new(),
            overviews: HashMap::new(),
            tracks: HashMap::new(),
            roles_completed_only: false,
            editor: None,
            confirm: None,
            toast: None,
            confetti: Confetti::default(),
            command: None,
            binge: None,
            detail_full: false,
            can_split: true,
            profiles: HashMap::new(),
            login: None,
            pending_login: None,
            refreshing: 0,
            unsynced: 0,
            sync_error: None,
            image_bytes: None,
            cache_bytes: None,
            kioku: KiokuView::default(),
            last_backup: None,
            backup_error: None,
            backup_at: 0.0,
            library_at: now(),
            calendar_text: String::new(),
            calendar_error: None,
            acts: Vec::new(),
            next_tick: 0.0,
            airing_at: 0,
            clipboard_seen: String::new(),
            really_quit: false,
            hidden,
            focused: false,
            start_hidden: if hidden { 3 } else { 0 },
            applied: None,
            icon_applied: None,
            tray_applied: false,
            time: 0.0,
            screen: egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(1200.0, 800.0)),
        };
        app.reload();
        app.startup();
        app
    }

    fn startup(&mut self) {
        self.clipboard_seen = self.sys.clipboard().unwrap_or_default();
        for s in Service::ALL {
            if self.bg.api.is_signed_in(s) {
                self.bg.profile(s);
            }
        }
        self.refresh();
        self.bg.maintenance();
    }

    pub fn act(&mut self, a: Act) {
        self.acts.push(a);
    }

    // Library

    /// Reads the library of the primary service from disk.
    pub fn reload(&mut self) {
        let store = self.bg.store();
        for k in MediaKind::ALL {
            match store.entries(self.s.primary, k) {
                Ok(v) => self.lib[slot(k)] = v.into_iter().map(Rc::new).collect(),
                Err(e) => tracing::error!("library could not be read: {e}"),
            }
        }
        self.unsynced = store.pending().map_or(0, |p| p.len());
        drop(store);
        self.reindex();
    }

    fn reindex(&mut self) {
        self.lib_version += 1;
        self.index.clear();
        for list in &self.lib {
            for (i, e) in list.iter().enumerate() {
                self.index.insert(e.media.media_ref(), i);
            }
        }
    }

    pub fn library(&self, k: MediaKind) -> &[Rc<ListEntry>] {
        &self.lib[slot(k)]
    }

    pub fn entry_at(&self, r: MediaRef) -> Option<Rc<ListEntry>> {
        self.index.get(&r).and_then(|i| self.lib[slot(r.kind)].get(*i)).cloned()
    }

    /// The user's entry, whichever service `m` came from.
    pub fn entry(&self, m: &Media) -> Option<Rc<ListEntry>> {
        self.entry_at(self.home_ref(m))
    }

    /// The title's id on the primary service, when known.
    fn home_ref(&self, m: &Media) -> MediaRef {
        match m.id_on(self.s.primary) {
            Some(id) => MediaRef { service: self.s.primary, kind: m.kind, id },
            None => m.media_ref(),
        }
    }

    fn home_media(&self, m: &Media) -> Media {
        let r = self.home_ref(m);
        let mut out = m.clone();
        if r.service != m.service {
            match m.service {
                Service::AniList => out.anilist_id = Some(m.id),
                Service::Mal => out.mal_id = Some(m.id),
            }
            (out.service, out.id) = (r.service, r.id);
        }
        out
    }

    fn targets(&self) -> Vec<Service> {
        Service::ALL
            .into_iter()
            .filter(|s| (*s == self.s.primary || self.s.save_both) && self.bg.api.is_signed_in(*s))
            .collect()
    }

    pub fn save(&mut self, entry: &ListEntry) {
        let mut e = progress::sanitize(entry);
        e.media = self.home_media(&e.media);
        e.updated_at = now();
        let before = self.entry(&e.media);
        let targets = self.targets();
        let saved = self.bg.store().save_and_queue(&e, &targets);
        if let Err(err) = saved {
            tracing::error!("save failed: {err}");
            self.say_error(t("Your change could not be saved."));
            return;
        }
        let done = e.status == ListStatus::Completed
            && before.as_ref().is_none_or(|b| b.status != ListStatus::Completed || b.repeating);
        self.lib_version += 1;
        let k = slot(e.media.kind);
        let r = e.media.media_ref();
        match self.index.get(&r) {
            Some(i) => self.lib[k][*i] = Rc::new(e),
            None => {
                self.index.insert(r, self.lib[k].len());
                self.lib[k].push(Rc::new(e));
            }
        }
        if done && self.s.confetti && self.s.animations {
            let colors = [self.p.tint, self.p.yellow, self.p.green, self.p.orange, self.p.red];
            self.confetti.fire(self.screen, &colors);
        }
        self.unsynced = self.bg.store().pending().map_or(0, |p| p.len());
        if !targets.is_empty() {
            self.bg.sync();
        }
    }

    fn delete(&mut self, r: MediaRef) {
        let Some(e) = self.entry_at(r) else { return };
        let targets = self.targets();
        let saved = self.bg.store().delete_and_queue(&e, &targets);
        if let Err(err) = saved {
            tracing::error!("delete failed: {err}");
            self.say_error(t("Your change could not be saved."));
            return;
        }
        self.lib[slot(r.kind)].retain(|x| x.media.media_ref() != r);
        self.reindex();
        self.bg.sync();
        self.say(t("Removed from your list"));
    }

    /// The entry to change, or a new one for a title not in the list yet.
    /// Moves the list up to `ep`, as a notification's button asks. Never moves it back.
    pub fn mark_episode(&mut self, r: MediaRef, ep: u32) {
        let Some(e) = self.entry_at(r) else { return };
        if ep <= e.progress {
            return;
        }
        let next = progress::set_progress(&e, ep, today());
        self.save(&next);
    }

    fn working_entry(&self, m: &Media) -> ListEntry {
        match self.entry(m) {
            Some(e) => (*e).clone(),
            None => ListEntry::new(self.home_media(m)),
        }
    }

    pub fn refresh(&mut self) {
        let s = self.s.primary;
        if self.bg.api.is_signed_in(s) && self.bg.online() {
            self.refreshing = 2;
            self.bg.refresh_library(s, false);
        } else {
            self.refresh_airing();
        }
        self.bg.sync();
        self.tex.forget_failures();
    }

    fn refresh_airing(&mut self) {
        use kuro_core::ReleaseStatus as R;
        self.airing_at = now();
        let refs = self.lib[0]
            .iter()
            .filter(|e| matches!(e.status, ListStatus::Current | ListStatus::Planning))
            .filter(|e| matches!(e.media.release, R::Releasing | R::NotYetReleased))
            .map(|e| e.media.media_ref())
            .collect();
        self.bg.airing(refs);
    }

    /// Sorts by `key`, or flips the direction when it is already in use.
    pub fn sort_by(&mut self, key: SortKey) {
        if self.s.sort == key {
            self.s.sort_asc = !self.s.sort_asc;
        } else {
            // Start with the direction people expect.
            self.s.sort_asc = matches!(key, SortKey::Title | SortKey::NextEpisode);
            self.s.sort = key;
        }
    }

    // Feedback

    pub fn say(&mut self, text: impl Into<String>) {
        self.toast = Some(Toast { text: text.into(), until: self.time + 2.6, error: false });
    }

    pub fn say_error(&mut self, text: impl Into<String>) {
        self.toast = Some(Toast { text: text.into(), until: self.time + 6.0, error: true });
    }

    // Browsing requests

    pub fn season_query(&self) -> SeasonQuery {
        SeasonQuery { season: self.season.at, adult: self.s.show_adult }
    }

    pub fn ranking_query(&self) -> RankingQuery {
        let filters = self.s.primary == Service::AniList;
        RankingQuery {
            kind: self.ranking.kind,
            ranking: self.ranking.ranking,
            genre: self.ranking.genre.clone().filter(|_| filters),
            year: self.ranking.year.filter(|_| filters),
            adult: self.s.show_adult,
        }
    }

    pub fn load_season(&mut self) {
        let page = self.season.feed.begin_page();
        self.bg.season(self.s.primary, self.season_query(), self.season.continuing, page);
    }

    pub fn load_ranking(&mut self) {
        let page = self.ranking.feed.begin_page();
        self.bg.ranking(self.s.primary, self.ranking_query(), page);
    }

    pub fn load_search(&mut self) {
        let page = self.search.feed.begin_page();
        let (kind, text) = (self.search.kind, self.search.sent.clone());
        self.bg.search(self.s.primary, kind, text, self.s.show_adult, page);
    }

    pub fn start_search(&mut self) {
        let text = self.search.text.trim().to_owned();
        self.search.feed.reset();
        self.search.sent = text;
        if !self.search.sent.is_empty() {
            self.load_search();
        }
    }

    fn reset_feeds(&mut self) {
        self.season.feed.reset();
        self.ranking.feed.reset();
        self.search.feed.reset();
        self.search.sent.clear();
    }

    pub fn ensure_detail(&mut self, r: MediaRef) {
        let d = self.details.entry(r).or_default();
        if d.data.is_none() && d.error.is_none() && !d.loading {
            d.loading = true;
            self.bg.detail(r, false);
        }
    }

    fn open(&mut self, r: MediaRef) {
        if self.routes.last() != Some(&Route::Media(r)) {
            self.routes.push(Route::Media(r));
        }
    }

    pub fn open_link(&mut self, text: &str) -> bool {
        match links::parse(text) {
            Some(Link::Media(r)) => {
                self.act(Act::OpenRef(r));
                true
            }
            Some(Link::Search { kind, query }) => {
                self.search.kind = kind;
                self.search.text = query;
                self.start_search();
                self.act(Act::Go(Tab::Search));
                true
            }
            Some(Link::Tab(name)) => {
                let tab = match name.as_str() {
                    "anime" => Tab::Anime,
                    "manga" => Tab::Manga,
                    "seasonal" => Tab::Seasonal,
                    "ranking" => Tab::Ranking,
                    "search" => Tab::Search,
                    _ => Tab::Settings,
                };
                self.act(Act::Go(tab));
                true
            }
            Some(Link::Step { .. }) if !self.s.link_actions => {
                let off =
                    t("Links that change your list are off. Turn them on in Settings › Widget & automation.");
                self.report("Kuro", off);
                true
            }
            Some(Link::Step { media, by }) => {
                self.step_from_link(media, by);
                true
            }
            Some(Link::Binge(r)) => {
                match self.entry_at(r) {
                    Some(_) => self.act(Act::Binge(Some(r))),
                    None => self.report("Kuro", t("This title is not in your list.")),
                }
                true
            }
            Some(Link::Random(k)) => {
                self.act(Act::Random(k));
                true
            }
            Some(Link::Refresh) => {
                self.act(Act::Refresh);
                true
            }
            None => false,
        }
    }

    /// Adds or removes episodes as a link asks, and says where the title is now.
    fn step_from_link(&mut self, r: MediaRef, by: i32) {
        let Some(e) = self.entry_at(r) else {
            return self.report("Kuro", t("This title is not in your list."));
        };
        let title = e.media.display_title(self.s.title_language).to_owned();
        if by > 0 && e.status == ListStatus::Completed && !e.repeating {
            return self.report(&title, format::status(e.status, e.media.kind));
        }
        let next = progress::step(&e, by, today());
        self.save(&next);
        let total = next.media.total().map_or_else(|| "?".to_owned(), |n| n.to_string());
        let body = format!("{} · {}/{total}", format::status(next.status, next.media.kind), next.progress);
        self.report(&title, &body);
    }

    /// Says something in the window when it has the focus, or as a notification.
    fn report(&mut self, title: &str, body: &str) {
        if self.focused && !self.hidden {
            self.say(if title == "Kuro" { body.to_owned() } else { format!("{title}: {body}") });
        } else {
            self.sys.notify(Notice::new(title, body));
        }
    }

    // Actions

    fn apply_acts(&mut self, ctx: &egui::Context) {
        while !self.acts.is_empty() {
            for a in std::mem::take(&mut self.acts) {
                self.apply(a);
            }
            ctx.request_repaint();
        }
    }

    fn apply(&mut self, a: Act) {
        match a {
            Act::Open(m) => {
                let r = self.home_ref(&m);
                // Show what is known while the rest loads.
                let d = self.details.entry(r).or_default();
                if d.data.is_none() {
                    let media = Media { service: r.service, id: r.id, ..(*m).clone() };
                    d.data = Some(Rc::new(MediaDetail { media, ..MediaDetail::default() }));
                    d.loading = true;
                    self.bg.detail(r, false);
                }
                self.open(r);
            }
            Act::OpenRef(r) => {
                if r.service == self.s.primary || !self.bg.online() {
                    self.ensure_detail(r);
                    self.open(r);
                } else {
                    self.say(t("Opening…"));
                    self.bg.resolve(r, self.s.primary);
                }
            }
            Act::Push(r) => {
                if self.routes.last() != Some(&r) {
                    self.routes.push(r);
                }
            }
            Act::Back => {
                self.routes.pop();
            }
            Act::Go(tab) => {
                self.cache_bytes = None;
                if self.tab != tab {
                    self.routes.clear();
                }
                self.tab = tab;
                if tab == Tab::Search {
                    self.search.focus = true;
                }
            }
            Act::Gesture(g, m) => self.gesture(g, m),
            Act::Step(m, n) => {
                let e = self.working_entry(&m);
                let e = if n > 0 && e.status == ListStatus::Completed && !e.repeating { return } else { e };
                let next = progress::step(&e, n, today());
                self.save(&next);
            }
            Act::StepVolumes(m, n) => {
                let next = progress::step_volumes(&self.working_entry(&m), n, today());
                self.save(&next);
            }
            Act::SetStatus(m, s) => {
                let next = progress::set_status(&self.working_entry(&m), s, today());
                self.save(&next);
            }
            Act::Repeat(m) => {
                let next = progress::start_repeat(&self.working_entry(&m));
                self.save(&next);
            }
            Act::Save(e) => self.save(&e),
            Act::Edit(m) => {
                let is_new = self.entry(&m).is_none();
                let entry = self.working_entry(&m);
                let text = |d: Option<kuro_core::FuzzyDate>| d.map(|d| d.iso()).unwrap_or_default();
                self.editor =
                    Some(Editor { start: text(entry.start), finish: text(entry.finish), entry, is_new });
            }
            Act::AskDelete(m) => {
                let r = self.home_ref(&m);
                if !self.s.confirm_delete {
                    self.delete(r);
                    return;
                }
                let name = m.display_title(self.s.title_language).to_owned();
                self.confirm = Some(Confirm {
                    title: t("Remove from your list?").into(),
                    body: tf("Your progress, score and notes for “{}” will be deleted.", &[&name]),
                    action: t("Remove").into(),
                    danger: true,
                    act: Some(Act::Delete(r)),
                });
            }
            Act::Delete(r) => self.delete(r),
            Act::Url(u) => self.sys.open_url(&u),
            Act::Copy(text) => {
                self.sys.copy(&text);
                self.clipboard_seen = text;
                self.say(t("Link copied"));
            }
            Act::Binge(r) => self.binge = r,
            Act::ToggleMute(r) => {
                let muted = &mut self.s.notifications.muted;
                match muted.iter().position(|m| *m == r) {
                    Some(i) => drop(muted.remove(i)),
                    None => muted.push(r),
                }
            }
            Act::Login(s) => {
                self.login = Some(s);
                self.bg.login(s);
            }
            Act::CancelLogin => {
                self.login = None;
                self.pending_login = None;
                self.bg.cancel_login();
            }
            Act::AcceptLogin => self.accept_login(),
            Act::AskSignOut(s) => {
                let mut body = tf(
                    "The {} list will be removed from this computer. Your online list is kept.",
                    &[&s.name()],
                );
                let waiting =
                    self.bg.store().pending().map_or(0, |p| p.iter().filter(|p| p.target == s).count());
                if waiting > 0 {
                    body.push_str("\n\n");
                    body.push_str(&tf("{} unsent changes will be lost.", &[&waiting]));
                }
                self.confirm = Some(Confirm {
                    title: t("Sign out?").into(),
                    body,
                    action: t("Sign out").into(),
                    danger: true,
                    act: Some(Act::SignOut(s)),
                });
            }
            Act::SignOut(s) => {
                if let Err(e) = self.bg.api.sign_out(s) {
                    self.say_error(format::error(&e));
                    return;
                }
                self.profiles.remove(&s);
                self.sync_error = None;
                if let Err(e) = self.bg.store().clear_account(s) {
                    tracing::error!("sign-out cleanup failed: {e}");
                }
                self.reload();
            }
            Act::Refresh => self.refresh(),
            Act::Retry(r) => {
                let d = self.details.entry(r).or_default();
                d.error = None;
                d.loading = true;
                d.themes = None;
                d.themes_asked = false;
                self.bg.detail(r, true);
            }
            Act::Random(k) => {
                self.search.random_busy = true;
                self.bg.random(k, self.s.show_adult);
            }
            Act::ShowStatus(k, s) => {
                self.apply(Act::Go(if k == MediaKind::Anime { Tab::Anime } else { Tab::Manga }));
                self.lists[slot(k)].filter = Filter { status: Some(s), ..Filter::default() };
            }
            Act::Import(json) => {
                let targets = self.targets();
                let r = self.bg.store().import(&json, &targets);
                match r {
                    Ok(n) => {
                        self.reload();
                        self.bg.sync();
                        self.say(tf("{} titles imported", &[&n]));
                    }
                    Err(e) => self.say_error(tf("This backup could not be read: {}", &[&e])),
                }
            }
            Act::KiokuConnect(p) => {
                self.kioku.connecting = true;
                self.kioku.error = None;
                self.bg.kioku_connect(p);
            }
            Act::KiokuAskDisconnect(p) => {
                self.confirm = Some(Confirm {
                    title: tf("kioku.disconnect.title", &[&p.name()]),
                    body: tf("kioku.disconnect.body", &[&p.name()]),
                    action: t("kioku.disconnect").into(),
                    danger: true,
                    act: Some(Act::KiokuDisconnect(p)),
                });
            }
            Act::KiokuDisconnect(p) => {
                use kuro_api::kioku::HistorySource;
                let r = match p {
                    Platform::Crunchyroll => self.bg.api.crunchyroll.disconnect(),
                    _ => Ok(()),
                };
                match r {
                    Ok(()) => {
                        self.kioku.connected = Some(false);
                        self.kioku.review = None;
                    }
                    Err(e) => self.say_error(format::error(&e)),
                }
            }
            Act::KiokuSync(p) => {
                self.kioku.syncing = true;
                self.kioku.progress = None;
                self.kioku.started = Some(self.time);
                self.kioku.took = None;
                self.kioku.error = None;
                self.kioku.review = None;
                self.bg.kioku_sync(p, self.s.primary);
            }
            Act::KiokuAskApply => {
                let n = self.kioku.review.as_ref().map_or(0, KiokuReview::selected);
                self.confirm = Some(Confirm {
                    title: tf("kioku.apply.title", &[&n]),
                    body: t("kioku.apply.body").into(),
                    action: t("kioku.apply.action").into(),
                    danger: false,
                    act: Some(Act::KiokuApply),
                });
            }
            Act::KiokuApply => self.kioku_apply(),
            Act::KiokuImport(p) => {
                if !self.bg.online() {
                    return;
                }
                self.kioku.syncing = true;
                self.kioku.progress = None;
                self.kioku.started = Some(self.time);
                self.kioku.took = None;
                self.kioku.error = None;
                self.kioku.review = None;
                self.bg.kioku_import(p, self.s.primary);
            }
            Act::KiokuDismissAll => {
                if let Some(r) = self.kioku.review.as_mut() {
                    for g in std::mem::take(&mut r.unmatched) {
                        r.dismissed.extend(g.items.into_iter().map(|i| i.external_id));
                    }
                }
            }
            Act::KiokuPick(from) => {
                let Some(r) = self.kioku.review.as_ref() else { return };
                let group = match from {
                    PickFrom::Unmatched(i) => r.unmatched.get(i).cloned(),
                    PickFrom::Change(i) => r.changes.get(i).map(|c| r.group_of(&c.items)),
                };
                if let Some(group) = group.filter(|g| !g.items.is_empty()) {
                    let text = group.query().to_owned();
                    self.kioku.picker =
                        Some(KiokuPicker { group, text, sent: String::new(), busy: false, results: None });
                    self.act(Act::KiokuLookup);
                }
            }
            Act::KiokuLookup => {
                let Some(pk) = self.kioku.picker.as_mut() else { return };
                let text = pk.text.trim().to_owned();
                if text.is_empty() || !self.bg.online() {
                    return;
                }
                pk.busy = true;
                pk.sent = text.clone();
                self.bg.kioku_search(self.s.primary, text);
            }
            Act::KiokuChoose(m) => self.kioku_choose(*m),
            Act::KiokuDismiss(i) => {
                if let Some(r) = self.kioku.review.as_mut()
                    && i < r.unmatched.len()
                {
                    let g = r.unmatched.remove(i);
                    r.dismissed.extend(g.items.into_iter().map(|i| i.external_id));
                }
            }
        }
    }

    /// Remembers the title picked for a series and reviews that series again with it.
    fn kioku_choose(&mut self, m: Media) {
        let Some(picker) = self.kioku.picker.take() else { return };
        let Some(mut r) = self.kioku.review.take() else { return };
        let Some(platform) = r.platform else { return };
        let saved = self.bg.store().kioku_choose(r.service, platform, &picker.group.key(), &m);
        if let Err(e) = saved {
            tracing::error!("kioku: the chosen title was not saved: {e}");
            self.say_error(t("Your change could not be saved."));
        }
        r.forget(&picker.group);
        let entry = |x: &Media| self.entry(x).map(|e| (*e).clone());
        let chosen = |_: &Group| Some(m.clone());
        r.add(crate::backend::kioku_outcome(picker.group, &|_| Vec::new(), &chosen, &entry, today()));
        r.sort();
        self.kioku.review = Some(r);
    }

    /// Writes the ticked Kioku changes, checked again against the list as it
    /// is now: progress never moves back and finished titles stay as they are.
    fn kioku_apply(&mut self) {
        let Some(review) = self.kioku.review.take() else { return };
        let Some(platform) = review.platform else { return };
        if review.service != self.s.primary {
            // The primary service changed since: the matches belong to the other one.
            self.say_error(t("kioku.stale"));
            return;
        }
        let mut written: HashMap<MediaRef, ListEntry> = HashMap::new();
        let mut applied: Vec<String> = review.settled.clone();
        let mut changes: Vec<&kuro_core::kioku::Proposal> =
            review.changes.iter().filter(|p| p.selected).collect();
        changes.sort_by_key(|p| p.after.progress);
        for p in changes {
            let r = self.home_ref(&p.media);
            let current = written.get(&r).cloned().or_else(|| self.entry_at(r).map(|e| (*e).clone()));
            let next = match &current {
                // Unchanged since the review: exactly what was shown.
                c if c.as_ref() == p.before.as_ref() => Some(p.after.clone()),
                Some(c) if c.status == ListStatus::Completed || c.repeating => None,
                Some(c) if c.progress >= p.after.progress => None,
                Some(c) => {
                    let mut e = c.clone();
                    e.start = e.start.or(p.after.start);
                    let mut e = progress::set_progress(&e, p.after.progress, today());
                    if e.status == ListStatus::Completed && c.finish.is_none() {
                        e.finish = p.after.finish.or(e.finish);
                    }
                    Some(e)
                }
                None => Some(p.after.clone()),
            };
            if let Some(e) = next {
                let mut e = progress::sanitize(&e);
                e.media = self.home_media(&e.media);
                e.updated_at = now();
                written.insert(e.media.media_ref(), e);
            }
            applied.extend(p.items.iter().cloned());
        }
        let entries: Vec<ListEntry> = written.into_values().collect();
        let targets = self.targets();
        let saved = self.bg.store().kioku_apply(&kuro_core::store::KiokuApply {
            service: review.service,
            platform,
            entries: &entries,
            targets: &targets,
            applied: &applied,
            dismissed: &review.dismissed,
            now: now(),
        });
        if let Err(err) = saved {
            tracing::error!("kioku import failed: {err}");
            self.say_error(t("Your change could not be saved."));
            self.kioku.review = Some(review);
            return;
        }
        self.reload();
        if !targets.is_empty() {
            self.bg.sync();
        }
        self.say(tf("kioku.applied", &[&entries.len()]));
    }

    fn gesture(&mut self, g: GestureAction, m: Rc<Media>) {
        let next = match g {
            GestureAction::None => return,
            GestureAction::Increment => Act::Step(m, 1),
            GestureAction::Decrement => Act::Step(m, -1),
            GestureAction::IncrementVolume => Act::StepVolumes(m, 1),
            GestureAction::Edit => Act::Edit(m),
            GestureAction::Complete => Act::SetStatus(m, ListStatus::Completed),
            GestureAction::Drop => Act::SetStatus(m, ListStatus::Dropped),
            GestureAction::Pause => Act::SetStatus(m, ListStatus::Paused),
            GestureAction::OpenWeb => match m.web_url(m.service) {
                Some(u) => Act::Url(u),
                None => return,
            },
            GestureAction::CopyLink => match m.web_url(m.service) {
                Some(u) => Act::Copy(u),
                None => return,
            },
            GestureAction::Binge => Act::Binge(Some(self.home_ref(&m))),
        };
        self.act(next);
    }

    fn accept_login(&mut self) {
        self.login = None;
        let Some((s, profile, tokens)) = self.pending_login.take() else { return };
        if let Err(e) = self.bg.api.accept_login(s, &profile, &tokens) {
            self.say_error(format::error(&e));
            return;
        }
        self.say(tf("Signed in as {}", &[&profile.name]));
        self.profiles.insert(s, profile);
        self.refreshing = 2;
        self.bg.refresh_library(s, true);
    }

    // Events

    fn poll(&mut self, ctx: &egui::Context) {
        while let Some(e) = self.bg.next_event() {
            self.handle(ctx, e);
        }
    }

    pub fn handle(&mut self, ctx: &egui::Context, e: Event) {
        match e {
            Event::Library { service, result, .. } => {
                self.refreshing = self.refreshing.saturating_sub(1);
                match result {
                    Ok(()) => {
                        if service == self.s.primary {
                            self.library_at = now();
                            self.reload();
                        }
                        self.airing_at = now();
                        self.bg.sync();
                    }
                    Err(e) => self.say_error(format::error(&e)),
                }
            }
            Event::Season { query, continuing, page, result } => {
                if query == self.season_query() && continuing == self.season.continuing {
                    self.season.feed.apply(page, result);
                }
            }
            Event::Ranking { query, page, result } => {
                if query == self.ranking_query() {
                    self.ranking.feed.apply(page, result);
                }
            }
            Event::Search { kind, text, page, result } => {
                if kind == self.search.kind && text == self.search.sent {
                    self.search.feed.apply(page, result);
                }
            }
            Event::Genres(r) => {
                if let Ok(g) = r {
                    self.ranking.genres = g;
                }
            }
            Event::Detail { media, result, fresh } => {
                let d = self.details.entry(media).or_default();
                d.loading = !fresh;
                match result {
                    Ok(x) => {
                        d.error = None;
                        if media.service == Service::Mal && !x.themes.is_empty() {
                            d.themes = Some(Ok(Rc::new(x.themes.clone())));
                            d.themes_asked = true;
                        }
                        d.data = Some(Rc::new(x));
                    }
                    Err(e) => d.error = Some(e),
                }
            }
            Event::Themes { media, result } => {
                self.details.entry(media).or_default().themes = Some(result.map(Rc::new));
            }
            Event::Track { theme, result } => {
                self.tracks.insert((theme.title, theme.artist), result.ok().flatten());
            }
            Event::Person { id, result } => {
                self.people.insert(id, result.map_or_else(Load::Failed, |p| Load::Ready(Rc::new(p))));
            }
            Event::Overview { media, result } => {
                self.overviews.insert(media, result.map_or_else(Load::Failed, |p| Load::Ready(Rc::new(p))));
            }
            Event::Random(r) => {
                self.search.random_busy = false;
                match r {
                    Ok(m) => self.act(Act::Open(Rc::new(m))),
                    Err(e) => self.say_error(format::error(&e)),
                }
            }
            Event::Resolved { from, result } => {
                let r = match result {
                    Ok(r) => r,
                    // Unknown to the primary service: show it from where it came.
                    Err(ApiError::NotFound | ApiError::NotConfigured) => from,
                    Err(e) => return self.say_error(format::error(&e)),
                };
                self.ensure_detail(r);
                self.open(r);
            }
            Event::Airing(r) => {
                if let Ok(list) = r {
                    self.update_airing(list);
                }
            }
            Event::LoginUrl { service, result } => match result {
                Ok(u) if self.login == Some(service) => self.sys.open_url(&u),
                Ok(_) => {}
                Err(e) => {
                    self.login = None;
                    self.say_error(format::error(&e));
                }
            },
            Event::Login { service, result } => match result {
                Ok((profile, tokens, verified)) if self.login == Some(service) => {
                    let name = profile.name.clone();
                    self.pending_login = Some((service, profile, tokens));
                    self.show_window(ctx);
                    if verified {
                        self.accept_login();
                    } else {
                        self.confirm = Some(Confirm {
                            title: tf("Sign in as {}?", &[&name]),
                            body: t("Continue only if this is your account.").into(),
                            action: t("Sign in").into(),
                            danger: false,
                            act: Some(Act::AcceptLogin),
                        });
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    self.login = None;
                    self.say_error(format::error(&e));
                }
            },
            Event::Profile { service, result } => match result {
                Ok(p) => drop(self.profiles.insert(service, p)),
                Err(ApiError::Unauthorized) => {
                    self.sync_error = Some((service, ApiError::Unauthorized));
                }
                Err(e) => tracing::warn!("profile: {e}"),
            },
            Event::Sync { left, error, .. } => {
                self.unsynced = left;
                if let Some((s, e)) = &error
                    && !e.is_transient()
                {
                    let text = format!("{}: {}", s.name(), format::error(e));
                    self.say_error(text);
                }
                self.sync_error = error;
            }
            Event::Image { url, image } => self.tex.insert(ctx, url, image),
            Event::ImageCacheSize(n) => self.image_bytes = Some(n),
            Event::ImportText(r) => match r {
                Ok(json) => {
                    self.confirm = Some(Confirm {
                        title: t("Import this backup?").into(),
                        body: t("Titles in the backup replace the ones in your list.").into(),
                        action: t("Import").into(),
                        danger: false,
                        act: Some(Act::Import(json)),
                    });
                }
                Err(e) => self.say_error(tf("This backup could not be read: {}", &[&e])),
            },
            Event::Exported(r) => match r {
                Ok(()) => self.say(t("Backup saved")),
                Err(e) => self.say_error(tf("Something went wrong: {}", &[&e])),
            },
            Event::BackupFolder(folder) => {
                self.s.backup.folder = folder;
                if self.s.backup.every == BackupEvery::Off {
                    self.s.backup.every = BackupEvery::Daily;
                }
            }
            Event::AutoBackup(r) => match r {
                Ok(latest) => {
                    self.last_backup = latest;
                    self.backup_error = None;
                }
                Err(e) => {
                    // Said once; the page keeps showing it.
                    if self.backup_error.as_ref() != Some(&e) {
                        self.say_error(tf("The automatic backup failed: {}", &[&e]));
                    }
                    self.backup_error = Some(e);
                }
            },
            Event::CalendarFile(path) => self.s.calendar_file = path,
            Event::CalendarWritten(r) => {
                if let Err(e) = &r
                    && self.calendar_error.as_ref() != Some(e)
                {
                    self.say_error(tf("The calendar could not be saved: {}", &[e]));
                    // Tried again with the next change.
                    self.calendar_text.clear();
                }
                self.calendar_error = r.err();
            }
            Event::KiokuConnected { platform, result } => {
                self.kioku.connecting = false;
                self.show_window(ctx);
                match result {
                    Ok(()) => {
                        self.kioku.connected = Some(true);
                        self.say(tf("kioku.connected.toast", &[&platform.name()]));
                    }
                    Err(e) => self.kioku.error = Some(e),
                }
            }
            Event::KiokuCancelled => {
                self.kioku.syncing = false;
                self.kioku.progress = None;
                self.kioku.started = None;
            }
            Event::KiokuFound { text, result } => {
                if let Some(pk) = self.kioku.picker.as_mut()
                    && pk.sent == text
                {
                    pk.busy = false;
                    pk.results = Some(result);
                }
            }
            Event::KiokuProgress(step) => {
                if self.kioku.syncing {
                    self.kioku.progress = Some(step);
                }
            }
            Event::KiokuReview(result) => {
                self.kioku.syncing = false;
                self.kioku.progress = None;
                self.kioku.took = self.kioku.started.take().map(|s| (ctx.input(|i| i.time) - s).max(0.0));
                match result {
                    Ok(r) => self.kioku.review = Some(r),
                    Err(e) => {
                        if e == ApiError::Unauthorized {
                            self.kioku.connected = None;
                        }
                        self.kioku.error = Some(e);
                    }
                }
            }
            Event::OpenLink(l) => {
                self.open_link(&l);
            }
            Event::MarkEpisode(r, ep) => self.mark_episode(r, ep),
            Event::ShowWindow => self.show_window(ctx),
            Event::Quit => {
                self.really_quit = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn update_airing(&mut self, list: Vec<Media>) {
        let mut changed = false;
        for m in list {
            let Some(e) = self.entry_at(m.media_ref()) else { continue };
            let mut e = (*e).clone();
            let old = e.media.clone();
            e.media.next_airing = m.next_airing;
            e.media.release = m.release;
            e.media.episodes = m.episodes.or(e.media.episodes);
            e.media.start = m.start.or(e.media.start);
            e.media.end = m.end.or(e.media.end);
            if e.media != old {
                // Not a user edit: nothing to send.
                if let Err(err) = self.bg.store().put_entry(&e) {
                    tracing::error!("airing update failed: {err}");
                }
                changed = true;
            }
        }
        if changed {
            self.reload();
        }
    }

    pub fn show_window(&mut self, ctx: &egui::Context) {
        self.hidden = false;
        self.start_hidden = 0;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    // Timers

    fn tick(&mut self, ctx: &egui::Context) {
        ctx.request_repaint_after(std::time::Duration::from_secs(20));
        if self.time < self.next_tick {
            return;
        }
        self.next_tick = self.time + 20.0;
        self.send_notifications();
        self.send_reminders();
        let passed = self.lib[0].iter().any(|e| {
            e.status == ListStatus::Current && e.media.next_airing.is_some_and(|a| a.at + 120 < now())
        });
        let age = now() - self.airing_at;
        if age > 6 * 3600 || (passed && age > 600) {
            self.refresh_airing();
        }
        self.auto_backup();
        self.write_calendar();
        if self.refreshing == 0
            && self.bg.online()
            && self.bg.api.is_signed_in(self.s.primary)
            && self.s.refresh_due(self.library_at, now())
        {
            // Not again for a while, even if this one fails.
            self.library_at = now();
            self.refresh();
        }
        if self.unsynced > 0 && self.sync_error.as_ref().is_none_or(|(_, e)| e.is_transient()) {
            self.bg.sync();
        }
    }

    /// Rewrites the calendar file when its episodes changed.
    fn write_calendar(&mut self) {
        if self.s.calendar_file.is_empty() {
            return;
        }
        let all: Vec<ListEntry> = self.lib[0].iter().map(|e| (**e).clone()).collect();
        let text = kuro_core::calendar::ics(&all, self.s.title_language, t("Episode"));
        if text != self.calendar_text {
            self.calendar_text = text.clone();
            self.bg.write_calendar(self.s.calendar_file.clone().into(), text);
        }
    }

    /// Looks at the backup folder about once an hour.
    fn auto_backup(&mut self) {
        let b = &self.s.backup;
        if b.every == BackupEvery::Off || b.folder.is_empty() || self.time < self.backup_at {
            return;
        }
        self.backup_at = self.time + 3600.0;
        self.bg.auto_backup(self.s.primary, b.folder.clone().into(), b.every, b.keep, today());
    }

    fn send_notifications(&mut self) {
        let all: Vec<ListEntry> = self.lib[0].iter().map(|e| (**e).clone()).collect();
        let plan = notify::plan(&all, &self.s.notifications, &TimeZone::system());
        let store = self.bg.store();
        let due = notify::due(plan, now(), |k| store.was_notified(k).unwrap_or(true));
        for d in &due {
            let _ = store.mark_notified(&d.key, now());
        }
        drop(store);
        for d in due {
            let notice = self.notice(&d);
            self.sys.notify(notice);
        }
    }

    /// Titles left behind, and the summary of the week.
    fn send_reminders(&mut self) {
        use kuro_core::reminders;
        let n = &self.s.notifications;
        if !n.enabled {
            return;
        }
        let (days, weekly, hour) = (n.stale_days, n.weekly, n.date_hour);
        let all: Vec<ListEntry> = self.lib.iter().flatten().map(|e| (**e).clone()).collect();
        let store = self.bg.store();
        let due: Vec<&ListEntry> = reminders::stale(&all, now(), days, &n.muted)
            .into_iter()
            .filter(|e| !store.was_notified(&reminders::stale_key(e)).unwrap_or(true))
            .collect();
        for e in &due {
            let _ = store.mark_notified(&reminders::stale_key(e), now());
        }
        let week = weekly
            .then(|| reminders::week_key(now(), hour, &TimeZone::system()))
            .flatten()
            .filter(|k| !store.was_notified(k).unwrap_or(true));
        if let Some(k) = &week {
            let _ = store.mark_notified(k, now());
        }
        drop(store);
        let lang = self.s.title_language;
        match due.as_slice() {
            [] => {}
            [e] => {
                let m = &e.media;
                let mut notice = Notice::new(m.display_title(lang), tf("No progress for {} days", &[&days]));
                notice.detail = Some(format!("{} · {}", format::status(e.status, m.kind), e.progress));
                notice.image = m.cover.as_deref().and_then(|u| self.bg.images.cached(u));
                notice.link = Some(links::to_link(m.media_ref()));
                notice.mark = reminders::next_unit(e)
                    .filter(|_| m.kind == MediaKind::Anime)
                    .map(|ep| (m.media_ref(), ep));
                self.sys.notify(notice);
            }
            many => {
                let names: Vec<&str> = many.iter().take(3).map(|e| e.media.display_title(lang)).collect();
                let title = tf("{} titles with no progress for {} days", &[&many.len(), &days]);
                self.sys.notify(Notice::new(title, names.join(" · ")));
            }
        }
        if week.is_some() {
            let w = reminders::week(&self.lib[0].iter().map(|e| (**e).clone()).collect::<Vec<_>>(), now());
            if w.airing > 0 || w.behind > 0 {
                let body = tf("{} titles air this week · {} episodes to catch up", &[&w.airing, &w.behind]);
                let mut notice = Notice::new(t("Your week"), body);
                notice.link = Some("kuro://tab/anime".into());
                self.sys.notify(notice);
            }
        }
    }

    /// What a notification about a new episode says and offers.
    pub fn notice(&self, d: &notify::Due) -> Notice {
        let (e, m) = (&d.entry, &d.entry.media);
        let (body, episode) = match d.event {
            notify::Event::Episode(n) => (tf("Episode {} is out", &[&n]), Some(n)),
            notify::Event::Premiere => (t("The first episode is out").to_owned(), Some(1)),
            notify::Event::Finished => (t("The last episode is out").to_owned(), m.total()),
        };
        let total = m.total().map_or_else(|| "?".to_owned(), |n| n.to_string());
        let mut n = Notice::new(m.display_title(self.s.title_language), body);
        n.detail = Some(format!("{} · {}/{total}", format::status(e.status, m.kind), e.progress));
        n.image = m.cover.as_deref().and_then(|u| self.bg.images.cached(u));
        n.link = Some(links::to_link(m.media_ref()));
        // Only an episode still ahead of the list can be marked.
        n.mark = episode.filter(|ep| *ep > e.progress).map(|ep| (m.media_ref(), ep));
        n
    }

    fn check_clipboard(&mut self, ctx: &egui::Context) {
        let focused = ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::WindowFocused(true))));
        if !focused || !self.s.watch_clipboard {
            return;
        }
        if let Some(text) = self.sys.clipboard()
            && text != self.clipboard_seen
        {
            if matches!(links::parse(&text), Some(Link::Media(_))) {
                self.open_link(&text);
            }
            self.clipboard_seen = text;
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let hit = |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_key(m, k));
        if hit(Modifiers::COMMAND, Key::K) {
            self.command = match self.command {
                Some(_) => None,
                None => Some(String::new()),
            };
        }
        if self.editor.is_some() || self.confirm.is_some() || self.command.is_some() {
            return;
        }
        let tabs = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6];
        for (k, tab) in tabs.into_iter().zip(Tab::ALL) {
            if hit(Modifiers::COMMAND, k) {
                self.act(Act::Go(tab));
            }
        }
        if hit(Modifiers::COMMAND, Key::F) {
            self.act(Act::Go(Tab::Search));
        }
        if hit(Modifiers::COMMAND, Key::Comma) {
            self.act(Act::Go(Tab::Settings));
        }
        if hit(Modifiers::COMMAND, Key::R) || hit(Modifiers::NONE, Key::F5) {
            self.act(Act::Refresh);
        }
        if hit(Modifiers::ALT, Key::ArrowLeft) {
            self.act(Act::Back);
        }
        let back = ctx.input(|i| i.pointer.button_pressed(egui::PointerButton::Extra1));
        let typing = ctx.egui_wants_keyboard_input();
        if back || (!typing && !egui::Popup::is_any_open(ctx) && hit(Modifiers::NONE, Key::Escape)) {
            self.act(Act::Back);
        }
        if typing {
            return;
        }
        // Acts on the title being shown.
        if let Some(Route::Media(r)) = self.routes.last().cloned()
            && let Some(m) =
                self.details.get(&r).and_then(|d| d.data.as_ref()).map(|d| Rc::new(d.media.clone()))
        {
            if hit(Modifiers::NONE, Key::Plus) || hit(Modifiers::NONE, Key::Equals) {
                self.act(Act::Step(m.clone(), 1));
            }
            if hit(Modifiers::NONE, Key::Minus) {
                self.act(Act::Step(m.clone(), -1));
            }
            if hit(Modifiers::NONE, Key::E) {
                self.act(Act::Edit(m));
            }
        }
    }

    /// Persists and applies changed settings.
    fn settle(&mut self, ctx: &egui::Context) {
        if self.s != self.saved {
            let old = std::mem::replace(&mut self.saved, self.s.clone());
            if let Err(e) = self.bg.store().save_settings(&self.s) {
                tracing::error!("settings could not be saved: {e}");
            }
            if old.language != self.s.language {
                i18n::set(&self.s.language);
            }
            if old.primary != self.s.primary {
                self.routes.retain(|r| matches!(r, Route::Settings(_)));
                self.reset_feeds();
                self.ranking.ranking = Ranking::All;
                self.reload();
                self.refresh();
            }
            if old.show_adult != self.s.show_adult {
                self.reset_feeds();
            }
            if old.mal_client_id != self.s.mal_client_id || old.anilist_client_id != self.s.anilist_client_id
            {
                self.bg.api.set_ids(kuro_api::ClientIds::with_defaults(
                    &self.s.anilist_client_id,
                    &self.s.mal_client_id,
                ));
            }
            if old.calendar_file != self.s.calendar_file || old.title_language != self.s.title_language {
                self.calendar_text.clear();
                self.calendar_error = None;
                self.next_tick = 0.0;
            }
            if old.backup != self.s.backup {
                // Look at the new folder now rather than within the hour.
                (self.backup_at, self.next_tick) = (0.0, 0.0);
                (self.last_backup, self.backup_error) = (None, None);
            }
            if old.autostart != self.s.autostart
                && let Err(e) = self.sys.set_autostart(self.s.autostart)
            {
                self.say_error(tf("Something went wrong: {}", &[&e]));
            }
            if old.handle_links != self.s.handle_links
                && let Err(e) = self.sys.set_link_handler(self.s.handle_links)
            {
                self.say_error(tf("Something went wrong: {}", &[&e]));
            }
        }
        let dark = ctx.input(|i| i.raw.system_theme) != Some(egui::Theme::Light);
        self.p = Palette::new(self.s.theme, dark, self.s.tint);
        let look = (self.p, self.s.animations, self.s.zoom);
        if self.applied != Some(look) {
            self.applied = Some(look);
            theme::apply(ctx, &self.p, self.s.animations);
            ctx.set_zoom_factor(self.s.zoom);
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(min_size()));
        }
        let icon_changed = self.icon_applied != Some(self.s.icon);
        if icon_changed {
            self.icon_applied = Some(self.s.icon);
            let data = std::sync::Arc::new(icon::render(self.s.icon, 128));
            self.sys.set_notice_icon(&data);
            ctx.send_viewport_cmd(egui::ViewportCommand::Icon(Some(data)));
        }
        let tray = self.s.close_to_tray || self.hidden;
        if tray != self.tray_applied || (icon_changed && tray) {
            self.tray_applied = tray;
            if let Err(e) = self.sys.set_tray(tray, &icon::render(self.s.icon, 32)) {
                // Without a tray icon a hidden window could never come back.
                tracing::warn!("tray unavailable: {e}");
                self.s.close_to_tray = false;
                self.tray_applied = false;
                if self.hidden {
                    self.show_window(ctx);
                }
            }
        }
    }

    fn close_request(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && self.s.close_to_tray && !self.really_quit {
            self.hidden = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.time = ctx.input(|i| i.time);
        self.poll(ctx);
        self.apply_acts(ctx);
        self.settle(ctx);
        self.tick(ctx);
        self.close_request(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.screen = ctx.content_rect();
        self.focused = ctx.input(|i| i.focused);
        self.tex.begin_frame();
        // The window is shown after its first frame whatever was asked for.
        if self.start_hidden > 0 {
            self.start_hidden -= 1;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            ctx.request_repaint();
        }
        self.check_clipboard(&ctx);
        self.shortcuts(&ctx);
        crate::ui::show(self, ui);
        self.confetti.show(&ctx);
        self.apply_acts(&ctx);
        self.settle(&ctx);
    }
}

pub fn min_size() -> Vec2 {
    Vec2::new(340.0, 400.0)
}
