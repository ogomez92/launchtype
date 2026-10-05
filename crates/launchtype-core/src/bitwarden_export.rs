//! Reading a file made by Bitwarden's "Export vault".
//!
//! Bitwarden writes four kinds of file, and this module tells them apart by
//! what is in them rather than by the name they were saved under:
//!
//! * **JSON** — `{"encrypted": false, "items": [...]}`, every item in the
//!   same shape [`crate::bitwarden::BwItem`] reads. The organisation export
//!   has the same shape, and the bare array `bw list items` prints is taken
//!   too.
//! * **JSON, password protected** — the JSON above, sealed with a password
//!   chosen at export time. Opened by [`ProtectedExport::unlock`]; see below.
//! * **JSON, account restricted** — every value sealed with the account's own
//!   key. Nothing outside a Bitwarden client logged into that account can read
//!   it, so it is recognised only to say so.
//! * **CSV** — logins and secure notes only, since that is all Bitwarden puts
//!   in one: no cards, identities or SSH keys.
//!
//! The `.zip` export that carries attachments is recognised and turned away
//! with the same advice as the account-restricted one: export again as JSON.
//!
//! # The password-protected format
//!
//! Taken from Bitwarden's own exporter (`bitwarden-exporters` in its SDK, and
//! `BitwardenPasswordProtectedImporter` in its clients):
//!
//! 1. The password is stretched with the KDF the file names — PBKDF2-SHA256,
//!    or Argon2id with the salt hashed through SHA-256 first — into 32 bytes.
//!    The salt is the *base64 text* in the file, used as it is, not decoded.
//! 2. Those 32 bytes are expanded with HKDF-SHA256 (no extract step) into an
//!    encryption key (info `"enc"`) and a MAC key (info `"mac"`).
//! 3. `data` is an "EncString" of type 2, `2.<iv>|<ciphertext>|<mac>`:
//!    AES-256-CBC with PKCS#7 padding, then HMAC-SHA256 over iv + ciphertext.
//!
//! `encKeyValidation_DO_NOT_EDIT` is a random value sealed the same way. It
//! exists so a wrong password can be told apart from a damaged file: if that
//! opens, the password was right.
//!
//! The MAC is checked before anything is decrypted, which is also what makes a
//! wrong password come out as a clean refusal rather than as garbage.

use aes::Aes256;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use cbc::cipher::block_padding::Pkcs7;
use cbc::cipher::{BlockModeDecrypt, KeyIvInit};
use hmac::{Hmac, KeyInit, Mac};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::bitwarden::{BwField, BwItem, BwLogin, TYPE_LOGIN, TYPE_SECURE_NOTE};

/// What was in the file.
#[derive(Debug)]
pub enum Export {
    /// Readable as it stands.
    Items(Vec<BwItem>),
    /// Needs the export password first.
    PasswordProtected(ProtectedExport),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ExportError {
    #[error("not a Bitwarden export")]
    NotAnExport,
    /// Sealed with the account's own key: only Bitwarden can open it.
    #[error("an account-restricted export")]
    AccountRestricted,
    /// The `.zip` export with attachments.
    #[error("a zip export")]
    Zip,
    /// A key derivation this module does not know, or settings outside what
    /// Bitwarden itself allows.
    #[error("an unsupported key derivation")]
    UnsupportedKdf,
    #[error("wrong export password")]
    WrongPassword,
    /// The password was right but the contents would not open or parse.
    #[error("the export is damaged")]
    Damaged,
}

/// The sealed half of a password-protected export, kept until the user has
/// typed the password.
pub struct ProtectedExport {
    salt: String,
    kdf: Kdf,
    validation: String,
    data: String,
}

/// Hand-written so the sealed data does not end up in a log line; it is
/// ciphertext, but there is no reason to spread it about either.
impl std::fmt::Debug for ProtectedExport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProtectedExport").field("kdf", &self.kdf).finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kdf {
    Pbkdf2 { iterations: u32 },
    /// Memory in MiB, as Bitwarden stores it.
    Argon2id { iterations: u32, memory_mib: u32, parallelism: u32 },
}

/// The top of a JSON export. `items` is read separately, once it is known the
/// export is not encrypted: in an account-restricted file every value in there
/// is ciphertext and not worth the attempt.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsonHeader {
    #[serde(default)]
    encrypted: bool,
    #[serde(default)]
    password_protected: bool,
    #[serde(default)]
    salt: Option<String>,
    #[serde(default)]
    kdf_type: Option<u8>,
    #[serde(default)]
    kdf_iterations: Option<u32>,
    #[serde(default)]
    kdf_memory: Option<u32>,
    #[serde(default)]
    kdf_parallelism: Option<u32>,
    #[serde(rename = "encKeyValidation_DO_NOT_EDIT", default)]
    enc_key_validation: Option<String>,
    #[serde(default)]
    data: Option<String>,
}

