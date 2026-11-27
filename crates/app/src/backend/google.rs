//! Google Calendar.

use super::*;

impl Backend {
    pub fn google_connect(&mut self) {
        self.cancel_login();
        let (google, reply) = (self.api.google.clone(), self.reply());
        self.login = self.spawn(async move {
            let login = match google.begin_login().await {
                Ok(l) => l,
                Err(e) => return reply.send(Event::GoogleConnected(Err(e))),
            };
            reply.send(Event::GoogleUrl(login.authorize_url().map(String::from)));
            reply.send(Event::GoogleConnected(google.finish_login(login).await));
        });
    }

    pub fn google_sign_out(&self) {
        let google = self.api.google.clone();
        self.spawn(async move {
            if let Err(e) = google.sign_out().await {
                tracing::warn!("google sign-out: {e}");
            }
        });
    }

    /// Makes the Google calendar show `episodes`.
    pub fn google_sync(&self, calendar: String, name: String, episodes: Vec<kuro_core::calendar::Episode>) {
        let (google, reply) = (self.api.google.clone(), self.reply());
        self.spawn(async move {
            let result = google.sync(&calendar, &name, &episodes).await;
            reply.send(Event::GoogleSynced { asked: calendar, name, result });
        });
    }

    pub fn google_create(&self, name: String) {
        let (google, reply) = (self.api.google.clone(), self.reply());
        self.spawn(async move {
            let result = google.create(&name).await;
            reply.send(Event::GoogleCreated { name, result });
        });
    }

    /// Takes the episodes still to come out of a calendar left behind.
    pub fn google_clear(&self, calendar: String) {
        let google = self.api.google.clone();
        self.spawn(async move {
            if let Err(e) = google.clear(&calendar).await {
                tracing::warn!("google calendar left behind: {e}");
            }
        });
    }
}
