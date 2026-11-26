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
                } else if let Some(link) = links::parse(line) {
                    if !link.quiet() {
                        reply.send(Event::ShowWindow);
                    }
                    reply.send(Event::OpenLink(line.to_owned()));
                }
            });
        }
    });
}

/// A Windows notification.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Notice {
    pub title: String,
    pub body: String,
    /// A quieter last line.
    pub detail: Option<String>,
    /// A picture already on disk, shown beside the text.
    pub image: Option<std::path::PathBuf>,
    /// Opened when the notification or its Open button is clicked.
    pub link: Option<String>,
    /// Offers a button that marks this episode as watched.
    pub mark: Option<(kuro_core::MediaRef, u32)>,
}

impl Notice {
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self { title: title.into(), body: body.into(), ..Default::default() }
    }
}

pub trait System {
    fn notify(&self, n: Notice);
    /// The picture notifications carry as the app's.
    fn set_notice_icon(&mut self, _icon: &eframe::egui::IconData) {}
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
    fn notify(&self, _: Notice) {}
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
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::i18n::{t, tf};
    use eframe::egui::IconData;
    use tauri_winrt_notification::{Duration as ToastDuration, IconCrop, Toast};
    use tray_icon::menu::{Menu, MenuEvent, MenuItem};
    use tray_icon::{MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent};
    use windows_registry::CURRENT_USER;

    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const RUN_VALUE: &str = "Kuro";
    const PROTOCOL: &str = r"Software\Classes\kuro";
    /// Toast button actions.
    const MARK: &str = "mark";
    const OPEN: &str = "open";

    fn toast_key() -> String {
        format!(r"Software\Classes\AppUserModelId\{APP_ID}")
    }

    pub struct WindowsSystem {
        reply: Reply,
        tray: Option<TrayIcon>,
        /// Pictures for notifications, which Windows reads from disk.
        dir: PathBuf,
    }

    const COVERS_KEPT: usize = 30;

