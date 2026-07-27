//! Resolving [`launchtype_core::portable`] placeholders against the real
//! machine: where this user's folders are, and which browsers are installed.
//!
//! The rules live in core and are pure; everything platform-specific is here.
//! A browser that is not installed resolves to [`VarValue::DefaultOpener`]
//! rather than being dropped, so `{{chrome}}` on a Mac without Chrome still
//! opens the URL instead of failing.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use launchtype_core::portable::{VarValue, Vars};

static VARS: OnceLock<Vars> = OnceLock::new();

/// This machine's placeholder values, probed once and reused.
///
/// Resolution touches the filesystem (which browsers are installed), so it is
/// cached; nothing here changes while the app runs. The app makes its data
/// folder the working directory at startup, which is what `{{launchtype}}`
/// resolves to.
pub fn vars() -> &'static Vars {
    VARS.get_or_init(|| {
        let app_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        system_vars(&app_dir)
    })
}

/// Candidate install locations per browser placeholder, most likely first.
/// `~` is expanded against the home folder; on Windows a leading `%pf%` /
/// `%pf86%` / `%local%` stands in for the corresponding folder.
#[cfg(windows)]
const BROWSER_CANDIDATES: &[(&str, &[&str])] = &[
    (
        "chrome",
        &[
            r"%pf%\Google\Chrome\Application\chrome.exe",
            r"%pf86%\Google\Chrome\Application\chrome.exe",
            r"%local%\Google\Chrome\Application\chrome.exe",
        ],
    ),
    ("firefox", &[r"%pf%\Mozilla Firefox\firefox.exe", r"%pf86%\Mozilla Firefox\firefox.exe"]),
    (
        "edge",
        &[r"%pf86%\Microsoft\Edge\Application\msedge.exe", r"%pf%\Microsoft\Edge\Application\msedge.exe"],
    ),
    (
        "brave",
        &[
            r"%pf%\BraveSoftware\Brave-Browser\Application\brave.exe",
            r"%pf86%\BraveSoftware\Brave-Browser\Application\brave.exe",
            r"%local%\BraveSoftware\Brave-Browser\Application\brave.exe",
        ],
    ),
    ("vivaldi", &[r"%pf%\Vivaldi\Application\vivaldi.exe", r"%local%\Vivaldi\Application\vivaldi.exe"]),
    ("opera", &[r"%local%\Programs\Opera\opera.exe", r"%pf%\Opera\opera.exe"]),
    // Safari is macOS-only; the placeholder still exists so a commands file
    // written on a Mac keeps working here (it falls back to the default).
    ("safari", &[]),
];

#[cfg(not(windows))]
const BROWSER_CANDIDATES: &[(&str, &[&str])] = &[
    ("chrome", &["/Applications/Google Chrome.app", "~/Applications/Google Chrome.app"]),
    ("firefox", &["/Applications/Firefox.app", "~/Applications/Firefox.app"]),
    ("edge", &["/Applications/Microsoft Edge.app", "~/Applications/Microsoft Edge.app"]),
    ("brave", &["/Applications/Brave Browser.app", "~/Applications/Brave Browser.app"]),
    ("vivaldi", &["/Applications/Vivaldi.app", "~/Applications/Vivaldi.app"]),
    ("opera", &["/Applications/Opera.app", "~/Applications/Opera.app"]),
    ("safari", &["/Applications/Safari.app", "/System/Applications/Safari.app"]),
];

