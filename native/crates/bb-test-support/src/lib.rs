//! Shared test helpers: temp databases and trees under %TEMP%, in-memory pipe pairs, fixtures copied
//! verbatim from MetadataFixtures and ArchiveFixtures, in-memory probes. A dev-dependency only.

pub mod metadata_fixtures;
pub mod metadata_fixtures_data;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bb_core::data::Db;

pub mod duplex;
pub mod pipe_fakes;

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

/// A migrated database in a temp file, deleted with its WAL and SHM on drop.
pub struct TempDb {
    pub path: PathBuf,
    pub db: Arc<Db>,
}

impl TempDb {
    pub fn new() -> TempDb {
        let path = unique_temp_path("bertbrowser-test", ".db");
        let db = Arc::new(Db::new(path.clone()));
        db.migrate().expect("migrate");
        TempDb { path, db }
    }

    /// A database file that was never migrated: no tables at all.
    pub fn bare() -> TempDb {
        let path = unique_temp_path("bertbrowser-test", ".db");
        TempDb {
            path: path.clone(),
            db: Arc::new(Db::new(path)),
        }
    }

    pub fn db(&self) -> Arc<Db> {
        self.db.clone()
    }
}

impl Default for TempDb {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        self.db.clear_pool();
        remove_db_files(&self.path);
    }
}

/// Deletes `path` and its `-wal`/`-shm` siblings, ignoring what is not there.
pub fn remove_db_files(path: &Path) {
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let mut p = path.as_os_str().to_owned();
        p.push(suffix);
        let _ = std::fs::remove_file(p);
    }
}

/// A scratch tree under %TEMP%, erased on drop, and a writer for the archives the tests round-trip
/// through, standing in for the `WriterFactory.Open` calls the C# tests made.
pub mod archive_support {
    use std::path::PathBuf;

    use bb_core::FileTime;
    use bb_core::archives::{ArchiveType, ArchiveWriter, CompressionType, WriterOptions};
    use bb_core::fs::DirectoryRemoval;

    pub struct TempRoot {
        pub root: PathBuf,
    }

    impl TempRoot {
        pub fn new(prefix: &str) -> TempRoot {
            let root = super::unique_temp_path(prefix, "");
            std::fs::create_dir_all(&root).expect("create temp root");
            TempRoot { root }
        }

        /// `root\relative`, with `/` accepted as a separator.
        pub fn path(&self, relative: &str) -> String {
            let mut p = self.root.clone();
            for part in relative.split(['/', '\\']) {
                p.push(part);
            }
            p.to_string_lossy().into_owned()
        }

        pub fn root_str(&self) -> String {
            self.root.to_string_lossy().into_owned()
        }

        /// Writes a file, creating its folders.
        pub fn make(&self, relative: &str, body: &[u8]) -> String {
            let path = self.path(relative);
            if let Some(parent) = std::path::Path::new(&path).parent() {
                std::fs::create_dir_all(parent).expect("create parent");
            }
            std::fs::write(&path, body).expect("write file");
            path
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = DirectoryRemoval::remove_tree(&self.root.to_string_lossy());
        }
    }

    /// The timestamp every fixture entry carries, as the C# tests' `new DateTime(2026, 2, 3, 4, 5,
    /// 6)` did.
    pub fn fixture_time() -> FileTime {
        FileTime::from_ymd_hms(2026, 2, 3, 4, 5, 6)
    }

    /// Writes a container holding `entries` and returns its path.
    pub fn write_archive(
        path: &str,
        kind: ArchiveType,
        compression: CompressionType,
        entries: &[(&str, &[u8])],
    ) -> String {
        let mut writer = ArchiveWriter::create(path, kind, WriterOptions::new(compression))
            .expect("open writer");
        for (key, body) in entries {
            let mut source = std::io::Cursor::new(body.to_vec());
            writer
                .write(key, &mut source, body.len() as u64, Some(fixture_time()))
                .expect("write entry");
        }
        writer.finish().expect("finish");
        path.to_string()
    }

    /// A deflated zip at `path`.
    pub fn write_zip(path: &str, entries: &[(&str, &[u8])]) -> String {
        write_archive(path, ArchiveType::Zip, CompressionType::Deflate, entries)
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
