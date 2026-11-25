//! Filtering and sorting of the user's list.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::model::{Format, ListEntry, ListStatus, TitleLanguage};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SortKey {
    #[default]
    Updated,
    Title,
    MyScore,
    MeanScore,
    Progress,
    StartDate,
    NextEpisode,
    ReleaseDate,
}

impl SortKey {
    pub const ALL: [Self; 8] = [
        Self::Updated,
        Self::Title,
        Self::MyScore,
        Self::MeanScore,
        Self::Progress,
        Self::StartDate,
        Self::NextEpisode,
        Self::ReleaseDate,
    ];
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Filter {
    pub status: Option<ListStatus>,
    pub format: Option<Format>,
    pub genre: Option<String>,
    pub query: String,
    pub hide_adult: bool,
}

impl Filter {
    pub fn accepts(&self, e: &ListEntry) -> bool {
        self.status.is_none_or(|s| s == e.status)
            && self.format.is_none_or(|f| f == e.media.format)
            && self.genre.as_ref().is_none_or(|g| e.media.genres.contains(g))
            && !(self.hide_adult && e.media.adult)
            && e.media.matches(&self.query)
    }
}

/// `None` always sorts last, whichever direction is chosen.
fn opt<T: PartialOrd>(a: Option<T>, b: Option<T>, asc: bool) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => {
            let o = a.partial_cmp(&b).unwrap_or(Ordering::Equal);
            if asc { o } else { o.reverse() }
        }
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

pub fn sort(entries: &mut [&ListEntry], key: SortKey, asc: bool, lang: TitleLanguage) {
    let title = |e: &ListEntry| e.media.display_title(lang).to_lowercase();
    entries.sort_by(|a, b| {
        match key {
            SortKey::Updated => opt(Some(a.updated_at), Some(b.updated_at), asc),
            SortKey::Title => opt(Some(title(a)), Some(title(b)), asc),
            SortKey::MyScore => opt((a.score > 0).then_some(a.score), (b.score > 0).then_some(b.score), asc),
            SortKey::MeanScore => opt(a.media.mean_score, b.media.mean_score, asc),
            SortKey::Progress => opt(a.fraction(), b.fraction(), asc),
            SortKey::StartDate => opt(a.start, b.start, asc),
            SortKey::NextEpisode => {
                opt(a.media.next_airing.map(|n| n.at), b.media.next_airing.map(|n| n.at), asc)
            }
            SortKey::ReleaseDate => opt(a.media.start, b.media.start, asc),
        }
        .then_with(|| title(a).cmp(&title(b)))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Airing, Media};

    fn entry(title: &str, score: u8, airing: Option<i64>) -> ListEntry {
        ListEntry {
            score,
            ..ListEntry::new(Media {
                title: title.into(),
                next_airing: airing.map(|at| Airing { episode: 1, at }),
                ..Media::default()
            })
        }
    }
    fn titles(v: &[&ListEntry]) -> String {
        v.iter().map(|e| e.media.title.as_str()).collect()
    }

    #[test]
    fn missing_values_sort_last_in_both_directions() {
        let (a, b, c) = (entry("a", 0, None), entry("b", 90, Some(5)), entry("c", 10, Some(9)));
        let mut v = vec![&a, &b, &c];
        sort(&mut v, SortKey::NextEpisode, true, TitleLanguage::Romaji);
        assert_eq!(titles(&v), "bca");
        sort(&mut v, SortKey::NextEpisode, false, TitleLanguage::Romaji);
        assert_eq!(titles(&v), "cba");
        sort(&mut v, SortKey::MyScore, false, TitleLanguage::Romaji);
        assert_eq!(titles(&v), "bca");
    }

    #[test]
    fn filter_searches_every_title_and_hides_adult() {
        let mut e = entry("Sousou", 0, None);
        e.media.title_native = Some("葬送".into());
        e.media.adult = true;
        assert!(Filter { query: "葬".into(), ..Filter::default() }.accepts(&e));
        assert!(Filter { query: " SOUSOU ".into(), ..Filter::default() }.accepts(&e));
        assert!(!Filter { hide_adult: true, ..Filter::default() }.accepts(&e));
        assert!(!Filter { status: Some(ListStatus::Dropped), ..Filter::default() }.accepts(&e));
    }
}
