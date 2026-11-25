//! Windows integration: toasts, tray, autostart, links and single instance.

use std::io::{BufRead, BufReader, Read, Write};
use std::time::{Duration, Instant};

use interprocess::local_socket::traits::{ListenerExt, Stream as _};
use interprocess::local_socket::{GenericNamespaced, ListenerOptions, Stream, ToNsName};
use kuro_core::links;

use crate::backend::{Event, Reply};

pub const APP_ID: &str = "Kuro.Tracker";
const MAX_LINE: u64 = 4096;
const SHOW: &str = "show";
const QUIT: &str = "quit";

fn pipe_name(profile: Option<&str>) -> String {
    let user = std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_default();
    let user: String = user.chars().filter(char::is_ascii_alphanumeric).collect();
    match profile {
        // Pipe names allow only a few characters, so use a hash of the folder.
        Some(p) => {
            use std::hash::{Hash, Hasher};
            let mut h = std::hash::DefaultHasher::new();
            p.hash(&mut h);
            format!("kuro-{user}-{:x}", h.finish())
        }
        None => format!("kuro-{user}"),
    }
}

/// Passes `link` to a running instance. Returns false when there is none.
pub fn forward(profile: Option<&str>, link: Option<&str>) -> bool {
    forward_to(&pipe_name(profile), link)
}

fn forward_to(name: &str, link: Option<&str>) -> bool {
    send_to(name, link.filter(|l| links::parse(l).is_some()).unwrap_or(SHOW))
}

fn send_to(name: &str, line: &str) -> bool {
    let Ok(name) = name.to_ns_name::<GenericNamespaced>() else { return false };
    let Ok(mut s) = Stream::connect(name) else { return false };
    writeln!(s, "{line}").is_ok()
}

/// Closes the running instance, even from the tray, and waits for it. The installer
/// needs its files free. Returns false if it is still there after `wait`.
pub fn quit(profile: Option<&str>, wait: Duration) -> bool {
    quit_at(&pipe_name(profile), wait)
}

fn quit_at(name: &str, wait: Duration) -> bool {
    let end = Instant::now() + wait;
    while send_to(name, QUIT) {
        if Instant::now() >= end {
            return false;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    true
}

/// Receives links from later instances.
pub fn listen(profile: Option<&str>, reply: Reply) {
    listen_on(&pipe_name(profile), reply);
}

fn listen_on(name: &str, reply: Reply) {
    let Ok(name) = name.to_owned().to_ns_name::<GenericNamespaced>() else { return };
    let Ok(listener) = ListenerOptions::new().name(name).create_sync() else { return };
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let reply = reply.clone();
            std::thread::spawn(move || {
                let mut line = String::new();
                if BufReader::new(conn.take(MAX_LINE)).read_line(&mut line).is_err() {
                    return;
                }
                let line = line.trim();
                // Any local program can write here, so only known words and links get through.
                if line == SHOW {
                    reply.send(Event::ShowWindow);
                } else if line == QUIT {
                    reply.send(Event::Quit);
                } else if links::parse(line).is_some() {
                    reply.send(Event::ShowWindow);
                    reply.send(Event::OpenLink(line.to_owned()));
                }
            });
        }
    });
}

pub trait System {
    fn notify(&self, title: &str, body: &str, link: Option<String>);
    fn set_autostart(&self, on: bool) -> Result<(), String>;
    fn set_link_handler(&self, on: bool) -> Result<(), String>;
    /// Shows or hides the tray icon.
    fn set_tray(&mut self, on: bool, icon: &eframe::egui::IconData) -> Result<(), String>;
    fn open_url(&self, url: &str);
    fn copy(&self, text: &str);
    fn clipboard(&self) -> Option<String>;
}

/// Opens only web addresses, never files or other protocols.
pub fn is_web_url(url: &str) -> bool {
    let ok = url.starts_with("https://") || url.starts_with("http://");
    ok && url.len() <= 2048 && !url.chars().any(|c| c.is_control() || c.is_whitespace() || c == '"')
}

/// Does nothing. Used by tests.
pub struct NoSystem;

