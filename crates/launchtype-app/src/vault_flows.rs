//! Encrypted vault mode (`*`) — the UI half of [`launchtype_core::vault`].
//!
//! Entering the mode asks for the master password (or, the first time, for one
//! to be chosen); after that the results list is the entry names and Enter
//! copies the secret behind the selected one.
//!
//! Copying is the whole point and also the risk, so the copy path does three
//! things beyond writing the clipboard: it tells the clipboard history to
//! never record that value — otherwise the 100ms poller would write the
//! password straight into `clipboard_history.json`, in the clear — it takes
//! the secret back off the clipboard a configurable number of seconds later
//! unless something else has been copied since, and it re-locks the vault when
//! the timeout is set to zero.

use std::sync::{Arc, Mutex};

use launchtype_core::bitwarden::{plan_import, ImportPlan};
use launchtype_core::i18n::{format_args, tr, Arg};
use launchtype_core::totp::Totp;
use launchtype_core::vault::{EntryKind, VaultError, VaultSession};
use launchtype_services::bitwarden::{BwAuth, BwCredentials, BwError, BwSession};
use launchtype_services::clipboard;
use zeroize::Zeroizing;

use crate::dialogs::{self, VaultEntryFields};
use crate::shell::{report_error, update_list, with_shell, SharedShell};
use crate::speech::speak_now;

/// The message shown for a vault failure. The error type lives in the core
/// crate, which has no catalog; the wording belongs here with the rest of the
/// translated text.
fn error_text(error: &VaultError) -> String {
    match error {
        VaultError::WrongPassword => tr("That is not the master password."),
        VaultError::Damaged => tr("That vault file is damaged, or is not a vault file."),
        VaultError::Locked => tr("The vault is locked."),
        VaultError::NoSuchEntry => tr("That entry is no longer in the vault."),
        VaultError::PasswordTooShort => tr("That master password is too short."),
        VaultError::AlreadyExists => tr("There is already a vault in that folder."),
        VaultError::Io(e) => e.to_string(),
    }
}

fn report(shell: &SharedShell, error: &VaultError) {
    shell.borrow().sounds.play("error");
    report_error(shell, &tr("Vault"), &error_text(error));
}

fn session(shell: &SharedShell) -> Arc<Mutex<VaultSession>> {
    shell.borrow().controller.vault.clone()
}

/// Called right after `*` switches the mode: open the vault, or set one up the
/// first time. Cancelling leaves the mode showing its "Unlock the vault" row,
/// so Enter tries again rather than stranding the user.
pub fn enter_vault_mode(shell: &SharedShell) {
    let vault = session(shell);
    let (unlocked, is_new) = {
        let v = vault.lock().unwrap();
        (v.is_unlocked(), v.is_new())
    };
    if unlocked {
        // Arriving counts as using it, so browsing does not run into the
        // idle timeout mid-search.
        let now = shell.borrow().controller.clock.now();
        vault.lock().unwrap().touch(now);
        return;
    }
    if is_new {
        create_vault(shell);
    } else {
        unlock_vault(shell);
    }
}

/// Enter on one of the rows that is an instruction rather than an entry.
pub fn run_action(shell: &SharedShell, action: &str) {
    match action {
        "create" => create_vault(shell),
        "unlock" => unlock_vault(shell),
        "lock" => lock_now(shell),
        "add" => add_entry(shell),
        "password" => change_password(shell),
        "import" => import_from_bitwarden(shell),
        other => log::warn!("unknown vault action {other:?}"),
    }
}

fn unlock_vault(shell: &SharedShell) {
    let frame = shell.borrow().frame;
    let Some(password) = dialogs::vault_unlock_dialog(&frame) else { return };

    let vault = session(shell);
    let now = shell.borrow().controller.clock.now();
    // Stretching the password takes a moment; there is nothing useful to do
    // with the UI in the meantime, so it happens here rather than on a thread.
    let result = vault.lock().unwrap().unlock(&password, now);
    match result {
        Ok(()) => {
            announce_unlocked(shell, &vault);
            update_list(shell);
        }
        // A mistyped password is not worth a modal: the row the user pressed
        // Enter on is still there, so saying so and letting them press it
        // again is the shortest way back.
        Err(VaultError::WrongPassword) => {
            shell.borrow().sounds.play("error");
            speak_now(&tr("That is not the master password."), true);
        }
        Err(error) => report(shell, &error),
    }
}

