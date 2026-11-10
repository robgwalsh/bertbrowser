//! When each file verb is available. These rules used to exist only as lines setting `IsEnabled` on
//! menu items; they are here so the menu, a shortcut and the palette all get one answer. A keybinding
//! must not be able to route around a menu. Spec: Core/Services/Commands/FileVerbRules.cs.

use crate::format::thousands;

/// The things a file list's right-click menu offers, each of which is also a command.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum FileVerb {
    New,
    Open,
    RunAsAdmin,
    OpenInNewTab,
    OpenInNewPane,
    OpenInTerminal,
    OpenInVSCode,
    DiskUsage,
    Duplicates,
    Changes,
    ComparePanes,
    Compress,
    ExtractHere,
    ExtractTo,
    CopyPath,
    CopyName,
    CompareFiles,
    SettleByContent,
    VerifyChecksums,
    Checksum,
    Cut,
    Copy,
    Paste,
    Rename,
    Delete,
    DeletePermanently,
    Bookmark,
    Properties,
    UnlockArchive,
}

impl FileVerb {
    pub const ALL: [FileVerb; 29] = [
        FileVerb::New,
        FileVerb::Open,
        FileVerb::RunAsAdmin,
        FileVerb::OpenInNewTab,
        FileVerb::OpenInNewPane,
        FileVerb::OpenInTerminal,
        FileVerb::OpenInVSCode,
        FileVerb::DiskUsage,
        FileVerb::Duplicates,
        FileVerb::Changes,
        FileVerb::ComparePanes,
        FileVerb::Compress,
        FileVerb::ExtractHere,
        FileVerb::ExtractTo,
        FileVerb::CopyPath,
        FileVerb::CopyName,
        FileVerb::CompareFiles,
        FileVerb::SettleByContent,
        FileVerb::VerifyChecksums,
        FileVerb::Checksum,
        FileVerb::Cut,
        FileVerb::Copy,
        FileVerb::Paste,
        FileVerb::Rename,
        FileVerb::Delete,
        FileVerb::DeletePermanently,
        FileVerb::Bookmark,
        FileVerb::Properties,
        FileVerb::UnlockArchive,
    ];
}

/// What a file verb needs to know about the tab it would act on. Counts rather than items, so the
/// rules can be tested without a list; the two answers that cost something (the clipboard and the
/// registry) are asked for only by the verbs that need them.
pub struct FileVerbSnapshot<'a> {
    /// How many rows are selected.
    pub count: usize,
    /// How many of those are files rather than folders.
    pub files: usize,
    pub in_archive: bool,
    pub is_search_result: bool,
    /// The tab is showing somewhere with a path, not This PC.
    pub has_folder: bool,
    pub comparing: bool,
    /// Exactly one row is selected and it is an archive file.
    pub single_is_archive: bool,
    pub all_bookmarked: bool,
    pub archive_locked: bool,
    pub clipboard_has_files: Option<&'a dyn Fn() -> bool>,
    /// Whether the one selected item has anything to elevate.
    pub can_run_elevated: Option<&'a dyn Fn() -> bool>,
}

impl<'a> FileVerbSnapshot<'a> {
    pub fn new(
        count: usize,
        files: usize,
        in_archive: bool,
        is_search_result: bool,
        has_folder: bool,
    ) -> FileVerbSnapshot<'a> {
        FileVerbSnapshot {
            count,
            files,
            in_archive,
            is_search_result,
            has_folder,
            comparing: false,
            single_is_archive: false,
            all_bookmarked: false,
            archive_locked: false,
            clipboard_has_files: None,
            can_run_elevated: None,
        }
    }

    pub fn folders(&self) -> usize {
        self.count - self.files
    }
}

/// Whether a verb is offered, whether it can run, what it is called for this selection, and when it
/// cannot run, why, in words fit to show beside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileVerbState {
    pub visible: bool,
    pub enabled: bool,
    pub header: String,
    pub reason: Option<String>,
}

impl FileVerbState {
    pub fn on(header: impl Into<String>) -> FileVerbState {
        FileVerbState {
            visible: true,
            enabled: true,
            header: header.into(),
            reason: None,
        }
    }

    pub fn off(header: impl Into<String>, reason: impl Into<String>) -> FileVerbState {
        FileVerbState {
            visible: true,
            enabled: false,
            header: header.into(),
            reason: Some(reason.into()),
        }
    }

