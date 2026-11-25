//! AniList GraphQL client.

use std::time::Duration;

use mango_core::{
    Airing, CharacterRole, ExternalLink, Format, FuzzyDate, ListEntry, ListStatus, Media, MediaDetail,
    MediaKind, MediaRef, Person, PersonDetail, Relation, RelationKind, ReleaseStatus, Review, Season,
    SeasonYear, Service, StaffRole, UserProfile, VoiceRole,
};
use reqwest::StatusCode;
use serde_json::{Map, Value, json};

use crate::http::{ApiError, Http, Result, Throttle, decode};
use crate::{Page, Ranking, RankingQuery, SeasonQuery};

const ENDPOINT: &str = "https://graphql.anilist.co";
const PER_PAGE: usize = 50;

const SUMMARY: &str = "fragment M on Media{id idMal type title{romaji english native} synonyms \
coverImage{extraLarge large} format status(version:2) season seasonYear startDate{year month day} \
endDate{year month day} episodes chapters volumes duration source(version:3) genres averageScore meanScore \
popularity isAdult nextAiringEpisode{episode airingAt} studios(isMain:true){nodes{name}}}";

#[derive(Clone)]
pub struct AniList {
    http: Http,
    throttle: Throttle,
    endpoint: String,
}

impl AniList {
    pub fn new(http: Http) -> Self {
        // The public limit is 30 requests per minute.
        Self { http, throttle: Throttle::new(Duration::from_millis(2100)), endpoint: ENDPOINT.into() }
    }

    pub fn with_endpoint(http: Http, endpoint: &str) -> Self {
        Self { http, throttle: Throttle::new(Duration::ZERO), endpoint: endpoint.into() }
    }

    pub fn throttle(&self) -> &Throttle {
        &self.throttle
    }

    async fn query(&self, token: Option<&str>, query: &str, vars: Value) -> Result<Value> {
        let mut req = self
            .http
            .client()
            .post(&self.endpoint)
            .header("Accept", "application/json")
            .json(&json!({ "query": query, "variables": vars }));
        if let Some(t) = token {
            req = req.bearer_auth(t);
        }
        let res = self.http.send(req, &self.throttle).await?;
        let status = res.status();
        if status == StatusCode::UNAUTHORIZED {
            return Err(ApiError::Unauthorized);
        }
        if status.is_server_error() || status == StatusCode::FORBIDDEN {
            return Err(ApiError::Status(status.as_u16()));
        }
        let mut body: Value = decode(res).await?;
        if let Some(e) = body["errors"].as_array().and_then(|e| e.first()) {
            let message: String = e["message"].as_str().unwrap_or("error").chars().take(200).collect();
            return Err(match e["status"].as_u64() {
                Some(404) => ApiError::NotFound,
                Some(401) => ApiError::Unauthorized,
                _ if message == "Invalid token" => ApiError::Unauthorized,
                _ => ApiError::Service(message),
            });
        }
        if !status.is_success() {
            return Err(ApiError::Status(status.as_u16()));
        }
        Ok(body["data"].take())
    }

    async fn page(&self, args: &str, vars: Value, page: u32) -> Result<Page<Media>> {
        let decl = vars
            .as_object()
            .map(|m| m.keys().map(|k| format!("${k}:{}", var_type(k))).collect::<Vec<_>>().join(","))
            .unwrap_or_default();
        let decl = if decl.is_empty() { decl } else { format!("({decl})") };
        let q = format!(
            "query{decl}{{Page(page:{page},perPage:{PER_PAGE}){{pageInfo{{hasNextPage}} media({args}){{...M description(asHtml:false)}}}}}} {SUMMARY}"
        );
        let d = self.query(None, &q, vars).await?;
        Ok(Page {
            items: list(&d["Page"]["media"]).iter().filter_map(media).collect(),
            more: d["Page"]["pageInfo"]["hasNextPage"].as_bool().unwrap_or(false),
        })
    }

    pub async fn search(&self, kind: MediaKind, text: &str, adult: bool, page: u32) -> Result<Page<Media>> {
        let mut args = format!("type:{},sort:SEARCH_MATCH,search:$search", gql_kind(kind));
        if !adult {
            args.push_str(",isAdult:false");
        }
        self.page(&args, json!({ "search": text }), page).await
    }

