//! Produces a new container from files on disk. Spec: Core/Services/Archives/ArchiveCreator.cs.
//!
//! **Over this app's own walk, not the library's**, so the browse setting decides about hidden
//! files, reparse points are skipped rather than followed, and a cancel can land inside a file.
//! **A cancelled create leaves nothing.** It writes to `<target>.bertbrowser-partial` and renames
//! on success: writing straight to `archive.zip` and cancelling would leave a truncated file under
//! exactly the name every other tool on the machine will try to open. Progress rides the
//! coalescer, so the status strip need have no idea this is not an ordinary transfer.

use std::fs::OpenOptions;
use std::io::{self, Read};
use std::os::windows::fs::OpenOptionsExt;

use bb_paths::{file_name, split_last};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
};

use super::{
    ArchiveSource, ArchiveType, ArchiveWriteFormat, ArchiveWriter, CompressionLevel,
    CompressionType, PauseGate, ProgressCoalescer, ProgressSink, WriterOptions,
};
use crate::FileTime;
use crate::cancel::CancelToken;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateArchiveOutcome {
    pub archive_path: String,
    pub files_written: i32,
    pub bytes_read: i64,
    pub failed: Vec<String>,
    pub cancelled: bool,
}

enum Stop {
    Cancelled,
    Failed(String),
}

#[derive(Default)]
pub struct ArchiveCreator;

impl ArchiveCreator {
    /// The suffix a half-written archive carries until it is finished.
    pub const PARTIAL_SUFFIX: &'static str = ".bertbrowser-partial";

    pub fn new() -> ArchiveCreator {
        ArchiveCreator
    }

    // The C# signature, kept until the transfer port's progress bundle arrives.
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        &self,
        archive_path: &str,
        format: ArchiveWriteFormat,
        level: CompressionLevel,
        sources: &[ArchiveSource],
        cancel: &CancelToken,
        progress: Option<ProgressSink<'_>>,
        pause: Option<&PauseGate>,
    ) -> CreateArchiveOutcome {
        let partial = format!("{archive_path}{}", Self::PARTIAL_SUFFIX);
        let mut run = ProgressCoalescer::new(cancel, progress, sources.len() as i32, pause);
        let mut failed: Vec<String> = Vec::new();
        let mut written = 0i32;
        let mut read = 0i64;
        let mut cancelled = false;

        match Self::build(
            archive_path,
            &partial,
            format,
            level,
            sources,
            cancel,
            &mut run,
            &mut failed,
            &mut written,
            &mut read,
        ) {
            Ok(()) => {}
            Err(Stop::Cancelled) => {
                cancelled = true;
                try_delete(&partial);
            }
            Err(Stop::Failed(message)) => {
                failed.push(message);
                try_delete(&partial);
            }
        }

        run.finished();
        CreateArchiveOutcome {
            archive_path: archive_path.to_string(),
            files_written: written,
            bytes_read: read,
            failed,
            cancelled,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn build(
        archive_path: &str,
        partial: &str,
        format: ArchiveWriteFormat,
        level: CompressionLevel,
        sources: &[ArchiveSource],
        cancel: &CancelToken,
        run: &mut ProgressCoalescer<'_, '_>,
        failed: &mut Vec<String>,
        written: &mut i32,
        read: &mut i64,
    ) -> Result<(), Stop> {
        let (parent, _) = split_last(archive_path);
        if !parent.is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| Stop::Failed(e.to_string()))?;
        }

        let mut writer = ArchiveWriter::create(
            partial,
            Self::type_for(format),
            Self::options_for(format, level),
        )
        .map_err(|e| Stop::Failed(e.to_string()))?;

        for source in sources {
            cancel.check().map_err(|_| Stop::Cancelled)?;
            run.begin_item(file_name(&source.path));
            run.begin_file(source.size_bytes);

            match Self::add(&mut writer, source, run, cancel) {
                Ok(bytes) => {
                    *read += bytes;
                    *written += 1;
                }
                Err(e) if cancel.is_cancelled() => {
                    let _ = e;
                    return Err(Stop::Cancelled);
                }
                Err(e) => failed.push(format!("{}: {e}", file_name(&source.path))),
            }

            run.end_file();
            run.end_item();
        }

        writer.finish().map_err(|e| Stop::Failed(e.to_string()))?;

        // Only now does the name everyone recognises come into being, and never over something
        // already there.
        if std::fs::symlink_metadata(archive_path).is_ok() {
            return Err(Stop::Failed(format!(
                "{} already exists.",
                file_name(archive_path)
            )));
        }
        std::fs::rename(partial, archive_path).map_err(|e| Stop::Failed(e.to_string()))
    }

    /// Streams one file in, counting what the writer pulls through so a cancel lands *inside* a
    /// file rather than only between them: a single large file is the case that matters.
    fn add(
        writer: &mut ArchiveWriter,
        source: &ArchiveSource,
        run: &mut ProgressCoalescer<'_, '_>,
        cancel: &CancelToken,
    ) -> io::Result<i64> {
        let file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .open(&source.path)?;
        let meta = file.metadata()?;
        let length = meta.len();
        let modified = FileTime::from_system_time(meta.modified()?);
        let mut counted = CountingReader {
            inner: file,
            run,
            cancel,
            position: 0,
            length: length as i64,
        };
        writer.write(&source.entry_name, &mut counted, length, Some(modified))?;
        Ok(counted.position)
    }

    fn type_for(format: ArchiveWriteFormat) -> ArchiveType {
        match format {
            ArchiveWriteFormat::Zip => ArchiveType::Zip,
            _ => ArchiveType::Tar,
        }
    }

    /// One place the options are chosen, because they genuinely differ per format and a switch
    /// inside the dialog would drift from the writer. Store is the right answer for a folder of
    /// JPEGs; Normal and Maximum are deflate's default and best levels, and for a while both
    /// mapped to plain Deflate and produced byte-identical archives.
    pub fn options_for(format: ArchiveWriteFormat, level: CompressionLevel) -> WriterOptions {
        match format {
            ArchiveWriteFormat::Zip => match level {
                CompressionLevel::Store => WriterOptions::new(CompressionType::None),
                CompressionLevel::Maximum => {
                    WriterOptions::new(CompressionType::Deflate).with_level(9)
                }
                CompressionLevel::Normal => {
                    WriterOptions::new(CompressionType::Deflate).with_level(6)
                }
            },
            ArchiveWriteFormat::Tar => WriterOptions::new(CompressionType::None),
            ArchiveWriteFormat::TarGz => WriterOptions::new(CompressionType::GZip),
            ArchiveWriteFormat::TarBz2 => WriterOptions::new(CompressionType::BZip2),
        }
    }
}

fn try_delete(path: &str) {
    if std::fs::symlink_metadata(path).is_ok() {
        let _ = std::fs::remove_file(path);
    }
}

/// Reports what the writer pulls through and lets a cancel land between chunks. Progress is read
/// from the position rather than accumulated, so a writer that seeks back and re-reads never
/// counts those bytes twice.
struct CountingReader<'a, 'p, 'g> {
    inner: std::fs::File,
    run: &'a mut ProgressCoalescer<'p, 'g>,
    cancel: &'a CancelToken,
    position: i64,
    length: i64,
}

impl Read for CountingReader<'_, '_, '_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.cancel.is_cancelled() {
            return Err(io::Error::other("cancelled"));
        }
        let n = self.inner.read(buf)?;
        if n > 0 {
            self.position += n as i64;
            self.run
                .file_progress(self.position, self.length.max(self.position));
        }
        Ok(n)
    }
}
