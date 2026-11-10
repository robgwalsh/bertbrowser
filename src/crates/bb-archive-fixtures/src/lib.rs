//! A scratch tree under %TEMP%, erased on drop, and a writer for the archives the tests round-trip
//! through, standing in for the `WriterFactory.Open` calls the C# tests made.
//! Its own crate because it needs bb-archives' writer: bb-fixtures stays below bb-archives, and
//! bb-test-support above bb-core. Re-exported as `bb_test_support::archive_support`.

pub mod archive_support {
    use std::path::PathBuf;

    use bb_archives::{ArchiveType, ArchiveWriter, CompressionType, WriterOptions};
    use bb_base::FileTime;
    use bb_base::fs::DirectoryRemoval;

    pub struct TempRoot {
        pub root: PathBuf,
    }

    impl TempRoot {
        pub fn new(prefix: &str) -> TempRoot {
            let root = bb_fixtures::unique_temp_path(prefix, "");
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