    pub async fn season(&self, q: &SeasonQuery, page: u32) -> Result<Page<Media>> {
        let mut args = format!(
            "type:ANIME,season:{},seasonYear:{},sort:POPULARITY_DESC",
            q.season.season.key().to_uppercase(),
            q.season.year
        );
        if !q.adult {
            args.push_str(",isAdult:false");
        }
        self.page(&args, json!({}), page).await
    }

    /// Long-running shows from earlier seasons that are still airing.
    pub async fn continuing(&self, q: &SeasonQuery, page: u32) -> Result<Page<Media>> {
        let start = season_start(q.season);
        let mut args = format!(
            "type:ANIME,status:RELEASING,startDate_lesser:{start},sort:POPULARITY_DESC,format_not_in:[MUSIC]"
        );
        if !q.adult {
            args.push_str(",isAdult:false");
        }
        self.page(&args, json!({}), page).await
    }

    pub async fn ranking(&self, q: &RankingQuery, page: u32) -> Result<Page<Media>> {
        let (sort, extra) = match q.ranking {
            Ranking::All => ("SCORE_DESC", String::new()),
            Ranking::Airing => ("SCORE_DESC", ",status:RELEASING".into()),
            Ranking::Upcoming => ("POPULARITY_DESC", ",status:NOT_YET_RELEASED".into()),
            Ranking::Popularity => ("POPULARITY_DESC", String::new()),
            Ranking::Favorites => ("FAVOURITES_DESC", String::new()),
            Ranking::Trending => ("TRENDING_DESC", String::new()),
            Ranking::Format(f) => ("SCORE_DESC", ranking_format(f)),
        };
        let mut args = format!("type:{},sort:{sort}{extra}", gql_kind(q.kind));
        let mut vars = Map::new();
        if let Some(g) = &q.genre {
            args.push_str(",genre:$genre");
            vars.insert("genre".into(), json!(g));
        }
        if let Some(y) = q.year {
            args.push_str(&format!(",startDate_greater:{}1231,startDate_lesser:{}0101", y - 1, y + 1));
        }
        if !q.adult {
            args.push_str(",isAdult:false");
        }
        self.page(&args, Value::Object(vars), page).await
    }

    pub async fn genres(&self) -> Result<Vec<String>> {
        let d = self.query(None, "query{GenreCollection}", json!({})).await?;
        Ok(strings(&d["GenreCollection"]))
    }

    /// Titles by id, in the namespace of `by`.
    pub async fn by_ids(&self, by: Service, kind: MediaKind, ids: &[u64]) -> Result<Vec<Media>> {
        let field = match by {
            Service::AniList => "id_in",
            Service::Mal => "idMal_in",
        };
        let mut out = Vec::new();
        for chunk in ids.chunks(PER_PAGE) {
            let args = format!("type:{},{field}:$ids", gql_kind(kind));
            out.extend(self.page(&args, json!({ "ids": chunk }), 1).await?.items);
        }
        Ok(out)
    }

    /// Relations of several titles at once, for the series overview.
    pub async fn relations(&self, kind: MediaKind, ids: &[u64]) -> Result<Vec<(MediaRef, Vec<Relation>)>> {
        let q = format!(
            "query($ids:[Int]){{Page(perPage:10){{media(id_in:$ids,type:{}){{id relations{{edges{{\
             relationType(version:2) node{{...M}}}}}}}}}}}} {SUMMARY}",
            gql_kind(kind)
        );
        let mut out = Vec::new();
        for chunk in ids.chunks(10) {
            let d = self.query(None, &q, json!({ "ids": chunk })).await?;
            for m in list(&d["Page"]["media"]) {
                if let Some(id) = m["id"].as_u64() {
                    out.push((MediaRef { service: Service::AniList, kind, id }, relations(&m["relations"])));
                }
            }
        }
        Ok(out)
    }

    /// Full details, looked up by an id of either service.
    pub async fn detail(&self, r: MediaRef) -> Result<MediaDetail> {
        let key = match r.service {
            Service::AniList => "id",
            Service::Mal => "idMal",
        };
        let q = format!(
            "query($id:Int){{Media({key}:$id,type:{}){{...M description(asHtml:false) rankings{{rank type allTime}} \
             tags{{name rank isMediaSpoiler}} trailer{{id site}} externalLinks{{site url type}} \
             stats{{scoreDistribution{{score amount}} statusDistribution{{status amount}}}} \
             relations{{edges{{relationType(version:2) node{{...M}}}}}} \
             characters(sort:[ROLE,RELEVANCE,ID],perPage:25){{edges{{role node{{id name{{full}} image{{large}}}} \
             voiceActors{{id name{{full}} image{{large}} languageV2}}}}}} \
             staff(sort:[RELEVANCE],perPage:12){{edges{{role node{{id name{{full}} image{{large}}}}}}}} \
             recommendations(sort:RATING_DESC,perPage:20){{nodes{{mediaRecommendation{{...M}}}}}} \
             reviews(sort:RATING_DESC,perPage:8){{nodes{{id summary body(asHtml:false) score rating ratingAmount \
             createdAt user{{name}}}}}}}}}} {SUMMARY}",
            gql_kind(r.kind)
        );
        let d = self.query(None, &q, json!({ "id": r.id })).await?;
        detail(&d["Media"]).ok_or_else(|| ApiError::Decode("media".into()))
    }

