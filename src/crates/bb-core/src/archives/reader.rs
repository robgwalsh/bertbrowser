//! Reading a container: its entries, and the bytes of one entry. Spec: Core/Services/Archives/
//! ArchiveReader.cs (`IArchiveReader`, `SharpCompressArchiveReader`).
//!
//! The trait exists so the rules above it are testable without a real archive, and so the
//! compression crates are confined to one implementation: a crate reachable from one file is a
//! crate that can be replaced. **What the bytes turn out to be must match what the name claimed.**
//! 512,000 zero bytes named `archive.zip` is a valid empty tar, so the suffix picks the decoder and
//! a decoder that will not parse is "damaged", never a browse of something else. **Nothing is held
//! open**: every read opens with `ReadWrite | Delete` sharing, takes what it needs and closes,
//! because what a held handle would block is this app's own rename, move and delete. **The catch
//! is deliberately wide**: a malformed archive is attacker-controlled input handed to third-party
//! decoders, and whatever they fail with is a message, never a panic. Synchronous on purpose, like
//! the file-system service; callers own their worker thread.

use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::os::windows::fs::OpenOptionsExt;

use bb_paths::{file_name, fold_upper};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
};

use super::{
    ArchiveCapabilities, ArchiveContainer, ArchiveFailure, ArchiveFormat, ArchiveFormats,
    ArchiveIndex, ArchiveIndexBuilder, RawArchiveEntry,
};
use crate::FileTime;
use crate::cancel::{CancelToken, Cancelled};

/// Why `read_entries` stopped short.
#[derive(Debug)]
pub enum ArchiveReadError {
    Cancelled,
    /// The container itself failed part-way: a truncated stream, a decoder giving up.
    Failed(String),
}

impl std::fmt::Display for ArchiveReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArchiveReadError::Cancelled => f.write_str("cancelled"),
            ArchiveReadError::Failed(m) => f.write_str(m),
        }
    }
}

/// Receives one wanted entry: its normalised key, its bytes, and its uncompressed size. Returning
/// `Err(Cancelled)` stops the pass.
pub type EntrySink<'a> = &'a mut dyn FnMut(&str, &mut dyn Read, i64) -> Result<(), Cancelled>;

pub trait ArchiveReader: Send + Sync {
    /// Reads a container's directory. **Never fails**: a damaged, encrypted, oversized, unreadable
    /// or cancelled read comes back as an `ArchiveIndex` carrying an `ArchiveFailure` and a message.
    fn read(
        &self,
        archive_file: &str,
        password: Option<&str>,
        cancel: &CancelToken,
    ) -> ArchiveIndex;

    /// Copies up to `max_bytes` of one entry into memory. None when the entry is missing or
    /// unreadable. **Bounding the read bounds the decompression**, which is what a zip bomb cannot
    /// get around: pulling 1 MB out of a stream that would have produced 10 GB costs 1 MB.
    fn read_entry(
        &self,
        archive_file: &str,
        entry_path: &str,
        max_bytes: i64,
        password: Option<&str>,
        cancel: &CancelToken,
    ) -> Option<Vec<u8>>;

    /// Hands each wanted entry's content to `on_entry`, in one pass over the container. **One
    /// pass, not one per entry**: pulling three files out of a solid 7z entry by entry means
    /// decompressing from the start three times. This owns iterating the archive and the caller
    /// owns the filesystem. Entry paths are normalised keys as the index reports them; anything
    /// not found is simply never announced.
    fn read_entries(
        &self,
        archive_file: &str,
        entry_paths: &[String],
        password: Option<&str>,
        on_entry: EntrySink<'_>,
        cancel: &CancelToken,
    ) -> Result<(), ArchiveReadError>;
}

/// What a backend ran into, before it is worded for the user.
enum Stop {
    /// A password is needed, or the one given did not decrypt the headers.
    PasswordRequired,
    /// The bytes would not parse as the format the name claimed.
    Malformed(String),
    NotFound,
    Unreadable,
    Cancelled,
}