    fn hidden(header: impl Into<String>, reason: impl Into<String>) -> FileVerbState {
        FileVerbState {
            visible: false,
            enabled: false,
            header: header.into(),
            reason: Some(reason.into()),
        }
    }
}

const NOTHING_SELECTED: &str = "Nothing is selected.";
const INSIDE_ARCHIVE: &str = "Not available inside an archive.";

pub struct FileVerbRules;

impl FileVerbRules {
    pub fn for_verb(verb: FileVerb, s: &FileVerbSnapshot<'_>) -> FileVerbState {
        // Inside an archive nothing has a path an executor can act on, so everything that writes to
        // disk by path is off.
        match verb {
            FileVerb::Cut | FileVerb::Copy => Self::cut_or_copy(verb, s),
            FileVerb::Paste => Self::paste(s),
            FileVerb::CopyPath => selected(
                s,
                if s.count > 1 {
                    "Copy as paths"
                } else {
                    "Copy as path"
                },
            ),
            FileVerb::CopyName => selected(
                s,
                if s.count > 1 {
                    "Copy names"
                } else {
                    "Copy name"
                },
            ),
            FileVerb::Checksum => Self::checksum(s),
            FileVerb::VerifyChecksums => Self::verify_checksums(s),
            FileVerb::CompareFiles => Self::compare_files(s),
            FileVerb::SettleByContent => Self::settle_by_content(s),
            FileVerb::Open => selected(s, "Open"),
            FileVerb::OpenInNewTab => Self::open_in_new_tab(s),
            FileVerb::OpenInNewPane => {
                if s.folders() > 0 {
                    FileVerbState::on("Open in new pane")
                } else {
                    FileVerbState::off("Open in new pane", "Select a folder.")
                }
            }
            FileVerb::OpenInTerminal | FileVerb::OpenInVSCode => Self::open_in_program(verb, s),
            // One folder, since the view analyses a single root; with nothing selected, the folder
            // being shown. Every size inside a container is already exact, so it stays on there.
            FileVerb::DiskUsage => one_folder_or_none(s, "Analyse disk usage\u{2026}"),
            // Duplicates reads whole files by path, which an entry does not have.
            FileVerb::Duplicates => {
                if s.in_archive {
                    FileVerbState::off("Find duplicates\u{2026}", INSIDE_ARCHIVE)
                } else {
                    one_folder_or_none(s, "Find duplicates\u{2026}")
                }
            }
            // The change log is keyed by real paths.
            FileVerb::Changes => {
                if s.in_archive {
                    FileVerbState::off("What changed here\u{2026}", INSIDE_ARCHIVE)
                } else {
                    one_folder_or_none(s, "What changed here\u{2026}")
                }
            }
            // Not keyed on the selection, and never disabled: pressing it is how you find out whether
            // there are two panes. The header flips because pressing it again stops the comparison.
            FileVerb::ComparePanes => FileVerbState::on(if s.comparing {
                "Stop comparing"
            } else {
                "Compare with other pane"
            }),
            FileVerb::ExtractHere | FileVerb::ExtractTo => Self::extract(verb, s),
            FileVerb::Compress => Self::compress(s),
            FileVerb::RunAsAdmin => Self::run_as_admin(s),
            FileVerb::Rename => Self::rename(s),
            FileVerb::Delete => {
                let header = if s.count > 1 {
                    format!("Delete {} items\u{2026}", s.count)
                } else {
                    "Delete\u{2026}".to_string()
                };
                selected(s, &header)
            }
            FileVerb::DeletePermanently => Self::delete_permanently(s),
            FileVerb::Bookmark => Self::bookmark(s),
            FileVerb::Properties => selected(s, "Properties\u{2026}"),
            FileVerb::New => Self::new_item(s),
            FileVerb::UnlockArchive => {
                if s.archive_locked {
                    FileVerbState::on("Unlock\u{2026}")
                } else {
                    FileVerbState::off(
                        "Unlock\u{2026}",
                        "This tab is not showing a locked archive.",
                    )
                }
            }
        }
    }

