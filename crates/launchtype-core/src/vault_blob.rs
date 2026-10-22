//! The contents of a file or folder kept in the vault: `vault/<uuid>.blob`.
//!
//! An entry file has to stay small — every one of them is decrypted on unlock
//! just to read its name — so a document's bytes live next to it in a blob of
//! their own, sealed under a random key that exists nowhere but inside that
//! entry's sealed payload. See [`crate::vault`] for the entry side.
//!
//! # Format
//!
//! `LTB1`, a 7-byte random nonce prefix, then the contents cut into 64 KiB
//! chunks, each sealed with AES-256-GCM and followed by its tag. The nonce of
//! a chunk is the prefix, its big-endian counter and a flag byte that is 1 on
//! the last chunk only (the STREAM construction), and the entry's uuid is the
//! associated data of every chunk. So:
//!
//! - chunks cannot be reordered, dropped, or taken from another blob;
//! - cutting a blob short is caught, because whatever chunk ends up last was
//!   not sealed as the last one;
//! - memory stays at one chunk however big the file is.
//!
//! A folder is a zip of it, written straight into the cipher, so the zip never
//! exists in the clear anywhere; on the way out it is decrypted into memory
//! (it is capped at [`MAX_BLOB_BYTES`]) and unpacked from there.

use std::io::{self, Cursor, Read, Write};
use std::path::{Path, PathBuf};

use aes_gcm::aead::{AeadInOut, Generate, KeyInit, Nonce, Tag};
use aes_gcm::Aes256Gcm;
use zeroize::Zeroizing;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

/// First four bytes of every blob.
const MAGIC: [u8; 4] = *b"LTB1";
const PREFIX_LEN: usize = 7;
const TAG_LEN: usize = 16;
/// Plaintext bytes per chunk.
const CHUNK: usize = 64 * 1024;
/// One sealed chunk on disk: the ciphertext and its tag.
const FRAME: usize = CHUNK + TAG_LEN;

/// The most a blob may hold. A vault is a folder that gets copied and synced
/// around; this stops a stray disk image from landing in it by accident, and
/// it bounds the memory a folder takes on the way out.
pub const MAX_BLOB_BYTES: u64 = 200 * 1024 * 1024;

/// A blob that is not one, or has been altered. Raised as
/// [`io::ErrorKind::InvalidData`] so it travels through `io::copy` and the zip
/// crate; [`crate::vault`] turns it back into `VaultError::Damaged`.
fn damaged() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "damaged vault blob")
}

fn too_large() -> io::Error {
    io::Error::new(io::ErrorKind::FileTooLarge, "too large for the vault")
}

fn chunk_nonce(prefix: &[u8; PREFIX_LEN], counter: u32, last: bool) -> Nonce<Aes256Gcm> {
    let mut nonce = [0u8; 12];
    nonce[..PREFIX_LEN].copy_from_slice(prefix);
    nonce[PREFIX_LEN..PREFIX_LEN + 4].copy_from_slice(&counter.to_be_bytes());
    nonce[11] = u8::from(last);
    Nonce::<Aes256Gcm>::from(nonce)
}

/// Encrypts everything written to it into `inner`. Nothing is complete until
/// [`Self::finish`] seals the last chunk: a writer dropped without it leaves a
/// blob that will never open, which is the point.
pub struct BlobWriter<W: Write> {
    inner: W,
    cipher: Aes256Gcm,
    prefix: [u8; PREFIX_LEN],
    aad: Vec<u8>,
    counter: u32,
    /// Plaintext waiting to be sealed. Sized once and never grown, so it is
    /// never reallocated and no stray copy of it is left behind.
    chunk: Zeroizing<Vec<u8>>,
    written: u64,
}

