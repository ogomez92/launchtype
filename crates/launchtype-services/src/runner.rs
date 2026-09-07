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
    // PATH; there is no stable absolute install location for a Store app. It
    // is looked up on PATH explicitly rather than handed to `Command` as a
    // name, which would search the app's own folder first (see
    // [`crate::program`]).
    let terminal = crate::program::on_path("wt").ok_or_else(|| {
        RunError(launchtype_core::i18n::tr("Windows Terminal was not found on this machine."))
    })?;
    std::process::Command::new(terminal)
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

/// Where Visual Studio Code installs itself when it is not on `PATH`.
/// `Code.exe` is preferred over the `code.cmd` launcher: a batch file is a
/// thornier thing to spawn safely, and the executable takes the same
/// file-and-folder arguments.
#[cfg(windows)]
const VSCODE_CANDIDATES: &[&str] = &[
    r"%local%\Programs\Microsoft VS Code\{name}.exe",
    r"%pf%\Microsoft VS Code\{name}.exe",
];

/// Open files and folders in Visual Studio Code, all in one window, the way
/// `code a b c` does from a shell.
#[cfg(windows)]
pub fn open_in_vscode(paths: &[String]) -> Result<(), RunError> {
    let program = crate::media::find_program("Code", VSCODE_CANDIDATES)
        // The install put its launcher on PATH but the executable somewhere
        // this does not know about: `code` (a .cmd) still gets there.
        .or_else(|| crate::media::find_program("code", &[]))
        .ok_or_else(|| {
            RunError(launchtype_core::i18n::tr(
                "Visual Studio Code was not found on this machine.",
            ))
        })?;
    std::process::Command::new(program)
        .args(paths)
        .spawn()
        .map(|_| ())
        .map_err(|e| RunError(e.to_string()))
}

