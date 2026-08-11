//! The encrypted vault behind `*` mode: passwords and other secrets living in
//! `vault/` as AES-256-GCM `.enc` files that only exist in the clear inside
//! this process, for as long as the vault is unlocked.
//!
//! # How it is keyed
//!
//! Two keys, not one. `vault/vault.meta` holds a random 32-byte *vault key*
//! wrapped with a *master key* that Argon2id stretches out of the master
//! password; the entries are sealed with the vault key. The indirection buys
//! two things: changing the master password rewrites one small file instead of
//! re-encrypting every entry, and the password itself is never the thing an
//! entry file was encrypted with.
//!
//! # What an attacker with the folder learns
//!
//! Only how many entries there are and roughly how long each one is. An entry
//! file is named after a random uuid and holds nothing but a nonce and
//! ciphertext — the entry's *name* and shortcut are inside the sealed payload
//! along with the secret, because "amazon" or "work vpn" sitting in a file name
//! gives away most of what a password list is worth. The uuid is authenticated
//! as associated data, so entry files cannot be swapped around either.
//!
//! Plaintext never reaches the disk: reads decrypt into [`Zeroizing`] buffers
//! that wipe themselves on drop, and the session holds entry *names* — never
//! secrets, which are decrypted one at a time, on demand.

use std::path::{Path, PathBuf};

use aes_gcm::aead::{Aead, Generate, KeyInit, Nonce, Payload};
use aes_gcm::Aes256Gcm;
use argon2::{Algorithm, Argon2, Params, Version};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::storage::{atomic_write, atomic_write_json};

/// The vault folder, relative to the app folder (portable, like `snippets/`).
pub const VAULT_DIR: &str = "vault";

/// Holds the wrapped vault key. Its absence is what "the vault has not been
/// set up yet" means.
const META_NAME: &str = "vault.meta";

const ENTRY_EXT: &str = "enc";

/// First four bytes of every entry file, so a truncated or unrelated file is
/// rejected before anything is fed to the cipher.
const MAGIC: [u8; 4] = *b"LTV1";

const KEY_LEN: usize = 32;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const HEADER_LEN: usize = MAGIC.len() + NONCE_LEN;

/// Associated data for the wrapped vault key: it is not an entry, so it must
/// not be interchangeable with one.
const META_AAD: &[u8] = b"launchtype-vault-key";

/// Master passwords shorter than this are refused outright. Everything in the
/// vault is only ever as strong as this one string.
pub const MIN_PASSWORD_LEN: usize = 8;

/// Argon2id cost. [`Kdf::STRONG`] is what the app writes; the tests use
/// [`Kdf::WEAK`], because a debug-build derivation at the real cost takes
/// seconds and the suite would spend minutes stretching passwords.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Kdf {
    /// Memory cost in KiB.
    pub m_cost: u32,
    /// Number of passes.
    pub t_cost: u32,
    /// Degree of parallelism.
    pub p_cost: u32,
}

impl Kdf {
    /// 256 MiB over 4 passes — an order of magnitude past OWASP's floor for
    /// Argon2id (19 MiB, 2 passes), and still about half a second to unlock.
    /// The memory is what makes guessing the master password expensive on the
    /// hardware an attacker would bring to it, so it is set as high as a
    /// noticeable-but-not-annoying wait allows.
    ///
    /// Every vault records the cost it was created with, so raising this only
    /// affects vaults made (or given a new master password) afterwards.
    pub const STRONG: Kdf = Kdf { m_cost: 256 * 1024, t_cost: 4, p_cost: 1 };

    /// The cheapest parameters Argon2 accepts. Test use only.
    pub const WEAK: Kdf = Kdf { m_cost: 8, t_cost: 1, p_cost: 1 };
}

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    /// The master key did not unwrap the vault key: the password is wrong (or
    /// `vault.meta` belongs to a different vault).
    #[error("wrong master password")]
    WrongPassword,
    /// A file is not a vault file, or has been altered since it was written.
    #[error("damaged vault file")]
    Damaged,
    /// An operation that needs the key was attempted while locked.
    #[error("the vault is locked")]
    Locked,
    #[error("no such entry")]
    NoSuchEntry,
    /// Creating a vault where one already exists. Refused rather than
    /// obliged: a second `vault.meta` would hold a different vault key, and
    /// every entry written under the first one would become unopenable.
    #[error("there is already a vault here")]
    AlreadyExists,
    #[error("the master password is too short")]
    PasswordTooShort,
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