fn create_vault(shell: &SharedShell) {
    let vault = session(shell);
    let frame = shell.borrow().frame;

    // Encrypted entries with no key file next to them cannot be opened by any
    // password, and a fresh vault would quietly bury them.
    let orphans = vault.lock().unwrap().orphan_count();
    if orphans > 0 && !dialogs::confirm_vault_orphans(&frame, orphans) {
        return;
    }

    let Some(passwords) = dialogs::vault_password_dialog(&frame, false) else { return };
    let now = shell.borrow().controller.clock.now();
    let result = vault.lock().unwrap().create(&passwords.new, now);
    match result {
        Ok(()) => {
            shell.borrow().sounds.play("match");
            speak_now(&tr("The vault is ready and unlocked."), true);
            update_list(shell);
        }
        Err(error) => report(shell, &error),
    }
}

fn announce_unlocked(shell: &SharedShell, vault: &Arc<Mutex<VaultSession>>) {
    let count = vault.lock().unwrap().entries().len();
    shell.borrow().sounds.play("match");
    speak_now(
        &format_args(
            &tr("Vault unlocked, {count} entries"),
            &[("count", Arg::Int(count as i64))],
        ),
        true,
    );
}

fn lock_now(shell: &SharedShell) {
    session(shell).lock().unwrap().lock();
    shell.borrow().sounds.play("match");
    speak_now(&tr("Vault locked"), true);
    update_list(shell);
}

fn change_password(shell: &SharedShell) {
    let frame = shell.borrow().frame;
    let Some(passwords) = dialogs::vault_password_dialog(&frame, true) else { return };
    let result = session(shell).lock().unwrap().change_password(&passwords.current, &passwords.new);
    match result {
        Ok(()) => {
            shell.borrow().sounds.play("match");
            speak_now(&tr("Master password changed"), true);
        }
        Err(error) => report(shell, &error),
    }
}

/// Add an entry. Also the Add button's job while the vault mode is showing.
pub fn add_entry(shell: &SharedShell) {
    if !ensure_unlocked(shell) {
        return;
    }
    let frame = shell.borrow().frame;
    let Some(fields) = dialogs::vault_entry_dialog(&frame, None) else { return };
    save(shell, None, &fields, tr("Added to the vault"));
}

/// Edit the entry with `id`, reopening the dialog on its decrypted contents.
pub fn edit_entry(shell: &SharedShell, id: &str) {
    if !ensure_unlocked(shell) {
        return;
    }
    let existing = session(shell).lock().unwrap().entry(id);
    let (info, secret) = match existing {
        Ok(entry) => entry,
        Err(error) => return report(shell, &error),
    };
    let seed = VaultEntryFields {
        name: info.name,
        shortcut: info.shortcut,
        secret: secret.to_string(),
        kind: info.kind,
    };
    let frame = shell.borrow().frame;
    let Some(fields) = dialogs::vault_entry_dialog(&frame, Some(seed)) else { return };
    save(shell, Some(id), &fields, tr("Vault entry saved"));
}

fn save(shell: &SharedShell, id: Option<&str>, fields: &VaultEntryFields, spoken: String) {
    let result = session(shell).lock().unwrap().save(
        id,
        &fields.name,
        &fields.shortcut,
        &fields.secret,
        fields.kind,
    );
    match result {
        Ok(_) => {
            shell.borrow().sounds.play("match");
            speak_now(&spoken, true);
            update_list(shell);
        }
        Err(error) => report(shell, &error),
    }
}

/// Delete the entry with `id` after asking; there is no undo and no other copy.
pub fn delete_entry(shell: &SharedShell, id: &str, name: &str) {
    if !ensure_unlocked(shell) {
        return;
    }
    let frame = shell.borrow().frame;
    if !dialogs::confirm_vault_delete(&frame, name) {
        return;
    }
    let result = session(shell).lock().unwrap().delete(id);
    match result {
        Ok(()) => {
            shell.borrow().sounds.play("match");
            speak_now(&tr("Deleted from the vault"), true);
            update_list(shell);
        }
        Err(error) => report(shell, &error),
    }
}

