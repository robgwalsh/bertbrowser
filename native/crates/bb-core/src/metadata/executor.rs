//! Carries out a `MetadataEditPlan` by rewriting each file beside itself, checking the rewrite,
//! and swapping it in. Spec: Core/Services/Metadata/MetadataEditExecutor.cs.
//!
//! **The order is the safety**, as it is for an archive edit. Write a sibling; read it back and
//! require that it holds what was asked for *and* that everything which is not metadata is
//! byte-for-byte what the original had; only then swap. Until the swap the file on disk is the
//! one the user started with, so a cancel, a crash or a codec bug costs a temporary file.
//!
//! **The original is held, not deleted**, under `REPLACED_MARKER`, until the undo history retires
//! the edit. An undo swaps it back and holds the edited file in turn, under `EDITED_MARKER`. Both
//! directions are the same swap, which is why there is one `swap` and not an undo and a redo.
//!
//! One file's failure never affects the others, and nothing here is retried elevated: a tag
//! parser reads bytes somebody else wrote, which is not something to do with an administrator's
//! token.

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{self, Seek, SeekFrom};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};

use bb_paths::file_name;
use windows_sys::Win32::Storage::FileSystem::SetFileAttributesW;

use super::codec::{MetadataCodec, MetadataError, MetadataResult, Source, copy_to_end, identical};
use super::codecs::MetadataCodecs;
use super::fields::{MetadataField, MetadataFields};
use super::models::{
    FailedMetadataEdit, HeldVersion, MetadataEdit, MetadataEditOutcome, MetadataEditPlan,
    MetadataSwapResult, PlannedMetadataEdit,
};
use super::planner::{LOCAL_PROBE, MetadataEditPlanner, MetadataProbe};
use super::property_fallback::PropertyFallback;
use crate::attributes::HIDDEN;
use crate::cancel::CancelToken;
use crate::fs::{AccessDenied, EntryStamp, ReadOnlyFile, StagedReplace};
use crate::transfer::{PauseGate, ProgressCoalescer, TransferProgress};

/// Marks a file being rewritten, before it is swapped in.
pub const REWRITE_MARKER: &str = ".bertbrowser-rewrite-";

/// Marks a replaced original while the edit can still be undone.
pub const REPLACED_MARKER: &str = ".bertbrowser-replaced-";

/// Marks an edited file an undo set aside, while it can still be redone.
pub const EDITED_MARKER: &str = ".bertbrowser-edited-";

pub struct MetadataEditExecutor<'a> {
    probe: &'a dyn MetadataProbe,
    fallback: Option<&'a dyn PropertyFallback>,
}

