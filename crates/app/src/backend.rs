//! Background work: runs requests off the UI thread and reports events back.

use std::collections::HashSet;
use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard};

use eframe::egui;
use kuro_api::images::ImageCache;
use kuro_api::kioku::HistorySource;
use kuro_api::music::Track;
use kuro_api::{Api, ApiError, Page, RankingQuery, SeasonQuery, Tokens};
use kuro_core::kioku::{self, Group, Platform, Proposal, WatchedItem};
use kuro_core::overview::Section;
use kuro_core::{
    ListEntry, Media, MediaDetail, MediaKind, MediaRef, PersonDetail, Service, Store, Theme, UserProfile,
};

pub type Outcome<T> = Result<T, ApiError>;

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
    /// Kioku: the sign-in window closed.
    KiokuConnected {
        platform: Platform,
        result: Outcome<()>,
    },
    /// Kioku: history read and matched, waiting for the user.
    KiokuReview(Outcome<KiokuReview>),
    /// A link passed by another process or the tray.
    OpenLink(String),
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

/// What a Kioku sync found, before anything is written.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct KiokuReview {
    pub platform: Option<Platform>,
    pub service: Service,
    /// Proposed changes; the ticked ones are applied.
    pub changes: Vec<Proposal>,
    /// Platform ids of titles already up to date.
    pub settled: Vec<String>,
    /// Series no title was found for.
    pub unmatched: Vec<Group>,
    /// Platform ids the user chose to leave out.
    pub dismissed: Vec<String>,
    /// Items to keep for later until applied or dismissed.
    pub held: Vec<WatchedItem>,
}

impl KiokuReview {
    pub fn selected(&self) -> usize {
        self.changes.iter().filter(|p| p.selected).count()
    }
}

/// Texts to search the tracker with for one group.
pub fn kioku_queries(g: &Group) -> Vec<String> {
    let mut q = vec![g.query().to_owned()];
    if let Some(i) = g.items.first() {
        let extra = if i.movie { i.episode_title.clone() } else { i.season_title.clone() };
        q.extend(extra.filter(|t| !t.trim().is_empty() && !t.eq_ignore_ascii_case(&i.series_title)));
    }
    q.retain(|t| !t.trim().is_empty());
    q
}

/// Matches groups to titles and proposes changes. Pure, so the interface tests
/// can feed it a simulated source.
pub fn kioku_review(
    platform: Platform,
    service: Service,
    groups: Vec<Group>,
    candidates: impl Fn(&Group) -> Vec<Media>,
    entry: impl Fn(&Media) -> Option<ListEntry>,
    today: jiff::civil::Date,
) -> KiokuReview {
    let mut r = KiokuReview { platform: Some(platform), service, ..KiokuReview::default() };
    for g in groups {
        r.held.extend(g.items.iter().cloned());
        let found = candidates(&g);
        match kioku::review::review(g, &found, &entry, today) {
            kioku::Outcome::Change(p) => r.changes.push(*p),
            kioku::Outcome::Nothing { items, .. } => r.settled.extend(items),
            kioku::Outcome::Unmatched(g) => r.unmatched.push(g),
        }
    }
    let settled: HashSet<&String> = r.settled.iter().collect();
    r.held.retain(|i| !settled.contains(&i.external_id));
    r.changes.sort_by(|a, b| b.selected.cmp(&a.selected).then_with(|| a.media.title.cmp(&b.media.title)));
    r
}