/// Enter on an entry: decrypt it, put it on the clipboard, and get out of the
/// way so it can be pasted.
///
/// For an authenticator entry what reaches the clipboard is the six digits for
/// right now, not the seed they come from — the seed is what the entry stores,
/// but it is never what the user wants to paste.
pub fn copy_secret(shell: &SharedShell, id: &str, name: &str) {
    // The vault may have auto-locked with this very list still on screen, so
    // Enter on an entry asks for the password and then carries on rather than
    // reporting a failure the user can do nothing useful with.
    if !ensure_unlocked(shell) {
        return;
    }
    let vault = session(shell);
    // Bound to a `let` so the lock is released before anything below can open
    // a dialog on it: holding it across a modal would stall the auto-locker.
    let decrypted = vault.lock().unwrap().entry(id);
    let (info, stored) = match decrypted {
        Ok(entry) => entry,
        Err(error) => return report(shell, &error),
    };

    // Worked out before the clipboard is touched, so a seed that will not
    // parse leaves the clipboard alone instead of half-doing the job.
    let (secret, spoken) = match info.kind {
        EntryKind::Password => (stored, format_args(&tr("{name} copied"), &[("name", Arg::Str(name))])),
        EntryKind::Totp => {
            let unix = shell.borrow().controller.clock.now().timestamp();
            match Totp::parse(&stored) {
                Ok(totp) => {
                    let code = totp.code_at(unix);
                    // The countdown is the point of saying anything at all: a
                    // code with four seconds left is one to wait out rather
                    // than paste, and a screen reader user cannot see the ring
                    // an authenticator app would draw.
                    let spoken = format_args(
                        &tr("{name}, code {code}, {seconds} seconds left"),
                        &[
                            ("name", Arg::Str(name)),
                            ("code", Arg::Str(&code)),
                            ("seconds", Arg::Int(totp.seconds_remaining(unix) as i64)),
                        ],
                    );
                    (code, spoken)
                }
                Err(e) => {
                    shell.borrow().sounds.play("error");
                    return report_error(shell, &tr("Vault"), &dialogs::totp_error_text(&e));
                }
            }
        }
    };

    let (clipboard_history, clear_seconds, now) = {
        let s = shell.borrow();
        (
            s.controller.clipboard.clone(),
            s.settings.settings.vault_clipboard_seconds,
            s.controller.clock.now(),
        )
    };
    // Before the clipboard is written, not after: the history poller runs on
    // its own thread every 100ms and would otherwise be free to record the
    // secret in the window between the two.
    clipboard_history.lock().unwrap().suppress(&secret);
    clipboard::set_text(&secret);

    {
        let mut vault = vault.lock().unwrap();
        vault.touch(now);
        if vault.locks_on_use() {
            vault.lock();
        }
    }

    let s = shell.borrow();
    s.sounds.play("copy");
    speak_now(&spoken, true);
    crate::shell::dismiss(&s.frame);
    drop(s);
    schedule_clipboard_clear(&secret, clear_seconds);
}

/// Take the secret back off the clipboard after `seconds`, unless something
/// else has been copied in the meantime.
///
/// The waiting thread carries a SHA-256 of the secret rather than the secret
/// itself, so nothing here keeps a second copy of a password alive for the
/// length of the wait; it is only ever compared against what the clipboard
/// holds when the time is up.
fn schedule_clipboard_clear(secret: &str, seconds: u32) {
    if seconds == 0 {
        return;
    }
    let fingerprint = launchtype_core::clipboard_history::fingerprint(secret);
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(u64::from(seconds)));
        wxdragon::call_after(Box::new(move || {
            let ours = clipboard::get_text()
                .is_some_and(|text| launchtype_core::clipboard_history::fingerprint(&text) == fingerprint);
            if ours {
                clipboard::clear();
                log::info!("vault secret cleared from the clipboard");
            }
        }));
    });
}