impl<'a> MetadataEditExecutor<'a> {
    pub fn new(
        probe: &'a dyn MetadataProbe,
        fallback: Option<&'a dyn PropertyFallback>,
    ) -> MetadataEditExecutor<'a> {
        MetadataEditExecutor { probe, fallback }
    }

    /// An executor over the real disk, with no Windows fallback.
    pub fn local() -> MetadataEditExecutor<'static> {
        MetadataEditExecutor {
            probe: &LOCAL_PROBE,
            fallback: None,
        }
    }

    pub fn execute(
        &self,
        plan: &MetadataEditPlan,
        cancel: &CancelToken,
        progress: Option<&mut dyn FnMut(TransferProgress)>,
        pause: Option<&PauseGate>,
    ) -> MetadataEditOutcome {
        if !plan.has_work() {
            return MetadataEditOutcome::empty();
        }

        let mut run = ProgressCoalescer::new(cancel.clone(), progress, plan.files.len(), pause);
        let mut edited = Vec::new();
        let mut failed = Vec::new();
        let mut unchanged = Vec::new();
        let mut cancelled = false;

        for file in &plan.files {
            if cancel.is_cancelled() {
                cancelled = true;
                break;
            }

            run.begin_item(file_name(&file.path));
            run.begin_file(file.length);

            let rewrite = StagedReplace::beside(&file.path, REWRITE_MARKER);
            let result = if file.via_windows {
                self.edit_via_windows(file, &rewrite, cancel)
            } else {
                self.edit_one(file, &rewrite, cancel)
            };
            match result {
                Ok(Some(held)) => edited.push(held),
                Ok(None) => unchanged.push(file.path.clone()),
                Err(MetadataError::Cancelled) => {
                    try_delete(&rewrite);
                    cancelled = true;
                    break;
                }
                Err(MetadataError::Format(message)) => {
                    try_delete(&rewrite);
                    failed.push(FailedMetadataEdit::new(&file.path, message));
                }
                Err(MetadataError::Io(e)) => {
                    try_delete(&rewrite);
                    failed.push(FailedMetadataEdit {
                        path: file.path.clone(),
                        message: format!("{} could not be changed: {e}", file_name(&file.path)),
                        access_denied: AccessDenied::caused(Some(&e)),
                    });
                }
            }

            run.end_file();
            run.end_item();
        }

        run.finished();
        MetadataEditOutcome {
            edited,
            failed,
            cancelled,
            unchanged,
        }
    }

    /// Puts the originals back, holding each edited file so the edit can be redone.
    pub fn undo(&self, edited: &[HeldVersion]) -> MetadataSwapResult {
        swap(edited, EDITED_MARKER)
    }

    /// Takes undone edits back in, holding each original again.
    pub fn redo(&self, restored: &[HeldVersion]) -> MetadataSwapResult {
        swap(restored, REPLACED_MARKER)
    }

    /// Erases held versions once the step that needs them can no longer be taken — when the undo
    /// history releases the entry, or the session ends.
    pub fn commit_staging(held: &[HeldVersion]) {
        for version in held {
            // Named the way this type names them, or it is not ours to erase.
            if !StagedReplace::is_marked(&version.held_path, REPLACED_MARKER)
                && !StagedReplace::is_marked(&version.held_path, EDITED_MARKER)
            {
                continue;
            }
            try_delete(&version.held_path);
        }
    }

    /// The file with its original held, or None when the edit turned out to change nothing — a
    /// location removed from a picture that had none.
    fn edit_one(
        &self,
        file: &PlannedMetadataEdit,
        rewrite: &str,
        cancel: &CancelToken,
    ) -> MetadataResult<Option<HeldVersion>> {
        let path = &file.path;
        let name = file_name(path);

        // The planner decided this file could be edited; disk is the authority.
        if let Some(refusal) =
            MetadataEditPlanner::refusal(path, self.probe.attributes_of(path), false)
        {
            return Err(MetadataError::Format(refusal.message));
        }

        let codec = MetadataCodecs::for_path(path).expect("handled, so a codec exists");
        let original = EntryStamp::of(path).ok_or_else(|| gone(path))?;

        let mut source = ReadOnlyFile::try_open(path, false).ok_or_else(could_not_open)?;
        let mut destination = create_new(rewrite)?;

        let before = codec.payload_digest(&mut source)?;
        let edit = resolve(&file.edit, codec.as_ref(), &mut source, path)?;

        source.seek(SeekFrom::Start(0))?;
        codec.write(&mut source, &mut destination, &edit, cancel)?;
        destination.sync_data()?;

        // Verified before anything irreversible happens.
        destination.seek(SeekFrom::Start(0))?;
        let written = codec.read(&mut destination)?;
        for (field, value) in &edit.changes {
            if written.get(*field) != value {
                return Err(MetadataError::format(format!(
                    "{} did not read back as it was written, so {name} was left as it is.",
                    MetadataFields::get(*field).label
                )));
            }
        }

        if let Some(picture) = &edit.set_picture
            && written.picture.as_deref() != Some(picture.as_slice())
        {
            return Err(MetadataError::format(format!(
                "The picture did not read back as it was written, so {name} was left as it is."
            )));
        }

        if edit.remove_picture && edit.set_picture.is_none() && written.picture.is_some() {
            return Err(MetadataError::format(format!(
                "The picture could not be removed from {name}, so it was left as it is."
            )));
        }

        // A removal is a promise, and the only honest way to keep one is to look afterwards.
        if (edit.remove_location || edit.remove_all) && written.has_location {
            return Err(MetadataError::format(format!(
                "The location could not be removed from {name}, so it was left as it is."
            )));
        }

        if edit.remove_all && written.values.keys().any(|k| !edit.changes.contains_key(k)) {
            return Err(MetadataError::format(format!(
                "Not everything could be removed from {name}, so it was left as it is."
            )));
        }

        destination.seek(SeekFrom::Start(0))?;
        if codec.payload_digest(&mut destination)? != before {
            return Err(MetadataError::format(format!(
                "Rewriting {name} would have changed more than its details, so it was left as it is."
            )));
        }

        // Nothing to swap, nothing to hold, and no history entry for a change that was not one.
        let same = identical(&mut source, &mut destination)?;
        drop(source);
        drop(destination);

        if same {
            try_delete(rewrite);
            return Ok(None);
        }
        swap_in(path, rewrite, &original, cancel).map(Some)
    }

    /// The same sequence with Windows' property handler doing the writing, for a file no codec
    /// here reads. The handler writes in place, so it is given a **copy** to write in: until the
    /// swap the user's file has not been opened for writing by anything. The fields are read
    /// back from the copy like any other rewrite's. What cannot be done is the payload check —
    /// there is no reading of this format to hash — which is the whole difference, and why
    /// `PlannedMetadataEdit::via_windows` is carried to the pane rather than hidden.
    fn edit_via_windows(
        &self,
        file: &PlannedMetadataEdit,
        rewrite: &str,
        cancel: &CancelToken,
    ) -> MetadataResult<Option<HeldVersion>> {
        let path = &file.path;
        let name = file_name(path);

        if let Some(refusal) =
            MetadataEditPlanner::refusal(path, self.probe.attributes_of(path), true)
        {
            return Err(MetadataError::Format(refusal.message));
        }

        let fallback = self
            .fallback
            .ok_or_else(|| MetadataError::format(format!("Nothing here can write to {name}.")))?;
        let original = EntryStamp::of(path).ok_or_else(|| gone(path))?;

        {
            let mut source = ReadOnlyFile::try_open(path, false).ok_or_else(could_not_open)?;
            let mut destination = create_new(rewrite)?;
            copy_to_end(&mut source, &mut destination, 0, cancel)?;
            destination.sync_data()?;
        }

        fallback.write(rewrite, &file.edit.changes)?;

        let written = fallback.read(rewrite).ok_or_else(|| {
            MetadataError::format(format!(
                "Windows could not read {name} back after writing it, so it was left as it is."
            ))
        })?;
        for (field, value) in &file.edit.changes {
            if written.document.get(*field) != value {
                return Err(MetadataError::format(format!(
                    "{} did not read back as it was written, so {name} was left as it is.",
                    MetadataFields::get(*field).label
                )));
            }
        }

        let same = {
            let mut source = ReadOnlyFile::try_open(path, false).ok_or_else(could_not_open)?;
            let mut destination =
                ReadOnlyFile::try_open(rewrite, false).ok_or_else(could_not_open)?;
            identical(&mut source, &mut destination)?
        };

        if same {
            try_delete(rewrite);
            return Ok(None);
        }
        swap_in(path, rewrite, &original, cancel).map(Some)
    }
}

