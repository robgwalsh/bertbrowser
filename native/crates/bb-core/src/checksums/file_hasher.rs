//! The real-filesystem `FileHasher` and `FileDigester`. Spec: the `FileSystemFileHasher` class in
//! Core/Services/Duplicates/IFileHasher.cs, and Core/Interop/FileIdentityNative.cs.
//!
//! **SHA-256 for the duplicate finder.** It is hardware-accelerated on every machine this app runs
//! on and its equality is strong enough to act on — which matters, because what the user does with
//! the answer is delete files. A faster non-cryptographic hash would have to be followed by a
//! byte-for-byte compare to be safe, and that costs more than the stronger hash did. The checksum
//! tool's choice of algorithm reaches `digest` and can never reach `hash`: that is why they are two
//! traits rather than one with a parameter.
//!
//! **One read loop.** Both seams run `read_file`, so the sharing rules, the placeholder and reparse
//! refusals, the buffer size and the per-chunk progress contract exist once. A regression in any of
//! them shows up in both features at the same time, which is the point —
//! `file_system_file_hasher_tests` is what holds them down and it did not change when the digester
//! arrived.
//!
//! **Nothing is held open.** Every read goes through `ReadOnlyFile`, the rule the preview pane
//! already follows: hashing must never block this app's own rename, move and delete executors,
//! which is what a plain read lock would do to the folder the user is standing in.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::os::windows::io::AsRawHandle;

use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
};

use super::sink::DigestSink;
use super::{ByteProgress, ChecksumAlgorithm, ChecksumAlgorithms, FileDigester, FileDigests};
use crate::cancel::{CancelToken, Cancelled};
use crate::duplicates::{FileFingerprint, FileHasher, FileIdentity};
use crate::fs::ReadOnlyFile;

/// Big enough that a large file is a few hundred reads, small enough not to matter.
const BUFFER_BYTES: usize = 1024 * 1024;

#[derive(Default)]
pub struct FileSystemFileHasher;

impl FileSystemFileHasher {
    pub fn new() -> FileSystemFileHasher {
        FileSystemFileHasher
    }
}

impl FileHasher for FileSystemFileHasher {
    fn hash(
        &self,
        path: &str,
        max_bytes: i64,
        progress: ByteProgress<'_>,
        cancel: &CancelToken,
    ) -> Result<Option<FileFingerprint>, Cancelled> {
        let mut sinks = vec![DigestSink::create(ChecksumAlgorithm::Sha256)];

        let Some(outcome) = read_file(path, max_bytes, &mut sinks, true, progress, cancel)? else {
            return Ok(None);
        };

        let digest = sinks.pop().expect("one sink").finish();
        Ok(Some(FileFingerprint {
            hash: digest,
            bytes_read: outcome.bytes_read,
            identity: outcome.identity,
        }))
    }
}

impl FileDigester for FileSystemFileHasher {
    fn digest(
        &self,
        path: &str,
        algorithms: &[ChecksumAlgorithm],
        progress: ByteProgress<'_>,
        cancel: &CancelToken,
    ) -> Result<Option<FileDigests>, Cancelled> {
        let wanted = ChecksumAlgorithms::normalise(algorithms.iter().copied());

        // Asking for nothing is an empty answer, not a failure — a UI with every box unticked has
        // nothing to say about a file, which is different from being unable to read it.
        if wanted.is_empty() {
            return Ok(Some(FileDigests {
                by_algorithm: HashMap::new(),
                bytes_read: 0,
            }));
        }

        let mut sinks: Vec<DigestSink> = wanted.iter().map(|&a| DigestSink::create(a)).collect();

        let Some(outcome) = read_file(path, 0, &mut sinks, false, progress, cancel)? else {
            return Ok(None);
        };

        let by_algorithm = wanted
            .into_iter()
            .zip(sinks)
            .map(|(algorithm, sink)| (algorithm, sink.finish()))
            .collect();

        Ok(Some(FileDigests {
            by_algorithm,
            bytes_read: outcome.bytes_read,
        }))
    }
}

/// What one pass over a file learned, beyond what the digests themselves hold.
struct ReadOutcome {
    bytes_read: i64,
    identity: Option<FileIdentity>,
}

/// Reads a file once, feeding every byte to every sink. The shared half of both seams. Note that
/// `progress` is invoked once per chunk regardless of how many sinks there are: the caller is being
/// told how much of the file has been read, not how much hashing happened.
fn read_file(
    path: &str,
    max_bytes: i64,
    sinks: &mut [DigestSink],
    want_identity: bool,
    progress: ByteProgress<'_>,
    cancel: &CancelToken,
) -> Result<Option<ReadOutcome>, Cancelled> {
    cancel.check()?;

    // A reparse point is the one entry it is, not the file it points at — the same reading the
    // delete surveyor takes. Following it would hash the target a second time under a name that is
    // not really a copy of anything.
    let Some(mut file) = ReadOnlyFile::try_open(path, false) else {
        return Ok(None);
    };

    let identity = if want_identity {
        read_identity(&file)
    } else {
        None
    };

    let mut buffer = vec![0u8; BUFFER_BYTES];
    let limit = if max_bytes > 0 { max_bytes } else { i64::MAX };
    let mut read = 0i64;

    while read < limit {
        cancel.check()?;

        let want = (limit - read).min(buffer.len() as i64) as usize;
        let got = match file.read(&mut buffer[..want]) {
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            // Failing part-way is still a failure: a hash of the first half of a file would
            // compare equal to nothing and unequal to everything, silently.
            Err(_) => return Ok(None),
        };

        if got == 0 {
            break;
        }

        let chunk = &buffer[..got];
        for sink in sinks.iter_mut() {
            sink.append(chunk);
        }

        read += got as i64;
        if let Some(report) = progress {
            report(got as i64);
        }
    }

    Ok(Some(ReadOutcome {
        bytes_read: read,
        identity,
    }))
}

/// Asks an already-open handle which file on disk it actually refers to, and answers only when it
/// carries more than one name.
///
/// NTFS lets one file carry several names. A hardlinked file appears once per name in the MFT —
/// same size, same bytes — so a duplicate finder that only compared content would report every one
/// of them as a copy worth deleting, when deleting one frees nothing at all. `C:\Windows\WinSxS` is
/// built almost entirely this way. The volume serial plus the file index is the identity that
/// settles it, at one call on a handle the hasher has open anyway. A call that fails is not an
/// error worth surfacing: a filesystem that will not answer gets the safe reading, one link and no
/// identity. Under-collapsing shows a real file twice; over-collapsing would hide a duplicate the
/// user could have removed.
fn read_identity(file: &File) -> Option<FileIdentity> {
    // SAFETY: an all-zero BY_HANDLE_FILE_INFORMATION is a valid value of a plain-data struct.
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: the handle is open for the lifetime of `file`; the out pointer is valid for the call.
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) };
    if ok == 0 || info.nNumberOfLinks <= 1 {
        return None;
    }
    Some(FileIdentity {
        volume_serial_number: info.dwVolumeSerialNumber,
        file_index: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    })
}
