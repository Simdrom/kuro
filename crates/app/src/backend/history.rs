//! Kioku: reading a streaming history and matching it to titles.

use super::*;

/// Where a Kioku sync is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KiokuStep {
    /// Pages of history read so far.
    Reading(u32),
    /// Titles looked up on the tracker, of how many.
    Searching { done: usize, total: usize },
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
/// can feed it a simulated source. `chosen` gives the title the user picked
/// for a group, which wins over any match.
pub fn kioku_review(
    platform: Platform,
    service: Service,
    groups: Vec<Group>,
    candidates: impl Fn(&Group) -> Vec<Media>,
    chosen: impl Fn(&Group) -> Option<Media>,
    entry: impl Fn(&Media) -> Option<ListEntry>,
    today: jiff::civil::Date,
) -> KiokuReview {
    let mut r = KiokuReview { platform: Some(platform), service, ..KiokuReview::default() };
    for g in groups {
        r.held.extend(g.items.iter().cloned());
        r.add(kioku_outcome(g, &candidates, &chosen, &entry, today));
    }
    let settled: HashSet<&String> = r.settled.iter().collect();
    r.held.retain(|i| !settled.contains(&i.external_id));
    r.sort();
    r
}

/// One group, through a chosen title or the candidates.
pub fn kioku_outcome(
    g: Group,
    candidates: &impl Fn(&Group) -> Vec<Media>,
    chosen: &impl Fn(&Group) -> Option<Media>,
    entry: &impl Fn(&Media) -> Option<ListEntry>,
    today: jiff::civil::Date,
) -> kioku::Outcome {
    match chosen(&g) {
        Some(m) => kioku::review::review_with(g, Some(kioku::Match::chosen(m)), entry, today),
        None => {
            let found = candidates(&g);
            kioku::review::review(g, &found, entry, today)
        }
    }
}

impl KiokuReview {
    pub fn add(&mut self, o: kioku::Outcome) {
        match o {
            kioku::Outcome::Change(p) => self.changes.push(*p),
            kioku::Outcome::Nothing { items, .. } => self.settled.extend(items),
            kioku::Outcome::Unmatched(g) => self.unmatched.push(g),
        }
    }

    /// Ticked first, then by title.
    pub fn sort(&mut self) {
        self.changes
            .sort_by(|a, b| b.selected.cmp(&a.selected).then_with(|| a.media.title.cmp(&b.media.title)));
    }

    /// The items of a group, as the platform gave them, for the ids of a change.
    pub fn group_of(&self, ids: &[String]) -> Group {
        Group { items: self.held.iter().filter(|i| ids.contains(&i.external_id)).cloned().collect() }
    }

    /// Drops everything that came from `g`, before it is reviewed again.
    pub fn forget(&mut self, g: &Group) {
        let ids: HashSet<&String> = g.items.iter().map(|i| &i.external_id).collect();
        let key = g.key();
        self.changes.retain(|c| !c.items.iter().any(|i| ids.contains(i)));
        self.unmatched.retain(|u| u.key() != key);
        self.settled.retain(|i| !ids.contains(i));
    }
}

/// Days a title lookup is reused for.
const LOOKUP_DAYS: i64 = 30;

/// A search on the tracker, from memory, from the store when recent, or asked.
async fn kioku_lookup(
    api: &Api,
    store: &Mutex<Store>,
    service: Service,
    seen: &mut std::collections::HashMap<String, Vec<Media>>,
    q: &str,
) -> Outcome<Vec<Media>> {
    if let Some(v) = seen.get(q) {
        return Ok(v.clone());
    }
    let key = format!("kioku-search:{}:{}", service.key(), q.to_lowercase());
    let hit = lock(store).cached::<Vec<Media>>(&key, now()).ok().flatten();
    let found = match hit {
        Some((v, age)) if age < LOOKUP_DAYS * 86_400 => v,
        _ => {
            // Anime only: what the tracker does not know as anime stays unmatched.
            match api.search(service, MediaKind::Anime, q, false, 1).await {
                Ok(p) => {
                    if let Err(e) = lock(store).put_cache(&key, &p.items, now()) {
                        tracing::warn!("kioku: lookup not cached: {e}");
                    }
                    p.items
                }
                Err(e) if e.is_transient() || e == ApiError::NotConfigured => return Err(e),
                Err(_) => Vec::new(),
            }
        }
    };
    seen.insert(q.to_owned(), found.clone());
    Ok(found)
}

