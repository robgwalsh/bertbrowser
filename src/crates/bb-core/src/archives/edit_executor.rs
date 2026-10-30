//! Carries out an `ArchiveEditPlan` by rewriting the container beside itself and swapping it in.
//! Spec: Core/Services/Archives/ArchiveEditExecutor.cs.
//!
//! **The order is the safety.** Write a sibling; verify it opens and holds what was planned; move
//! the original into staging; rename the sibling into its place. Nothing is deleted to make room
//! and a failure at any step puts everything back, the invariant the transfer executor keeps for
//! a Replace, because it is the same risk: the thing being displaced is the user's own data.
//! **The original goes to staging, not to the bin and not to oblivion.** That is what makes the
//! edit undoable, and retiring its history entry is the only thing that finally erases it. An
//! undo sets the *edited* container aside in turn, under `EDITED_MARKER`, so the edit can be
//! redone. **A cancel deletes the sibling and touches nothing else.**

use std::fs::OpenOptions;
use std::hash::{BuildHasher, RandomState};
use std::io::{self, Cursor, Read};
use std::os::windows::fs::OpenOptionsExt;

use bb_paths::{file_name, fold_upper, split_last};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
};

use super::{
    ArchiveContainer, ArchiveEditOutcome, ArchiveEditPlan, ArchiveEditUndo, ArchiveFormat,
    ArchiveFormats, ArchiveReadError, ArchiveReader, ArchiveType, ArchiveWriter, CompressionType,
    ExtractRules, WriterOptions,
};
use crate::FileTime;
use crate::cancel::{CancelToken, Cancelled};
use crate::fs::EntryStamp;
use crate::transfer::{PauseGate, ProgressCoalescer, TransferProgress};

pub struct ArchiveEditExecutor<'r> {
    reader: &'r dyn ArchiveReader,
}

enum Stop {
    Cancelled,
    Failed(String),
}

impl From<io::Error> for Stop {
    fn from(e: io::Error) -> Stop {
        Stop::Failed(e.to_string())
    }
}

impl<'r> ArchiveEditExecutor<'r> {
    /// Marks a container being built, before it is swapped in.
    pub const REWRITE_MARKER: &'static str = ".bertbrowser-rewrite-";

    /// Marks a replaced container while it is still undoable.
    pub const REPLACED_MARKER: &'static str = ".bertbrowser-replaced-";

    /// Marks an edited container an undo set aside, while it can still be redone.
    pub const EDITED_MARKER: &'static str = ".bertbrowser-edited-";

