use crate::model::{ListEntry, ListStatus, MediaKind};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Media;

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
}
