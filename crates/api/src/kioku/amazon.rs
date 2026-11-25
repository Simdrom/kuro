//! Prime Video: not written yet.
//!
//! Amazon lets customers request their personal data, and that export is
//! assumed to include the Prime Video viewing history as a CSV file. Its name
//! and columns have not been checked yet: the fixture is invented, only to fix
//! what the parser should give.

use kuro_core::kioku::Platform;

use super::{HistorySource, Mode};

pub struct Amazon;

impl HistorySource for Amazon {
    fn platform(&self) -> Platform {
        Platform::Amazon
    }

    fn mode(&self) -> Mode {
        Mode::File
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ApiError;

    const FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/kioku/amazon.csv");

    #[test]
    fn not_available_yet() {
        assert_eq!(Amazon.parse(FIXTURE), Err(ApiError::Unavailable));
    }

    #[test]
    #[ignore = "the Prime Video parser is not written yet"]
    fn parses_the_data_export() {
        let items = Amazon.parse(FIXTURE).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].series_title, "Iron Orchard");
    }
}
