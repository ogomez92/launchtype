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
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = Path::new(&tmp);
    {
        let mut f = std::fs::File::create(tmp)?;
        f.write_all(json.as_bytes())?;
    }
    // fs::rename replaces an existing destination on Windows (MOVEFILE_REPLACE_EXISTING),
    // matching Python's os.replace.
    std::fs::rename(tmp, path)
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
