#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;

use eframe::egui;
use mango_api::{Api, ClientIds, Keyring};
use mango_app::app::{App, min_size};
use mango_app::backend::Backend;
use mango_app::{icon, system};
use mango_core::Store;

type AnyError = Box<dyn std::error::Error + Send + Sync>;

fn main() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));
    tracing_subscriber::fmt().with_env_filter(filter).with_writer(std::io::stderr).init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let hidden = args.iter().any(|a| a == "--hidden");
    let link = args.iter().find(|a| mango_core::links::parse(a).is_some()).cloned();
    // A separate folder keeps a second, independent profile.
    let profile = std::env::var("MANGO_PROFILE_DIR").ok();
    if system::forward(profile.as_deref(), link.as_deref()) {
        return;
    }
    if let Err(e) = run(hidden, link, profile) {
        tracing::error!("{e}");
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("Mango Desktop")
            .set_description(e.to_string())
            .show();
        std::process::exit(1);
    }
}

fn run(hidden: bool, link: Option<String>, profile: Option<String>) -> Result<(), AnyError> {
    let (data, cache, secrets) = match &profile {
        Some(p) => {
            let p = std::path::PathBuf::from(p);
            (p.clone(), p.join("cache"), "mango-desktop-profile")
        }
        None => {
            let dirs = directories::ProjectDirs::from("", "", "MangoDesktop").ok_or("no home directory")?;
            (dirs.data_local_dir().to_owned(), dirs.cache_dir().to_owned(), "mango-desktop")
        }
    };
    std::fs::create_dir_all(&data)?;
    let store = Store::open(&data.join("mango.db"))?;
    let s = store.settings()?;
    let ids = ClientIds::with_defaults(&s.anilist_client_id, &s.mal_client_id);
    let api = Api::new(Arc::new(Keyring::new(secrets)), ids)?;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Mango Desktop")
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
        "Mango Desktop",
        options,
        Box::new(move |cc| {
            let bg = Backend::new(api, store, cache.join("images"), cc.egui_ctx.clone(), true);
            let reply = bg.reply();
            system::listen(profile.as_deref(), reply.clone());
            #[cfg(windows)]
            let sys: Box<dyn system::System> = Box::new(system::WindowsSystem::new(reply));
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
