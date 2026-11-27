//! Reading the list from the account and sending changes to it.

use super::*;

impl Backend {
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

#[cfg(test)]
mod tests {
    use super::*;
    use kuro_api::{ClientIds, MemorySecrets};

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