/// Make sure the vault is open before an action that needs the key, prompting
/// if it is not (it may have auto-locked while the window was up). Returns
/// whether it is open now.
fn ensure_unlocked(shell: &SharedShell) -> bool {
    if session(shell).lock().unwrap().is_unlocked() {
        return true;
    }
    enter_vault_mode(shell);
    session(shell).lock().unwrap().is_unlocked()
}

/// Copy an account's passwords and authenticator seeds out of Bitwarden (or a
/// self-hosted Vaultwarden) and into this vault.
///
/// # Why it shells out to `bw`
///
/// Reading someone's Bitwarden vault means their key hierarchy, their KDF
/// settings and the per-item unwrapping, against a server this app has no
/// control over. Bitwarden's own CLI already does all of that and is the only
/// thing that can be trusted to keep doing it as the format moves; see
/// [`launchtype_services::bitwarden`].
///
/// # Why the network half runs on a thread
///
/// `bw login` and `bw sync` take seconds against a remote server and can take
/// a great deal longer against a slow one. The vault's own Argon2 stretch is
/// done on the UI thread because it is bounded and short; this is neither, and
/// a frozen window with no explanation is exactly the failure a screen reader
/// user cannot diagnose. So the user is told it has started, the work happens
/// off the UI thread, and the result comes back through `call_after`.
///
/// # What it will not do
///
/// It never overwrites. Anything whose name is already in the vault is left
/// exactly as it is, so importing twice is harmless and a password changed
/// here is never quietly replaced by an older one from the server.
pub fn import_from_bitwarden(shell: &SharedShell) {
    if !ensure_unlocked(shell) {
        return;
    }

    // Checked before the user is asked for anything: being told at the end
    // that the tool was never installed would waste a password prompt.
    let Some(program) = launchtype_services::bitwarden::find_cli() else {
        shell.borrow().sounds.play("error");
        return report_error(
            shell,
            &tr("Import from Bitwarden"),
            &tr("The Bitwarden command line tool (bw) is not installed, or is not on the PATH. Install it from bitwarden.com/help/cli and try again."),
        );
    };

    let (frame, server) = {
        let s = shell.borrow();
        (s.frame, s.settings.settings.bitwarden_server.clone())
    };
    let Some(fields) = dialogs::bitwarden_import_dialog(&frame, &server) else { return };

    // Remembered before the import runs, so a server that turns out to need a
    // second attempt does not have to be retyped.
    {
        let mut s = shell.borrow_mut();
        s.settings.settings.bitwarden_server = fields.server.clone();
        let _ = s.settings.save();
    }

    speak_now(&tr("Contacting the server, this can take a moment"), true);

    // A pasted session key means the user has already done the logging in, so
    // the account fields are not consulted at all.
    let auth = if fields.session.is_empty() {
        BwAuth::Login(BwCredentials {
            server: fields.server,
            email: fields.email,
            password: Zeroizing::new(fields.password),
            two_factor: fields.two_factor,
            code: fields.code,
        })
    } else {
        BwAuth::Session(Zeroizing::new(fields.session))
    };

    std::thread::spawn(move || {
        let fetched = fetch_items(&program, &auth);
        // `auth` dies here, with the master password or session key in it,
        // rather than riding back to the UI thread inside the closure below.
        drop(auth);
        wxdragon::call_after(Box::new(move || match fetched {
            Ok(items) => with_shell(|shell| apply_import(shell, items)),
            Err(message) => with_shell(|shell| {
                shell.borrow().sounds.play("error");
                report_error(shell, &tr("Import from Bitwarden"), &message);
            }),
        }));
    });
}

/// The whole of the off-thread half: get a session, pull, read. A session this
/// made is logged out on the way out of this function, whichever branch is
/// taken; a borrowed one is left alone.
fn fetch_items(
    program: &str,
    auth: &BwAuth,
) -> Result<Vec<launchtype_core::bitwarden::BwItem>, String> {
    let session = BwSession::open(program, auth).map_err(login_error_text)?;
    session.sync().map_err(login_error_text)?;
    session.items().map_err(login_error_text)
}

