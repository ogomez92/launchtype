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
//! Nor when an entry was added, changed or last used: the vault's files carry
//! random dates from long before it existed; see [`crate::vault_dates`].
//!
//! Plaintext never reaches the disk: reads decrypt into [`Zeroizing`] buffers
//! that wipe themselves on drop, and the session holds entry *names* — never
//! secrets, which are decrypted one at a time, on demand.
//!
//! # Files and folders
//!
//! An entry can also hold a document, or a whole folder of them. The entry
//! file stays as small as any other — it carries the original name, the size,
//! and a random key made for that one file — and the contents go in
//! `<uuid>.blob` next to it, sealed under that key; see [`crate::vault_blob`].
//! The only way back out is [`VaultSession::export_file`] or
//! [`VaultSession::export_folder`] to a place the user chose: nothing is ever
//! decrypted into a temp folder or handed to another program to open.

use std::io;
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
use crate::vault_blob::{self, BlobReader, BlobWriter, MAX_BLOB_BYTES};
use crate::vault_dates::{stamp_read, stamp_written};

/// The vault folder, relative to the app folder (portable, like `snippets/`).
pub const VAULT_DIR: &str = "vault";

/// Holds the wrapped vault key. Its absence is what "the vault has not been
/// set up yet" means.
const META_NAME: &str = "vault.meta";

const ENTRY_EXT: &str = "enc";

/// The sealed contents of a file or folder entry. A different extension from
/// [`ENTRY_EXT`], so nothing that lists entries ever mistakes one for an entry.
const BLOB_EXT: &str = "blob";

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
    /// A file or folder over [`MAX_BLOB_BYTES`].
    #[error("too large to keep in the vault")]
    TooLarge,
    /// Asking a password entry for a file, or the other way round.
    #[error("that entry does not hold that kind of thing")]
    WrongKind,
    /// Adding the vault folder, or a folder inside it, to the vault.
    #[error("the vault cannot hold itself")]
    InsideVault,
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

type Result<T> = std::result::Result<T, VaultError>;

/// What kind of secret an entry holds, which decides what Enter puts on the
/// clipboard: the stored text itself, or the six digits derived from it.
///
/// Serialised as a string and defaulted on read, so entries written before
/// this field existed load as passwords instead of failing to open.
///
/// A version from before files and folders reads those two kinds as an entry
/// it cannot open, and leaves it out of the list rather than failing the
/// unlock — see [`VaultSession::read_entries`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    #[default]
    Password,
    /// The secret is an `otpauth://` URI or a bare base32 seed; see
    /// [`crate::totp`].
    Totp,
    /// A document, kept in the entry's blob.
    File,
    /// A folder, kept in the entry's blob as a zip of it.
    Folder,
}

impl EntryKind {
    /// Whether the entry's contents are a blob rather than its secret text.
    pub fn holds_file(self) -> bool {
        matches!(self, EntryKind::File | EntryKind::Folder)
    }
}

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
    pub kind: EntryKind,
}

/// The sealed payload of an entry file.
#[derive(Serialize, Deserialize)]
struct EntryData {
    name: String,
    #[serde(default)]
    shortcut: String,
    secret: String,
    /// Absent in vaults written before authenticator entries existed.
    #[serde(default)]
    kind: EntryKind,
    /// Only on file and folder entries, and left out of every other entry
    /// entirely so a password entry seals to the same shape it always did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    file: Option<StoredFile>,
}

/// What a file or folder entry knows about its blob.
#[derive(Serialize, Deserialize)]
struct StoredFile {
    /// The file's or folder's name when it was added, offered back as the
    /// name to save it under.
    name: String,
    /// Plaintext bytes in the blob (the zip's, for a folder).
    size: u64,
    /// The blob's own key, base64. It lives only here, inside the sealed
    /// entry, so the blob opens only through its entry.
    key: String,
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

