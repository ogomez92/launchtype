//! Not-installed Steam games mode (`;`) — the UI half of the Web API calls in
//! [`launchtype_services::steam`].
//!
//! Entering the mode asks Steam which games the account owns, in the
//! background, and lists the ones none of this machine's Steam libraries
//! holds; Enter on one opens Steam's install dialog for it. The first time,
//! it asks for the Steam Web API key that takes, and for the account too when
//! it cannot be read off the Steam the library belongs to.

use std::path::PathBuf;

use launchtype_core::i18n::{format_args, tr, Arg};
use launchtype_core::mode::UiMode;
use launchtype_core::steam::{not_installed, parse_steam_account, SteamGame};
use launchtype_services::steam::{
    installed_appids, owned_games, resolve_account, signed_in_account, SteamApiError,
};
use wxdragon::WxWidget;

use crate::controller::SteamMissingState;
use crate::shell::{update_list, with_shell, with_window_up, SharedShell};
use crate::speech::speak_now;

/// Called right after `;` switches the mode: fetch the games, asking for the
/// key first if there is none yet. Cancelling leaves the mode's "Enter a Steam
/// Web API key" row there to try again.
pub fn enter_mode(shell: &SharedShell) {
    if shell.borrow().settings.settings.steam_api_key.trim().is_empty() {
        shell.borrow_mut().controller.steam_missing = SteamMissingState::NotSetUp;
        update_list(shell);
        if !set_up(shell) {
            return;
        }
    }
    refresh(shell);
}

/// Enter on one of the mode's action rows.
pub fn run_action(shell: &SharedShell, action: &str) {
    match action {
        "setup" if set_up(shell) => refresh(shell),
        "refresh" => refresh(shell),
        "loading" => speak_now(&tr("Still asking Steam which games you own"), true),
        _ => {}
    }
}

/// The account the games are asked about: the one set in Settings, or else
/// the one signed in to the Steam the library belongs to.
fn has_account(shell: &SharedShell) -> bool {
    let s = shell.borrow();
    !s.settings.settings.steam_account.trim().is_empty()
        || signed_in_account(&s.controller.steam_library).is_some()
}

/// Ask for the key, and for the account when there is no telling which it
/// is. Both are kept in settings.json.
fn set_up(shell: &SharedShell) -> bool {
    let ask_account = !has_account(shell);
    let (key, account) = {
        let s = shell.borrow();
        let settings = &s.settings.settings;
        (settings.steam_api_key.clone(), settings.steam_account.clone())
    };
    let Some((key, account)) = with_window_up(shell, |frame| {
        crate::dialogs::steam_api_key_dialog(frame, &key, &account, ask_account)
    }) else {
        return false;
    };
    let mut s = shell.borrow_mut();
    s.settings.settings.steam_api_key = key;
    if let Some(account) = account {
        s.settings.settings.steam_account = account;
    }
    let _ = s.settings.save();
    true
}

/// Everything the background fetch needs, read off the shell before it goes.
struct Request {
    api_key: String,
    account: String,
    library: PathBuf,
}

/// Ask Steam, then scan the libraries; the scan comes second so a game that
/// finished installing while the request was out is not offered.
fn fetch(request: &Request) -> Result<Vec<SteamGame>, SteamApiError> {
    let account = parse_steam_account(&request.account)
        .or_else(|| signed_in_account(&request.library).and_then(|id| parse_steam_account(&id)))
        .ok_or_else(|| SteamApiError {
            message: tr("There is no telling which Steam account is yours. Enter it along with the key."),
            key_rejected: false,
        })?;
    let steam_id = resolve_account(&request.api_key, &account)?;
    let owned = owned_games(&request.api_key, &steam_id)?;
    Ok(not_installed(owned, &installed_appids(&request.library)))
}

/// Fetch the games again on a background thread. The list says so meanwhile,
/// and the result is announced when it lands — unless the user has moved on
/// to another mode, in which case it is only kept for next time.
fn refresh(shell: &SharedShell) {
    let request = {
        let mut s = shell.borrow_mut();
        s.controller.steam_missing = SteamMissingState::Loading;
        let settings = &s.settings.settings;
        Request {
            api_key: settings.steam_api_key.trim().to_string(),
            account: settings.steam_account.clone(),
            library: s.controller.steam_library.clone(),
        }
    };
    update_list(shell);
    std::thread::spawn(move || {
        let result = fetch(&request);
        wxdragon::call_after(Box::new(move || {
            with_shell(|shell| {
                let announcement = match &result {
                    Ok(games) if games.is_empty() => tr("Every game you own is installed"),
                    Ok(games) => format_args(
                        &tr("{count} games not installed"),
                        &[("count", Arg::Int(games.len() as i64))],
                    ),
                    Err(error) => error.message.clone(),
                };
                let showing = {
                    let mut s = shell.borrow_mut();
                    s.controller.steam_missing = match result {
                        Ok(games) => SteamMissingState::Loaded(games),
                        Err(error) => {
                            // A rejected key is forgotten, so the next visit
                            // asks for a fresh one instead of failing the same way.
                            if error.key_rejected {
                                s.settings.settings.steam_api_key.clear();
                                let _ = s.settings.save();
                            }
                            SteamMissingState::Failed(error.message)
                        }
                    };
                    s.mode == UiMode::SteamNotInstalled && s.frame.is_shown()
                };
                if !showing {
                    return;
                }
                update_list(shell);
                speak_now(&announcement, true);
            });
        }));
    });
}
