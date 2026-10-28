//! Reads and rewrites the metadata of one file format. Spec: Core/Services/Metadata/IMetadataCodec.cs.
//!
//! **A codec never opens a file.** It is handed streams, so the share flags, the cloud placeholder
//! refusal and the reparse rule stay with `ReadOnlyFile` and the executor decides where a rewrite
//! lands. **`payload_digest` is the safety argument.** It hashes everything that is *not*
//! metadata — the scan data of a picture, the frames of a song — and the executor refuses to swap a
//! rewrite in unless that hash is the one the original had. A codec bug then costs an error
//! message rather than somebody's photograph.

use std::fmt;
use std::fs::File;
use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};

use sha2::{Digest, Sha256};

use super::fields::{MetadataFamily, MetadataField};
use super::models::{MetadataDocument, MetadataEdit};
use crate::cancel::{CancelToken, Cancelled};

/// What a codec reads from: the file, or a buffer in a test.
pub trait Source: Read + Seek {}

impl<T: Read + Seek + ?Sized> Source for T {}

/// What a codec writes into: empty, readable, writable and seekable. Two more questions than
/// `Write + Seek` because the audio library edits its copy in place and needs to cut it short.
#[allow(clippy::len_without_is_empty)]
pub trait Destination: Read + Write + Seek {
    fn truncate(&mut self, len: u64) -> io::Result<()>;
    fn len(&self) -> io::Result<u64>;
}

impl Destination for File {
    fn truncate(&mut self, len: u64) -> io::Result<()> {
        self.set_len(len)
    }

    fn len(&self) -> io::Result<u64> {
        Ok(self.metadata()?.len())
    }
}

impl Destination for Cursor<Vec<u8>> {
    fn truncate(&mut self, len: u64) -> io::Result<()> {
        self.get_mut().truncate(len as usize);
        Ok(())
    }

    fn len(&self) -> io::Result<u64> {
        Ok(self.get_ref().len() as u64)
    }
}

/// Why a read or a rewrite did not happen.
#[derive(Debug)]
pub enum MetadataError {
    /// The file is not what its name says, is damaged past reading, or the change does not fit in
    /// it — in words fit to show.
    Format(String),
    Io(io::Error),
    Cancelled,
}

impl MetadataError {
    pub fn format(message: impl Into<String>) -> MetadataError {
        MetadataError::Format(message.into())
    }

    pub fn is_format(&self) -> bool {
        matches!(self, MetadataError::Format(_))
    }
}

impl fmt::Display for MetadataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MetadataError::Format(m) => f.write_str(m),
            MetadataError::Io(e) => write!(f, "{e}"),
            MetadataError::Cancelled => f.write_str("cancelled"),
        }
    }
}

impl std::error::Error for MetadataError {}

impl From<io::Error> for MetadataError {
    fn from(e: io::Error) -> MetadataError {
        MetadataError::Io(e)
    }
}

impl From<Cancelled> for MetadataError {
    fn from(_: Cancelled) -> MetadataError {
        MetadataError::Cancelled
    }
}

pub type MetadataResult<T> = Result<T, MetadataError>;

pub trait MetadataCodec {
    /// Which codec this is, for anything that needs to say so: `jpeg`, `audio`, `openxml`…
    fn id(&self) -> &'static str;

    fn family(&self) -> MetadataFamily;

    /// The fields this format has somewhere to put. Its own answer rather than its family's: two
    /// audio containers do not have room for the same things.
    fn fields(&self) -> &[MetadataField];

    /// Whether a cover picture can be embedded.
    fn holds_picture(&self) -> bool {
        false
    }

    /// `Format` when the stream is not this format.
    fn read(&self, source: &mut dyn Source) -> MetadataResult<MetadataDocument>;

    /// Writes `source` to `destination` with `edit` applied. Nothing but metadata may differ
    /// between the two. `Format` when the stream is not this format, or the change does not fit
    /// in it.
    fn write(
        &self,
        source: &mut dyn Source,
        destination: &mut dyn Destination,
        edit: &MetadataEdit,
        cancel: &CancelToken,
    ) -> MetadataResult<()>;

    /// A hash of everything in `source` that is not metadata.
    fn payload_digest(&self, source: &mut dyn Source) -> MetadataResult<Vec<u8>>;
}