type Result<T> = std::result::Result<T, VaultError>;

/// What the results list shows for one entry. Deliberately has no `secret`
/// field: the list is rebuilt on every keystroke and read out loud, and the
/// secret has no business being anywhere near it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryInfo {
    /// The uuid that names the entry's file.
    pub id: String,
    pub name: String,
    /// Optional lowercase shortcut; an exact match jumps straight to the entry.
    pub shortcut: String,
}

/// The sealed payload of an entry file.
#[derive(Serialize, Deserialize)]
struct EntryData {
    name: String,
    #[serde(default)]
    shortcut: String,
    secret: String,
}

/// `vault/vault.meta`.
#[derive(Serialize, Deserialize)]
struct Meta {
    version: u32,
    kdf: String,
    m_cost: u32,
    t_cost: u32,
    p_cost: u32,
    /// Argon2id salt, base64.
    salt: String,
    /// nonce + sealed vault key, base64.
    wrapped_key: String,
}

/// Stretch `password` into a master key with Argon2id.
fn derive_master_key(password: &str, salt: &[u8], kdf: Kdf) -> Result<Zeroizing<[u8; KEY_LEN]>> {
    let params = Params::new(kdf.m_cost, kdf.t_cost, kdf.p_cost, Some(KEY_LEN))
        .map_err(|_| VaultError::Damaged)?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    argon
        .hash_password_into(password.as_bytes(), salt, key.as_mut_slice())
        .map_err(|_| VaultError::Damaged)?;
    Ok(key)
}

fn cipher(key: &[u8; KEY_LEN]) -> Aes256Gcm {
    Aes256Gcm::new_from_slice(key).expect("32 bytes is the AES-256 key size")
}

/// Seal `plaintext` under `key`, returning `nonce || ciphertext`.
fn seal(key: &[u8; KEY_LEN], plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
    let nonce_bytes: [u8; NONCE_LEN] = Generate::generate();
    let nonce = Nonce::<Aes256Gcm>::from(nonce_bytes);
    let sealed = cipher(key)
        .encrypt(&nonce, Payload { msg: plaintext, aad })
        .map_err(|_| VaultError::Damaged)?;
    let mut out = Vec::with_capacity(NONCE_LEN + sealed.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&sealed);
    Ok(out)
}

/// The inverse of [`seal`]. A failed tag check is reported as `wrong_password`
/// or [`VaultError::Damaged`] depending on what the caller was doing: the same
/// AEAD failure means "you typed the wrong password" when unwrapping the vault
/// key and "this file has been altered" for an entry.
fn open(key: &[u8; KEY_LEN], sealed: &[u8], aad: &[u8], on_tag_failure: VaultError) -> Result<Zeroizing<Vec<u8>>> {
    if sealed.len() <= NONCE_LEN {
        return Err(VaultError::Damaged);
    }
    let mut nonce_bytes = [0u8; NONCE_LEN];
    nonce_bytes.copy_from_slice(&sealed[..NONCE_LEN]);
    let nonce = Nonce::<Aes256Gcm>::from(nonce_bytes);
    let plaintext = cipher(key)
        .decrypt(&nonce, Payload { msg: &sealed[NONCE_LEN..], aad })
        .map_err(|_| on_tag_failure)?;
    Ok(Zeroizing::new(plaintext))
}

/// The vault as this process sees it: a folder on disk plus, once unlocked,
/// the vault key and the list of entry names.
///
/// Shared as `Arc<Mutex<VaultSession>>` so the auto-lock thread can wipe the
/// key on idle without going through the UI.
pub struct VaultSession {
    dir: PathBuf,
    kdf: Kdf,
    /// Minutes of inactivity before the key is wiped; 0 additionally means
    /// "lock the moment a secret has been copied" (see [`Self::locks_on_use`]).
    lock_after_minutes: u32,
    key: Option<Zeroizing<[u8; KEY_LEN]>>,
    entries: Vec<EntryInfo>,
    last_used: Option<DateTime<Local>>,
}

