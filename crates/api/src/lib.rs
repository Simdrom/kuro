//! Remote services behind one facade.

pub mod anilist;
pub mod http;
pub mod images;
pub mod kioku;
pub mod mal;
pub mod music;
pub mod oauth;
pub mod secrets;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use kuro_core::overview::{Section, Walk};
use kuro_core::store::{Change, Pending};
use kuro_core::{
    Format, ListEntry, Media, MediaDetail, MediaKind, MediaRef, PersonDetail, SeasonYear, Service,
    UserProfile,
};

pub use http::{ApiError, Http, Result};
pub use secrets::{Keyring, MemorySecrets, SecretKey, SecretStore, Tokens};

#[derive(Debug, Clone, PartialEq)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub more: bool,
}

impl<T> Default for Page<T> {
    fn default() -> Self {
        Self { items: Vec::new(), more: false }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Ranking {
    #[default]
    All,
    Airing,
    Upcoming,
    Popularity,
    Favorites,
    Trending,
    Format(Format),
}

impl Ranking {
    pub fn options(kind: MediaKind, service: Service) -> Vec<Self> {
        use Format as F;
        let mut v = vec![Self::All, Self::Popularity, Self::Favorites];
        if service == Service::AniList {
            v.push(Self::Trending);
        }
        let formats: &[F] = match kind {
            MediaKind::Anime => {
                v.extend([Self::Airing, Self::Upcoming]);
                &[F::Tv, F::Movie, F::Ova, F::Special]
            }
            MediaKind::Manga => &[F::Manga, F::Novel, F::OneShot, F::Manhwa, F::Manhua],
        };
        v.extend(formats.iter().map(|f| Self::Format(*f)));
        v
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SeasonQuery {
    pub season: SeasonYear,
    pub adult: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RankingQuery {
    pub kind: MediaKind,
    pub ranking: Ranking,
    /// AniList only.
    pub genre: Option<String>,
    /// AniList only.
    pub year: Option<i32>,
    pub adult: bool,
}

/// Client ids, from settings or baked in at build time.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClientIds {
    pub anilist: String,
    pub mal: String,
}

impl ClientIds {
    pub fn with_defaults(anilist: &str, mal: &str) -> Self {
        let or =
            |v: &str, d: Option<&str>| if v.is_empty() { d.unwrap_or_default().into() } else { v.to_owned() };
        Self {
            anilist: or(anilist, option_env!("KURO_ANILIST_CLIENT_ID")),
            mal: or(mal, option_env!("KURO_MAL_CLIENT_ID")),
        }
    }

    pub fn get(&self, s: Service) -> &str {
        match s {
            Service::AniList => &self.anilist,
            Service::Mal => &self.mal,
        }
    }
}

#[derive(Clone)]
pub struct Api {
    http: Http,
    pub anilist: anilist::AniList,
    pub mal: mal::Mal,
    pub music: music::Music,
    /// Kioku's only account source so far.
    pub crunchyroll: kioku::crunchyroll::Crunchyroll,
    secrets: Arc<dyn SecretStore>,
    ids: Arc<RwLock<ClientIds>>,
    users: Arc<Mutex<HashMap<Service, u64>>>,
    /// Whether a token is stored, so the interface can ask on every frame.
    signed: Arc<Mutex<HashMap<Service, bool>>>,
    /// Serialises token refreshes: a refresh token works only once.
    refreshing: Arc<tokio::sync::Mutex<()>>,
    mal_endpoints: oauth::Endpoints,
    clock: fn() -> i64,
}

/// Refresh this long before a token expires.
const EARLY: i64 = 300;

fn now() -> i64 {
    jiff::Timestamp::now().as_second()
}

impl Api {
    pub fn new(secrets: Arc<dyn SecretStore>, ids: ClientIds) -> Result<Self> {
        let http = Http::new()?;
        Ok(Self {
            anilist: anilist::AniList::new(http.clone()),
            mal: mal::Mal::new(http.clone()),
            music: music::Music::new(http.clone()),
            crunchyroll: kioku::crunchyroll::Crunchyroll::new(secrets.clone())?,
            http,
            secrets,
            ids: Arc::new(RwLock::new(ids)),
            users: Arc::default(),
            signed: Arc::default(),
            refreshing: Arc::default(),
            mal_endpoints: oauth::Endpoints::of(Service::Mal),
            clock: now,
        })
    }

    pub fn http(&self) -> &Http {
        &self.http
    }

    pub fn set_ids(&self, ids: ClientIds) {
        *self.ids.write().unwrap_or_else(|e| e.into_inner()) = ids;
    }

    fn client_id(&self, s: Service) -> String {
        self.ids.read().unwrap_or_else(|e| e.into_inner()).get(s).to_owned()
    }

    pub fn can_sign_in(&self, s: Service) -> bool {
        !self.client_id(s).is_empty()
    }

    /// Whether public data can be requested.
    pub fn is_available(&self, s: Service) -> bool {
        s == Service::AniList || self.can_sign_in(s) || self.is_signed_in(s)
    }

    pub fn is_signed_in(&self, s: Service) -> bool {
        let mut signed = self.signed.lock().unwrap_or_else(|e| e.into_inner());
        *signed.entry(s).or_insert_with(|| matches!(self.secrets.load(s.into()), Ok(Some(_))))
    }

    // Sign-in

    pub async fn begin_login(&self, s: Service) -> Result<oauth::Login> {
        oauth::Login::start(s, &self.client_id(s)).await
    }

    /// Completes a sign-in without storing anything yet.
    pub async fn finish_login(
        &self,
        s: Service,
        login: oauth::Login,
    ) -> Result<(UserProfile, oauth::LoginResult)> {
        let throttle = match s {
            Service::AniList => self.anilist.throttle(),
            Service::Mal => self.mal.throttle(),
        };
        let result = login.finish(&self.http, throttle, (self.clock)()).await?;
        let profile = match s {
            Service::AniList => self.anilist.viewer(&result.tokens.access).await?,
            Service::Mal => self.mal.viewer(&result.tokens.access).await?,
        };
        Ok((profile, result))
    }

    pub fn accept_login(&self, s: Service, user: &UserProfile, tokens: &Tokens) -> Result<()> {
        self.secrets.save(s.into(), tokens)?;
        self.signed.lock().unwrap_or_else(|e| e.into_inner()).insert(s, true);
        self.users.lock().unwrap_or_else(|e| e.into_inner()).insert(s, user.id);
        Ok(())
    }

    pub fn sign_out(&self, s: Service) -> Result<()> {
        self.users.lock().unwrap_or_else(|e| e.into_inner()).remove(&s);
        self.secrets.delete(s.into())?;
        self.signed.lock().unwrap_or_else(|e| e.into_inner()).insert(s, false);
        Ok(())
    }

    async fn token(&self, s: Service) -> Result<Tokens> {
        let load = || self.secrets.load(s.into())?.ok_or(ApiError::Unauthorized);
        let tokens = load()?;
        if tokens.expires_at - EARLY > (self.clock)() {
            return Ok(tokens);
        }
        let _guard = self.refreshing.lock().await;
        let tokens = load()?;
        if tokens.expires_at - EARLY > (self.clock)() {
            return Ok(tokens);
        }
        if s == Service::AniList || tokens.refresh.is_none() {
            return if tokens.expires_at > (self.clock)() { Ok(tokens) } else { Err(ApiError::Unauthorized) };
        }
        let fresh = oauth::refresh(
            &self.http,
            self.mal.throttle(),
            &self.mal_endpoints,
            &self.client_id(s),
            &tokens,
            (self.clock)(),
        )
        .await?;
        self.secrets.save(s.into(), &fresh)?;
        Ok(fresh)
    }

    pub async fn profile(&self, s: Service) -> Result<UserProfile> {
        let t = self.token(s).await?;
        let p = match s {
            Service::AniList => self.anilist.viewer(&t.access).await?,
            Service::Mal => self.mal.viewer(&t.access).await?,
        };
        self.users.lock().unwrap_or_else(|e| e.into_inner()).insert(s, p.id);
        Ok(p)
    }

    async fn user_id(&self, s: Service) -> Result<u64> {
        let known = self.users.lock().unwrap_or_else(|e| e.into_inner()).get(&s).copied();
        match known {
            Some(id) => Ok(id),
            None => Ok(self.profile(s).await?.id),
        }
    }

    // Lists

    pub async fn library(&self, s: Service, kind: MediaKind) -> Result<Vec<ListEntry>> {
        let t = self.token(s).await?;
        match s {
            Service::AniList => {
                let user = self.user_id(s).await?;
                self.anilist.library(&t.access, user, kind).await
            }
            Service::Mal => self.mal.library(&t.access, kind).await,
        }
    }

    /// Sends one queued change.
    pub async fn push(&self, p: &Pending) -> Result<()> {
        let t = self.token(p.target).await?;
        match (&p.change, p.target) {
            (Change::Save(e), Service::AniList) => {
                let id = match (e.media.id_on(Service::AniList), e.media.mal_id) {
                    (Some(id), _) => id,
                    (None, Some(mal)) => self.anilist.id_from_mal(e.media.kind, mal).await?,
                    _ => return Err(ApiError::NotFound),
                };
                self.anilist.save(&t.access, id, e).await
            }
            (Change::Save(e), Service::Mal) => {
                let id = e.media.id_on(Service::Mal).ok_or(ApiError::NotFound)?;
                self.mal.save(&t.access, e.media.kind, id, e).await
            }
            (Change::Delete(m), Service::AniList) => {
                let user = self.user_id(Service::AniList).await?;
                self.anilist.delete(&t.access, user, m.id).await
            }
            (Change::Delete(m), Service::Mal) => self.mal.delete(&t.access, m.kind, m.id).await,
        }
    }

    // Browsing

    async fn mal_call<T, F>(&self, call: F) -> Result<T>
    where
        F: AsyncFnOnce(mal::Access<'_>) -> Result<T>,
    {
        match self.token(Service::Mal).await {
            Ok(t) => call(mal::Access::Token(&t.access)).await,
            Err(_) => {
                let id = self.client_id(Service::Mal);
                if id.is_empty() {
                    return Err(ApiError::NotConfigured);
                }
                call(mal::Access::Client(&id)).await
            }
        }
    }

    pub async fn search(
        &self,
        s: Service,
        kind: MediaKind,
        text: &str,
        adult: bool,
        page: u32,
    ) -> Result<Page<Media>> {
        match s {
            Service::AniList => self.anilist.search(kind, text, adult, page).await,
            Service::Mal => self.mal_call(async |a| self.mal.search(a, kind, text, adult, page).await).await,
        }
    }

    pub async fn season(&self, s: Service, q: &SeasonQuery, page: u32) -> Result<Page<Media>> {
        match s {
            Service::AniList => self.anilist.season(q, page).await,
            Service::Mal => self.mal_call(async |a| self.mal.season(a, q, page).await).await,
        }
    }

    pub async fn continuing(&self, s: Service, q: &SeasonQuery, page: u32) -> Result<Page<Media>> {
        match s {
            Service::AniList => self.anilist.continuing(q, page).await,
            Service::Mal => {
                // The service lists carried-over shows in the season itself.
                let rq = RankingQuery {
                    kind: MediaKind::Anime,
                    ranking: Ranking::Airing,
                    genre: None,
                    year: None,
                    adult: q.adult,
                };
                let mut p = self.mal_call(async |a| self.mal.ranking(a, &rq, page).await).await?;
                p.items.retain(|m| m.season != Some(q.season));
                Ok(p)
            }
        }
    }

    pub async fn ranking(&self, s: Service, q: &RankingQuery, page: u32) -> Result<Page<Media>> {
        match s {
            Service::AniList => self.anilist.ranking(q, page).await,
            Service::Mal => self.mal_call(async |a| self.mal.ranking(a, q, page).await).await,
        }
    }

    /// Details from the title's own service. MyAnimeList has no cast, staff or
    /// reviews, so those come from AniList when it knows the title.
    pub async fn detail(&self, r: MediaRef) -> Result<MediaDetail> {
        match r.service {
            Service::AniList => self.anilist.detail(r).await,
            Service::Mal => {
                let (mal, extra) = tokio::join!(
                    self.mal_call(async |a| self.mal.detail(a, r.kind, r.id).await),
                    self.anilist.detail(r)
                );
                let mut d = mal?;
                if let Ok(x) = extra {
                    d.media.anilist_id = Some(x.media.id);
                    d.media.next_airing = x.media.next_airing;
                    d.media.tags = x.media.tags;
                    d.characters = x.characters;
                    d.staff = x.staff;
                    d.reviews = x.reviews;
                    d.links = x.links;
                    d.trailer = x.trailer;
                    d.languages = x.languages;
                    d.score_stats = x.score_stats;
                    if d.status_stats.is_empty() {
                        d.status_stats = x.status_stats;
                    }
                }
                Ok(d)
            }
        }
    }

    /// A title as another service knows it.
    pub async fn resolve(&self, r: MediaRef, to: Service) -> Result<MediaRef> {
        if r.service == to {
            return Ok(r);
        }
        let id = match to {
            Service::AniList => self.anilist.id_from_mal(r.kind, r.id).await?,
            Service::Mal => {
                let m = self.anilist.by_ids(Service::AniList, r.kind, &[r.id]).await?;
                m.first().and_then(|m| m.mal_id).ok_or(ApiError::NotFound)?
            }
        };
        Ok(MediaRef { service: to, id, ..r })
    }

    pub async fn person(&self, id: u64) -> Result<PersonDetail> {
        self.anilist.person(id).await
    }

    /// Every title of a franchise, converted to the namespace of `root`.
    pub async fn overview(&self, root: &Media) -> Result<Vec<(Section, Vec<Media>)>> {
        let start = match root.service {
            Service::AniList => root.clone(),
            Service::Mal => self
                .anilist
                .by_ids(Service::Mal, root.kind, &[root.id])
                .await?
                .into_iter()
                .next()
                .ok_or(ApiError::NotFound)?,
        };
        let mut walk = Walk::start(start);
        loop {
            let ids: Vec<u64> = walk.next_batch(10).iter().map(|r| r.id).collect();
            if ids.is_empty() {
                break;
            }
            for (r, rels) in self.anilist.relations(root.kind, &ids).await? {
                walk.add(r, rels);
            }
        }
        let mut out = walk.finish();
        if root.service == Service::Mal {
            for (_, v) in &mut out {
                *v = std::mem::take(v).into_iter().filter_map(into_mal).collect();
            }
        }
        Ok(out)
    }

    /// Fresh airing data for titles of either service.
    pub async fn airing(&self, refs: &[MediaRef]) -> Result<Vec<Media>> {
        let mut out = Vec::new();
        for s in Service::ALL {
            let ids: Vec<u64> =
                refs.iter().filter(|r| r.service == s && r.kind == MediaKind::Anime).map(|r| r.id).collect();
            if ids.is_empty() {
                continue;
            }
            let found = self.anilist.by_ids(s, MediaKind::Anime, &ids).await?;
            out.extend(
                found.into_iter().filter_map(|m| if s == Service::Mal { into_mal(m) } else { Some(m) }),
            );
        }
        Ok(out)
    }
}

/// Re-keys an AniList title by its MyAnimeList id.
fn into_mal(m: Media) -> Option<Media> {
    Some(Media { service: Service::Mal, id: m.mal_id?, ..m })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::Duration;
    use wiremock::matchers::{any, body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn api(uri: &str, secrets: Arc<MemorySecrets>) -> Api {
        let http = Http::insecure_for_tests().unwrap();
        Api {
            anilist: anilist::AniList::with_endpoint(http.clone(), &format!("{uri}/gql")),
            mal: mal::Mal::with_endpoint(http.clone(), &format!("{uri}/mal")),
            music: music::Music::with_endpoints(http.clone(), uri, uri, uri, Duration::ZERO),
            crunchyroll: kioku::crunchyroll::Crunchyroll::new(secrets.clone()).unwrap(),
            http,
            secrets,
            ids: Arc::new(RwLock::new(ClientIds { anilist: "1".into(), mal: "cid".into() })),
            users: Arc::default(),
            signed: Arc::default(),
            refreshing: Arc::default(),
            mal_endpoints: oauth::Endpoints { authorize: String::new(), token: format!("{uri}/token") },
            clock: || 1000,
        }
    }

    fn tokens(expires_at: i64) -> Tokens {
        Tokens { access: "old".into(), refresh: Some("r1".into()), expires_at, device: None }
    }

    #[tokio::test]
    async fn expired_token_is_refreshed_exactly_once_under_concurrency() {
        let s = MockServer::start().await;
        Mock::given(path("/token"))
            .and(body_string_contains("refresh_token=r1"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"access_token": "new", "refresh_token": "r2", "expires_in": 9999}))
                    .set_delay(Duration::from_millis(50)),
            )
            .expect(1)
            .mount(&s)
            .await;
        let secrets = Arc::new(MemorySecrets::default());
        secrets.save(Service::Mal.into(), &tokens(0)).unwrap();
        let a = api(&s.uri(), secrets);
        let (x, y) = tokio::join!(a.token(Service::Mal), a.token(Service::Mal));
        assert_eq!((x.unwrap().access.clone(), y.unwrap().access.clone()), ("new".into(), "new".into()));
    }

    #[tokio::test]
    async fn rejected_refresh_asks_to_sign_in_again() {
        let s = MockServer::start().await;
        Mock::given(any()).respond_with(ResponseTemplate::new(400)).mount(&s).await;
        let secrets = Arc::new(MemorySecrets::default());
        secrets.save(Service::Mal.into(), &tokens(0)).unwrap();
        let r = api(&s.uri(), secrets).token(Service::Mal).await;
        assert_eq!(r.unwrap_err(), ApiError::Unauthorized);
    }

    #[tokio::test]
    async fn cross_service_save_translates_the_id() {
        let s = MockServer::start().await;
        Mock::given(path("/gql"))
            .and(body_string_contains("idMal"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"Media": {"id": 77}}})))
            .expect(1)
            .mount(&s)
            .await;
        Mock::given(path("/gql"))
            .and(body_string_contains("\"mediaId\":77"))
            .and(header("authorization", "Bearer old"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {}})))
            .expect(1)
            .mount(&s)
            .await;
        let secrets = Arc::new(MemorySecrets::default());
        secrets.save(Service::AniList.into(), &tokens(99_999)).unwrap();
        let media = Media { service: Service::Mal, id: 5, mal_id: Some(5), ..Media::default() };
        let p = Pending {
            target: Service::AniList,
            media: media.media_ref(),
            change: Change::Save(Box::new(ListEntry::new(media))),
            attempts: 0,
            revision: 1,
        };
        api(&s.uri(), secrets).push(&p).await.unwrap();
    }

    #[tokio::test]
    async fn signed_out_mal_uses_the_client_id() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(header("X-MAL-CLIENT-ID", "cid"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
            .expect(1)
            .mount(&s)
            .await;
        let a = api(&s.uri(), Arc::default());
        a.search(Service::Mal, MediaKind::Anime, "naruto", false, 1).await.unwrap();
        a.set_ids(ClientIds::default());
        let r = a.search(Service::Mal, MediaKind::Anime, "naruto", false, 1).await;
        assert_eq!(r, Err(ApiError::NotConfigured));
    }

    #[test]
    fn sign_out_removes_tokens() {
        let secrets = Arc::new(MemorySecrets::default());
        secrets.save(Service::Mal.into(), &tokens(0)).unwrap();
        let a = api("http://x", secrets);
        assert!(a.is_signed_in(Service::Mal));
        a.sign_out(Service::Mal).unwrap();
        assert!(!a.is_signed_in(Service::Mal));
    }
}