    // Copy is off inside a container with Cut: Ctrl+C is a promise to produce bytes *later*,
    // and an entry cannot keep it. Extract is the honest verb.
    fn cut_or_copy(verb: FileVerb, s: &FileVerbSnapshot<'_>) -> FileVerbState {
        let header = if verb == FileVerb::Cut { "Cut" } else { "Copy" };
        if s.count == 0 {
            return FileVerbState::off(header, NOTHING_SELECTED);
        }
        if s.in_archive {
            FileVerbState::off(
                header,
                "Not available inside an archive \u{2014} use Extract.",
            )
        } else {
            FileVerbState::on(header)
        }
    }

    fn paste(s: &FileVerbSnapshot<'_>) -> FileVerbState {
        if s.in_archive {
            return FileVerbState::off("Paste", INSIDE_ARCHIVE);
        }
        if s.clipboard_has_files.is_some_and(|f| f()) {
            FileVerbState::on("Paste")
        } else {
            FileVerbState::off("Paste", "There are no files on the clipboard.")
        }
    }

    // Real files only: a checksum answers for a file's bytes, a folder has none of its own.
    fn checksum(s: &FileVerbSnapshot<'_>) -> FileVerbState {
        let header = if s.files > 1 {
            format!("Checksums of {} files\u{2026}", thousands(s.files as i64))
        } else {
            "Checksum\u{2026}".to_string()
        };
        if s.in_archive {
            return FileVerbState::off(header, INSIDE_ARCHIVE);
        }
        if s.count == 0 {
            return FileVerbState::off(header, NOTHING_SELECTED);
        }
        if s.files == s.count {
            FileVerbState::on(header)
        } else {
            FileVerbState::off(
                header,
                "A checksum is of a file; the selection includes a folder.",
            )
        }
    }

    // Verifying is a verb about a folder: a checksum file describes what is around it.
    fn verify_checksums(s: &FileVerbSnapshot<'_>) -> FileVerbState {
        if s.in_archive {
            FileVerbState::off("Verify checksums\u{2026}", INSIDE_ARCHIVE)
        } else {
            FileVerbState::on("Verify checksums\u{2026}")
        }
    }

    // Exactly two real files; not conditional on a folder comparison being under way.
    fn compare_files(s: &FileVerbSnapshot<'_>) -> FileVerbState {
        let header = "Compare these two files\u{2026}";
        if s.in_archive {
            return FileVerbState::off(header, INSIDE_ARCHIVE);
        }
        if s.count == 2 && s.files == 2 {
            FileVerbState::on(header)
        } else {
            FileVerbState::off(header, "Select exactly two files.")
        }
    }

    // Only while a folder comparison is up. Hidden rather than greyed.
    fn settle_by_content(s: &FileVerbSnapshot<'_>) -> FileVerbState {
        let header = if s.files > 1 {
            "Settle these by content"
        } else {
            "Settle by content"
        };
        if !s.comparing {
            return FileVerbState::hidden(header, "Only while two panes are being compared.");
        }
        if s.in_archive {
            return FileVerbState::off(header, INSIDE_ARCHIVE);
        }
        if s.files > 0 {
            FileVerbState::on(header)
        } else {
            FileVerbState::off(header, "Select the files to settle.")
        }
    }

    // "Open in new tab/pane" only makes sense for folders.
    fn open_in_new_tab(s: &FileVerbSnapshot<'_>) -> FileVerbState {
        let header = if s.folders() > 1 {
            format!("Open {} folders in new tabs", s.folders())
        } else {
            "Open in new tab".to_string()
        };
        if s.folders() > 0 {
            FileVerbState::on(header)
        } else {
            FileVerbState::off(header, "Select a folder.")
        }
    }

    // Both act on the selection or, with nothing selected, on the folder being shown.
    fn open_in_program(verb: FileVerb, s: &FileVerbSnapshot<'_>) -> FileVerbState {
        let header = if verb == FileVerb::OpenInTerminal {
            "Open in Terminal"
        } else {
            "Open in VS Code"
        };
        if s.in_archive {
            return FileVerbState::off(header, INSIDE_ARCHIVE);
        }
        if s.count > 0 || s.has_folder {
            FileVerbState::on(header)
        } else {
            FileVerbState::off(header, "There is no folder here to open.")
        }
    }