impl System for NoSystem {
    fn notify(&self, _: &str, _: &str, _: Option<String>) {}
    fn set_autostart(&self, _: bool) -> Result<(), String> {
        Ok(())
    }
    fn set_link_handler(&self, _: bool) -> Result<(), String> {
        Ok(())
    }
    fn set_tray(&mut self, _: bool, _: &eframe::egui::IconData) -> Result<(), String> {
        Ok(())
    }
    fn open_url(&self, _: &str) {}
    fn copy(&self, _: &str) {}
    fn clipboard(&self) -> Option<String> {
        None
    }
}

#[cfg(windows)]
pub use windows::WindowsSystem;

#[cfg(windows)]
mod windows {
    use super::*;
    use crate::i18n::t;
    use eframe::egui::IconData;
    use tauri_winrt_notification::{Duration as ToastDuration, Toast};
    use tray_icon::menu::{Menu, MenuEvent, MenuItem};
    use tray_icon::{MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent};
    use windows_registry::CURRENT_USER;

    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const RUN_VALUE: &str = "Kuro";
    const PROTOCOL: &str = r"Software\Classes\kuro";

    pub struct WindowsSystem {
        reply: Reply,
        tray: Option<TrayIcon>,
    }

    fn exe() -> Result<String, String> {
        let p = std::env::current_exe().map_err(|e| e.to_string())?;
        p.to_str().map(str::to_owned).ok_or_else(|| "path".into())
    }

    fn err(e: impl std::fmt::Display) -> String {
        e.to_string()
    }

    impl WindowsSystem {
        pub fn new(reply: Reply) -> Self {
            // Gives toasts the right name.
            let id = CURRENT_USER.create(format!(r"Software\Classes\AppUserModelId\{APP_ID}"));
            if let Ok(k) = id {
                let _ = k.set_string("DisplayName", "Kuro");
            }
            Self { reply, tray: None }
        }

        /// Moves registrations made before the app was renamed.
        pub fn adopt_previous(&self) {
            let _ = CURRENT_USER.remove_tree(r"Software\Classes\AppUserModelId\MangoDesktop.Tracker");
            if let Ok(run) = CURRENT_USER.options().read().write().open(RUN)
                && run.get_string("MangoDesktop").is_ok()
                && self.set_autostart(true).is_ok()
            {
                let _ = run.remove_value("MangoDesktop");
            }
            // The scheme may belong to another program, so look before removing it.
            const OLD: &str = r"Software\Classes\mango";
            let command = CURRENT_USER
                .open(format!(r"{OLD}\shell\open\command"))
                .and_then(|k| k.get_string(""))
                .unwrap_or_default();
            if command.contains("mango-desktop.exe") && self.set_link_handler(true).is_ok() {
                let _ = CURRENT_USER.remove_tree(OLD);
            }
        }
    }

    impl System for WindowsSystem {
        fn notify(&self, title: &str, body: &str, link: Option<String>) {
            let reply = self.reply.clone();
            let toast =
                Toast::new(APP_ID).title(title).text1(body).duration(ToastDuration::Short).on_activated(
                    move |_| {
                        reply.send(Event::ShowWindow);
                        if let Some(l) = &link {
                            reply.send(Event::OpenLink(l.clone()));
                        }
                        Ok(())
                    },
                );
            if let Err(e) = toast.show() {
                tracing::warn!("toast failed: {e}");
            }
        }

        fn set_autostart(&self, on: bool) -> Result<(), String> {
            let key = CURRENT_USER.create(RUN).map_err(err)?;
            if on {
                key.set_string(RUN_VALUE, format!("\"{}\" --hidden", exe()?)).map_err(err)
            } else {
                let _ = key.remove_value(RUN_VALUE);
                Ok(())
            }
        }

        fn set_link_handler(&self, on: bool) -> Result<(), String> {
            if !on {
                let _ = CURRENT_USER.remove_tree(PROTOCOL);
                return Ok(());
            }
            let key = CURRENT_USER.create(PROTOCOL).map_err(err)?;
            key.set_string("", "URL:Kuro").map_err(err)?;
            key.set_string("URL Protocol", "").map_err(err)?;
            let cmd = key.create(r"shell\open\command").map_err(err)?;
            cmd.set_string("", format!("\"{}\" \"%1\"", exe()?)).map_err(err)
        }

