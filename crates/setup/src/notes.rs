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

    /// What is wrong with `src` as the notes of a build of `version`, as advice.
    fn check(src: &str, version: &str) -> Vec<String> {
        const LANGS: &str = "en|es|fr|de|it|nb|ko";
        let mut problems = Vec::new();
        let mut seen: Vec<&str> = Vec::new();
        for (n, line) in src.lines().map(str::trim).enumerate().map(|(i, l)| (i + 1, l)) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(v) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                let v = v.trim();
                if seen.is_empty() && v != version {
                    problems.push(format!(
                        "line {n}: the first version is [{v}], but this is {version}. Add [{version}] above it, with what changed for users."
                    ));
                }
                if let Some(prev) = seen.last()
                    && compare(prev, v) != Ordering::Greater
                {
                    problems.push(format!(
                        "line {n}: [{v}] comes after [{prev}]; versions go newest first, once each."
                    ));
                }
                seen.push(v);
                continue;
            }
            if seen.is_empty() {
                problems.push(format!("line {n}: a change before any [version] header."));
            }
            let cells: Vec<_> = line.split('|').map(str::trim).collect();
            if cells.len() != 7 || cells.iter().any(|c| c.is_empty()) {
                problems.push(format!(
                    "line {n}: {} of 7 languages ({LANGS}); write every one, split by |: {line}",
                    cells.iter().filter(|c| !c.is_empty()).count()
                ));
            }
        }
        if seen.is_empty() {
            problems.push(format!("No versions: add [{version}] with what changed for users."));
        }
        problems
    }

    #[test]
    fn the_notes_are_complete_and_newest_first() {
        // An empty section for the version being built is fine: the release asks for lines.
        let problems = check(NOTES, env!("CARGO_PKG_VERSION"));
        assert!(problems.is_empty(), "crates/setup/whats-new.txt:\n{}", problems.join("\n"));
    }

    #[test]
    fn the_check_finds_what_is_wrong() {
        const FULL: &str = "a|b|c|d|e|f|g";
        assert!(check(&format!("[0.3.0]\n[0.2.0]\n{FULL}\n"), "0.3.0").is_empty());
        let wrong = [
            (format!("[0.2.0]\n{FULL}\n"), "Add [0.3.0] above it"),
            (format!("{FULL}\n[0.3.0]\n"), "before any [version]"),
            (format!("[0.3.0]\n[0.3.0]\n{FULL}\n"), "newest first"),
            ("[0.3.0]\n[0.1.0]\n[0.2.0]\n".to_owned(), "newest first"),
            ("[0.3.0]\na|b|c\n".to_owned(), "3 of 7"),
            ("[0.3.0]\na|b|c|d||f|g\n".to_owned(), "6 of 7"),
            ("# nothing\n".to_owned(), "No versions"),
        ];
        for (src, says) in wrong {
            let problems = check(&src, "0.3.0").join("\n");
            assert!(problems.contains(says), "{src:?}: {problems}");
        }
    }
}
