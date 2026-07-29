//! Command launching — port of `services/runner_service.py`.
//! Arguments are a comma-separated string; the working directory is the
//! executable's parent; `run_as_admin` (and Windows error 740) elevate
//! via ShellExecuteW "runas".
//!
//! Both fields go through [`launchtype_core::portable`] first, so a command
//! stored as `{{chrome}}` + a URL runs on whatever machine it lands on. A
//! `path` that resolves to [`Target::DefaultOpener`] has no executable to
//! spawn: the argument is handed to the OS instead.

use std::path::Path;

use launchtype_core::portable::{
    arg_segments, expand, looks_like_url, resolve_target, Target, Vars,
};
#[cfg(target_os = "macos")]
use launchtype_core::portable::browser_for_executable;

use crate::sounds::SoundPlayer;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct RunError(pub String);

pub fn run_command(
    path: &str,
    args: &str,
    run_as_admin: bool,
    sounds: &SoundPlayer,
    vars: &Vars,
) -> Result<(), RunError> {
    let split_args: Vec<String> =
        arg_segments(args).iter().map(|arg| expand(arg, vars)).collect();

    sounds.play("run");

    match resolve_target(path, vars) {
        Target::DefaultOpener => open_with_default_handler(&split_args),
        Target::Path(resolved) => {
            let cwd = Path::new(&resolved).parent().map(|p| p.to_path_buf()).unwrap_or_default();
            launch(&resolved, &split_args, &cwd, run_as_admin)
        }
    }
}

/// Open the system terminal in `folder`: Windows Terminal on Windows,
/// Terminal.app (in /Applications/Utilities) on macOS.
#[cfg(windows)]
pub fn open_terminal_at(folder: &Path) -> Result<(), RunError> {
    // `wt.exe` is the per-user execution alias Windows Terminal installs on
    // PATH; there is no stable absolute install location for a Store app.
    std::process::Command::new("wt.exe")
        .arg("-d")
        .arg(folder)
        .spawn()
        .map(|_| ())
        .map_err(|e| RunError(e.to_string()))
}

#[cfg(not(windows))]
pub fn open_terminal_at(folder: &Path) -> Result<(), RunError> {
    // `open -a Terminal <folder>` starts (or reuses) Terminal.app with a
    // window whose working directory is the folder.
    let status = std::process::Command::new("/usr/bin/open")
        .arg("-a")
        .arg("Terminal")
        .arg(folder)
        .status()
        .map_err(|e| RunError(e.to_string()))?;
    if status.success() {
        Ok(())
    } else {
        Err(RunError(format!("open failed for {} ({status})", folder.display())))
    }
}

/// Hand the first argument to whatever the OS uses for it — the default
/// browser for a URL. Used by `{{browser}}` and by a specific browser
/// placeholder on a machine where that browser is not installed.
fn open_with_default_handler(args: &[String]) -> Result<(), RunError> {
    let Some(target) = args.iter().find(|arg| !arg.is_empty()) else {
        return Err(RunError(launchtype_core::i18n::tr(
            "this command opens its first argument with the default application, but it has no arguments",
        )));
    };
    open::that_detached(with_scheme(target)).map_err(|e| RunError(e.to_string()))
}

/// Give a bare domain the `https://` a browser would have inferred.
///
/// Plenty of commands store their address the way it is typed into an address
/// bar — `gmail.com`, `calendar.google.com`. A browser executable resolves
/// that itself, but the OS opener would take it for a file name and fail, so
/// the scheme has to be put back before handing it over.
fn with_scheme(target: &str) -> String {
    if looks_like_url(target)
        || launchtype_core::portable::is_absolute_location(target)
        || target.starts_with('-')
        || !target.contains('.')
        || Path::new(target).exists()
    {
        return target.to_string();
    }
    format!("https://{target}")
}

#[cfg(windows)]
fn launch(path: &str, args: &[String], cwd: &Path, run_as_admin: bool) -> Result<(), RunError> {
    if run_as_admin {
        return shell_execute_runas(path, args, cwd);
    }
    match std::process::Command::new(path).args(args).current_dir(cwd).spawn() {
        Ok(_child) => Ok(()),
        // 740 = ERROR_ELEVATION_REQUIRED: the target demands elevation even
        // though the command is not flagged run_as_admin. Retry elevated.
        Err(e) if e.raw_os_error() == Some(740) => shell_execute_runas(path, args, cwd),
        Err(e) => Err(RunError(e.to_string())),
    }
}

