//! Work done on a timer: notifications, reminders, backups and refreshes.

use super::*;

impl App {
    pub(super) fn tick(&mut self, ctx: &egui::Context) {
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
        self.kioku_check();
        self.scan_downloads();
        self.write_calendar();
        self.sync_google();
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

    /// Looks at the backup folder about once an hour.
    fn auto_backup(&mut self) {
        let b = &self.s.backup;
        if b.every == Every::Off || b.folder.is_empty() || self.time < self.backup_at {
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
}