impl From<Cancelled> for Stop {
    fn from(_: Cancelled) -> Stop {
        Stop::Cancelled
    }
}

impl From<io::Error> for Stop {
    fn from(e: io::Error) -> Stop {
        match e.kind() {
            io::ErrorKind::NotFound => Stop::NotFound,
            io::ErrorKind::PermissionDenied => Stop::Unreadable,
            _ => Stop::Malformed(e.to_string()),
        }
    }
}

const DAMAGED: &str = "Not a readable archive, or damaged.";

/// The only module in this crate that names the compression crates. The C# name was
/// `SharpCompressArchiveReader`.
pub struct LibraryArchiveReader {
    max_entries: usize,
}

impl Default for LibraryArchiveReader {
    fn default() -> Self {
        LibraryArchiveReader::new()
    }
}

impl LibraryArchiveReader {
    pub fn new() -> LibraryArchiveReader {
        LibraryArchiveReader::with_cap(ArchiveIndex::MAX_ENTRIES)
    }

    pub fn with_cap(max_entries: usize) -> LibraryArchiveReader {
        LibraryArchiveReader { max_entries }
    }

    fn list(
        &self,
        archive_file: &str,
        format: &ArchiveFormat,
        password: Option<&str>,
        cancel: &CancelToken,
    ) -> Result<ArchiveIndex, Stop> {
        let (raw, capabilities) = match format.container {
            ArchiveContainer::Zip => zip_list(archive_file, password, self.max_entries, cancel)?,
            ArchiveContainer::SevenZip => {
                sevenz_list(archive_file, password, self.max_entries, cancel)?
            }
            ArchiveContainer::Rar => rar_list(archive_file, password, self.max_entries, cancel)?,
            ArchiveContainer::Tar => tar_list(archive_file, format, self.max_entries, cancel)?,
            ArchiveContainer::GZip => gz_list(archive_file)?,
        };
        Ok(ArchiveIndexBuilder::build(
            raw,
            capabilities,
            self.max_entries,
        ))
    }
}

impl ArchiveReader for LibraryArchiveReader {
    fn read(
        &self,
        archive_file: &str,
        password: Option<&str>,
        cancel: &CancelToken,
    ) -> ArchiveIndex {
        let Some(format) = ArchiveFormats::match_name(file_name(archive_file)) else {
            return ArchiveIndex::failed(ArchiveFailure::Damaged, DAMAGED);
        };

        match self.list(archive_file, format, password, cancel) {
            Ok(index) => index,
            Err(Stop::PasswordRequired) => ArchiveIndex::failed(
                ArchiveFailure::PasswordRequired,
                if password.is_none() {
                    "This archive is protected. A password is needed to read it."
                } else {
                    "That password did not work."
                },
            ),
            Err(Stop::NotFound) => ArchiveIndex::failed(
                ArchiveFailure::Unreadable,
                "The archive is no longer there.",
            ),
            Err(Stop::Unreadable) => ArchiveIndex::failed(
                ArchiveFailure::Unreadable,
                "The archive could not be opened.",
            ),
            Err(Stop::Cancelled) => {
                ArchiveIndex::failed(ArchiveFailure::Unreadable, "The read was cancelled.")
            }
            // A password was given and the container still would not parse. Decrypting a header
            // with the wrong key produces garbage rather than a crypto error, so it arrives here,
            // and reporting it as "damaged" would dead-end the one user who can fix it. With a key
            // in hand, a wrong key is overwhelmingly the likelier reading; without one, damaged is
            // right.
            Err(Stop::Malformed(_)) if password.is_some() => ArchiveIndex::failed(
                ArchiveFailure::PasswordRequired,
                "That password did not work.",
            ),
            Err(Stop::Malformed(_)) => ArchiveIndex::failed(ArchiveFailure::Damaged, DAMAGED),
        }
    }

