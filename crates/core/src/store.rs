//! SQLite storage: library, pending changes, response cache and settings.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use thiserror::Error;

use crate::model::{ListEntry, MediaKind, MediaRef, Service};
use crate::settings::Settings;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("invalid data: {0}")]
    Json(#[from] serde_json::Error),
    #[error("database was created by a newer version ({0})")]
    TooNew(i64),
    #[error("backup has an unsupported format")]
    BadBackup,
}

type Result<T> = std::result::Result<T, StoreError>;

/// A change that still has to reach a remote service.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    Save(Box<ListEntry>),
    Delete(MediaRef),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pending {
    /// Service the change is sent to.
    pub target: Service,
    /// Identity of the title in the list it was edited from.
    pub media: MediaRef,
    pub change: Change,
    pub attempts: u32,
    /// Distinguishes this edit from a newer one to the same title.
    pub revision: i64,
}

#[derive(Serialize, Deserialize)]
struct Backup {
    format: u32,
    entries: Vec<ListEntry>,
}

pub struct Store {
    db: Connection,
}

const SCHEMA_VERSION: i64 = 1;
const BACKUP_FORMAT: u32 = 1;

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        Self::init(Connection::open(path)?)
    }

    pub fn in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(db: Connection) -> Result<Self> {
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.pragma_update(None, "trusted_schema", false)?;
        let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(StoreError::TooNew(version));
        }
        if version < 1 {
            db.execute_batch(
                "BEGIN;
                 CREATE TABLE settings(id INTEGER PRIMARY KEY CHECK(id=1), json TEXT NOT NULL);
                 CREATE TABLE entries(service TEXT NOT NULL, kind TEXT NOT NULL, id INTEGER NOT NULL,
                    json TEXT NOT NULL, PRIMARY KEY(service, kind, id)) WITHOUT ROWID;
                 CREATE TABLE outbox(revision INTEGER PRIMARY KEY AUTOINCREMENT, target TEXT NOT NULL,
                    service TEXT NOT NULL, kind TEXT NOT NULL, id INTEGER NOT NULL, json TEXT NOT NULL,
                    attempts INTEGER NOT NULL DEFAULT 0, error TEXT,
                    UNIQUE(target, service, kind, id));
                 CREATE TABLE cache(key TEXT PRIMARY KEY, json TEXT NOT NULL,
                    stored_at INTEGER NOT NULL) WITHOUT ROWID;
                 CREATE TABLE notified(key TEXT PRIMARY KEY, at INTEGER NOT NULL) WITHOUT ROWID;
                 PRAGMA user_version=1;
                 COMMIT;",
            )?;
        }
        Ok(Self { db })
    }

    // Settings

    pub fn settings(&self) -> Result<Settings> {
        let json: Option<String> =
            self.db.query_row("SELECT json FROM settings WHERE id=1", [], |r| r.get(0)).optional()?;
        // A damaged settings row must never prevent startup.
        Ok(json.and_then(|j| serde_json::from_str::<Settings>(&j).ok()).unwrap_or_default().normalized())
    }

    pub fn save_settings(&self, s: &Settings) -> Result<()> {
        self.db.execute(
            "INSERT INTO settings(id,json) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET json=?1",
            [serde_json::to_string(s)?],
        )?;
        Ok(())
    }

    // Library

    pub fn entries(&self, service: Service, kind: MediaKind) -> Result<Vec<ListEntry>> {
        let mut q = self.db.prepare_cached("SELECT json FROM entries WHERE service=?1 AND kind=?2")?;
        let rows = q.query_map([service.key(), kind.key()], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(serde_json::from_str(&row?)?);
        }
        Ok(out)
    }

    pub fn entry(&self, m: MediaRef) -> Result<Option<ListEntry>> {
        let json: Option<String> = self
            .db
            .query_row(
                "SELECT json FROM entries WHERE service=?1 AND kind=?2 AND id=?3",
                params![m.service.key(), m.kind.key(), m.id as i64],
                |r| r.get(0),
            )
            .optional()?;
        Ok(json.map(|j| serde_json::from_str(&j)).transpose()?)
    }

    pub fn put_entry(&self, e: &ListEntry) -> Result<()> {
        put_entry(&self.db, e)
    }

    pub fn delete_entry(&self, m: MediaRef) -> Result<()> {
        delete_entry(&self.db, m)
    }

    /// Saves an edit locally and queues it for `targets`, atomically.
    pub fn save_and_queue(&mut self, e: &ListEntry, targets: &[Service]) -> Result<()> {
        let tx = self.db.transaction()?;
        put_entry(&tx, e)?;
        for t in targets {
            queue(&tx, *t, e.media.media_ref(), &Change::Save(Box::new(e.clone())))?;
        }
        Ok(tx.commit()?)
    }

    pub fn delete_and_queue(&mut self, e: &ListEntry, targets: &[Service]) -> Result<()> {
        let m = e.media.media_ref();
        let tx = self.db.transaction()?;
        delete_entry(&tx, m)?;
        for t in targets {
            // Other services know the title under their own id.
            if let Some(id) = e.media.id_on(*t) {
                queue(&tx, *t, m, &Change::Delete(MediaRef { service: *t, id, ..m }))?;
            }
        }
        Ok(tx.commit()?)
    }

    /// Replaces one list with the remote copy, keeping edits that have not been
    /// sent yet. With `merge`, local titles missing from the remote copy are kept
    /// and queued for upload instead of dropped.
    pub fn replace_library(
        &mut self,
        service: Service,
        kind: MediaKind,
        remote: &[ListEntry],
        merge: bool,
    ) -> Result<()> {
        let keep: Vec<ListEntry> = if merge {
            let known: std::collections::HashSet<u64> = remote.iter().map(|e| e.media.id).collect();
            self.entries(service, kind)?.into_iter().filter(|e| !known.contains(&e.media.id)).collect()
        } else {
            Vec::new()
        };
        let pending: Vec<Pending> = self
            .pending()?
            .into_iter()
            .filter(|p| p.target == service && p.media.service == service && p.media.kind == kind)
            .collect();
        let tx = self.db.transaction()?;
        tx.execute("DELETE FROM entries WHERE service=?1 AND kind=?2", [service.key(), kind.key()])?;
        for e in remote.iter().filter(|e| e.media.service == service && e.media.kind == kind) {
            put_entry(&tx, e)?;
        }
        for e in &keep {
            put_entry(&tx, e)?;
            queue(&tx, service, e.media.media_ref(), &Change::Save(Box::new(e.clone())))?;
        }
        for p in pending {
            match p.change {
                Change::Save(e) => put_entry(&tx, &e)?,
                Change::Delete(m) => delete_entry(&tx, m)?,
            }
        }
        Ok(tx.commit()?)
    }

    // Outbox

    pub fn pending(&self) -> Result<Vec<Pending>> {
        let mut q = self.db.prepare_cached(
            "SELECT revision,target,service,kind,id,json,attempts FROM outbox ORDER BY revision",
        )?;
        let rows = q.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, u32>(6)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (revision, target, service, kind, id, json, attempts) = row?;
            let svc = |s: &str| Service::ALL.into_iter().find(|x| x.key() == s);
            let kind = MediaKind::ALL.into_iter().find(|x| x.key() == kind);
            if let (Some(target), Some(service), Some(kind)) = (svc(&target), svc(&service), kind) {
                out.push(Pending {
                    target,
                    media: MediaRef { service, kind, id: id as u64 },
                    change: serde_json::from_str(&json)?,
                    attempts,
                    revision,
                });
            }
        }
        Ok(out)
    }

    /// Marks an edit as sent. A newer edit to the same title stays queued.
    pub fn sent(&self, revision: i64) -> Result<()> {
        self.db.execute("DELETE FROM outbox WHERE revision=?1", [revision])?;
        Ok(())
    }

    pub fn failed(&self, revision: i64, error: &str) -> Result<()> {
        let error: String = error.chars().take(300).collect();
        self.db.execute(
            "UPDATE outbox SET attempts=attempts+1, error=?2 WHERE revision=?1",
            params![revision, error],
        )?;
        Ok(())
    }

    /// Forgets everything that belongs to an account, on sign-out.
    pub fn clear_account(&mut self, service: Service) -> Result<()> {
        let tx = self.db.transaction()?;
        tx.execute("DELETE FROM outbox WHERE target=?1", [service.key()])?;
        tx.execute("DELETE FROM entries WHERE service=?1", [service.key()])?;
        Ok(tx.commit()?)
    }

    // Cache

    /// A cached value and its age in seconds.
    pub fn cached<T: DeserializeOwned>(&self, key: &str, now: i64) -> Result<Option<(T, i64)>> {
        let row: Option<(String, i64)> = self
            .db
            .query_row("SELECT json,stored_at FROM cache WHERE key=?1", [key], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        Ok(row.and_then(|(j, at)| Some((serde_json::from_str(&j).ok()?, (now - at).max(0)))))
    }

    pub fn put_cache<T: Serialize>(&self, key: &str, value: &T, now: i64) -> Result<()> {
        self.db.execute(
            "INSERT INTO cache(key,json,stored_at) VALUES(?1,?2,?3)
             ON CONFLICT(key) DO UPDATE SET json=?2, stored_at=?3",
            params![key, serde_json::to_string(value)?, now],
        )?;
        Ok(())
    }

    pub fn clear_cache(&self) -> Result<()> {
        self.db.execute("DELETE FROM cache", [])?;
        Ok(())
    }

    pub fn cache_bytes(&self) -> Result<u64> {
        let n: i64 =
            self.db.query_row("SELECT COALESCE(SUM(LENGTH(json)),0) FROM cache", [], |r| r.get(0))?;
        Ok(n.max(0) as u64)
    }

    /// Drops cache and notification rows older than `days`.
    pub fn prune(&self, now: i64, days: i64) -> Result<()> {
        let limit = now - days * 86_400;
        self.db.execute("DELETE FROM cache WHERE stored_at<?1", [limit])?;
        self.db.execute("DELETE FROM notified WHERE at<?1", [limit])?;
        Ok(())
    }

    // Notifications

    pub fn was_notified(&self, key: &str) -> Result<bool> {
        Ok(self.db.query_row("SELECT EXISTS(SELECT 1 FROM notified WHERE key=?1)", [key], |r| r.get(0))?)
    }

    pub fn mark_notified(&self, key: &str, now: i64) -> Result<()> {
        self.db.execute("INSERT OR REPLACE INTO notified(key,at) VALUES(?1,?2)", params![key, now])?;
        Ok(())
    }

    // Backup

    pub fn export(&self, service: Service) -> Result<String> {
        let mut entries = self.entries(service, MediaKind::Anime)?;
        entries.extend(self.entries(service, MediaKind::Manga)?);
        Ok(serde_json::to_string_pretty(&Backup { format: BACKUP_FORMAT, entries })?)
    }

    /// Adds entries from a backup, replacing ones with the same id, and queues
    /// them for `targets`. Returns how many there were.
    pub fn import(&mut self, json: &str, targets: &[Service]) -> Result<usize> {
        let b: Backup = serde_json::from_str(json)?;
        let valid = |e: &ListEntry| e.media.id > 0 && i64::try_from(e.media.id).is_ok();
        if b.format != BACKUP_FORMAT || !b.entries.iter().all(valid) {
            return Err(StoreError::BadBackup);
        }
        let tx = self.db.transaction()?;
        for e in &b.entries {
            let e = crate::progress::sanitize(e);
            put_entry(&tx, &e)?;
            for t in targets {
                queue(&tx, *t, e.media.media_ref(), &Change::Save(Box::new(e.clone())))?;
            }
        }
        tx.commit()?;
        Ok(b.entries.len())
    }
}

