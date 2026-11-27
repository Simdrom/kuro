//! Requests from the interface, applied after drawing.

use super::*;

impl App {
    pub(super) fn apply_acts(&mut self, ctx: &egui::Context) {
        while !self.acts.is_empty() {
            for a in std::mem::take(&mut self.acts) {
                self.apply(a);
            }
            ctx.request_repaint();
        }
    }

    pub(super) fn apply(&mut self, a: Act) {
        match a {
            Act::Open(m) => {
                let r = self.home_ref(&m);
                // A friend's title from the other service is looked up on this one.
                if r.service != self.s.primary && self.bg.online() {
                    return self.apply(Act::OpenRef(r));
                }
                // Show what is known while the rest loads.
                let d = self.details.entry(r).or_default();
                if d.data.is_none() {
                    let media = Media { service: r.service, id: r.id, ..(*m).clone() };
                    d.data = Some(Rc::new(MediaDetail { media, ..MediaDetail::default() }));
                    d.loading = true;
                    self.bg.detail(r, false);
                }
                self.open(r);
            }
            Act::OpenRef(r) => {
                if r.service == self.s.primary || !self.bg.online() {
                    self.ensure_detail(r);
                    self.open(r);
                } else {
                    self.say(t("Opening…"));
                    self.bg.resolve(r, self.s.primary);
                }
            }
            Act::Push(r) => {
                if self.routes.last() != Some(&r) {
                    self.routes.push(r);
                }
            }
            Act::Back => {
                self.routes.pop();
            }
            Act::Go(tab) => {
                self.cache_bytes = None;
                if self.tab != tab {
                    self.routes.clear();
                }
                self.tab = tab;
                if tab == Tab::Search {
                    self.search.focus = true;
                }
            }
            Act::Gesture(g, m) => self.gesture(g, m),
            Act::Step(m, n) => {
                let e = self.working_entry(&m);
                let e = if n > 0 && e.status == ListStatus::Completed && !e.repeating { return } else { e };
                let next = progress::step(&e, n, today());
                self.change(&next);
            }
            Act::StepVolumes(m, n) => {
                let next = progress::step_volumes(&self.working_entry(&m), n, today());
                self.change(&next);
            }
            Act::SetStatus(m, s) => {
                let next = progress::set_status(&self.working_entry(&m), s, today());
                self.change(&next);
            }
            Act::Repeat(m) => {
                let next = progress::start_repeat(&self.working_entry(&m));
                self.change(&next);
            }
            Act::Save(e) => self.change(&e),
            Act::Edit(m) => {
                let is_new = self.entry(&m).is_none();
                let entry = self.working_entry(&m);
                let text = |d: Option<kuro_core::FuzzyDate>| d.map(|d| d.iso()).unwrap_or_default();
                self.editor = Some(Editor {
                    start: text(entry.start),
                    finish: text(entry.finish),
                    entry,
                    is_new,
                    new_list: String::new(),
                });
            }
            Act::AskDelete(m) => {
                let r = self.home_ref(&m);
                if !self.s.confirm_delete {
                    self.delete(r);
                    return;
                }
                let name = m.display_title(self.s.title_language).to_owned();
                self.confirm = Some(Confirm {
                    title: t("Remove from your list?").into(),
                    body: tf("Your progress, score and notes for “{}” will be deleted.", &[&name]),
                    action: t("Remove").into(),
                    danger: true,
                    act: Some(Act::Delete(r)),
                });
            }
            Act::Delete(r) => self.delete(r),
            Act::EditLists(op) => self.edit_lists(op),
            Act::AskDeleteList(name) => {
                self.confirm = Some(Confirm {
                    title: tf("Delete the list “{}”?", &[&name]),
                    body: t("Its titles stay in your list; they just leave this one.").into(),
                    action: t("Delete").into(),
                    danger: true,
                    act: Some(Act::EditLists(ListOp::Delete(name))),
                });
            }
            Act::ToggleList(m, name) => {
                let mut e = self.working_entry(&m);
                let low = name.to_lowercase();
                let next: Vec<String> = if e.lists().iter().any(|x| x.to_lowercase() == low) {
                    e.lists().iter().filter(|x| x.to_lowercase() != low).cloned().collect()
                } else {
                    e.lists().iter().cloned().chain([name]).collect()
                };
                e.lists = Some(kuro_core::clean_lists(next));
                self.change(&e);
            }
            Act::Undo => self.undo(),
            Act::Url(u) => self.sys.open_url(&u),
            Act::Copy(text) => {
                self.sys.copy(&text);
                self.clipboard_seen = text;
                self.say(t("Link copied"));
            }
            Act::Binge(r) => self.binge = r,
            Act::ToggleMute(r) => {
                let muted = &mut self.s.notifications.muted;
                match muted.iter().position(|m| *m == r) {
                    Some(i) => drop(muted.remove(i)),
                    None => muted.push(r),
                }
            }
            Act::Login(s) => {
                self.login = Some(s);
                self.bg.login(s);
            }
            Act::CancelLogin => {
                self.login = None;
                self.pending_login = None;
                self.bg.cancel_login();
            }
            Act::AcceptLogin => self.accept_login(),
            Act::AskSignOut(s) => {
                let mut body = tf(
                    "The {} list will be removed from this computer. Your online list is kept.",
                    &[&s.name()],
                );
                let waiting =
                    self.bg.store().pending().map_or(0, |p| p.iter().filter(|p| p.target == s).count());
                if waiting > 0 {
                    body.push_str("\n\n");
                    body.push_str(&tf("{} unsent changes will be lost.", &[&waiting]));
                }
                self.confirm = Some(Confirm {
                    title: t("Sign out?").into(),
                    body,
                    action: t("Sign out").into(),
                    danger: true,
                    act: Some(Act::SignOut(s)),
                });
            }
            Act::SignOut(s) => {
                if let Err(e) = self.bg.api.sign_out(s) {
                    self.say_error(format::error(&e));
                    return;
                }
                self.profiles.remove(&s);
                self.sync_error = None;
                if let Err(e) = self.bg.store().clear_account(s) {
                    tracing::error!("sign-out cleanup failed: {e}");
                }
                self.reload();
            }
            Act::Refresh => self.refresh(),
            Act::Retry(r) => {
                let d = self.details.entry(r).or_default();
                d.error = None;
                d.loading = true;
                d.themes = None;
                d.themes_asked = false;
                self.bg.detail(r, true);
            }
            Act::Random(k) => {
                self.search.random_busy = true;
                self.bg.random(k, self.s.show_adult);
            }
            Act::ShowStatus(k, s) => {
                self.apply(Act::Go(if k == MediaKind::Anime { Tab::Anime } else { Tab::Manga }));
                self.lists[slot(k)].filter = Filter { status: Some(s), ..Filter::default() };
            }
            Act::Import(json) => {
                let targets = self.targets();
                let r = self.bg.store().import(&json, &targets);
                match r {
                    Ok(n) => {
                        self.reload();
                        self.bg.sync();
                        self.say(tf("{} titles imported", &[&n]));
                    }
                    Err(e) => self.say_error(tf("This backup could not be read: {}", &[&e])),
                }
            }
            Act::KiokuConnect(p) => {
                self.kioku.connecting = true;
                self.kioku.error = None;
                self.bg.kioku_connect(p);
            }
            Act::KiokuAskDisconnect(p) => {
                self.confirm = Some(Confirm {
                    title: tf("kioku.disconnect.title", &[&p.name()]),
                    body: tf("kioku.disconnect.body", &[&p.name()]),
                    action: t("kioku.disconnect").into(),
                    danger: true,
                    act: Some(Act::KiokuDisconnect(p)),
                });
            }
            Act::KiokuDisconnect(p) => {
                use kuro_api::kioku::HistorySource;
                let r = match p {
                    Platform::Crunchyroll => self.bg.api.crunchyroll.disconnect(),
                    _ => Ok(()),
                };
                match r {
                    Ok(()) => {
                        self.kioku.connected = Some(false);
                        self.kioku.review = None;
                    }
                    Err(e) => self.say_error(format::error(&e)),
                }
            }
            Act::KiokuSync(p) => {
                self.kioku.syncing = true;
                self.kioku.progress = None;
                self.kioku.started = Some(self.time);
                self.kioku.took = None;
                self.kioku.error = None;
                self.kioku.review = None;
                self.bg.kioku_sync(p, self.s.primary);
            }
            Act::KiokuAskApply => {
                let n = self.kioku.review.as_ref().map_or(0, KiokuReview::selected);
                self.confirm = Some(Confirm {
                    title: tf("kioku.apply.title", &[&n]),
                    body: t("kioku.apply.body").into(),
                    action: t("kioku.apply.action").into(),
                    danger: false,
                    act: Some(Act::KiokuApply),
                });
            }
            Act::KiokuApply => self.kioku_apply(),
            Act::KiokuImport(p) => {
                if !self.bg.online() {
                    return;
                }
                self.kioku.syncing = true;
                self.kioku.progress = None;
                self.kioku.started = Some(self.time);
                self.kioku.took = None;
                self.kioku.error = None;
                self.kioku.review = None;
                self.bg.kioku_import(p, self.s.primary);
            }
            Act::KiokuImportFound => {
                let Some(f) = self.kioku.found.take() else { return };
                let _ = self.bg.store().mark_notified(&f.key, now());
                if !self.bg.online() {
                    return;
                }
                self.kioku.syncing = true;
                self.kioku.progress = None;
                self.kioku.started = Some(self.time);
                self.kioku.took = None;
                self.kioku.error = None;
                self.kioku.review = None;
                self.bg.kioku_import_file(f.platform, self.s.primary, f.path);
            }
            Act::KiokuForgetFound => {
                if let Some(f) = self.kioku.found.take() {
                    let _ = self.bg.store().mark_notified(&f.key, now());
                }
            }
            Act::KiokuDismissAll => {
                if let Some(r) = self.kioku.review.as_mut() {
                    for g in std::mem::take(&mut r.unmatched) {
                        r.dismissed.extend(g.items.into_iter().map(|i| i.external_id));
                    }
                }
            }
            Act::KiokuPick(from) => {
                let Some(r) = self.kioku.review.as_ref() else { return };
                let group = match from {
                    PickFrom::Unmatched(i) => r.unmatched.get(i).cloned(),
                    PickFrom::Change(i) => r.changes.get(i).map(|c| r.group_of(&c.items)),
                };
                if let Some(group) = group.filter(|g| !g.items.is_empty()) {
                    let text = group.query().to_owned();
                    self.kioku.picker =
                        Some(KiokuPicker { group, text, sent: String::new(), busy: false, results: None });
                    self.act(Act::KiokuLookup);
                }
            }
            Act::KiokuLookup => {
                let Some(pk) = self.kioku.picker.as_mut() else { return };
                let text = pk.text.trim().to_owned();
                if text.is_empty() || !self.bg.online() {
                    return;
                }
                pk.busy = true;
                pk.sent = text.clone();
                self.bg.kioku_search(self.s.primary, text);
            }
            Act::KiokuChoose(m) => self.kioku_choose(*m),
            Act::AddFriend => {
                let Some(f) = Friend::parse(&self.friend_text, self.friend_service) else {
                    return self.say_error(t("Type a user name or paste a link to a profile."));
                };
                if self.s.friends.iter().any(|x| x.same(&f)) {
                    return self.say(tf("{} is already in your friends", &[&f.name]));
                }
                if !self.add_friend(f.clone()) {
                    return self.say_error(tf("You can add up to {} friends.", &[&MAX_FRIENDS]));
                }
                self.friend_text.clear();
                self.ensure_friend(&f);
            }
            Act::RemoveFriend(f) => {
                self.s.friends.retain(|x| !x.same(&f));
                self.friends.remove(&f);
                self.routes.retain(|r| !matches!(r, Route::Friend(x) if x.same(&f)));
                self.say(tf("{} removed from your friends", &[&f.name]));
            }
            Act::ImportFollowing => {
                self.following_busy = true;
                self.bg.following();
            }
            Act::RefreshFriends(one) => {
                for f in one.map_or_else(|| self.s.friends.clone(), |f| vec![f]) {
                    let v = self.friends.entry(f.clone()).or_default();
                    v.loading = true;
                    v.error = None;
                    self.bg.friend(f, true);
                }
            }
            Act::KiokuDismiss(i) => {
                if let Some(r) = self.kioku.review.as_mut()
                    && i < r.unmatched.len()
                {
                    let g = r.unmatched.remove(i);
                    r.dismissed.extend(g.items.into_iter().map(|i| i.external_id));
                }
            }
        }
    }

