//! Netflix: not written yet.
//!
//! Netflix has no API for this, but lets every profile download its viewing
//! activity as a CSV file (Account › Profile › Viewing activity › Download
//! all). The fixture assumes its usual shape, a `Title,Date` header and titles
//! like `Series: Season 1: Episode title`; that shape and the date format,
//! which follows the account's language, still have to be checked against a
//! real download before the parser is written.

use kuro_core::kioku::Platform;

use super::{HistorySource, Mode};

pub struct Netflix;

impl HistorySource for Netflix {
    fn platform(&self) -> Platform {
        Platform::Netflix
    }

    fn mode(&self) -> Mode {
        Mode::File
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ApiError;

    const FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/kioku/netflix.csv");

    #[test]
    fn not_available_yet() {
        assert_eq!(Netflix.parse(FIXTURE), Err(ApiError::Unavailable));
    }

    /// What the parser has to give for the invented fixture.
    #[test]
    #[ignore = "the Netflix parser is not written yet"]
    fn parses_viewing_activity() {
        let items = Netflix.parse(FIXTURE).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].series_title, "Lanterns Over Saltmarsh");
        assert_eq!((items[0].season, items[0].episode_title.as_deref()), (Some(2), Some("Low Tide")));
        assert!(items[2].movie);
    }
}