    fn read_entry(
        &self,
        archive_file: &str,
        entry_path: &str,
        max_bytes: i64,
        password: Option<&str>,
        cancel: &CancelToken,
    ) -> Option<Vec<u8>> {
        if max_bytes <= 0 {
            return None;
        }
        let format = ArchiveFormats::match_name(file_name(archive_file))?;
        let wanted = fold_upper(entry_path);
        let mut out: Option<Vec<u8>> = None;
        let mut sink = |key: &str, content: &mut dyn Read, _size: i64| -> Result<(), Cancelled> {
            if fold_upper(key) == wanted {
                out = Some(copy_bounded(content, max_bytes, cancel)?);
                // One entry was asked for; stopping here is what bounds a solid pass.
                return Err(Cancelled);
            }
            Ok(())
        };
        let _ = walk(
            archive_file,
            format,
            &[entry_path.to_string()],
            password,
            &mut sink,
            cancel,
        );
        out
    }

    fn read_entries(
        &self,
        archive_file: &str,
        entry_paths: &[String],
        password: Option<&str>,
        on_entry: EntrySink<'_>,
        cancel: &CancelToken,
    ) -> Result<(), ArchiveReadError> {
        if entry_paths.is_empty() {
            return Ok(());
        }
        let format = ArchiveFormats::match_name(file_name(archive_file))
            .ok_or_else(|| ArchiveReadError::Failed(DAMAGED.to_string()))?;
        match walk(
            archive_file,
            format,
            entry_paths,
            password,
            on_entry,
            cancel,
        ) {
            Ok(()) => Ok(()),
            Err(Stop::Cancelled) => Err(ArchiveReadError::Cancelled),
            Err(Stop::PasswordRequired) => Err(ArchiveReadError::Failed(
                "This archive is protected. A password is needed to read it.".to_string(),
            )),
            Err(Stop::NotFound) => Err(ArchiveReadError::Failed(
                "The archive is no longer there.".to_string(),
            )),
            Err(Stop::Unreadable) => Err(ArchiveReadError::Failed(
                "The archive could not be opened.".to_string(),
            )),
            Err(Stop::Malformed(m)) => Err(ArchiveReadError::Failed(m)),
        }
    }
}

/// One pass over the container, announcing each wanted file entry once. Stops when every wanted
/// entry has been seen.
fn walk(
    archive_file: &str,
    format: &ArchiveFormat,
    entry_paths: &[String],
    password: Option<&str>,
    on_entry: EntrySink<'_>,
    cancel: &CancelToken,
) -> Result<(), Stop> {
    let wanted: HashSet<String> = entry_paths.iter().map(|p| fold_upper(p)).collect();
    let mut pass = Pass {
        wanted,
        seen: 0,
        on_entry,
    };
    match format.container {
        ArchiveContainer::Zip => zip_walk(archive_file, password, &mut pass, cancel),
        ArchiveContainer::SevenZip => sevenz_walk(archive_file, password, &mut pass, cancel),
        ArchiveContainer::Rar => rar_walk(archive_file, password, &mut pass, cancel),
        ArchiveContainer::Tar => tar_walk(archive_file, format, &mut pass, cancel),
        ArchiveContainer::GZip => gz_walk(archive_file, &mut pass, cancel),
    }
}

struct Pass<'a> {
    wanted: HashSet<String>,
    seen: usize,
    on_entry: EntrySink<'a>,
}

impl Pass<'_> {
    /// The normalised key when `raw_key` names a wanted file, so the backend opens only those.
    fn wants(&self, raw_key: Option<&str>) -> Option<String> {
        let key = ArchiveIndexBuilder::normalize(raw_key)?;
        self.wanted.contains(&fold_upper(&key)).then_some(key)
    }

    /// Announces one entry. Ok(false) once every wanted entry has been seen.
    fn announce(&mut self, key: &str, content: &mut dyn Read, size: i64) -> Result<bool, Stop> {
        (self.on_entry)(key, content, size)?;
        self.seen += 1;
        Ok(self.seen < self.wanted.len())
    }
}

/// The sharing flags are the rule, not a detail: this app's own executors are what a held handle
/// would block.
fn open_shared(path: &str) -> Result<BufReader<File>, Stop> {
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(path)?;
    Ok(BufReader::with_capacity(64 * 1024, file))
}

