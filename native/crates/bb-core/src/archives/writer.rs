//! Writing a container, one entry at a time: the zip and tar families this app produces, and a
//! single gzipped file for fixtures. Spec: the `WriterFactory.Open` / `WriterOptions` calls in
//! Core/Services/Archives/{ArchiveCreator,ArchiveEditExecutor}.cs, confined to one module for
//! the reason the reader is.

use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Read, Write};
use std::os::windows::fs::OpenOptionsExt;

use crate::FileTime;

/// The container kind a writer produces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArchiveType {
    Zip,
    Tar,
    /// One compressed file carrying the entry's name in its header.
    GZip,
}

/// What the container's bytes are run through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompressionType {
    None,
    Deflate,
    GZip,
    BZip2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriterOptions {
    pub compression_type: CompressionType,
    /// Deflate's 0..=9; ignored by the others.
    pub compression_level: i32,
}

impl WriterOptions {
    pub fn new(compression_type: CompressionType) -> WriterOptions {
        WriterOptions {
            compression_type,
            compression_level: 6,
        }
    }

    pub fn with_level(mut self, level: i32) -> WriterOptions {
        self.compression_level = level;
        self
    }
}

enum TarSink {
    Plain(BufWriter<File>),
    Gz(flate2::write::GzEncoder<BufWriter<File>>),
    Bz(bzip2::write::BzEncoder<BufWriter<File>>),
}

impl Write for TarSink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            TarSink::Plain(w) => w.write(buf),
            TarSink::Gz(w) => w.write(buf),
            TarSink::Bz(w) => w.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            TarSink::Plain(w) => w.flush(),
            TarSink::Gz(w) => w.flush(),
            TarSink::Bz(w) => w.flush(),
        }
    }
}

impl TarSink {
    fn finish(self) -> io::Result<()> {
        match self {
            TarSink::Plain(mut w) => w.flush(),
            TarSink::Gz(w) => w.finish()?.flush(),
            TarSink::Bz(w) => w.finish()?.flush(),
        }
    }
}

enum Inner {
    Zip {
        // Boxed: the zip writer's buffers dwarf the other variants.
        writer: Box<zip::ZipWriter<BufWriter<File>>>,
        method: zip::CompressionMethod,
        level: Option<i64>,
    },
    Tar(tar::Builder<TarSink>),
    GZip {
        /// The file until the one entry arrives, then None.
        file: Option<BufWriter<File>>,
        encoder: Option<flate2::write::GzEncoder<BufWriter<File>>>,
    },
}

pub struct ArchiveWriter {
    inner: Inner,
}

impl ArchiveWriter {
    /// Opens `path` for writing, exclusively, and prepares a container of `kind`.
    pub fn create(
        path: &str,
        kind: ArchiveType,
        options: WriterOptions,
    ) -> io::Result<ArchiveWriter> {
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .share_mode(0)
            .open(path)?;
        let file = BufWriter::with_capacity(64 * 1024, file);

        let inner = match kind {
            ArchiveType::Zip => {
                let (method, level) = match options.compression_type {
                    CompressionType::None => (zip::CompressionMethod::Stored, None),
                    _ => (
                        zip::CompressionMethod::Deflated,
                        Some(i64::from(options.compression_level.clamp(0, 9))),
                    ),
                };
                Inner::Zip {
                    writer: Box::new(zip::ZipWriter::new(file)),
                    method,
                    level,
                }
            }
            ArchiveType::Tar => {
                let sink = match options.compression_type {
                    CompressionType::GZip => TarSink::Gz(flate2::write::GzEncoder::new(
                        file,
                        flate2::Compression::default(),
                    )),
                    CompressionType::BZip2 => TarSink::Bz(bzip2::write::BzEncoder::new(
                        file,
                        bzip2::Compression::default(),
                    )),
                    _ => TarSink::Plain(file),
                };
                Inner::Tar(tar::Builder::new(sink))
            }
            ArchiveType::GZip => Inner::GZip {
                file: Some(file),
                encoder: None,
            },
        };
        Ok(ArchiveWriter { inner })
    }

    /// Appends one entry. `name` uses forward slashes; `size` is what the tar header has to know
    /// up front; `modified` is UTC, written as the container's own local or Unix form.
    pub fn write(
        &mut self,
        name: &str,
        data: &mut dyn Read,
        size: u64,
        modified: Option<FileTime>,
    ) -> io::Result<()> {
        match &mut self.inner {
            Inner::Zip {
                writer,
                method,
                level,
            } => {
                let mut options = zip::write::SimpleFileOptions::default()
                    .compression_method(*method)
                    .compression_level(*level)
                    .large_file(size >= 0xFFFF_FFFF);
                if let Some(dt) = modified.and_then(zip_time) {
                    options = options.last_modified_time(dt);
                }
                writer.start_file(name, options).map_err(zip_io)?;
                io::copy(data, writer)?;
                Ok(())
            }
            Inner::Tar(builder) => {
                let mut header = tar::Header::new_gnu();
                header.set_size(size);
                header.set_mode(0o644);
                header.set_mtime(unix_secs(modified));
                header.set_entry_type(tar::EntryType::Regular);
                builder.append_data(&mut header, name, data)
            }
            Inner::GZip { file, encoder } => {
                let Some(file) = file.take() else {
                    return Err(io::Error::other("a gzip file holds exactly one entry"));
                };
                let mut gz = flate2::GzBuilder::new()
                    .filename(name)
                    .mtime(unix_secs(modified) as u32)
                    .write(file, flate2::Compression::default());
                io::copy(data, &mut gz)?;
                *encoder = Some(gz);
                Ok(())
            }
        }
    }

    /// Writes the trailer and closes the file.
    pub fn finish(self) -> io::Result<()> {
        match self.inner {
            Inner::Zip { writer, .. } => {
                let mut file = writer.finish().map_err(zip_io)?;
                file.flush()
            }
            Inner::Tar(builder) => builder.into_inner()?.finish(),
            Inner::GZip {
                encoder: Some(encoder),
                ..
            } => encoder.finish()?.flush(),
            Inner::GZip {
                file: Some(file), ..
            } => {
                // An empty gzip member still names a well-formed file.
                let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
                encoder.finish()?.flush()
            }
            Inner::GZip { .. } => Ok(()),
        }
    }
}

fn zip_io(e: zip::result::ZipError) -> io::Error {
    match e {
        zip::result::ZipError::Io(io) => io,
        other => io::Error::other(other.to_string()),
    }
}

fn zip_time(utc: FileTime) -> Option<zip::DateTime> {
    let c = utc.to_local().civil();
    zip::DateTime::from_date_and_time(
        u16::try_from(c.year).ok()?,
        c.month as u8,
        c.day as u8,
        c.hour as u8,
        c.minute as u8,
        c.second as u8,
    )
    .ok()
}

fn unix_secs(utc: Option<FileTime>) -> u64 {
    let Some(utc) = utc else {
        return 0;
    };
    (utc.to_unix_millis() / 1000).max(0) as u64
}