#[derive(Deserialize)]
struct JsonItems {
    items: Vec<BwItem>,
}

/// Work out what `bytes` is and read as much of it as can be read without a
/// password.
pub fn read_export(bytes: &[u8]) -> Result<Export, ExportError> {
    if bytes.starts_with(b"PK\x03\x04") {
        return Err(ExportError::Zip);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| ExportError::NotAnExport)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text).trim_start();
    match text.chars().next() {
        Some('{') => read_json(text),
        Some('[') => serde_json::from_str(text).map(Export::Items).map_err(|_| ExportError::NotAnExport),
        Some(_) => read_csv(text).map(Export::Items),
        None => Err(ExportError::NotAnExport),
    }
}

fn read_json(text: &str) -> Result<Export, ExportError> {
    let header: JsonHeader = serde_json::from_str(text).map_err(|_| ExportError::NotAnExport)?;
    if !header.encrypted {
        let items: JsonItems = serde_json::from_str(text).map_err(|_| ExportError::NotAnExport)?;
        return Ok(Export::Items(items.items));
    }
    if !header.password_protected {
        return Err(ExportError::AccountRestricted);
    }

    let iterations = header.kdf_iterations.ok_or(ExportError::Damaged)?;
    let kdf = match header.kdf_type {
        Some(0) => Kdf::Pbkdf2 { iterations },
        Some(1) => Kdf::Argon2id {
            iterations,
            memory_mib: header.kdf_memory.ok_or(ExportError::Damaged)?,
            parallelism: header.kdf_parallelism.ok_or(ExportError::Damaged)?,
        },
        _ => return Err(ExportError::UnsupportedKdf),
    };
    check_kdf(kdf)?;
    Ok(Export::PasswordProtected(ProtectedExport {
        salt: header.salt.filter(|s| !s.is_empty()).ok_or(ExportError::Damaged)?,
        kdf,
        validation: header.enc_key_validation.ok_or(ExportError::Damaged)?,
        data: header.data.ok_or(ExportError::Damaged)?,
    }))
}

/// The ranges Bitwarden lets an account choose. Anything outside them did not
/// come from Bitwarden, and a file asking for a terabyte of Argon2 memory or a
/// billion PBKDF2 rounds should be refused, not obeyed.
fn check_kdf(kdf: Kdf) -> Result<(), ExportError> {
    let ok = match kdf {
        Kdf::Pbkdf2 { iterations } => (1..=2_000_000).contains(&iterations),
        Kdf::Argon2id { iterations, memory_mib, parallelism } => {
            (1..=10).contains(&iterations) && (1..=1024).contains(&memory_mib) && (1..=16).contains(&parallelism)
        }
    };
    if ok {
        Ok(())
    } else {
        Err(ExportError::UnsupportedKdf)
    }
}

impl ProtectedExport {
    /// Open the export with the password it was made with. Stretching takes
    /// as long as the file's KDF settings say — around a second at
    /// Bitwarden's defaults.
    pub fn unlock(&self, password: &str) -> Result<Vec<BwItem>, ExportError> {
        let keys = stretch(&*derive(password, &self.salt, self.kdf)?);
        // A validation value that will not open means the wrong password; one
        // that opens followed by data that will not means a damaged file.
        decrypt(&self.validation, &keys).map_err(|_| ExportError::WrongPassword)?;
        let plain = decrypt(&self.data, &keys).map_err(|_| ExportError::Damaged)?;
        let text = std::str::from_utf8(&plain).map_err(|_| ExportError::Damaged)?;
        // What comes out is an ordinary unencrypted JSON export.
        match read_json(text) {
            Ok(Export::Items(items)) => Ok(items),
            _ => Err(ExportError::Damaged),
        }
    }
}

