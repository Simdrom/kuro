#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;

use eframe::egui;
use kuro_api::{Api, ClientIds, Keyring};
use kuro_app::app::{App, min_size};
use kuro_app::backend::Backend;
use kuro_app::{icon, system};
use kuro_core::Store;

type AnyError = Box<dyn std::error::Error + Send + Sync>;

fn main() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));
    tracing_subscriber::fmt().with_env_filter(filter).with_writer(std::io::stderr).init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let hidden = args.iter().any(|a| a == "--hidden");
    let link = args.iter().find(|a| kuro_core::links::parse(a).is_some()).cloned();
    // A separate folder keeps a second, independent profile.
    let profile = std::env::var("KURO_PROFILE_DIR").ok();
    // Started by the main window for Kioku's sign-in, before the single-instance check.
    #[cfg(windows)]
    if args.first().map(String::as_str) == Some(kuro_app::kioku_login::ARG) {
        let code = match args.get(1).map(String::as_str) {
            Some("crunchyroll") => kuro_app::kioku_login::run(secrets_name(profile.as_deref())),
            _ => kuro_app::kioku_login::FAILED,
        };
        std::process::exit(code);
    }
    if args.iter().any(|a| a == "--quit") {
        let gone = system::quit(profile.as_deref(), std::time::Duration::from_secs(15));
        std::process::exit(i32::from(!gone));
    }
    if system::forward(profile.as_deref(), link.as_deref()) {
        return;
    }
    if let Err(e) = run(hidden, link, profile) {
        tracing::error!("{e}");
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("Kuro")
            .set_description(e.to_string())
            .show();
        std::process::exit(1);
    }
}

/// Brings over the list, settings and sessions saved before the app was renamed.
fn adopt_previous(data: &std::path::Path, default_profile: bool, secrets: &str) {
    const OLD: &str = "mango.db";
    let mut places = vec![data.join(OLD)];
    if default_profile && let Some(d) = directories::ProjectDirs::from("", "", "MangoDesktop") {
        places.push(d.data_local_dir().join(OLD));
    }
    for old in places {
        match Store::adopt(&old, &data.join("kuro.db")) {
            Ok(true) => break,
            Ok(false) => {}
            Err(e) => tracing::warn!("the previous list could not be brought over: {e}"),
        }
    }
    let old = if default_profile { "mango-desktop" } else { "mango-desktop-profile" };
    if let Err(e) = Keyring::new(secrets).adopt(&Keyring::new(old)) {
        tracing::warn!("the previous sessions could not be brought over: {e}");
    }
}

/// Name of the Credential Manager entries of a profile.
fn secrets_name(profile: Option<&str>) -> &'static str {
    if profile.is_some() { "kuro-profile" } else { "kuro" }
}

fn run(hidden: bool, link: Option<String>, profile: Option<String>) -> Result<(), AnyError> {
    let secrets = secrets_name(profile.as_deref());
    let (data, cache) = match &profile {
        Some(p) => {
            let p = std::path::PathBuf::from(p);
            (p.clone(), p.join("cache"))
        }
        None => {
            let dirs = directories::ProjectDirs::from("", "", "Kuro").ok_or("no home directory")?;
            (dirs.data_local_dir().to_owned(), dirs.cache_dir().to_owned())
        }
    };
    std::fs::create_dir_all(&data)?;
    adopt_previous(&data, profile.is_none(), secrets);
    let store = Store::open(&data.join("kuro.db"))?;
    let s = store.settings()?;
    let ids = ClientIds::with_defaults(&s.anilist_client_id, &s.mal_client_id);
    let api = Api::new(Arc::new(Keyring::new(secrets)), ids)?;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Kuro")
            .with_app_id(system::APP_ID)
            .with_inner_size([1240.0, 820.0])
            .with_min_inner_size(min_size())
            .with_visible(!hidden)
            .with_icon(icon::render(s.icon, 128)),
        persist_window: true,
        persistence_path: Some(data.join("window.ron")),
        ..Default::default()
    };
    eframe::run_native(
        "Kuro",
        options,
        Box::new(move |cc| {
            let bg = Backend::new(api, store, cache.join("images"), cc.egui_ctx.clone(), true);
            let reply = bg.reply();
            system::listen(profile.as_deref(), reply.clone());
            #[cfg(windows)]
            let sys = system::WindowsSystem::new(reply, data.join("notifications"));
            #[cfg(windows)]
            if profile.is_none() {
                sys.adopt_previous();
            }
            #[cfg(windows)]
            let sys: Box<dyn system::System> = Box::new(sys);
            #[cfg(not(windows))]
            let sys: Box<dyn system::System> = Box::new(system::NoSystem);
            let mut app = App::new(&cc.egui_ctx, bg, sys, hidden);
            if let Some(l) = link {
                app.open_link(&l);
            }
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| e.to_string().into())
}