    // Extract shows from either side of the container: the one write verb that is *more*
    // available inside an archive than outside one.
    fn extract(verb: FileVerb, s: &FileVerbSnapshot<'_>) -> FileVerbState {
        let in_archive = s.in_archive;
        let header = if verb == FileVerb::ExtractTo {
            "Extract to\u{2026}".to_string()
        } else if in_archive && s.count > 0 {
            format!("Extract {} item(s) here", thousands(s.count as i64))
        } else {
            "Extract here".to_string()
        };
        if in_archive || s.single_is_archive {
            FileVerbState::on(header)
        } else {
            FileVerbState::hidden(header, "Select one archive, or open one.")
        }
    }

    // Compressing reads files by path: off inside a container and off over a search result,
    // where "the folder being shown" is not a folder. A flat branch view does have one.
    fn compress(s: &FileVerbSnapshot<'_>) -> FileVerbState {
        let header = if s.count > 1 {
            format!("Compress {} items\u{2026}", thousands(s.count as i64))
        } else {
            "Compress\u{2026}".to_string()
        };
        if s.in_archive {
            return FileVerbState::off(header, INSIDE_ARCHIVE);
        }
        if s.is_search_result {
            return FileVerbState::off(header, "Not available over search results.");
        }
        if s.has_folder {
            FileVerbState::on(header)
        } else {
            FileVerbState::off(header, "There is no folder here to compress.")
        }
    }

    // Only ever one file, and only where there is something to elevate.
    fn run_as_admin(s: &FileVerbSnapshot<'_>) -> FileVerbState {
        let header = "Run as administrator";
        if s.count != 1 {
            return FileVerbState::off(header, "Select one item.");
        }
        if s.can_run_elevated.is_some_and(|f| f()) {
            FileVerbState::on(header)
        } else {
            FileVerbState::off(
                header,
                "Nothing about this item can be run as administrator.",
            )
        }
    }

    // Rename and Delete stay on inside a container; renaming several at once is not offered
    // in there, since a rewrite writes each entry exactly once.
    fn rename(s: &FileVerbSnapshot<'_>) -> FileVerbState {
        let header = if s.count > 1 {
            format!("Rename {} items\u{2026}", s.count)
        } else {
            "Rename\u{2026}".to_string()
        };
        if s.count == 0 {
            return FileVerbState::off(header, NOTHING_SELECTED);
        }
        if s.count > 1 && s.in_archive {
            FileVerbState::off(header, "Inside an archive, rename one item at a time.")
        } else {
            FileVerbState::on(header)
        }
    }

    // Shift+Delete has no meaning in there: an entry has no Recycle Bin and no staging.
    fn delete_permanently(s: &FileVerbSnapshot<'_>) -> FileVerbState {
        let header = "Delete permanently\u{2026}";
        if s.count == 0 {
            return FileVerbState::off(header, NOTHING_SELECTED);
        }
        if s.in_archive {
            FileVerbState::off(header, "Inside an archive there is only Delete.")
        } else {
            FileVerbState::on(header)
        }
    }

    // A virtual path in the bookmark table would poison every subtree query over it.
    fn bookmark(s: &FileVerbSnapshot<'_>) -> FileVerbState {
        let header = if s.all_bookmarked {
            "Remove bookmark"
        } else {
            "Bookmark"
        };
        if s.count == 0 {
            return FileVerbState::off(header, NOTHING_SELECTED);
        }
        if s.in_archive {
            FileVerbState::off(header, INSIDE_ARCHIVE)
        } else {
            FileVerbState::on(header)
        }
    }

    // New acts on the folder being shown, so it needs one, and a search result is not one.
    fn new_item(s: &FileVerbSnapshot<'_>) -> FileVerbState {
        if s.in_archive {
            return FileVerbState::off("New", INSIDE_ARCHIVE);
        }
        if s.is_search_result {
            return FileVerbState::off("New", "Not available over search results.");
        }
        if s.has_folder {
            FileVerbState::on("New")
        } else {
            FileVerbState::off("New", "There is no folder here to create in.")
        }
    }
}

fn selected(s: &FileVerbSnapshot<'_>, header: &str) -> FileVerbState {
    if s.count > 0 {
        FileVerbState::on(header)
    } else {
        FileVerbState::off(header, NOTHING_SELECTED)
    }
}

fn one_folder_or_none(s: &FileVerbSnapshot<'_>, header: &str) -> FileVerbState {
    if s.count == 0 || (s.count == 1 && s.folders() == 1) {
        FileVerbState::on(header)
    } else {
        FileVerbState::off(
            header,
            "Select one folder, or nothing for the folder being shown.",
        )
    }
}
