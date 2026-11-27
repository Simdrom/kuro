//! A closer look at the list: scores, genres, studios, formats and years.

use eframe::egui::{self, Align2, Role, Sense, Ui, pos2, vec2};
use kuro_core::MediaKind;
use kuro_core::stats::{self, Insights, Share};

use crate::app::App;
use crate::i18n::{decimal, number, t, tf};
use crate::theme::{self, Palette};
use crate::widgets::{self, card, fact, section, separator};
use crate::{format, icons};

/// Genres and studios shown; the rest are left out.
const TOP: usize = 10;
/// Years shown in the finished chart, the most recent ones.
const YEARS: usize = 8;

pub fn show(app: &mut App, ui: &mut Ui, k: MediaKind) {
    let p = app.p;
    super::header(app, ui, &tf("{} statistics", &[&format::kind(k)]), |_, _| {});
    let s = stats::insights(app.library(k).iter().map(|e| &**e), k);
    super::scroll(ui, ("stats", k), |ui| {
        ui.set_max_width(ui.available_width().min(720.0));
        if s.titles == 0 {
            widgets::empty(ui, &p, icons::CHART_BAR, t("Nothing to count yet"));
            return;
        }
        summary(ui, &p, &s, k);
        if s.scores.iter().any(|n| *n > 0) {
            section(ui, &p, t("Scores"));
            card(ui, &p, |ui| histogram(ui, &p, &s.scores));
        }
        shares(ui, &p, t("Genres"), &s.genres);
        if k == MediaKind::Anime {
            shares(ui, &p, t("Studios"), &s.studios);
        }
        if !s.formats.is_empty() {
            section(ui, &p, t("Format"));
            card(ui, &p, |ui| {
                let most = s.formats.first().map_or(1, |f| f.1);
                for (i, (f, n)) in s.formats.iter().enumerate() {
                    if i > 0 {
                        separator(ui, &p);
                    }
                    bar_row(ui, &p, format::format(*f), *n, most, &number(*n as u64));
                }
            });
        }
        if !s.finished_by_year.is_empty() {
            section(ui, &p, t("Completed by year"));
            card(ui, &p, |ui| {
                let years = &s.finished_by_year[s.finished_by_year.len().saturating_sub(YEARS)..];
                let most = years.iter().map(|y| y.1).max().unwrap_or(1);
                for (i, (year, n)) in years.iter().rev().enumerate() {
                    if i > 0 {
                        separator(ui, &p);
                    }
                    bar_row(ui, &p, &year.to_string(), *n, most, &number(*n as u64));
                }
            });
        }
    });
}

fn summary(ui: &mut Ui, p: &Palette, s: &Insights, k: MediaKind) {
    section(ui, p, t("Overview"));
    card(ui, p, |ui| {
        let mut rows: Vec<(&str, String)> = vec![(t("Titles"), number(s.titles as u64))];
        if k == MediaKind::Anime {
            rows.push((t("Episodes"), number(s.units)));
            rows.push((t("Time watched"), tf("{} days", &[&decimal(s.days, 1)])));
            rows.push((t("Times rewatched"), number(s.rewatches)));
        } else {
            rows.push((t("Chapters"), number(s.units)));
            rows.push((t("Volumes"), number(s.volumes)));
            rows.push((t("Times reread"), number(s.rewatches)));
        }
        if let Some(m) = s.mean_score {
            rows.push((t("My average score"), decimal(m, 2)));
        }
        if let Some(m) = s.median_score {
            rows.push((t("Median score"), decimal(m, 1)));
        }
        if let Some(c) = s.completion {
            rows.push((t("Finished rather than dropped"), format!("{}%", (c * 100.0).round())));
        }
        if let Some(g) = s.favourite_genre(3) {
            let score = g.mean_score.map(|m| decimal(m, 1)).unwrap_or_default();
            rows.push((t("Best scored genre"), format!("{} · {score}", g.name)));
        }
        for (i, (label, value)) in rows.iter().enumerate() {
            if i > 0 {
                separator(ui, p);
            }
            fact(ui, p, label, value);
        }
    });
}

/// Columns for scores 1 to 10, each labelled with its count.
fn histogram(ui: &mut Ui, p: &Palette, scores: &[usize; 10]) {
    let most = scores.iter().copied().max().unwrap_or(1).max(1);
    let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), 150.0), Sense::hover());
    let text: Vec<String> = scores.iter().enumerate().map(|(i, n)| format!("{}: {n}", i + 1)).collect();
    widgets::label_a11y(&r, Role::Image, &format!("{} · {}", t("Scores"), text.join(", ")));
    let pt = ui.painter();
    let slot = rect.width() / 10.0;
    let (top, bottom) = (rect.top() + 18.0, rect.bottom() - 20.0);
    for (i, n) in scores.iter().enumerate() {
        let x = rect.left() + slot * (i as f32 + 0.5);
        let h = (bottom - top) * *n as f32 / most as f32;
        let w = (slot * 0.62).min(34.0);
        let col = egui::Rect::from_min_max(pos2(x - w / 2.0, bottom - h.max(2.0)), pos2(x + w / 2.0, bottom));
        let fill = if *n == most { p.tint } else { theme::blend(p.tint, p.card, 0.45) };
        pt.rect_filled(col, egui::CornerRadius::same(4), fill);
        if *n > 0 {
            let at = pos2(x, col.top() - 3.0);
            pt.text(at, Align2::CENTER_BOTTOM, number(*n as u64), theme::regular(11.5), p.dim);
        }
        pt.text(
            pos2(x, rect.bottom()),
            Align2::CENTER_BOTTOM,
            (i + 1).to_string(),
            theme::regular(12.0),
            p.faint,
        );
    }
}

/// The most common names, with a bar for each and their average score.
fn shares(ui: &mut Ui, p: &Palette, title: &str, all: &[Share]) {
    if all.is_empty() {
        return;
    }
    section(ui, p, title);
    card(ui, p, |ui| {
        let most = all.first().map_or(1, |s| s.count);
        for (i, s) in all.iter().take(TOP).enumerate() {
            if i > 0 {
                separator(ui, p);
            }
            let mut value = number(s.count as u64);
            if let Some(m) = s.mean_score {
                value = format!("{value}  ·  ★ {}", decimal(m, 1));
            }
            bar_row(ui, p, &s.name, s.count, most, &value);
        }
    });
}

/// `label  ▬▬▬▬▬▬▬▬   value`
fn bar_row(ui: &mut Ui, p: &Palette, label: &str, n: usize, most: usize, value: &str) {
    let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover());
    widgets::label_a11y(&r, Role::Label, &format!("{label}: {value}"));
    let c = rect.center().y;
    let name_w = (rect.width() * 0.34).clamp(70.0, 200.0);
    let value_w = 96.0;
    let l = widgets::galley(ui, label, theme::regular(13.5), p.text, name_w - 8.0, 1);
    let v = widgets::galley(ui, value, theme::regular(12.5), p.dim, value_w, 1);
    let pt = ui.painter();
    pt.galley(pos2(rect.left(), c - l.size().y / 2.0), l, p.text);
    pt.galley(pos2(rect.right() - v.size().x, c - v.size().y / 2.0), v, p.dim);
    let track = egui::Rect::from_min_max(
        pos2(rect.left() + name_w, c - 4.0),
        pos2((rect.right() - value_w - 8.0).max(rect.left() + name_w + 10.0), c + 4.0),
    );
    widgets::bar(pt, track, n as f32 / most.max(1) as f32, p.tint, p.line);
}
