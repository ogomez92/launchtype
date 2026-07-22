//! SSH mode ($) — the UI half of `launchtype_services::ssh`.
//!
//! Entering the mode opens one connection with one persistent login shell
//! and keeps both; each Enter sends the contents of the input field as a
//! command and appends its output to the results list, one row per line.
//! Anything the command wrote to stderr is surfaced as an alert, because
//! that is what the user needs to read rather than arrow through.

use launchtype_core::i18n::{format_args, tr, Arg};
use launchtype_core::mode::UiMode;
use launchtype_core::settings::Settings;
use launchtype_services::clipboard;
use launchtype_services::ssh::{output_lines, CommandOutput, SshConfig, SshError, SshSession};

use crate::shell::{show_alert, show_error, update_list, with_shell, SharedShell};
use crate::speech::speak_now;

/// The results list is rebuilt from scratch on every keystroke, so the
/// transcript is trimmed to the most recent lines rather than growing without
/// bound over a session.
const MAX_TRANSCRIPT_LINES: usize = 500;

/// Whether the two settings snapshots point SSH mode at a different server or
/// credentials, which invalidates a live connection.
pub fn config_changed(before: &Settings, after: &Settings) -> bool {
    (
        &before.ssh_host, before.ssh_port, &before.ssh_user,
        &before.ssh_key_path, &before.ssh_password,
    ) != (
        &after.ssh_host, after.ssh_port, &after.ssh_user,
        &after.ssh_key_path, &after.ssh_password,
    )
}

fn config_from(settings: &Settings) -> SshConfig {
    SshConfig {
        host: settings.ssh_host.clone(),
        port: settings.ssh_port,
        user: settings.ssh_user.clone(),
        key_path: settings.ssh_key_path.clone(),
        password: settings.ssh_password.clone(),
    }
}

/// Called right after `$` switches the mode: start connecting, or bounce back
/// to commands mode when there is nothing to connect to.
pub fn enter_ssh_mode(shell: &SharedShell) {
    let (configured, connected, config, host, frame) = {
        let s = shell.borrow();
        let settings = &s.settings.settings;
        (
            settings.ssh_configured(),
            s.ssh.is_some(),
            config_from(settings),
            settings.ssh_host.trim().to_string(),
            s.frame,
        )
    };

    if !configured {
        shell.borrow_mut().mode = UiMode::Commands;
        show_error(
            &frame,
            &tr("SSH is not configured"),
            &tr("Open Settings and fill in the SSH server, user, and either a key file or a password."),
        );
        update_list(shell);
        return;
    }
    if connected {
        return;
    }

    speak_now(&format_args(&tr("Connecting to {host}"), &[("host", Arg::Str(&host))]), true);
    let announce_host = host.clone();
    let session = SshSession::connect(config, move |result| {
        let host = announce_host.clone();
        wxdragon::call_after(Box::new(move || {
            with_shell(|shell| match &result {
                Ok(startup_stderr) => {
                    let frame = shell.borrow().frame;
                    shell.borrow().sounds.play("match");
                    speak_now(
                        &format_args(&tr("Connected to {host}"), &[("host", Arg::Str(&host))]),
                        true,
                    );
                    // A broken .zshrc/.bashrc announces itself once, here,
                    // rather than silently polluting every command.
                    if !startup_stderr.trim().is_empty() {
                        speak_now(&tr("The login scripts wrote to standard error"), true);
                        show_alert(&frame, &tr("Error output"), startup_stderr.trim_end());
                    }
                }
                Err(error) => {
                    // The connection is dead; drop it so the next `$` retries.
                    let frame = {
                        let mut s = shell.borrow_mut();
                        s.ssh = None;
                        s.ssh_busy = false;
                        s.frame
                    };
                    speak_now(&tr("Could not connect"), true);
                    show_error(&frame, &tr("SSH error"), &error.message);
                }
            });
        }));
    });
    shell.borrow_mut().ssh = Some(session);
}

/// Enter in SSH mode: send the typed command, or — when nothing is typed —
/// copy the selected output line, which is the only other useful thing Enter
/// can do here.
pub fn run_ssh_command(shell: &SharedShell) {
    let command = shell.borrow().edit.get_value().trim().to_string();
    if command.is_empty() {
        copy_selected_line(shell);
        return;
    }
    if shell.borrow().ssh_busy {
        speak_now(&tr("The previous command is still running"), true);
        return;
    }
    if shell.borrow().ssh.is_none() {
        // The connection failed or was invalidated by a settings change.
        enter_ssh_mode(shell);
        return;
    }

    {
        let mut s = shell.borrow_mut();
        s.ssh_busy = true;
        s.edit.change_value("");
        s.sounds.play("run");
        // Echo the command so the transcript reads like a terminal.
        s.controller.ssh_output.push(format!("$ {command}"));
        if let Some(session) = &s.ssh {
            session.exec(&command, |result| {
                wxdragon::call_after(Box::new(move || {
                    with_shell(|shell| finish_command(shell, &result));
                }));
            });
        }
    }
    speak_now(&format_args(&tr("Running {command}"), &[("command", Arg::Str(&command))]), true);
    update_list(shell);
}

fn finish_command(shell: &SharedShell, result: &Result<CommandOutput, SshError>) {
    let mut first_new_line = shell.borrow().controller.ssh_output.len();
    let (frame, stderr) = {
        let mut s = shell.borrow_mut();
        s.ssh_busy = false;
        match result {
            Ok(output) => {
                if !output.stdout.trim().is_empty() {
                    s.controller.ssh_output.extend(output_lines(&output.stdout));
                }
                // Trimming shifts every index, including the one the cursor
                // is about to land on.
                let overflow = s.controller.ssh_output.len().saturating_sub(MAX_TRANSCRIPT_LINES);
                if overflow > 0 {
                    s.controller.ssh_output.drain(..overflow);
                    first_new_line = first_new_line.saturating_sub(overflow);
                }
                s.sounds.play("match");
                (s.frame, output.stderr.clone())
            }
            Err(error) => {
                let frame = s.frame;
                // A broken connection cannot be reused; the next command
                // reconnects.
                s.ssh = None;
                drop(s);
                speak_now(&tr("The command failed"), true);
                show_error(&frame, &tr("SSH error"), &error.message);
                update_list(shell);
                return;
            }
        }
    };

    update_list(shell);
    let total = shell.borrow().controller.ssh_output.len();
    let new_lines = total.saturating_sub(first_new_line);
    if new_lines > 0 {
        // Land the cursor on the first line of this command's output — but
        // only if the mode is still SSH: a command can finish long after the
        // user has moved on, and the list is then showing something else.
        let s = shell.borrow();
        if s.mode == UiMode::Ssh {
            s.list.set_selection(first_new_line as u32, true);
        }
        drop(s);
        speak_now(
            &format_args(
                &tr("{count} lines of output"),
                &[("count", Arg::Int(new_lines as i64))],
            ),
            true,
        );
    } else if stderr.trim().is_empty() {
        speak_now(&tr("The command produced no output"), true);
    }

    if !stderr.trim().is_empty() {
        speak_now(&tr("The command wrote to standard error"), true);
        show_alert(&frame, &tr("Error output"), stderr.trim_end());
    }
}

fn copy_selected_line(shell: &SharedShell) {
    let s = shell.borrow();
    let Some(index) = s.list.get_selection() else {
        speak_now(&tr("No command entered"), true);
        return;
    };
    let Some(item) = s.items.get(index as usize) else { return };
    clipboard::set_text(&item.name);
    s.sounds.play("copy");
    speak_now(&tr("Line copied"), true);
}