fn copy_bounded(
    source: &mut dyn Read,
    max_bytes: i64,
    cancel: &CancelToken,
) -> Result<Vec<u8>, Cancelled> {
    let mut buffer = vec![0u8; 81_920];
    let mut sink = Vec::new();
    let mut total = 0i64;
    while total < max_bytes {
        cancel.check()?;
        let want = buffer.len().min((max_bytes - total) as usize);
        let read = match source.read(&mut buffer[..want]) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        sink.extend_from_slice(&buffer[..read]);
        total += read as i64;
    }
    Ok(sink)
}

fn unix_seconds(secs: i64) -> FileTime {
    FileTime::from_unix_millis(secs.saturating_mul(1000))
}

// ---------------------------------------------------------------- zip ----------------------------

fn zip_time(dt: Option<zip::DateTime>) -> Option<FileTime> {
    // A DOS timestamp is local wall-clock time.
    let dt = dt?;
    Some(
        FileTime::from_ymd_hms(
            i64::from(dt.year()),
            u32::from(dt.month()),
            u32::from(dt.day()),
            u32::from(dt.hour()),
            u32::from(dt.minute()),
            u32::from(dt.second()),
        )
        .local_to_utc(),
    )
}

fn zip_stop(e: zip::result::ZipError) -> Stop {
    match e {
        zip::result::ZipError::Io(io) => io.into(),
        zip::result::ZipError::InvalidPassword => Stop::PasswordRequired,
        other => Stop::Malformed(other.to_string()),
    }
}

fn zip_open(archive_file: &str) -> Result<zip::ZipArchive<BufReader<File>>, Stop> {
    zip::ZipArchive::new(open_shared(archive_file)?).map_err(zip_stop)
}

fn zip_entry<'a>(
    archive: &'a mut zip::ZipArchive<BufReader<File>>,
    index: usize,
    password: Option<&str>,
) -> Result<zip::read::ZipFile<'a, BufReader<File>>, Stop> {
    match password {
        Some(pw) => archive.by_index_decrypt(index, pw.as_bytes()),
        None => archive.by_index(index),
    }
    .map_err(zip_stop)
}

fn zip_list(
    archive_file: &str,
    password: Option<&str>,
    max_entries: usize,
    cancel: &CancelToken,
) -> Result<(Vec<RawArchiveEntry>, ArchiveCapabilities), Stop> {
    let mut archive = zip_open(archive_file)?;
    let mut raw = Vec::new();
    let mut any_encrypted = false;

    // Encryption is reported per entry, not on the archive, and deriving it from the entries is
    // the better source anyway: a zip may hold two encrypted files among a hundred plain ones, and
    // that is exactly the case where the listing is shown in full with a lock on the rows that
    // need a password.
    for i in 0..archive.len() {
        cancel.check()?;
        let (key, size, compressed, modified, is_dir, encrypted, is_symlink) = {
            let entry = archive.by_index_data(i).map_err(zip_stop)?;
            (
                entry.name().ok().map(|n| n.into_owned()),
                entry.size() as i64,
                entry.compressed_size() as i64,
                zip_time(entry.last_modified()),
                entry.is_dir(),
                entry.encrypted(),
                entry.is_symlink(),
            )
        };
        any_encrypted |= encrypted;
        // A symlink's target is its content. Listed, never followed.
        let link_target = if is_symlink && !encrypted {
            zip_entry(&mut archive, i, password)
                .ok()
                .and_then(|mut f| copy_bounded(&mut f, 4096, cancel).ok())
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        } else {
            None
        };
        raw.push(RawArchiveEntry {
            key,
            size,
            compressed_size: compressed,
            modified,
            is_directory: is_dir,
            is_encrypted: encrypted,
            link_target,
        });
        if raw.len() > max_entries {
            break;
        }
    }

    Ok((raw, ArchiveCapabilities::new(false, any_encrypted, true)))
}

