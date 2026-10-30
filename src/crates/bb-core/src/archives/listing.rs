//! Lists a zip container's contents from its central directory for the preview pane. Only the
//! directory is read: no entry is ever opened, decompressed or written anywhere, so previewing an
//! archive costs the same whether it holds one file or a gigabyte of them.
//! Spec: Core/Services/Preview/ArchiveListing.cs.

use std::io::{Read, Seek};

use bb_paths::fold_upper;

use crate::FileTime;

/// One entry inside an archive. Nothing is extracted to produce it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveEntry {
    pub path: String,
    pub size_bytes: i64,
    pub compressed_bytes: i64,
    /// UTC; unknown when the container carried none.
    pub modified: FileTime,
    pub is_directory: bool,
}

/// What an archive holds, or why it could not be read. `error` is set when the archive could not
/// be opened and `entries` is then empty: a damaged archive is a message, never a panic, since a
/// preview must not be able to fail louder than the thing it is previewing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveContents {
    /// Up to the entry cap, ordered by path.
    pub entries: Vec<ArchiveEntry>,
    /// Every entry, including the ones past the cap.
    pub total_count: i32,
    pub total_bytes: i64,
    pub total_compressed_bytes: i64,
    /// There are more entries than `entries` shows.
    pub truncated: bool,
    pub error: Option<String>,
}

impl ArchiveContents {
    pub fn failed(error: &str) -> ArchiveContents {
        ArchiveContents {
            entries: Vec::new(),
            total_count: 0,
            total_bytes: 0,
            total_compressed_bytes: 0,
            truncated: false,
            error: Some(error.to_string()),
        }
    }

    /// Ratio of packed to unpacked, 0 when there is nothing to compare.
    pub fn compression_ratio(&self) -> f64 {
        if self.total_bytes > 0 {
            1.0 - self.total_compressed_bytes as f64 / self.total_bytes as f64
        } else {
            0.0
        }
    }
}

pub struct ArchiveListing;

impl ArchiveListing {
    /// Rows past this are counted but not listed. A `node_modules` zip has hundreds of thousands of
    /// entries and nobody scrolls them in a preview pane.
    pub const DEFAULT_MAX_ENTRIES: usize = 1_000;

    /// Reads the directory of the zip at `stream`, which is left open for the caller to close.
    pub fn read<R: Read + Seek>(stream: &mut R, max_entries: usize) -> ArchiveContents {
        let archive = match zip::ZipArchive::new(stream) {
            Ok(a) => a,
            Err(zip::result::ZipError::Io(_)) => {
                return ArchiveContents::failed("The archive could not be read.");
            }
            Err(_) => return ArchiveContents::failed("Not a readable archive, or damaged."),
        };

        let mut rows = Vec::with_capacity(max_entries.min(archive.len()));
        let (mut total_bytes, mut total_compressed) = (0i64, 0i64);

        for i in 0..archive.len() {
            let entry = match archive.by_index_data(i) {
                Ok(e) => e,
                Err(zip::result::ZipError::Io(_)) => {
                    return ArchiveContents::failed("The archive could not be read.");
                }
                Err(_) => return ArchiveContents::failed("Not a readable archive, or damaged."),
            };
            let size = entry.size() as i64;
            let compressed = entry.compressed_size() as i64;
            total_bytes += size;
            total_compressed += compressed;

            let name = entry
                .name()
                .map(|n| n.into_owned())
                .unwrap_or_else(|_| String::from_utf8_lossy(entry.name_raw()).into_owned());
            // A folder inside a zip is a zero-length entry whose name ends in a separator; it
            // carries no data and is listed for shape, not for size.
            let is_directory = name.ends_with('/') || name.ends_with('\\');

            rows.push(ArchiveEntry {
                path: name.replace('/', "\\"),
                size_bytes: if is_directory { 0 } else { size },
                compressed_bytes: if is_directory { 0 } else { compressed },
                modified: entry
                    .last_modified()
                    .map(|dt| {
                        FileTime::from_ymd_hms(
                            i64::from(dt.year()),
                            u32::from(dt.month()),
                            u32::from(dt.day()),
                            u32::from(dt.hour()),
                            u32::from(dt.minute()),
                            u32::from(dt.second()),
                        )
                        .local_to_utc()
                    })
                    .unwrap_or(FileTime::UNKNOWN),
                is_directory,
            });
        }

        let total = rows.len();
        rows.sort_by_cached_key(|r| fold_upper(&r.path));

        let truncated = total > max_entries;
        if truncated {
            rows.truncate(max_entries);
        }

        ArchiveContents {
            entries: rows,
            total_count: total as i32,
            total_bytes,
            total_compressed_bytes: total_compressed,
            truncated,
            error: None,
        }
    }
}
