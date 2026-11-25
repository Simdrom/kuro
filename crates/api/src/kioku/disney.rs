//! Disney+: not written yet.
//!
//! Disney+ offers no viewing history download in its apps. The way in is a
//! personal data request to Disney, whose answer is assumed to include the
//! viewing history. Neither the request process nor the files it returns have
//! been checked yet: the fixture is invented, only to fix what the parser
//! should give.

use kuro_core::kioku::Platform;

use super::{HistorySource, Mode};

pub struct DisneyPlus;

impl HistorySource for DisneyPlus {
    fn platform(&self) -> Platform {
        Platform::DisneyPlus
    }

    fn mode(&self) -> Mode {
        Mode::File
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ApiError;

    const FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/kioku/disney.json");

    #[test]
    fn not_available_yet() {
        assert_eq!(DisneyPlus.parse(FIXTURE), Err(ApiError::Unavailable));
    }

    #[test]
    #[ignore = "the Disney+ parser is not written yet"]
    fn parses_the_data_export() {
        let items = DisneyPlus.parse(FIXTURE).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!((items[0].season, items[0].episode), (Some(1), Some(4)));
    }
}
