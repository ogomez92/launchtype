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
    // run_as_admin has no macOS equivalent for GUI launches; run normally.
    std::process::Command::new(path)
        .args(args)
        .current_dir(cwd)
        .spawn()
        .map(|_| ())
        .map_err(|e| RunError(e.to_string()))
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
