//! Command launching — port of `services/runner_service.py`.
//! Arguments are a comma-separated string; the working directory is the
//! executable's parent; `run_as_admin` (and Windows error 740) elevate
//! via ShellExecuteW "runas".

use std::path::Path;

use crate::sounds::SoundPlayer;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct RunError(pub String);

pub fn run_command(
    path: &str,
    args: &str,
    run_as_admin: bool,
    sounds: &SoundPlayer,
) -> Result<(), RunError> {
    let split_args: Vec<String> = if args.is_empty() {
        Vec::new()
    } else {
        args.split(',').map(|a| a.trim().to_string()).collect()
    };
    let cwd = Path::new(path).parent().map(|p| p.to_path_buf()).unwrap_or_default();

    sounds.play("run");

    launch(path, &split_args, &cwd, run_as_admin)
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
    let params_string = args.join(" ");
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

    fn quiet_sounds() -> SoundPlayer {
        SoundPlayer::new("nonexistent-sounds-dir", false)
    }

    #[cfg(windows)]
    #[test]
    fn spawns_a_simple_command() {
        let result = run_command(r"C:\Windows\System32\cmd.exe", "/c, exit 0", false, &quiet_sounds());
        assert!(result.is_ok(), "{result:?}");
    }

    #[cfg(windows)]
    #[test]
    fn missing_executable_is_an_error() {
        let result = run_command(r"C:\definitely\missing.exe", "", false, &quiet_sounds());
        assert!(result.is_err());
    }
}
