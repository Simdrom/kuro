//! Keys, the clipboard, settings changes and the window itself.

use super::*;

impl App {
    pub(super) fn check_clipboard(&mut self, ctx: &egui::Context) {
        let focused = ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::WindowFocused(true))));
        if !focused || !self.s.watch_clipboard {
            return;
        }
        if let Some(text) = self.sys.clipboard()
            && text != self.clipboard_seen
        {
            if matches!(links::parse(&text), Some(Link::Media(_))) {
                self.open_link(&text);
            }
            self.clipboard_seen = text;
        }
    }

    pub(super) fn shortcuts(&mut self, ctx: &egui::Context) {
        let hit = |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_key(m, k));
        if hit(Modifiers::COMMAND, Key::K) {
            self.command = match self.command {
                Some(_) => None,
                None => Some(String::new()),
            };
        }
        if self.editor.is_some() || self.confirm.is_some() || self.command.is_some() {
            return;
        }
        let tabs = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8];
        for (k, tab) in tabs.into_iter().zip(Tab::ALL) {
            if hit(Modifiers::COMMAND, k) {
                self.act(Act::Go(tab));
            }
        }
        if hit(Modifiers::COMMAND, Key::F) {
            self.act(Act::Go(Tab::Search));
        }
        if hit(Modifiers::COMMAND, Key::Comma) {
            self.act(Act::Go(Tab::Settings));
        }
        if hit(Modifiers::COMMAND, Key::R) || hit(Modifiers::NONE, Key::F5) {
            self.act(Act::Refresh);
        }
        if hit(Modifiers::ALT, Key::ArrowLeft) {
            self.act(Act::Back);
        }
        let back = ctx.input(|i| i.pointer.button_pressed(egui::PointerButton::Extra1));
        let typing = ctx.egui_wants_keyboard_input();
        if back || (!typing && !egui::Popup::is_any_open(ctx) && hit(Modifiers::NONE, Key::Escape)) {
            self.act(Act::Back);
        }
        // A menu's own keys act on its title, not on the one being shown.
        if typing || egui::Popup::is_any_open(ctx) {
            return;
        }
        // Acts on the title being shown.
        if let Some(Route::Media(r)) = self.routes.last().cloned()
            && let Some(m) =
                self.details.get(&r).and_then(|d| d.data.as_ref()).map(|d| Rc::new(d.media.clone()))
        {
            if hit(Modifiers::NONE, Key::Plus) || hit(Modifiers::NONE, Key::Equals) {
                self.act(Act::Step(m.clone(), 1));
            }
            if hit(Modifiers::NONE, Key::Minus) {
                self.act(Act::Step(m.clone(), -1));
            }
            if hit(Modifiers::NONE, Key::E) {
                self.act(Act::Edit(m));
            }
        }
    }

    /// Persists and applies changed settings.
    pub(super) fn settle(&mut self, ctx: &egui::Context) {
        if self.s != self.saved {
            let old = std::mem::replace(&mut self.saved, self.s.clone());
            if let Err(e) = self.bg.store().save_settings(&self.s) {
                tracing::error!("settings could not be saved: {e}");
            }
            if old.language != self.s.language {
                i18n::set(&self.s.language);
            }
            if old.primary != self.s.primary {
                self.routes.retain(|r| matches!(r, Route::Settings(_)));
                self.reset_feeds();
                self.ranking.ranking = Ranking::All;
                self.reload();
                self.refresh();
            }
            if old.show_adult != self.s.show_adult {
                self.reset_feeds();
            }
            if old.mal_client_id != self.s.mal_client_id || old.anilist_client_id != self.s.anilist_client_id
            {
                self.bg.api.set_ids(kuro_api::ClientIds::with_defaults(
                    &self.s.anilist_client_id,
                    &self.s.mal_client_id,
                ));
            }
            if old.title_language != self.s.title_language {
                self.google_sent.clear();
            }
            if old.google_calendar != self.s.google_calendar && !self.s.google_calendar.is_empty() {
                // Chosen by the user, rather than made again after a deletion.
                if old.google_calendars.iter().any(|c| c.id == old.google_calendar)
                    && self.s.google_calendars.iter().any(|c| c.id == old.google_calendar)
                {
                    self.bg.google_clear(old.google_calendar.clone());
                }
                (self.google_sent, self.next_tick) = (Vec::new(), 0.0);
            }
            if old.calendar_file != self.s.calendar_file || old.title_language != self.s.title_language {
                self.calendar_text.clear();
                self.calendar_error = None;
                self.next_tick = 0.0;
            }
            if old.check_updates != self.s.check_updates || old.skipped_version != self.s.skipped_version {
                // Asked again at once; skipping hides the version.
                (self.update_at, self.next_tick) = (0.0, 0.0);
                if !self.s.check_updates
                    || self.update.as_ref().is_some_and(|u| u.version == self.s.skipped_version)
                {
                    self.update = None;
                }
            }
            if old.backup != self.s.backup {
                // Look at the new folder now rather than within the hour.
                (self.backup_at, self.next_tick) = (0.0, 0.0);
                (self.last_backup, self.backup_error) = (None, None);
            }
            if old.autostart != self.s.autostart
                && let Err(e) = self.sys.set_autostart(self.s.autostart)
            {
                self.say_error(tf("Something went wrong: {}", &[&e]));
            }
            if old.handle_links != self.s.handle_links
                && let Err(e) = self.sys.set_link_handler(self.s.handle_links)
            {
                self.say_error(tf("Something went wrong: {}", &[&e]));
            }
        }
        let dark = ctx.input(|i| i.raw.system_theme) != Some(egui::Theme::Light);
        self.p = Palette::new(self.s.theme, dark, self.s.tint);
        let look = (self.p, self.s.animations, self.s.zoom);
        if self.applied != Some(look) {
            self.applied = Some(look);
            theme::apply(ctx, &self.p, self.s.animations);
            kuro_api::oauth::set_page_look(theme::login_page(&self.p, self.s.animations));
            ctx.set_zoom_factor(self.s.zoom);
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(min_size()));
        }
        let icon_changed = self.icon_applied != Some(self.s.icon);
        if icon_changed {
            self.icon_applied = Some(self.s.icon);
            let data = std::sync::Arc::new(icon::render(self.s.icon, 128));
            self.sys.set_notice_icon(&data);
            ctx.send_viewport_cmd(egui::ViewportCommand::Icon(Some(data)));
        }
        let tray = self.s.close_to_tray || self.hidden;
        if tray != self.tray_applied || (icon_changed && tray) {
            self.tray_applied = tray;
            if let Err(e) = self.sys.set_tray(tray, &icon::render(self.s.icon, 32)) {
                // Without a tray icon a hidden window could never come back.
                tracing::warn!("tray unavailable: {e}");
                self.s.close_to_tray = false;
                self.tray_applied = false;
                if self.hidden {
                    self.show_window(ctx);
                }
            }
        }
    }

    pub(super) fn close_request(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && self.s.close_to_tray && !self.really_quit {
            self.hidden = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
    }
}