    pub async fn person(&self, id: u64) -> Result<PersonDetail> {
        let q = format!(
            "query($id:Int){{Staff(id:$id){{id name{{full native}} image{{large}} languageV2 \
             description(asHtml:false) characterMedia(sort:POPULARITY_DESC,perPage:50){{edges{{characterRole \
             characters{{name{{full}} image{{large}}}} node{{...M}}}}}}}}}} {SUMMARY}"
        );
        let d = self.query(None, &q, json!({ "id": id })).await?;
        let s = &d["Staff"];
        let mut roles = Vec::new();
        for e in list(&s["characterMedia"]["edges"]) {
            let Some(media) = media(&e["node"]) else { continue };
            for c in list(&e["characters"]).iter().filter(|c| !c.is_null()) {
                roles.push(VoiceRole {
                    character: text(&c["name"]["full"]),
                    character_image: opt_text(&c["image"]["large"]),
                    role: text(&e["characterRole"]),
                    media: media.clone(),
                });
            }
        }
        Ok(PersonDetail {
            person: person(s),
            native_name: opt_text(&s["name"]["native"]),
            description: opt_text(&s["description"]).map(|t| clean(&t)),
            roles,
        })
    }

    /// A random title of the given kind.
    pub async fn random(&self, kind: MediaKind, adult: bool, seed: u64) -> Result<Media> {
        // Pages past 100 are not served reliably, so pick among the popular titles.
        let n = seed % 5000;
        let mut args = format!("type:{},sort:POPULARITY_DESC", gql_kind(kind));
        if !adult {
            args.push_str(",isAdult:false");
        }
        let page = (n / PER_PAGE as u64) as u32 + 1;
        let mut items = self.page(&args, json!({}), page).await?.items;
        if items.is_empty() {
            return Err(ApiError::NotFound);
        }
        let i = (n % PER_PAGE as u64) as usize % items.len();
        Ok(items.swap_remove(i))
    }

    // Account

    pub async fn viewer(&self, token: &str) -> Result<UserProfile> {
        let d = self.query(Some(token), "query{Viewer{id name avatar{large}}}", json!({})).await?;
        let v = &d["Viewer"];
        Ok(UserProfile {
            id: v["id"].as_u64().ok_or_else(|| ApiError::Decode("viewer".into()))?,
            name: text(&v["name"]),
            avatar: opt_text(&v["avatar"]["large"]),
        })
    }

    pub async fn library(&self, token: &str, user: u64, kind: MediaKind) -> Result<Vec<ListEntry>> {
        let q = format!(
            "query($user:Int,$chunk:Int){{MediaListCollection(userId:$user,type:{},chunk:$chunk,perChunk:500){{\
             hasNextChunk lists{{isCustomList entries{{status score(format:POINT_100) progress progressVolumes \
             repeat notes startedAt{{year month day}} completedAt{{year month day}} updatedAt media{{...M}}}}}}}}}} \
             {SUMMARY}",
            gql_kind(kind)
        );
        let mut out: Vec<ListEntry> = Vec::new();
        for chunk in 1..=40 {
            let d = self.query(Some(token), &q, json!({ "user": user, "chunk": chunk })).await?;
            let c = &d["MediaListCollection"];
            for l in list(&c["lists"]) {
                if l["isCustomList"].as_bool() == Some(true) {
                    continue;
                }
                out.extend(list(&l["entries"]).iter().filter_map(entry));
            }
            if c["hasNextChunk"].as_bool() != Some(true) {
                break;
            }
        }
        Ok(out)
    }

