//! Kuro Setup: installs, updates, reinstalls, goes back to an older version or uninstalls Kuro,
//! for the current user, with a window or silently.
//!
//! Command line: /S (silent), /uninstall, /purge (with /uninstall: also delete lists, settings
//! and sessions), /desktop=yes|no, /downgrade (silent: allow going back to this version).
//! Silent mode exits with 1 when it refused or failed; %TEMP%\Kuro-Setup.log says why.
#![windows_subsystem = "windows"]

#[cfg(not(windows))]
compile_error!("Kuro Setup is for Windows");

mod engine;
mod text;
mod ui;
mod win;

use engine::{Choices, Log, Plan};

#[derive(Default)]
pub struct Opts {
    silent: bool,
    uninstall: bool,
    purge: bool,
    downgrade: bool,
    desktop: Option<bool>,
}

impl Opts {
    fn parse(args: impl Iterator<Item = String>) -> Self {
        let mut o = Self::default();
        for a in args {
            match a.to_lowercase().as_str() {
                "/s" => o.silent = true,
                "/uninstall" => o.uninstall = true,
                "/purge" => o.purge = true,
                "/downgrade" => o.downgrade = true,
                "/desktop=yes" => o.desktop = Some(true),
                "/desktop=no" => o.desktop = Some(false),
                _ => {}
            }
        }
        o
    }
}

fn silent(o: &Opts) -> i32 {
    let log = Log::default();
    let st = engine::read_state();
    let r = if o.uninstall {
        engine::uninstall(&st, o.purge, &log)
    } else {
        match engine::plan(&st) {
            Plan::TooNew(v) => Err(text::tf(ui::TOO_NEW, &[&v, engine::version()])),
            Plan::Downgrade(v) if !o.downgrade => Err(format!("Kuro {v} is newer; /downgrade goes back")),
            _ => engine::install(
                &st,
                Choices { desktop: o.desktop.unwrap_or(st.desktop), launch: false },
                &log,
            ),
        }
    };
    if let Err(e) = &r {
        log.add(format!("Failed: {e}"));
    }
    i32::from(r.is_err())
}

fn main() {
    let o = Opts::parse(std::env::args().skip(1));
    let code = if o.silent { silent(&o) } else { ui::run(&o) };
    engine::finish();
    std::process::exit(code);
}