#[cfg(windows)]
fn shell_execute_runas(path: &str, args: &[String], cwd: &Path) -> Result<(), RunError> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        s.encode_wide().chain(std::iter::once(0)).collect()
    }

    let verb = wide("runas".as_ref());
    let file = wide(path.as_ref());
    // ShellExecuteW takes one command-line string rather than a list, so an
    // argument holding spaces has to be re-quoted here — the segments arrive
    // already unquoted, which is what `spawn` needs on the non-elevated path.
    let params_string = args
        .iter()
        .map(|arg| {
            if arg.contains(' ') && !arg.starts_with('"') {
                format!("\"{arg}\"")
            } else {
                arg.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    let params = wide(params_string.as_ref());
    let dir = wide(cwd.as_os_str());

    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(verb.as_ptr()),
            PCWSTR(file.as_ptr()),
            PCWSTR(params.as_ptr()),
            PCWSTR(dir.as_ptr()),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecuteW returns a fake HINSTANCE; values > 32 mean success.
    if result.0 as usize > 32 {
        Ok(())
    } else {
        Err(RunError(format!("ShellExecuteW failed (code {})", result.0 as usize)))
    }
}

#[cfg(not(windows))]
fn launch(path: &str, args: &[String], cwd: &Path, _run_as_admin: bool) -> Result<(), RunError> {
    // A .app is a directory, so exec'ing it fails with EACCES ("permission
    // denied", os error 13) rather than anything that names the real problem.
    // Every macOS browser placeholder resolves to one, so this is the normal
    // path on a Mac, not an edge case.
    #[cfg(target_os = "macos")]
    if is_app_bundle(path) {
        return open_app_bundle(path, args, cwd);
    }

    // run_as_admin has no macOS equivalent for GUI launches; run normally.
    std::process::Command::new(path)
        .args(args)
        .current_dir(cwd)
        .spawn()
        .map(|_| ())
        .map_err(|e| RunError(e.to_string()))
}

#[cfg(target_os = "macos")]
fn is_app_bundle(path: &str) -> bool {
    let path = Path::new(path);
    path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("app")) && path.is_dir()
}

/// Launch a bundle through `open`, which is the only supported way to start
/// one: it finds the real executable inside `Contents/MacOS` and hands the app
/// to the window server so it comes up focused.
///
/// `open` splits its tail two ways, and so does this. Plain arguments are
/// documents/URLs for the app to open — `open -a Safari https://x.com` — while
/// anything starting with `-` is a switch for the program itself and has to sit
/// behind `--args`. Sending a URL through `--args` would leave Safari opening
/// an empty window, which is the shape of the original bug.
#[cfg(target_os = "macos")]
fn open_app_bundle(bundle: &str, args: &[String], cwd: &Path) -> Result<(), RunError> {
    let (flags, documents): (Vec<&String>, Vec<&String>) =
        args.iter().partition(|arg| arg.starts_with('-'));

    let mut command = std::process::Command::new("/usr/bin/open");
    command.arg("-a").arg(bundle).current_dir(cwd);
    for document in documents {
        // Only a browser may assume a bare `gmail.com` is a website. For any
        // other app the argument is far more likely to be a file, and turning
        // `notes.txt` into `https://notes.txt` would break it.
        if browser_for_executable(bundle).is_some() {
            command.arg(with_scheme(document));
        } else {
            command.arg(document);
        }
    }
    if !flags.is_empty() {
        command.arg("--args").args(flags);
    }

    // `open` reports a failure to start the app in its exit status, not by
    // failing to spawn, so this waits rather than detaching. It returns as soon
    // as the app is launched, not when it exits.
    match command.status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(RunError(format!("open failed for {bundle} ({status})"))),
        Err(e) => Err(RunError(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use launchtype_core::portable::VarValue;

    fn quiet_sounds() -> SoundPlayer {
        SoundPlayer::new("nonexistent-sounds-dir", false)
    }

    fn vars() -> Vars {
        crate::portable::system_vars(Path::new("."))
    }

    #[cfg(windows)]
    #[test]
    fn spawns_a_simple_command() {
        let result =
            run_command(r"C:\Windows\System32\cmd.exe", "/c, exit 0", false, &quiet_sounds(), &vars());
        assert!(result.is_ok(), "{result:?}");
    }

    #[cfg(windows)]
    #[test]
    fn missing_executable_is_an_error() {
        let result = run_command(r"C:\definitely\missing.exe", "", false, &quiet_sounds(), &vars());
        assert!(result.is_err());
    }

    /// A placeholder path must be resolved before spawning, not passed through.
    #[cfg(windows)]
    #[test]
    fn placeholders_in_the_path_are_expanded_before_launching() {
        let vars = Vars::new(
            [("shell".to_string(), VarValue::Path(r"C:\Windows\System32".to_string()))],
            true,
            '\\',
        );
        let result = run_command(r"{{shell}}\cmd.exe", "/c, exit 0", false, &quiet_sounds(), &vars);
        assert!(result.is_ok(), "{result:?}");
    }

    /// `{{browser}}` with nothing to open is a broken command, not a silent
    /// no-op: the user needs to be told.
    #[test]
    fn the_default_opener_needs_something_to_open() {
        let result = run_command("{{browser}}", "", false, &quiet_sounds(), &vars());
        assert!(result.is_err());
    }

    /// A browser executable resolves a bare domain itself; the OS opener does
    /// not, so `{{browser}}` on a Mac has to put the scheme back.
    #[test]
    fn bare_domains_get_a_scheme_before_the_opener_sees_them() {
        assert_eq!(with_scheme("gmail.com"), "https://gmail.com");
        assert_eq!(with_scheme("calendar.google.com"), "https://calendar.google.com");
        // Anything already addressable is passed through untouched.
        assert_eq!(with_scheme("https://gmail.com"), "https://gmail.com");
        assert_eq!(with_scheme("steam://rungameid/12"), "steam://rungameid/12");
        assert_eq!(with_scheme(r"C:\Users\me\notes.txt"), r"C:\Users\me\notes.txt");
        assert_eq!(with_scheme("/Users/me/notes.txt"), "/Users/me/notes.txt");
        assert_eq!(with_scheme("--accessibility"), "--accessibility");
        assert_eq!(with_scheme("some-flag"), "some-flag");
    }

    /// An installed browser resolves to its `.app`, and a bundle is a directory,
    /// so the old plain `spawn` came back EACCES — "permission denied (os error
    /// 13)" — for every browser command on a Mac.
    #[cfg(target_os = "macos")]
    #[test]
    fn app_bundles_are_recognised_and_never_spawned_directly() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("Some Browser.app");
        std::fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        assert!(is_app_bundle(bundle.to_str().unwrap()));

        // The extension alone is not enough; a plain file keeps the exec path.
        let file = dir.path().join("regular.app");
        std::fs::write(&file, b"").unwrap();
        assert!(!is_app_bundle(file.to_str().unwrap()));
        assert!(!is_app_bundle("/bin/echo"));

        // The real regression: spawning the bundle is what produced os error 13.
        let spawned = std::process::Command::new(bundle.to_str().unwrap()).spawn();
        assert_eq!(
            spawned.err().and_then(|e| e.raw_os_error()),
            Some(13),
            "a .app must still be unspawnable, or this fix is guarding nothing"
        );
    }

    /// End-to-end check of a real stored command against this machine, run by
    /// hand because it opens a browser window:
    /// `cargo test -p launchtype-services -- --ignored --nocapture browser`
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore]
    fn browser_placeholder_launches() {
        let vars = vars();
        eprintln!("{{{{firefox}}}} resolves to {:?}", resolve_target("{{firefox}}", &vars));
        let result =
            run_command("{{firefox}}", "https://example.com/", false, &quiet_sounds(), &vars);
        assert!(result.is_ok(), "{result:?}");
    }

    /// Arguments reach the process unquoted: a quoted path used to be passed
    /// through with its quotes, which the target program saw as part of the
    /// name. (The elevated path re-quotes, since it builds one string.)
    #[test]
    fn quoted_arguments_are_unquoted_and_expanded() {
        let vars = Vars::new(
            [("saves".to_string(), VarValue::Path(r"C:\Users\me\Saved Games".to_string()))],
            true,
            '\\',
        );
        let segments: Vec<String> = arg_segments(r#"-n, "{{saves}}\Entombed""#)
            .iter()
            .map(|arg| expand(arg, &vars))
            .collect();
        assert_eq!(segments, vec!["-n", r"C:\Users\me\Saved Games\Entombed"]);
    }
}
