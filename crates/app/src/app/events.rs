//! Events from the background.

use super::*;

impl App {
    pub(super) fn poll(&mut self, ctx: &egui::Context) {
        while let Some(e) = self.bg.next_event() {
            self.handle(ctx, e);
        }
    }

    pub fn handle(&mut self, ctx: &egui::Context, e: Event) {
        match e {
            Event::Library { service, result, .. } => {
                self.refreshing = self.refreshing.saturating_sub(1);
                match result {
                    Ok(()) => {
                        if service == self.s.primary {
                            self.library_at = now();
                            self.reload();
                        }
                        self.airing_at = now();
                        self.bg.sync();
                    }
                    Err(e) => self.say_error(format::error(&e)),
                }
            }
            Event::Season { query, continuing, page, result } => {
                if query == self.season_query() && continuing == self.season.continuing {
                    self.season.feed.apply(page, result);
                }
            }
            Event::Ranking { query, page, result } => {
                if query == self.ranking_query() {
                    self.ranking.feed.apply(page, result);
                }
            }
            Event::Search { kind, text, page, result } => {
                if kind == self.search.kind && text == self.search.sent {
                    self.search.feed.apply(page, result);
                }
            }
            Event::Genres(r) => {
                if let Ok(g) = r {
                    self.ranking.genres = g;
                }
            }
            Event::Detail { media, result, fresh } => {
                let d = self.details.entry(media).or_default();
                d.loading = !fresh;
                match result {
                    Ok(x) => {
                        d.error = None;
                        if media.service == Service::Mal && !x.themes.is_empty() {
                            d.themes = Some(Ok(Rc::new(x.themes.clone())));
                            d.themes_asked = true;
                        }
                        d.data = Some(Rc::new(x));
                    }
                    Err(e) => d.error = Some(e),
                }
            }
            Event::Themes { media, result } => {
                self.details.entry(media).or_default().themes = Some(result.map(Rc::new));
            }
            Event::Track { theme, result } => {
                self.tracks.insert((theme.title, theme.artist), result.ok().flatten());
            }
            Event::Person { id, result } => {
                self.people.insert(id, result.map_or_else(Load::Failed, |p| Load::Ready(Rc::new(p))));
            }
            Event::Overview { media, result } => {
                self.overviews.insert(media, result.map_or_else(Load::Failed, |p| Load::Ready(Rc::new(p))));
            }
            Event::Friend { friend, result, fresh } => {
                let v = self.friends.entry(friend).or_default();
                v.loading = !fresh;
                match result {
                    Ok(p) => {
                        v.error = None;
                        v.data = Some(Rc::new(p));
                    }
                    Err(e) => v.error = Some(e),
                }
            }
            Event::Following(r) => {
                self.following_busy = false;
                match r {
                    Ok(names) => {
                        let added = names
                            .into_iter()
                            .filter(|n| valid_name(n))
                            .filter(|n| {
                                self.add_friend(Friend { service: Service::AniList, name: n.clone() })
                            })
                            .count();
                        if added == 0 {
                            self.say(t("Everyone you follow is already here"));
                        } else {
                            self.say(tf("{} friends added", &[&added]));
                        }
                    }
                    Err(e) => self.say_error(format::error(&e)),
                }
            }
            Event::Random(r) => {
                self.search.random_busy = false;
                match r {
                    Ok(m) => self.act(Act::Open(Rc::new(m))),
                    Err(e) => self.say_error(format::error(&e)),
                }
            }
            Event::Resolved { from, result } => {
                let r = match result {
                    Ok(r) => r,
                    // Unknown to the primary service: show it from where it came.
                    Err(ApiError::NotFound | ApiError::NotConfigured) => from,
                    Err(e) => return self.say_error(format::error(&e)),
                };
                self.ensure_detail(r);
                self.open(r);
            }
            Event::Airing(r) => {
                if let Ok(list) = r {
                    self.update_airing(list);
                }
            }
            Event::LoginUrl { service, result } => match result {
                Ok(u) if self.login == Some(service) => self.sys.open_url(&u),
                Ok(_) => {}
                Err(e) => {
                    self.login = None;
                    self.say_error(format::error(&e));
                }
            },
            Event::Login { service, result } => match result {
                Ok((profile, tokens, verified)) if self.login == Some(service) => {
                    let name = profile.name.clone();
                    self.pending_login = Some((service, profile, tokens));
                    self.show_window(ctx);
                    if verified {
                        self.accept_login();
                    } else {
                        self.confirm = Some(Confirm {
                            title: tf("Sign in as {}?", &[&name]),
                            body: t("Continue only if this is your account.").into(),
                            action: t("Sign in").into(),
                            danger: false,
                            act: Some(Act::AcceptLogin),
                        });
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    self.login = None;
                    self.say_error(format::error(&e));
                }
            },
            Event::Profile { service, result } => match result {
                Ok(p) => drop(self.profiles.insert(service, p)),
                Err(ApiError::Unauthorized) => {
                    self.sync_error = Some((service, ApiError::Unauthorized));
                }
                Err(e) => tracing::warn!("profile: {e}"),
            },
            Event::Sync { left, error, .. } => {
                self.unsynced = left;
                if let Some((s, e)) = &error
                    && !e.is_transient()
                {
                    let text = format!("{}: {}", s.name(), format::error(e));
                    self.say_error(text);
                }
                self.sync_error = error;
            }
            Event::Image { url, image } => self.tex.insert(ctx, url, image),
            Event::ImageCacheSize(n) => self.image_bytes = Some(n),
            Event::ImportText(r) => match r {
                Ok(json) => {
                    self.confirm = Some(Confirm {
                        title: t("Import this backup?").into(),
                        body: t("Titles in the backup replace the ones in your list.").into(),
                        action: t("Import").into(),
                        danger: false,
                        act: Some(Act::Import(json)),
                    });
                }
                Err(e) => self.say_error(tf("This backup could not be read: {}", &[&e])),
            },
            Event::Exported(r) => match r {
                Ok(()) => self.say(t("Backup saved")),
                Err(e) => self.say_error(tf("Something went wrong: {}", &[&e])),
            },
            Event::BackupFolder(folder) => {
                self.s.backup.folder = folder;
                if self.s.backup.every == Every::Off {
                    self.s.backup.every = Every::Daily;
                }
            }
            Event::AutoBackup(r) => match r {
                Ok(latest) => {
                    self.last_backup = latest;
                    self.backup_error = None;
                }
                Err(e) => {
                    // Said once; the page keeps showing it.
                    if self.backup_error.as_ref() != Some(&e) {
                        self.say_error(tf("The automatic backup failed: {}", &[&e]));
                    }
                    self.backup_error = Some(e);
                }
            },
            Event::CalendarFile(path) => self.s.calendar_file = path,
            Event::CalendarWritten(r) => {
                if let Err(e) = &r
                    && self.calendar_error.as_ref() != Some(e)
                {
                    self.say_error(tf("The calendar could not be saved: {}", &[e]));
                    // Tried again with the next change.
                    self.calendar_text.clear();
                }
                self.calendar_error = r.err();
            }
            Event::GoogleUrl(r) => match r {
                Ok(u) if self.google_connecting => self.sys.open_url(&u),
                Ok(_) => {}
                Err(e) => {
                    self.google_connecting = false;
                    self.say_error(format::error(&e));
                }
            },
            Event::GoogleConnected(r) => {
                self.google_connecting = false;
                self.show_window(ctx);
                match r {
                    Ok(email) => {
                        let email = if email.is_empty() { "Google".to_owned() } else { email };
                        if !self.s.google_account.is_empty() && self.s.google_account != email {
                            // Reconnected with another account: the calendars were the old one's.
                            self.s.google_calendar.clear();
                            self.s.google_calendars.clear();
                        }
                        self.s.google_account = email;
                        self.google_error = None;
                        self.google_expired = false;
                        // Sent again at once, with the new session.
                        self.google_sent.clear();
                    }
                    Err(e) => self.say_error(google_error(&e)),
                }
            }
            Event::GoogleSynced { asked, name, result } => {
                self.google_busy = false;
                match result {
                    Ok(id) if asked == self.s.google_calendar => {
                        let known =
                            self.s.google_calendars.iter_mut().find(|c| !asked.is_empty() && c.id == asked);
                        match known {
                            // Made again after it was deleted in Google Calendar.
                            Some(c) => c.id.clone_from(&id),
                            None => self.s.google_calendars.push(GoogleCalendar { id: id.clone(), name }),
                        }
                        self.s.google_calendar = id;
                        self.google_error = None;
                    }
                    // Another calendar was chosen meanwhile: this one is left behind.
                    Ok(id) => self.bg.google_clear(id),
                    Err(e) => {
                        self.google_expired = matches!(e, ApiError::Unauthorized);
                        let text = google_error(&e);
                        // Said once; the page keeps showing it.
                        if self.google_error.as_ref() != Some(&text) {
                            self.say_error(tf("Google Calendar could not be updated: {}", &[&text]));
                        }
                        self.google_error = Some(text);
                        // Tried again with the next change, or within the hour.
                        self.google_sent.clear();
                        self.google_at = now() - GOOGLE_EVERY + 3600;
                    }
                }
            }
            Event::GoogleCreated { name, result } => {
                self.google_creating = false;
                match result {
                    Ok(id) => {
                        self.google_new = None;
                        self.s.google_calendars.push(GoogleCalendar { id: id.clone(), name });
                        // Moving the episodes there happens with the settings change.
                        self.s.google_calendar = id;
                    }
                    Err(e) => {
                        self.google_expired = matches!(e, ApiError::Unauthorized);
                        self.say_error(tf("Google Calendar could not be updated: {}", &[&google_error(&e)]))
                    }
                }
            }
            Event::KiokuConnected { platform, result } => {
                self.kioku.connecting = false;
                self.show_window(ctx);
                match result {
                    Ok(()) => {
                        self.kioku.connected = Some(true);
                        self.say(tf("kioku.connected.toast", &[&platform.name()]));
                    }
                    Err(e) => self.kioku.error = Some(e),
                }
            }
            Event::KiokuCancelled => {
                self.kioku.syncing = false;
                self.kioku.progress = None;
                self.kioku.started = None;
            }
            Event::KiokuFound { text, result } => {
                if let Some(pk) = self.kioku.picker.as_mut()
                    && pk.sent == text
                {
                    pk.busy = false;
                    pk.results = Some(result);
                }
            }
            Event::KiokuProgress(step) => {
                if self.kioku.syncing {
                    self.kioku.progress = Some(step);
                }
            }
            Event::KiokuReview(result)
                if self.kioku.background && result.as_ref().is_ok_and(|r| r.changes.is_empty()) =>
            {
                // Nothing new: no review is left waiting.
                self.kioku.background = false;
                self.kioku.syncing = false;
                self.kioku.progress = None;
                self.kioku.started = None;
            }
            Event::KiokuReview(result) => {
                let background = std::mem::take(&mut self.kioku.background);
                if background {
                    self.kioku_checked(&result);
                }
                self.kioku.syncing = false;
                self.kioku.progress = None;
                self.kioku.took = self.kioku.started.take().map(|s| (ctx.input(|i| i.time) - s).max(0.0));
                match result {
                    Ok(r) => self.kioku.review = Some(r),
                    Err(e) => {
                        if e == ApiError::Unauthorized {
                            self.kioku.connected = None;
                        }
                        self.kioku.error = Some(e);
                    }
                }
            }
            Event::KiokuFileFound { platform, path, modified } => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let key = format!("kioku-file:{}:{name}:{modified}", platform.key());
                let store = self.bg.store();
                let (handled, told) = (store.was_notified(&key).unwrap_or(true), format!("{key}:told"));
                let new = !handled && !store.was_notified(&told).unwrap_or(true);
                if new {
                    let _ = store.mark_notified(&told, now());
                }
                drop(store);
                if handled || self.kioku.found.as_ref().is_some_and(|f| f.key == key) {
                    return;
                }
                if new {
                    let mut n = Notice::new("Kioku", tf("kioku.found", &[&platform.name(), &name]));
                    n.link = Some("kuro://tab/kioku".into());
                    self.sys.notify(n);
                }
                self.kioku.found = Some(FoundFile { platform, path, key });
            }
            Event::OpenLink(l) => {
                self.open_link(&l);
            }
            Event::MarkEpisode(r, ep) => self.mark_episode(r, ep),
            Event::ShowWindow => self.show_window(ctx),
            Event::Quit => {
                self.really_quit = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn update_airing(&mut self, list: Vec<Media>) {
        let mut changed = false;
        for m in list {
            let Some(e) = self.entry_at(m.media_ref()) else { continue };
            let mut e = (*e).clone();
            let old = e.media.clone();
            e.media.next_airing = m.next_airing;
            e.media.release = m.release;
            e.media.episodes = m.episodes.or(e.media.episodes);
            e.media.start = m.start.or(e.media.start);
            e.media.end = m.end.or(e.media.end);
            if e.media != old {
                // Not a user edit: nothing to send.
                if let Err(err) = self.bg.store().put_entry(&e) {
                    tracing::error!("airing update failed: {err}");
                }
                changed = true;
            }
        }
        if changed {
            self.reload();
        }
    }

    pub fn show_window(&mut self, ctx: &egui::Context) {
        self.hidden = false;
        self.start_hidden = 0;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }
}
