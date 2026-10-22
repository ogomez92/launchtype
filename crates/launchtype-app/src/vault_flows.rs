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
//!
//! Files and folders kept in the vault are never opened from it. Enter on one
//! asks where to save it and decrypts it there, so the only plaintext copy is
//! one the user put somewhere on purpose.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use launchtype_core::bitwarden::{plan_import, BwItem, ImportPlan};
use launchtype_core::bitwarden_export::{read_export, Export, ExportError, ProtectedExport};
use launchtype_core::i18n::{format_args, tr, Arg};
use launchtype_core::totp::Totp;
use launchtype_core::vault::{EntryKind, VaultError, VaultSession};
use launchtype_core::vault_blob::MAX_BLOB_BYTES;
use launchtype_services::clipboard;
use wxdragon::dialogs::dir_dialog::DirDialog;
use wxdragon::dialogs::file_dialog::{FileDialog, FileDialogStyle};
use zeroize::Zeroizing;

use crate::dialogs::{self, VaultEntryFields};
use crate::shell::{report_error, update_list, SharedShell};
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
        VaultError::TooLarge => format_args(
            &tr("That is too large to keep in the vault. The limit is {size} MB."),
            &[("size", Arg::Int((MAX_BLOB_BYTES / (1024 * 1024)) as i64))],
        ),
        VaultError::WrongKind => tr("That entry does not hold a file."),
        VaultError::InsideVault => tr("The vault folder cannot be put inside the vault."),
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
        "import" => import_bitwarden_export(shell),
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
///
/// Picking a file or a folder as the kind goes on to ask which one, after
/// the name and shortcut have been typed.
pub fn add_entry(shell: &SharedShell) {
    if !ensure_unlocked(shell) {
        return;
    }
    let frame = shell.borrow().frame;
    let Some(fields) = dialogs::vault_entry_dialog(&frame, None) else { return };
    if fields.kind.holds_file() {
        let Some(source) = pick_source(shell, fields.kind) else { return };
        return store_copy(shell, &source, &fields);
    }
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
    // A file has no text to show, so only its name and shortcut are edited.
    if info.kind.holds_file() {
        let frame = shell.borrow().frame;
        let note = tr("Pressing Enter on this entry asks where to save a copy of it.");
        let title = tr("Edit vault entry");
        let Some(fields) =
            dialogs::vault_file_entry_dialog(&frame, &title, &note, &info.name, &info.shortcut)
        else {
            return;
        };
        let result = session(shell).lock().unwrap().rename(id, &fields.name, &fields.shortcut);
        return finish_write(shell, result, tr("Vault entry saved"));
    }
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
    finish_write(shell, result.map(drop), spoken);
}

/// Say how a change to the vault went, and show the list as it is now.
fn finish_write(shell: &SharedShell, result: Result<(), VaultError>, spoken: String) {
    match result {
        Ok(()) => {
            shell.borrow().sounds.play("match");
            speak_now(&spoken, true);
            update_list(shell);
        }
        Err(error) => report(shell, &error),
    }
}

/// Ask which file, or which folder, to put in the vault. A copy goes in; the
/// original is not touched, and deleting it is the user's call.
fn pick_source(shell: &SharedShell, kind: EntryKind) -> Option<PathBuf> {
    let frame = shell.borrow().frame;
    let path = if kind == EntryKind::Folder {
        let dir_dialog =
            DirDialog::builder(&frame, &tr("Choose a folder to keep in the vault"), "").build();
        if dir_dialog.show_modal() != wxdragon::id::ID_OK {
            return None;
        }
        dir_dialog.get_path()
    } else {
        let file_dialog = FileDialog::builder(&frame)
            .with_message(&tr("Choose a file to keep in the vault"))
            .with_wildcard(&tr("All files (*.*)|*.*"))
            .with_style(FileDialogStyle::Open | FileDialogStyle::FileMustExist)
            .build();
        if file_dialog.show_modal() != wxdragon::id::ID_OK {
            return None;
        }
        file_dialog.get_path()
    };
    path.map(PathBuf::from)
}