fn zip_walk(
    archive_file: &str,
    password: Option<&str>,
    pass: &mut Pass<'_>,
    cancel: &CancelToken,
) -> Result<(), Stop> {
    let mut archive = zip_open(archive_file)?;
    for i in 0..archive.len() {
        cancel.check()?;
        let (key, size) = {
            let entry = archive.by_index_data(i).map_err(zip_stop)?;
            if entry.is_dir() {
                continue;
            }
            let name = entry.name().ok().map(|n| n.into_owned());
            (pass.wants(name.as_deref()), entry.size() as i64)
        };
        let Some(key) = key else {
            continue;
        };
        let mut content = zip_entry(&mut archive, i, password)?;
        if !pass.announce(&key, &mut content, size)? {
            return Ok(());
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- 7z -----------------------------

fn sevenz_password(password: Option<&str>) -> sevenz_rust2::Password {
    match password {
        Some(pw) => sevenz_rust2::Password::new(pw),
        None => sevenz_rust2::Password::empty(),
    }
}

fn sevenz_stop(e: sevenz_rust2::Error) -> Stop {
    match e {
        sevenz_rust2::Error::PasswordRequired => Stop::PasswordRequired,
        sevenz_rust2::Error::MaybeBadPassword(_) => Stop::PasswordRequired,
        sevenz_rust2::Error::Io(io, _) => io.into(),
        sevenz_rust2::Error::FileOpen(io, _) => io.into(),
        other => Stop::Malformed(other.to_string()),
    }
}

fn sevenz_open(
    archive_file: &str,
    password: Option<&str>,
) -> Result<sevenz_rust2::ArchiveReader<BufReader<File>>, Stop> {
    sevenz_rust2::ArchiveReader::new(open_shared(archive_file)?, sevenz_password(password))
        .map_err(sevenz_stop)
}

fn sevenz_list(
    archive_file: &str,
    password: Option<&str>,
    max_entries: usize,
    cancel: &CancelToken,
) -> Result<(Vec<RawArchiveEntry>, ArchiveCapabilities), Stop> {
    let reader = sevenz_open(archive_file, password)?;
    let archive = reader.archive();
    let mut raw = Vec::new();
    let mut any_encrypted = false;

    for (i, entry) in archive.files.iter().enumerate() {
        cancel.check()?;
        // An entry's data is encrypted when the block it lives in runs through an AES coder.
        let encrypted = archive
            .stream_map
            .file_block_index
            .get(i)
            .copied()
            .flatten()
            .and_then(|b| archive.blocks.get(b))
            .is_some_and(|block| {
                block
                    .coders
                    .iter()
                    .any(|c| c.encoder_method_id() == sevenz_rust2::EncoderMethod::ID_AES256_SHA256)
            });
        any_encrypted |= encrypted;
        raw.push(RawArchiveEntry {
            key: Some(entry.name().to_string()),
            size: entry.size() as i64,
            compressed_size: entry.compressed_size as i64,
            modified: entry
                .has_last_modified_date
                .then(|| FileTime(u64::from(entry.last_modified_date()) as i64)),
            is_directory: entry.is_directory(),
            is_encrypted: encrypted,
            link_target: None,
        });
        if raw.len() > max_entries {
            break;
        }
    }

    Ok((
        raw,
        ArchiveCapabilities::new(archive.is_solid, any_encrypted, true),
    ))
}

fn sevenz_walk(
    archive_file: &str,
    password: Option<&str>,
    pass: &mut Pass<'_>,
    cancel: &CancelToken,
) -> Result<(), Stop> {
    let mut reader = sevenz_open(archive_file, password)?;
    let mut stop: Option<Stop> = None;
    let result = reader.for_each_entries(|entry, content| {
        if cancel.is_cancelled() {
            stop = Some(Stop::Cancelled);
            return Ok(false);
        }
        // A solid block is one stream: an entry nobody reads to its end leaves the next entry's
        // reader standing in the middle of this one, so whatever the caller left is drained.
        let key = if entry.is_directory() {
            None
        } else {
            pass.wants(Some(entry.name()))
        };
        let Some(key) = key else {
            let _ = io::copy(content, &mut io::sink());
            return Ok(true);
        };
        let announced = pass.announce(&key, content, entry.size() as i64);
        let _ = io::copy(content, &mut io::sink());
        match announced {
            Ok(more) => Ok(more),
            Err(s) => {
                stop = Some(s);
                Ok(false)
            }
        }
    });
    if let Some(stop) = stop {
        return Err(stop);
    }
    result.map_err(sevenz_stop)
}

// ---------------------------------------------------------------- rar ----------------------------
//
// RAR is read through the `unrar` crate, which builds the UnRAR source bundled in `unrar_sys`.
// That source is freeware whose licence asks for this paragraph in the package's source: "UnRAR
// source code may be used in any software to handle RAR archives without limitations free of
// charge, but cannot be used to develop RAR (WinRAR) compatible archiver and to re-create RAR
// compression algorithm, which is proprietary. Distribution of modified UnRAR source code in
// separate form or as a part of other software is permitted, provided that full text of this
// paragraph, starting from "UnRAR source code" words, is included in license, or in documentation
// if license is not available, and in source code comments of resulting package." The full text
// is in THIRD-PARTY-NOTICES.md.

// The bundled source calls CryptGenRandom, which `unrar_sys` does not link for.
#[link(name = "advapi32")]
unsafe extern "C" {}

fn rar_stop(e: unrar::error::UnrarError, archive_file: &str) -> Stop {
    use unrar::error::Code;
    match e.code {
        Code::MissingPassword | Code::BadPassword => Stop::PasswordRequired,
        Code::EOpen | Code::ERead => {
            if std::path::Path::new(archive_file).exists() {
                Stop::Unreadable
            } else {
                Stop::NotFound
            }
        }
        _ => Stop::Malformed(e.to_string()),
    }
}

fn rar_archive<'a>(archive_file: &'a str, password: Option<&'a str>) -> unrar::Archive<'a> {
    match password {
        Some(pw) => unrar::Archive::with_password(archive_file, pw),
        None => unrar::Archive::new(archive_file),
    }
}

/// A DOS date-time packed the way the RAR headers carry it: date in the high word, time in the
/// low word, local wall-clock.
fn dos_time(packed: u32) -> Option<FileTime> {
    let date = (packed >> 16) as u16;
    let time = (packed & 0xFFFF) as u16;
    let year = 1980 + i64::from(date >> 9);
    let month = u32::from((date >> 5) & 0xF);
    let day = u32::from(date & 0x1F);
    let hour = u32::from(time >> 11);
    let minute = u32::from((time >> 5) & 0x3F);
    let second = u32::from(time & 0x1F) * 2;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(FileTime::from_ymd_hms(year, month, day, hour, minute, second).local_to_utc())
}

fn rar_list(
    archive_file: &str,
    password: Option<&str>,
    max_entries: usize,
    cancel: &CancelToken,
) -> Result<(Vec<RawArchiveEntry>, ArchiveCapabilities), Stop> {
    let open = rar_archive(archive_file, password)
        .open_for_listing()
        .map_err(|e| rar_stop(e, archive_file))?;
    let solid = open.is_solid();
    let mut raw = Vec::new();
    let mut any_encrypted = false;

    for header in open {
        cancel.check()?;
        let header = header.map_err(|e| rar_stop(e, archive_file))?;
        any_encrypted |= header.is_encrypted();
        raw.push(RawArchiveEntry {
            key: Some(header.filename.to_string_lossy().into_owned()),
            size: header.unpacked_size as i64,
            compressed_size: 0,
            modified: dos_time(header.file_time),
            is_directory: header.is_directory(),
            is_encrypted: header.is_encrypted(),
            link_target: None,
        });
        if raw.len() > max_entries {
            break;
        }
    }

    Ok((raw, ArchiveCapabilities::new(solid, any_encrypted, true)))
}

fn rar_walk(
    archive_file: &str,
    password: Option<&str>,
    pass: &mut Pass<'_>,
    cancel: &CancelToken,
) -> Result<(), Stop> {
    let mut open = rar_archive(archive_file, password)
        .open_for_processing()
        .map_err(|e| rar_stop(e, archive_file))?;
    loop {
        cancel.check()?;
        let Some(cursor) = open.read_header().map_err(|e| rar_stop(e, archive_file))? else {
            return Ok(());
        };
        let key = if cursor.entry().is_directory() {
            None
        } else {
            pass.wants(Some(&cursor.entry().filename.to_string_lossy()))
        };
        let size = cursor.entry().unpacked_size as i64;
        match key {
            Some(key) => {
                // The library can only hand back a whole entry, so the bound lands on the caller's
                // side of the copy rather than on the decompression.
                let (bytes, next) = cursor.read().map_err(|e| rar_stop(e, archive_file))?;
                open = next;
                let mut content = io::Cursor::new(bytes);
                if !pass.announce(&key, &mut content, size)? {
                    return Ok(());
                }
            }
            None => {
                open = cursor.skip().map_err(|e| rar_stop(e, archive_file))?;
            }
        }
    }
}

// ---------------------------------------------------------------- tar ----------------------------

/// The decompressor a tar family suffix needs over the raw file. `.tar.lz` is liblzma's lzip
/// decoder, `.tar.zst` is `ruzstd` (pure Rust, decode only).
fn tar_stream(archive_file: &str, format: &ArchiveFormat) -> Result<Box<dyn Read>, Stop> {
    let file = open_shared(archive_file)?;
    Ok(match format.suffix {
        ".tar" => Box::new(file),
        ".tar.gz" | ".tgz" => Box::new(flate2::read::MultiGzDecoder::new(file)),
        ".tar.bz2" | ".tbz2" => Box::new(bzip2::read::MultiBzDecoder::new(file)),
        ".tar.xz" | ".txz" => Box::new(liblzma::read::XzDecoder::new_multi_decoder(file)),
        ".tar.lz" => {
            let stream =
                liblzma::stream::Stream::new_lzip_decoder(u64::MAX, liblzma::stream::CONCATENATED)
                    .map_err(|e| Stop::Malformed(e.to_string()))?;
            Box::new(liblzma::read::XzDecoder::new_stream(file, stream))
        }
        ".tar.zst" => Box::new(ZstdFrames::new(BufReader::new(file))?),
        other => return Err(Stop::Malformed(format!("{other} is not decoded here"))),
    })
}

/// Every zstd frame of a file in turn, as `zstd -d` reads one (`pzstd` writes a frame per chunk,
/// and files are concatenated): `ruzstd`'s decoder stops at the end of one.
struct ZstdFrames<R: io::BufRead> {
    frame: Option<ruzstd::decoding::StreamingDecoder<R, ruzstd::decoding::FrameDecoder>>,
}

impl<R: io::BufRead> ZstdFrames<R> {
    fn new(source: R) -> Result<Self, Stop> {
        let frame = ruzstd::decoding::StreamingDecoder::new(source)
            .map_err(|e| Stop::Malformed(e.to_string()))?;
        Ok(ZstdFrames { frame: Some(frame) })
    }
}

impl<R: io::BufRead> Read for ZstdFrames<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            let Some(frame) = self.frame.as_mut() else {
                return Ok(0);
            };
            let n = frame.read(buf)?;
            if n > 0 || buf.is_empty() {
                return Ok(n);
            }
            // That frame is done: the next one starts where it ended, unless the file did.
            let (mut source, decoder) = self.frame.take().expect("just read from it").into_parts();
            if source.fill_buf()?.is_empty() {
                return Ok(0);
            }
            self.frame = Some(
                ruzstd::decoding::StreamingDecoder::new_with_decoder(source, decoder)
                    .map_err(io::Error::other)?,
            );
        }
    }
}

