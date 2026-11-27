//! Kioku: applying a review, and the checks it makes on its own.

use super::*;

impl App {
    /// Remembers the title picked for a series and reviews that series again with it.
    pub(super) fn kioku_choose(&mut self, m: Media) {
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
    pub(super) fn kioku_apply(&mut self) {
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

    /// Reads the Crunchyroll history once a day or week, without the user.
    /// Nothing is written: the review waits for them, as after a manual sync.
    pub(super) fn kioku_check(&mut self) {
        use kuro_api::kioku::HistorySource;
        let Some(period) = self.s.kioku_check.period(today()) else { return };
        let key = format!("kioku:crunchyroll:{period}");
        if self.kioku.checked == key || self.kioku.syncing || self.kioku.review.is_some() || !self.bg.online()
        {
            return;
        }
        self.kioku.checked = key.clone();
        if self.bg.store().was_notified(&key).unwrap_or(true) || !self.bg.api.crunchyroll.connected() {
            return;
        }
        let _ = self.bg.store().mark_notified(&key, now());
        self.apply(Act::KiokuSync(Platform::Crunchyroll));
        self.kioku.background = true;
    }

    /// Looks in Downloads for streaming history files about once an hour.
    pub(super) fn scan_downloads(&mut self) {
        if !self.s.watch_downloads || !self.bg.online() || self.time < self.downloads_at {
            return;
        }
        self.downloads_at = self.time + 3600.0;
        if let Some(dir) =
            directories::UserDirs::new().and_then(|d| d.download_dir().map(|p| p.to_path_buf()))
        {
            self.bg.scan_downloads(dir, now() - 14 * 86_400);
        }
    }

    /// Says what an automatic Kioku check found.
    pub(super) fn kioku_checked(&mut self, result: &Result<KiokuReview, ApiError>) {
        match result {
            Ok(r) if !r.changes.is_empty() => {
                let mut n = Notice::new("Kioku", tf("kioku.auto.ready", &[&r.changes.len()]));
                n.link = Some("kuro://tab/kioku".into());
                self.sys.notify(n);
            }
            Ok(_) => {}
            Err(e) => tracing::warn!("kioku: automatic check failed: {e}"),
        }
    }
}