impl<W: Write> BlobWriter<W> {
    pub fn new(mut inner: W, key: &[u8; 32], aad: &[u8]) -> io::Result<Self> {
        let prefix: [u8; PREFIX_LEN] = Generate::generate();
        inner.write_all(&MAGIC)?;
        inner.write_all(&prefix)?;
        Ok(BlobWriter {
            inner,
            cipher: Aes256Gcm::new_from_slice(key).expect("32 bytes is the AES-256 key size"),
            prefix,
            aad: aad.to_vec(),
            counter: 0,
            chunk: Zeroizing::new(Vec::with_capacity(CHUNK)),
            written: 0,
        })
    }

    /// Plaintext bytes written so far.
    pub fn bytes_written(&self) -> u64 {
        self.written
    }

    /// Seal the last chunk (empty, if the contents ended on a chunk boundary)
    /// and hand back the inner writer.
    pub fn finish(mut self) -> io::Result<W> {
        self.seal_chunk(true)?;
        self.inner.flush()?;
        Ok(self.inner)
    }

    fn seal_chunk(&mut self, last: bool) -> io::Result<()> {
        let nonce = chunk_nonce(&self.prefix, self.counter, last);
        // In place: the plaintext is overwritten by its own ciphertext.
        let tag = self
            .cipher
            .encrypt_inout_detached(&nonce, &self.aad, self.chunk.as_mut_slice().into())
            .map_err(|_| damaged())?;
        self.inner.write_all(&self.chunk)?;
        self.inner.write_all(&tag)?;
        self.chunk.clear();
        self.counter = self.counter.checked_add(1).ok_or_else(too_large)?;
        Ok(())
    }
}

impl<W: Write> Write for BlobWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.written + buf.len() as u64 > MAX_BLOB_BYTES {
            return Err(too_large());
        }
        let mut rest = buf;
        while !rest.is_empty() {
            // A full chunk is only sealed once more data turns up, because
            // until then it might be the last one.
            if self.chunk.len() == CHUNK {
                self.seal_chunk(false)?;
            }
            let take = (CHUNK - self.chunk.len()).min(rest.len());
            self.chunk.extend_from_slice(&rest[..take]);
            rest = &rest[take..];
        }
        self.written += buf.len() as u64;
        Ok(buf.len())
    }

    /// Does not seal anything: a partial chunk sealed here would have to be
    /// the last.
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Decrypts a blob as it is read. Every byte it hands out comes from a chunk
/// whose tag has already checked out, and it reports the end only after the
/// chunk sealed as the last one, so a truncated blob is an error rather than a
/// short file.
pub struct BlobReader<R: Read> {
    inner: R,
    cipher: Aes256Gcm,
    prefix: [u8; PREFIX_LEN],
    aad: Vec<u8>,
    counter: u32,
    /// The current frame, decrypted in place; `pos` is how much of it has
    /// been handed out. One byte longer than a frame, to look past its end.
    frame: Zeroizing<Vec<u8>>,
    pos: usize,
    /// The byte read past the end of the previous frame, proving it was not
    /// the last.
    carry: Option<u8>,
    done: bool,
}

impl<R: Read> BlobReader<R> {
    pub fn new(mut inner: R, key: &[u8; 32], aad: &[u8]) -> io::Result<Self> {
        let mut header = [0u8; MAGIC.len() + PREFIX_LEN];
        inner.read_exact(&mut header).map_err(|e| match e.kind() {
            io::ErrorKind::UnexpectedEof => damaged(),
            _ => e,
        })?;
        if header[..MAGIC.len()] != MAGIC {
            return Err(damaged());
        }
        let mut prefix = [0u8; PREFIX_LEN];
        prefix.copy_from_slice(&header[MAGIC.len()..]);
        Ok(BlobReader {
            inner,
            cipher: Aes256Gcm::new_from_slice(key).expect("32 bytes is the AES-256 key size"),
            prefix,
            aad: aad.to_vec(),
            counter: 0,
            frame: Zeroizing::new(Vec::with_capacity(FRAME + 1)),
            pos: 0,
            carry: None,
            done: false,
        })
    }

