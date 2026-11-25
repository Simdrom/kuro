//! Translations. English text is the key; `strings.txt` holds the rest.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct Language {
    pub code: &'static str,
    pub name: &'static str,
    /// Digit group separator and decimal mark.
    pub marks: (char, char),
    /// Short names, from January.
    pub months: &'static str,
    /// Short names, from Monday.
    pub days: &'static str,
}

/// Column order of `strings.txt`.
pub const LANGUAGES: [Language; 7] = [
    Language {
        code: "en",
        name: "English",
        marks: (',', '.'),
        months: "Jan Feb Mar Apr May Jun Jul Aug Sep Oct Nov Dec",
        days: "Mon Tue Wed Thu Fri Sat Sun",
    },
    Language {
        code: "es",
        name: "Español",
        marks: ('.', ','),
        months: "ene feb mar abr may jun jul ago sep oct nov dic",
        days: "lun mar mié jue vie sáb dom",
    },
    Language {
        code: "fr",
        name: "Français",
        marks: ('\u{202f}', ','),
        months: "janv. févr. mars avr. mai juin juil. août sept. oct. nov. déc.",
        days: "lun. mar. mer. jeu. ven. sam. dim.",
    },
    Language {
        code: "de",
        name: "Deutsch",
        marks: ('.', ','),
        months: "Jan. Feb. März Apr. Mai Juni Juli Aug. Sept. Okt. Nov. Dez.",
        days: "Mo. Di. Mi. Do. Fr. Sa. So.",
    },
    Language {
        code: "it",
        name: "Italiano",
        marks: ('.', ','),
        months: "gen feb mar apr mag giu lug ago set ott nov dic",
        days: "lun mar mer gio ven sab dom",
    },
    Language {
        code: "nb",
        name: "Norsk bokmål",
        marks: ('\u{a0}', ','),
        months: "jan. feb. mars apr. mai juni juli aug. sep. okt. nov. des.",
        days: "man. tir. ons. tor. fre. lør. søn.",
    },
    Language {
        code: "ko",
        name: "한국어",
        marks: (',', '.'),
        months: "1월 2월 3월 4월 5월 6월 7월 8월 9월 10월 11월 12월",
        days: "월 화 수 목 금 토 일",
    },
];

impl Language {
    pub fn month(&self, month: i8) -> &'static str {
        self.months.split(' ').nth(usize::try_from(month - 1).unwrap_or(0)).unwrap_or_default()
    }

    pub fn weekday(&self, day: jiff::civil::Weekday) -> &'static str {
        self.days.split(' ').nth(day.to_monday_zero_offset() as usize).unwrap_or_default()
    }

    /// `3 Oct 2025`, in the order the language uses.
    pub fn date(&self, d: jiff::civil::Date, with_day: bool) -> String {
        let (y, m) = (d.year(), self.month(d.month()));
        match (self.code, with_day) {
            ("ko", true) => format!("{y}년 {m} {}일", d.day()),
            ("ko", false) => format!("{y}년 {m}"),
            (_, true) => format!("{} {m} {y}", d.day()),
            (_, false) => format!("{m} {y}"),
        }
    }
}

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
    fn every_language_names_all_months_and_days() {
        for l in &LANGUAGES {
            assert_eq!((l.months.split(' ').count(), l.days.split(' ').count()), (12, 7), "{}", l.code);
        }
        assert_eq!(LANGUAGES[1].date(jiff::civil::date(2025, 10, 3), true), "3 oct 2025");
        assert_eq!(LANGUAGES[6].date(jiff::civil::date(2025, 10, 3), true), "2025년 10월 3일");
    }

    #[test]
    fn placeholders() {
        assert_eq!(tf("{} of {}", &[&1, &"x"]), "1 of x");
        assert_eq!(tf("{} of {}", &[&1]), "1 of ");
    }
}
