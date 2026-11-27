use std::collections::HashMap;

use crate::model::{Format, ListEntry, ListStatus, MediaKind};

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct StatusStat {
    pub count: usize,
    /// Watch time, anime only.
    pub days: f32,
    pub units: u64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Stats {
    pub by_status: [StatusStat; 5],
    pub mean_score: Option<f32>,
}

impl Stats {
    pub fn get(&self, status: ListStatus) -> StatusStat {
        let i = ListStatus::ALL.iter().position(|s| *s == status).unwrap_or(0);
        self.by_status[i]
    }
}

pub fn compute<'a>(entries: impl IntoIterator<Item = &'a ListEntry>, kind: MediaKind) -> Stats {
    let mut s = Stats::default();
    let (mut sum, mut scored) = (0u64, 0u64);
    for e in entries.into_iter().filter(|e| e.media.kind == kind) {
        let i = ListStatus::ALL.iter().position(|x| *x == e.status).unwrap_or(0);
        let slot = &mut s.by_status[i];
        slot.count += 1;
        let total = u64::from(e.media.total().unwrap_or(0));
        let units = u64::from(e.progress) + u64::from(e.repeat_count).saturating_mul(total);
        slot.units += units;
        if kind == MediaKind::Anime {
            slot.days += units as f32 * e.media.duration.unwrap_or(0) as f32 / 1440.0;
        }
        if e.score > 0 {
            sum += u64::from(e.score);
            scored += 1;
        }
    }
    s.mean_score = (scored > 0).then(|| sum as f32 / scored as f32 / 10.0);
    s
}

/// How many titles share a genre or studio, and how they were scored.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Share {
    pub name: String,
    pub count: usize,
    /// Average of the titles scored, out of 10.
    pub mean_score: Option<f32>,
}

/// A closer look at one kind of list, beyond the counts by status.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Insights {
    /// Titles started, finished or left: everything but the plans.
    pub titles: usize,
    /// Episodes or chapters, rewatches included.
    pub units: u64,
    pub volumes: u64,
    /// Watch time, anime only.
    pub days: f32,
    /// Times a title was watched or read again.
    pub rewatches: u64,
    /// Titles by score, rounded down: `scores[0]` counts 1 to 1.9, `scores[9]` 10.
    pub scores: [usize; 10],
    pub mean_score: Option<f32>,
    pub median_score: Option<f32>,
    /// Completed among the titles completed or dropped.
    pub completion: Option<f32>,
    /// Most common first.
    pub genres: Vec<Share>,
    pub studios: Vec<Share>,
    pub formats: Vec<(Format, usize)>,
    /// Titles completed in each year, oldest first.
    pub finished_by_year: Vec<(i32, usize)>,
}

impl Insights {
    /// The best scored genre among those with at least `min` titles.
    pub fn favourite_genre(&self, min: usize) -> Option<&Share> {
        self.genres
            .iter()
            .filter(|g| g.mean_score.is_some() && g.count >= min)
            .max_by(|a, b| a.mean_score.partial_cmp(&b.mean_score).unwrap_or(std::cmp::Ordering::Equal))
    }
}

/// Everything in [`Insights`] for the titles of `kind`.
pub fn insights<'a>(entries: impl IntoIterator<Item = &'a ListEntry>, kind: MediaKind) -> Insights {
    let mut out = Insights::default();
    let mut scores: Vec<u8> = Vec::new();
    let (mut genres, mut studios) = (Tally::default(), Tally::default());
    let mut formats: HashMap<Format, usize> = HashMap::new();
    let mut years: HashMap<i32, usize> = HashMap::new();
    let (mut completed, mut dropped) = (0usize, 0usize);
    for e in entries.into_iter().filter(|e| e.media.kind == kind && e.status != ListStatus::Planning) {
        out.titles += 1;
        let total = u64::from(e.media.total().unwrap_or(0));
        let units = u64::from(e.progress) + u64::from(e.repeat_count).saturating_mul(total);
        out.units += units;
        out.volumes += u64::from(e.progress_volumes);
        out.rewatches += u64::from(e.repeat_count);
        if kind == MediaKind::Anime {
            out.days += units as f32 * e.media.duration.unwrap_or(0) as f32 / 1440.0;
        }
        if e.score > 0 {
            scores.push(e.score);
            out.scores[usize::from(e.score / 10).clamp(1, 10) - 1] += 1;
        }
        for g in &e.media.genres {
            genres.add(g, e.score);
        }
        for s in &e.media.studios {
            studios.add(s, e.score);
        }
        *formats.entry(e.media.format).or_default() += 1;
        match e.status {
            ListStatus::Completed => {
                completed += 1;
                if let Some(d) = e.finish {
                    *years.entry(d.year).or_default() += 1;
                }
            }
            ListStatus::Dropped => dropped += 1,
            _ => {}
        }
    }
    out.mean_score = mean(&scores);
    out.median_score = median(&mut scores);
    out.completion = (completed + dropped > 0).then(|| completed as f32 / (completed + dropped) as f32);
    out.genres = genres.sorted();
    out.studios = studios.sorted();
    out.formats = formats.into_iter().filter(|(f, _)| *f != Format::Unknown).collect();
    out.formats.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    out.finished_by_year = years.into_iter().collect();
    out.finished_by_year.sort_unstable();
    out
}

