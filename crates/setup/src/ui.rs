//! The setup window: what is installed, the one action that fits, and uninstalling. The work
//! runs on its own thread so the window keeps drawing.

use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, atomic::AtomicBool, atomic::Ordering};

use eframe::egui::{
    self, Align, CentralPanel, Color32, Frame, Layout, Panel, RichText, ScrollArea, TextureHandle,
};

use crate::Opts;
use crate::engine::{self, Choices, Log, Plan, State, version};
use crate::text::{t, tf};

pub const TOO_NEW: &str = "Kuro {0} has changed your lists in a way that Kuro {1} cannot read, so going back is not possible. Nothing has been changed.";

enum Page {
    Home,
    Uninstall,
    Working,
    Done(Result<String, String>),
}

struct Setup {
    st: State,
    plan: Plan,
    page: Page,
    desktop: bool,
    launch: bool,
    purge: bool,
    details: bool,
    log: Arc<Log>,
    work: Option<Receiver<Result<String, String>>>,
    failed: Arc<AtomicBool>,
    icon: Option<TextureHandle>,
}

pub fn run(o: &Opts) -> i32 {
    let st = engine::read_state();
    let plan = engine::plan(&st);
    let failed = Arc::new(AtomicBool::new(false));
    let setup = Setup {
        desktop: o.desktop.unwrap_or(st.desktop),
        // An update brings Kuro back only if it was open.
        launch: st.installed.is_none() || st.running,
        page: if o.uninstall && st.installed.is_some() { Page::Uninstall } else { Page::Home },
        st,
        plan,
        purge: false,
        details: false,
        log: Arc::default(),
        work: None,
        failed: failed.clone(),
        icon: None,
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(t("Kuro Setup"))
            .with_inner_size([560.0, 380.0])
            .with_resizable(false)
            .with_maximize_button(false)
            .with_icon(icon()),
        centered: true,
        ..Default::default()
    };
    let shown = eframe::run_native("Kuro Setup", options, Box::new(|_| Ok(Box::new(setup))));
    i32::from(shown.is_err() || failed.load(Ordering::Relaxed))
}

/// 黒 on an ink tile, drawn by assets/make_icon.py.
fn icon() -> egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../assets/setup.png")).unwrap_or_default()
}

impl Setup {
    fn start(&mut self, ctx: &egui::Context, uninstall: bool) {
        let (tx, rx) = channel();
        let (st, log, ctx) = (self.st.clone(), self.log.clone(), ctx.clone());
        let (c, purge) = (Choices { desktop: self.desktop, launch: self.launch }, self.purge);
        std::thread::spawn(move || {
            let r = if uninstall {
                engine::uninstall(&st, purge, &log).map(|()| t("Kuro is uninstalled."))
            } else {
                engine::install(&st, c, &log).map(|()| tf("Kuro {0} is installed.", &[version()]))
            };
            if let Err(e) = &r {
                log.add(format!("Failed: {e}"));
            }
            let _ = tx.send(r);
            ctx.request_repaint();
        });
        self.work = Some(rx);
        self.page = Page::Working;
    }

    fn poll(&mut self) {
        if let Some(r) = self.work.as_ref().and_then(|w| w.try_recv().ok()) {
            if r.is_err() {
                self.failed.store(true, Ordering::Relaxed);
            }
            self.work = None;
            self.page = Page::Done(r);
        }
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        let icon = self.icon.get_or_insert_with(|| {
            let i = icon();
            let size = [i.width as usize, i.height as usize];
            let image = egui::ColorImage::from_rgba_unmultiplied(size, &i.rgba);
            ui.ctx().load_texture("icon", image, Default::default())
        });
        ui.horizontal(|ui| {
            ui.add(egui::Image::new((icon.id(), egui::vec2(48.0, 48.0))));
            ui.vertical(|ui| {
                ui.label(RichText::new("Kuro").size(24.0).strong());
                ui.label(RichText::new(tf("Version {0}", &[version()])).weak());
            });
        });
        ui.add_space(16.0);
    }

