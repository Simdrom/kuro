//! Application state, actions and event handling.
//!
//! The state lives here; what changes it is split by where it comes from:
//! [`acts`] (the interface), [`events`] (the background), [`timers`],
//! [`calendar`], [`kioku`] and [`window`] (keys, settings and the window).

use std::collections::HashMap;
use std::rc::Rc;

use eframe::egui::{self, Key, Modifiers, Vec2};
use jiff::civil::Date;
use jiff::tz::TimeZone;
use kuro_api::music::Track;
use kuro_api::{ApiError, Page, Ranking, RankingQuery, SeasonQuery, Tokens};
use kuro_core::friends::{Friend, FriendProfile, MAX_FRIENDS, valid_name};
use kuro_core::kioku::{Group, Platform};
use kuro_core::links::{self, Link};
use kuro_core::list::{Filter, SortKey};
use kuro_core::notify;
use kuro_core::overview::Section;
use kuro_core::settings::{Every, GOOGLE_CALENDAR_NAME, GestureAction, GoogleCalendar, Settings};
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

mod acts;
mod calendar;
mod events;
mod kioku;
mod timers;
mod views;
mod window;

use calendar::{GOOGLE_EVERY, google_error};
pub use views::*;

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
    pub friends: HashMap<Friend, FriendView>,
    /// The name or link being typed in the Friends tab.
    pub friend_text: String,
    /// The service a typed name belongs to.
    pub friend_service: Service,
    /// Waiting for the people followed on AniList.
    pub following_busy: bool,
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
    /// When Downloads is looked at next.
    downloads_at: f64,
    /// The calendar text last sent to the file.
    calendar_text: String,
    pub calendar_error: Option<String>,
    /// Waiting for the Google sign-in in the browser.
    pub google_connecting: bool,
    /// A Google Calendar sync is under way.
    google_busy: bool,
    /// The episodes last sent to Google Calendar, and when.
    google_sent: Vec<kuro_core::calendar::Episode>,
    google_at: i64,
    pub google_error: Option<String>,
    /// Google refused the session: only signing in again brings it back.
    pub google_expired: bool,
    /// A newer Kuro, unless the user skipped it.
    pub update: Option<kuro_api::updates::Release>,
    /// When GitHub is asked next whether a newer Kuro is out.
    update_at: f64,
    /// The name being typed for a new Google calendar.
    pub google_new: Option<String>,
    /// A new Google calendar is being made.
    pub google_creating: bool,
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

/// Seconds a toast that can undo a change stays up.
const UNDO_SECONDS: f64 = 6.0;

