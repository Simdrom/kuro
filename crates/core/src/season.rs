use jiff::civil::Date;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Season {
    Winter,
    Spring,
    Summer,
    Fall,
}

impl Season {
    pub const ALL: [Self; 4] = [Self::Winter, Self::Spring, Self::Summer, Self::Fall];

    pub fn of_month(month: u32) -> Self {
        match month {
            1..=3 => Self::Winter,
            4..=6 => Self::Spring,
            7..=9 => Self::Summer,
            _ => Self::Fall,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Winter => "winter",
            Self::Spring => "spring",
            Self::Summer => "summer",
            Self::Fall => "fall",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SeasonYear {
    pub season: Season,
    pub year: i32,
}

impl SeasonYear {
    pub fn of_date(d: Date) -> Self {
        Self { season: Season::of_month(d.month() as u32), year: d.year().into() }
    }

    pub fn next(self) -> Self {
        match self.season {
            Season::Winter => Self { season: Season::Spring, ..self },
            Season::Spring => Self { season: Season::Summer, ..self },
            Season::Summer => Self { season: Season::Fall, ..self },
            Season::Fall => Self { season: Season::Winter, year: self.year + 1 },
        }
    }

    pub fn prev(self) -> Self {
        match self.season {
            Season::Winter => Self { season: Season::Fall, year: self.year - 1 },
            Season::Spring => Self { season: Season::Winter, ..self },
            Season::Summer => Self { season: Season::Spring, ..self },
            Season::Fall => Self { season: Season::Summer, ..self },
        }
    }
}

/// Years offered by the picker, newest first. Next year appears from September.
pub fn years(today: Date) -> Vec<i32> {
    let last = i32::from(today.year()) + i32::from(today.month() >= 9);
    (1960..=last).rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_and_prev_are_inverse_across_years() {
        let s = SeasonYear { season: Season::Fall, year: 2026 };
        assert_eq!(s.next(), SeasonYear { season: Season::Winter, year: 2027 });
        let mut c = s;
        for _ in 0..9 {
            assert_eq!(c.next().prev(), c);
            c = c.next();
        }
    }

    #[test]
    fn upcoming_year_listed_from_september() {
        let aug = jiff::civil::date(2026, 8, 31);
        let sep = jiff::civil::date(2026, 9, 1);
        assert_eq!(years(aug)[0], 2026);
        assert_eq!(years(sep)[0], 2027);
    }
}