fn could_not_open() -> MetadataError {
    MetadataError::Io(io::Error::other("it could not be opened."))
}

fn gone(path: &str) -> MetadataError {
    MetadataError::Io(io::Error::other(format!(
        "{} is no longer there.",
        file_name(path)
    )))
}

/// A fresh, exclusive, read-write file for the rewrite.
fn create_new(path: &str) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .share_mode(0)
        .open(path)
}

fn swap_in(
    path: &str,
    rewrite: &str,
    original: &EntryStamp,
    cancel: &CancelToken,
) -> MetadataResult<HeldVersion> {
    cancel.check()?;

    // Somebody saved over it while the rewrite was being built: theirs is newer than ours.
    if !original.matches(path) {
        return Err(MetadataError::format(format!(
            "{} changed while it was being edited, so it was left as it is.",
            file_name(path)
        )));
    }

    let staged = StagedReplace::beside(path, REPLACED_MARKER);
    StagedReplace::swap(rewrite, path, &staged)?;
    hide(&staged);

    Ok(HeldVersion {
        path: path.to_string(),
        held_path: staged,
        in_place: EntryStamp::of(path).ok_or_else(|| gone(path))?,
    })
}

/// Turns a relative change into the absolute one this file needs, so a codec only ever sets
/// values and the read-back has something exact to compare against.
fn resolve(
    edit: &MetadataEdit,
    codec: &dyn MetadataCodec,
    source: &mut dyn Source,
    path: &str,
) -> MetadataResult<MetadataEdit> {
    let Some(shift) = edit.shift_date_taken else {
        return Ok(edit.clone());
    };

    source.seek(SeekFrom::Start(0))?;
    let taken = codec
        .read(source)?
        .get(MetadataField::DateTaken)
        .to_string();
    let date = MetadataFields::try_parse_date(&taken).ok_or_else(|| {
        MetadataError::format(format!("{} has no date taken to move.", file_name(path)))
    })?;

    let moved = date
        .0
        .checked_add(shift)
        .map(crate::FileTime)
        .filter(|m| (1..=9999).contains(&m.civil().year))
        .ok_or_else(|| {
            MetadataError::format(format!(
                "That would move the date of {} out of range.",
                file_name(path)
            ))
        })?;

    let mut changes: BTreeMap<MetadataField, String> = edit.changes.clone();
    changes.insert(MetadataField::DateTaken, MetadataFields::format_date(moved));

    Ok(MetadataEdit {
        changes,
        shift_date_taken: None,
        ..edit.clone()
    })
}