fn put_entry(db: &Connection, e: &ListEntry) -> Result<()> {
    let m = e.media.media_ref();
    db.execute(
        "INSERT INTO entries(service,kind,id,json) VALUES(?1,?2,?3,?4)
         ON CONFLICT(service,kind,id) DO UPDATE SET json=?4",
        params![m.service.key(), m.kind.key(), m.id as i64, serde_json::to_string(e)?],
    )?;
    Ok(())
}

fn delete_entry(db: &Connection, m: MediaRef) -> Result<()> {
    db.execute(
        "DELETE FROM entries WHERE service=?1 AND kind=?2 AND id=?3",
        params![m.service.key(), m.kind.key(), m.id as i64],
    )?;
    Ok(())
}

fn queue(db: &Connection, target: Service, m: MediaRef, change: &Change) -> Result<()> {
    // INSERT OR REPLACE assigns a fresh revision, so an in-flight older edit
    // cannot acknowledge this one.
    db.execute(
        "INSERT OR REPLACE INTO outbox(target,service,kind,id,json) VALUES(?1,?2,?3,?4,?5)",
        params![target.key(), m.service.key(), m.kind.key(), m.id as i64, serde_json::to_string(change)?],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ListStatus, Media};

    fn entry(id: u64, progress: u32) -> ListEntry {
        ListEntry { progress, ..ListEntry::new(Media { id, mal_id: Some(id + 100), ..Media::default() }) }
    }

    #[test]
    fn persists_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        Store::open(&path).unwrap().put_entry(&entry(1, 3)).unwrap();
        let s = Store::open(&path).unwrap();
        assert_eq!(s.entry(entry(1, 0).media.media_ref()).unwrap().unwrap().progress, 3);
    }

    #[test]
    fn refuses_newer_schema() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        Store::open(&path).unwrap();
        Connection::open(&path).unwrap().pragma_update(None, "user_version", 99).unwrap();
        assert!(matches!(Store::open(&path), Err(StoreError::TooNew(99))));
    }

    #[test]
    fn refresh_keeps_unsent_edits() {
        let mut s = Store::in_memory().unwrap();
        s.save_and_queue(&entry(1, 5), &[Service::AniList]).unwrap();
        s.put_entry(&entry(3, 1)).unwrap();
        s.replace_library(Service::AniList, MediaKind::Anime, &[entry(1, 4), entry(2, 0)], false).unwrap();
        let mut got: Vec<_> = s
            .entries(Service::AniList, MediaKind::Anime)
            .unwrap()
            .iter()
            .map(|e| (e.media.id, e.progress))
            .collect();
        got.sort_unstable();
        assert_eq!(got, [(1, 5), (2, 0)]);
    }

    #[test]
    fn refresh_does_not_resurrect_unsent_deletes() {
        let mut s = Store::in_memory().unwrap();
        s.delete_and_queue(&entry(1, 0), &[Service::AniList]).unwrap();
        s.replace_library(Service::AniList, MediaKind::Anime, &[entry(1, 0)], false).unwrap();
        assert!(s.entries(Service::AniList, MediaKind::Anime).unwrap().is_empty());
    }

    #[test]
    fn first_sign_in_uploads_only_missing_titles() {
        let mut s = Store::in_memory().unwrap();
        s.put_entry(&entry(1, 9)).unwrap();
        s.put_entry(&entry(2, 5)).unwrap();
        s.replace_library(Service::AniList, MediaKind::Anime, &[entry(1, 3)], true).unwrap();
        assert_eq!(s.entry(entry(1, 0).media.media_ref()).unwrap().unwrap().progress, 3);
        let p = s.pending().unwrap();
        assert_eq!((p.len(), p[0].media.id), (1, 2));
        s.clear_account(Service::AniList).unwrap();
        assert!(s.pending().unwrap().is_empty());
        assert!(s.entries(Service::AniList, MediaKind::Anime).unwrap().is_empty());
    }

    #[test]
    fn newer_edit_survives_ack_of_older_one() {
        let mut s = Store::in_memory().unwrap();
        s.save_and_queue(&entry(1, 1), &[Service::AniList]).unwrap();
        let old = s.pending().unwrap()[0].revision;
        s.save_and_queue(&entry(1, 2), &[Service::AniList]).unwrap();
        s.sent(old).unwrap();
        let p = s.pending().unwrap();
        assert_eq!(p.len(), 1);
        assert!(matches!(&p[0].change, Change::Save(e) if e.progress == 2));
    }

    #[test]
    fn queues_per_service_with_translated_delete_ids() {
        let mut s = Store::in_memory().unwrap();
        s.delete_and_queue(&entry(1, 0), &Service::ALL).unwrap();
        let p = s.pending().unwrap();
        assert_eq!(p.len(), 2);
        assert!(
            p.iter()
                .any(|p| matches!(p.change, Change::Delete(m) if m.service == Service::Mal && m.id == 101))
        );
    }

    #[test]
    fn damaged_settings_fall_back_to_defaults() {
        let s = Store::in_memory().unwrap();
        s.db.execute("INSERT INTO settings VALUES(1,'{oops')", []).unwrap();
        assert_eq!(s.settings().unwrap(), Settings::default());
    }

    #[test]
    fn cache_reports_age_and_ignores_corruption() {
        let s = Store::in_memory().unwrap();
        s.put_cache("k", &vec![1, 2], 100).unwrap();
        assert_eq!(s.cached::<Vec<i32>>("k", 160).unwrap(), Some((vec![1, 2], 60)));
        assert_eq!(s.cached::<String>("k", 160).unwrap(), None);
        s.prune(100 + 10 * 86_400, 5).unwrap();
        assert_eq!(s.cached::<Vec<i32>>("k", 160).unwrap(), None);
    }

    #[test]
    fn backup_round_trip_and_validation() {
        let a = Store::in_memory().unwrap();
        let mut e = entry(7, 2);
        e.status = ListStatus::Paused;
        e.notes = "it's \"quoted\"; DROP TABLE entries;--".into();
        a.put_entry(&e).unwrap();
        let json = a.export(Service::AniList).unwrap();
        let mut b = Store::in_memory().unwrap();
        assert_eq!(b.import(&json, &[Service::Mal]).unwrap(), 1);
        assert_eq!(b.entry(e.media.media_ref()).unwrap(), Some(e));
        assert_eq!(b.pending().unwrap().len(), 1);
        assert!(b.import("{}", &[]).is_err());
        assert!(matches!(b.import(r#"{"format":9,"entries":[]}"#, &[]), Err(StoreError::BadBackup)));
        assert!(matches!(
            b.import(r#"{"format":1,"entries":[{"media":{"id":18446744073709551615}}]}"#, &[]),
            Err(StoreError::BadBackup)
        ));
    }

    #[test]
    fn notifications_fire_once() {
        let s = Store::in_memory().unwrap();
        assert!(!s.was_notified("x").unwrap());
        s.mark_notified("x", 1).unwrap();
        assert!(s.was_notified("x").unwrap());
    }
}