    pub async fn save(&self, token: &str, media_id: u64, e: &ListEntry) -> Result<()> {
        let q = "mutation($mediaId:Int,$status:MediaListStatus,$score:Int,$progress:Int,$volumes:Int,$repeat:Int,\
                 $notes:String,$start:FuzzyDateInput,$finish:FuzzyDateInput){SaveMediaListEntry(mediaId:$mediaId,\
                 status:$status,scoreRaw:$score,progress:$progress,progressVolumes:$volumes,repeat:$repeat,\
                 notes:$notes,startedAt:$start,completedAt:$finish){id}}";
        let vars = json!({
            "mediaId": media_id,
            "status": gql_status(e),
            "score": e.score,
            "progress": e.progress,
            "volumes": e.progress_volumes,
            "repeat": e.repeat_count,
            "notes": e.notes,
            "start": gql_date(e.start),
            "finish": gql_date(e.finish),
        });
        self.query(Some(token), q, vars).await.map(drop)
    }

    pub async fn delete(&self, token: &str, user: u64, media_id: u64) -> Result<()> {
        let q = "query($user:Int,$id:Int){MediaList(userId:$user,mediaId:$id){id}}";
        let d = match self.query(Some(token), q, json!({ "user": user, "id": media_id })).await {
            Err(ApiError::NotFound) => return Ok(()),
            other => other?,
        };
        let id = d["MediaList"]["id"].as_u64().ok_or_else(|| ApiError::Decode("entry".into()))?;
        let q = "mutation($id:Int){DeleteMediaListEntry(id:$id){deleted}}";
        self.query(Some(token), q, json!({ "id": id })).await.map(drop)
    }

    /// Translates a MyAnimeList id.
    pub async fn id_from_mal(&self, kind: MediaKind, mal: u64) -> Result<u64> {
        let q = format!("query($id:Int){{Media(idMal:$id,type:{}){{id}}}}", gql_kind(kind));
        let d = self.query(None, &q, json!({ "id": mal })).await?;
        d["Media"]["id"].as_u64().ok_or(ApiError::NotFound)
    }
}

// Conversion

fn var_type(name: &str) -> &'static str {
    match name {
        "ids" => "[Int]",
        _ => "String",
    }
}

fn gql_kind(kind: MediaKind) -> &'static str {
    match kind {
        MediaKind::Anime => "ANIME",
        MediaKind::Manga => "MANGA",
    }
}

fn gql_status(e: &ListEntry) -> &'static str {
    match e.status {
        ListStatus::Current if e.repeating => "REPEATING",
        ListStatus::Current => "CURRENT",
        ListStatus::Completed => "COMPLETED",
        ListStatus::Paused => "PAUSED",
        ListStatus::Dropped => "DROPPED",
        ListStatus::Planning => "PLANNING",
    }
}

fn gql_date(d: Option<FuzzyDate>) -> Value {
    match d {
        Some(d) => json!({ "year": d.year, "month": d.month, "day": d.day }),
        None => json!({ "year": null, "month": null, "day": null }),
    }
}

fn season_start(s: SeasonYear) -> String {
    let month = match s.season {
        Season::Winter => 1,
        Season::Spring => 4,
        Season::Summer => 7,
        Season::Fall => 10,
    };
    format!("{:04}{month:02}01", s.year)
}

fn ranking_format(f: Format) -> String {
    match f {
        Format::Tv => ",format_in:[TV,TV_SHORT]".into(),
        Format::Movie => ",format:MOVIE".into(),
        Format::Ova => ",format:OVA".into(),
        Format::Ona => ",format:ONA".into(),
        Format::Special => ",format:SPECIAL".into(),
        Format::Novel => ",format:NOVEL".into(),
        Format::OneShot => ",format:ONE_SHOT".into(),
        Format::Manhwa => ",countryOfOrigin:KR".into(),
        Format::Manhua => ",countryOfOrigin:CN".into(),
        Format::Manga => ",format:MANGA,countryOfOrigin:JP".into(),
        _ => String::new(),
    }
}

fn list(v: &Value) -> &[Value] {
    v.as_array().map_or(&[], Vec::as_slice)
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().trim().to_owned()
}

fn opt_text(v: &Value) -> Option<String> {
    Some(text(v)).filter(|s| !s.is_empty())
}

fn strings(v: &Value) -> Vec<String> {
    list(v).iter().filter_map(opt_text).collect()
}

fn num(v: &Value) -> Option<u32> {
    v.as_u64().and_then(|n| u32::try_from(n).ok())
}

fn date(v: &Value) -> Option<FuzzyDate> {
    FuzzyDate::new(i32::try_from(v["year"].as_i64()?).ok()?, num(&v["month"]), num(&v["day"]))
}