    fn blob_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.{BLOB_EXT}"))
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
        crate::storage::create_private_dir(&self.dir)?;
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
        // The entries were dated as they were read. Blobs are not read here,
        // but one written before dates were scrambled still needs it.
        for id in ids_with_ext(&self.dir, BLOB_EXT) {
            stamp_read(&self.blob_path(&id));
        }
        stamp_read(&self.dir);
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
            kind: data.kind,
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
        kind: EntryKind,
    ) -> Result<String> {
        if self.key.is_none() {
            return Err(VaultError::Locked);
        }
        // Files come in through `add_file` and `add_folder`. Saving over one
        // here would drop its key and strand the blob for good.
        let over_a_file =
            id.is_some_and(|id| self.entries.iter().any(|e| e.id == id && e.kind.holds_file()));
        if kind.holds_file() || over_a_file {
            return Err(VaultError::WrongKind);
        }
        let id = match id {
            Some(id) => id.to_string(),
            None => uuid::Uuid::new_v4().to_string(),
        };
        let data = EntryData {
            name: name.trim().to_string(),
            shortcut: shortcut.trim().to_lowercase(),
            secret: secret.to_string(),
            kind,
            file: None,
        };
        self.write_entry(&id, data)?;
        Ok(id)
    }

    /// Change an entry's name and shortcut and nothing else. How a file or
    /// folder entry is edited, since there is no secret text to show.
    pub fn rename(&mut self, id: &str, name: &str, shortcut: &str) -> Result<()> {
        let mut data = self.read_entry(id)?;
        data.name = name.trim().to_string();
        data.shortcut = shortcut.trim().to_lowercase();
        self.write_entry(id, data)
    }

    /// Seal `data` into the entry file for `id` and update the list to match.
    /// Every write reseals with a fresh nonce.
    fn write_entry(&mut self, id: &str, data: EntryData) -> Result<()> {
        let key = self.key.as_ref().ok_or(VaultError::Locked)?;
        let plaintext = Zeroizing::new(serde_json::to_vec(&data).map_err(|_| VaultError::Damaged)?);
        let sealed = seal(key, &plaintext, id.as_bytes())?;

        crate::storage::create_private_dir(&self.dir)?;
        let mut file = Vec::with_capacity(HEADER_LEN + sealed.len());
        file.extend_from_slice(&MAGIC);
        file.extend_from_slice(&sealed);
        atomic_write(&self.entry_path(id), &file)?;
        stamp_written(&self.entry_path(id));
        stamp_written(&self.dir);

        let info =
            EntryInfo { id: id.to_string(), name: data.name, shortcut: data.shortcut, kind: data.kind };
        match self.entries.iter_mut().find(|e| e.id == id) {
            Some(existing) => *existing = info,
            None => self.entries.push(info),
        }
        sort_entries(&mut self.entries);
        Ok(())
    }

    /// Delete an entry's file, and its blob if it has one. Unrecoverable,
    /// which is why the UI asks first.
    ///
    /// The entry goes first: interrupted in between, what is left is a blob
    /// no key will ever open again, rather than an entry whose contents are
    /// gone.
    pub fn delete(&mut self, id: &str) -> Result<()> {
        if !self.entries.iter().any(|e| e.id == id) {
            return Err(VaultError::NoSuchEntry);
        }
        std::fs::remove_file(self.entry_path(id))?;
        self.entries.retain(|e| e.id != id);
        match std::fs::remove_file(self.blob_path(id)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => {
                log::warn!("the blob of deleted vault entry {id} could not be removed: {e}");
            }
            _ => {}
        }
        stamp_written(&self.dir);
        Ok(())
    }

    /// Put a copy of the file at `source` in the vault. The original is left
    /// where it is; removing it is up to the user.
    pub fn add_file(&mut self, name: &str, shortcut: &str, source: &Path) -> Result<String> {
        if std::fs::metadata(source)?.len() > MAX_BLOB_BYTES {
            return Err(VaultError::TooLarge);
        }
        let file_name = display_name(source);
        self.add_blob(name, shortcut, EntryKind::File, file_name, |blob| {
            io::copy(&mut std::fs::File::open(source)?, blob).map(drop)
        })
    }

    /// Put a copy of the folder at `source`, with everything in it, in the
    /// vault. Links inside it are not followed, and the vault folder itself is
    /// left out should it be somewhere inside.
    pub fn add_folder(&mut self, name: &str, shortcut: &str, source: &Path) -> Result<String> {
        // Zipping the vault into itself would read the blob being written.
        let source_real = source.canonicalize()?;
        if self.dir.canonicalize().is_ok_and(|vault| source_real.starts_with(vault)) {
            return Err(VaultError::InsideVault);
        }
        let vault_dir = self.dir.clone();
        if vault_blob::folder_size(source, Some(&vault_dir))? > MAX_BLOB_BYTES {
            return Err(VaultError::TooLarge);
        }
        let folder_name = display_name(source);
        self.add_blob(name, shortcut, EntryKind::Folder, folder_name, |blob| {
            vault_blob::zip_folder(source, Some(&vault_dir), blob)
        })
    }

    /// Write a blob under a fresh key with `fill`, then the entry that holds
    /// that key. In that order, so the entry never exists without its blob.
    fn add_blob(
        &mut self,
        name: &str,
        shortcut: &str,
        kind: EntryKind,
        original_name: String,
        fill: impl FnOnce(&mut BlobWriter<std::fs::File>) -> io::Result<()>,
    ) -> Result<String> {
        if self.key.is_none() {
            return Err(VaultError::Locked);
        }
        crate::storage::create_private_dir(&self.dir)?;
        let id = uuid::Uuid::new_v4().to_string();
        let file_key = Zeroizing::new(<[u8; KEY_LEN]>::generate());
        let path = self.blob_path(&id);
        let tmp = path.with_extension(format!("{BLOB_EXT}.tmp"));

        let written = (|| {
            let file = crate::storage::create_private_file(&tmp)?;
            let mut blob = BlobWriter::new(file, &file_key, id.as_bytes())?;
            fill(&mut blob)?;
            let size = blob.bytes_written();
            blob.finish()?.sync_all()?;
            std::fs::rename(&tmp, &path)?;
            Ok(size)
        })();
        let size = match written {
            Ok(size) => size,
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                stamp_written(&self.dir);
                return Err(blob_error(e));
            }
        };
        stamp_written(&path);

        let data = EntryData {
            name: name.trim().to_string(),
            shortcut: shortcut.trim().to_lowercase(),
            secret: String::new(),
            kind,
            file: Some(StoredFile { name: original_name, size, key: BASE64.encode(*file_key) }),
        };
        if let Err(e) = self.write_entry(&id, data) {
            let _ = std::fs::remove_file(&path);
            stamp_written(&self.dir);
            return Err(e);
        }
        Ok(id)
    }

    /// The name a file or folder entry had when it was added: what the save
    /// dialog offers. Reduced to a bare name, never a path.
    pub fn original_name(&self, id: &str) -> Result<String> {
        let data = self.read_entry(id)?;
        if !data.kind.holds_file() {
            return Err(VaultError::WrongKind);
        }
        let file = data.file.ok_or(VaultError::Damaged)?;
        Ok(display_name(Path::new(&file.name)))
    }

    /// Decrypt a file entry to `dest`, replacing whatever is there (the save
    /// dialog has already asked).
    ///
    /// Written beside `dest` under a temporary name and only renamed into
    /// place once the last chunk has checked out, so a blob that turns out to
    /// be damaged halfway leaves nothing behind rather than half a document.
    pub fn export_file(&self, id: &str, dest: &Path) -> Result<()> {
        let (_, key) = self.stored_file(id, EntryKind::File)?;
        let tmp = temp_sibling(dest);
        let result = (|| {
            let mut blob = BlobReader::new(self.open_blob(id)?, &key, id.as_bytes())?;
            let mut out = std::fs::File::create(&tmp)?;
            io::copy(&mut blob, &mut out)?;
            out.sync_all()?;
            drop(out);
            std::fs::rename(&tmp, dest)
        })();
        stamp_read(&self.blob_path(id));
        if let Err(e) = result {
            let _ = std::fs::remove_file(&tmp);
            return Err(blob_error(e));
        }
        Ok(())
    }

    /// Decrypt a folder entry into a new folder inside `parent`, named as the
    /// folder was when it was added, with " (2)" and so on added if that name
    /// is taken: an existing folder is never merged into or replaced. Returns
    /// the folder made.
    ///
    /// Like [`Self::export_file`], it is unpacked under a temporary name and
    /// only renamed into place once all of it is out.
    pub fn export_folder(&self, id: &str, parent: &Path) -> Result<PathBuf> {
        let (stored, key) = self.stored_file(id, EntryKind::Folder)?;
        let blob = BlobReader::new(self.open_blob(id)?, &key, id.as_bytes()).map_err(blob_error)?;
        let zip = vault_blob::read_all(blob, stored.size);
        stamp_read(&self.blob_path(id));
        let zip = zip.map_err(blob_error)?;

        let target = free_name(parent, &display_name(Path::new(&stored.name)));
        let tmp = temp_sibling(&target);
        std::fs::create_dir(&tmp)?;
        let result =
            vault_blob::unzip_folder(&zip, &tmp).and_then(|()| std::fs::rename(&tmp, &target));
        if let Err(e) = result {
            let _ = std::fs::remove_dir_all(&tmp);
            return Err(blob_error(e));
        }
        Ok(target)
    }

    /// The stored-file record and blob key of a file or folder entry.
    fn stored_file(&self, id: &str, kind: EntryKind) -> Result<(StoredFile, Zeroizing<[u8; KEY_LEN]>)> {
        let data = self.read_entry(id)?;
        if data.kind != kind {
            return Err(VaultError::WrongKind);
        }
        let file = data.file.ok_or(VaultError::Damaged)?;
        let raw = Zeroizing::new(BASE64.decode(&file.key).map_err(|_| VaultError::Damaged)?);
        let key: [u8; KEY_LEN] = raw.as_slice().try_into().map_err(|_| VaultError::Damaged)?;
        Ok((file, Zeroizing::new(key)))
    }

    /// An entry whose blob is missing is as broken as one whose blob is
    /// altered, and is reported the same way.
    fn open_blob(&self, id: &str) -> io::Result<std::fs::File> {
        std::fs::File::open(self.blob_path(id)).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => io::Error::new(io::ErrorKind::InvalidData, e),
            _ => e,
        })
    }

    /// Derive the master key from `password` and unwrap the vault key with it.
    fn unwrap_key(&self, password: &str) -> Result<Zeroizing<[u8; KEY_LEN]>> {
        let text = std::fs::read_to_string(self.meta_path())?;
        // Before the password is checked: a wrong guess is no less worth hiding.
        stamp_read(&self.meta_path());
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
        crate::storage::create_private_dir(&self.dir)?;
        atomic_write_json(&self.meta_path(), &meta, Some(2))?;
        stamp_written(&self.meta_path());
        stamp_written(&self.dir);
        Ok(())
    }

    fn read_entry(&self, id: &str) -> Result<EntryData> {
        let key = self.key.as_ref().ok_or(VaultError::Locked)?;
        let raw = match std::fs::read(self.entry_path(id)) {
            Ok(raw) => {
                stamp_read(&self.entry_path(id));
                raw
            }
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
                Ok(data) => Some(EntryInfo {
                    id,
                    name: data.name,
                    shortcut: data.shortcut,
                    kind: data.kind,
                }),
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

/// [`crate::vault_blob`] reports trouble as `io::Error`s of particular kinds,
/// so they can pass through `io::copy` and the zip crate on the way.
fn blob_error(error: io::Error) -> VaultError {
    match error.kind() {
        io::ErrorKind::InvalidData => VaultError::Damaged,
        io::ErrorKind::FileTooLarge => VaultError::TooLarge,
        _ => VaultError::Io(error),
    }
}

/// The last part of `path`, or a stand-in for one that has none (a drive
/// root).
fn display_name(path: &Path) -> String {
    match path.file_name() {
        Some(name) if !name.is_empty() => name.to_string_lossy().into_owned(),
        _ => "vault item".to_string(),
    }
}

/// A name next to `path` to write it under until it is complete: hidden on
/// Unix, and unique, so it never lands on anything already there.
fn temp_sibling(path: &Path) -> PathBuf {
    let name = display_name(path);
    let tag = &uuid::Uuid::new_v4().simple().to_string()[..8];
    path.with_file_name(format!(".{name}.{tag}.tmp"))
}

/// `parent/name`, or `parent/name (2)` and up if that is already taken.
fn free_name(parent: &Path, name: &str) -> PathBuf {
    let first = parent.join(name);
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| parent.join(format!("{name} ({n})")))
        .find(|candidate| !candidate.exists())
        .expect("some numbered name is free")
}

/// The uuids of every `*.enc` file in the vault folder.
fn entry_ids(dir: &Path) -> Vec<String> {
    ids_with_ext(dir, ENTRY_EXT)
}

/// The uuids of every `*.<ext>` file in the vault folder.
fn ids_with_ext(dir: &Path, ext: &str) -> Vec<String> {
    let Ok(read_dir) = std::fs::read_dir(dir) else { return Vec::new() };
    read_dir
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|e| e == ext))
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
        let id = session.save(None, "github", "gh", "hunter2", EntryKind::Password).unwrap();
        session.save(None, "bank", "", "1234-5678", EntryKind::Password).unwrap();

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
        session.save(None, "github", "gh", "hunter2", EntryKind::Password).unwrap();
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
        let id = session.save(None, "github", "gh", "hunter2", EntryKind::Password).unwrap();

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
        let id = session.save(None, "github", "gh", "hunter2", EntryKind::Password).unwrap();
        let path = dir.path().join(VAULT_DIR).join(format!("{id}.enc"));

        let mut raw = std::fs::read(&path).unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 0x01;
        std::fs::write(&path, &raw).unwrap();
        assert!(matches!(session.secret(&id), Err(VaultError::Damaged)));

        // ...and so is an entry file moved onto another entry's name: the id
        // is authenticated alongside the ciphertext.
        std::fs::write(&path, std::fs::read(&path).unwrap()).unwrap();
        let other = session.save(None, "bank", "", "1234", EntryKind::Password).unwrap();
        let sealed = std::fs::read(dir.path().join(VAULT_DIR).join(format!("{other}.enc"))).unwrap();
        std::fs::write(&path, sealed).unwrap();
        assert!(matches!(session.secret(&id), Err(VaultError::Damaged)));
    }

    #[test]
    fn saving_over_an_entry_replaces_it_instead_of_adding_a_second() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = vault(dir.path());
        let id = session.save(None, "github", "gh", "hunter2", EntryKind::Password).unwrap();
        let same = session.save(Some(&id), "github work", "gh", "hunter3", EntryKind::Password).unwrap();

        assert_eq!(same, id);
        assert_eq!(session.entries().len(), 1);
        assert_eq!(session.entries()[0].name, "github work");
        assert_eq!(&*session.secret(&id).unwrap(), "hunter3");
    }

    #[test]
    fn deleting_takes_the_file_with_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = vault(dir.path());
        let id = session.save(None, "github", "gh", "hunter2", EntryKind::Password).unwrap();
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
        let id = session.save(None, "github", "gh", "hunter2", EntryKind::Password).unwrap();
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
        let id = session.save(None, "github", "gh", "hunter2", EntryKind::Password).unwrap();

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
        session.save(None, "github", "gh", "hunter2", EntryKind::Password).unwrap();

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
        session.save(None, "github", "gh", "hunter2", EntryKind::Password).unwrap();
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
        let id = session.save(None, "github", "gh", "hunter2", EntryKind::Password).unwrap();
        session.lock();
        session.unlock(PASSWORD, now()).unwrap();
        println!("Argon2id at {:?}: {:?} for create + unlock", Kdf::STRONG, started.elapsed());
        assert_eq!(&*session.secret(&id).unwrap(), "hunter2");
    }

    #[test]
    fn one_damaged_entry_does_not_take_the_others_down() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = vault(dir.path());
        let broken = session.save(None, "github", "gh", "hunter2", EntryKind::Password).unwrap();
        session.save(None, "bank", "", "1234", EntryKind::Password).unwrap();
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

    /// Every entry already in a user's vault was written before `kind`
    /// existed. They must keep opening, as passwords, or the authenticator
    /// feature would eat the vault it was added to.
    #[test]
    fn an_entry_written_before_kinds_existed_still_opens_as_a_password() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = vault(dir.path());
        let id = session.save(None, "github", "gh", "placeholder", EntryKind::Password).unwrap();

        // Reseal the payload in the old shape: no `kind` field at all.
        let legacy = br#"{"name":"github","shortcut":"gh","secret":"hunter2"}"#;
        let key = session.key.as_ref().unwrap();
        let sealed = seal(key, legacy, id.as_bytes()).unwrap();
        let mut file = MAGIC.to_vec();
        file.extend_from_slice(&sealed);
        std::fs::write(dir.path().join(VAULT_DIR).join(format!("{id}.enc")), file).unwrap();

        session.lock();
        session.unlock(PASSWORD, now()).unwrap();
        assert_eq!(session.entries().len(), 1);
        assert_eq!(session.entries()[0].kind, EntryKind::Password);
        assert_eq!(&*session.secret(&id).unwrap(), "hunter2");
    }

    /// A file to add, and somewhere to save things back out to.
    fn document(dir: &Path, name: &str, contents: &[u8]) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }

    fn blob_of(dir: &Path, id: &str) -> PathBuf {
        dir.join(VAULT_DIR).join(format!("{id}.{BLOB_EXT}"))
    }

    #[test]
    fn a_file_comes_back_out_exactly_after_a_lock() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = vault(dir.path());
        let contents: Vec<u8> = (0..200_000u32).map(|i| (i % 253) as u8).collect();
        let source = document(dir.path(), "manual.pdf", &contents);
        let id = session.add_file("printer manual", "pm", &source).unwrap();

        session.lock();
        session.unlock(PASSWORD, now()).unwrap();
        assert_eq!(session.entries()[0].kind, EntryKind::File);
        assert_eq!(session.original_name(&id).unwrap(), "manual.pdf");

        let out = dir.path().join("saved.pdf");
        session.export_file(&id, &out).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), contents);
        assert!(source.exists(), "the original is left alone");
    }

    #[test]
    fn no_file_in_the_vault_dates_from_after_2005_and_all_still_open() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = vault(dir.path());
        let password = session.save(None, "github", "gh", "hunter2", EntryKind::Password).unwrap();
        let source = document(dir.path(), "manual.pdf", b"contents");
        let file = session.add_file("printer manual", "pm", &source).unwrap();
        session.lock();
        session.unlock(PASSWORD, now()).unwrap();
        session.secret(&password).unwrap();
        session.export_file(&file, &dir.path().join("out.pdf")).unwrap();

        let vault_dir = dir.path().join(VAULT_DIR);
        let after_2005 = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_136_073_600);
        // The folder first: listing it below is a read of it in its own right.
        let mut dated = vec![(vault_dir.clone(), std::fs::metadata(&vault_dir).unwrap())];
        for entry in std::fs::read_dir(&vault_dir).unwrap() {
            let path = entry.unwrap().path();
            dated.push((path.clone(), std::fs::metadata(&path).unwrap()));
        }
        for (path, meta) in dated {
            assert!(meta.modified().unwrap() < after_2005, "{}", path.display());
            assert!(meta.accessed().unwrap() < after_2005, "{}", path.display());
            #[cfg(windows)]
            assert!(meta.created().unwrap() < after_2005, "{}", path.display());
        }

        session.lock();
        session.unlock(PASSWORD, now()).unwrap();
        assert_eq!(*session.secret(&password).unwrap(), "hunter2");
        session.export_file(&file, &dir.path().join("again.pdf")).unwrap();
        assert_eq!(std::fs::read(dir.path().join("again.pdf")).unwrap(), b"contents");
    }

    #[test]
    fn a_folder_comes_back_out_beside_any_folder_of_the_same_name() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = vault(dir.path());
        let source = dir.path().join("taxes");
        std::fs::create_dir_all(source.join("2025")).unwrap();
        std::fs::write(source.join("2025/return.pdf"), b"numbers").unwrap();
        let id = session.add_folder("taxes", "", &source).unwrap();

        let out = dir.path().join("out");
        std::fs::create_dir(&out).unwrap();
        let first = session.export_folder(&id, &out).unwrap();
        assert_eq!(first, out.join("taxes"));
        assert_eq!(std::fs::read(first.join("2025/return.pdf")).unwrap(), b"numbers");

        let second = session.export_folder(&id, &out).unwrap();
        assert_eq!(second, out.join("taxes (2)"), "an existing folder is never merged into");
        let leftovers: Vec<_> = std::fs::read_dir(&out).unwrap().collect();
        assert_eq!(leftovers.len(), 2, "no temporary folder is left behind");
    }

    #[test]
    fn neither_the_name_nor_the_contents_of_a_file_are_readable_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = vault(dir.path());
        let source = document(dir.path(), "passport-scan.pdf", b"%PDF-1.7 passport number X123");
        let id = session.add_file("passport", "", &source).unwrap();

        let entry = std::fs::read(dir.path().join(VAULT_DIR).join(format!("{id}.enc"))).unwrap();
        let blob = std::fs::read(blob_of(dir.path(), &id)).unwrap();
        for plaintext in ["passport", "%PDF", "X123"] {
            for (what, raw) in [("entry", &entry), ("blob", &blob)] {
                assert!(
                    !raw.windows(plaintext.len()).any(|w| w == plaintext.as_bytes()),
                    "{plaintext:?} is readable in the {what} file"
                );
            }
        }
    }

    #[test]
    fn a_damaged_or_missing_blob_is_refused_and_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = vault(dir.path());
        let source = document(dir.path(), "a.txt", &[1u8; 100_000]);
        let id = session.add_file("a", "", &source).unwrap();
        let out_dir = dir.path().join("out");
        std::fs::create_dir(&out_dir).unwrap();
        let out = out_dir.join("a.txt");

        let blob = blob_of(dir.path(), &id);
        let mut raw = std::fs::read(&blob).unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 1;
        std::fs::write(&blob, &raw).unwrap();
        assert!(matches!(session.export_file(&id, &out), Err(VaultError::Damaged)));
        assert_eq!(std::fs::read_dir(&out_dir).unwrap().count(), 0, "not even half a file");

        std::fs::remove_file(&blob).unwrap();
        assert!(matches!(session.export_file(&id, &out), Err(VaultError::Damaged)));
    }

    /// Each blob has its own key, held only by its own entry, so one entry's
    /// blob moved onto another's name opens for neither.
    #[test]
    fn blobs_cannot_be_swapped_between_entries() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = vault(dir.path());
        let a = session.add_file("a", "", &document(dir.path(), "a.txt", b"aaa")).unwrap();
        let b = session.add_file("b", "", &document(dir.path(), "b.txt", b"bbb")).unwrap();
        std::fs::copy(blob_of(dir.path(), &b), blob_of(dir.path(), &a)).unwrap();
        let out = dir.path().join("out.txt");
        assert!(matches!(session.export_file(&a, &out), Err(VaultError::Damaged)));
    }

    #[test]
    fn deleting_a_file_entry_takes_its_blob_with_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = vault(dir.path());
        let id = session.add_file("a", "", &document(dir.path(), "a.txt", b"aaa")).unwrap();
        assert!(blob_of(dir.path(), &id).exists());
        session.delete(&id).unwrap();
        assert!(!blob_of(dir.path(), &id).exists());
    }

    /// `save` rewrites the whole payload; on a file entry that would throw
    /// away the only copy of the blob's key. Renaming is the way to edit one.
    #[test]
    fn a_file_entry_is_renamed_rather_than_saved_over() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = vault(dir.path());
        let id = session.add_file("a", "", &document(dir.path(), "a.txt", b"aaa")).unwrap();
        assert!(matches!(
            session.save(Some(&id), "a", "", "text", EntryKind::Password),
            Err(VaultError::WrongKind)
        ));
        assert!(matches!(
            session.save(None, "x", "", "", EntryKind::File),
            Err(VaultError::WrongKind)
        ));

        session.rename(&id, "Renamed", "RN").unwrap();
        session.lock();
        session.unlock(PASSWORD, now()).unwrap();
        assert_eq!(session.entries()[0].name, "Renamed");
        assert_eq!(session.entries()[0].shortcut, "rn");
        let out = dir.path().join("out.txt");
        session.export_file(&id, &out).unwrap();
        assert_eq!(std::fs::read(out).unwrap(), b"aaa");
    }

    #[test]
    fn password_and_file_operations_do_not_mix() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = vault(dir.path());
        let password = session.save(None, "github", "", "hunter2", EntryKind::Password).unwrap();
        let file = session.add_file("a", "", &document(dir.path(), "a.txt", b"aaa")).unwrap();
        let out = dir.path().join("out");
        assert!(matches!(session.export_file(&password, &out), Err(VaultError::WrongKind)));
        assert!(matches!(session.export_folder(&file, dir.path()), Err(VaultError::WrongKind)));
        assert!(matches!(session.original_name(&password), Err(VaultError::WrongKind)));
    }

    /// Entries that were already there must seal to exactly what they did
    /// before files existed, so nothing about them changes.
    #[test]
    fn a_password_entry_carries_no_file_field() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = vault(dir.path());
        let id = session.save(None, "github", "gh", "hunter2", EntryKind::Password).unwrap();
        let raw = std::fs::read(dir.path().join(VAULT_DIR).join(format!("{id}.enc"))).unwrap();
        let key = session.key.as_ref().unwrap();
        let plain = open(key, &raw[MAGIC.len()..], id.as_bytes(), VaultError::Damaged).unwrap();
        assert_eq!(
            std::str::from_utf8(&plain).unwrap(),
            r#"{"name":"github","shortcut":"gh","secret":"hunter2","kind":"password"}"#
        );
    }

    #[test]
    fn the_vault_folder_cannot_be_added_to_itself() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = vault(dir.path());
        let vault_dir = dir.path().join(VAULT_DIR);
        assert!(matches!(session.add_folder("v", "", &vault_dir), Err(VaultError::InsideVault)));

        // Its parent can be, with the vault left out of it.
        std::fs::write(dir.path().join("notes.txt"), b"n").unwrap();
        let id = session.add_folder("app", "", dir.path()).unwrap();
        let out = tempfile::tempdir().unwrap();
        let back = session.export_folder(&id, out.path()).unwrap();
        assert!(back.join("notes.txt").exists());
        assert!(!back.join(VAULT_DIR).exists());
    }

    #[test]
    fn a_file_over_the_limit_is_refused_before_it_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = vault(dir.path());
        let path = dir.path().join("huge.iso");
        std::fs::File::create(&path).unwrap().set_len(MAX_BLOB_BYTES + 1).unwrap();
        assert!(matches!(session.add_file("iso", "", &path), Err(VaultError::TooLarge)));
        assert!(session.entries().is_empty());
        let leftovers = std::fs::read_dir(dir.path().join(VAULT_DIR)).unwrap().count();
        assert_eq!(leftovers, 1, "only vault.meta");
    }

    #[test]
    fn an_authenticator_entry_keeps_its_kind_across_a_lock() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = vault(dir.path());
        let id = session
            .save(None, "github code", "ghc", "JBSWY3DPEHPK3PXP", EntryKind::Totp)
            .unwrap();

        session.lock();
        session.unlock(PASSWORD, now()).unwrap();
        assert_eq!(session.entries()[0].kind, EntryKind::Totp);
        let (info, secret) = session.entry(&id).unwrap();
        assert_eq!(info.kind, EntryKind::Totp);
        assert_eq!(&*secret, "JBSWY3DPEHPK3PXP");
    }
}