/// This machine's values for every placeholder in the catalog.
///
/// `app_dir` is the folder Launchtype runs from — the caller knows it (it is
/// the process working directory), and passing it in keeps this testable.
pub fn system_vars(app_dir: &Path) -> Vars {
    let mut entries: Vec<(String, VarValue)> = Vec::new();
    let mut push_dir = |name: &str, path: Option<PathBuf>| {
        if let Some(path) = path {
            let text = path.to_string_lossy().trim_end_matches(['\\', '/']).to_string();
            if !text.is_empty() {
                entries.push((name.to_string(), VarValue::Path(text)));
            }
        }
    };

    let home = dirs::home_dir();
    push_dir("home", home.clone());
    push_dir("desktop", dirs::desktop_dir());
    push_dir("documents", dirs::document_dir());
    push_dir("downloads", dirs::download_dir());
    push_dir("music", dirs::audio_dir());
    push_dir("pictures", dirs::picture_dir());
    push_dir("videos", dirs::video_dir());
    push_dir("appdata", roaming_data_dir());
    push_dir("localappdata", local_data_dir());
    push_dir("programfiles", program_files());
    push_dir("programfiles86", program_files_x86());
    push_dir("programdata", program_data());
    push_dir("temp", Some(std::env::temp_dir()));
    push_dir("launchtype", Some(app_dir.to_path_buf()));
    push_dir("onedrive", onedrive_dir(home.as_deref()));

    if let Some(name) = login_name() {
        entries.push(("username".to_string(), VarValue::Path(name)));
    }

    // The default browser is never a path: whatever handles the URL wins.
    entries.push(("browser".to_string(), VarValue::DefaultOpener));
    for (name, candidates) in BROWSER_CANDIDATES {
        let found = candidates
            .iter()
            .map(|candidate| expand_candidate(candidate, home.as_deref()))
            .find(|path| Path::new(path).exists());
        entries.push((
            name.to_string(),
            // Not installed here: fall back to the default browser rather than
            // leaving the placeholder dangling.
            found.map_or(VarValue::DefaultOpener, VarValue::Path),
        ));
    }

    Vars::new(entries, cfg!(windows), if cfg!(windows) { '\\' } else { '/' })
}

fn expand_candidate(candidate: &str, home: Option<&Path>) -> String {
    let mut path = candidate.to_string();
    if let Some(rest) = candidate.strip_prefix("~/") {
        if let Some(home) = home {
            path = home.join(rest).to_string_lossy().into_owned();
        }
    }
    for (token, value) in [
        ("%pf%", program_files()),
        ("%pf86%", program_files_x86()),
        ("%local%", local_data_dir()),
    ] {
        if path.contains(token) {
            let replacement = value.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
            path = path.replace(token, &replacement);
        }
    }
    path
}

/// Roaming application data: `%APPDATA%` on Windows, Application Support on
/// macOS (which draws no roaming/local distinction).
fn roaming_data_dir() -> Option<PathBuf> {
    dirs::config_dir()
}

fn local_data_dir() -> Option<PathBuf> {
    dirs::data_local_dir()
}

#[cfg(windows)]
fn program_files() -> Option<PathBuf> {
    std::env::var_os("ProgramW6432")
        .or_else(|| std::env::var_os("ProgramFiles"))
        .map(PathBuf::from)
}

#[cfg(windows)]
fn program_files_x86() -> Option<PathBuf> {
    std::env::var_os("ProgramFiles(x86)").map(PathBuf::from).or_else(program_files)
}

#[cfg(windows)]
fn program_data() -> Option<PathBuf> {
    std::env::var_os("ProgramData").map(PathBuf::from)
}

/// macOS has one applications folder; both Program Files placeholders point at
/// it so a Windows-authored command still resolves.
#[cfg(not(windows))]
fn program_files() -> Option<PathBuf> {
    Some(PathBuf::from("/Applications"))
}

#[cfg(not(windows))]
fn program_files_x86() -> Option<PathBuf> {
    Some(PathBuf::from("/Applications"))
}

#[cfg(not(windows))]
fn program_data() -> Option<PathBuf> {
    Some(PathBuf::from("/Library/Application Support"))
}

fn onedrive_dir(home: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("OneDrive") {
        let path = PathBuf::from(path);
        if path.exists() {
            return Some(path);
        }
    }
    let candidate = home?.join("OneDrive");
    candidate.exists().then_some(candidate)
}