fn source_name(source: &Path) -> String {
    source.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Encrypt the file or folder at `source` into the vault as `fields.kind`,
/// named `fields.name`, or after itself if that was left empty.
fn store_copy(shell: &SharedShell, source: &Path, fields: &VaultEntryFields) {
    // The dialogs give the auto-locker plenty of time to have run.
    if !ensure_unlocked(shell) {
        return;
    }
    let name = if fields.name.is_empty() { source_name(source) } else { fields.name.clone() };
    // Encrypting a large folder takes a moment, and happens here rather than
    // on a thread like unlocking does: the vault is unusable until it is in.
    let vault = session(shell);
    let result = {
        let mut vault = vault.lock().unwrap();
        match fields.kind {
            EntryKind::Folder => vault.add_folder(&name, &fields.shortcut, source),
            _ => vault.add_file(&name, &fields.shortcut, source),
        }
    };
    finish_write(shell, result.map(drop), tr("Added to the vault"));
}

/// Enter on a file or folder entry: ask where to put a decrypted copy, and
/// write it there. A file is saved under a name the user picks; a folder is
/// recreated inside the folder they pick, next to (never over) anything of
/// the same name.
///
/// The dialogs name it by its original file name, which is what it is about
/// to be saved as, rather than by its row in the list.
fn save_copy(shell: &SharedShell, id: &str, kind: EntryKind) {
    let vault = session(shell);
    let original = vault.lock().unwrap().original_name(id);
    let original = match original {
        Ok(original) => original,
        Err(error) => return report(shell, &error),
    };
    let frame = shell.borrow().frame;
    let chosen = if kind == EntryKind::Folder {
        let message =
            format_args(&tr("Choose where to save the folder {name}"), &[("name", Arg::Str(&original))]);
        let dir_dialog = DirDialog::builder(&frame, &message, "").build();
        if dir_dialog.show_modal() != wxdragon::id::ID_OK {
            return;
        }
        dir_dialog.get_path()
    } else {
        let message = format_args(&tr("Save {name} as"), &[("name", Arg::Str(&original))]);
        let file_dialog = FileDialog::builder(&frame)
            .with_message(&message)
            .with_default_file(&original)
            .with_wildcard(&tr("All files (*.*)|*.*"))
            .with_style(FileDialogStyle::Save | FileDialogStyle::OverwritePrompt)
            .build();
        if file_dialog.show_modal() != wxdragon::id::ID_OK {
            return;
        }
        file_dialog.get_path()
    };
    let Some(chosen) = chosen else { return };
    let chosen = PathBuf::from(chosen);
    if !ensure_unlocked(shell) {
        return;
    }

    let result = {
        let vault = vault.lock().unwrap();
        match kind {
            EntryKind::Folder => vault.export_folder(id, &chosen),
            _ => vault.export_file(id, &chosen).map(|()| chosen),
        }
    };
    let saved = match result {
        Ok(saved) => saved,
        Err(error) => return report(shell, &error),
    };
    after_use(shell, &vault);
    shell.borrow().sounds.play("match");
    // The full path, because a folder may have landed as "name (2)".
    speak_now(
        &format_args(&tr("Saved to {path}"), &[("path", Arg::Str(&saved.to_string_lossy()))]),
        true,
    );
}

/// Using an entry counts as activity, and with the timeout at zero it is
/// also the moment to lock again.
fn after_use(shell: &SharedShell, vault: &Arc<Mutex<VaultSession>>) {
    let now = shell.borrow().controller.clock.now();
    let mut vault = vault.lock().unwrap();
    vault.touch(now);
    if vault.locks_on_use() {
        vault.lock();
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
/// but it is never what the user wants to paste. A file or folder entry is not
/// copied at all, but saved somewhere; see [`save_copy`].
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
        EntryKind::File | EntryKind::Folder => return save_copy(shell, id, info.kind),
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

    let (clipboard_history, clear_seconds) = {
        let s = shell.borrow();
        (s.controller.clipboard.clone(), s.settings.settings.vault_clipboard_seconds)
    };
    // Before the clipboard is written, not after: the history poller runs on
    // its own thread every 100ms and would otherwise be free to record the
    // secret in the window between the two.
    clipboard_history.lock().unwrap().suppress(&secret);
    clipboard::set_text(&secret);

    after_use(shell, &vault);

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

/// Copy everything out of a file made by Bitwarden's "Export vault" and into
/// this vault. See [`launchtype_core::bitwarden`] for what each item becomes,
/// and [`launchtype_core::bitwarden_export`] for the file formats.
///
/// # Keeping the file's contents short-lived
///
/// An unencrypted export is the user's whole password list in the clear. The
/// file is read into a [`Zeroizing`] buffer that is wiped as soon as the items
/// have been read out of it, and nothing about its contents is logged.
///
/// # What it will not do
///
/// It never overwrites. Anything whose name is already in the vault is left
/// exactly as it is, so importing twice is harmless and a password changed
/// here is never quietly replaced by an older one from the export.
pub fn import_bitwarden_export(shell: &SharedShell) {
    if !ensure_unlocked(shell) {
        return;
    }
    let frame = shell.borrow().frame;
    let file_dialog = FileDialog::builder(&frame)
        .with_message(&tr("Choose a Bitwarden export file"))
        .with_wildcard(&tr("Bitwarden exports (*.json;*.csv)|*.json;*.csv|All files (*.*)|*.*"))
        .with_style(FileDialogStyle::Open | FileDialogStyle::FileMustExist)
        .build();
    if file_dialog.show_modal() != wxdragon::id::ID_OK {
        return;
    }
    let Some(path) = file_dialog.get_path() else { return };

    let bytes = match std::fs::read(&path) {
        Ok(bytes) => Zeroizing::new(bytes),
        Err(error) => return import_failed(shell, &error.to_string()),
    };
    let export = read_export(&bytes);
    drop(bytes);

    let items = match export {
        Ok(Export::Items(items)) => items,
        Ok(Export::PasswordProtected(sealed)) => match unlock_export(shell, &sealed) {
            Some(items) => items,
            None => return,
        },
        Err(error) => return import_failed(shell, &export_error_text(error)),
    };
    apply_import(shell, items);
}

/// Ask for the export password until it opens the file or the user gives up.
fn unlock_export(shell: &SharedShell, sealed: &ProtectedExport) -> Option<Vec<BwItem>> {
    let frame = shell.borrow().frame;
    loop {
        let password = Zeroizing::new(dialogs::export_password_dialog(&frame)?);
        // Stretching takes a second or so at Bitwarden's defaults; like the
        // vault's own unlock it is bounded, so it runs here.
        match sealed.unlock(&password) {
            Ok(items) => return Some(items),
            // Said in a box rather than spoken: the password prompt comes
            // straight back, and its own announcement would talk over speech.
            Err(error) => {
                import_failed(shell, &export_error_text(error));
                if error != ExportError::WrongPassword {
                    return None;
                }
            }
        }
    }
}

fn import_failed(shell: &SharedShell, message: &str) {
    shell.borrow().sounds.play("error");
    report_error(shell, &tr("Import a Bitwarden export"), message);
}

/// Each refusal says what to do about it, which for most of them is to export
/// again in a form that can be read.
fn export_error_text(error: ExportError) -> String {
    match error {
        ExportError::NotAnExport => tr(
            "That file is not a Bitwarden export. In Bitwarden, use Export vault and choose the .json or .csv format.",
        ),
        ExportError::AccountRestricted => tr(
            "That export is account restricted, so only Bitwarden itself can open it. Export again and choose either the unencrypted .json format or a password-protected export.",
        ),
        ExportError::Zip => tr(
            "That is the .zip export with attachments, which cannot be read here. Export again in the .json format.",
        ),
        ExportError::UnsupportedKdf => tr(
            "That export is protected with settings this version cannot open. Export again, either unencrypted or protected with a password.",
        ),
        ExportError::WrongPassword => tr("That is not the password this export was protected with."),
        ExportError::Damaged => tr("That export is damaged and could not be read."),
    }
}

/// Work out what would be added, ask, and write it.
fn apply_import(shell: &SharedShell, items: Vec<BwItem>) {
    // The vault may have auto-locked while the file dialog was open.
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
        return report_error(shell, &tr("Import a Bitwarden export"), &nothing_to_import_text(&plan));
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
/// user guessing between an empty export, a vault that already holds it all,
/// and an export full of things this vault cannot store.
fn nothing_to_import_text(plan: &ImportPlan) -> String {
    if plan.already_there > 0 {
        return format_args(
            &tr("Nothing new to import: all {count} entries found are already in this vault."),
            &[("count", Arg::Int(plan.already_there as i64))],
        );
    }
    if plan.unsupported > 0 {
        return format_args(
            &tr("Nothing to import: the {count} items found are of a kind this version does not know."),
            &[("count", Arg::Int(plan.unsupported as i64))],
        );
    }
    tr("Nothing to import: that export holds no passwords, codes or other details to copy.")
}
