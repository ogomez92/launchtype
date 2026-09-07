//! Turning a Bitwarden (or Vaultwarden) vault into entries for the `*` vault.
//!
//! This module is the pure half of the import: it takes the JSON that
//! `bw list items` prints and works out what would be added, without touching
//! the network, the disk or the `bw` process — [`crate::bitwarden`] decides,
//! `launchtype_services::bitwarden` fetches. That split is what lets the
//! interesting decisions below be tested without a server.
//!
//! # What comes across, and what does not
//!
//! Login items only. Cards, identities and secure notes have a shape the vault
//! has no room for — an entry is one name and one secret — and inventing four
//! entries out of a credit card would be worse than leaving it where it works.
//!
//! Each login can yield up to two entries, because a password and a second
//! factor are two different things to copy:
//!
//! * the password, named after the item;
//! * the authenticator seed, named after the item with a "code" marker, stored
//!   as [`EntryKind::Totp`] so Enter copies the six digits rather than the seed.
//!
//! # Why the username ends up in the name
//!
//! A vault entry has no username field, and two accounts on the same site are
//! very common — a personal and a work GitHub, say. Folding the username into
//! the name ("GitHub (oriol)") is what makes them tellable apart in a list that
//! is often being read aloud rather than looked at.
//!
//! # Re-running the import
//!
//! Importing twice must not double the vault, so anything whose name is
//! already taken is counted as skipped rather than added. Names are compared
//! the way the rest of the app compares them: case-insensitively, trimmed.
//! This deliberately does not try to *update* an existing entry — a password
//! changed on this machine would be silently overwritten by a stale one from
//! the server, and quietly losing a secret is the one outcome worth designing
//! against.

use serde::Deserialize;
use zeroize::Zeroizing;

use crate::i18n::{format_args, tr, Arg};
use crate::totp::Totp;
use crate::vault::EntryKind;

/// Bitwarden's item type discriminator; 1 is a login, and the rest (secure
/// note, card, identity) have no vault entry to become.
const TYPE_LOGIN: u8 = 1;

/// One item as `bw list items` prints it, cut down to the fields the import
/// reads. Everything is optional because a self-hosted Vaultwarden of any age
/// may omit fields a current Bitwarden always sends.
#[derive(Debug, Clone, Deserialize)]
pub struct BwItem {
    #[serde(default)]
    pub name: String,
    #[serde(rename = "type", default)]
    pub item_type: u8,
    #[serde(default)]
    pub login: Option<BwLogin>,
    /// Set on items sitting in the trash, which must not be imported.
    #[serde(rename = "deletedDate", default)]
    pub deleted_date: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BwLogin {
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    /// Either a bare base32 seed or a whole `otpauth://` URI, depending on how
    /// it was originally entered.
    #[serde(default)]
    pub totp: Option<String>,
}

/// An entry the import would add. The secret is in a [`Zeroizing`] buffer: a
/// plan is built, shown as a count, and applied, and for that whole time it is
/// the user's entire password list sitting in this process.
#[derive(Debug)]
pub struct PlannedEntry {
    pub name: String,
    pub secret: Zeroizing<String>,
    pub kind: EntryKind,
}

/// What an import would do, worked out before anything is written so the user
/// can be told and can say no.
#[derive(Debug, Default)]
pub struct ImportPlan {
    pub entries: Vec<PlannedEntry>,
    /// Items skipped because an entry of that name is already in the vault.
    pub already_there: usize,
    /// Login items carrying neither a password nor a seed.
    pub empty: usize,
    /// Non-login items: cards, identities, secure notes.
    pub unsupported: usize,
    /// Names whose authenticator seed could not be read. Kept as names rather
    /// than a count because this is the one failure the user may want to go
    /// and fix by hand.
    pub unreadable_codes: Vec<String>,
}

impl ImportPlan {
    pub fn passwords(&self) -> usize {
        self.entries.iter().filter(|e| e.kind == EntryKind::Password).count()
    }

