//! Finding a helper program by name, safely.
//!
//! # Why this exists rather than `Command::new("bw")`
//!
//! Handing a bare name to [`std::process::Command`] on Windows lets
//! `CreateProcessW` resolve it, and the first place it looks is *the directory
//! the running executable sits in* — ahead of `PATH`, and whether or not
//! anything there was ever meant to be a program.
//!
//! Launchtype ships as a portable folder, which people unzip wherever it is
//! convenient: very often the same Downloads folder a browser drops files into
//! without asking. A `bw.exe` landing there is then the `bw` that the vault
//! import runs — and that one is handed the Bitwarden master password in its
//! environment. Same shape for `nvidia-smi`, `wt.exe` and the rest: a file
//! appearing next to the app should never become the app's idea of a system
//! tool.
//!
//! So every helper is resolved to an absolute path here first, out of `PATH`
//! or out of the Windows system directory, and never out of the app folder.
//!
//! (The current directory is *not* part of the problem: Rust's `Command` does
//! not search it. The executable's own directory is the one that matters, and
//! it is also the folder Launchtype keeps its data in — see `data_dir()` in
//! the app crate.)

use std::path::PathBuf;

/// Executable extensions to try when the name carries none. `.cmd`/`.bat` are
/// included because some tools ship only a launcher script (`bw` from npm is
/// one).
#[cfg(windows)]
const EXTENSIONS: &[&str] = &["exe", "cmd", "bat", ""];
#[cfg(not(windows))]
const EXTENSIONS: &[&str] = &[""];

/// The absolute path of `name` as found on `PATH`, or `None`.
///
/// Only `PATH` — not the app folder, not the current directory. Directories on
/// `PATH` are ones the user (or an installer they ran) deliberately put there;
/// the folder the app happens to live in is not.
pub fn on_path(name: &str) -> Option<PathBuf> {
    in_dirs(name, std::env::split_paths(&std::env::var_os("PATH")?))
}

/// [`on_path`] over a given list of directories, so the search itself can be
/// tested without reaching into the process environment (and because cargo
/// puts the target folder on `PATH` while tests run, which would make a test
/// of the real `PATH` prove nothing).
fn in_dirs(name: &str, dirs: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    for dir in dirs {
        // An empty entry in PATH means "the current directory" — one of the
        // implicit locations this is here to avoid.
        if dir.as_os_str().is_empty() {
            continue;
        }
        for extension in EXTENSIONS {
            let file = if extension.is_empty() {
                dir.join(name)
            } else {
                dir.join(name).with_extension(extension)
            };
            if file.is_file() {
                return Some(file);
            }
        }
    }
    None
}

/// A Windows program that lives in a known place under `%SystemRoot%`, given
/// as a path relative to it (`"explorer.exe"`,
/// `r"System32\WindowsPowerShell\v1.0\powershell.exe"`).
///
/// These are never looked for on `PATH`: their location is fixed, and the
/// whole point is not to let anything else answer to the name.
#[cfg(windows)]
pub fn in_windows_dir(relative: &str) -> PathBuf {
    let root = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    root.join(relative)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The regression this module exists for: a program is only ever found in
    /// a directory that was actually listed, never in one that merely happens
    /// to hold the running executable.
    #[test]
    fn only_the_listed_directories_answer_for_a_name() {
        let app_folder = tempfile::tempdir().unwrap();
        let real = tempfile::tempdir().unwrap();
        let name = "launchtype-planted-probe";
        let file = |dir: &std::path::Path| {
            dir.join(if cfg!(windows) { format!("{name}.exe") } else { name.to_string() })
        };
        // What an attacker drops beside a portable install, and what a real
        // install of the tool would look like.
        std::fs::write(file(app_folder.path()), b"planted").unwrap();
        std::fs::write(file(real.path()), b"the real one").unwrap();

        // The app folder is not on the list, so it does not get a say — even
        // though `Command::new(name)` would have started that very file.
        assert_eq!(
            in_dirs(name, [real.path().to_path_buf()]),
            Some(file(real.path())),
            "a listed directory should answer"
        );
        assert_eq!(
            in_dirs(name, [PathBuf::from(real.path()).join("nowhere")]),
            None,
            "nothing else may answer"
        );
    }

    /// An empty entry in PATH means "the current directory"; it is skipped.
    #[test]
    fn an_empty_path_entry_names_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let name = "launchtype-cwd-probe";
        let planted =
            dir.path().join(if cfg!(windows) { format!("{name}.exe") } else { name.to_string() });
        std::fs::write(&planted, b"planted").unwrap();

        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        let found = in_dirs(name, [PathBuf::new()]);
        std::env::set_current_dir(previous).unwrap();
        assert_eq!(found, None);
    }

    #[cfg(windows)]
    #[test]
    fn system_programs_resolve_under_the_windows_directory() {
        assert!(in_windows_dir("explorer.exe").is_file());
        assert!(in_windows_dir(r"System32\WindowsPowerShell\v1.0\powershell.exe").is_file());
    }

    /// Something every machine has, to prove the lookup works at all.
    #[test]
    fn a_real_program_is_found_on_the_path() {
        let name = if cfg!(windows) { "cmd" } else { "sh" };
        let found = on_path(name).expect("{name} should be on PATH");
        assert!(found.is_absolute(), "{found:?} is not absolute");
    }
}