fn login_name() -> Option<String> {
    for key in ["USERNAME", "USER", "LOGNAME"] {
        if let Some(value) = std::env::var_os(key) {
            let value = value.to_string_lossy().trim().to_string();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    // Fall back to the home folder's own name.
    dirs::home_dir()?.file_name().map(|n| n.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use launchtype_core::portable::{expand, resolve_target, Target};

    fn vars() -> Vars {
        system_vars(Path::new("."))
    }

    #[test]
    fn the_home_folder_resolves_and_is_absolute() {
        let vars = vars();
        let home = expand("{{home}}", &vars);
        assert!(!home.contains("{{"), "unresolved: {home}");
        assert!(Path::new(&home).is_absolute(), "not absolute: {home}");
        assert!(Path::new(&home).exists(), "missing: {home}");
    }

    /// Every catalogued placeholder must resolve to something, or a command
    /// using it would launch a path with a literal `{{name}}` in it.
    ///
    /// Machine-dependent placeholders are exempt: `{{onedrive}}` has no value
    /// without OneDrive installed, so this machine having none proves nothing.
    /// The insert menu filters on the value being present, so an absent one is
    /// never offered.
    #[test]
    fn every_catalogued_placeholder_has_a_value() {
        let vars = vars();
        for spec in launchtype_core::portable::all_specs() {
            if launchtype_core::portable::is_machine_dependent(spec.name) {
                continue;
            }
            assert!(
                vars.get(spec.name).is_some(),
                "{} has no value on this machine",
                spec.name
            );
        }
    }

    /// The exemption above must stay narrow: a placeholder is only allowed to be
    /// machine-dependent if it is in the catalog in the first place.
    #[test]
    fn machine_dependent_placeholders_are_catalogued() {
        let catalogued: Vec<&str> = launchtype_core::portable::all_specs()
            .iter()
            .map(|spec| spec.name)
            .filter(|name| launchtype_core::portable::is_machine_dependent(name))
            .collect();
        assert_eq!(catalogued, ["onedrive"]);
    }

    #[test]
    fn the_default_browser_placeholder_is_never_a_path() {
        let vars = vars();
        assert_eq!(vars.get("browser"), Some(&VarValue::DefaultOpener));
        assert_eq!(resolve_target("{{browser}}", &vars), Target::DefaultOpener);
    }

    /// The portability promise: a browser that is missing here still opens the
    /// URL through the default handler.
    #[test]
    fn a_missing_browser_degrades_to_the_default_opener() {
        let vars = vars();
        for name in ["chrome", "firefox", "edge", "brave", "vivaldi", "opera", "safari"] {
            match vars.get(name).unwrap() {
                VarValue::Path(path) => {
                    assert!(Path::new(path).exists(), "{name} resolved to a missing {path}")
                }
                VarValue::DefaultOpener => {
                    assert_eq!(resolve_target(&format!("{{{{{name}}}}}"), &vars), Target::DefaultOpener)
                }
            }
        }
    }

    #[test]
    fn the_launchtype_placeholder_points_at_the_app_folder() {
        let dir = tempfile::tempdir().unwrap();
        let vars = system_vars(dir.path());
        assert_eq!(
            expand("{{launchtype}}", &vars),
            dir.path().to_string_lossy().trim_end_matches(['\\', '/'])
        );
    }

    #[test]
    fn folder_values_carry_no_trailing_separator() {
        let vars = vars();
        for spec in launchtype_core::portable::DIR_VARS {
            if let Some(VarValue::Path(path)) = vars.get(spec.name) {
                assert!(
                    !path.ends_with('\\') && !path.ends_with('/'),
                    "{} ends with a separator: {path}",
                    spec.name
                );
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_program_files_placeholders_resolve_separately() {
        let vars = vars();
        assert_eq!(expand("{{programfiles}}", &vars), r"C:\Program Files");
        assert_eq!(expand("{{programfiles86}}", &vars), r"C:\Program Files (x86)");
    }
}