/// A held version sits beside its file, where it can be found again after a crash — and so it is
/// hidden, or retagging an album would double the folder until the history let go. Safe to do
/// because the swap takes its attributes from the file being replaced, never from the one coming
/// in: a held copy that was hidden here does not come back hidden.
fn hide(held: &str) {
    let Ok(meta) = std::fs::symlink_metadata(held) else {
        return;
    };
    let wide: Vec<u16> = held.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: a NUL-terminated path, valid for the call. Visible is untidy, not wrong, so the
    // result is not checked.
    unsafe {
        SetFileAttributesW(wide.as_ptr(), meta.file_attributes() | HIDDEN);
    }
}

/// Uncancellable and silent on purpose, the same as every other staging move here: a cancel
/// landing half-way through putting files back would leave some edited and some not, with no
/// record that says which.
fn swap(versions: &[HeldVersion], aside_marker: &str) -> MetadataSwapResult {
    let mut swapped = Vec::new();
    let mut failed = Vec::new();

    for version in versions {
        let name = file_name(&version.path);

        if !std::fs::symlink_metadata(&version.held_path).is_ok_and(|m| !m.is_dir()) {
            failed.push(FailedMetadataEdit::new(
                &version.path,
                format!("The other version of {name} is no longer being held."),
            ));
            continue;
        }

        // The history can reach an edit many operations later. A file changed by anything else
        // since is somebody else's work, and swapping it out would undo that instead.
        if !version.in_place.matches(&version.path) {
            failed.push(FailedMetadataEdit::new(
                &version.path,
                format!(
                    "{name} has changed since, so it was left as it is. Its other version is at {}.",
                    version.held_path
                ),
            ));
            continue;
        }

        let aside = StagedReplace::beside(&version.path, aside_marker);
        match StagedReplace::swap(&version.held_path, &version.path, &aside) {
            Ok(()) => {
                hide(&aside);
                match EntryStamp::of(&version.path) {
                    Some(in_place) => swapped.push(HeldVersion {
                        path: version.path.clone(),
                        held_path: aside,
                        in_place,
                    }),
                    None => failed.push(FailedMetadataEdit::new(
                        &version.path,
                        format!("{name} is no longer there."),
                    )),
                }
            }
            Err(e) => failed.push(FailedMetadataEdit {
                path: version.path.clone(),
                message: format!(
                    "{name} could not be put back: {e} Its other version is at {}.",
                    version.held_path
                ),
                access_denied: AccessDenied::caused(Some(&e)),
            }),
        }
    }

    MetadataSwapResult { swapped, failed }
}

fn try_delete(path: &str) {
    if std::fs::symlink_metadata(path).is_ok() {
        let _ = std::fs::remove_file(path);
    }
}