    pub fn codes(&self) -> usize {
        self.entries.iter().filter(|e| e.kind == EntryKind::Totp).count()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Parse the JSON array `bw list items` writes to stdout.
pub fn parse_items(json: &str) -> Result<Vec<BwItem>, serde_json::Error> {
    serde_json::from_str(json)
}

/// Work out which entries `items` would add to a vault that already holds
/// `existing_names`.
///
/// `existing_names` is what the caller reads off the unlocked session; passing
/// it in rather than taking a `VaultSession` keeps this testable and keeps the
/// vault's lock out of a function that does no I/O.
pub fn plan_import(items: &[BwItem], existing_names: &[String]) -> ImportPlan {
    let mut plan = ImportPlan::default();
    // Names already taken, plus the ones this batch claims as it goes, so two
    // Bitwarden items that fold down to the same name do not both go in.
    let mut taken: Vec<String> = existing_names.iter().map(|n| fold_name(n)).collect();

    for item in items {
        if item.deleted_date.is_some() {
            continue;
        }
        if item.item_type != TYPE_LOGIN {
            plan.unsupported += 1;
            continue;
        }
        let Some(login) = &item.login else {
            plan.empty += 1;
            continue;
        };

        let base = entry_name(&item.name, login.username.as_deref());
        let password = non_empty(login.password.as_deref());
        let seed = non_empty(login.totp.as_deref());
        if password.is_none() && seed.is_none() {
            plan.empty += 1;
            continue;
        }

        if let Some(password) = password {
            push(&mut plan, &mut taken, base.clone(), password, EntryKind::Password);
        }

        if let Some(seed) = seed {
            // Refused here rather than at copy time: an entry that can never
            // produce a code is worse than being told now which item to fix.
            if Totp::parse(seed).is_err() {
                plan.unreadable_codes.push(base.clone());
                continue;
            }
            let name = format_args(&tr("{name} (code)"), &[("name", Arg::Str(&base))]);
            push(&mut plan, &mut taken, name, seed, EntryKind::Totp);
        }
    }

    plan
}

fn push(
    plan: &mut ImportPlan,
    taken: &mut Vec<String>,
    name: String,
    secret: &str,
    kind: EntryKind,
) {
    let folded = fold_name(&name);
    if taken.contains(&folded) {
        plan.already_there += 1;
        return;
    }
    taken.push(folded);
    plan.entries.push(PlannedEntry { name, secret: Zeroizing::new(secret.to_string()), kind });
}

/// The name an item becomes: its own, with the username in brackets when there
/// is one to tell it apart by.
fn entry_name(item_name: &str, username: Option<&str>) -> String {
    let base = item_name.trim();
    let base = if base.is_empty() { tr("Untitled") } else { base.to_string() };
    match non_empty(username) {
        // A username that only repeats the item name adds nothing to read out.
        Some(user) if !user.eq_ignore_ascii_case(&base) => {
            format_args(&tr("{name} ({user})"), &[("name", Arg::Str(&base)), ("user", Arg::Str(user))])
        }
        _ => base,
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|v| !v.is_empty())
}

fn fold_name(name: &str) -> String {
    name.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn login(name: &str, username: Option<&str>, password: Option<&str>, totp: Option<&str>) -> BwItem {
        BwItem {
            name: name.to_string(),
            item_type: TYPE_LOGIN,
            login: Some(BwLogin {
                username: username.map(str::to_string),
                password: password.map(str::to_string),
                totp: totp.map(str::to_string),
            }),
            deleted_date: None,
        }
    }

    const SEED: &str = "JBSWY3DPEHPK3PXP";

    fn names(plan: &ImportPlan) -> Vec<&str> {
        plan.entries.iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn a_login_with_a_password_becomes_one_entry() {
        let items = [login("GitHub", Some("oriol"), Some("hunter2"), None)];
        let plan = plan_import(&items, &[]);
        assert_eq!(names(&plan), ["GitHub (oriol)"]);
        assert_eq!(&*plan.entries[0].secret, "hunter2");
        assert_eq!(plan.entries[0].kind, EntryKind::Password);
        assert_eq!(plan.passwords(), 1);
        assert_eq!(plan.codes(), 0);
    }

    #[test]
    fn a_login_with_both_becomes_a_password_and_a_code() {
        let items = [login("GitHub", Some("oriol"), Some("hunter2"), Some(SEED))];
        let plan = plan_import(&items, &[]);
        assert_eq!(names(&plan), ["GitHub (oriol)", "GitHub (oriol) (code)"]);
        assert_eq!(plan.entries[0].kind, EntryKind::Password);
        assert_eq!(plan.entries[1].kind, EntryKind::Totp);
        assert_eq!(&*plan.entries[1].secret, SEED);
    }

    #[test]
    fn a_login_with_only_a_seed_still_comes_across() {
        let items = [login("Bank", None, None, Some(SEED))];
        let plan = plan_import(&items, &[]);
        assert_eq!(names(&plan), ["Bank (code)"]);
        assert_eq!(plan.passwords(), 0);
        assert_eq!(plan.codes(), 1);
    }

    #[test]
    fn the_username_is_left_off_when_it_would_only_repeat_the_name() {
        let items = [login("oriol", Some("oriol"), Some("x"), None)];
        assert_eq!(names(&plan_import(&items, &[])), ["oriol"]);
    }

    #[test]
    fn an_item_with_no_username_keeps_its_plain_name() {
        let items = [login("Bank", None, Some("x"), None)];
        assert_eq!(names(&plan_import(&items, &[])), ["Bank"]);
        let blank = [login("Bank", Some("   "), Some("x"), None)];
        assert_eq!(names(&plan_import(&blank, &[])), ["Bank"]);
    }

    #[test]
    fn two_accounts_on_one_site_stay_tellable_apart() {
        let items = [
            login("GitHub", Some("me"), Some("a"), None),
            login("GitHub", Some("work"), Some("b"), None),
        ];
        assert_eq!(names(&plan_import(&items, &[])), ["GitHub (me)", "GitHub (work)"]);
    }

    #[test]
    fn re_running_the_import_adds_nothing_and_overwrites_nothing() {
        let items = [login("GitHub", Some("oriol"), Some("hunter2"), Some(SEED))];
        let first = plan_import(&items, &[]);
        let existing: Vec<String> = first.entries.iter().map(|e| e.name.clone()).collect();

        let second = plan_import(&items, &existing);
        assert!(second.is_empty(), "nothing new the second time");
        assert_eq!(second.already_there, 2);
    }

    #[test]
    fn an_existing_name_is_matched_however_it_was_capitalised() {
        let items = [login("GitHub", None, Some("x"), None)];
        let plan = plan_import(&items, &["  github ".to_string()]);
        assert!(plan.is_empty());
        assert_eq!(plan.already_there, 1);
    }

    #[test]
    fn two_items_folding_to_the_same_name_do_not_both_go_in() {
        // Same site, same username, entered twice over the years.
        let items = [
            login("GitHub", Some("oriol"), Some("old"), None),
            login("github", Some("Oriol"), Some("new"), None),
        ];
        let plan = plan_import(&items, &[]);
        assert_eq!(plan.entries.len(), 1);
        assert_eq!(&*plan.entries[0].secret, "old");
        assert_eq!(plan.already_there, 1);
    }

    #[test]
    fn cards_and_notes_are_counted_but_not_invented_into_entries() {
        let mut card = login("Visa", None, Some("x"), None);
        card.item_type = 3;
        let mut note = login("Recovery codes", None, None, None);
        note.item_type = 2;
        let plan = plan_import(&[card, note], &[]);
        assert!(plan.is_empty());
        assert_eq!(plan.unsupported, 2);
    }

    #[test]
    fn trashed_items_are_not_resurrected_by_the_import() {
        let mut deleted = login("Old thing", None, Some("x"), None);
        deleted.deleted_date = Some("2026-01-01T00:00:00.000Z".to_string());
        let plan = plan_import(&[deleted], &[]);
        assert!(plan.is_empty());
        // Not "unsupported" either: it was a login, it just is not there any more.
        assert_eq!(plan.unsupported, 0);
        assert_eq!(plan.empty, 0);
    }

    #[test]
    fn a_login_holding_neither_secret_is_counted_as_empty() {
        let items = [login("Just a bookmark", Some("me"), None, None), login("Nothing", None, Some("  "), Some(""))];
        let plan = plan_import(&items, &[]);
        assert!(plan.is_empty());
        assert_eq!(plan.empty, 2);
    }

    #[test]
    fn an_unreadable_seed_is_reported_by_name_rather_than_stored() {
        let items = [login("Weird", None, Some("hunter2"), Some("not base32!"))];
        let plan = plan_import(&items, &[]);
        // The password still comes across; only the seed is refused.
        assert_eq!(names(&plan), ["Weird"]);
        assert_eq!(plan.unreadable_codes, ["Weird"]);
    }

    #[test]
    fn an_otpauth_uri_is_accepted_as_readily_as_a_bare_seed() {
        let items = [login("Amazon", None, None, Some("otpauth://totp/Amazon?secret=JBSWY3DPEHPK3PXP"))];
        let plan = plan_import(&items, &[]);
        assert_eq!(plan.codes(), 1);
    }

    #[test]
    fn an_item_with_no_name_gets_a_placeholder_rather_than_an_empty_row() {
        let items = [login("   ", None, Some("x"), None)];
        assert_eq!(names(&plan_import(&items, &[])), ["Untitled"]);
    }

    /// The shape `bw list items` actually prints, including the fields this
    /// module ignores, so an added field upstream does not break the parse.
    #[test]
    fn the_real_cli_output_shape_parses() {
        let json = r#"[
          {
            "object": "item",
            "id": "8f1c...",
            "organizationId": null,
            "folderId": null,
            "type": 1,
            "reprompt": 0,
            "name": "GitHub",
            "notes": "some notes",
            "favorite": false,
            "login": {
              "fido2Credentials": [],
              "uris": [{"match": null, "uri": "https://github.com"}],
              "username": "oriol",
              "password": "hunter2",
              "totp": "JBSWY3DPEHPK3PXP",
              "passwordRevisionDate": null
            },
            "collectionIds": [],
            "revisionDate": "2026-08-11T09:00:00.000Z",
            "creationDate": "2024-01-01T00:00:00.000Z",
            "deletedDate": null
          },
          {
            "object": "item",
            "id": "aa22...",
            "type": 2,
            "name": "A secure note",
            "notes": "text",
            "secureNote": {"type": 0},
            "deletedDate": null
          }
        ]"#;
        let items = parse_items(json).expect("the documented shape parses");
        assert_eq!(items.len(), 2);
        let plan = plan_import(&items, &[]);
        assert_eq!(names(&plan), ["GitHub (oriol)", "GitHub (oriol) (code)"]);
        assert_eq!(plan.unsupported, 1);
    }

    #[test]
    fn an_item_missing_the_login_object_entirely_does_not_panic() {
        let items = parse_items(r#"[{"type": 1, "name": "Odd"}]"#).unwrap();
        let plan = plan_import(&items, &[]);
        assert!(plan.is_empty());
        assert_eq!(plan.empty, 1);
    }

    #[test]
    fn an_empty_vault_export_is_an_empty_plan_not_an_error() {
        let items = parse_items("[]").unwrap();
        assert!(plan_import(&items, &[]).is_empty());
    }
}
