//! Atomic JSON persistence, byte-compatible with the Python app's
//! `json.dumps` output (`helpers/json_storage.py`): same separators
//! (`", "` / `": "` compact, `","` / `": "` indented), same `ensure_ascii`
//! escaping, and the same temp-file + rename swap.

use std::io::{self, Write};
use std::path::Path;

use serde::Serialize;

/// Serialize `value` exactly like Python's `json.dumps(value, indent=indent)`.
pub fn to_python_json<T: Serialize>(value: &T, indent: Option<usize>) -> serde_json::Result<String> {
    let mut out = Vec::with_capacity(256);
    let mut ser = serde_json::Serializer::with_formatter(&mut out, PyFormatter::new(indent));
    value.serialize(&mut ser)?;
    // PyFormatter only ever writes valid UTF-8 (ASCII, in fact).
    Ok(String::from_utf8(out).expect("formatter emits UTF-8"))
}

/// Persist JSON by writing `<path>.tmp` and swapping it in, so a process
/// killed mid-write can never leave a truncated/corrupt file behind.
pub fn atomic_write_json<T: Serialize>(
    path: &Path,
    value: &T,
    indent: Option<usize>,
) -> io::Result<()> {
    let json = to_python_json(value, indent).map_err(io::Error::other)?;
    atomic_write(path, json.as_bytes())
}

/// The same temp-file + rename swap for content that is not JSON (the vault's
/// encrypted entry files).
///
/// # Why the file is created owner-only
///
/// Nearly everything written through here is either a secret or next door to
/// one: `settings.json` holds the SSH password and the Notebrook token,
/// `clipboard_history.json` holds the last fifty things the user copied, and
/// the Codex write-back rewrites `~/.codex/auth.json`, whose OAuth refresh
/// token is a standing key to the account.
///
/// A plain `File::create` takes its mode from the umask — 0644 on a normal
/// desktop — and because the swap *replaces* the destination, the new file's
/// mode is the one that sticks. Writing `~/.codex/auth.json` therefore used to
/// downgrade a file the Codex CLI had deliberately created as 0600, handing
/// every other account on the machine a working refresh token. So the temp
/// file is opened 0600 and the rename carries that over.
///
/// Windows has no umask: a new file inherits the ACL of the folder it is
/// created in, so the app folder's own permissions are what govern there.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = Path::new(&tmp);
    {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut f = options.open(tmp)?;
        f.write_all(bytes)?;
    }
    // An existing destination created before this (or by another program) keeps
    // its own mode through the rename, so tighten it explicitly too.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(tmp, std::fs::Permissions::from_mode(0o600));
    }
    // fs::rename replaces an existing destination on Windows (MOVEFILE_REPLACE_EXISTING),
    // matching Python's os.replace.
    std::fs::rename(tmp, path)
}

/// `create_dir_all` for a directory whose contents are nobody else's business
/// — the vault, and the scratch folder a transcription decodes into.
///
/// Only the leaf is tightened: the parents may be shared folders that have no
/// business being narrowed (`/tmp`, the app folder). On Windows the new
/// directory inherits the parent's ACL, as everything else does.
pub fn create_private_dir(path: &Path) -> io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    // Set at creation rather than afterwards, so there is no instant during
    // which the folder exists and is readable — which is the whole window a
    // scratch folder in a world-writable /tmp would otherwise have.
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    // `recursive` is a no-op on a folder that is already there, and one made
    // by an older version has the umask's mode; narrow that too.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// serde_json Formatter reproducing Python's json.dumps style.
struct PyFormatter {
    indent: Option<usize>,
    current_indent: usize,
    has_value: bool,
}

impl PyFormatter {
    fn new(indent: Option<usize>) -> Self {
        PyFormatter { indent, current_indent: 0, has_value: false }
    }

    fn write_newline_indent<W: ?Sized + Write>(&self, writer: &mut W) -> io::Result<()> {
        if let Some(n) = self.indent {
            writer.write_all(b"\n")?;
            for _ in 0..(n * self.current_indent) {
                writer.write_all(b" ")?;
            }
        }
        Ok(())
    }
}

