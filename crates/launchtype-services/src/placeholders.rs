//! Reading and writing `snippets/placeholders.json` — the placeholders the
//! user defines for themselves ([`launchtype_core::placeholders`]).
//!
//! It lives in the snippets folder rather than beside `commands.json` because
//! that is the folder the app already offers to open, and because a
//! placeholder is mostly written while writing a snippet. It works in commands
//! all the same.
//!
//! The set is cached, since it is consulted on every keystroke that expands a
//! command; [`reload`] drops the cache, and is what the app calls after the
//! user defines one.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use launchtype_core::placeholders::{Placeholders, FILE_NAME};

static CACHE: RwLock<Option<Arc<Placeholders>>> = RwLock::new(None);

/// Where the file lives, relative to the working directory — the same
/// `snippets/` folder [`crate::snippets`] writes into.
pub fn path() -> PathBuf {
    Path::new("snippets").join(FILE_NAME)
}

/// The user's placeholders, read once and reused.
pub fn current() -> Arc<Placeholders> {
    if let Some(cached) = CACHE.read().unwrap().clone() {
        return cached;
    }
    let loaded = Arc::new(read(&path()));
    *CACHE.write().unwrap() = Some(loaded.clone());
    loaded
}

/// Forget the cached set, so the next [`current`] reads the file again. Also
/// clears the placeholder half of [`crate::portable::vars`], which is built
/// from it.
pub fn reload() {
    *CACHE.write().unwrap() = None;
    crate::portable::forget_placeholders();
}

/// Read the file. A missing one is an empty set — most people never write one.
pub fn read(path: &Path) -> Placeholders {
    match std::fs::read_to_string(path) {
        Ok(text) => Placeholders::from_json(&text),
        Err(_) => Placeholders::default(),
    }
}

/// Write the file, creating the snippets folder if this is the first one.
pub fn write(placeholders: &Placeholders) -> std::io::Result<()> {
    let path = path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, placeholders.to_json())
}

/// Define one placeholder and save, so the next command or snippet that names
/// it finds it. Returns the whole set as it now stands.
pub fn define(name: &str, text: &str) -> std::io::Result<Arc<Placeholders>> {
    let mut placeholders = (*current()).clone();
    placeholders.set(name, text);
    write(&placeholders)?;
    reload();
    Ok(current())
}

/// Forget one placeholder and save. Returns whether there was one to forget.
pub fn forget(name: &str) -> std::io::Result<bool> {
    let mut placeholders = (*current()).clone();
    if !placeholders.remove(name) {
        return Ok(false);
    }
    write(&placeholders)?;
    reload();
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_or_unreadable_file_is_an_empty_set() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read(&dir.path().join("nothing.json")).is_empty());
        // A folder where a file should be: still empty, still no panic.
        assert!(read(dir.path()).is_empty());
    }

    #[test]
    fn what_is_written_is_what_comes_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        let mut placeholders = Placeholders::default();
        placeholders.set("hi", "Hola, ¿qué tal?");
        std::fs::write(&path, placeholders.to_json()).unwrap();
        assert_eq!(read(&path), placeholders);
    }

    /// The file the user opens by hand is the file the app reads.
    #[test]
    fn the_file_sits_in_the_snippets_folder() {
        assert_eq!(path(), Path::new("snippets").join("placeholders.json"));
    }
}