    fn home(&mut self, ui: &mut egui::Ui) {
        match &self.st.installed {
            Some(i) => {
                ui.label(tf("Kuro {0} is installed in {1}", &[&i.version, &i.dir.display().to_string()]));
                if self.st.running {
                    ui.label(RichText::new(t("Kuro is open: it will be closed first.")).weak());
                }
            }
            None => {
                ui.label(t("Kuro is not installed on this computer."));
            }
        }
        ui.add_space(8.0);
        let this = version();
        let message = match &self.plan {
            Plan::Install => t("Installs Kuro for your user only, without administrator rights."),
            Plan::Update(from) => tf(
                "Kuro {0} will be updated to {1}. Your lists, settings and sessions are kept.",
                &[from, this],
            ),
            Plan::Reinstall => t("This version is already installed. Installing it again repairs it."),
            Plan::Downgrade(from) => tf(
                "Kuro {0} is newer than this setup. You can go back to {1}; your lists are kept.",
                &[from, this],
            ),
            Plan::TooNew(from) => tf(TOO_NEW, &[from, this]),
        };
        ui.label(message);
        if matches!(self.plan, Plan::TooNew(_)) {
            return;
        }
        ui.add_space(12.0);
        ui.checkbox(&mut self.desktop, t("Desktop shortcut"));
        ui.checkbox(&mut self.launch, t("Open Kuro when done"));
    }

    fn uninstall(&mut self, ui: &mut egui::Ui) {
        let v = self.st.installed.as_ref().map_or("", |i| i.version.as_str());
        ui.label(RichText::new(tf("Uninstall Kuro {0}", &[v])).strong());
        ui.add_space(4.0);
        ui.label(t("Kuro is closed and removed from this computer. Your lists, settings and sessions stay, so installing it again picks up where you left off."));
        ui.add_space(12.0);
        ui.checkbox(&mut self.purge, t("Also delete my lists, settings and sessions"));
        if self.purge {
            let warn = if ui.visuals().dark_mode {
                Color32::from_rgb(255, 140, 120)
            } else {
                Color32::from_rgb(180, 40, 20)
            };
            ui.label(
                RichText::new(t(
                    "Lists not synced with AniList or MyAnimeList are lost. This cannot be undone.",
                ))
                .color(warn),
            );
        }
    }

    fn done(&mut self, ui: &mut egui::Ui) {
        let Page::Done(r) = &self.page else { return };
        match r {
            Ok(text) => {
                ui.label(RichText::new(text).size(16.0).strong());
            }
            Err(e) => {
                ui.label(RichText::new(t("Something went wrong")).size(16.0).strong());
                ui.label(e);
            }
        }
    }

    fn buttons(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let close = |ui: &mut egui::Ui| {
            if ui.button(t("Close")).clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        };
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| match self.page {
            Page::Home => {
                let action = match &self.plan {
                    Plan::Install => Some(t("Install")),
                    Plan::Update(_) => Some(tf("Update to {0}", &[version()])),
                    Plan::Reinstall => Some(t("Reinstall")),
                    Plan::Downgrade(_) => Some(tf("Go back to {0}", &[version()])),
                    Plan::TooNew(_) => None,
                };
                if let Some(a) = action
                    && ui
                        .add(egui::Button::new(RichText::new(a).strong()).min_size(egui::vec2(110.0, 30.0)))
                        .clicked()
                {
                    self.start(&ctx, false);
                }
                close(ui);
                if self.st.installed.is_some() {
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        if ui.button(t("Uninstall…")).clicked() {
                            self.page = Page::Uninstall;
                        }
                    });
                }
            }
            Page::Uninstall => {
                if ui
                    .add(
                        egui::Button::new(RichText::new(t("Uninstall")).strong())
                            .min_size(egui::vec2(110.0, 30.0)),
                    )
                    .clicked()
                {
                    self.start(&ctx, true);
                }
                if ui.button(t("Back")).clicked() {
                    self.page = Page::Home;
                }
            }
            Page::Working => {
                ui.spinner();
                ui.label(t("Please wait…"));
            }
            Page::Done(_) => {
                close(ui);
                if ui.selectable_label(self.details, t("Details")).clicked() {
                    self.details = !self.details;
                }
            }
        });
    }
}

impl eframe::App for Setup {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        self.poll();
        let fill = ui.visuals().panel_fill;
        Panel::bottom("buttons")
            .show_separator_line(false)
            .frame(Frame::new().fill(fill).inner_margin(egui::Margin {
                left: 24,
                right: 24,
                top: 8,
                bottom: 16,
            }))
            .show(ui, |ui| self.buttons(ui));
        CentralPanel::no_frame().frame(Frame::new().fill(fill).inner_margin(24)).show(ui, |ui| {
            self.header(ui);
            match self.page {
                Page::Home => self.home(ui),
                Page::Uninstall => self.uninstall(ui),
                Page::Working => {}
                Page::Done(_) => {
                    self.done(ui);
                    if self.details {
                        ui.add_space(8.0);
                        ScrollArea::vertical().show(ui, |ui| {
                            for line in self.log.lines() {
                                ui.label(RichText::new(line).monospace().small());
                            }
                        });
                    }
                }
            }
        });
    }
}