/// Reads the history (or takes the items of a file), looks titles up and
/// builds the review.
async fn kioku_run(
    api: &Api,
    store: &Mutex<Store>,
    platform: Platform,
    service: Service,
    reply: &Reply,
    file: Option<Vec<WatchedItem>>,
) -> Outcome<KiokuReview> {
    let db = |e: kuro_core::StoreError| ApiError::Service(e.to_string());
    let (known, pending, chosen) = {
        let s = lock(store);
        (
            s.kioku_known(service, platform).map_err(db)?,
            s.kioku_pending(service, platform).map_err(db)?,
            s.kioku_chosen(service, platform).map_err(db)?,
        )
    };
    let mut items = match file {
        Some(items) => items,
        None => {
            let source: &dyn HistorySource = match platform {
                Platform::Crunchyroll => &api.crunchyroll,
                _ => return Err(ApiError::Unavailable),
            };
            let on_page = {
                let reply = reply.clone();
                move |n| reply.send(Event::KiokuProgress(KiokuStep::Reading(n)))
            };
            source.fetch(&known, &on_page).await?
        }
    };
    let read = items.len();
    items.extend(pending);
    let groups = kioku::review::groups(items, &known);
    let total = groups.len();
    // Counts only: titles and ids never reach the log.
    tracing::info!(read, groups = total, chosen = chosen.len(), "kioku: history read");
    let mut seen = std::collections::HashMap::new();
    let mut found: std::collections::HashMap<String, Vec<Media>> = std::collections::HashMap::new();
    for (done, g) in groups.iter().enumerate() {
        reply.send(Event::KiokuProgress(KiokuStep::Searching { done, total }));
        if done % 10 == 0 {
            tracing::info!(done, total, "kioku: looking up titles");
        }
        if chosen.contains_key(&g.key()) {
            continue;
        }
        let queries = kioku_queries(g);
        let mut all: Vec<Media> = Vec::new();
        for (i, q) in queries.iter().enumerate() {
            // The other names only when the first one is not enough.
            if i > 0 && kioku::matcher::best(&g.items, &all).is_some_and(|m| m.sure()) {
                break;
            }
            for m in kioku_lookup(api, store, service, &mut seen, q).await? {
                if !all.iter().any(|x| x.id == m.id) {
                    all.push(m);
                }
            }
        }
        found.insert(g.key(), all);
    }
    let candidates = |g: &Group| found.get(&g.key()).cloned().unwrap_or_default();
    let picked = |g: &Group| chosen.get(&g.key()).cloned();
    let entry = |m: &Media| {
        let r = MediaRef { service, kind: m.kind, id: m.id_on(service).unwrap_or(m.id) };
        lock(store).entry(r).ok().flatten()
    };
    let review = kioku_review(platform, service, groups, candidates, picked, entry, crate::app::today());
    lock(store).kioku_set_pending(service, platform, &review.held).map_err(db)?;
    tracing::info!(
        changes = review.changes.len(),
        ticked = review.selected(),
        unmatched = review.unmatched.len(),
        up_to_date = review.settled.len(),
        "kioku: review ready"
    );
    Ok(review)
}

/// Runs a sync in a task of its own, so that a bug ends it with an error
/// instead of leaving it spinning forever.
async fn kioku_task(
    api: Api,
    store: Arc<Mutex<Store>>,
    platform: Platform,
    service: Service,
    reply: Reply,
    file: Option<Vec<WatchedItem>>,
) {
    let started = std::time::Instant::now();
    let run = {
        let reply = reply.clone();
        tokio::spawn(async move { kioku_run(&api, &store, platform, service, &reply, file).await })
    };
    let result = run.await.unwrap_or_else(|e| {
        tracing::error!("kioku: sync stopped unexpectedly: {e}");
        Err(ApiError::Service("Kioku stopped unexpectedly".into()))
    });
    let seconds = started.elapsed().as_secs();
    match &result {
        Ok(_) => tracing::info!(seconds, "kioku: sync finished"),
        Err(e) => tracing::warn!(seconds, "kioku: sync failed: {e}"),
    }
    reply.send(Event::KiokuReview(result));
}

