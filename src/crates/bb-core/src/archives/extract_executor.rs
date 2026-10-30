//! Carries out an `ExtractPlan`. Spec: Core/Services/Archives/ExtractExecutor.cs.
//!
//! **A cancelled extract adds nothing**, which is the cancelled-copy rule, but the mechanism has to
//! be stricter than a copy's. A copy owns its whole destination tree; an extract routinely lands
//! *into a folder the user already has files in*, so removing the tree would delete their work.
//! Instead this records exactly what it created, in creation order, and on a cancel removes only
//! those, in reverse, and only while still empty. A skipped conflict was never created and is
//! never removed. **One pass over the container**, through `ArchiveReader::read_entries`: a solid
//! archive is decompressed once for the whole selection rather than once per file.

use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::{self, Read, Write};

use bb_paths::{file_name, fold_upper, split_last};

use super::{
    ArchiveReadError, ArchiveReader, ExtractOutcome, ExtractPlan, ExtractProbe, FailedExtraction,
    FileSystemExtractProbe, PlannedExtraction,
};
use crate::cancel::{CancelToken, Cancelled};
use crate::transfer::{PauseGate, ProgressCoalescer, TransferProgress};

pub struct ExtractExecutor<'r, P: ExtractProbe = FileSystemExtractProbe> {
    reader: &'r dyn ArchiveReader,
    probe: P,
}

enum WriteStop {
    Cancelled,
    Io(io::Error),
}

