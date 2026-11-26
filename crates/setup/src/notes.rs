//! What changed in each version, from `whats-new.txt`, built into the setup. An update lists
//! the versions after the installed one, up to this one.

use std::cmp::Ordering;

use crate::engine::compare;

const NOTES: &str = include_str!("../whats-new.txt");

/// A version and its changes, in one language.
pub type Release = (&'static str, Vec<&'static str>);

/// The changes after `from` up to `to`, newest first, in the setup's language.
pub fn between(from: &str, to: &str) -> Vec<Release> {
    select(parse(NOTES, crate::text::column()), from, to)
}

fn select(mut all: Vec<Release>, from: &str, to: &str) -> Vec<Release> {
    all.retain(|(v, lines)| {
        !lines.is_empty() && compare(v, from) == Ordering::Greater && compare(v, to) != Ordering::Greater
    });
    all.sort_by(|a, b| compare(b.0, a.0));
    all
}

/// "[x.y.z]" starts a version; each line after it is one change, its languages split by `|`.
fn parse(src: &'static str, column: usize) -> Vec<Release> {
    let mut all: Vec<Release> = Vec::new();
    for line in src.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        if let Some(v) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            all.push((v.trim(), Vec::new()));
        } else if let Some((_, lines)) = all.last_mut() {
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            let text = cells.get(column).filter(|c| !c.is_empty()).unwrap_or(&cells[0]);
            lines.push(text);
        }
    }
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str =
        "# comment\n[0.3.0]\nThree|Tres\n[0.2.0]\nTwo a|Dos a\nTwo b\n\n[0.1.1]\n[0.1.0]\nOne|Uno\n";

    fn versions(r: &[Release]) -> Vec<&str> {
        r.iter().map(|(v, _)| *v).collect()
    }

    #[test]
    fn only_versions_after_the_installed_one_up_to_this_one() {
        let all = parse(SAMPLE, 0);
        assert_eq!(versions(&select(all.clone(), "0.1.0", "0.2.0")), ["0.2.0"]);
        assert_eq!(versions(&select(all.clone(), "0.1.0", "0.3.0")), ["0.3.0", "0.2.0"]);
        assert_eq!(versions(&select(all.clone(), "0.0.9", "0.1.1")), ["0.1.0"]);
        assert!(select(all.clone(), "0.3.0", "0.3.0").is_empty());
        assert!(select(all, "0.2.0", "0.2.5").is_empty());
    }

    #[test]
    fn a_missing_translation_falls_back_to_english() {
        let all = parse(SAMPLE, 1);
        assert_eq!(all[1], ("0.2.0", vec!["Dos a", "Two b"]));
    }

    #[test]
    fn the_notes_are_complete_and_newest_first() {
        let all = parse(NOTES, 0);
        assert!(!all.is_empty());
        for w in all.windows(2) {
            assert_eq!(compare(w[0].0, w[1].0), Ordering::Greater, "{} before {}", w[0].0, w[1].0);
        }
        for line in NOTES.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
                continue;
            }
            let cells: Vec<_> = line.split('|').map(str::trim).collect();
            assert_eq!(cells.len(), 7, "{line}");
            assert!(cells.iter().all(|c| !c.is_empty()), "{line}");
        }
    }
}