// ---- stream helpers every codec shares ----------------------------------------------------

pub(crate) const COPY_BUFFER: usize = 128 * 1024;

pub(crate) fn stream_len(source: &mut dyn Source) -> io::Result<u64> {
    let here = source.stream_position()?;
    let end = source.seek(SeekFrom::End(0))?;
    source.seek(SeekFrom::Start(here))?;
    Ok(end)
}

/// `Stream.ReadAtLeast(buffer, buffer.Length, throwOnEndOfStream: false)`: as much as there is,
/// up to the buffer.
pub(crate) fn read_fully(source: &mut dyn Read, into: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < into.len() {
        match source.read(&mut into[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

/// Reads at `position`, as much as there is up to the buffer; zero when the position is past the
/// end. The C# `ReadAt` every payload locator used.
pub(crate) fn read_at(
    source: &mut dyn Source,
    position: u64,
    into: &mut [u8],
) -> io::Result<usize> {
    if position >= stream_len(source)? {
        return Ok(0);
    }
    source.seek(SeekFrom::Start(position))?;
    read_fully(source, into)
}

/// Copies `length` bytes from `offset` of `source` to `destination`, checking for cancellation
/// between blocks. `short` is the error for a source that ends early.
pub(crate) fn copy_range(
    source: &mut dyn Source,
    destination: &mut dyn Write,
    offset: u64,
    mut length: u64,
    cancel: &CancelToken,
    short: impl Fn() -> MetadataError,
) -> MetadataResult<()> {
    let mut buffer = vec![0u8; COPY_BUFFER];
    source.seek(SeekFrom::Start(offset))?;
    while length > 0 {
        cancel.check()?;
        let want = (buffer.len() as u64).min(length) as usize;
        let read = source.read(&mut buffer[..want])?;
        if read == 0 {
            return Err(short());
        }
        destination.write_all(&buffer[..read])?;
        length -= read as u64;
    }
    Ok(())
}

/// Copies from `offset` to the end of `source`.
pub(crate) fn copy_to_end(
    source: &mut dyn Source,
    destination: &mut dyn Write,
    offset: u64,
    cancel: &CancelToken,
) -> MetadataResult<()> {
    let mut buffer = vec![0u8; COPY_BUFFER];
    source.seek(SeekFrom::Start(offset))?;
    loop {
        cancel.check()?;
        let read = source.read(&mut buffer)?;
        if read == 0 {
            return Ok(());
        }
        destination.write_all(&buffer[..read])?;
    }
}

/// Hashes `length` bytes from `start`; `short` when the range runs past the end.
pub(crate) fn hash_range(
    hash: &mut Sha256,
    source: &mut dyn Source,
    start: u64,
    mut length: u64,
    short: impl Fn() -> MetadataError,
) -> MetadataResult<()> {
    if start
        .checked_add(length)
        .is_none_or(|end| end > stream_len(source).unwrap_or(0))
    {
        return Err(short());
    }
    source.seek(SeekFrom::Start(start))?;
    let mut buffer = vec![0u8; COPY_BUFFER];
    while length > 0 {
        let want = (buffer.len() as u64).min(length) as usize;
        let read = source.read(&mut buffer[..want])?;
        if read == 0 {
            return Err(short());
        }
        hash.update(&buffer[..read]);
        length -= read as u64;
    }
    Ok(())
}

pub(crate) fn finish(hash: Sha256) -> Vec<u8> {
    hash.finalize().to_vec()
}

/// Whether two streams hold the same bytes.
pub(crate) fn identical(a: &mut dyn Source, b: &mut dyn Source) -> io::Result<bool> {
    if stream_len(a)? != stream_len(b)? {
        return Ok(false);
    }
    a.seek(SeekFrom::Start(0))?;
    b.seek(SeekFrom::Start(0))?;
    let mut left = vec![0u8; COPY_BUFFER];
    let mut right = vec![0u8; COPY_BUFFER];
    loop {
        let read = read_fully(a, &mut left)?;
        if read_fully(b, &mut right[..read])? != read {
            return Ok(false);
        }
        if read == 0 {
            return Ok(true);
        }
        if left[..read] != right[..read] {
            return Ok(false);
        }
    }
}

/// The first index of `needle` in `haystack`.
pub(crate) fn index_of(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}
