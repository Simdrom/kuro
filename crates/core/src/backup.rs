//! Automatic backups: their names, when one is due and which old ones go.

use jiff::civil::Date;

use crate::settings::BackupEvery;

const PREFIX: &str = "kuro-backup-";
const SUFFIX: &str = ".json";

/// `kuro-backup-2026-11-26.json`
pub fn file_name(d: Date) -> String {
    format!("{PREFIX}{d}{SUFFIX}")
}

/// The date of an automatic backup, from its file name. Other files give `None`.
pub fn date_of(name: &str) -> Option<Date> {
    name.strip_prefix(PREFIX)?.strip_suffix(SUFFIX)?.parse().ok()
}

/// The newest automatic backup among `names`.
pub fn latest<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<Date> {
    names.into_iter().filter_map(date_of).max()
}

/// Whether a new backup is due today.
pub fn due<'a>(names: impl IntoIterator<Item = &'a str>, today: Date, every: BackupEvery) -> bool {
    let days = match every {
        BackupEvery::Off => return false,
        BackupEvery::Daily => 1,
        BackupEvery::Weekly => 7,
    };
    latest(names).is_none_or(|last| last.until(today).is_ok_and(|s| s.get_days() >= days))
}

/// Automatic backups beyond the newest `keep`, oldest first. Other files are never listed.
pub fn surplus<'a>(names: impl IntoIterator<Item = &'a str>, keep: u32) -> Vec<String> {
    let mut ours: Vec<(Date, &str)> = names.into_iter().filter_map(|n| Some((date_of(n)?, n))).collect();
    ours.sort();
    let extra = ours.len().saturating_sub(keep.max(1) as usize);
    ours.into_iter().take(extra).map(|(_, n)| n.to_owned()).collect()
}

#[cfg(test)]
mod tests {
    use jiff::civil::date;

    use super::*;

    #[test]
    fn names_round_trip_and_other_files_are_ignored() {
        let d = date(2026, 11, 26);
        assert_eq!(file_name(d), "kuro-backup-2026-11-26.json");
        assert_eq!(date_of(&file_name(d)), Some(d));
        for other in
            ["kuro-backup.json", "kuro-backup-2026-13-01.json", "notes.json", "kuro-backup-2026-11-26.txt"]
        {
            assert_eq!(date_of(other), None, "{other}");
        }
    }

    #[test]
    fn a_backup_is_due_after_its_interval() {
        let today = date(2026, 11, 26);
        let yesterday = file_name(date(2026, 11, 25));
        let this_morning = file_name(today);
        assert!(due([], today, BackupEvery::Daily));
        assert!(!due([], today, BackupEvery::Off));
        assert!(due([yesterday.as_str()], today, BackupEvery::Daily));
        assert!(!due([this_morning.as_str(), yesterday.as_str()], today, BackupEvery::Daily));
        assert!(!due([yesterday.as_str()], today, BackupEvery::Weekly));
        let last_week = file_name(date(2026, 11, 19));
        assert!(due([last_week.as_str(), "notes.json"], today, BackupEvery::Weekly));
    }

    #[test]
    fn only_the_oldest_of_our_own_files_go() {
        let names = [
            "kuro-backup-2026-11-24.json",
            "notes.json",
            "kuro-backup-2026-11-20.json",
            "kuro-backup-2026-11-26.json",
        ];
        assert_eq!(surplus(names, 2), ["kuro-backup-2026-11-20.json"]);
        assert_eq!(surplus(names, 0), ["kuro-backup-2026-11-20.json", "kuro-backup-2026-11-24.json"]);
        assert!(surplus(names, 10).is_empty());
    }
}