impl Backend {
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
        self.spawn(kioku_task(api, store, platform, service, reply, None));
    }

    /// Asks for an exported history file and reviews it like a sync.
    pub fn kioku_import(&self, platform: Platform, service: Service) {
        let reply = self.reply();
        let import = self.kioku_reader(platform, service);
        std::thread::spawn(move || {
            let Some(path) = rfd::FileDialog::new().add_filter("CSV / JSON", &["csv", "json"]).pick_file()
            else {
                return reply.send(Event::KiokuCancelled);
            };
            if let Some(import) = import {
                import(path);
            }
        });
    }

    /// Reviews a history file already found, like [`Self::kioku_import`].
    pub fn kioku_import_file(&self, platform: Platform, service: Service, path: PathBuf) {
        if let Some(import) = self.kioku_reader(platform, service) {
            std::thread::spawn(move || import(path));
        }
    }

    /// Reads, parses and reviews a history file, off the interface thread.
    fn kioku_reader(
        &self,
        platform: Platform,
        service: Service,
    ) -> Option<impl FnOnce(PathBuf) + Send + 'static> {
        let rt = self.rt.as_ref().map(|rt| rt.handle().clone())?;
        let (api, store, reply) = (self.api.clone(), self.store.clone(), self.reply());
        Some(move |path: PathBuf| match read_history(platform, &path) {
            Ok(items) => drop(rt.spawn(kioku_task(api, store, platform, service, reply, Some(items)))),
            Err(e) => reply.send(Event::KiokuReview(Err(e))),
        })
    }

    /// Looks in `dir` for streaming history files changed since `since`, and
    /// reports the newest one of each platform that reads as its history.
    pub fn scan_downloads(&self, dir: PathBuf, since: i64) {
        let reply = self.reply();
        std::thread::spawn(move || {
            let Ok(list) = std::fs::read_dir(&dir) else { return };
            let mut newest: std::collections::HashMap<Platform, (i64, PathBuf)> = Default::default();
            for e in list.flatten() {
                let Some(platform) = e.file_name().to_str().and_then(kioku::export_platform) else {
                    continue;
                };
                let Ok(meta) = e.metadata() else { continue };
                let modified = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .and_then(|d| i64::try_from(d.as_secs()).ok())
                    .unwrap_or(0);
                // Only a file that reads as a history counts, so another one
                // with a similar name never hides it.
                if meta.is_file()
                    && modified >= since
                    && newest.get(&platform).is_none_or(|(m, _)| modified > *m)
                    && read_history(platform, &e.path()).is_ok_and(|items| !items.is_empty())
                {
                    newest.insert(platform, (modified, e.path()));
                }
            }
            for (platform, (modified, path)) in newest {
                reply.send(Event::KiokuFileFound { platform, path, modified });
            }
        });
    }

    /// Searches the tracker for the title picker.
    pub fn kioku_search(&self, service: Service, text: String) {
        let (api, reply) = (self.api.clone(), self.reply());
        self.spawn(async move {
            let result = api.search(service, MediaKind::Anime, &text, false, 1).await.map(|p| p.items);
            reply.send(Event::KiokuFound { text, result });
        });
    }
}

/// A history file saved from `platform`, read within its size limit.
fn read_history(platform: Platform, path: &std::path::Path) -> Outcome<Vec<WatchedItem>> {
    use kuro_api::kioku::amazon::Amazon;
    use kuro_api::kioku::netflix::{MAX_FILE, Netflix};
    let source: &dyn HistorySource = match platform {
        Platform::Netflix => &Netflix,
        Platform::Amazon => &Amazon,
        _ => return Err(ApiError::Unavailable),
    };
    let bytes = match std::fs::metadata(path) {
        Ok(m) if m.len() > MAX_FILE as u64 => Err(ApiError::TooLarge),
        Ok(_) => std::fs::read(path).map_err(|e| ApiError::Service(e.to_string())),
        Err(e) => Err(ApiError::Service(e.to_string())),
    }?;
    source.parse(&bytes)
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