fn format_of(s: &str) -> Format {
    match s {
        "TV" => Format::Tv,
        "TV_SHORT" => Format::TvShort,
        "MOVIE" => Format::Movie,
        "SPECIAL" => Format::Special,
        "OVA" => Format::Ova,
        "ONA" => Format::Ona,
        "MUSIC" => Format::Music,
        "MANGA" => Format::Manga,
        "NOVEL" => Format::Novel,
        "ONE_SHOT" => Format::OneShot,
        _ => Format::Unknown,
    }
}

/// `LIGHT_NOVEL` → `Light Novel`.
fn humanize(s: &str) -> String {
    s.split('_')
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c.flat_map(char::to_lowercase)).collect())
                .unwrap_or_default()
        })
        .collect::<Vec<String>>()
        .join(" ")
}

pub(crate) fn media(v: &Value) -> Option<Media> {
    let id = v["id"].as_u64()?;
    let kind = match v["type"].as_str()? {
        "ANIME" => MediaKind::Anime,
        "MANGA" => MediaKind::Manga,
        _ => return None,
    };
    let t = &v["title"];
    let english = opt_text(&t["english"]);
    let native = opt_text(&t["native"]);
    let season = match v["season"].as_str() {
        Some("WINTER") => Some(Season::Winter),
        Some("SPRING") => Some(Season::Spring),
        Some("SUMMER") => Some(Season::Summer),
        Some("FALL") => Some(Season::Fall),
        _ => None,
    };
    Some(Media {
        service: Service::AniList,
        kind,
        id,
        mal_id: v["idMal"].as_u64(),
        anilist_id: Some(id),
        title: opt_text(&t["romaji"])
            .or_else(|| english.clone())
            .or_else(|| native.clone())
            .unwrap_or_default(),
        title_english: english,
        title_native: native,
        synonyms: strings(&v["synonyms"]),
        cover: opt_text(&v["coverImage"]["extraLarge"]).or_else(|| opt_text(&v["coverImage"]["large"])),
        format: format_of(v["format"].as_str().unwrap_or_default()),
        release: match v["status"].as_str() {
            Some("FINISHED") => ReleaseStatus::Finished,
            Some("RELEASING") => ReleaseStatus::Releasing,
            Some("NOT_YET_RELEASED") => ReleaseStatus::NotYetReleased,
            Some("CANCELLED") => ReleaseStatus::Cancelled,
            Some("HIATUS") => ReleaseStatus::Hiatus,
            _ => ReleaseStatus::Unknown,
        },
        season: season.and_then(|season| {
            Some(SeasonYear { season, year: i32::try_from(v["seasonYear"].as_i64()?).ok()? })
        }),
        synopsis: opt_text(&v["description"]).map(|t| clean(&t)),
        start: date(&v["startDate"]),
        end: date(&v["endDate"]),
        episodes: num(&v["episodes"]),
        chapters: num(&v["chapters"]),
        volumes: num(&v["volumes"]),
        duration: num(&v["duration"]),
        source: opt_text(&v["source"]).map(|s| humanize(&s)),
        genres: strings(&v["genres"]),
        studios: list(&v["studios"]["nodes"]).iter().filter_map(|s| opt_text(&s["name"])).collect(),
        mean_score: v["averageScore"].as_f64().or_else(|| v["meanScore"].as_f64()).map(|s| s as f32 / 10.0),
        members: v["popularity"].as_u64(),
        adult: v["isAdult"].as_bool().unwrap_or(false),
        next_airing: v["nextAiringEpisode"]["airingAt"]
            .as_i64()
            .and_then(|at| Some(Airing { episode: num(&v["nextAiringEpisode"]["episode"])?, at })),
        ..Media::default()
    })
}

fn relations(v: &Value) -> Vec<Relation> {
    list(&v["edges"])
        .iter()
        .filter_map(|e| {
            let kind = match e["relationType"].as_str()? {
                "SEQUEL" => RelationKind::Sequel,
                "PREQUEL" => RelationKind::Prequel,
                "PARENT" => RelationKind::Parent,
                "SIDE_STORY" => RelationKind::SideStory,
                "SPIN_OFF" => RelationKind::SpinOff,
                "ALTERNATIVE" => RelationKind::Alternative,
                "ADAPTATION" => RelationKind::Adaptation,
                "SOURCE" => RelationKind::Source,
                "SUMMARY" => RelationKind::Summary,
                "CHARACTER" => RelationKind::Character,
                "COMPILATION" => RelationKind::Compilation,
                "CONTAINS" => RelationKind::Contains,
                _ => RelationKind::Other,
            };
            Some(Relation { kind, media: media(&e["node"])? })
        })
        .collect()
}