fn tar_list(
    archive_file: &str,
    format: &ArchiveFormat,
    max_entries: usize,
    cancel: &CancelToken,
) -> Result<(Vec<RawArchiveEntry>, ArchiveCapabilities), Stop> {
    let mut archive = tar::Archive::new(tar_stream(archive_file, format)?);
    let mut raw = Vec::new();

    for entry in archive.entries()? {
        cancel.check()?;
        let entry = entry?;
        let header = entry.header();
        let kind = header.entry_type();
        let link_target = if kind.is_symlink() {
            entry
                .link_name()
                .ok()
                .flatten()
                .map(|p| p.to_string_lossy().into_owned())
        } else {
            None
        };
        raw.push(RawArchiveEntry {
            key: entry.path().ok().map(|p| p.to_string_lossy().into_owned()),
            size: entry.size() as i64,
            compressed_size: 0,
            modified: header.mtime().ok().map(|m| unix_seconds(m as i64)),
            is_directory: kind.is_dir(),
            is_encrypted: false,
            link_target,
        });
        if raw.len() > max_entries {
            break;
        }
    }

    // A plain tar is addressable by walking its headers; a compressed one decodes end to end.
    Ok((
        raw,
        ArchiveCapabilities::new(!format.random_access, false, true),
    ))
}

