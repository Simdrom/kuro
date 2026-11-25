//! Groups every title of a franchise into ordered sections.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::model::{Format, Media, MediaKind, MediaRef, Relation, RelationKind};

/// Upper bound on the titles visited, so a huge franchise cannot flood the API.
pub const MAX_TITLES: usize = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Section {
    Main,
    Movies,
    SideStories,
    Other,
}

impl Section {
    pub const ALL: [Self; 4] = [Self::Main, Self::Movies, Self::SideStories, Self::Other];
}

/// Breadth-first walk state. The caller fetches relations for each `next_batch`.
#[derive(Debug, Clone, Default)]
pub struct Walk {
    seen: HashSet<MediaRef>,
    queue: Vec<MediaRef>,
    found: Vec<(Section, Media)>,
}

fn follows(kind: RelationKind) -> bool {
    matches!(
        kind,
        RelationKind::Sequel
            | RelationKind::Prequel
            | RelationKind::Parent
            | RelationKind::SideStory
            | RelationKind::SpinOff
            | RelationKind::Summary
            | RelationKind::Alternative
            | RelationKind::Compilation
            | RelationKind::Contains
    )
}

fn section(media: &Media, via: RelationKind, from: Section) -> Section {
    match via {
        RelationKind::Summary | RelationKind::Alternative | RelationKind::Compilation => Section::Other,
        _ if media.format == Format::Movie => Section::Movies,
        RelationKind::SideStory | RelationKind::SpinOff => Section::SideStories,
        // The parent of a side story is the main line again.
        RelationKind::Parent => Section::Main,
        _ if matches!(media.format, Format::Special | Format::Ova | Format::Music) => Section::SideStories,
        // A movie's sequel may be a season.
        _ if from == Section::Movies => Section::Main,
        _ => from,
    }
}

impl Walk {
    pub fn start(root: Media) -> Self {
        let r = root.media_ref();
        let s = match root.format {
            Format::Movie => Section::Movies,
            Format::Special | Format::Ova | Format::Music => Section::SideStories,
            _ => Section::Main,
        };
        Self { seen: HashSet::from([r]), queue: vec![r], found: vec![(s, root)] }
    }

    /// Titles whose relations are still needed.
    pub fn next_batch(&mut self, size: usize) -> Vec<MediaRef> {
        let n = size.min(self.queue.len());
        self.queue.drain(..n).collect()
    }

    pub fn add(&mut self, of: MediaRef, relations: Vec<Relation>) {
        let Some(from) = self.found.iter().find(|(_, m)| m.media_ref() == of).map(|(s, _)| *s) else {
            return;
        };
        for r in relations {
            if r.media.kind != of.kind || !follows(r.kind) || self.found.len() >= MAX_TITLES {
                continue;
            }
            // Light novels and manga share a kind but are different works.
            if of.kind == MediaKind::Manga && r.kind == RelationKind::Alternative {
                continue;
            }
            let id = r.media.media_ref();
            if self.seen.insert(id) {
                let s = section(&r.media, r.kind, from);
                self.queue.push(id);
                self.found.push((s, r.media));
            }
        }
    }

    /// Sections in display order, each sorted by release date.
    pub fn finish(mut self) -> Vec<(Section, Vec<Media>)> {
        self.found.sort_by_key(|(_, m)| (m.start.is_none(), m.start, m.id));
        Section::ALL
            .into_iter()
            .filter_map(|s| {
                let v: Vec<_> = self.found.iter().filter(|(x, _)| *x == s).map(|(_, m)| m.clone()).collect();
                (!v.is_empty()).then_some((s, v))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::FuzzyDate;

    fn m(id: u64, format: Format, year: i32) -> Media {
        Media { id, format, start: FuzzyDate::new(year, Some(1), None), ..Media::default() }
    }
    fn rel(kind: RelationKind, media: Media) -> Relation {
        Relation { kind, media }
    }

    #[test]
    fn walks_cycles_once_and_sorts_by_date() {
        let (a, b, c) = (m(1, Format::Tv, 2013), m(2, Format::Tv, 2017), m(3, Format::Tv, 2019));
        let mut w = Walk::start(b.clone());
        while let Some(r) = w.next_batch(1).pop() {
            let rels = match r.id {
                1 => vec![rel(RelationKind::Sequel, b.clone())],
                2 => vec![
                    rel(RelationKind::Sequel, c.clone()),
                    rel(RelationKind::Prequel, a.clone()),
                    rel(RelationKind::SideStory, m(4, Format::Ova, 2014)),
                    rel(RelationKind::SideStory, m(5, Format::Movie, 2015)),
                    rel(RelationKind::Character, m(6, Format::Tv, 2000)),
                ],
                _ => vec![rel(RelationKind::Prequel, b.clone())],
            };
            w.add(r, rels);
        }
        let out = w.finish();
        let ids = |s: Section| -> Vec<u64> {
            out.iter().find(|x| x.0 == s).map(|x| x.1.iter().map(|m| m.id).collect()).unwrap_or_default()
        };
        assert_eq!(ids(Section::Main), [1, 2, 3]);
        assert_eq!(ids(Section::SideStories), [4]);
        assert_eq!(ids(Section::Movies), [5]);
    }

    #[test]
    fn is_bounded() {
        let mut w = Walk::start(m(0, Format::Tv, 2000));
        let many = (1..500).map(|i| rel(RelationKind::Sequel, m(i, Format::Tv, 2000))).collect();
        let r = w.next_batch(1)[0];
        w.add(r, many);
        assert_eq!(w.finish()[0].1.len(), MAX_TITLES);
    }
}
