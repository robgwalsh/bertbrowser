//! Test fixtures that need nothing of the app: a unique temp path, a junction maker, and the metadata
//! and archive fixtures copied verbatim from MetadataFixtures and ArchiveFixtures. A dev-dependency
//! only. bb-test-support (on bb-index) re-exports it; bb-file-fixtures (on bb-files and bb-archives)
//! holds the fakes that need those.

pub mod metadata_fixtures;
pub mod metadata_fixtures_data;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A unique name under %TEMP%, as the C# tests built theirs.
pub fn unique_temp_path(prefix: &str, suffix: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "{prefix}-{}-{n}-{nanos:x}{suffix}",
        std::process::id()
    ))
}

/// A directory junction, which, unlike a symlink, an ordinary account may create. Shelled out
/// because the standard library exposes no API for one. False when it could not be made.
pub fn try_create_junction(link: &std::path::Path, target: &std::path::Path) -> bool {
    use bb_base::attributes::{DIRECTORY, REPARSE_POINT};
    use std::os::windows::fs::MetadataExt;
    let _ = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output();
    match std::fs::symlink_metadata(link) {
        // The attribute bits: `FileType::is_dir` is false for a junction, which it reads as a symlink.
        Ok(meta) => {
            meta.file_attributes() & (DIRECTORY | REPARSE_POINT) == (DIRECTORY | REPARSE_POINT)
        }
        Err(_) => false,
    }
}

/// Containers this repository cannot produce, kept as base64 and written to a temp file by the
/// tests that need them. Copied verbatim from src/BertBrowser.Core/Services/Archives/
/// ArchiveFixtures.cs, with the command that produced each. **These are the ones that matter
/// most**: the rest of the archive fixtures are written with this crate's own writer, which makes
/// those round-trip tests rather than compatibility tests. Nothing here can write an encrypted zip
/// or a header-encrypted 7z, so these are the only evidence that the reader handles what other
/// tools really produce.
pub mod archive_fixtures {
    /// The password for `ENCRYPTED_ZIP`.
    pub const ZIP_PASSWORD: &str = "hunter2";

    /// The password for `HEADER_ENCRYPTED_SEVEN_ZIP`.
    pub const SEVEN_ZIP_PASSWORD: &str = "correct-horse";

    /// A zip holding `secret.txt` and `notes.txt`, AES-256 encrypted, headers in the clear, so the
    /// listing is readable and only the contents are not.
    /// 7-Zip 25.x: `7z a -tzip -phunter2 -mem=AES256 enc.zip secret.txt notes.txt`
    pub const ENCRYPTED_ZIP: &str = concat!(
        "UEsDBDMAAQBjAJqLHl0AAAAAJwAAAAsAAAAJAAsAbm90ZXMudHh0AZkHAAIAQUUDAAACnFlaLcnYjNP9jyKR7Egyrm3r0LG/O0vm",
        "GpP9Q5cBsogFdahIyERQSwMEMwABAGMAmoseXQAAAAAmAAAACgAAAAoACwBzZWNyZXQudHh0AZkHAAIAQUUDAAAnutkTAWK+mMVR",
        "TnY1AFxpNwOuPQhd4u5/PjhFXoGcjIliHMmMslBLAQI/ADMAAQBjAJqLHl0AAAAAJwAAAAsAAAAJAC8AAAAAAAAAIAAAAAAAAABu",
        "b3Rlcy50eHQKACAAAAAAAAEAGAA2CvWMxjjdATYK9YzGON0BNgr1jMY43QEBmQcAAgBBRQMAAFBLAQI/ADMAAQBjAJqLHl0AAAAA",
        "JgAAAAoAAAAKAC8AAAAAAAAAIAAAAFkAAABzZWNyZXQudHh0CgAgAAAAAAABABgAAaj0jMY43QEBqPSMxjjdAbqA9IzGON0BAZkH",
        "AAIAQUUDAABQSwUGAAAAAAIAAgDNAAAAsgAAAAAA"
    );

    /// A 7z with its *headers* encrypted, so not even the names can be listed without the
    /// password: the case that has to look different from an encrypted-contents zip.
    /// 7-Zip 25.x: `7z a -t7z -pcorrect-horse -mhe=on enchdr.7z secret.txt notes.txt`
    pub const HEADER_ENCRYPTED_SEVEN_ZIP: &str = concat!(
        "N3q8ryccAASKxhnysAAAAAAAAAA+AAAAAAAAANeaSMmORgcj9UHYJ6oJ0HuFjuISYs7nVh6Ea8Y/+KOZO0KfX8eLFfkVanwLMxqJ",
        "OIkRR0FrSFFLWtYotGnjDpa51pm1jFAT9mAVEIRVwnDEqmHIXeZJvvcLUVrZqfeS2YRkOl8/bx5tv4RxQ+Ot4bW/26AO+h7SRP1n",
        "tLLzZV08AUPGOhnQDxxWeMe7+I3z0z9K4ymfKftaHws8L8sUsL993FMREyDWXy6GfuWzbo00uplhhBcGIAEJgJAABwsBAAIkBvEH",
        "ARJTD7duEwaZGJFAP0kVEQmi5r0jAwEBBV0AEAAAAQAMgISAngoBo37mIAAA"
    );

    /// An ordinary 7z holding the same two files. Nothing here can *write* 7z, so without this
    /// there would be no evidence the reader handles the format at all.
    /// 7-Zip 25.x: `7z a -t7z plain.7z secret.txt notes.txt`
    pub const PLAIN_SEVEN_ZIP: &str = concat!(
        "N3q8ryccAAQm7sIAgQAAAAAAAAAgAAAAAAAAAJBsR/4BABRhbHNvIHNlY3JldGNsYXNzaWZpZWQAAACBMweuD8+SbmAP6+qcvzY9",
        "/nHKUfIBttljND19I61lOccsXwBFigB+kaExKBg6qKVdGIsXhoIiXqndBZ1CuVXh1KU+KEr3BbWiHV/exhdmDVOsDDRJvuEzQZid",
        "SjEFX/mu6jMJvyAXBhkBCWgABwsBAAEjAwEBBV0AEAAADH4KAXdY2lkAAA=="
    );

    /// Writes one of the fixtures to `path` and returns it.
    pub fn write_to(path: &std::path::Path, base64: &str) -> String {
        std::fs::write(path, decode(base64)).expect("write fixture");
        path.to_string_lossy().into_owned()
    }

    /// Standard base64, padding optional. Small enough to keep the support crate dependency-free.
    pub fn decode(text: &str) -> Vec<u8> {
        fn value(c: u8) -> Option<u32> {
            match c {
                b'A'..=b'Z' => Some(u32::from(c - b'A')),
                b'a'..=b'z' => Some(u32::from(c - b'a') + 26),
                b'0'..=b'9' => Some(u32::from(c - b'0') + 52),
                b'+' => Some(62),
                b'/' => Some(63),
                _ => None,
            }
        }
        let mut out = Vec::with_capacity(text.len() * 3 / 4);
        let (mut acc, mut bits) = (0u32, 0u32);
        for &c in text.as_bytes() {
            let Some(v) = value(c) else {
                continue;
            };
            acc = (acc << 6) | v;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((acc >> bits) as u8);
                acc &= (1 << bits) - 1;
            }
        }
        out
    }
}
