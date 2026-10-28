//! Carries out a `NewItemPlan`. Spec: Core/Services/NewItem/NewItemExecutor.cs.
//!
//! Nothing is ever overwritten, and — the part worth stating separately — nothing existing is ever
//! *adopted*. A create-directory call that succeeds on a folder already there would hand the user
//! someone else's folder and report success, so existence is checked immediately before the call
//! rather than trusted from the plan, which was built while the dialog sat open.
//!
//! Files need no such care and do not get it: a create-new open and a copy that fails if the
//! target exists both refuse a taken path, which closes the window a check-then-create leaves open.

use std::io;

use windows_sys::Win32::Storage::FileSystem::{
    CopyFileW, GetFileAttributesW, INVALID_FILE_ATTRIBUTES, SetFileAttributesW,
};

use super::{FailedNewItem, NewItemKind, NewItemOutcome, NewItemPlan};
use crate::attributes::{HIDDEN, READONLY, SYSTEM};
use crate::fs::AccessDenied;
use crate::rename::paths::{exists, file_exists, wide};

pub struct NewItemExecutor;

impl NewItemExecutor {
    pub fn execute(&self, plan: &NewItemPlan) -> NewItemOutcome {
        if !plan.has_work() {
            return NewItemOutcome::empty();
        }

        let target = plan.target_path();
        let result = match plan.kind {
            NewItemKind::Folder => {
                // Re-checked against live disk, not taken from the plan — see above.
                if exists(&target) {
                    return already_there(plan);
                }
                std::fs::create_dir(&target)
            }
            NewItemKind::File => match plan.template_path.as_deref().filter(|t| !t.is_empty()) {
                Some(template) => {
                    if !file_exists(template) {
                        return NewItemOutcome {
                            created_path: None,
                            failed: Some(FailedNewItem::new(format!(
                                "The template for this file type is missing: {template}"
                            ))),
                        };
                    }
                    copy_no_replace(template, &target).map(|()| clear_inherited_attributes(&target))
                }
                // Creates and closes; the file is meant to be empty.
                None => std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&target)
                    .map(|_| ()),
            },
        };

        match result {
            Ok(()) => NewItemOutcome {
                created_path: Some(target),
                failed: None,
            },
            // The atomic creates report a taken path as a failure; say what actually happened
            // rather than passing on "the file already exists" from a layer down.
            Err(_) if exists(&target) => already_there(plan),
            Err(e) => NewItemOutcome {
                created_path: None,
                failed: Some(FailedNewItem {
                    message: format!("Could not create '{}' \u{2014} {e}", plan.name),
                    access_denied: AccessDenied::caused(Some(&e)),
                }),
            },
        }
    }
}

fn already_there(plan: &NewItemPlan) -> NewItemOutcome {
    NewItemOutcome {
        created_path: None,
        failed: Some(FailedNewItem::new(format!(
            "'{}' already exists in this folder.",
            plan.name
        ))),
    }
}

/// `File.Copy(overwrite: false)`: `CopyFile` failing if the target exists. (`std::fs::copy` would
/// replace a file in its way.)
fn copy_no_replace(from: &str, to: &str) -> io::Result<()> {
    // SAFETY: two NUL-terminated paths, valid for the call.
    let ok = unsafe { CopyFileW(wide(from).as_ptr(), wide(to).as_ptr(), 1) };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// A template kept under `%APPDATA%\Microsoft\Windows\Templates` is often marked Hidden or
/// ReadOnly; the file the user asked for should be neither. Best-effort — a copy that was made is
/// not a failure just because its attributes would not come off.
fn clear_inherited_attributes(path: &str) {
    let wide = wide(path);
    // SAFETY: a NUL-terminated path.
    let attributes = unsafe { GetFileAttributesW(wide.as_ptr()) };
    if attributes == INVALID_FILE_ATTRIBUTES {
        return;
    }
    let wanted = attributes & !(HIDDEN | READONLY | SYSTEM);
    if wanted != attributes {
        // SAFETY: a NUL-terminated path.
        unsafe { SetFileAttributesW(wide.as_ptr(), wanted) };
    }
}
