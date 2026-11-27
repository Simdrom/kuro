//! The episode calendar: the .ics file and Google Calendar.

use super::*;

/// Google Calendar is sent the episodes at least this often, in seconds.
pub(super) const GOOGLE_EVERY: i64 = 6 * 3600;

/// A Google Calendar error in words, naming the unticked calendar box.
pub(super) fn google_error(e: &ApiError) -> String {
    match e {
        ApiError::Auth(m) if m == kuro_api::google::NO_CALENDAR => t("Google did not give Kuro access to the calendar. Sign out, sign in again and tick the calendar box.").into(),
        ApiError::Service(m) if m.contains("insufficient authentication scopes") => {
            t("Google did not give Kuro access to the calendar. Sign out, sign in again and tick the calendar box.").into()
        }
        ApiError::Unauthorized => {
            t("The Google session has expired. Reconnect to keep your calendar up to date.").into()
        }
        e => format::error(e),
    }
}

impl App {
    /// Rewrites the calendar file when its episodes changed.
    pub(super) fn write_calendar(&mut self) {
        if self.s.calendar_file.is_empty() {
            return;
        }
        let all: Vec<ListEntry> = self.lib[0].iter().map(|e| (**e).clone()).collect();
        let text = kuro_core::calendar::ics(&all, self.s.title_language, t("Episode"));
        if text != self.calendar_text {
            self.calendar_text = text.clone();
            self.bg.write_calendar(self.s.calendar_file.clone().into(), text);
        }
    }

    /// Sends the upcoming episodes to Google Calendar when they changed, and
    /// now and then anyway in case events were edited there.
    pub(super) fn sync_google(&mut self) {
        if self.google_busy
            || self.google_expired
            || self.s.google_account.is_empty()
            || !self.bg.online()
            || !self.bg.api.google.is_signed_in()
        {
            return;
        }
        let all: Vec<ListEntry> = self.lib[0].iter().map(|e| (**e).clone()).collect();
        let episodes = kuro_core::calendar::episodes(&all, self.s.title_language, t("Episode"));
        if episodes == self.google_sent && now() - self.google_at < GOOGLE_EVERY {
            return;
        }
        (self.google_busy, self.google_at) = (true, now());
        self.google_sent = episodes.clone();
        let id = self.s.google_calendar.clone();
        let name = self.s.google_calendars.iter().find(|c| c.id == id).map(|c| c.name.clone());
        self.bg.google_sync(id, name.unwrap_or_else(|| GOOGLE_CALENDAR_NAME.into()), episodes);
    }

    /// Makes a calendar called `name` and sends the episodes there.
    pub fn google_create(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() || self.google_creating {
            return;
        }
        self.google_creating = true;
        self.bg.google_create(name.chars().take(100).collect());
    }

    pub fn google_connect(&mut self) {
        self.google_connecting = true;
        self.bg.google_connect();
    }

    pub fn google_disconnect(&mut self) {
        self.bg.google_sign_out();
        self.s.google_account.clear();
        self.s.google_calendar.clear();
        // They belong to that account.
        self.s.google_calendars.clear();
        (self.google_sent, self.google_error, self.google_new) = (Vec::new(), None, None);
        self.google_expired = false;
    }
}