    /// The file extension Windows needs to read an image, from its first bytes.
    pub(super) fn image_ext(b: &[u8]) -> Option<&'static str> {
        if b.starts_with(b"\x89PNG\r\n\x1a\n") {
            Some("png")
        } else if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
            Some("jpg")
        } else if b.starts_with(b"GIF8") {
            Some("gif")
        } else {
            None
        }
    }

    /// Files in `dir` starting with `prefix`, newest first.
    fn files_with(dir: &Path, prefix: &str) -> Vec<PathBuf> {
        let Ok(read) = std::fs::read_dir(dir) else { return Vec::new() };
        let mut all: Vec<_> = read
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(prefix))
            .map(|e| (e.metadata().and_then(|m| m.modified()).ok(), e.path()))
            .collect();
        all.sort_by_key(|f| std::cmp::Reverse(f.0));
        all.into_iter().map(|(_, p)| p).collect()
    }

    fn exe() -> Result<String, String> {
        let p = std::env::current_exe().map_err(|e| e.to_string())?;
        p.to_str().map(str::to_owned).ok_or_else(|| "path".into())
    }

    fn err(e: impl std::fmt::Display) -> String {
        e.to_string()
    }

    impl WindowsSystem {
        pub fn new(reply: Reply, dir: PathBuf) -> Self {
            // Gives toasts the right name.
            if let Ok(k) = CURRENT_USER.create(toast_key()) {
                let _ = k.set_string("DisplayName", "Kuro");
            }
            Self { reply, tray: None, dir }
        }

        /// A copy of a cached cover that Windows can read: it needs the file extension.
        fn stage_cover(&self, src: &Path) -> Option<PathBuf> {
            let bytes = std::fs::read(src).ok()?;
            let ext = image_ext(&bytes)?;
            let name = src.file_name()?.to_str()?;
            let dest = self.dir.join(format!("cover-{name}.{ext}"));
            if !dest.is_file() {
                std::fs::create_dir_all(&self.dir).ok()?;
                std::fs::write(&dest, &bytes).ok()?;
            }
            for old in files_with(&self.dir, "cover-").into_iter().skip(COVERS_KEPT) {
                let _ = std::fs::remove_file(old);
            }
            Some(dest)
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
        fn notify(&self, n: Notice) {
            let mut toast = Toast::new(APP_ID).title(&n.title).text1(&n.body).duration(ToastDuration::Short);
            if let Some(d) = &n.detail {
                toast = toast.text2(d);
            }
            if let Some(p) = n.image.as_deref().and_then(|p| self.stage_cover(p)) {
                toast = toast.icon(&p, IconCrop::Square, &n.title);
            }
            if let Some((_, ep)) = n.mark {
                toast = toast.add_button(&tf("Mark episode {}", &[&ep]), MARK);
            }
            if n.link.is_some() {
                toast = toast.add_button(t("Open"), OPEN);
            }
            let reply = self.reply.clone();
            let (link, mark) = (n.link, n.mark);
            let toast = toast.on_activated(move |action| {
                match (action.as_deref(), mark) {
                    // Marking happens in the background, without bringing the window up.
                    (Some(MARK), Some((r, ep))) => reply.send(Event::MarkEpisode(r, ep)),
                    _ => {
                        reply.send(Event::ShowWindow);
                        if let Some(l) = &link {
                            reply.send(Event::OpenLink(l.clone()));
                        }
                    }
                }
                Ok(())
            });
            if let Err(e) = toast.show() {
                tracing::warn!("toast failed: {e}");
            }
        }

        fn set_notice_icon(&mut self, icon: &IconData) {
            // A new name per drawing, as Windows keeps showing a file it has read once.
            use std::hash::{Hash, Hasher};
            let mut h = std::hash::DefaultHasher::new();
            icon.rgba.hash(&mut h);
            let path = self.dir.join(format!("icon-{:x}.png", h.finish()));
            let written = path.is_file()
                || (std::fs::create_dir_all(&self.dir).is_ok()
                    && image::RgbaImage::from_raw(icon.width, icon.height, icon.rgba.clone())
                        .is_some_and(|img| img.save_with_format(&path, image::ImageFormat::Png).is_ok()));
            if !written {
                tracing::warn!("the notification icon could not be written");
                return;
            }
            for old in files_with(&self.dir, "icon-").into_iter().filter(|p| *p != path) {
                let _ = std::fs::remove_file(old);
            }
            if let Ok(k) = CURRENT_USER.create(toast_key()) {
                let _ = k.set_string("IconUri", path.display().to_string());
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
        let dir = std::env::temp_dir().join("kuro-toast-test");
        let mut sys = WindowsSystem::new(bg.reply(), dir.clone());
        sys.set_notice_icon(&crate::icon::render(Default::default(), 64));
        // Any picture stands in for a cover.
        let i = crate::icon::render(kuro_core::settings::AppIcon::Night, 96);
        let cover = dir.join("cached-cover");
        image::RgbaImage::from_raw(96, 96, i.rgba)
            .unwrap()
            .save_with_format(&cover, image::ImageFormat::Png)
            .unwrap();
        let r = kuro_core::MediaRef {
            service: kuro_core::Service::AniList,
            kind: kuro_core::MediaKind::Anime,
            id: 1,
        };
        sys.notify(Notice {
            title: "Black Clover 2nd Season".into(),
            body: "Episode 9 is out".into(),
            detail: Some("Watching · 8/?".into()),
            image: Some(cover),
            link: Some("https://anilist.co/anime/1".into()),
            mark: Some((r, 9)),
        });
        assert!(
            std::fs::read_dir(&dir)
                .unwrap()
                .flatten()
                .any(|e| e.path().extension().is_some_and(|x| x == "png"))
        );
    }

    #[cfg(windows)]
    #[test]
    fn toast_pictures_get_the_extension_windows_needs() {
        assert_eq!(windows::image_ext(b"\x89PNG\r\n\x1a\nrest"), Some("png"));
        assert_eq!(windows::image_ext(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("jpg"));
        assert_eq!(windows::image_ext(b"<html>"), None);
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
