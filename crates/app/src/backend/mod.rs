//! Background work: runs requests off the UI thread and reports events back.

use std::collections::HashSet;
use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard};

use eframe::egui;
use jiff::civil::Date;
use kuro_api::images::ImageCache;
use kuro_api::kioku::HistorySource;
use kuro_api::music::Track;
use kuro_api::{Api, ApiError, Page, RankingQuery, SeasonQuery, Tokens};
use kuro_core::backup;
use kuro_core::friends::{Friend, FriendProfile};
use kuro_core::kioku::{self, Group, Platform, Proposal, WatchedItem};
use kuro_core::overview::Section;
use kuro_core::settings::Every;
use kuro_core::{
    ListEntry, Media, MediaDetail, MediaKind, MediaRef, PersonDetail, Service, Store, Theme, UserProfile,
};

pub type Outcome<T> = Result<T, ApiError>;

mod files;
mod google;
mod history;
mod sync;

pub use history::{KiokuReview, KiokuStep, kioku_outcome, kioku_queries, kioku_review};

#[allow(clippy::large_enum_variant)]
pub enum Event {
    Library {
        service: Service,
        kind: MediaKind,
        result: Outcome<()>,
    },
    Season {
        query: SeasonQuery,
        continuing: bool,
        page: u32,
        result: Outcome<Page<Media>>,
    },
    Ranking {
        query: RankingQuery,
        page: u32,
        result: Outcome<Page<Media>>,
    },
    Search {
        kind: MediaKind,
        text: String,
        page: u32,
        result: Outcome<Page<Media>>,
    },
    Genres(Outcome<Vec<String>>),
    Detail {
        media: MediaRef,
        result: Outcome<MediaDetail>,
        fresh: bool,
    },
    Themes {
        media: MediaRef,
        result: Outcome<Vec<Theme>>,
    },
    Track {
        theme: Theme,
        result: Outcome<Option<Track>>,
    },
    Person {
        id: u64,
        result: Outcome<PersonDetail>,
    },
    Overview {
        media: MediaRef,
        result: Outcome<Vec<(Section, Vec<Media>)>>,
    },
    /// A friend's profile and list; `fresh` once the network answered.
    Friend {
        friend: Friend,
        result: Outcome<FriendProfile>,
        fresh: bool,
    },
    /// The names of the people followed on AniList.
    Following(Outcome<Vec<String>>),
    Random(Outcome<Media>),
    Resolved {
        from: MediaRef,
        result: Outcome<MediaRef>,
    },
    Airing(Outcome<Vec<Media>>),
    LoginUrl {
        service: Service,
        result: Outcome<String>,
    },
    Login {
        service: Service,
        result: Outcome<(UserProfile, Tokens, bool)>,
    },
    Profile {
        service: Service,
        result: Outcome<UserProfile>,
    },
    Sync {
        sent: usize,
        left: usize,
        error: Option<(Service, ApiError)>,
    },
    Image {
        url: String,
        image: Option<egui::ColorImage>,
    },
    ImageCacheSize(u64),
    /// Text chosen in a file dialog.
    ImportText(Result<String, String>),
    Exported(Result<(), String>),
    /// A folder chosen for automatic backups.
    BackupFolder(String),
    /// An automatic backup ran: the newest copy in the folder afterwards.
    AutoBackup(Result<Option<String>, String>),
    /// A file chosen for the episode calendar.
    CalendarFile(String),
    /// The episode calendar was written, or why not.
    CalendarWritten(Result<(), String>),
    /// Google Calendar: the page to sign in on.
    GoogleUrl(Outcome<String>),
    /// Google Calendar: signed in as this address, or why not.
    GoogleConnected(Outcome<String>),
    /// Google Calendar: the episodes were sent to the calendar `asked` for,
    /// whose id is now the result: new when it had to be made again.
    GoogleSynced {
        asked: String,
        name: String,
        result: Outcome<String>,
    },
    /// Google Calendar: a calendar was made with this name.
    GoogleCreated {
        name: String,
        result: Outcome<String>,
    },
    /// Kioku: the sign-in window closed.
    KiokuConnected {
        platform: Platform,
        result: Outcome<()>,
    },
    /// Kioku: how far a sync has got.
    KiokuProgress(KiokuStep),
    /// Kioku: the file dialog was closed without a file.
    KiokuCancelled,
    /// Kioku: titles for the picker.
    KiokuFound {
        text: String,
        result: Outcome<Vec<Media>>,
    },
    /// Kioku: history read and matched, waiting for the user.
    KiokuReview(Outcome<KiokuReview>),
    /// Kioku: a streaming history file in the Downloads folder.
    KiokuFileFound {
        platform: Platform,
        path: PathBuf,
        /// Unix seconds of its last change.
        modified: i64,
    },
    /// The newest published Kuro.
    Release(Outcome<kuro_api::updates::Release>),
    /// A link passed by another process or the tray.
    OpenLink(String),
    /// A notification's button: this episode was watched.
    MarkEpisode(kuro_core::MediaRef, u32),
    ShowWindow,
    Quit,
}