impl serde_json::ser::Formatter for PyFormatter {
    fn begin_array<W: ?Sized + Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.current_indent += 1;
        self.has_value = false;
        writer.write_all(b"[")
    }

    fn end_array<W: ?Sized + Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.current_indent -= 1;
        if self.has_value {
            self.write_newline_indent(writer)?;
        }
        writer.write_all(b"]")
    }

    fn begin_array_value<W: ?Sized + Write>(&mut self, writer: &mut W, first: bool) -> io::Result<()> {
        match self.indent {
            None => {
                if !first {
                    writer.write_all(b", ")?;
                }
            }
            Some(_) => {
                if !first {
                    writer.write_all(b",")?;
                }
                self.write_newline_indent(writer)?;
            }
        }
        Ok(())
    }

    fn end_array_value<W: ?Sized + Write>(&mut self, _writer: &mut W) -> io::Result<()> {
        self.has_value = true;
        Ok(())
    }

    fn begin_object<W: ?Sized + Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.current_indent += 1;
        self.has_value = false;
        writer.write_all(b"{")
    }

    fn end_object<W: ?Sized + Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.current_indent -= 1;
        if self.has_value {
            self.write_newline_indent(writer)?;
        }
        writer.write_all(b"}")
    }

    fn begin_object_key<W: ?Sized + Write>(&mut self, writer: &mut W, first: bool) -> io::Result<()> {
        self.begin_array_value(writer, first)
    }

    fn begin_object_value<W: ?Sized + Write>(&mut self, writer: &mut W) -> io::Result<()> {
        writer.write_all(b": ")
    }

    fn end_object_value<W: ?Sized + Write>(&mut self, _writer: &mut W) -> io::Result<()> {
        self.has_value = true;
        Ok(())
    }

    /// Python's ensure_ascii=True: escape every non-ASCII char as \uXXXX
    /// (UTF-16 units, so astral chars become surrogate pairs).
    fn write_string_fragment<W: ?Sized + Write>(&mut self, writer: &mut W, fragment: &str) -> io::Result<()> {
        let mut buf = [0u16; 2];
        for c in fragment.chars() {
            if c.is_ascii() {
                writer.write_all(&[c as u8])?;
            } else {
                for unit in c.encode_utf16(&mut buf) {
                    write!(writer, "\\u{unit:04x}")?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn compact_matches_python_separators() {
        let value = json!({"a": 1, "b": [1, 2], "c": {"d": "x"}, "e": [], "f": {}});
        assert_eq!(
            to_python_json(&value, None).unwrap(),
            r#"{"a": 1, "b": [1, 2], "c": {"d": "x"}, "e": [], "f": {}}"#
        );
    }

    #[test]
    fn indent_2_matches_python_style() {
        let value = json!({"a": 1, "b": [1, 2], "e": [], "f": {}});
        let expected = "{\n  \"a\": 1,\n  \"b\": [\n    1,\n    2\n  ],\n  \"e\": [],\n  \"f\": {}\n}";
        assert_eq!(to_python_json(&value, Some(2)).unwrap(), expected);
    }

    #[test]
    fn non_ascii_escaped_like_ensure_ascii() {
        assert_eq!(to_python_json(&json!("café"), None).unwrap(), "\"caf\\u00e9\"");
        assert_eq!(to_python_json(&json!("año"), None).unwrap(), "\"a\\u00f1o\"");
        // Astral char -> surrogate pair, lowercase hex, like Python.
        assert_eq!(to_python_json(&json!("😀"), None).unwrap(), "\"\\ud83d\\ude00\"");
    }

    #[test]
    fn atomic_write_replaces_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.json");
        std::fs::write(&path, "old garbage").unwrap();
        atomic_write_json(&path, &json!({"k": "v"}), Some(2)).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text, "{\n  \"k\": \"v\"\n}");
        assert!(!path.with_extension("json.tmp").exists());
    }
}