/// Counts and scores by name.
#[derive(Default)]
struct Tally(HashMap<String, (usize, Vec<u8>)>);

impl Tally {
    fn add(&mut self, name: &str, score: u8) {
        let (count, scores) = self.0.entry(name.to_owned()).or_default();
        *count += 1;
        if score > 0 {
            scores.push(score);
        }
    }

    fn sorted(self) -> Vec<Share> {
        let mut v: Vec<Share> = self
            .0
            .into_iter()
            .map(|(name, (count, scores))| Share { name, count, mean_score: mean(&scores) })
            .collect();
        v.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
        v
    }
}

/// The average of scores out of 100, out of 10.
fn mean(scores: &[u8]) -> Option<f32> {
    let sum: u64 = scores.iter().map(|s| u64::from(*s)).sum();
    (!scores.is_empty()).then(|| sum as f32 / scores.len() as f32 / 10.0)
}

/// The middle score out of 100, out of 10.
fn median(scores: &mut [u8]) -> Option<f32> {
    scores.sort_unstable();
    let n = scores.len();
    let at = |i: usize| f32::from(scores[i]);
    match n {
        0 => None,
        _ if n % 2 == 1 => Some(at(n / 2) / 10.0),
        _ => Some((at(n / 2 - 1) + at(n / 2)) / 20.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Format, FuzzyDate, Media};

    #[test]
    fn counts_rewatches_in_watch_time() {
        let mut e = ListEntry::new(Media { episodes: Some(12), duration: Some(24), ..Media::default() });
        e.status = ListStatus::Completed;
        e.progress = 12;
        e.repeat_count = 4;
        e.score = 80;
        let s = compute([&e], MediaKind::Anime);
        assert!((s.get(ListStatus::Completed).days - 1.0).abs() < 1e-4);
        assert_eq!(s.mean_score, Some(8.0));
        assert_eq!(compute([&e], MediaKind::Manga), Stats::default());
    }

    fn titled(status: ListStatus, score: u8, genres: &[&str], finish: Option<i32>) -> ListEntry {
        let media = Media {
            episodes: Some(10),
            duration: Some(24),
            genres: genres.iter().map(|g| (*g).to_owned()).collect(),
            studios: vec!["Studio".into()],
            format: Format::Tv,
            ..Media::default()
        };
        ListEntry {
            status,
            score,
            progress: if status == ListStatus::Completed { 10 } else { 2 },
            finish: finish.and_then(|y| FuzzyDate::new(y, Some(3), Some(1))),
            ..ListEntry::new(media)
        }
    }

    #[test]
    fn insights_look_past_the_plans() {
        use ListStatus as L;
        let list = [
            titled(L::Completed, 90, &["Drama", "Fantasy"], Some(2025)),
            titled(L::Completed, 70, &["Drama"], Some(2026)),
            titled(L::Completed, 0, &["Comedy"], Some(2026)),
            titled(L::Dropped, 30, &["Comedy"], None),
            titled(L::Current, 100, &["Fantasy"], None),
            titled(L::Planning, 50, &["Horror"], None),
        ];
        let s = insights(&list, MediaKind::Anime);
        assert_eq!(s.titles, 5);
        assert_eq!(s.units, 10 * 3 + 2 * 2);
        assert_eq!(s.scores, [0, 0, 1, 0, 0, 0, 1, 0, 1, 1]);
        assert_eq!(s.mean_score, Some(7.25));
        assert_eq!(s.median_score, Some(8.0));
        assert_eq!(s.completion, Some(0.75));
        let names: Vec<_> = s.genres.iter().map(|g| (g.name.as_str(), g.count)).collect();
        assert_eq!(names, [("Comedy", 2), ("Drama", 2), ("Fantasy", 2)]);
        assert!(s.genres.iter().all(|g| g.name != "Horror"), "plans are left out");
        assert_eq!(s.favourite_genre(2).map(|g| g.name.as_str()), Some("Fantasy"));
        assert_eq!(s.studios[0].count, 5);
        assert_eq!(s.formats, [(Format::Tv, 5)]);
        assert_eq!(s.finished_by_year, [(2025, 1), (2026, 2)]);
        assert_eq!(insights(&list, MediaKind::Manga), Insights::default());
    }

    #[test]
    fn medians_of_even_counts_meet_halfway() {
        assert_eq!(median(&mut [80, 60]), Some(7.0));
        assert_eq!(median(&mut []), None);
    }
}