    fn gesture(&mut self, g: GestureAction, m: Rc<Media>) {
        let next = match g {
            GestureAction::None => return,
            GestureAction::Increment => Act::Step(m, 1),
            GestureAction::Decrement => Act::Step(m, -1),
            GestureAction::IncrementVolume => Act::StepVolumes(m, 1),
            GestureAction::Edit => Act::Edit(m),
            GestureAction::Complete => Act::SetStatus(m, ListStatus::Completed),
            GestureAction::Drop => Act::SetStatus(m, ListStatus::Dropped),
            GestureAction::Pause => Act::SetStatus(m, ListStatus::Paused),
            GestureAction::OpenWeb => match m.web_url(m.service) {
                Some(u) => Act::Url(u),
                None => return,
            },
            GestureAction::CopyLink => match m.web_url(m.service) {
                Some(u) => Act::Copy(u),
                None => return,
            },
            GestureAction::Binge => Act::Binge(Some(self.home_ref(&m))),
        };
        self.act(next);
    }

    pub(super) fn accept_login(&mut self) {
        self.login = None;
        let Some((s, profile, tokens)) = self.pending_login.take() else { return };
        if let Err(e) = self.bg.api.accept_login(s, &profile, &tokens) {
            self.say_error(format::error(&e));
            return;
        }
        self.say(tf("Signed in as {}", &[&profile.name]));
        self.profiles.insert(s, profile);
        self.refreshing = 2;
        self.bg.refresh_library(s, true);
    }
}
