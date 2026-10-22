//! Dates on the vault's files that say nothing about the entries inside them.
//!
//! The entries are sealed, but their file dates are not: left alone, the
//! modified and created dates of each `*.enc` say when that entry was added
//! or last changed, and with last-access updates on (the Windows default on
//! smaller drives) the accessed date says when it was last copied. Anyone
//! looking through the folder could pick out the newest or most used entries
//! without opening a single one.
//!
//! So every file the vault writes gets one random date, used as its modified,
//! accessed and (on Windows) created date, and every file it reads has its
//! accessed date put back afterwards. A date is never part of the file's
//! contents, so nothing sealed or hashed is touched.
//!
//! The dates fall between 1980 and 2005: before the vault could have existed,
//! so a real date always stands out from a scrambled one, and not before
//! 1980, the earliest date FAT and exFAT drives — a portable copy on a USB
//! stick — can hold.
//!
//! Every write draws a new date rather than reusing one, so a changed file
//! never keeps the size and date it had before: anything that skips a file
//! whose size and date look unchanged (robocopy, rsync, syncMe) still picks
//! it up. Tools that only copy a file when its date is *newer* (`xcopy /D`,
//! `rsync -u`) will miss changes, though.
//!
//! Dating is best effort: a file whose dates cannot be set is logged and left
//! as it is, never a reason to fail the save or the read that came before.

use std::fs::{File, FileTimes};
use std::io;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aes_gcm::aead::Generate;

/// 1980-01-02 UTC: a day after FAT's earliest date, so no time zone turns it
/// into 1979.
const EARLIEST: u64 = 315_619_200;

/// 2006-01-01 UTC. Anything from here on is a real date.
const LATEST: u64 = 1_136_073_600;

/// After writing `path`: give it a new random date.
pub fn stamp_written(path: &Path) {
    let result = open(path).and_then(|file| {
        scramble(&file)?;
        Ok(file.metadata()?.is_file())
    });
    match result {
        Ok(is_file) => hide_folder_use(path, is_file),
        Err(e) => log::warn!("could not scramble the dates of {}: {e}", path.display()),
    }
}

/// After reading `path`: put its accessed date back to its modified one, and
/// scramble it if it still carries a real date (written before the vault
/// scrambled dates, or copied in by something else).
pub fn stamp_read(path: &Path) {
    let result = open(path).and_then(|file| {
        let meta = file.metadata()?;
        let modified = meta.modified()?;
        if is_real(modified) || created_is_real(&meta) {
            scramble(&file)?;
        } else {
            file.set_times(FileTimes::new().set_accessed(modified))?;
        }
        Ok(meta.is_file())
    });
    match result {
        Ok(is_file) => hide_folder_use(path, is_file),
        Err(e) => log::warn!("could not hide the dates of {}: {e}", path.display()),
    }
}

/// On Windows, opening a file to change its dates is itself a use of the
/// folder it is in: the folder's accessed date moves to now. Put it back,
/// once the file's handle is closed.
fn hide_folder_use(path: &Path, is_file: bool) {
    if !cfg!(windows) || !is_file {
        return;
    }
    let Some(folder) = path.parent().filter(|p| !p.as_os_str().is_empty()) else { return };
    let result = open(folder).and_then(|handle| {
        let modified = handle.metadata()?.modified()?;
        handle.set_times(FileTimes::new().set_accessed(modified))
    });
    if let Err(e) = result {
        log::warn!("could not hide the dates of {}: {e}", folder.display());
    }
}

fn scramble(file: &File) -> io::Result<()> {
    let date = random_date();
    let times = FileTimes::new().set_modified(date).set_accessed(date);
    #[cfg(windows)]
    let times = {
        use std::os::windows::fs::FileTimesExt;
        times.set_created(date)
    };
    file.set_times(times)
}

fn random_date() -> SystemTime {
    let n = u64::from_le_bytes(Generate::generate());
    UNIX_EPOCH + Duration::from_secs(EARLIEST + n % (LATEST - EARLIEST))
}

fn is_real(date: SystemTime) -> bool {
    date >= UNIX_EPOCH + Duration::from_secs(LATEST)
}

/// Only Windows lets a created date be set, so only there is a real one
/// something to fix; elsewhere it would be "fixed" on every read, forever.
fn created_is_real(meta: &std::fs::Metadata) -> bool {
    cfg!(windows) && meta.created().is_ok_and(is_real)
}

/// A handle that can change the dates of `path`, file or folder, without
/// reading or writing a byte of it.
///
/// On Unix the owner may set a file's dates through any handle, so a plain
/// read-only open does, and works on folders too. Windows wants the right to
/// write attributes asked for, and backup semantics to open a folder at all.
fn open(path: &Path) -> io::Result<File> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_READ_ATTRIBUTES: u32 = 0x0080;
        const FILE_WRITE_ATTRIBUTES: u32 = 0x0100;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        std::fs::OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)
    }
    #[cfg(not(windows))]
    File::open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dates(path: &Path) -> (SystemTime, SystemTime) {
        let meta = std::fs::metadata(path).unwrap();
        (meta.modified().unwrap(), meta.accessed().unwrap())
    }

    #[test]
    fn a_written_file_gets_one_old_date_and_keeps_its_contents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.enc");
        std::fs::write(&path, b"sealed bytes").unwrap();

        stamp_written(&path);

        let (modified, accessed) = dates(&path);
        assert!(!is_real(modified));
        assert!(modified >= UNIX_EPOCH + Duration::from_secs(EARLIEST));
        assert_eq!(accessed, modified);
        #[cfg(windows)]
        assert_eq!(std::fs::metadata(&path).unwrap().created().unwrap(), modified);
        assert_eq!(std::fs::read(&path).unwrap(), b"sealed bytes");
    }

    #[test]
    fn every_write_draws_a_new_date() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.enc");
        std::fs::write(&path, b"x").unwrap();
        let drawn: std::collections::HashSet<SystemTime> = (0..5)
            .map(|_| {
                stamp_written(&path);
                dates(&path).0
            })
            .collect();
        assert!(drawn.len() > 1);
    }

    #[test]
    fn reading_leaves_no_access_date_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.enc");
        std::fs::write(&path, b"x").unwrap();
        stamp_written(&path);
        let before = dates(&path).0;

        std::fs::read(&path).unwrap();
        stamp_read(&path);

        assert_eq!(dates(&path), (before, before));
    }

    #[test]
    fn a_file_with_a_real_date_is_scrambled_when_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.enc");
        std::fs::write(&path, b"x").unwrap();
        assert!(is_real(dates(&path).0));

        stamp_read(&path);

        assert!(!is_real(dates(&path).0));
    }

    #[test]
    fn dating_a_file_leaves_no_access_date_on_its_folder() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.enc");
        std::fs::write(&path, b"x").unwrap();
        stamp_written(dir.path());
        let before = dates(dir.path());

        stamp_written(&path);
        std::fs::read(&path).unwrap();
        stamp_read(&path);

        assert_eq!(dates(dir.path()), before);
    }

    #[test]
    fn folders_can_be_dated_too() {
        let dir = tempfile::tempdir().unwrap();
        stamp_written(dir.path());
        assert!(!is_real(dates(dir.path()).0));
    }
}
