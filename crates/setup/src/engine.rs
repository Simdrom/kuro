//! What is installed, and installing or removing it, for the current user only. The window
//! and silent mode (/S) both drive it. Every step goes to the log, also kept in
//! %TEMP%\Kuro-Setup.log.

use std::cmp::Ordering;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::text::{t, tf};
use crate::win;

pub const EXE: &str = "kuro.exe";
const SETUP: &str = "setup.exe";
const LICENCES: &str = "THIRD-PARTY-LICENSES.html";
const SHORTCUT: &str = "Kuro.lnk";
/// The id the first installer used, kept so Windows lists one Kuro, not two.
const APP_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{430942B2-A6DE-46B7-96A7-DDF405974F13}_is1";
// Written by Kuro itself; only cleaned up here.
const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const LINKS: &str = r"Software\Classes\kuro";
const TOAST_ID: &str = r"Software\Classes\AppUserModelId\Kuro.Tracker";
/// Left behind by the first installer.
const OLD_UNINSTALLER: [&str; 2] = ["unins000.exe", "unins000.dat"];

static PROGRAM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/kuro.exe.z"));
static LICENCES_Z: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/THIRD-PARTY-LICENSES.html.z"));

/// The version this setup installs. Test builds pretend to be another one.
pub fn version() -> &'static str {
    option_env!("KURO_SETUP_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
}

/// The newest database layout the Kuro inside can open.
pub fn schema() -> i64 {
    option_env!("KURO_SETUP_SCHEMA").and_then(|s| s.parse().ok()).unwrap_or(kuro_core::store::SCHEMA_VERSION)
}