/// Step 1: the password stretched into 32 bytes.
fn derive(password: &str, salt: &str, kdf: Kdf) -> Result<Zeroizing<[u8; 32]>, ExportError> {
    let mut key = Zeroizing::new([0u8; 32]);
    match kdf {
        Kdf::Pbkdf2 { iterations } => {
            pbkdf2::pbkdf2_hmac::<Sha256>(password.as_bytes(), salt.as_bytes(), iterations, key.as_mut_slice());
        }
        Kdf::Argon2id { iterations, memory_mib, parallelism } => {
            use argon2::{Algorithm, Argon2, Params, Version};
            let salt_hash = Sha256::digest(salt.as_bytes());
            let params = Params::new(memory_mib * 1024, iterations, parallelism, Some(32))
                .map_err(|_| ExportError::UnsupportedKdf)?;
            Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
                .hash_password_into(password.as_bytes(), &salt_hash, key.as_mut_slice())
                .map_err(|_| ExportError::UnsupportedKdf)?;
        }
    }
    Ok(key)
}

/// The encryption and MAC keys a stretched password expands into.
struct Keys {
    enc: Zeroizing<[u8; 32]>,
    mac: Zeroizing<[u8; 32]>,
}

/// Step 2: HKDF-Expand straight from the derived key, as Bitwarden does.
fn stretch(key: &[u8; 32]) -> Keys {
    let hkdf = hkdf::Hkdf::<Sha256>::from_prk(key).expect("32 bytes is a valid SHA-256 PRK");
    let mut enc = Zeroizing::new([0u8; 32]);
    let mut mac = Zeroizing::new([0u8; 32]);
    hkdf.expand(b"enc", enc.as_mut_slice()).expect("32 bytes is a valid HKDF length");
    hkdf.expand(b"mac", mac.as_mut_slice()).expect("32 bytes is a valid HKDF length");
    Keys { enc, mac }
}

/// Step 3: open a type 2 EncString. Every failure is the same failure — the
/// caller only needs to know it did not open.
fn decrypt(enc_string: &str, keys: &Keys) -> Result<Zeroizing<Vec<u8>>, ()> {
    let body = enc_string.trim().strip_prefix("2.").ok_or(())?;
    let mut parts = body.split('|');
    let (Some(iv), Some(data), Some(mac), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return Err(());
    };
    let iv = BASE64.decode(iv).map_err(|_| ())?;
    let data = BASE64.decode(data).map_err(|_| ())?;
    let mac = BASE64.decode(mac).map_err(|_| ())?;
    if iv.len() != 16 {
        return Err(());
    }

    let mut check = <Hmac<Sha256> as KeyInit>::new_from_slice(keys.mac.as_slice()).map_err(|_| ())?;
    check.update(&iv);
    check.update(&data);
    check.verify_slice(&mac).map_err(|_| ())?;

    let mut buffer = Zeroizing::new(data);
    let decryptor = cbc::Decryptor::<Aes256>::new_from_slices(keys.enc.as_slice(), &iv).map_err(|_| ())?;
    let len = decryptor.decrypt_padded::<Pkcs7>(&mut buffer).map_err(|_| ())?.len();
    buffer.truncate(len);
    Ok(buffer)
}

/// Bitwarden's CSV export: a header row naming the columns, then one row per
/// login or note. The organisation export swaps `folder` for `collections`;
/// neither is read, so both work.
fn read_csv(text: &str) -> Result<Vec<BwItem>, ExportError> {
    let mut rows = parse_csv(text).into_iter();
    let header = rows.next().ok_or(ExportError::NotAnExport)?;
    let column = |name: &str| header.iter().position(|h| h.trim() == name);
    let (Some(name), Some(kind)) = (column("name"), column("type")) else {
        return Err(ExportError::NotAnExport);
    };
    // Without these it is somebody else's CSV that happens to have a name and
    // a type column, and reading it as Bitwarden's would import nonsense.
    if column("login_password").is_none() && column("login_totp").is_none() {
        return Err(ExportError::NotAnExport);
    }
    let notes = column("notes");
    let fields = column("fields");
    let username = column("login_username");
    let password = column("login_password");
    let totp = column("login_totp");

    let cell = |row: &[String], index: Option<usize>| -> Option<String> {
        index.and_then(|i| row.get(i)).filter(|v| !v.is_empty()).cloned()
    };

    Ok(rows
        .filter(|row| row.iter().any(|c| !c.is_empty()))
        .map(|row| {
            let item_type = match row.get(kind).map(|t| t.trim().to_ascii_lowercase()).as_deref() {
                Some("login") => TYPE_LOGIN,
                Some("note") => TYPE_SECURE_NOTE,
                // Counted by the planner as a kind it does not know.
                _ => 0,
            };
            BwItem {
                name: row.get(name).cloned().unwrap_or_default(),
                item_type,
                notes: cell(&row, notes),
                fields: cell(&row, fields).map(|f| parse_csv_fields(&f)).unwrap_or_default(),
                login: (item_type == TYPE_LOGIN).then(|| BwLogin {
                    username: cell(&row, username),
                    password: cell(&row, password),
                    totp: cell(&row, totp),
                }),
                ..BwItem::default()
            }
        })
        .collect())
}

