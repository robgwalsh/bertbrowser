//! Decides which files of a selection an edit can be written to, and says why not for the rest.
//! Pure: every question about disk goes through `MetadataProbe`.
//! Spec: Core/Services/Metadata/MetadataEditPlanner.cs.

use std::collections::{BTreeMap, HashSet};
use std::os::windows::fs::MetadataExt;

use bb_paths::{file_name, fold_upper};

use super::codecs::{MetadataCodecs, extension_of};
use super::fields::{MetadataFamily, MetadataField};
use super::models::{
    MetadataEdit, MetadataEditPlan, MetadataRejection, PlannedMetadataEdit, RejectedMetadataEdit,
};
use crate::archives::ArchivePath;
use crate::attributes::{DIRECTORY, READONLY, REPARSE_POINT};
use crate::fs::ReadOnlyFile;
use crate::rename::paths::directory_name;

/// What the planner needs to know about disk, so it can be tested without one.
pub trait MetadataProbe {
    /// The entry's attributes, or None when nothing is there.
    fn attributes_of(&self, path: &str) -> Option<u32>;

    fn length_of(&self, path: &str) -> i64;
}

pub struct FileSystemMetadataProbe;

impl MetadataProbe for FileSystemMetadataProbe {
    fn attributes_of(&self, path: &str) -> Option<u32> {
        std::fs::symlink_metadata(path)
            .ok()
            .map(|m| m.file_attributes())
    }

    fn length_of(&self, path: &str) -> i64 {
        std::fs::symlink_metadata(path)
            .ok()
            .map(|m| m.file_size() as i64)
            .unwrap_or(0)
    }
}

pub static LOCAL_PROBE: FileSystemMetadataProbe = FileSystemMetadataProbe;

/// For files no codec here writes: the fields Windows' own property handler said it would take,
/// by path (see `PropertyFallback`).
pub type OfferedByWindows = BTreeMap<String, Vec<MetadataField>>;

pub struct MetadataEditPlanner<'a> {
    probe: &'a dyn MetadataProbe,
}

impl<'a> MetadataEditPlanner<'a> {
    pub fn new(probe: &'a dyn MetadataProbe) -> MetadataEditPlanner<'a> {
        MetadataEditPlanner { probe }
    }

    pub fn local() -> MetadataEditPlanner<'static> {
        MetadataEditPlanner {
            probe: &LOCAL_PROBE,
        }
    }

    /// `via_windows` is taken rather than asked for, so planning stays pure — asking is opening
    /// the file. Matched by path without regard to case.
    pub fn plan(
        &self,
        paths: &[String],
        edit: &MetadataEdit,
        via_windows: Option<&OfferedByWindows>,
    ) -> MetadataEditPlan {
        let mut files = Vec::new();
        let mut rejected = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();

        for path in paths {
            if !seen.insert(fold_upper(path)) {
                continue;
            }

            // A codec always wins: the fallback is for what nothing here can check.
            let offered = if MetadataCodecs::handles(path) {
                None
            } else {
                via_windows.and_then(|map| {
                    map.iter()
                        .find(|(p, _)| {
                            p.eq_ignore_ascii_case(path) || fold_upper(p) == fold_upper(path)
                        })
                        .map(|(_, fields)| fields)
                })
            };
            let windows = offered.is_some();

            if let Some(refusal) = Self::refusal(path, self.probe.attributes_of(path), windows) {
                rejected.push(refusal);
                continue;
            }

            if let Some(offered) = offered {
                // Fields only. A removal is a promise about the whole file, and a handler that
                // cannot be read behind is not something to make one through.
                let changes: BTreeMap<MetadataField, String> = edit
                    .changes
                    .iter()
                    .filter(|(f, _)| offered.contains(f))
                    .map(|(f, v)| (*f, v.clone()))
                    .collect();
                if changes.is_empty() {
                    rejected.push(RejectedMetadataEdit {
                        path: path.clone(),
                        reason: MetadataRejection::NothingApplies,
                        message: format!("Windows can change none of this in {}.", file_name(path)),
                    });
                    continue;
                }

                files.push(PlannedMetadataEdit {
                    path: path.clone(),
                    edit: MetadataEdit::none().with_changes(changes),
                    length: self.probe.length_of(path),
                    via_windows: true,
                });
                continue;
            }

            let codec = MetadataCodecs::for_path(path).expect("handled, so a codec exists");
            let applicable = edit.for_codec(codec.as_ref());
            if applicable.is_empty() {
                rejected.push(RejectedMetadataEdit {
                    path: path.clone(),
                    reason: MetadataRejection::NothingApplies,
                    message: format!(
                        "None of these fields exist for {}.",
                        describe(codec.family())
                    ),
                });
                continue;
            }

            files.push(PlannedMetadataEdit {
                path: path.clone(),
                edit: applicable,
                length: self.probe.length_of(path),
                via_windows: false,
            });
        }

        MetadataEditPlan { files, rejected }
    }

    /// Why this file cannot be edited, or None when it can. The executor asks again against live
    /// disk before it writes, because a plan describes the moment it was made. `via_windows`
    /// means the file is to be written by Windows' property handler rather than a codec, so
    /// having no codec is not a reason; every other refusal still is.
    pub fn refusal(
        path: &str,
        attributes: Option<u32>,
        via_windows: bool,
    ) -> Option<RejectedMetadataEdit> {
        let name = file_name(path);
        let no = |reason: MetadataRejection, message: String| {
            Some(RejectedMetadataEdit {
                path: path.to_string(),
                reason,
                message,
            })
        };

        // Asked first, and of the path alone: an entry inside an archive has no attributes to
        // probe, and looks like an ordinary missing file to anything that tries.
        if Self::is_inside_archive(path) {
            return no(
                MetadataRejection::InsideArchive,
                format!("{name} is inside an archive. Extract it to edit it."),
            );
        }

        let Some(found) = attributes else {
            return no(
                MetadataRejection::Missing,
                format!("{name} is no longer there."),
            );
        };

        if found & DIRECTORY != 0 {
            return no(MetadataRejection::IsFolder, format!("{name} is a folder."));
        }

        if found & REPARSE_POINT != 0 {
            return no(
                MetadataRejection::Link,
                format!("{name} is a link, and a link is not edited through."),
            );
        }

        if found & ReadOnlyFile::PLACEHOLDER != 0 {
            return no(
                MetadataRejection::CloudPlaceholder,
                format!("{name} is not downloaded to this PC, and editing it would download it."),
            );
        }

        if !via_windows && !MetadataCodecs::handles(path) {
            let extension = extension_of(path).trim_start_matches('.');
            return no(
                MetadataRejection::Unsupported,
                if extension.is_empty() {
                    "This kind of file cannot be edited.".to_string()
                } else {
                    format!("{} files cannot be edited.", extension.to_uppercase())
                },
            );
        }

        if found & READONLY != 0 {
            return no(MetadataRejection::ReadOnly, format!("{name} is read-only."));
        }

        None
    }

    /// Whether a path names an entry inside an archive. Asked of its folder, never of its own
    /// name: an EPUB or a .docx-shaped zip is an archive too, and standing on one is not being
    /// in it.
    pub fn is_inside_archive(path: &str) -> bool {
        directory_name(path).is_some_and(|dir| ArchivePath::looks_virtual(&dir))
    }
}

fn describe(family: MetadataFamily) -> &'static str {
    match family {
        MetadataFamily::Image => "a picture",
        MetadataFamily::Audio => "a song or video",
        MetadataFamily::Document => "a document",
    }
}