impl<'r> ExtractExecutor<'r, FileSystemExtractProbe> {
    pub fn new(reader: &'r dyn ArchiveReader) -> ExtractExecutor<'r, FileSystemExtractProbe> {
        ExtractExecutor::with_probe(reader, FileSystemExtractProbe)
    }
}

impl<'r, P: ExtractProbe> ExtractExecutor<'r, P> {
    pub fn with_probe(reader: &'r dyn ArchiveReader, probe: P) -> ExtractExecutor<'r, P> {
        ExtractExecutor { reader, probe }
    }

    pub fn execute(
        &self,
        plan: &ExtractPlan,
        password: Option<&str>,
        cancel: &CancelToken,
        progress: Option<&mut dyn FnMut(TransferProgress)>,
        pause: Option<&PauseGate>,
    ) -> ExtractOutcome {
        if !plan.has_work() {
            return ExtractOutcome::nothing();
        }

        let files: Vec<&PlannedExtraction> =
            plan.items.iter().filter(|i| !i.is_directory).collect();
        let mut run = ProgressCoalescer::new(cancel.clone(), progress, files.len(), pause);

        // Everything this call brings into being, in the order it did, so a cancel can undo
        // exactly that much and nothing of what was already there.
        let mut created: Vec<String> = Vec::new();
        let mut created_files: Vec<String> = Vec::new();
        let mut failed: Vec<FailedExtraction> = Vec::new();
        let mut written = 0i32;
        let mut bytes = 0i64;
        let mut cancelled = false;

        let outcome: Result<(), ArchiveReadError> = (|| {
            for dir in plan.items.iter().filter(|i| i.is_directory) {
                cancel.check().map_err(|_| ArchiveReadError::Cancelled)?;
                self.create_directories(&dir.destination_path, &mut created)
                    .map_err(|e| ArchiveReadError::Failed(e.to_string()))?;
            }

            let by_entry: HashMap<String, &PlannedExtraction> = files
                .iter()
                .map(|f| (fold_upper(&f.entry_path), *f))
                .collect();
            let wanted: Vec<String> = files.iter().map(|f| f.entry_path.clone()).collect();

            let mut on_entry =
                |entry_path: &str, content: &mut dyn Read, size: i64| -> Result<(), Cancelled> {
                    let Some(item) = by_entry.get(&fold_upper(entry_path)) else {
                        return Ok(());
                    };

                    run.begin_item(file_name(&item.destination_path));

                    // Re-checked against live disk, because the plan was built while a dialog was
                    // open. Create-new rather than a check-then-create: it fails on a taken path,
                    // which closes the window between the two.
                    let (parent, _) = split_last(&item.destination_path);
                    if !parent.is_empty()
                        && let Err(e) = self.create_directories(parent, &mut created)
                    {
                        failed.push(FailedExtraction {
                            entry_path: entry_path.to_string(),
                            message: e.to_string(),
                        });
                        return Ok(());
                    }

                    run.begin_file(size);
                    match write_entry(content, &item.destination_path, size, &mut run) {
                        Ok(copied) => {
                            created_files.push(item.destination_path.clone());
                            created.push(item.destination_path.clone());
                            bytes += copied;
                            written += 1;
                        }
                        Err(WriteStop::Cancelled) => {
                            // The partial file is this call's own doing, so it goes back, but the
                            // entries already finished stay, exactly as a cancelled transfer keeps
                            // what got across.
                            try_delete(&item.destination_path);
                            return Err(Cancelled);
                        }
                        Err(WriteStop::Io(e)) => {
                            try_delete(&item.destination_path);
                            failed.push(FailedExtraction {
                                entry_path: entry_path.to_string(),
                                message: e.to_string(),
                            });
                        }
                    }
                    run.end_file();
                    run.end_item();
                    Ok(())
                };

            self.reader
                .read_entries(&plan.archive_file, &wanted, password, &mut on_entry, cancel)
        })();

        match outcome {
            Ok(()) => {}
            Err(ArchiveReadError::Cancelled) => {
                cancelled = true;
                remove_what_this_call_created(&created, &created_files);
            }
            Err(ArchiveReadError::Failed(message)) => {
                // The container itself failed part-way. What reached disk is real and stays; the
                // outcome says the rest did not.
                failed.push(FailedExtraction {
                    entry_path: plan.archive_file.clone(),
                    message,
                });
            }
        }

        run.finished();
        ExtractOutcome {
            files_written: written,
            bytes_written: bytes,
            failed,
            cancelled,
        }
    }

    /// Creates a directory and any missing ancestors, recording each one this call actually
    /// brought into being. An ancestor that was already there is not recorded, so a cancel never
    /// removes a folder the user had.
    fn create_directories(&self, path: &str, created: &mut Vec<String>) -> io::Result<()> {
        if self.probe.directory_exists(path) {
            return Ok(());
        }

        let mut stack: Vec<String> = Vec::new();
        let mut current = path.to_string();
        while !current.is_empty() && !self.probe.directory_exists(&current) {
            stack.push(current.clone());
            let (parent, _) = split_last(&current);
            if parent.is_empty() || parent == current {
                break;
            }
            current = parent.to_string();
        }

        while let Some(next) = stack.pop() {
            if std::path::Path::new(&next).is_dir() {
                continue;
            }
            std::fs::create_dir(&next)?;
            created.push(next);
        }
        Ok(())
    }
}

/// Copies one entry to disk in bounded chunks, reporting bytes and honouring a cancel between
/// them: the shape the file copier has, without a source path for the OS to copy from.
fn write_entry(
    content: &mut dyn Read,
    destination: &str,
    size: i64,
    run: &mut ProgressCoalescer<'_, '_>,
) -> Result<i64, WriteStop> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(WriteStop::Io)?;

    let mut buffer = vec![0u8; 64 * 1024];
    let mut total = 0i64;
    loop {
        let read = content.read(&mut buffer).map_err(WriteStop::Io)?;
        if read == 0 {
            break;
        }
        if run.token().is_cancelled() {
            return Err(WriteStop::Cancelled);
        }
        file.write_all(&buffer[..read]).map_err(WriteStop::Io)?;
        total += read as i64;
        run.file_progress(total, size.max(total));
    }
    file.flush().map_err(WriteStop::Io)?;
    Ok(total)
}

/// Undoes exactly this call's additions, newest first. Files it wrote go; directories go only if
/// still empty, which is what keeps a folder the user had, or had put something else into while
/// this ran, from being removed.
fn remove_what_this_call_created(created: &[String], created_files: &[String]) {
    let file_set: HashSet<String> = created_files.iter().map(|f| fold_upper(f)).collect();

    for path in created.iter().rev() {
        if file_set.contains(&fold_upper(path)) {
            try_delete(path);
            continue;
        }
        // Never recursive: this must not be able to reach anything it did not put there.
        if let Ok(mut entries) = std::fs::read_dir(path)
            && entries.next().is_none()
        {
            let _ = std::fs::remove_dir(path);
        }
    }
}

fn try_delete(path: &str) {
    if std::path::Path::new(path).is_file() {
        let _ = std::fs::remove_file(path);
    }
}