/// Compares X.Y.Z versions by number. One that cannot be read counts as older.
pub fn compare(a: &str, b: &str) -> Ordering {
    let parse = |v: &str| v.split('.').map(str::parse::<u64>).collect::<Result<Vec<_>, _>>().ok();
    match (parse(a), parse(b)) {
        (Some(a), Some(b)) => a.cmp(&b),
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

// ---- log ----------------------------------------------------------------------------------------

#[derive(Default)]
pub struct Log(Mutex<Vec<String>>);

impl Log {
    pub fn add(&self, line: impl Into<String>) {
        let line = line.into();
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(std::env::temp_dir().join("Kuro-Setup.log"))
        {
            let _ = writeln!(f, "{line}");
        }
        self.0.lock().unwrap_or_else(|e| e.into_inner()).push(line);
    }

    pub fn lines(&self) -> Vec<String> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

// ---- what is installed --------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Installed {
    pub version: String,
    pub dir: PathBuf,
}

#[derive(Clone, Debug, Default)]
pub struct State {
    pub installed: Option<Installed>,
    /// Where the running copies of Kuro were started from, this one or any other.
    pub running: Vec<PathBuf>,
    /// Layout of the lists on this computer, if there are any.
    pub schema: Option<i64>,
    /// There is a desktop shortcut.
    pub desktop: bool,
}

pub fn read_state() -> State {
    let key = windows_registry::CURRENT_USER.open(APP_KEY).ok();
    let value = |name| key.as_ref().and_then(|k| k.get_string(name).ok());
    let installed = match (value("DisplayVersion"), value("InstallLocation")) {
        // The folder may have been deleted by hand.
        (Some(version), Some(dir)) if Path::new(&dir).join(EXE).is_file() => {
            Some(Installed { version, dir: PathBuf::from(dir) })
        }
        _ => None,
    };
    let running = win::running(EXE);
    // Unreadable lists are treated as too new: an older Kuro must not risk them.
    let schema = data_dirs().map_or(Ok(None), |(data, _)| kuro_core::Store::schema_of(&data.join("kuro.db")));
    let schema = schema.unwrap_or(Some(i64::MAX));
    let desktop = win::desktop().is_some_and(|d| d.join(SHORTCUT).is_file());
    State { installed, running, schema, desktop }
}

/// What running this setup means for what is installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Plan {
    Install,
    Update(String),
    Reinstall,
    /// Back to this older version.
    Downgrade(String),
    /// Installed is newer, and its lists are too new for this version.
    TooNew(String),
}

pub fn plan(st: &State) -> Plan {
    let Some(i) = &st.installed else { return Plan::Install };
    match compare(&i.version, version()) {
        Ordering::Less => Plan::Update(i.version.clone()),
        Ordering::Equal => Plan::Reinstall,
        Ordering::Greater if st.schema.is_some_and(|s| s > schema()) => Plan::TooNew(i.version.clone()),
        Ordering::Greater => Plan::Downgrade(i.version.clone()),
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Choices {
    pub desktop: bool,
    pub launch: bool,
}

// ---- helpers ------------------------------------------------------------------------------------

/// %LOCALAPPDATA%\Kuro\data and \cache, as Kuro finds them.
fn data_dirs() -> Option<(PathBuf, PathBuf)> {
    let d = directories::ProjectDirs::from("", "", "Kuro")?;
    Some((d.data_local_dir().to_owned(), d.cache_dir().to_owned()))
}

fn default_dir() -> Result<PathBuf, String> {
    let base = std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA is not set")?;
    Ok(Path::new(&base).join("Programs").join("Kuro"))
}

fn io(what: &Path) -> impl Fn(std::io::Error) -> String + '_ {
    move |e| tf("Could not write {0}: {1}", &[&what.display().to_string(), &e.to_string()])
}

fn unpack(packed: &[u8]) -> Result<Vec<u8>, String> {
    miniz_oxide::inflate::decompress_to_vec_zlib(packed).map_err(|e| format!("damaged setup: {e:?}"))
}

/// Asks the running Kuro to quit, as `kuro.exe --quit` does: only one runs per user, and it
/// listens on this pipe (system.rs in the app). Kuro 0.1.0 does not know the word.
fn ask_to_quit() {
    let user: String =
        std::env::var("USERNAME").unwrap_or_default().chars().filter(char::is_ascii_alphanumeric).collect();
    if let Ok(mut pipe) = std::fs::OpenOptions::new().write(true).open(format!(r"\\.\pipe\kuro-{user}")) {
        let _ = writeln!(pipe, "quit");
    }
}

/// Closes Kuro and waits until it is gone. `only` limits it to the copy in that folder; otherwise
/// any copy counts, since a copy left open would take over the one started after installing.
fn close_kuro(only: Option<&Path>, log: &Log) -> Result<(), String> {
    let open = || -> Vec<PathBuf> {
        win::running(EXE)
            .into_iter()
            .filter(|p| only.is_none_or(|d| p.parent().is_some_and(|q| same_file(q, d))))
            .collect()
    };
    let mut left = open();
    if left.is_empty() {
        return Ok(());
    }
    for p in &left {
        log.add(format!("Closing {}", p.display()));
    }
    let end = Instant::now() + Duration::from_secs(10);
    while !left.is_empty() {
        if Instant::now() >= end {
            return Err(tf(
                "Kuro is still open from {0}. Close it and try again.",
                &[&left[0].display().to_string()],
            ));
        }
        ask_to_quit();
        std::thread::sleep(Duration::from_millis(300));
        left = open();
    }
    Ok(())
}

/// Writes next to the old file and checks it before replacing it, so a failed write never
/// leaves a broken program.
fn replace(path: &Path, bytes: &[u8], log: &Log) -> Result<(), String> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let temp = path.with_file_name(format!("{name}.new"));
    std::fs::write(&temp, bytes).map_err(io(&temp))?;
    if std::fs::read(&temp).map_err(io(&temp))? != bytes {
        let _ = std::fs::remove_file(&temp);
        return Err(tf("Could not write {0}: {1}", &[&temp.display().to_string(), "the copy differs"]));
    }
    std::fs::rename(&temp, path).map_err(io(path))?;
    log.add(format!("Wrote {}", path.display()));
    Ok(())
}

/// A program that has just closed may keep its file locked a moment longer, or an antivirus may
/// be reading it, so a denied removal is tried again for a few seconds.
fn remove(path: &Path, log: &Log) -> Result<(), String> {
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        match std::fs::remove_file(path) {
            Ok(()) => {
                log.add(format!("Removed {}", path.display()));
                return Ok(());
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied && Instant::now() < end => {
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => {
                return Err(tf("Could not remove {0}: {1}", &[&path.display().to_string(), &e.to_string()]));
            }
        }
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

/// Does this command start the Kuro in `exe`? Another copy may own the entry.
pub fn points_to(command: &str, exe: &Path) -> bool {
    command.to_lowercase().contains(&exe.to_string_lossy().to_lowercase())
}

fn register(dir: &Path, log: &Log) -> Result<(), String> {
    fn reg(e: impl std::fmt::Display) -> String {
        format!("Settings > Apps: {e}")
    }
    let hive = windows_registry::CURRENT_USER;
    // Rewritten whole, which also drops what the first installer kept there.
    let _ = hive.remove_tree(APP_KEY);
    let key = hive.create(APP_KEY).map_err(reg)?;
    let (exe, setup) = (dir.join(EXE), dir.join(SETUP));
    let size: u64 =
        [&exe, &setup, &dir.join(LICENCES)].iter().filter_map(|p| p.metadata().ok()).map(|m| m.len()).sum();
    for (name, value) in [
        ("DisplayName", "Kuro".to_owned()),
        ("DisplayVersion", version().to_owned()),
        ("Publisher", "Simdrom".to_owned()),
        ("DisplayIcon", exe.display().to_string()),
        ("InstallLocation", dir.display().to_string()),
        ("UninstallString", format!("\"{}\" /uninstall", setup.display())),
        ("QuietUninstallString", format!("\"{}\" /uninstall /S", setup.display())),
    ] {
        key.set_string(name, value).map_err(reg)?;
    }
    for (name, value) in
        [("NoModify", 1), ("NoRepair", 1), ("EstimatedSize", u32::try_from(size / 1024).unwrap_or(0))]
    {
        key.set_u32(name, value).map_err(reg)?;
    }
    log.add(format!("Listed in Settings > Apps as Kuro {}", version()));
    Ok(())
}

// ---- install ------------------------------------------------------------------------------------

/// Installs, updates, reinstalls or goes back to this version. Lists, settings and sessions
/// live elsewhere and are never touched.
pub fn install(st: &State, c: Choices, log: &Log) -> Result<(), String> {
    if PROGRAM.is_empty() {
        return Err(t("This setup was built without Kuro inside."));
    }
    log.add(format!("Kuro {}: {:?}", version(), plan(st)));
    let dir = match &st.installed {
        Some(i) => i.dir.clone(),
        None => default_dir()?,
    };
    let exe = dir.join(EXE);
    std::fs::create_dir_all(&dir).map_err(io(&dir))?;
    close_kuro(None, log)?;
    replace(&exe, &unpack(PROGRAM)?, log)?;
    replace(&dir.join(LICENCES), &unpack(LICENCES_Z)?, log)?;
    // Uninstall in Settings runs this copy.
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let setup = dir.join(SETUP);
    if !same_file(&me, &setup) {
        replace(&setup, &std::fs::read(&me).map_err(io(&me))?, log)?;
    }
    for f in OLD_UNINSTALLER {
        remove(&dir.join(f), log)?;
    }
    if let Some(programs) = win::programs() {
        win::shortcut(&programs.join(SHORTCUT), &exe).map_err(|e| format!("Start menu: {e}"))?;
        log.add("Start menu shortcut");
    }
    if let Some(desktop) = win::desktop() {
        let lnk = desktop.join(SHORTCUT);
        if c.desktop {
            win::shortcut(&lnk, &exe).map_err(|e| format!("Desktop: {e}"))?;
            log.add("Desktop shortcut");
        } else {
            remove(&lnk, log)?;
        }
    }
    register(&dir, log)?;
    if c.launch {
        launch(&exe, log);
    }
    Ok(())
}

pub fn launch(exe: &Path, log: &Log) {
    match Command::new(exe).spawn() {
        Ok(_) => log.add("Started Kuro"),
        Err(e) => log.add(format!("Could not start Kuro: {e}")),
    }
}

// ---- uninstall ----------------------------------------------------------------------------------

/// Set when this setup is the copy in the install folder: it cannot delete itself while it runs.
static LEFT_BEHIND: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Closes Kuro and removes the program, its shortcuts and its entries. The lists, settings and
/// sessions stay unless `purge`.
pub fn uninstall(st: &State, purge: bool, log: &Log) -> Result<(), String> {
    let dir = match &st.installed {
        Some(i) => i.dir.clone(),
        None => default_dir()?,
    };
    log.add(format!("Uninstalling from {}", dir.display()));
    let exe = dir.join(EXE);
    close_kuro(Some(&dir), log)?;
    let hive = windows_registry::CURRENT_USER;
    if let Ok(run) = hive.options().read().write().open(RUN)
        && run.get_string("Kuro").is_ok_and(|c| points_to(&c, &exe))
    {
        let _ = run.remove_value("Kuro");
        log.add("No longer starts with Windows");
    }
    let opens = hive.open(format!(r"{LINKS}\shell\open\command")).and_then(|k| k.get_string(""));
    if opens.is_ok_and(|c| points_to(&c, &exe)) {
        let _ = hive.remove_tree(LINKS);
        log.add("kuro:// links released");
    }
    let _ = hive.remove_tree(TOAST_ID);
    for lnk in [win::programs(), win::desktop()].into_iter().flatten() {
        remove(&lnk.join(SHORTCUT), log)?;
    }
    for f in [EXE, LICENCES].into_iter().chain(OLD_UNINSTALLER) {
        remove(&dir.join(f), log)?;
        remove(&dir.join(format!("{f}.new")), log)?;
    }
    let _ = hive.remove_tree(APP_KEY);
    log.add("Removed from Settings > Apps");
    let setup = dir.join(SETUP);
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    if same_file(&me, &setup) {
        *LEFT_BEHIND.lock().unwrap_or_else(|e| e.into_inner()) = Some(dir.clone());
    } else {
        remove(&setup, log)?;
        let _ = std::fs::remove_dir(&dir);
    }
    if purge {
        forget(log)?;
    }
    Ok(())
}

fn forget(log: &Log) -> Result<(), String> {
    if let Some((data, cache)) = data_dirs() {
        for d in [&data, &cache] {
            match std::fs::remove_dir_all(d) {
                Ok(()) => log.add(format!("Removed {}", d.display())),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(tf("Could not remove {0}: {1}", &[&d.display().to_string(), &e.to_string()]));
                }
            }
        }
        if let Some(parent) = data.parent() {
            let _ = std::fs::remove_dir(parent);
        }
    }
    kuro_api::Keyring::new("kuro").forget_all().map_err(|e| e.to_string())?;
    log.add("Sessions removed from the Credential Manager");
    Ok(())
}

/// Removes the copy in the install folder once this setup has exited.
pub fn finish() {
    let Some(dir) = LEFT_BEHIND.lock().unwrap_or_else(|e| e.into_inner()).take() else { return };
    let setup = dir.join(SETUP);
    let script = format!(
        "/d /c ping -n 4 127.0.0.1 >nul & del /f /q \"{}\" & rmdir \"{}\"",
        setup.display(),
        dir.display()
    );
    win::run_hidden("cmd.exe", &script);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_as_numbers() {
        assert_eq!(compare("0.2.0", "0.12.0"), Ordering::Less);
        assert_eq!(compare("0.10.0", "0.9.9"), Ordering::Greater);
        assert_eq!(compare("1.0.0", "1.0.0"), Ordering::Equal);
        assert_eq!(compare("odd", "0.1.0"), Ordering::Less);
    }

    fn state(installed: Option<&str>, schema: Option<i64>) -> State {
        State {
            installed: installed.map(|v| Installed { version: v.into(), dir: PathBuf::new() }),
            schema,
            ..State::default()
        }
    }

    #[test]
    fn plans_follow_the_installed_version() {
        let this = version();
        assert_eq!(plan(&state(None, None)), Plan::Install);
        assert_eq!(plan(&state(Some("0.0.1"), Some(1))), Plan::Update("0.0.1".into()));
        assert_eq!(plan(&state(Some(this), Some(schema()))), Plan::Reinstall);
        assert_eq!(plan(&state(Some("999.0.0"), Some(schema()))), Plan::Downgrade("999.0.0".into()));
        assert_eq!(plan(&state(Some("999.0.0"), None)), Plan::Downgrade("999.0.0".into()));
        assert_eq!(plan(&state(Some("999.0.0"), Some(schema() + 1))), Plan::TooNew("999.0.0".into()));
    }

    #[test]
    fn entries_of_another_copy_do_not_match() {
        let exe = Path::new(r"C:\Users\a\AppData\Local\Programs\Kuro\kuro.exe");
        assert!(points_to(r#""C:\users\A\AppData\Local\Programs\Kuro\kuro.exe" --hidden"#, exe));
        assert!(!points_to(r#""C:\Other\kuro.exe" --hidden"#, exe));
    }

    #[test]
    fn finds_where_programs_run_from() {
        let me = std::env::current_exe().unwrap();
        let name = me.file_name().unwrap().to_string_lossy().into_owned();
        assert!(win::running(&name).iter().any(|p| same_file(p, &me)));
        assert!(win::running("no-such-program.exe").is_empty());
    }

    #[test]
    fn replacing_checks_the_copy() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("kuro.exe");
        std::fs::write(&p, b"old").unwrap();
        replace(&p, b"new", &Log::default()).unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"new");
        assert!(!dir.path().join("kuro.exe.new").exists());
    }
}