    fn next_frame(&mut self) -> io::Result<()> {
        self.frame.clear();
        self.pos = 0;
        if let Some(byte) = self.carry.take() {
            self.frame.push(byte);
        }
        let start = self.frame.len();
        self.frame.resize(FRAME + 1, 0);
        let got = read_full(&mut self.inner, &mut self.frame[start..])?;
        self.frame.truncate(start + got);

        let last = self.frame.len() <= FRAME;
        if !last {
            self.carry = self.frame.pop();
        }
        if self.frame.len() < TAG_LEN {
            return Err(damaged());
        }
        let split = self.frame.len() - TAG_LEN;
        let tag = Tag::<Aes256Gcm>::try_from(&self.frame[split..]).map_err(|_| damaged())?;
        let nonce = chunk_nonce(&self.prefix, self.counter, last);
        self.cipher
            .decrypt_inout_detached(&nonce, &self.aad, (&mut self.frame[..split]).into(), &tag)
            .map_err(|_| damaged())?;
        self.frame.truncate(split);
        self.counter = self.counter.checked_add(1).ok_or_else(damaged)?;
        self.done = last;
        Ok(())
    }
}

impl<R: Read> Read for BlobReader<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        loop {
            if self.pos < self.frame.len() {
                let n = (self.frame.len() - self.pos).min(out.len());
                out[..n].copy_from_slice(&self.frame[self.pos..self.pos + n]);
                self.pos += n;
                return Ok(n);
            }
            if self.done || out.is_empty() {
                return Ok(0);
            }
            self.next_frame()?;
        }
    }
}