fn person(v: &Value) -> Person {
    Person {
        id: v["id"].as_u64().unwrap_or(0),
        name: text(&v["name"]["full"]),
        image: opt_text(&v["image"]["large"]),
        language: opt_text(&v["languageV2"]),
    }
}

fn status_of(s: &str) -> Option<(ListStatus, bool)> {
    Some(match s {
        "CURRENT" => (ListStatus::Current, false),
        "REPEATING" => (ListStatus::Current, true),
        "COMPLETED" => (ListStatus::Completed, false),
        "PAUSED" => (ListStatus::Paused, false),
        "DROPPED" => (ListStatus::Dropped, false),
        "PLANNING" => (ListStatus::Planning, false),
        _ => return None,
    })
}

fn entry(v: &Value) -> Option<ListEntry> {
    let (status, repeating) = status_of(v["status"].as_str()?)?;
    Some(ListEntry {
        media: media(&v["media"])?,
        status,
        repeating,
        progress: num(&v["progress"]).unwrap_or(0),
        progress_volumes: num(&v["progressVolumes"]).unwrap_or(0),
        score: v["score"].as_f64().unwrap_or(0.0).clamp(0.0, 100.0).round() as u8,
        start: date(&v["startedAt"]),
        finish: date(&v["completedAt"]),
        repeat_count: num(&v["repeat"]).unwrap_or(0),
        notes: v["notes"].as_str().unwrap_or_default().to_owned(),
        updated_at: v["updatedAt"].as_i64().unwrap_or(0),
    })
}