/// `bw` says why it refused in its own words, which are usually the clearest
/// thing available — a wrong password, a missing two-step code, a server that
/// did not answer. Only the cases where it says nothing useful get replaced.
///
/// A two-step refusal gets one sentence added rather than rewritten: that
/// failure is the one with a way out the user cannot guess at, and the message
/// is the only place they will be looking when they hit it.
fn login_error_text(error: BwError) -> String {
    match error {
        BwError::NotInstalled => tr(
            "The Bitwarden command line tool (bw) is not installed, or is not on the PATH. Install it from bitwarden.com/help/cli and try again.",
        ),
        BwError::Cli(message) => {
            if mentions_two_step(&message) {
                format!("{message}\n\n{}", tr("If your account uses a second factor that cannot be typed in here, such as Duo or a passkey, run \"bw unlock --raw\" in a terminal and paste the session key it prints into the import instead."))
            } else {
                message
            }
        }
        other => other.to_string(),
    }
}

/// Whether `bw` is complaining about a second factor. Matched loosely and used
/// only to *add* a hint: a miss costs nothing, so there is no need to keep this
/// in step with Bitwarden's exact wording.
fn mentions_two_step(message: &str) -> bool {
    let lowered = message.to_lowercase();
    ["two-step", "two step", "twofactor", "two-factor", "two factor"]
        .iter()
        .any(|needle| lowered.contains(needle))
}

/// Back on the UI thread with the vault contents: work out what would be
/// added, ask, and write it.
fn apply_import(shell: &SharedShell, items: Vec<launchtype_core::bitwarden::BwItem>) {
    // The vault may have auto-locked while the download was running.
    if !ensure_unlocked(shell) {
        return;
    }
    let vault = session(shell);
    let existing: Vec<String> =
        vault.lock().unwrap().entries().iter().map(|e| e.name.clone()).collect();
    let plan = plan_import(&items, &existing);
    drop(items);

    if plan.is_empty() {
        shell.borrow().sounds.play("error");
        return report_error(shell, &tr("Import from Bitwarden"), &nothing_to_import_text(&plan));
    }

    let frame = shell.borrow().frame;
    if !dialogs::confirm_bitwarden_import(&frame, &plan) {
        return;
    }

    let mut added = 0usize;
    let mut failed = 0usize;
    for entry in &plan.entries {
        // Imported entries get no shortcut: shortcuts must be unique to be
        // worth anything, and inventing hundreds of them would collide with
        // each other and with the ones the user chose by hand.
        let result = vault.lock().unwrap().save(None, &entry.name, "", &entry.secret, entry.kind);
        match result {
            Ok(_) => added += 1,
            Err(e) => {
                log::warn!("importing {:?} failed: {e}", entry.name);
                failed += 1;
            }
        }
    }

    let now = shell.borrow().controller.clock.now();
    vault.lock().unwrap().touch(now);
    update_list(shell);

    let mut spoken = format_args(
        &tr("{count} added to the vault"),
        &[("count", Arg::Int(added as i64))],
    );
    if failed > 0 {
        spoken.push(' ');
        spoken.push_str(&format_args(
            &tr("{count} could not be saved."),
            &[("count", Arg::Int(failed as i64))],
        ));
    }
    shell.borrow().sounds.play("match");
    speak_now(&spoken, true);
}

/// Why an import found nothing. "Nothing to import" on its own would leave the
/// user guessing between an empty account, a vault that already holds it all,
/// and an account full of things this vault cannot store.
fn nothing_to_import_text(plan: &ImportPlan) -> String {
    if plan.already_there > 0 {
        return format_args(
            &tr("Nothing new to import: all {count} logins found are already in this vault."),
            &[("count", Arg::Int(plan.already_there as i64))],
        );
    }
    if plan.unsupported > 0 {
        return format_args(
            &tr("Nothing to import: the {count} items found are cards, identities or secure notes, which this vault has no room for."),
            &[("count", Arg::Int(plan.unsupported as i64))],
        );
    }
    tr("Nothing to import: that account holds no logins with a password or an authenticator seed in them.")
}