/// Custom fields in a CSV are packed into one cell as `name: value` lines.
/// Split at the *last* `": "`, the way Bitwarden's own importer reads them
/// back, and skip lines without one.
fn parse_csv_fields(cell: &str) -> Vec<BwField> {
    cell.lines()
        .filter_map(|line| {
            let at = line.rfind(": ")?;
            Some(BwField {
                name: Some(line[..at].to_string()),
                value: Some(line[at + 2..].to_string()),
                field_type: 0,
            })
        })
        .collect()
}

/// RFC 4180: commas between cells, double quotes around any cell holding a
/// comma, quote or line break, and a quote inside one written twice. Notes and
/// fields routinely span lines, so a line-at-a-time split would not do.
fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    cell.push('"');
                    chars.next();
                }
                '"' => quoted = false,
                _ => cell.push(c),
            }
            continue;
        }
        match c {
            '"' => quoted = true,
            ',' => row.push(std::mem::take(&mut cell)),
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' | '\r' => {
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
            }
            _ => cell.push(c),
        }
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwarden::plan_import;

    fn items(bytes: &[u8]) -> Vec<BwItem> {
        match read_export(bytes) {
            Ok(Export::Items(items)) => items,
            other => panic!("expected items, got {other:?}"),
        }
    }

    fn names(items: &[BwItem]) -> Vec<String> {
        plan_import(items, &[]).entries.iter().map(|e| e.name.clone()).collect()
    }

    /// The known answers Bitwarden's SDK tests its own KDFs and key stretching
    /// against (`bitwarden-crypto`, `kdf.rs` and `keys/utils.rs`).
    const KDF_ANSWER: [u8; 32] = [
        31, 79, 104, 226, 150, 71, 177, 90, 194, 80, 172, 209, 17, 129, 132, 81, 138, 167, 69, 167, 254, 149, 2,
        27, 39, 197, 64, 42, 22, 195, 86, 75,
    ];

    #[test]
    fn pbkdf2_matches_bitwarden() {
        let key = derive("67t9b5g67$%Dh89n", "test_key", Kdf::Pbkdf2 { iterations: 10_000 }).unwrap();
        assert_eq!(*key, KDF_ANSWER);
    }

    #[test]
    fn argon2id_matches_bitwarden() {
        let kdf = Kdf::Argon2id { iterations: 4, memory_mib: 32, parallelism: 2 };
        let key = derive("67t9b5g67$%Dh89n", "test_key", kdf).unwrap();
        assert_eq!(
            *key,
            [
                207, 240, 225, 177, 162, 19, 163, 76, 98, 106, 179, 175, 224, 9, 17, 240, 20, 147, 237, 47, 246,
                150, 141, 184, 62, 225, 131, 242, 51, 53, 225, 242
            ]
        );
    }

    #[test]
    fn stretching_matches_bitwarden() {
        let keys = stretch(&KDF_ANSWER);
        assert_eq!(
            *keys.enc,
            [
                111, 31, 178, 45, 238, 152, 37, 114, 143, 215, 124, 83, 135, 173, 195, 23, 142, 134, 120, 249, 61,
                132, 163, 182, 113, 197, 189, 204, 188, 21, 237, 96
            ]
        );
        assert_eq!(
            *keys.mac,
            [
                221, 127, 206, 234, 101, 27, 202, 38, 86, 52, 34, 28, 78, 28, 185, 16, 48, 61, 127, 166, 209, 247,
                194, 87, 232, 26, 48, 85, 193, 249, 179, 155
            ]
        );
    }

    /// A password-protected export built outside this crate — PBKDF2, HKDF
    /// and HMAC from Python's standard library, AES-256-CBC from OpenSSL —
    /// following Bitwarden's format, so the whole chain is checked against an
    /// implementation that is not this one. Password: "correct horse".
    const PROTECTED: &str = include_str!("../tests/data/bitwarden-protected.json");

    fn protected() -> ProtectedExport {
        match read_export(PROTECTED.as_bytes()) {
            Ok(Export::PasswordProtected(p)) => p,
            other => panic!("expected a password-protected export, got {other:?}"),
        }
    }

    #[test]
    fn a_password_protected_export_opens_with_its_password() {
        let items = protected().unlock("correct horse").expect("the right password opens it");
        assert_eq!(names(&items), ["GitHub (oriol)", "GitHub (oriol) (code)"]);
    }

    #[test]
    fn a_wrong_export_password_is_told_apart_from_a_damaged_file() {
        assert_eq!(protected().unlock("wrong horse").unwrap_err(), ExportError::WrongPassword);
        assert_eq!(protected().unlock("").unwrap_err(), ExportError::WrongPassword);
    }

    #[test]
    fn a_tampered_export_is_refused_rather_than_decrypted() {
        let mut export = protected();
        // Flip one character of the ciphertext; the MAC must catch it.
        let mut data: Vec<char> = export.data.chars().collect();
        let i = data.iter().position(|&c| c == '|').unwrap() + 5;
        data[i] = if data[i] == 'A' { 'B' } else { 'A' };
        export.data = data.into_iter().collect();
        assert_eq!(export.unlock("correct horse").unwrap_err(), ExportError::Damaged);
    }

    #[test]
    fn an_account_restricted_export_is_recognised() {
        let json = r#"{"encrypted": true, "encKeyValidation_DO_NOT_EDIT": "2.a|b|c",
            "folders": [], "items": [{"type": 1, "name": "2.x|y|z"}]}"#;
        assert_eq!(read_export(json.as_bytes()).unwrap_err(), ExportError::AccountRestricted);
    }

    #[test]
    fn absurd_kdf_settings_are_refused_rather_than_obeyed() {
        let json = r#"{"encrypted": true, "passwordProtected": true, "salt": "c2FsdA==",
            "kdfType": 1, "kdfIterations": 3, "kdfMemory": 1000000, "kdfParallelism": 4,
            "encKeyValidation_DO_NOT_EDIT": "2.a|b|c", "data": "2.a|b|c"}"#;
        assert_eq!(read_export(json.as_bytes()).unwrap_err(), ExportError::UnsupportedKdf);
        let unknown = json.replace(r#""kdfType": 1"#, r#""kdfType": 7"#);
        assert_eq!(read_export(unknown.as_bytes()).unwrap_err(), ExportError::UnsupportedKdf);
    }

    #[test]
    fn a_zip_export_is_recognised() {
        assert_eq!(read_export(b"PK\x03\x04rest of a zip").unwrap_err(), ExportError::Zip);
    }

    #[test]
    fn something_else_entirely_is_not_an_export() {
        for bytes in [&b""[..], b"   ", b"{\"commands\": []}", b"hello,world\n1,2\n", b"\xff\xfe\x00"] {
            assert_eq!(read_export(bytes).unwrap_err(), ExportError::NotAnExport, "{bytes:?}");
        }
    }

    /// The unencrypted export Bitwarden's SDK writes in its own tests
    /// (`bitwarden-exporters/resources/json_export.json`), cut down to one of
    /// each kind of item and with the fields this module ignores left in.
    #[test]
    fn bitwardens_own_json_export_reads() {
        let json = r#"{
          "encrypted": false,
          "folders": [{"id": "942e2984-1b9a-453b-b039-b107012713b9", "name": "Important"}],
          "items": [
            {
              "passwordHistory": null, "revisionDate": "2024-01-30T14:09:33.753Z",
              "creationDate": "2024-01-30T11:23:54.416Z", "deletedDate": null,
              "id": "25c8c414-b446-48e9-a1bd-b10700bbd740", "organizationId": null,
              "folderId": "942e2984-1b9a-453b-b039-b107012713b9", "type": 1, "reprompt": 0,
              "name": "Bitwarden", "notes": "My note", "favorite": true,
              "fields": [
                {"name": "Text", "value": "A", "type": 0, "linkedId": null},
                {"name": "Hidden", "value": "B", "type": 1, "linkedId": null},
                {"name": "Boolean (true)", "value": "true", "type": 2, "linkedId": null},
                {"name": "Linked", "value": null, "type": 3, "linkedId": 101}
              ],
              "login": {
                "fido2Credentials": [],
                "uris": [{"match": null, "uri": "https://vault.bitwarden.com"}],
                "username": "test@bitwarden.com", "password": "asdfasdfasdf", "totp": "ABC"
              },
              "collectionIds": null
            },
            {
              "id": "23f0f877-42b1-4820-a850-b10700bc41eb", "type": 2, "name": "My secure note",
              "notes": "Very secure!", "secureNote": {"type": 0}, "fields": null, "deletedDate": null
            },
            {
              "id": "3ed8de45-48ee-4e26-a2dc-b10701276c53", "type": 3, "name": "My card", "notes": null,
              "card": {"cardholderName": "John Doe", "expMonth": "1", "expYear": "2032",
                       "code": "123", "brand": "Visa", "number": "4111111111111111"},
              "deletedDate": null
            },
            {
              "id": "41cc3bc1-c3d9-4637-876c-b10701273712", "type": 4, "name": "My identity",
              "identity": {"title": "Mr", "firstName": "John", "middleName": null, "lastName": "Doe",
                           "address1": null, "address2": null, "address3": null, "city": null,
                           "state": null, "postalCode": null, "country": null, "company": "Bitwarden",
                           "email": null, "phone": null, "ssn": null, "username": "JDoe",
                           "passportNumber": null, "licenseNumber": null},
              "deletedDate": null
            },
            {
              "id": "646594a9-a9cb-4082-9d57-0024c3fbcaa9", "type": 5, "name": "My ssh key",
              "sshKey": {"privateKey": "-----BEGIN OPENSSH PRIVATE KEY-----\nx\n-----END OPENSSH PRIVATE KEY-----",
                         "publicKey": "ssh-ed25519 AAAA", "fingerprint": "SHA256:x"},
              "deletedDate": null
            }
          ]
        }"#;
        let items = items(json.as_bytes());
        let plan = plan_import(&items, &[]);
        let names: Vec<&str> = plan.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Bitwarden (test@bitwarden.com)",
                "Bitwarden (test@bitwarden.com) (code)",
                "Bitwarden (test@bitwarden.com) (notes)",
                "Bitwarden (test@bitwarden.com) (Text)",
                "Bitwarden (test@bitwarden.com) (Hidden)",
                "My secure note",
                "My card (card number)",
                "My card (security code)",
                "My card (expiry)",
                "My card (cardholder)",
                "My identity (full name)",
                "My identity (company)",
                "My identity (username)",
                "My ssh key (private key)",
                "My ssh key (public key)",
            ]
        );
    }

    #[test]
    fn the_bare_array_bw_list_items_prints_is_taken_too() {
        let items = items(br#"[{"type": 1, "name": "A", "login": {"password": "x"}}]"#);
        assert_eq!(names(&items), ["A"]);
    }

    #[test]
    fn a_byte_order_mark_does_not_hide_the_json() {
        let items = items("\u{feff}{\"encrypted\": false, \"items\": []}".as_bytes());
        assert!(items.is_empty());
    }

    /// The exact text Bitwarden's SDK expects its CSV exporter to write.
    #[test]
    fn bitwardens_own_csv_export_reads() {
        let csv = [
            "folder,favorite,type,name,notes,fields,reprompt,login_uri,login_username,login_password,login_totp",
            ",,login,test@bitwarden.com,,,0,https://google.com,test@bitwarden.com,Abc123,",
            "Test Folder B,1,login,Steam Account,,\"Test: v\nHidden: asdfer\",0,https://steampowered.com,steam,3Pvb8u7EfbV*nJ,steam://ABCD123",
            "",
        ]
        .join("\n");
        let items = items(csv.as_bytes());
        let plan = plan_import(&items, &[]);
        let got: Vec<(&str, &str)> = plan.entries.iter().map(|e| (e.name.as_str(), &**e.secret)).collect();
        assert_eq!(
            got,
            [
                ("test@bitwarden.com", "Abc123"),
                ("Steam Account (steam)", "3Pvb8u7EfbV*nJ"),
                // Stored the way Bitwarden reads it: the stray "1" dropped.
                ("Steam Account (steam) (code)", "steam://ABCD23"),
                ("Steam Account (steam) (Test)", "v"),
                ("Steam Account (steam) (Hidden)", "asdfer"),
            ]
        );
    }

    #[test]
    fn a_csv_note_with_quotes_commas_and_line_breaks_survives() {
        let csv = "collections,type,name,notes,fields,reprompt,login_uri,login_username,login_password,login_totp\r\n\
                   ,note,Wifi,\"line one, with a comma\r\nline \"\"two\"\"\",,0,,,,\r\n";
        let items = items(csv.as_bytes());
        let plan = plan_import(&items, &[]);
        assert_eq!(plan.entries.len(), 1);
        assert_eq!(plan.entries[0].name, "Wifi");
        assert_eq!(&*plan.entries[0].secret, "line one, with a comma\r\nline \"two\"");
    }
}