fn detail(v: &Value) -> Option<MediaDetail> {
    let mut m = media(v)?;
    m.tags = list(&v["tags"])
        .iter()
        .filter(|t| t["isMediaSpoiler"].as_bool() != Some(true) && t["rank"].as_u64().unwrap_or(0) >= 60)
        .filter_map(|t| opt_text(&t["name"]))
        .collect();
    for r in list(&v["rankings"]) {
        if r["allTime"].as_bool() == Some(true) {
            match r["type"].as_str() {
                Some("RATED") => m.rank = num(&r["rank"]),
                Some("POPULAR") => m.popularity = num(&r["rank"]),
                _ => {}
            }
        }
    }
    let mut characters = Vec::new();
    let mut languages: Vec<String> = Vec::new();
    for e in list(&v["characters"]["edges"]) {
        let voices: Vec<Person> = list(&e["voiceActors"]).iter().map(person).collect();
        for l in voices.iter().filter_map(|p| p.language.as_ref()) {
            if !languages.contains(l) {
                languages.push(l.clone());
            }
        }
        characters.push(CharacterRole {
            id: e["node"]["id"].as_u64().unwrap_or(0),
            name: text(&e["node"]["name"]["full"]),
            image: opt_text(&e["node"]["image"]["large"]),
            role: humanize(e["role"].as_str().unwrap_or_default()),
            voices,
        });
    }
    languages.sort_by_key(|l| (l != "Japanese", l.clone()));
    let score_stats: Vec<(u32, u64)> = list(&v["stats"]["scoreDistribution"])
        .iter()
        .filter_map(|s| Some((num(&s["score"])?, s["amount"].as_u64()?)))
        .collect();
    m.scored_by = Some(score_stats.iter().map(|s| s.1).sum()).filter(|n| *n > 0);
    Some(MediaDetail {
        relations: relations(&v["relations"]),
        recommendations: list(&v["recommendations"]["nodes"])
            .iter()
            .filter_map(|n| media(&n["mediaRecommendation"]))
            .collect(),
        characters,
        staff: list(&v["staff"]["edges"])
            .iter()
            .map(|e| StaffRole { person: person(&e["node"]), role: text(&e["role"]) })
            .collect(),
        status_stats: list(&v["stats"]["statusDistribution"])
            .iter()
            .filter_map(|s| Some((status_of(s["status"].as_str()?)?.0, s["amount"].as_u64()?)))
            .collect(),
        score_stats,
        reviews: list(&v["reviews"]["nodes"])
            .iter()
            .map(|r| Review {
                id: r["id"].as_u64().unwrap_or(0),
                user: text(&r["user"]["name"]),
                summary: text(&r["summary"]),
                body: clean(r["body"].as_str().unwrap_or_default()),
                score: num(&r["score"]),
                votes_up: num(&r["rating"]).unwrap_or(0),
                votes: num(&r["ratingAmount"]).unwrap_or(0),
                created_at: r["createdAt"].as_i64().unwrap_or(0),
            })
            .collect(),
        links: list(&v["externalLinks"])
            .iter()
            .filter_map(|l| {
                let url =
                    opt_text(&l["url"]).filter(|u| u.starts_with("https://") || u.starts_with("http://"))?;
                Some(ExternalLink { site: text(&l["site"]), url, streaming: l["type"] == "STREAMING" })
            })
            .collect(),
        trailer: opt_text(&v["trailer"]["id"])
            .filter(|id| id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
            .and_then(|id| match v["trailer"]["site"].as_str() {
                Some("youtube") => Some(format!("https://www.youtube.com/watch?v={id}")),
                Some("dailymotion") => Some(format!("https://www.dailymotion.com/video/{id}")),
                _ => None,
            }),
        languages,
        themes: Vec::new(),
        media: m,
    })
}

/// Turns AniList's HTML/markdown mix into plain text.
pub fn clean(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        let tag = &rest[i..];
        let end = tag.find('>').filter(|e| *e < 200);
        let is_tag = tag[1..].starts_with(|c: char| c.is_ascii_alphabetic() || c == '/' || c == '!');
        match end.filter(|_| is_tag) {
            Some(e) => {
                let name = tag[1..e].trim_start_matches('/').to_ascii_lowercase();
                if name.starts_with("br") || name == "p" {
                    out.push('\n');
                }
                rest = &tag[e + 1..];
            }
            None => {
                out.push('<');
                rest = &tag[1..];
            }
        }
    }
    out.push_str(rest);
    for (from, to) in [
        ("&quot;", "\""),
        ("&#039;", "'"),
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&nbsp;", " "),
        ("&mdash;", "—"),
        ("&amp;", "&"),
        ("~~~", ""),
        ("__", ""),
        ("**", ""),
        ("~!", ""),
        ("!~", ""),
        ("\r\n", "\n"),
    ] {
        out = out.replace(from, to);
    }
    while out.contains("\n\n\n") {
        out = out.replace("\n\n\n", "\n\n");
    }
    out.lines().map(str::trim_end).collect::<Vec<_>>().join("\n").trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{any, body_string_contains, header};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn server(body: Value) -> (MockServer, AniList) {
        let s = MockServer::start().await;
        Mock::given(any()).respond_with(ResponseTemplate::new(200).set_body_json(body)).mount(&s).await;
        let a = AniList::with_endpoint(Http::insecure_for_tests().unwrap(), &s.uri());
        (s, a)
    }

    fn media_json(id: u64) -> Value {
        json!({"id": id, "idMal": id + 1, "type": "ANIME", "title": {"romaji": "R", "english": null},
               "coverImage": {"large": "https://s4.anilist.co/x.jpg"}, "format": "TV", "status": "RELEASING",
               "season": "FALL", "seasonYear": 2026, "startDate": {"year": 2026, "month": 10, "day": null},
               "averageScore": 91, "nextAiringEpisode": {"episode": 3, "airingAt": 99}, "source": "LIGHT_NOVEL"})
    }

    #[tokio::test]
    async fn parses_pages_and_skips_broken_items() {
        let (_s, a) = server(json!({"data": {"Page": {"pageInfo": {"hasNextPage": true},
            "media": [media_json(1), null, {"id": 2}]}}}))
        .await;
        let p = a.search(MediaKind::Anime, "x", false, 1).await.unwrap();
        assert!(p.more);
        let m = &p.items[0];
        assert_eq!(p.items.len(), 1);
        assert_eq!((m.mal_id, m.mean_score, m.release), (Some(2), Some(9.1), ReleaseStatus::Releasing));
        assert_eq!(m.next_airing, Some(Airing { episode: 3, at: 99 }));
        assert_eq!(m.source.as_deref(), Some("Light Novel"));
        assert_eq!(m.start.unwrap().iso(), "2026-10");
    }

    #[tokio::test]
    async fn search_text_is_sent_as_a_variable() {
        let s = MockServer::start().await;
        Mock::given(body_string_contains(r#""search":"a\"){hack}""#))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"Page": {"media": []}}})))
            .expect(1)
            .mount(&s)
            .await;
        let a = AniList::with_endpoint(Http::insecure_for_tests().unwrap(), &s.uri());
        a.search(MediaKind::Anime, "a\"){hack}", false, 1).await.unwrap();
    }

    #[tokio::test]
    async fn library_merges_chunks_and_ignores_custom_lists() {
        let entry = json!({"status": "REPEATING", "score": 85, "progress": 4, "repeat": 2, "notes": null,
            "startedAt": {"year": 2020, "month": 1, "day": 2}, "completedAt": {"year": null},
            "updatedAt": 7, "media": media_json(1)});
        let (_s, a) = server(json!({"data": {"MediaListCollection": {"hasNextChunk": false, "lists": [
            {"isCustomList": false, "entries": [entry]}, {"isCustomList": true, "entries": [entry]}]}}}))
        .await;
        let l = a.library("t", 1, MediaKind::Anime).await.unwrap();
        assert_eq!(l.len(), 1);
        assert_eq!(
            (l[0].status, l[0].repeating, l[0].score, l[0].repeat_count),
            (ListStatus::Current, true, 85, 2)
        );
        assert_eq!(l[0].finish, None);
    }

    #[tokio::test]
    async fn save_sends_the_token_and_full_state() {
        let s = MockServer::start().await;
        Mock::given(header("authorization", "Bearer tok"))
            .and(body_string_contains("\"status\":\"REPEATING\""))
            .and(body_string_contains("\"score\":70"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {}})))
            .expect(1)
            .mount(&s)
            .await;
        let a = AniList::with_endpoint(Http::insecure_for_tests().unwrap(), &s.uri());
        let e = ListEntry { status: ListStatus::Current, repeating: true, score: 70, ..ListEntry::default() };
        a.save("tok", 5, &e).await.unwrap();
    }

    #[tokio::test]
    async fn deleting_a_missing_entry_succeeds() {
        let (_s, a) = server(
            json!({"errors": [{"message": "Not Found.", "status": 404}], "data": {"MediaList": null}}),
        )
        .await;
        assert_eq!(a.delete("t", 1, 2).await, Ok(()));
    }

    #[tokio::test]
    async fn graphql_errors_are_mapped() {
        let (_s, a) = server(json!({"errors": [{"message": "Invalid token", "status": 400}]})).await;
        assert_eq!(a.viewer("t").await, Err(ApiError::Unauthorized));
    }

    #[tokio::test]
    async fn detail_collects_languages_and_filters_unsafe_links() {
        let mut m = media_json(1);
        m["description"] = json!("Hello<br><br><i>world</i> &amp; 1 < 2");
        m["rankings"] = json!([{"rank": 4, "type": "RATED", "allTime": true}, {"rank": 1, "type": "RATED", "allTime": false}]);
        m["characters"] = json!({"edges": [{"role": "MAIN", "node": {"id": 1, "name": {"full": "C"}}, "voiceActors": [
            {"id": 2, "name": {"full": "E"}, "languageV2": "English"}, {"id": 3, "name": {"full": "J"}, "languageV2": "Japanese"}]}]});
        m["externalLinks"] = json!([{"site": "ok", "url": "https://a.b", "type": "STREAMING"}, {"site": "bad", "url": "file:///c:/x"}]);
        m["trailer"] = json!({"id": "ab&c", "site": "youtube"});
        m["stats"] = json!({"scoreDistribution": [{"score": 90, "amount": 3}], "statusDistribution": [{"status": "CURRENT", "amount": 8}]});
        let (_s, a) = server(json!({"data": {"Media": m}})).await;
        let r = MediaRef { service: Service::AniList, kind: MediaKind::Anime, id: 1 };
        let d = a.detail(r).await.unwrap();
        assert_eq!(d.media.synopsis.as_deref(), Some("Hello\n\nworld & 1 < 2"));
        assert_eq!((d.media.rank, d.media.scored_by), (Some(4), Some(3)));
        assert_eq!(d.languages, ["Japanese", "English"]);
        assert_eq!(d.links.len(), 1);
        assert_eq!(d.trailer, None);
        assert_eq!(d.status_stats, [(ListStatus::Current, 8)]);
    }

    #[test]
    fn clean_handles_odd_markup() {
        assert_eq!(clean("a <3 b"), "a <3 b");
        assert_eq!(clean("<p>x</p>"), "x");
        assert_eq!(clean("unterminated <b"), "unterminated <b");
        assert_eq!(clean("é<br/>ü"), "é\nü");
    }
}
