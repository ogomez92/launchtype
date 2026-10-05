//! Turning Bitwarden items into entries for the `*` vault.
//!
//! This module is the pure half of the import: it takes items in the shape a
//! Bitwarden export holds them — see [`crate::bitwarden_export`] for getting
//! them out of the file — and works out what would be added, without touching
//! the disk. That split is what lets the interesting decisions below be tested
//! without a real export lying around.
//!
//! # One entry per thing worth copying
//!
//! A vault entry is one name and one secret, and Enter copies the secret. A
//! Bitwarden item is several things at once, so each one that is worth pasting
//! somewhere becomes an entry of its own, named after the item with what it is
//! in brackets:
//!
//! * a login's password, named after the item alone;
//! * its authenticator seed, with a "code" marker, stored as
//!   [`EntryKind::Totp`] so Enter copies the current code rather than the seed.
//!   Keeping it apart from the password is what lets the code be pulled on its
//!   own when that is all a site is asking for;
//! * a card's number, security code, expiry and cardholder;
//! * each filled-in identity field, with the address lines joined into one;
//! * a secure note's text, named after the note alone;
//! * an SSH key's private and public halves;
//! * any item's notes, and its text and hidden custom fields, named after the
//!   field.
//!
//! Left behind: website addresses and login usernames, which are not secrets
//! and are already in the entry name; checkbox and linked custom fields, which
//! hold nothing to paste; passkeys, which only work inside a browser talking to
//! the site; and attachments, which a JSON or CSV export does not carry.
//!
//! # Why the username ends up in the name
//!
//! Two accounts on the same site are very common — a personal and a work
//! GitHub, say. Folding the username into the name ("GitHub (oriol)") is what
//! makes them tellable apart in a list that is often being read aloud rather
//! than looked at.
//!
//! # Re-running the import
//!
//! Importing twice must not double the vault, so anything whose name is
//! already taken is counted as skipped rather than added. Names are compared
//! the way the rest of the app compares them: case-insensitively, trimmed.
//! This deliberately does not try to *update* an existing entry — a password
//! changed on this machine would be silently overwritten by a stale one from
//! an old export, and quietly losing a secret is the one outcome worth
//! designing against.

use serde::{Deserialize, Deserializer};
use zeroize::Zeroizing;

use crate::i18n::{format_args, tr, Arg};
use crate::totp::Totp;
use crate::vault::EntryKind;

/// Bitwarden's item type discriminators.
pub(crate) const TYPE_LOGIN: u8 = 1;
pub(crate) const TYPE_SECURE_NOTE: u8 = 2;
const TYPE_CARD: u8 = 3;
const TYPE_IDENTITY: u8 = 4;
const TYPE_SSH_KEY: u8 = 5;

/// Custom field types: 0 is plain text and 1 is hidden, both worth copying;
/// 2 is a checkbox and 3 a link to another field of the same item.
const FIELD_TEXT: u8 = 0;
const FIELD_HIDDEN: u8 = 1;

