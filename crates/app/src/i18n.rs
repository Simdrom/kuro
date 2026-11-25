//! Translations. English text is the key; `strings.txt` holds the rest.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::Locale;

pub struct Language {
    pub code: &'static str,
    pub name: &'static str,
    pub locale: Locale,
    /// Digit group separator and decimal mark.
    pub marks: (char, char),
}

/// Column order of `strings.txt`.
pub const LANGUAGES: [Language; 7] = [
    Language { code: "en", name: "English", locale: Locale::en_US, marks: (',', '.') },
    Language { code: "es", name: "Español", locale: Locale::es_ES, marks: ('.', ',') },
    Language { code: "fr", name: "Français", locale: Locale::fr_FR, marks: ('\u{202f}', ',') },
    Language { code: "de", name: "Deutsch", locale: Locale::de_DE, marks: ('.', ',') },
    Language { code: "it", name: "Italiano", locale: Locale::it_IT, marks: ('.', ',') },
    Language { code: "nb", name: "Norsk bokmål", locale: Locale::nb_NO, marks: ('\u{a0}', ',') },
    Language { code: "ko", name: "한국어", locale: Locale::ko_KR, marks: (',', '.') },
];

static CURRENT: AtomicUsize = AtomicUsize::new(0);

type Table = HashMap<&'static str, Vec<&'static str>>;

fn table() -> &'static Table {
    static TABLE: OnceLock<Table> = OnceLock::new();
    TABLE.get_or_init(|| parse(include_str!("strings.txt")))
}

fn parse(text: &'static str) -> Table {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| {
            let mut cols = l.split('|');
            Some((cols.next()?, cols.collect()))
        })
        .collect()
}

/// Selects a language; an empty or unknown code follows the system.
pub fn set(code: &str) {
    let find = |c: &str| LANGUAGES.iter().position(|l| l.code == c);
    let system = || {
        let l = sys_locale::get_locale().unwrap_or_default().to_lowercase();
        let l = l.split(['-', '_']).next().unwrap_or_default().to_owned();
        find(if l == "no" || l == "nn" { "nb" } else { &l })
    };
    CURRENT.store(find(code).or_else(system).unwrap_or(0), Ordering::Relaxed);
}

pub fn current() -> &'static Language {
    &LANGUAGES[CURRENT.load(Ordering::Relaxed)]
}

pub fn t(key: &'static str) -> &'static str {
    let i = CURRENT.load(Ordering::Relaxed);
    if i == 0 {
        return key;
    }
    table().get(key).and_then(|c| c.get(i - 1)).copied().filter(|s| !s.is_empty()).unwrap_or(key)
}

/// Translates and replaces `{}` placeholders in order.
pub fn tf(key: &'static str, args: &[&dyn std::fmt::Display]) -> String {
    let mut out = String::new();
    let mut parts = t(key).split("{}");
    out.push_str(parts.next().unwrap_or_default());
    for (i, part) in parts.enumerate() {
        if let Some(a) = args.get(i) {
            out.push_str(&a.to_string());
        }
        out.push_str(part);
    }
    out
}

pub fn number(n: u64) -> String {
    let digits = n.to_string();
    let sep = current().marks.0;
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(sep);
        }
        out.push(c);
    }
    out
}

pub fn decimal(v: f32, places: usize) -> String {
    format!("{v:.places$}").replace('.', &current().marks.1.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `t("…")` / `tf("…")` / `tr!("…")` key in the sources.
    fn used_keys() -> Vec<String> {
        fn walk(dir: &std::path::Path, out: &mut Vec<String>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().is_some_and(|x| x == "rs") && !p.ends_with("i18n.rs") {
                    let src = std::fs::read_to_string(&p).unwrap();
                    for (i, _) in src.match_indices("t(").chain(src.match_indices("tf(")) {
                        let before = src[..i].chars().next_back().unwrap_or(' ');
                        let rest = src[i..].split_once('(').unwrap().1.trim_start();
                        if !before.is_alphanumeric() && before != '_' && rest.starts_with('"') {
                            out.push(rest[1..].split('"').next().unwrap().to_owned());
                        }
                    }
                }
            }
        }
        let mut out = Vec::new();
        walk(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut out);
        out.sort();
        out.dedup();
        out
    }

    #[test]
    fn every_string_is_translated_in_every_language() {
        let keys = used_keys();
        assert!(keys.len() > 50);
        let missing: Vec<_> = keys.iter().filter(|k| !table().contains_key(k.as_str())).collect();
        assert!(missing.is_empty(), "missing from strings.txt: {missing:#?}");
        for (k, cols) in table() {
            assert_eq!(cols.len(), LANGUAGES.len() - 1, "columns of {k:?}");
            for (c, l) in cols.iter().zip(&LANGUAGES[1..]) {
                assert!(!c.trim().is_empty(), "{k:?} is empty in {}", l.code);
                assert_eq!(c.matches("{}").count(), k.matches("{}").count(), "{k:?} in {}", l.code);
            }
        }
    }

    #[test]
    fn numbers_follow_the_language() {
        assert_eq!(number(0), "0");
        assert_eq!(number(1_131_247), "1,131,247");
        assert_eq!(decimal(9.3, 2), "9.30");
    }

    #[test]
    fn placeholders() {
        assert_eq!(tf("{} of {}", &[&1, &"x"]), "1 of x");
        assert_eq!(tf("{} of {}", &[&1]), "1 of ");
    }
}