impl VaultSession {
    pub fn new(dir: impl Into<PathBuf>, lock_after_minutes: u32) -> Self {
        VaultSession::with_kdf(dir, lock_after_minutes, Kdf::STRONG)
    }

    /// [`Self::new`] with the password-stretching cost spelled out. Only the
    /// tests have any business asking for anything but [`Kdf::STRONG`].
    pub fn with_kdf(dir: impl Into<PathBuf>, lock_after_minutes: u32, kdf: Kdf) -> Self {
        VaultSession {
            dir: dir.into(),
            kdf,
            lock_after_minutes,
            key: None,
            entries: Vec::new(),
            last_used: None,
        }
    }

    fn meta_path(&self) -> PathBuf {
        self.dir.join(META_NAME)
    }

    fn entry_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.{ENTRY_EXT}"))
    }

    /// True when there is no vault yet, so the next visit asks for a master
    /// password to create one.
    pub fn is_new(&self) -> bool {
        !self.meta_path().is_file()
    }

    /// Encrypted entries sitting in the folder without the `vault.meta` that
    /// holds the key to them. Normally zero; anything else means the key file
    /// was lost or the folder was half-copied, and creating a fresh vault
    /// would strand them — so the app says so before it does.
    pub fn orphan_count(&self) -> usize {
        entry_ids(&self.dir).len()
    }

    pub fn is_unlocked(&self) -> bool {
        self.key.is_some()
    }

    /// Entry names and shortcuts, sorted by name; empty while locked.
    pub fn entries(&self) -> &[EntryInfo] {
        &self.entries
    }

    /// Whether the vault re-locks as soon as a secret has been copied.
    pub fn locks_on_use(&self) -> bool {
        self.lock_after_minutes == 0
    }

    pub fn set_lock_after_minutes(&mut self, minutes: u32) {
        self.lock_after_minutes = minutes;
    }

    /// Wipe the key and the entry list. Idempotent.
    pub fn lock(&mut self) {
        self.key = None;
        self.entries.clear();
        self.last_used = None;
    }

    /// Push the idle deadline out; called after every successful use.
    pub fn touch(&mut self, now: DateTime<Local>) {
        if self.key.is_some() {
            self.last_used = Some(now);
        }
    }

    /// Lock if the vault has gone untouched for the configured time. Returns
    /// whether it locked, so the caller can announce it. Ticked once a second
    /// by the auto-lock thread.
    ///
    /// A zero timeout means "lock after each copy", which the copy path does
    /// itself; here it is read as one minute, so a vault unlocked and then
    /// abandoned without copying anything still does not stay open.
    pub fn expire(&mut self, now: DateTime<Local>) -> bool {
        let Some(last) = self.last_used else { return false };
        let idle = chrono::Duration::minutes(i64::from(self.lock_after_minutes.max(1)));
        if now.signed_duration_since(last) < idle {
            return false;
        }
        self.lock();
        true
    }

    /// Create the vault and unlock it. Fails if one is already there.
    pub fn create(&mut self, password: &str, now: DateTime<Local>) -> Result<()> {
        if !self.is_new() {
            return Err(VaultError::AlreadyExists);
        }
        if password.chars().count() < MIN_PASSWORD_LEN {
            return Err(VaultError::PasswordTooShort);
        }
        std::fs::create_dir_all(&self.dir)?;
        let vault_key = Zeroizing::new(<[u8; KEY_LEN]>::generate());
        self.write_meta(password, &vault_key)?;
        self.key = Some(vault_key);
        self.entries = self.read_entries();
        self.last_used = Some(now);
        Ok(())
    }

    /// Unwrap the vault key with `password` and read the entry list in.
    pub fn unlock(&mut self, password: &str, now: DateTime<Local>) -> Result<()> {
        let vault_key = self.unwrap_key(password)?;
        self.key = Some(vault_key);
        self.entries = self.read_entries();
        self.last_used = Some(now);
        Ok(())
    }

    /// Re-wrap the vault key under a new master password. The entries are not
    /// touched — they were never encrypted with the password to begin with.
    pub fn change_password(&mut self, current: &str, new: &str) -> Result<()> {
        if new.chars().count() < MIN_PASSWORD_LEN {
            return Err(VaultError::PasswordTooShort);
        }
        let vault_key = self.unwrap_key(current)?;
        self.write_meta(new, &vault_key)
    }

    /// Decrypt one entry's secret. Reads the file each time rather than
    /// holding secrets in the session, so an unlocked vault only ever has the
    /// one being used in memory.
    pub fn secret(&self, id: &str) -> Result<Zeroizing<String>> {
        let data = self.read_entry(id)?;
        Ok(Zeroizing::new(data.secret.clone()))
    }

    /// The stored name and shortcut plus the secret, for the edit dialog.
    pub fn entry(&self, id: &str) -> Result<(EntryInfo, Zeroizing<String>)> {
        let data = self.read_entry(id)?;
        let info = EntryInfo {
            id: id.to_string(),
            name: data.name.clone(),
            shortcut: data.shortcut.clone(),
        };
        Ok((info, Zeroizing::new(data.secret.clone())))
    }

    /// Add (`id` = `None`) or overwrite an entry, and return its id. Every
    /// write reseals with a fresh nonce.
    pub fn save(
        &mut self,
        id: Option<&str>,
        name: &str,
        shortcut: &str,
        secret: &str,
    ) -> Result<String> {
        let key = self.key.as_ref().ok_or(VaultError::Locked)?;
        let id = match id {
            Some(id) => id.to_string(),
            None => uuid::Uuid::new_v4().to_string(),
        };
        let data = EntryData {
            name: name.trim().to_string(),
            shortcut: shortcut.trim().to_lowercase(),
            secret: secret.to_string(),
        };
        let plaintext = Zeroizing::new(serde_json::to_vec(&data).map_err(|_| VaultError::Damaged)?);
        let sealed = seal(key, &plaintext, id.as_bytes())?;

        std::fs::create_dir_all(&self.dir)?;
        let mut file = Vec::with_capacity(HEADER_LEN + sealed.len());
        file.extend_from_slice(&MAGIC);
        file.extend_from_slice(&sealed);
        atomic_write(&self.entry_path(&id), &file)?;

        let info = EntryInfo { id: id.clone(), name: data.name, shortcut: data.shortcut };
        match self.entries.iter_mut().find(|e| e.id == id) {
            Some(existing) => *existing = info,
            None => self.entries.push(info),
        }
        sort_entries(&mut self.entries);
        Ok(id)
    }

    /// Delete an entry's file. Unrecoverable, which is why the UI asks first.
    pub fn delete(&mut self, id: &str) -> Result<()> {
        if !self.entries.iter().any(|e| e.id == id) {
            return Err(VaultError::NoSuchEntry);
        }
        std::fs::remove_file(self.entry_path(id))?;
        self.entries.retain(|e| e.id != id);
        Ok(())
    }

    /// Derive the master key from `password` and unwrap the vault key with it.
    fn unwrap_key(&self, password: &str) -> Result<Zeroizing<[u8; KEY_LEN]>> {
        let text = std::fs::read_to_string(self.meta_path())?;
        let meta: Meta = serde_json::from_str(&text).map_err(|_| VaultError::Damaged)?;
        let salt = BASE64.decode(&meta.salt).map_err(|_| VaultError::Damaged)?;
        let wrapped = BASE64.decode(&meta.wrapped_key).map_err(|_| VaultError::Damaged)?;
        let kdf = Kdf { m_cost: meta.m_cost, t_cost: meta.t_cost, p_cost: meta.p_cost };
        let master = derive_master_key(password, &salt, kdf)?;
        let plain = open(&master, &wrapped, META_AAD, VaultError::WrongPassword)?;
        let key: [u8; KEY_LEN] = plain.as_slice().try_into().map_err(|_| VaultError::Damaged)?;
        Ok(Zeroizing::new(key))
    }

    /// Wrap `vault_key` under a master key freshly derived from `password`
    /// (new salt every time) and write `vault.meta`.
    fn write_meta(&self, password: &str, vault_key: &[u8; KEY_LEN]) -> Result<()> {
        let salt: [u8; SALT_LEN] = Generate::generate();
        let master = derive_master_key(password, &salt, self.kdf)?;
        let wrapped = seal(&master, vault_key, META_AAD)?;
        let meta = Meta {
            version: 1,
            kdf: "argon2id".to_string(),
            m_cost: self.kdf.m_cost,
            t_cost: self.kdf.t_cost,
            p_cost: self.kdf.p_cost,
            salt: BASE64.encode(salt),
            wrapped_key: BASE64.encode(&wrapped),
        };
        std::fs::create_dir_all(&self.dir)?;
        atomic_write_json(&self.meta_path(), &meta, Some(2))?;
        Ok(())
    }

    fn read_entry(&self, id: &str) -> Result<EntryData> {
        let key = self.key.as_ref().ok_or(VaultError::Locked)?;
        let raw = match std::fs::read(self.entry_path(id)) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(VaultError::NoSuchEntry)
            }
            Err(e) => return Err(e.into()),
        };
        if raw.len() <= HEADER_LEN || raw[..MAGIC.len()] != MAGIC {
            return Err(VaultError::Damaged);
        }
        // The uuid is authenticated, so an entry file renamed onto another
        // entry's name fails here rather than impersonating it.
        let plain = open(key, &raw[MAGIC.len()..], id.as_bytes(), VaultError::Damaged)?;
        serde_json::from_slice(&plain).map_err(|_| VaultError::Damaged)
    }

    /// Decrypt every entry in the folder for its name and shortcut. A file
    /// that will not open is skipped rather than failing the unlock: one
    /// damaged entry must not take the rest of the vault down with it.
    fn read_entries(&self) -> Vec<EntryInfo> {
        let mut entries: Vec<EntryInfo> = entry_ids(&self.dir)
            .into_iter()
            .filter_map(|id| match self.read_entry(&id) {
                Ok(data) => {
                    Some(EntryInfo { id, name: data.name, shortcut: data.shortcut })
                }
                Err(e) => {
                    log::warn!("vault entry {id} could not be read: {e}");
                    None
                }
            })
            .collect();
        sort_entries(&mut entries);
        entries
    }
}