pub struct Backend {
    rt: Option<tokio::runtime::Runtime>,
    pub api: Api,
    store: Arc<Mutex<Store>>,
    pub images: ImageCache,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    ctx: egui::Context,
    syncing: Arc<AtomicBool>,
    resync: Arc<AtomicBool>,
    login: Option<tokio::task::AbortHandle>,
}

/// Sends an event and wakes the UI.
#[derive(Clone)]
pub struct Reply {
    tx: Sender<Event>,
    ctx: egui::Context,
}

impl Reply {
    pub fn send(&self, e: Event) {
        let _ = self.tx.send(e);
        self.ctx.request_repaint();
    }
}

pub fn now() -> i64 {
    jiff::Timestamp::now().as_second()
}

fn lock(store: &Mutex<Store>) -> MutexGuard<'_, Store> {
    store.lock().unwrap_or_else(|e| e.into_inner())
}

impl Backend {
    /// With `online` false no request is ever made, which tests rely on.
    pub fn new(api: Api, store: Store, image_dir: PathBuf, ctx: egui::Context, online: bool) -> Self {
        let (tx, rx) = channel();
        let rt = online
            .then(|| tokio::runtime::Builder::new_multi_thread().worker_threads(3).enable_all().build().ok())
            .flatten();
        let images = ImageCache::new(api.http().clone(), image_dir);
        Self {
            rt,
            api,
            store: Arc::new(Mutex::new(store)),
            images,
            tx,
            rx,
            ctx,
            syncing: Arc::default(),
            resync: Arc::default(),
            login: None,
        }
    }