    pub fn new(reader: &'r dyn ArchiveReader) -> ArchiveEditExecutor<'r> {
        ArchiveEditExecutor { reader }
    }

    pub fn execute(
        &self,
        plan: &ArchiveEditPlan,
        cancel: &CancelToken,
        progress: Option<&mut dyn FnMut(TransferProgress)>,
        pause: Option<&PauseGate>,
    ) -> ArchiveEditOutcome {
        if !plan.has_work() {
            return ArchiveEditOutcome::nothing(&plan.archive_file);
        }

        let archive = plan.archive_file.as_str();
        let rewrite = Self::beside(archive, Self::REWRITE_MARKER);
        let mut run = ProgressCoalescer::new(cancel.clone(), progress, 0, pause);

        let result = (|| -> Result<ArchiveEditOutcome, Stop> {
            let written = self.build_replacement(plan, &rewrite, &mut run, cancel)?;

            // Verified before anything irreversible happens: if the container just written will
            // not open, the one on disk is still untouched and this costs nothing but a temp file.
            let check = self.reader.read(&rewrite, None, &CancelToken::none());
            if !check.ok() {
                try_delete(&rewrite);
                return Ok(ArchiveEditOutcome::new(
                    archive,
                    None,
                    0,
                    Some(format!(
                        "The rewritten archive could not be read back ({}), so nothing was changed.",
                        check.error.as_deref().unwrap_or("")
                    )),
                    false,
                ));
            }

            let original = EntryStamp::of(archive);
            let staged = Self::beside(archive, Self::REPLACED_MARKER);
            std::fs::rename(archive, &staged)?;

            if let Err(e) = std::fs::rename(&rewrite, archive) {
                // Put the original back before letting the failure out. Leaving the archive absent
                // because the second half of a swap failed is the one outcome this must not have.
                try_restore(&staged, archive);
                return Err(e.into());
            }

            run.finished();
            let mut outcome = ArchiveEditOutcome::new(archive, Some(staged), written, None, false);
            outcome.original = original;
            outcome.edited = EntryStamp::of(archive);
            Ok(outcome)
        })();

        match result {
            Ok(outcome) => outcome,
            Err(Stop::Cancelled) => {
                try_delete(&rewrite);
                run.finished();
                ArchiveEditOutcome::new(archive, None, 0, None, true)
            }
            Err(Stop::Failed(message)) => {
                try_delete(&rewrite);
                run.finished();
                ArchiveEditOutcome::new(archive, None, 0, Some(message), false)
            }
        }
    }

    /// Undoes an edit by swapping the staged original back in. The edited container is set aside
    /// under `EDITED_MARKER` rather than deleted, which is what lets it be redone. Refused when
    /// the container is no longer the one the edit wrote: the history can reach an edit many
    /// operations later, and setting aside something else's rewrite for an older original would
    /// be an undo of somebody else's change. Uncancellable and silent on purpose.
    pub fn undo(&self, outcome: &ArchiveEditOutcome) -> ArchiveEditUndo {
        let archive = outcome.archive_file.as_str();
        let refuse = |why: String| ArchiveEditUndo {
            archive_file: archive.to_string(),
            staged_edited: None,
            original: outcome.original,
            edited: outcome.edited,
            failure: Some(why),
        };

        let Some(staged) = outcome.staged_original.as_deref() else {
            return refuse("There is nothing to put back.".to_string());
        };
        if !exists(staged) {
            return refuse("The replaced archive is no longer there.".to_string());
        }
        if let Some(edited) = outcome.edited
            && exists(archive)
            && !edited.matches(archive)
        {
            return refuse(format!(
                "{} has changed since the edit, so it was left as it is.",
                file_name(archive)
            ));
        }

        let aside = Self::beside(archive, Self::EDITED_MARKER);
        let swap = || -> io::Result<()> {
            if exists(archive) {
                std::fs::rename(archive, &aside)?;
            }
            if let Err(e) = std::fs::rename(staged, archive) {
                try_restore(&aside, archive);
                return Err(e);
            }
            Ok(())
        };

        match swap() {
            // Stamped afresh rather than carried over: a rename into a name freed seconds ago
            // takes on the old entry's creation time (NTFS tunnelling), so only a stamp taken
            // after the move describes what is really there now.
            Ok(()) => ArchiveEditUndo {
                archive_file: archive.to_string(),
                staged_edited: exists(&aside).then_some(aside),
                original: EntryStamp::of(archive),
                edited: outcome.edited,
                failure: None,
            },
            Err(e) => refuse(format!(
                "The archive could not be put back: {e}. It is at {staged}."
            )),
        }
    }

    /// Takes an undone edit back in: the original is set aside again and the edited container
    /// returns. Refused when the container in place is no longer the original the undo restored.
    pub fn redo(&self, undo: &ArchiveEditUndo) -> ArchiveEditOutcome {
        let archive = undo.archive_file.as_str();
        let refuse = |why: String| {
            let mut o = ArchiveEditOutcome::new(archive, None, 0, Some(why), false);
            o.original = undo.original;
            o.edited = undo.edited;
            o
        };

        let Some(edited) = undo.staged_edited.as_deref() else {
            return refuse("There is no edit to take back in.".to_string());
        };
        if !exists(edited) {
            return refuse("The edited archive is no longer being held.".to_string());
        }
        if let Some(original) = undo.original
            && exists(archive)
            && !original.matches(archive)
        {
            return refuse(format!(
                "{} has changed since the undo, so it was left as it is.",
                file_name(archive)
            ));
        }

        let staged = Self::beside(archive, Self::REPLACED_MARKER);
        let before = EntryStamp::of(archive);
        let swap = || -> io::Result<()> {
            if exists(archive) {
                std::fs::rename(archive, &staged)?;
            }
            if let Err(e) = std::fs::rename(edited, archive) {
                try_restore(&staged, archive);
                return Err(e);
            }
            Ok(())
        };

        match swap() {
            Ok(()) => {
                let mut outcome = ArchiveEditOutcome::new(
                    archive,
                    exists(&staged).then_some(staged),
                    0,
                    None,
                    false,
                );
                outcome.original = before;
                outcome.edited = EntryStamp::of(archive);
                outcome
            }
            Err(e) => refuse(format!(
                "The edit could not be taken back in: {e}. It is at {edited}."
            )),
        }
    }

    /// Erases a staged original, once the edit can no longer be undone: when the undo history
    /// releases it, or the session ends. Named the way this type names them, or it is not ours to
    /// erase: the guard every recursive delete in this codebase carries, applied to a single file.
    pub fn commit_staging(outcome: &ArchiveEditOutcome) {
        let Some(staged) = outcome.staged_original.as_deref() else {
            return;
        };
        if !file_name(staged).contains(Self::REPLACED_MARKER) {
            return;
        }
        try_delete(staged);
    }

    /// Erases a held edited container, once the undone edit can no longer be redone.
    pub fn commit_undo_staging(undo: &ArchiveEditUndo) {
        let Some(edited) = undo.staged_edited.as_deref() else {
            return;
        };
        if !file_name(edited).contains(Self::EDITED_MARKER) {
            return;
        }
        try_delete(edited);
    }

    /// Streams the old container into a new one, applying the edits on the way through.
    fn build_replacement(
        &self,
        plan: &ArchiveEditPlan,
        rewrite: &str,
        run: &mut ProgressCoalescer<'_, '_>,
        cancel: &CancelToken,
    ) -> Result<i32, Stop> {
        let format = ArchiveFormats::match_name(file_name(&plan.archive_file))
            .ok_or_else(|| Stop::Failed("That is not an archive this app can read.".to_string()))?;
        let mut written = 0i32;

        cancel.check().map_err(|_| Stop::Cancelled)?;
        let mut writer =
            ArchiveWriter::create(rewrite, Self::type_for(format), Self::options_for(format))?;

        // Read whole, like the verification read: a cancel lands between entries, below.
        let index = self
            .reader
            .read(&plan.archive_file, None, &CancelToken::none());
        if !index.ok() {
            return Err(Stop::Failed(
                index
                    .error
                    .clone()
                    .unwrap_or_else(|| "The archive could not be read.".to_string()),
            ));
        }
        let keep: Vec<String> = index
            .nodes()
            .filter(|n| !n.is_directory && !plan.removals.contains(&fold_upper(&n.path)))
            .map(|n| n.path.clone())
            .collect();

        if !keep.is_empty() {
            let mut write_failure: Option<io::Error> = None;
            let mut on_entry =
                |entry_path: &str, content: &mut dyn Read, size: i64| -> Result<(), Cancelled> {
                    cancel.check()?;

                    let name = plan
                        .renames
                        .get(&fold_upper(entry_path))
                        .map(String::as_str)
                        .unwrap_or(entry_path);
                    run.begin_item(file_name(name));
                    run.begin_file(size);

                    // Buffered because the writers want the length up front, and an entry stream out
                    // of a compressed container is forward-only. Bounded by the refusals in the
                    // planner rather than by hope.
                    let buffer = match buffered(content, size, cancel) {
                        Ok(b) => b,
                        Err(Buffered::Cancelled) => return Err(Cancelled),
                        Err(Buffered::Io(e)) => {
                            write_failure = Some(e);
                            return Err(Cancelled);
                        }
                    };
                    let modified = index.find(entry_path).and_then(|n| n.modified);
                    let length = buffer.len() as u64;
                    if let Err(e) = writer.write(
                        &name.replace('\\', "/"),
                        &mut Cursor::new(buffer),
                        length,
                        modified,
                    ) {
                        write_failure = Some(e);
                        return Err(Cancelled);
                    }

                    run.end_file();
                    run.end_item();
                    written += 1;
                    Ok(())
                };

            let pass =
                self.reader
                    .read_entries(&plan.archive_file, &keep, None, &mut on_entry, cancel);
            if let Some(e) = write_failure {
                return Err(e.into());
            }
            match pass {
                Ok(()) => {}
                Err(ArchiveReadError::Cancelled) => return Err(Stop::Cancelled),
                Err(ArchiveReadError::Failed(m)) => return Err(Stop::Failed(m)),
            }
        }

        for add in &plan.additions {
            cancel.check().map_err(|_| Stop::Cancelled)?;
            run.begin_item(file_name(&add.source_path));

            let mut source = OpenOptions::new()
                .read(true)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                .open(&add.source_path)?;
            let meta = source.metadata()?;
            let modified = meta.modified().ok().map(FileTime::from_system_time);

            run.begin_file(meta.len() as i64);
            writer.write(
                &add.entry_path.replace('\\', "/"),
                &mut source,
                meta.len(),
                modified,
            )?;

            run.end_file();
            run.end_item();
            written += 1;
        }

        writer.finish()?;
        Ok(written)
    }

    /// A working name beside the archive, so moving between them is a rename rather than a copy.
    /// **The marker goes before the suffix, not after it.** `a.zip.bertbrowser-rewrite-1234` is
    /// not a name any archive suffix matches, so the read-back that verifies the rewrite would
    /// fail every time. `a.bertbrowser-rewrite-1234.zip` is still a zip.
    fn beside(archive: &str, marker: &str) -> String {
        let (directory, name) = split_last(archive);
        let suffix = match ArchiveFormats::match_name(name) {
            Some(format) => format.suffix.to_string(),
            None => match name.rfind('.') {
                Some(dot) => name[dot..].to_string(),
                None => String::new(),
            },
        };
        let stem = ExtractRules::stem_of(archive);
        let nonce = RandomState::new().hash_one(0u8) as u32;
        let working = format!("{stem}{marker}{nonce:08x}{suffix}");
        if directory.is_empty() {
            working
        } else {
            format!("{directory}\\{working}")
        }
    }

    fn type_for(format: &ArchiveFormat) -> ArchiveType {
        if format.container == ArchiveContainer::Zip {
            ArchiveType::Zip
        } else {
            ArchiveType::Tar
        }
    }

    /// The compression the container already used, so a rewrite does not silently change what
    /// kind of file it is.
    fn options_for(format: &ArchiveFormat) -> WriterOptions {
        WriterOptions::new(match format.suffix {
            ".tar" => CompressionType::None,
            ".tar.gz" | ".tgz" => CompressionType::GZip,
            ".tar.bz2" | ".tbz2" => CompressionType::BZip2,
            _ => CompressionType::Deflate,
        })
    }
}

enum Buffered {
    Cancelled,
    Io(io::Error),
}

fn buffered(content: &mut dyn Read, size: i64, cancel: &CancelToken) -> Result<Vec<u8>, Buffered> {
    let mut buffer = Vec::with_capacity(size.clamp(0, 16 << 20) as usize);
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        let read = content.read(&mut chunk).map_err(Buffered::Io)?;
        if read == 0 {
            return Ok(buffer);
        }
        if cancel.is_cancelled() {
            return Err(Buffered::Cancelled);
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
}

fn exists(path: &str) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

fn try_delete(path: &str) {
    if std::path::Path::new(path).is_file() {
        let _ = std::fs::remove_file(path);
    }
}

fn try_restore(staged: &str, archive: &str) {
    if exists(staged) && !exists(archive) {
        let _ = std::fs::rename(staged, archive);
    }
}
