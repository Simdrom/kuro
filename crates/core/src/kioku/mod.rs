//! Kioku (記憶, "memory"): marks episodes as watched from the viewing history
//! of streaming platforms. Platform neutral and without network: the history
//! is read elsewhere and only matched, reviewed and remembered here.

pub mod item;
pub mod matcher;
pub mod review;

pub use item::{Platform, Watched, WatchedItem};
pub use matcher::{Match, THRESHOLD};
pub use review::{Doubt, Group, Outcome, Proposal, Skip};

/// The platform a downloaded history file seems to come from, by its name.
/// Only a hint: the file must still read as that platform's history.
pub fn export_platform(file_name: &str) -> Option<Platform> {
    let name = file_name.to_lowercase();
    if !(name.ends_with(".csv") || name.ends_with(".json")) {
        return None;
    }
    if name.contains("netflix") {
        Some(Platform::Netflix)
    } else if name.contains("prime") || name.contains("amazon") {
        Some(Platform::Amazon)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_files_are_told_apart_by_name() {
        assert_eq!(export_platform("NetflixViewingHistory.csv"), Some(Platform::Netflix));
        assert_eq!(export_platform("NetflixViewingHistory (1).csv"), Some(Platform::Netflix));
        assert_eq!(export_platform("prime-video-watch-history.json"), Some(Platform::Amazon));
        for other in ["NetflixViewingHistory.pdf", "holiday.csv", "amazon-order.txt"] {
            assert_eq!(export_platform(other), None, "{other}");
        }
    }
}