/// The uuids of every `*.enc` file in the vault folder.
fn entry_ids(dir: &Path) -> Vec<String> {
    let Ok(read_dir) = std::fs::read_dir(dir) else { return Vec::new() };
    read_dir
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == ENTRY_EXT))
        .filter_map(|path| path.file_stem().map(|stem| stem.to_string_lossy().into_owned()))
        .collect()
}

/// By name, case-insensitively, with the id breaking ties so the list order is
/// stable across unlocks.
fn sort_entries(entries: &mut [EntryInfo]) {
    entries.sort_by(|a, b| {
        a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.id.cmp(&b.id))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 8, 11, 9, 0, 0).unwrap()
    }

    const PASSWORD: &str = "correct horse battery";

    /// A vault in a temp folder, already unlocked, using throwaway KDF cost.
    fn vault(dir: &Path) -> VaultSession {
        let mut session = VaultSession::with_kdf(dir.join(VAULT_DIR), 5, Kdf::WEAK);
        session.create(PASSWORD, now()).unwrap();
        session
    }

    #[test]
    fn a_missing_folder_reads_as_a_vault_that_has_not_been_set_up() {
        let dir = tempfile::tempdir().unwrap();
        let session = VaultSession::with_kdf(dir.path().join(VAULT_DIR), 5, Kdf::WEAK);
        assert!(session.is_new());
        assert!(!session.is_unlocked());
        assert_eq!(session.orphan_count(), 0);

        let session = vault(dir.path());
        assert!(!session.is_new());
        assert!(session.is_unlocked());
    }

    #[test]
    fn entries_round_trip_through_a_lock_and_a_fresh_unlock() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = vault(dir.path());
        let id = session.save(None, "github", "gh", "hunter2").unwrap();
        session.save(None, "bank", "", "1234-5678").unwrap();

        session.lock();
        assert!(session.entries().is_empty(), "locking wipes the entry list");
        assert!(matches!(session.secret(&id), Err(VaultError::Locked)));

        session.unlock(PASSWORD, now()).unwrap();
        let names: Vec<&str> = session.entries().iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["bank", "github"], "sorted by name");
        assert_eq!(&*session.secret(&id).unwrap(), "hunter2");
        assert_eq!(session.entries()[1].shortcut, "gh");
    }

    #[test]
    fn the_wrong_password_is_rejected_and_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = vault(dir.path());
        session.save(None, "github", "gh", "hunter2").unwrap();
        session.lock();

        assert!(matches!(session.unlock("not it at all", now()), Err(VaultError::WrongPassword)));
        assert!(!session.is_unlocked());
        session.unlock(PASSWORD, now()).unwrap();
        assert_eq!(session.entries().len(), 1);
    }

    #[test]
    fn nothing_readable_is_left_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = vault(dir.path());
        let id = session.save(None, "github", "gh", "hunter2").unwrap();

        let path = dir.path().join(VAULT_DIR).join(format!("{id}.enc"));
        let raw = std::fs::read(&path).unwrap();
        assert_eq!(&raw[..4], b"LTV1");
        for plaintext in ["github", "gh", "hunter2"] {
            assert!(
                !raw.windows(plaintext.len()).any(|w| w == plaintext.as_bytes()),
                "{plaintext:?} is readable in the entry file"
            );
        }
        // The file name gives nothing away either.
        assert!(uuid::Uuid::parse_str(&id).is_ok());
    }

    #[test]
    fn a_tampered_entry_is_refused_rather_than_trusted() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = vault(dir.path());
        let id = session.save(None, "github", "gh", "hunter2").unwrap();
        let path = dir.path().join(VAULT_DIR).join(format!("{id}.enc"));

        let mut raw = std::fs::read(&path).unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 0x01;
        std::fs::write(&path, &raw).unwrap();
        assert!(matches!(session.secret(&id), Err(VaultError::Damaged)));

        // ...and so is an entry file moved onto another entry's name: the id
        // is authenticated alongside the ciphertext.
        std::fs::write(&path, std::fs::read(&path).unwrap()).unwrap();
        let other = session.save(None, "bank", "", "1234").unwrap();
        let sealed = std::fs::read(dir.path().join(VAULT_DIR).join(format!("{other}.enc"))).unwrap();
        std::fs::write(&path, sealed).unwrap();
        assert!(matches!(session.secret(&id), Err(VaultError::Damaged)));
    }

    #[test]
    fn saving_over_an_entry_replaces_it_instead_of_adding_a_second() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = vault(dir.path());
        let id = session.save(None, "github", "gh", "hunter2").unwrap();
        let same = session.save(Some(&id), "github work", "gh", "hunter3").unwrap();

        assert_eq!(same, id);
        assert_eq!(session.entries().len(), 1);
        assert_eq!(session.entries()[0].name, "github work");
        assert_eq!(&*session.secret(&id).unwrap(), "hunter3");
    }

    #[test]
    fn deleting_takes_the_file_with_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = vault(dir.path());
        let id = session.save(None, "github", "gh", "hunter2").unwrap();
        let path = dir.path().join(VAULT_DIR).join(format!("{id}.enc"));

        session.delete(&id).unwrap();
        assert!(!path.exists());
        assert!(session.entries().is_empty());
        assert!(matches!(session.delete(&id), Err(VaultError::NoSuchEntry)));
    }

    #[test]
    fn changing_the_master_password_leaves_the_entries_alone() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = vault(dir.path());
        let id = session.save(None, "github", "gh", "hunter2").unwrap();
        let before = std::fs::read(dir.path().join(VAULT_DIR).join(format!("{id}.enc"))).unwrap();

        assert!(matches!(
            session.change_password("wrong", "a new long password"),
            Err(VaultError::WrongPassword)
        ));
        session.change_password(PASSWORD, "a new long password").unwrap();

        let after = std::fs::read(dir.path().join(VAULT_DIR).join(format!("{id}.enc"))).unwrap();
        assert_eq!(before, after, "entry files are not rewritten");

        session.lock();
        assert!(matches!(session.unlock(PASSWORD, now()), Err(VaultError::WrongPassword)));
        session.unlock("a new long password", now()).unwrap();
        assert_eq!(&*session.secret(&id).unwrap(), "hunter2");
    }

    /// A second `create` would write a `vault.meta` holding a different vault
    /// key, and every entry written under the first one would stop opening.
    #[test]
    fn a_vault_is_never_created_on_top_of_an_existing_one() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = vault(dir.path());
        let id = session.save(None, "github", "gh", "hunter2").unwrap();

        assert!(matches!(
            session.create("a different password", now()),
            Err(VaultError::AlreadyExists)
        ));
        session.lock();
        session.unlock(PASSWORD, now()).unwrap();
        assert_eq!(&*session.secret(&id).unwrap(), "hunter2");
    }

    #[test]
    fn short_master_passwords_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = VaultSession::with_kdf(dir.path().join(VAULT_DIR), 5, Kdf::WEAK);
        assert!(matches!(session.create("short", now()), Err(VaultError::PasswordTooShort)));
        assert!(session.is_new());

        session.create(PASSWORD, now()).unwrap();
        assert!(matches!(
            session.change_password(PASSWORD, "tiny"),
            Err(VaultError::PasswordTooShort)
        ));
    }

    #[test]
    fn the_vault_locks_itself_once_the_idle_time_has_passed() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = vault(dir.path());
        session.save(None, "github", "gh", "hunter2").unwrap();

        assert!(!session.expire(now() + chrono::Duration::minutes(4)));
        assert!(session.is_unlocked());

        // Using it pushes the deadline out.
        session.touch(now() + chrono::Duration::minutes(4));
        assert!(!session.expire(now() + chrono::Duration::minutes(8)));

        assert!(session.expire(now() + chrono::Duration::minutes(10)));
        assert!(!session.is_unlocked());
        assert!(session.entries().is_empty());
        assert!(!session.expire(now() + chrono::Duration::hours(1)), "already locked");
    }

    /// A zero timeout means "lock after each copy", which the UI does itself.
    /// The idle sweep must still close an abandoned vault rather than reading
    /// zero as "never".
    #[test]
    fn a_zero_timeout_still_expires_after_a_minute() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = VaultSession::with_kdf(dir.path().join(VAULT_DIR), 0, Kdf::WEAK);
        session.create(PASSWORD, now()).unwrap();
        assert!(session.locks_on_use());
        assert!(!session.expire(now() + chrono::Duration::seconds(30)));
        assert!(session.expire(now() + chrono::Duration::seconds(61)));
    }

    #[test]
    fn entry_files_without_a_key_file_are_counted_rather_than_stranded_silently() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = vault(dir.path());
        session.save(None, "github", "gh", "hunter2").unwrap();
        std::fs::remove_file(dir.path().join(VAULT_DIR).join(META_NAME)).unwrap();

        let session = VaultSession::with_kdf(dir.path().join(VAULT_DIR), 5, Kdf::WEAK);
        assert!(session.is_new());
        assert_eq!(session.orphan_count(), 1);
    }

    /// Every other test here runs at [`Kdf::WEAK`], so this is the only one
    /// that proves the cost the app actually ships with is accepted and comes
    /// back in a reasonable time. Ignored by default because a debug build
    /// spends seconds on it:
    ///
    ///     cargo test --release -p launchtype-core -- --ignored --nocapture
    #[test]
    #[ignore = "slow in a debug build; the shipped Argon2 cost is the point"]
    fn the_shipped_kdf_cost_works() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = VaultSession::new(dir.path().join(VAULT_DIR), 5);
        let started = std::time::Instant::now();
        session.create(PASSWORD, now()).unwrap();
        let id = session.save(None, "github", "gh", "hunter2").unwrap();
        session.lock();
        session.unlock(PASSWORD, now()).unwrap();
        println!("Argon2id at {:?}: {:?} for create + unlock", Kdf::STRONG, started.elapsed());
        assert_eq!(&*session.secret(&id).unwrap(), "hunter2");
    }

    #[test]
    fn one_damaged_entry_does_not_take_the_others_down() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = vault(dir.path());
        let broken = session.save(None, "github", "gh", "hunter2").unwrap();
        session.save(None, "bank", "", "1234").unwrap();
        std::fs::write(
            dir.path().join(VAULT_DIR).join(format!("{broken}.enc")),
            b"LTV1 not a vault file at all",
        )
        .unwrap();

        session.lock();
        session.unlock(PASSWORD, now()).unwrap();
        assert_eq!(session.entries().len(), 1);
        assert_eq!(session.entries()[0].name, "bank");
    }
}