fn tar_walk(
    archive_file: &str,
    format: &ArchiveFormat,
    pass: &mut Pass<'_>,
    cancel: &CancelToken,
) -> Result<(), Stop> {
    let mut archive = tar::Archive::new(tar_stream(archive_file, format)?);
    for entry in archive.entries()? {
        cancel.check()?;
        let mut entry = entry?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let name = entry.path().ok().map(|p| p.to_string_lossy().into_owned());
        let Some(key) = pass.wants(name.as_deref()) else {
            continue;
        };
        let size = entry.size() as i64;
        if !pass.announce(&key, &mut entry, size)? {
            return Ok(());
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- gz -----------------------------

/// One compressed file. The inner name comes from the header when the writer recorded one, and
/// from the container's own stem otherwise, so `syslog.gz` browses to `syslog` either way.
struct GzInner {
    decoder: flate2::read::GzDecoder<BufReader<File>>,
    name: String,
    modified: Option<FileTime>,
}

fn gz_inner(archive_file: &str) -> Result<GzInner, Stop> {
    let decoder = flate2::read::GzDecoder::new(open_shared(archive_file)?);
    let header = decoder
        .header()
        .ok_or_else(|| Stop::Malformed(DAMAGED.to_string()))?;
    let name = header
        .filename()
        .map(|f| String::from_utf8_lossy(f).into_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| {
            let name = file_name(archive_file);
            name[..name.len() - 3].to_string()
        });
    let modified = (header.mtime() != 0).then(|| unix_seconds(i64::from(header.mtime())));
    Ok(GzInner {
        decoder,
        name,
        modified,
    })
}

fn gz_list(archive_file: &str) -> Result<(Vec<RawArchiveEntry>, ArchiveCapabilities), Stop> {
    let GzInner { name, modified, .. } = gz_inner(archive_file)?;
    // The trailer carries the uncompressed length (modulo 4 GB) and the file length is the stored
    // one; neither costs a decode.
    let mut file = open_shared(archive_file)?;
    let compressed = file.seek(SeekFrom::End(0))? as i64;
    let mut trailer = [0u8; 4];
    let size = if compressed >= 18 {
        file.seek(SeekFrom::End(-4))?;
        file.read_exact(&mut trailer)?;
        i64::from(u32::from_le_bytes(trailer))
    } else {
        return Err(Stop::Malformed(DAMAGED.to_string()));
    };
    Ok((
        vec![RawArchiveEntry {
            key: Some(name),
            size,
            compressed_size: compressed,
            modified,
            is_directory: false,
            is_encrypted: false,
            link_target: None,
        }],
        ArchiveCapabilities::new(true, false, true),
    ))
}

fn gz_walk(archive_file: &str, pass: &mut Pass<'_>, _cancel: &CancelToken) -> Result<(), Stop> {
    let GzInner {
        mut decoder, name, ..
    } = gz_inner(archive_file)?;
    let Some(key) = pass.wants(Some(&name)) else {
        return Ok(());
    };
    // The size is read from the trailer by the listing; a walk hands the decoder over and lets the
    // caller count what it gets.
    let size = gz_list(archive_file)?.0[0].size;
    pass.announce(&key, &mut decoder, size)?;
    Ok(())
}