/// Whether two entries hold the same progress, status, score and notes.
fn same_state(a: &ListEntry, b: &ListEntry) -> bool {
    let bare = |e: &ListEntry| ListEntry { media: Media::default(), updated_at: 0, ..e.clone() };
    bare(a) == bare(b)
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
        let saved_primary = s.primary;
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
            friends: HashMap::new(),
            friend_text: String::new(),
            friend_service: saved_primary,
            following_busy: false,
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
            downloads_at: 0.0,
            calendar_text: String::new(),
            calendar_error: None,
            google_connecting: false,
            google_busy: false,
            google_sent: Vec::new(),
            google_at: 0,
            google_error: None,
            google_expired: false,
            google_new: None,
            update: None,
            update_at: 0.0,
            google_creating: false,
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

    /// Saves `entry` without offering to undo it, as automatic changes do.
    /// False when it could not be saved, which the user is told.
    pub fn save(&mut self, entry: &ListEntry) -> bool {
        self.write(entry, true)
    }

    /// Saves a change the user made here, with a toast that can undo it.
    pub fn change(&mut self, entry: &ListEntry) {
        let before = self.entry(&entry.media).map(|e| (*e).clone());
        if !self.save(entry) {
            return;
        }
        let Some(after) = self.entry(&entry.media) else { return };
        if before.as_ref().is_some_and(|b| same_state(b, &after)) {
            return;
        }
        let title = after.media.display_title(self.s.title_language);
        let total = after.media.total().map_or_else(|| "?".to_owned(), |n| n.to_string());
        let status = format::status(after.status, after.media.kind);
        let text = format!("{title} · {status} · {}/{total}", after.progress);
        self.offer_undo(text, Undo { before, at: after.media.media_ref() });
    }

    fn offer_undo(&mut self, text: String, undo: Undo) {
        self.toast = Some(Toast { text, until: self.time + UNDO_SECONDS, error: false, undo: Some(undo) });
    }

    /// Puts back what the change in the toast replaced.
    pub(super) fn undo(&mut self) {
        let Some(u) = self.toast.take().and_then(|t| t.undo) else { return };
        let done = match u.before {
            Some(b) => self.write(&b, false),
            None => self.remove(u.at).is_some(),
        };
        if done {
            self.say(t("Change undone"));
        }
    }

    fn write(&mut self, entry: &ListEntry, celebrate: bool) -> bool {
        let mut e = progress::sanitize(entry);
        e.media = self.home_media(&e.media);
        e.updated_at = now();
        let before = self.entry(&e.media);
        let targets = self.targets();
        let saved = self.bg.store().save_and_queue(&e, &targets);
        if let Err(err) = saved {
            tracing::error!("save failed: {err}");
            self.say_error(t("Your change could not be saved."));
            return false;
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
        if celebrate && done && self.s.confetti && self.s.animations {
            let colors = [self.p.tint, self.p.yellow, self.p.green, self.p.orange, self.p.red];
            self.confetti.fire(self.screen, &colors);
        }
        self.unsynced = self.bg.store().pending().map_or(0, |p| p.len());
        if !targets.is_empty() {
            self.bg.sync();
        }
        true
    }

    /// Removes a title from the list and returns it as it was.
    fn remove(&mut self, r: MediaRef) -> Option<ListEntry> {
        let e = self.entry_at(r)?;
        let targets = self.targets();
        let saved = self.bg.store().delete_and_queue(&e, &targets);
        if let Err(err) = saved {
            tracing::error!("delete failed: {err}");
            self.say_error(t("Your change could not be saved."));
            return None;
        }
        self.lib[slot(r.kind)].retain(|x| x.media.media_ref() != r);
        self.reindex();
        self.bg.sync();
        Some((*e).clone())
    }

    fn delete(&mut self, r: MediaRef) {
        if let Some(e) = self.remove(r) {
            self.offer_undo(t("Removed from your list").into(), Undo { before: Some(e), at: r });
        }
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
        self.toast = Some(Toast { text: text.into(), until: self.time + 2.6, error: false, undo: None });
    }

    pub fn say_error(&mut self, text: impl Into<String>) {
        self.toast = Some(Toast { text: text.into(), until: self.time + 6.0, error: true, undo: None });
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

    /// Starts reading a friend's list unless it is known or on its way.
    pub fn ensure_friend(&mut self, f: &Friend) {
        let v = self.friends.entry(f.clone()).or_default();
        if v.data.is_none() && v.error.is_none() && !v.loading {
            v.loading = true;
            self.bg.friend(f.clone(), false);
        }
    }

    /// Adds `f` unless it is there already or the list is full.
    fn add_friend(&mut self, f: Friend) -> bool {
        if self.s.friends.len() >= MAX_FRIENDS || self.s.friends.iter().any(|x| x.same(&f)) {
            return false;
        }
        self.s.friends.push(f);
        true
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
                    "calendar" => Tab::Calendar,
                    "seasonal" => Tab::Seasonal,
                    "ranking" => Tab::Ranking,
                    "search" => Tab::Search,
                    "friends" => Tab::Friends,
                    _ => Tab::Settings,
                };
                self.act(Act::Go(tab));
                if name == "kioku" {
                    self.act(Act::Push(Route::Kioku));
                }
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