/// `read` until `buf` is full or the reader runs dry.
fn read_full(reader: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

/// Decrypt a whole blob into memory, refusing anything bigger than `expected`
/// so the buffer is allocated once and never moved (and so never leaves a
/// copy of the plaintext behind in memory it no longer owns).
pub fn read_all<R: Read>(mut reader: BlobReader<R>, expected: u64) -> io::Result<Zeroizing<Vec<u8>>> {
    if expected > MAX_BLOB_BYTES {
        return Err(damaged());
    }
    let mut out = Zeroizing::new(Vec::with_capacity(expected as usize));
    let mut chunk = Zeroizing::new(vec![0u8; CHUNK]);
    loop {
        let n = reader.read(&mut chunk)?;
        if n == 0 {
            return Ok(out);
        }
        if (out.len() + n) as u64 > expected {
            return Err(damaged());
        }
        out.extend_from_slice(&chunk[..n]);
    }
}

/// The bytes of every regular file under `root`, which is what a folder is
/// checked against [`MAX_BLOB_BYTES`] by before any of it is read. Symbolic
/// links are skipped here as they are when zipping, and so is `skip` (the
/// vault folder, should the user pick a folder that contains it).
pub fn folder_size(root: &Path, skip: Option<&Path>) -> io::Result<u64> {
    let mut total = 0u64;
    walk(root, skip, &mut |_, kind| {
        if let Walked::File(len) = kind {
            total += len;
        }
        Ok(())
    })?;
    Ok(total)
}

enum Walked {
    Dir,
    File(u64),
}

/// Depth-first over `dir`, sorted by name so the same folder always zips the
/// same way. Links are not followed: a link pointing back up the tree would
/// otherwise never end, and one pointing out of it would put files the user
/// did not pick into the vault.
fn walk(
    dir: &Path,
    skip: Option<&Path>,
    visit: &mut dyn FnMut(&Path, Walked) -> io::Result<()>,
) -> io::Result<()> {
    let mut children: Vec<_> = std::fs::read_dir(dir)?.collect::<io::Result<_>>()?;
    children.sort_by_key(|entry| entry.file_name());
    for child in children {
        let path = child.path();
        let kind = child.file_type()?;
        if kind.is_symlink() {
            log::info!("not storing the link {}", path.display());
            continue;
        }
        if kind.is_dir() {
            if skip.is_some_and(|skip| same_dir(&path, skip)) {
                continue;
            }
            visit(&path, Walked::Dir)?;
            walk(&path, skip, visit)?;
        } else if kind.is_file() {
            visit(&path, Walked::File(child.metadata()?.len()))?;
        }
    }
    Ok(())
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Zip the contents of `root` (not `root` itself) into `out`.
pub fn zip_folder<W: Write>(root: &Path, skip: Option<&Path>, out: W) -> io::Result<()> {
    let mut zip = ZipWriter::new_stream(out);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    walk(root, skip, &mut |path, kind| {
        let name = archive_name(root, path)?;
        match kind {
            // Listed explicitly so empty folders come back out too.
            Walked::Dir => zip.add_directory(name, options).map_err(zip_error),
            Walked::File(_) => {
                zip.start_file(name, options).map_err(zip_error)?;
                io::copy(&mut std::fs::File::open(path)?, &mut zip)?;
                Ok(())
            }
        }
    })?;
    zip.finish().map_err(zip_error)?;
    Ok(())
}

/// `path` relative to `root`, with `/` between the parts as zip wants.
fn archive_name(root: &Path, path: &Path) -> io::Result<String> {
    let relative = path.strip_prefix(root).map_err(|_| damaged())?;
    let parts: Vec<String> =
        relative.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    Ok(parts.join("/"))
}

/// Unpack a zip made by [`zip_folder`] into `dest`, which must exist.
///
/// The archive was authenticated on its way out of the vault, so it is one
/// this app wrote. Its names are still only ever joined onto `dest` through
/// [`zip::read::ZipFile::enclosed_name`], which refuses anything absolute or
/// climbing out with `..`, because "we made it" is a promise about the past
/// and the cost of checking is nothing.
pub fn unzip_folder(zip: &[u8], dest: &Path) -> io::Result<()> {
    let mut archive = ZipArchive::new(Cursor::new(zip)).map_err(zip_error)?;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(zip_error)?;
        let Some(relative) = file.enclosed_name() else { return Err(damaged()) };
        if file.is_symlink() {
            continue;
        }
        let target: PathBuf = dest.join(relative);
        if file.is_dir() {
            std::fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        io::copy(&mut file, &mut std::fs::File::create(&target)?)?;
    }
    Ok(())
}

fn zip_error(error: zip::result::ZipError) -> io::Error {
    match error {
        zip::result::ZipError::Io(e) => e,
        _ => damaged(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; 32] = [7; 32];
    const AAD: &[u8] = b"5f1d0c55-0000-4000-8000-000000000000";

    fn seal(plain: &[u8]) -> Vec<u8> {
        let mut writer = BlobWriter::new(Vec::new(), &KEY, AAD).unwrap();
        writer.write_all(plain).unwrap();
        writer.finish().unwrap()
    }

    fn open(sealed: &[u8], key: &[u8; 32], aad: &[u8]) -> io::Result<Vec<u8>> {
        let mut out = Vec::new();
        BlobReader::new(sealed, key, aad)?.read_to_end(&mut out)?;
        Ok(out)
    }

    fn sample(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i * 31 % 251) as u8).collect()
    }

    /// The sizes around a chunk boundary are where a STREAM implementation
    /// goes wrong: an exact multiple must still end on a chunk marked last.
    #[test]
    fn every_size_round_trips_including_the_chunk_edges() {
        for len in [0, 1, CHUNK - 1, CHUNK, CHUNK + 1, 2 * CHUNK, 3 * CHUNK + 17] {
            let plain = sample(len);
            let sealed = seal(&plain);
            assert_eq!(open(&sealed, &KEY, AAD).unwrap(), plain, "length {len}");
        }
    }

    #[test]
    fn nothing_of_the_plaintext_is_visible() {
        let plain = b"%PDF-1.7 a very private manual".repeat(100);
        let sealed = seal(&plain);
        assert!(!sealed.windows(8).any(|w| w == b"%PDF-1.7"));
    }

    #[test]
    fn a_flipped_bit_anywhere_is_refused() {
        let sealed = seal(&sample(2 * CHUNK + 5));
        for at in [MAGIC.len() + 2, 20, CHUNK, sealed.len() - 1] {
            let mut bad = sealed.clone();
            bad[at] ^= 1;
            let err = open(&bad, &KEY, AAD).unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::InvalidData, "byte {at}");
        }
    }

    /// Cutting whole chunks off the end would go unnoticed if chunks were
    /// sealed independently. Here the new last chunk was never sealed as last.
    #[test]
    fn a_blob_cut_short_on_a_chunk_boundary_is_refused() {
        let sealed = seal(&sample(3 * CHUNK + 10));
        let header = MAGIC.len() + PREFIX_LEN;
        for frames in 1..=3 {
            let cut = &sealed[..header + frames * FRAME];
            assert_eq!(open(cut, &KEY, AAD).unwrap_err().kind(), io::ErrorKind::InvalidData);
        }
        assert_eq!(open(&sealed[..header], &KEY, AAD).unwrap_err().kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn chunks_cannot_be_swapped_around() {
        let sealed = seal(&sample(3 * CHUNK));
        let header = MAGIC.len() + PREFIX_LEN;
        let mut swapped = sealed[..header].to_vec();
        swapped.extend_from_slice(&sealed[header + FRAME..header + 2 * FRAME]);
        swapped.extend_from_slice(&sealed[header..header + FRAME]);
        swapped.extend_from_slice(&sealed[header + 2 * FRAME..]);
        assert_eq!(open(&swapped, &KEY, AAD).unwrap_err().kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn the_wrong_key_or_entry_id_does_not_open_it() {
        let sealed = seal(b"hello");
        assert!(open(&sealed, &[8; 32], AAD).is_err());
        assert!(open(&sealed, &KEY, b"another entry").is_err());
        assert!(open(b"LTB0 nope", &KEY, AAD).is_err());
        assert!(open(b"", &KEY, AAD).is_err());
    }

    #[test]
    fn read_all_refuses_more_than_it_was_told_to_expect() {
        let sealed = seal(&sample(1000));
        let reader = BlobReader::new(&sealed[..], &KEY, AAD).unwrap();
        assert_eq!(read_all(reader, 1000).unwrap().len(), 1000);
        let reader = BlobReader::new(&sealed[..], &KEY, AAD).unwrap();
        assert!(read_all(reader, 999).is_err());
    }

    #[test]
    fn a_folder_comes_back_out_with_its_layout_and_empty_folders() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("docs");
        std::fs::create_dir_all(src.join("manuals/empty")).unwrap();
        std::fs::write(src.join("readme.txt"), b"top").unwrap();
        std::fs::write(src.join("manuals/printer.pdf"), sample(CHUNK * 2 + 3)).unwrap();

        assert_eq!(folder_size(&src, None).unwrap(), 3 + (CHUNK * 2 + 3) as u64);

        let mut writer = BlobWriter::new(Vec::new(), &KEY, AAD).unwrap();
        zip_folder(&src, None, &mut writer).unwrap();
        let sealed = writer.finish().unwrap();
        let reader = BlobReader::new(&sealed[..], &KEY, AAD).unwrap();
        let zip = read_all(reader, sealed.len() as u64).unwrap();

        let out = dir.path().join("out");
        std::fs::create_dir(&out).unwrap();
        unzip_folder(&zip, &out).unwrap();
        assert_eq!(std::fs::read(out.join("readme.txt")).unwrap(), b"top");
        assert_eq!(std::fs::read(out.join("manuals/printer.pdf")).unwrap(), sample(CHUNK * 2 + 3));
        assert!(out.join("manuals/empty").is_dir());
    }

    #[test]
    fn a_skipped_folder_is_left_out_of_the_zip() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("app");
        std::fs::create_dir_all(src.join("vault")).unwrap();
        std::fs::write(src.join("vault/secret.enc"), b"x").unwrap();
        std::fs::write(src.join("notes.txt"), b"y").unwrap();
        let vault = src.join("vault");
        assert_eq!(folder_size(&src, Some(&vault)).unwrap(), 1);
    }
}