async fn kioku_run(
    api: &Api,
    store: &Mutex<Store>,
    platform: Platform,
    service: Service,
) -> Outcome<KiokuReview> {
    let db = |e: kuro_core::StoreError| ApiError::Service(e.to_string());
    let (known, pending) = {
        let s = lock(store);
        (s.kioku_known(service, platform).map_err(db)?, s.kioku_pending(service, platform).map_err(db)?)
    };
    let source: &dyn HistorySource = match platform {
        Platform::Crunchyroll => &api.crunchyroll,
        _ => return Err(ApiError::Unavailable),
    };
    let mut items = source.fetch(&known).await?;
    items.extend(pending);
    let groups = kioku::review::groups(items, &known);
    let mut found: std::collections::HashMap<String, Vec<Media>> = std::collections::HashMap::new();
    for q in groups.iter().flat_map(kioku_queries) {
        if found.contains_key(&q) {
            continue;
        }
        // Anime only: what the tracker does not know as anime stays unmatched.
        let page = api.search(service, MediaKind::Anime, &q, false, 1).await;
        match page {
            Ok(p) => drop(found.insert(q, p.items)),
            Err(e) if e.is_transient() || e == ApiError::NotConfigured => return Err(e),
            Err(_) => drop(found.insert(q, Vec::new())),
        }
    }
    let candidates = |g: &Group| {
        let mut all: Vec<Media> = Vec::new();
        for q in kioku_queries(g) {
            for m in found.get(&q).into_iter().flatten() {
                if !all.iter().any(|x| x.id == m.id) {
                    all.push(m.clone());
                }
            }
        }
        all
    };
    let entry = |m: &Media| {
        let r = MediaRef { service, kind: m.kind, id: m.id_on(service).unwrap_or(m.id) };
        lock(store).entry(r).ok().flatten()
    };
    let review = kioku_review(platform, service, groups, candidates, entry, crate::app::today());
    lock(store).kioku_set_pending(service, platform, &review.held).map_err(db)?;
    Ok(review)
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

    // Library

    /// With `merge`, local titles the account lacks are kept and uploaded.
    pub fn refresh_library(&self, service: Service, merge: bool) {
        for kind in MediaKind::ALL {
            let (api, store, reply) = (self.api.clone(), self.store.clone(), self.reply());
            self.spawn(async move {
                let result = match api.library(service, kind).await {
                    Ok(mut list) => {
                        if service == Service::Mal && kind == MediaKind::Anime {
                            add_airing(&api, &mut list).await;
                        }
                        lock(&store)
                            .replace_library(service, kind, &list, merge)
                            .map_err(|e| ApiError::Service(e.to_string()))
                    }
                    Err(e) => Err(e),
                };
                reply.send(Event::Library { service, kind, result });
            });
        }
    }

    /// Sends queued edits. Calls made while a run is active start another run after it.
    pub fn sync(&self) {
        if self.rt.is_none() {
            return;
        }
        if self.syncing.swap(true, Ordering::SeqCst) {
            self.resync.store(true, Ordering::SeqCst);
            return;
        }
        let (api, store, reply) = (self.api.clone(), self.store.clone(), self.reply());
        let (syncing, resync) = (self.syncing.clone(), self.resync.clone());
        self.spawn(async move {
            loop {
                resync.store(false, Ordering::SeqCst);
                let (sent, left, error) = run_sync(&api, &store).await;
                reply.send(Event::Sync { sent, left, error });
                if !resync.load(Ordering::SeqCst) {
                    break;
                }
            }
            syncing.store(false, Ordering::SeqCst);
        });
    }

    pub fn airing(&self, refs: Vec<MediaRef>) {
        if refs.is_empty() {
            return;
        }
        let (api, reply) = (self.api.clone(), self.reply());
        self.spawn(async move { reply.send(Event::Airing(api.airing(&refs).await)) });
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

    // Kioku

    /// Opens the platform's sign-in window in a process of its own.
    pub fn kioku_connect(&self, platform: Platform) {
        if self.rt.is_none() {
            return;
        }
        let reply = self.reply();
        std::thread::spawn(move || {
            let result = kioku_sign_in(platform);
            reply.send(Event::KiokuConnected { platform, result });
        });
    }

    /// Reads the history and matches it; nothing is written to the list.
    pub fn kioku_sync(&self, platform: Platform, service: Service) {
        let (api, store, reply) = (self.api.clone(), self.store.clone(), self.reply());
        self.spawn(async move {
            reply.send(Event::KiokuReview(kioku_run(&api, &store, platform, service).await))
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

    // Images and files

    pub fn image(&self, url: String) {
        let (images, reply) = (self.images.clone(), self.reply());
        self.spawn(async move {
            let bytes = images.get(&url).await;
            let image = match bytes {
                Ok(b) => tokio::task::spawn_blocking(move || decode_image(&b)).await.ok().flatten(),
                Err(e) => {
                    tracing::debug!("image failed: {e}");
                    None
                }
            };
            reply.send(Event::Image { url, image });
        });
    }

    pub fn image_cache_size(&self) {
        let (images, reply) = (self.images.clone(), self.reply());
        self.spawn(async move {
            if let Ok(n) = tokio::task::spawn_blocking(move || images.size()).await {
                reply.send(Event::ImageCacheSize(n));
            }
        });
    }

    pub fn maintenance(&self) {
        let (images, store) = (self.images.clone(), self.store.clone());
        self.spawn(async move {
            let _ = tokio::task::spawn_blocking(move || {
                images.trim(400 * 1024 * 1024);
                let _ = lock(&store).prune(now(), 60);
            })
            .await;
        });
    }

    pub fn export(&self, service: Service) {
        let (store, reply) = (self.store.clone(), self.reply());
        std::thread::spawn(move || {
            let Some(path) = rfd::FileDialog::new()
                .add_filter("JSON", &["json"])
                .set_file_name("kuro-backup.json")
                .save_file()
            else {
                return;
            };
            let text = lock(&store).export(service).map_err(|e| e.to_string());
            let r = text.and_then(|t| std::fs::write(path, t).map_err(|e| e.to_string()));
            reply.send(Event::Exported(r));
        });
    }

    pub fn import(&self) {
        const MAX: u64 = 64 * 1024 * 1024;
        let reply = self.reply();
        std::thread::spawn(move || {
            let Some(path) = rfd::FileDialog::new().add_filter("JSON", &["json"]).pick_file() else { return };
            let r = match std::fs::metadata(&path) {
                Ok(m) if m.len() > MAX => Err("file is too large".to_owned()),
                Ok(_) => std::fs::read_to_string(&path).map_err(|e| e.to_string()),
                Err(e) => Err(e.to_string()),
            };
            reply.send(Event::ImportText(r));
        });
    }
}

#[cfg(windows)]
fn kioku_sign_in(platform: Platform) -> Outcome<()> {
    use crate::kioku_login as login;
    let exe = std::env::current_exe().map_err(|e| ApiError::Service(e.to_string()))?;
    let status = std::process::Command::new(exe)
        .args([login::ARG, platform.key()])
        .status()
        .map_err(|e| ApiError::Service(e.to_string()))?;
    match status.code() {
        Some(login::SIGNED_IN) => Ok(()),
        Some(login::CANCELLED) => Err(ApiError::Auth("cancelled".into())),
        Some(login::EXPIRED) => Err(ApiError::Unauthorized),
        _ => Err(ApiError::Auth("Crunchyroll".into())),
    }
}

#[cfg(not(windows))]
fn kioku_sign_in(_: Platform) -> Outcome<()> {
    Err(ApiError::Unavailable)
}

fn to_page((items, more): (Vec<Media>, bool)) -> Page<Media> {
    Page { items, more }
}

async fn add_airing(api: &Api, list: &mut [ListEntry]) {
    use kuro_core::ReleaseStatus as R;
    let refs: Vec<MediaRef> = list
        .iter()
        .filter(|e| matches!(e.media.release, R::Releasing | R::NotYetReleased))
        .map(|e| e.media.media_ref())
        .collect();
    let Ok(found) = api.airing(&refs).await else { return };
    for e in list {
        if let Some(m) = found.iter().find(|m| m.media_ref() == e.media.media_ref()) {
            e.media.next_airing = m.next_airing;
            e.media.anilist_id = m.anilist_id;
        }
    }
}

async fn run_sync(api: &Api, store: &Mutex<Store>) -> (usize, usize, Option<(Service, ApiError)>) {
    let pending = match lock(store).pending() {
        Ok(p) => p,
        Err(e) => return (0, 0, Some((Service::AniList, ApiError::Service(e.to_string())))),
    };
    let mut blocked = HashSet::new();
    let (mut sent, mut left, mut error) = (0, 0, None);
    for p in pending {
        if blocked.contains(&p.target) || !api.is_signed_in(p.target) {
            left += 1;
            continue;
        }
        match api.push(&p).await {
            Ok(()) => {
                let _ = lock(store).sent(p.revision);
                sent += 1;
            }
            // The other service does not have this title; retrying cannot help.
            Err(ApiError::NotFound) if p.target != p.media.service => {
                let _ = lock(store).sent(p.revision);
            }
            Err(e) => {
                let _ = lock(store).failed(p.revision, &e.to_string());
                left += 1;
                if e.is_transient() || e == ApiError::Unauthorized {
                    blocked.insert(p.target);
                }
                error = Some((p.target, e));
            }
        }
    }
    (sent, left, error)
}

/// Decodes within limits that stop decompression bombs.
fn decode_image(bytes: &[u8]) -> Option<egui::ColorImage> {
    const MAX_SIDE: u32 = 4096;
    const SHOWN: u32 = 640;
    let mut r = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(96 * 1024 * 1024);
    r.limits(limits);
    let mut img = r.decode().ok()?;
    if img.width() > SHOWN || img.height() > SHOWN * 2 {
        img = img.thumbnail(SHOWN, SHOWN * 2);
    }
    let rgba = img.into_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(size, &rgba))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kuro_api::{ClientIds, MemorySecrets};

    #[test]
    fn hostile_images_are_rejected() {
        assert!(decode_image(b"not an image").is_none());
        // PNG header announcing 60000x60000 pixels.
        let mut png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
        png.extend([0, 0, 0xEA, 0x60, 0, 0, 0xEA, 0x60, 8, 6, 0, 0, 0, 0, 0, 0, 0]);
        assert!(decode_image(&png).is_none());
    }

    #[test]
    fn large_images_are_downscaled() {
        let mut buf = std::io::Cursor::new(Vec::new());
        image::RgbImage::new(2000, 1000).write_to(&mut buf, image::ImageFormat::Png).unwrap();
        assert!(decode_image(buf.get_ref()).unwrap().size[0] <= 640);
    }

    #[tokio::test]
    async fn sync_leaves_edits_queued_while_signed_out() {
        let api = Api::new(Arc::new(MemorySecrets::default()), ClientIds::default()).unwrap();
        let mut s = Store::in_memory().unwrap();
        let e = ListEntry::new(Media { id: 1, ..Media::default() });
        s.save_and_queue(&e, &[Service::AniList]).unwrap();
        let store = Mutex::new(s);
        assert_eq!(run_sync(&api, &store).await, (0, 1, None));
        assert_eq!(lock(&store).pending().unwrap().len(), 1);
    }
}