/// One item as a Bitwarden export holds it, cut down to the fields the import
/// reads. Everything is optional because an export from a self-hosted
/// Vaultwarden of any age may omit fields a current Bitwarden always writes.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BwItem {
    #[serde(default, deserialize_with = "null_as_default")]
    pub name: String,
    #[serde(rename = "type", default)]
    pub item_type: u8,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub fields: Vec<BwField>,
    #[serde(default)]
    pub login: Option<BwLogin>,
    #[serde(default)]
    pub card: Option<BwCard>,
    #[serde(default)]
    pub identity: Option<BwIdentity>,
    #[serde(default)]
    pub ssh_key: Option<BwSshKey>,
    /// Set on items sitting in the trash, which must not be imported.
    #[serde(default)]
    pub deleted_date: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BwLogin {
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    /// A bare base32 seed, a whole `otpauth://` URI or a `steam://` seed,
    /// depending on how it was originally entered.
    #[serde(default)]
    pub totp: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BwCard {
    #[serde(default)]
    pub cardholder_name: Option<String>,
    #[serde(default)]
    pub number: Option<String>,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub exp_month: Option<String>,
    #[serde(default)]
    pub exp_year: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BwIdentity {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub first_name: Option<String>,
    #[serde(default)]
    pub middle_name: Option<String>,
    #[serde(default)]
    pub last_name: Option<String>,
    #[serde(default)]
    pub address1: Option<String>,
    #[serde(default)]
    pub address2: Option<String>,
    #[serde(default)]
    pub address3: Option<String>,
    #[serde(default)]
    pub city: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub postal_code: Option<String>,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub company: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub ssn: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub passport_number: Option<String>,
    #[serde(default)]
    pub license_number: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BwSshKey {
    #[serde(default)]
    pub private_key: Option<String>,
    #[serde(default)]
    pub public_key: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BwField {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(rename = "type", default)]
    pub field_type: u8,
}

/// Bitwarden writes `null` rather than leaving a field out, `"fields": null`
/// on items that never had any being the usual one.
fn null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// What an entry holds, for the counts the user is shown before anything is
/// written. All but [`Category::Code`] are stored as [`EntryKind::Password`]:
/// Enter copies them as they are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Password,
    Code,
    Other,
}

/// An entry the import would add. The secret is in a [`Zeroizing`] buffer: a
/// plan is built, shown as a count, and applied, and for that whole time it is
/// the user's entire password list sitting in this process.
#[derive(Debug)]
pub struct PlannedEntry {
    pub name: String,
    pub secret: Zeroizing<String>,
    pub kind: EntryKind,
    pub category: Category,
}

/// What an import would do, worked out before anything is written so the user
/// can be told and can say no.
#[derive(Debug, Default)]
pub struct ImportPlan {
    pub entries: Vec<PlannedEntry>,
    /// Entries skipped because one of that name is already in the vault.
    pub already_there: usize,
    /// Items carrying nothing worth copying, such as a login that is only a
    /// bookmark or holds only a passkey.
    pub empty: usize,
    /// Items of a type this version does not know.
    pub unsupported: usize,
    /// Names whose authenticator seed could not be read. Kept as names rather
    /// than a count because this is the one failure the user may want to go
    /// and fix by hand.
    pub unreadable_codes: Vec<String>,
}

impl ImportPlan {
    pub fn passwords(&self) -> usize {
        self.count(Category::Password)
    }

    pub fn codes(&self) -> usize {
        self.count(Category::Code)
    }

    /// Card details, identity fields, notes, keys and custom fields.
    pub fn others(&self) -> usize {
        self.count(Category::Other)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn count(&self, category: Category) -> usize {
        self.entries.iter().filter(|e| e.category == category).count()
    }
}

/// Work out which entries `items` would add to a vault that already holds
/// `existing_names`.
///
/// `existing_names` is what the caller reads off the unlocked session; passing
/// it in rather than taking a `VaultSession` keeps this testable and keeps the
/// vault's lock out of a function that does no I/O.
pub fn plan_import(items: &[BwItem], existing_names: &[String]) -> ImportPlan {
    let mut planner = Planner {
        plan: ImportPlan::default(),
        // Names already taken, plus the ones this batch claims as it goes, so
        // two items that fold down to the same name do not both go in.
        taken: existing_names.iter().map(|n| fold_name(n)).collect(),
    };

    for item in items {
        if item.deleted_date.is_some() {
            continue;
        }
        let before = planner.outcomes();
        let base = match item.item_type {
            TYPE_LOGIN => planner.login(item),
            TYPE_SECURE_NOTE => planner.secure_note(item),
            TYPE_CARD => planner.card(item),
            TYPE_IDENTITY => planner.identity(item),
            TYPE_SSH_KEY => planner.ssh_key(item),
            _ => {
                planner.plan.unsupported += 1;
                continue;
            }
        };
        planner.extras(item, &base);
        if planner.outcomes() == before {
            planner.plan.empty += 1;
        }
    }

    planner.plan
}

struct Planner {
    plan: ImportPlan,
    taken: Vec<String>,
}

impl Planner {
    /// Everything an item can lead to, so the caller can tell an item that
    /// produced nothing at all.
    fn outcomes(&self) -> usize {
        self.plan.entries.len() + self.plan.already_there + self.plan.unreadable_codes.len()
    }

    /// The password under the item's name, the seed beside it. Returns the
    /// name the item's other details hang off.
    fn login(&mut self, item: &BwItem) -> String {
        let login = item.login.clone().unwrap_or_default();
        let base = entry_name(&item.name, login.username.as_deref());
        if let Some(password) = non_empty(login.password.as_deref()) {
            self.push(base.clone(), password, EntryKind::Password, Category::Password);
        }
        if let Some(seed) = non_empty(login.totp.as_deref()) {
            // Refused here rather than at copy time: an entry that can never
            // produce a code is worse than being told now which item to fix.
            match readable_seed(seed) {
                Some(seed) => {
                    let name = format_args(&tr("{name} (code)"), &[("name", Arg::Str(&base))]);
                    self.push(name, &seed, EntryKind::Totp, Category::Code);
                }
                None => self.plan.unreadable_codes.push(base.clone()),
            }
        }
        base
    }

    /// A note is its text, so it takes the plain name, and [`Planner::extras`]
    /// does not add the same text a second time.
    fn secure_note(&mut self, item: &BwItem) -> String {
        let base = entry_name(&item.name, None);
        if let Some(text) = non_empty(item.notes.as_deref()) {
            self.push(base.clone(), text, EntryKind::Password, Category::Other);
        }
        base
    }

    fn card(&mut self, item: &BwItem) -> String {
        let base = entry_name(&item.name, None);
        let card = item.card.clone().unwrap_or_default();
        self.detail(&base, &tr("card number"), card.number.as_deref());
        self.detail(&base, &tr("security code"), card.code.as_deref());
        // Written the way it is printed on the card and typed into forms.
        let expiry = match (non_empty(card.exp_month.as_deref()), non_empty(card.exp_year.as_deref())) {
            (Some(month), Some(year)) => Some(format!("{month:0>2}/{year}")),
            (Some(month), None) => Some(format!("{month:0>2}")),
            (None, Some(year)) => Some(year.to_string()),
            (None, None) => None,
        };
        self.detail(&base, &tr("expiry"), expiry.as_deref());
        self.detail(&base, &tr("cardholder"), card.cardholder_name.as_deref());
        base
    }

    fn identity(&mut self, item: &BwItem) -> String {
        let base = entry_name(&item.name, None);
        let id = item.identity.clone().unwrap_or_default();
        let full_name = join_present(&[&id.title, &id.first_name, &id.middle_name, &id.last_name], " ");
        self.detail(&base, &tr("full name"), full_name.as_deref());
        // One entry for the whole address: it is pasted as a block far more
        // often than a line at a time, and seven entries for one address would
        // bury everything else in the list.
        let address = join_present(
            &[&id.address1, &id.address2, &id.address3, &id.city, &id.state, &id.postal_code, &id.country],
            ", ",
        );
        self.detail(&base, &tr("address"), address.as_deref());
        self.detail(&base, &tr("company"), id.company.as_deref());
        self.detail(&base, &tr("email"), id.email.as_deref());
        self.detail(&base, &tr("phone"), id.phone.as_deref());
        self.detail(&base, &tr("username"), id.username.as_deref());
        self.detail(&base, &tr("social security number"), id.ssn.as_deref());
        self.detail(&base, &tr("passport number"), id.passport_number.as_deref());
        self.detail(&base, &tr("licence number"), id.license_number.as_deref());
        base
    }

    fn ssh_key(&mut self, item: &BwItem) -> String {
        let base = entry_name(&item.name, None);
        let key = item.ssh_key.clone().unwrap_or_default();
        self.detail(&base, &tr("private key"), key.private_key.as_deref());
        self.detail(&base, &tr("public key"), key.public_key.as_deref());
        base
    }

    /// What any item can carry besides its own fields: notes and custom fields.
    fn extras(&mut self, item: &BwItem, base: &str) {
        if item.item_type != TYPE_SECURE_NOTE {
            self.detail(base, &tr("notes"), item.notes.as_deref());
        }
        for field in &item.fields {
            if field.field_type != FIELD_TEXT && field.field_type != FIELD_HIDDEN {
                continue;
            }
            let label = non_empty(field.name.as_deref()).map(str::to_string).unwrap_or_else(|| tr("field"));
            self.detail(base, &label, field.value.as_deref());
        }
    }

    /// "{base} ({what})", if there is a value to put in it.
    fn detail(&mut self, base: &str, what: &str, value: Option<&str>) {
        let Some(value) = non_empty(value) else { return };
        let name =
            format_args(&tr("{name} ({detail})"), &[("name", Arg::Str(base)), ("detail", Arg::Str(what))]);
        self.push(name, value, EntryKind::Password, Category::Other);
    }

    fn push(&mut self, name: String, secret: &str, kind: EntryKind, category: Category) {
        let folded = fold_name(&name);
        if self.taken.contains(&folded) {
            self.plan.already_there += 1;
            return;
        }
        self.taken.push(folded);
        self.plan.entries.push(PlannedEntry {
            name,
            secret: Zeroizing::new(secret.to_string()),
            kind,
            category,
        });
    }
}

/// The seed as it should be stored, or `None` if no code can come of it.
///
/// Bitwarden is forgiving about seeds: a bare or `steam://` seed with a stray
/// `1`, `0` or punctuation mark in it still gives codes there, because its
/// decoder skips anything outside the base32 alphabet. [`Totp::parse`] refuses
/// such a seed outright, which is right for one typed in by hand but would
/// turn a code that works in Bitwarden today into one that does not come
/// across. So a seed the strict parse rejects is cleaned the way Bitwarden
/// reads it and stored cleaned, which gives exactly the codes Bitwarden gives.
fn readable_seed(seed: &str) -> Option<String> {
    if Totp::parse(seed).is_ok() {
        return Some(seed.to_string());
    }
    if seed.len() >= 8 && seed[..8].eq_ignore_ascii_case("otpauth:") {
        return None;
    }
    let (prefix, rest) = match seed.get(..8) {
        Some(head) if head.eq_ignore_ascii_case("steam://") => ("steam://", &seed[8..]),
        _ => ("", seed),
    };
    let cleaned: String = rest
        .chars()
        .map(|c| c.to_ascii_uppercase())
        .filter(|c| c.is_ascii_uppercase() || ('2'..='7').contains(c))
        .collect();
    let cleaned = format!("{prefix}{cleaned}");
    Totp::parse(&cleaned).is_ok().then_some(cleaned)
}

/// The non-blank values among `parts`, trimmed and joined, or `None` when
/// every one is blank.
fn join_present(parts: &[&Option<String>], separator: &str) -> Option<String> {
    let present: Vec<&str> = parts.iter().filter_map(|p| non_empty(p.as_deref())).collect();
    (!present.is_empty()).then(|| present.join(separator))
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
            ..BwItem::default()
        }
    }

    fn parse(json: &str) -> Vec<BwItem> {
        serde_json::from_str(json).expect("the items parse")
    }

    const SEED: &str = "JBSWY3DPEHPK3PXP";

    fn names(plan: &ImportPlan) -> Vec<&str> {
        plan.entries.iter().map(|e| e.name.as_str()).collect()
    }

    fn secret_of<'a>(plan: &'a ImportPlan, name: &str) -> &'a str {
        &plan.entries.iter().find(|e| e.name == name).unwrap_or_else(|| panic!("no {name}")).secret
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
    fn the_password_and_the_code_are_separate_entries() {
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
    fn a_steam_guard_seed_comes_across_as_a_code() {
        let items = [login("Steam", Some("me"), Some("pw"), Some("steam://JBSWY3DPEHPK3PXP"))];
        let plan = plan_import(&items, &[]);
        assert_eq!(plan.codes(), 1);
        assert!(plan.unreadable_codes.is_empty());
    }

    /// Bitwarden's SDK expects `steam://ABCD123` — with a `1`, which base32
    /// has no letter for — to give N26DF at 2023-01-01T00:00:00Z. The import
    /// has to keep that code working rather than drop the seed as unreadable.
    #[test]
    fn a_seed_bitwarden_reads_despite_stray_characters_still_gives_its_codes() {
        let items = [login("Steam", None, None, Some("steam://ABCD123"))];
        let plan = plan_import(&items, &[]);
        assert!(plan.unreadable_codes.is_empty());
        let totp = Totp::parse(&plan.entries[0].secret).unwrap();
        assert_eq!(&*totp.code_at(1_672_531_200), "N26DF");

        // The same leniency for an ordinary seed, against the SDK's vector.
        let items = [login("Odd", None, None, Some("KAKFJWOSFJ12NWL"))];
        let plan = plan_import(&items, &[]);
        let totp = Totp::parse(&plan.entries[0].secret).unwrap();
        assert_eq!(&*totp.code_at(1_672_531_200), "093430");
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
        assert_eq!(second.empty, 0, "an item already imported is not an empty one");
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
    fn a_secure_note_is_its_text_under_its_own_name() {
        let items = parse(r#"[{"type": 2, "name": "Recovery codes", "notes": "aaaa-bbbb\ncccc-dddd", "secureNote": {"type": 0}}]"#);
        let plan = plan_import(&items, &[]);
        assert_eq!(names(&plan), ["Recovery codes"]);
        assert_eq!(&*plan.entries[0].secret, "aaaa-bbbb\ncccc-dddd");
        assert_eq!(plan.others(), 1);
    }

    #[test]
    fn a_card_becomes_one_entry_per_detail() {
        let items = parse(
            r#"[{"type": 3, "name": "Visa", "notes": null, "card": {
                "cardholderName": "John Doe", "brand": "Visa", "number": "4111111111111111",
                "expMonth": "1", "expYear": "2032", "code": "123"}}]"#,
        );
        let plan = plan_import(&items, &[]);
        assert_eq!(
            names(&plan),
            ["Visa (card number)", "Visa (security code)", "Visa (expiry)", "Visa (cardholder)"]
        );
        assert_eq!(secret_of(&plan, "Visa (card number)"), "4111111111111111");
        assert_eq!(secret_of(&plan, "Visa (security code)"), "123");
        assert_eq!(secret_of(&plan, "Visa (expiry)"), "01/2032");
        assert_eq!(plan.others(), 4);
        assert_eq!(plan.passwords(), 0);
    }

    #[test]
    fn an_identity_keeps_only_the_fields_that_were_filled_in() {
        let items = parse(
            r#"[{"type": 4, "name": "Me", "identity": {
                "title": "Mr", "firstName": "John", "middleName": null, "lastName": "Doe",
                "address1": "1 Main St", "address2": "", "address3": null, "city": "Springfield",
                "state": null, "postalCode": "12345", "country": "US",
                "company": null, "email": "john@example.com", "phone": "555-0100",
                "ssn": "000-00-0000", "username": null, "passportNumber": "X123", "licenseNumber": null}}]"#,
        );
        let plan = plan_import(&items, &[]);
        assert_eq!(
            names(&plan),
            [
                "Me (full name)",
                "Me (address)",
                "Me (email)",
                "Me (phone)",
                "Me (social security number)",
                "Me (passport number)",
            ]
        );
        assert_eq!(secret_of(&plan, "Me (full name)"), "Mr John Doe");
        assert_eq!(secret_of(&plan, "Me (address)"), "1 Main St, Springfield, 12345, US");
    }

    #[test]
    fn an_ssh_key_gives_both_halves() {
        let items = parse(
            r#"[{"type": 5, "name": "Server", "sshKey": {
                "privateKey": "-----BEGIN OPENSSH PRIVATE KEY-----\nabc\n-----END OPENSSH PRIVATE KEY-----",
                "publicKey": "ssh-ed25519 AAAA", "keyFingerprint": "SHA256:x"}}]"#,
        );
        let plan = plan_import(&items, &[]);
        assert_eq!(names(&plan), ["Server (private key)", "Server (public key)"]);
        assert!(secret_of(&plan, "Server (private key)").contains("\nabc\n"));
    }

    #[test]
    fn notes_and_copyable_custom_fields_come_along_with_any_item() {
        let items = parse(
            r#"[{"type": 1, "name": "Bank", "notes": "PIN is 1234",
                "login": {"username": "me", "password": "pw", "totp": null},
                "fields": [
                    {"name": "Security answer", "value": "Rex", "type": 1},
                    {"name": "Customer ID", "value": "998877", "type": 0},
                    {"name": "Remember me", "value": "true", "type": 2},
                    {"name": "Linked", "value": null, "type": 3, "linkedId": 101},
                    {"name": null, "value": "anonymous", "type": 0}
                ]}]"#,
        );
        let plan = plan_import(&items, &[]);
        assert_eq!(
            names(&plan),
            [
                "Bank (me)",
                "Bank (me) (notes)",
                "Bank (me) (Security answer)",
                "Bank (me) (Customer ID)",
                "Bank (me) (field)",
            ]
        );
        assert_eq!(plan.passwords(), 1);
        assert_eq!(plan.others(), 4);
    }

    #[test]
    fn trashed_items_are_not_resurrected_by_the_import() {
        let mut deleted = login("Old thing", None, Some("x"), None);
        deleted.deleted_date = Some("2026-01-01T00:00:00.000Z".to_string());
        let plan = plan_import(&[deleted], &[]);
        assert!(plan.is_empty());
        assert_eq!(plan.unsupported, 0);
        assert_eq!(plan.empty, 0);
    }

    #[test]
    fn an_item_holding_nothing_to_copy_is_counted_as_empty() {
        let items = [login("Just a bookmark", Some("me"), None, None), login("Nothing", None, Some("  "), Some(""))];
        let plan = plan_import(&items, &[]);
        assert!(plan.is_empty());
        assert_eq!(plan.empty, 2);
    }

    #[test]
    fn an_item_type_from_the_future_is_counted_rather_than_guessed_at() {
        let items = parse(r#"[{"type": 9, "name": "Something new", "notes": "x"}]"#);
        let plan = plan_import(&items, &[]);
        assert!(plan.is_empty());
        assert_eq!(plan.unsupported, 1);
    }

    #[test]
    fn an_unreadable_seed_is_reported_by_name_rather_than_stored() {
        // A link asking for a twelve-digit code, which nothing can produce.
        let items = [login("Weird", None, Some("hunter2"), Some("otpauth://totp/Weird?secret=JBSWY3DPEHPK3PXP&digits=12"))];
        let plan = plan_import(&items, &[]);
        // The password still comes across; only the seed is refused.
        assert_eq!(names(&plan), ["Weird"]);
        assert_eq!(plan.unreadable_codes, ["Weird"]);
        assert_eq!(plan.empty, 0);
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

    #[test]
    fn an_item_missing_its_type_object_entirely_does_not_panic() {
        let items = parse(r#"[{"type": 1, "name": "Odd"}, {"type": 3, "name": "Odd card"}, {"type": 4, "name": null}]"#);
        let plan = plan_import(&items, &[]);
        assert!(plan.is_empty());
        assert_eq!(plan.empty, 3);
    }
}
