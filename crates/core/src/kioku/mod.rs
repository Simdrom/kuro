//! Kioku (記憶, "memory"): marks episodes as watched from the viewing history
//! of streaming platforms. Platform neutral and without network: the history
//! is read elsewhere and only matched, reviewed and remembered here.

pub mod item;
pub mod matcher;
pub mod review;

pub use item::{Platform, Watched, WatchedItem};
pub use matcher::{Match, THRESHOLD};
pub use review::{Doubt, Group, Outcome, Proposal, Skip};