        fn set_tray(&mut self, on: bool, icon: &IconData) -> Result<(), String> {
            if !on {
                self.tray = None;
                return Ok(());
            }
            let icon = tray_icon::Icon::from_rgba(icon.rgba.clone(), icon.width, icon.height).map_err(err)?;
            if let Some(tray) = &self.tray {
                return tray.set_icon(Some(icon)).map_err(err);
            }
            let (open, quit) = (MenuItem::new(t("Open"), true, None), MenuItem::new(t("Quit"), true, None));
            let menu = Menu::new();
            menu.append(&open).map_err(err)?;
            menu.append(&quit).map_err(err)?;
            let (open_id, quit_id) = (open.id().clone(), quit.id().clone());
            let reply = self.reply.clone();
            MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
                if e.id == open_id {
                    reply.send(Event::ShowWindow);
                } else if e.id == quit_id {
                    reply.send(Event::Quit);
                }
            }));
            let reply = self.reply.clone();
            TrayIconEvent::set_event_handler(Some(move |e| {
                if matches!(e, TrayIconEvent::DoubleClick { button: MouseButton::Left, .. }) {
                    reply.send(Event::ShowWindow);
                }
            }));
            let tray = TrayIconBuilder::new()
                .with_tooltip("Kuro")
                .with_icon(icon)
                .with_menu(Box::new(menu))
                .build()
                .map_err(err)?;
            self.tray = Some(tray);
            Ok(())
        }

        fn open_url(&self, url: &str) {
            if is_web_url(url)
                && let Err(e) = open::that_detached(url)
            {
                tracing::warn!("could not open browser: {e}");
            }
        }

        fn copy(&self, text: &str) {
            if let Ok(mut c) = arboard::Clipboard::new() {
                let _ = c.set_text(text);
            }
        }

        fn clipboard(&self) -> Option<String> {
            arboard::Clipboard::new().ok()?.get_text().ok()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Backend;
    use kuro_api::{Api, ClientIds, MemorySecrets};
    use kuro_core::Store;
    use std::sync::Arc;

    #[test]
    fn only_web_addresses_can_be_opened() {
        assert!(is_web_url("https://anilist.co/anime/1"));
        for bad in ["file:///c:/x", "calc.exe", "https://a.b/\" & calc", "https://a.b/ x", "javascript:1", ""]
        {
            assert!(!is_web_url(bad), "{bad}");
        }
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "shows a notification"]
    fn toast_is_accepted() {
        let api = Api::new(Arc::new(MemorySecrets::default()), ClientIds::default()).unwrap();
        let ctx = eframe::egui::Context::default();
        let bg = Backend::new(api, Store::in_memory().unwrap(), std::env::temp_dir(), ctx, false);
        let _sys = WindowsSystem::new(bg.reply());
        tauri_winrt_notification::Toast::new(APP_ID).title("Kuro").text1("Test").show().unwrap();
    }

    #[test]
    fn second_instance_hands_over_valid_links_only() {
        let name = format!("kuro-test-{}", std::process::id());
        assert!(!forward_to(&name, None));
        let api = Api::new(Arc::new(MemorySecrets::default()), ClientIds::default()).unwrap();
        let bg = Backend::new(
            api,
            Store::in_memory().unwrap(),
            std::env::temp_dir(),
            eframe::egui::Context::default(),
            false,
        );
        listen_on(&name, bg.reply());
        assert!(forward_to(&name, Some("https://anilist.co/anime/5")));
        assert!(forward_to(&name, Some("file:///c:/evil")));
        // A raw writer must not be able to inject anything either.
        let mut s = Stream::connect(name.clone().to_ns_name::<GenericNamespaced>().unwrap()).unwrap();
        writeln!(s, "calc.exe").unwrap();
        let (mut open, mut show) = (Vec::new(), 0);
        let end = Instant::now() + Duration::from_secs(2);
        while Instant::now() < end && (show < 2 || open.is_empty()) {
            match bg.next_event() {
                Some(Event::OpenLink(l)) => open.push(l),
                Some(Event::ShowWindow) => show += 1,
                _ => std::thread::sleep(Duration::from_millis(10)),
            }
        }
        assert_eq!(open, ["https://anilist.co/anime/5"]);
        assert_eq!(show, 2);

        // The installer asks for a way out; with nobody listening there is nothing to wait for.
        assert!(!quit_at(&name, Duration::ZERO));
        let end = Instant::now() + Duration::from_secs(2);
        while !matches!(bg.next_event(), Some(Event::Quit)) {
            assert!(Instant::now() < end, "quit did not arrive");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(quit_at(&format!("{name}-nobody"), Duration::ZERO));
    }
}