/// The same on macOS, where the bundle is opened by name rather than by path:
/// `open -a` finds it wherever it was installed.
#[cfg(not(windows))]
pub fn open_in_vscode(paths: &[String]) -> Result<(), RunError> {
    let status = std::process::Command::new("/usr/bin/open")
        .arg("-a")
        .arg("Visual Studio Code")
        .args(paths)
        .status()
        .map_err(|e| RunError(e.to_string()))?;
    if status.success() {
        Ok(())
    } else {
        Err(RunError(launchtype_core::i18n::tr(
            "Visual Studio Code was not found on this machine.",
        )))
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
    // ShellExecuteW takes one command-line string rather than a list, so the
    // segments — which arrive already unquoted, which is what `spawn` needs on
    // the non-elevated path — have to be quoted back into one here.
    let params_string =
        args.iter().map(|arg| quote_argument(arg)).collect::<Vec<_>>().join(" ");
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

/// Quote one argument for a Windows command line, by the rules
/// `CommandLineToArgvW` parses it back with — which is what every program the
/// elevated path launches uses to split the string it is handed.
///
/// # Why the obvious version is not enough
///
/// Wrapping only arguments that hold a space, and leaving the rest alone, lets
/// an argument that holds a `"` end the quoting early and have the rest of
/// itself read as *further arguments*. That matters here and nowhere else in
/// this file: this is the path that runs a command elevated, so the user has
/// approved a UAC prompt naming one program and would then be handing it
/// switches nobody agreed to. A `{{query}}` answer typed into a stored admin
/// command is enough to do it — `x" --other-flag "y` used to arrive as three
/// arguments.
///
/// So: quote whenever the argument could otherwise be misread, double the run
/// of backslashes that precedes a quote (and the run at the very end, which
/// would otherwise escape the closing quote), and escape the quotes
/// themselves. An empty argument becomes `""`, which is the only way to pass
/// one at all.
#[cfg(windows)]
fn quote_argument(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_string();
    }
    let mut out = String::with_capacity(arg.len() + 2);
    out.push('"');
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => {
                backslashes += 1;
                continue;
            }
            '"' => {
                // Every backslash before a quote is doubled, then the quote
                // itself is escaped.
                out.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                out.push('"');
            }
            _ => {
                out.extend(std::iter::repeat_n('\\', backslashes));
                out.push(c);
            }
        }
        backslashes = 0;
    }
    // A trailing run would escape the closing quote if it were not doubled.
    out.extend(std::iter::repeat_n('\\', backslashes * 2));
    out.push('"');
    out
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

    /// Split a command line the way a launched program does, by asking Windows
    /// itself. Asserting against a hand-written expected string would only
    /// prove the quoting matches this test's idea of the rules; this proves it
    /// matches the parser on the other end.
    #[cfg(windows)]
    fn windows_argv(command_line: &str) -> Vec<String> {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::UI::Shell::CommandLineToArgvW;

        let wide: Vec<u16> = std::ffi::OsStr::new(command_line)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut count = 0i32;
        let argv = unsafe { CommandLineToArgvW(PCWSTR(wide.as_ptr()), &mut count) };
        assert!(!argv.is_null(), "CommandLineToArgvW rejected {command_line:?}");
        let parsed = (0..count as usize)
            .map(|i| unsafe { (*argv.add(i)).to_string().unwrap() })
            .collect::<Vec<_>>();
        unsafe {
            let _ = windows::Win32::Foundation::LocalFree(Some(
                windows::Win32::Foundation::HLOCAL(argv as *mut _),
            ));
        }
        // argv[0] is the program name the caller prepended.
        parsed[1..].to_vec()
    }

    /// The elevated path builds one string; whatever it builds has to split
    /// back into exactly the arguments it was given, or a UAC prompt the user
    /// approved for one thing ran it with something else.
    #[cfg(windows)]
    #[test]
    fn elevated_arguments_survive_the_round_trip_intact() {
        let cases: &[&[&str]] = &[
            &["/c", "exit 0"],
            &["a b", "c"],
            // The injection: a query answer that closes the quoting and adds
            // switches of its own.
            &[r#"x" --other-flag "y"#],
            &[r#"say "hi""#],
            &[r"C:\Program Files\app\", "next"],
            &[r"ends\with\backslashes\\"],
            &["", "after an empty one"],
            &["plain", "--flag=value", "tab\there"],
        ];
        for args in cases {
            let owned: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            let line = format!(
                "prog.exe {}",
                owned.iter().map(|a| quote_argument(a)).collect::<Vec<_>>().join(" ")
            );
            assert_eq!(windows_argv(&line), owned, "built {line:?}");
        }
    }

    /// The specific regression: an answer holding a quote used to reach the
    /// elevated process as extra arguments.
    #[cfg(windows)]
    #[test]
    fn a_quote_in_an_argument_cannot_add_arguments() {
        let injected = r#"target" --run-as-something-else "rest"#;
        let line = format!("prog.exe {}", quote_argument(injected));
        assert_eq!(windows_argv(&line), vec![injected.to_string()], "one argument, not three");
    }

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

    /// The whole launch pipeline for the command this feature exists for: one
    /// stored Google search, filled in at launch, then split and expanded the
    /// way `run_command` does it. The assertion is literally what the browser
    /// process is handed.
    ///
    /// The comma in the search words is the point. Arguments are one
    /// comma-separated string, so an unencoded comma would have split the URL
    /// in half and opened a search for "screen reader" with "braille" as a
    /// second, meaningless argument.
    #[test]
    fn a_google_search_reaches_the_browser_as_one_argument() {
        let vars = Vars::new(
            [("chrome".to_string(), VarValue::Path(r"C:\chrome.exe".to_string()))],
            true,
            '\\',
        );
        let (path, args) = launchtype_core::query::fill(
            "{{chrome}}",
            "https://www.google.com/search?q={{query}}",
            &["screen reader, braille".to_string()],
        );

        assert_eq!(resolve_target(&path, &vars), Target::Path(r"C:\chrome.exe".to_string()));
        let split: Vec<String> = arg_segments(&args).iter().map(|a| expand(a, &vars)).collect();
        assert_eq!(split, vec!["https://www.google.com/search?q=screen%20reader%2C%20braille"]);
    }

    /// Away from a URL the answer is the user's own text — a file name, a
    /// search string for a desktop program — and reaches the process as typed,
    /// alongside the ordinary placeholders in the same argument list.
    #[test]
    fn a_query_outside_a_url_reaches_the_program_verbatim() {
        let vars = Vars::new(
            [("home".to_string(), VarValue::Path(r"C:\Users\me".to_string()))],
            true,
            '\\',
        );
        let (_, args) = launchtype_core::query::fill(
            "",
            r"-i, {{query}}, {{home}}\notes",
            &["to do".to_string()],
        );
        let split: Vec<String> = arg_segments(&args).iter().map(|a| expand(a, &vars)).collect();
        assert_eq!(split, vec!["-i", "to do", r"C:\Users\me\notes"]);
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