    pub fn store(&self) -> MutexGuard<'_, Store> {
        lock(&self.store)
    }

    pub fn reply(&self) -> Reply {
        Reply { tx: self.tx.clone(), ctx: self.ctx.clone() }
    }

    pub fn next_event(&self) -> Option<Event> {
        self.rx.try_recv().ok()
    }

    pub fn online(&self) -> bool {
        self.rt.is_some()
    }

    fn spawn<F>(&self, f: F) -> Option<tokio::task::AbortHandle>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.rt.as_ref().map(|rt| rt.spawn(f).abort_handle())
    }

    /// Serves `key` from the cache at once, and from the network when older
    /// than `max_age` seconds.
    fn cached<T, F, E>(&self, key: String, max_age: i64, fetch: F, event: E)
    where
        T: serde::Serialize + serde::de::DeserializeOwned + Send + 'static,
        F: Future<Output = Outcome<T>> + Send + 'static,
        E: Fn(Outcome<T>, bool) -> Event + Send + 'static,
    {
        let (store, reply) = (self.store.clone(), self.reply());
        self.spawn(async move {
            let hit = lock(&store).cached::<T>(&key, now()).ok().flatten();
            let had = hit.is_some();
            if let Some((v, age)) = hit {
                let fresh = age < max_age;
                reply.send(event(Ok(v), fresh));
                if fresh {
                    return;
                }
            }
            match fetch.await {
                Ok(v) => {
                    if let Err(e) = lock(&store).put_cache(&key, &v, now()) {
                        tracing::warn!("cache write failed: {e}");
                    }
                    reply.send(event(Ok(v), true));
                }
                // Keep showing what the cache gave us.
                Err(e) if had && e.is_transient() => tracing::warn!("refresh of cached data failed: {e}"),
                Err(e) => reply.send(event(Err(e), true)),
            }
        });
    }

    // Browsing

    pub fn season(&self, s: Service, query: SeasonQuery, continuing: bool, page: u32) {
        let api = self.api.clone();
        let key = format!("season2:{s:?}:{query:?}:{continuing}:{page}");
        let q = query.clone();
        self.cached(
            key,
            3600,
            async move {
                let p = if continuing {
                    api.continuing(s, &q, page).await
                } else {
                    api.season(s, &q, page).await
                }?;
                Ok((p.items, p.more))
            },
            move |r, _| Event::Season { query: query.clone(), continuing, page, result: r.map(to_page) },
        );
    }

    pub fn ranking(&self, s: Service, query: RankingQuery, page: u32) {
        let api = self.api.clone();
        let key = format!("ranking2:{s:?}:{query:?}:{page}");
        let q = query.clone();
        self.cached(
            key,
            3600,
            async move {
                let p = api.ranking(s, &q, page).await?;
                Ok((p.items, p.more))
            },
            move |r, _| Event::Ranking { query: query.clone(), page, result: r.map(to_page) },
        );
    }

    pub fn search(&self, s: Service, kind: MediaKind, text: String, adult: bool, page: u32) {
        let api = self.api.clone();
        let key = format!("search2:{s:?}:{kind:?}:{adult}:{page}:{}", text.to_lowercase());
        let q = text.clone();
        self.cached(
            key,
            3600,
            async move {
                let p = api.search(s, kind, &q, adult, page).await?;
                Ok((p.items, p.more))
            },
            move |r, _| Event::Search { kind, text: text.clone(), page, result: r.map(to_page) },
        );
    }

    pub fn genres(&self) {
        let api = self.api.clone();
        self.cached("genres".into(), 30 * 86_400, async move { api.anilist.genres().await }, |r, _| {
            Event::Genres(r)
        });
    }

    pub fn detail(&self, media: MediaRef, force: bool) {
        let api = self.api.clone();
        let max_age = if force { 0 } else { 6 * 3600 };
        self.cached(
            format!("detail:{media:?}"),
            max_age,
            async move { api.detail(media).await },
            move |result, fresh| Event::Detail { media, result, fresh },
        );
    }

    pub fn themes(&self, m: &Media) {
        let api = self.api.clone();
        let media = m.media_ref();
        let (mal, anilist) = (m.id_on(Service::Mal), m.id_on(Service::AniList));
        self.cached(
            format!("themes:{media:?}"),
            7 * 86_400,
            async move { api.music.themes(mal, anilist).await },
            move |result, _| Event::Themes { media, result },
        );
    }

    pub fn track(&self, theme: Theme) {
        let api = self.api.clone();
        let q = theme.clone();
        self.cached(
            format!("track:{}:{}", theme.title, theme.artist),
            30 * 86_400,
            async move { api.music.apple(&q).await },
            move |result, _| Event::Track { theme: theme.clone(), result },
        );
    }

    pub fn person(&self, id: u64) {
        let api = self.api.clone();
        self.cached(
            format!("person:{id}"),
            7 * 86_400,
            async move { api.person(id).await },
            move |result, _| Event::Person { id, result },
        );
    }

    pub fn overview(&self, root: Media) {
        let api = self.api.clone();
        let media = root.media_ref();
        self.cached(
            format!("overview:{media:?}"),
            86_400,
            async move { api.overview(&root).await },
            move |result, _| Event::Overview { media, result },
        );
    }

    // Friends

    /// With `force`, asks the service even when a recent copy is cached.
    pub fn friend(&self, friend: Friend, force: bool) {
        let api = self.api.clone();
        let key = format!("friend:{}:{}", friend.service.key(), friend.name.to_lowercase());
        let f = friend.clone();
        self.cached(
            key,
            if force { 0 } else { 1800 },
            async move { api.friend(&f).await },
            move |result, fresh| Event::Friend { friend: friend.clone(), result, fresh },
        );
    }

    /// The newest published Kuro, asked of GitHub at most once a day.
    pub fn latest_release(&self) {
        let updates = self.api.updates.clone();
        self.cached("latest-release".into(), 86_400, async move { updates.latest().await }, |r, _| {
            Event::Release(r)
        });
    }

    pub fn following(&self) {
        let (api, reply) = (self.api.clone(), self.reply());
        self.spawn(async move { reply.send(Event::Following(api.following().await)) });
    }

    pub fn random(&self, kind: MediaKind, adult: bool) {
        let (api, reply) = (self.api.clone(), self.reply());
        let seed = jiff::Timestamp::now().subsec_nanosecond().unsigned_abs();
        self.spawn(
            async move { reply.send(Event::Random(api.anilist.random(kind, adult, seed.into()).await)) },
        );
    }

    pub fn resolve(&self, from: MediaRef, to: Service) {
        let (api, reply) = (self.api.clone(), self.reply());
        self.spawn(async move { reply.send(Event::Resolved { from, result: api.resolve(from, to).await }) });
    }

    // Accounts

    pub fn login(&mut self, service: Service) {
        self.cancel_login();
        let (api, reply) = (self.api.clone(), self.reply());
        self.login = self.spawn(async move {
            let login = match api.begin_login(service).await {
                Ok(l) => l,
                Err(e) => return reply.send(Event::Login { service, result: Err(e) }),
            };
            let url = login.authorize_url().map(String::from);
            reply.send(Event::LoginUrl { service, result: url });
            let result = api.finish_login(service, login).await;
            let result = result.map(|(p, r)| (p, r.tokens, r.verified));
            reply.send(Event::Login { service, result });
        });
    }

    pub fn cancel_login(&mut self) {
        if let Some(h) = self.login.take() {
            h.abort();
        }
    }

    pub fn profile(&self, service: Service) {
        let (api, reply) = (self.api.clone(), self.reply());
        self.spawn(async move { reply.send(Event::Profile { service, result: api.profile(service).await }) });
    }
}

fn to_page((items, more): (Vec<Media>, bool)) -> Page<Media> {
    Page { items, more }
}
