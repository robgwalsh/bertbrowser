//! One file per C# test file, one test per C# test method. Theories iterate their rows and name the
//! failing row in the assertion message, so `tools/parity/tests.ps1` can match names one to one.

mod archives;
mod benchmarking;
mod changes;
mod checksums;
mod cli;
mod columns;
mod commandbars;
mod commands;
mod compare;
mod data;
mod delete;
mod diff;
mod diskusage;
mod duplicates;
mod elevation;
mod fs;
mod ipc;
mod launch;
mod layout;
mod metadata;
mod mft;
mod newitem;
mod preview;
mod rename;
mod rules;
mod search;
mod settings;
mod shell_integration;
mod shellmenu;
mod shortcuts;
mod theming;
mod timestamps;
mod transfer;
mod undo;
mod updates;
mod util;

/// An owned candidate for tests: the search sees borrowed, upper-cased keys, so a helper has to own
/// the strings it hands out.
pub mod support {
    use bb_core::FileTime;
    use bb_core::search::{ContentText, SearchCandidate};
    use bb_paths::fold_upper;

    pub struct Entry {
        pub name_key: String,
        pub path_key: String,
        pub is_directory: bool,
        pub size_bytes: i64,
        pub modified_utc: FileTime,
        pub hidden: bool,
        pub attributes: u32,
        pub created_utc: FileTime,
        pub content: Option<ContentText>,
    }

    impl Entry {
        pub fn file(name: &str) -> Entry {
            Entry::in_dir(name, r"C:\Data")
        }

        pub fn in_dir(name: &str, dir: &str) -> Entry {
            let noon = FileTime::from_ymd_hms(2026, 6, 1, 12, 0, 0);
            Entry {
                name_key: fold_upper(name),
                path_key: fold_upper(&format!("{dir}\\{name}")),
                is_directory: false,
                size_bytes: 100,
                modified_utc: noon,
                hidden: false,
                attributes: bb_core::attributes::ARCHIVE,
                created_utc: noon,
                content: None,
            }
        }

        pub fn dir(name: &str) -> Entry {
            let mut e = Entry::file(name);
            e.is_directory = true;
            e.size_bytes = 0;
            e.attributes = bb_core::attributes::DIRECTORY;
            e
        }

        pub fn size(mut self, size: i64) -> Entry {
            self.size_bytes = size;
            self
        }
        pub fn hidden(mut self, hidden: bool) -> Entry {
            self.hidden = hidden;
            self
        }
        pub fn attributes(mut self, attributes: u32) -> Entry {
            self.attributes = attributes;
            self
        }
        pub fn modified(mut self, t: FileTime) -> Entry {
            self.modified_utc = t;
            self
        }
        pub fn created(mut self, t: FileTime) -> Entry {
            self.created_utc = t;
            self
        }
        pub fn content(mut self, text: &str) -> Entry {
            self.content = Some(ContentText::new(text, false));
            self
        }

        pub fn candidate(&self) -> SearchCandidate<'_> {
            SearchCandidate {
                name_key: &self.name_key,
                path_key: &self.path_key,
                is_directory: self.is_directory,
                size_bytes: self.size_bytes,
                modified_utc: self.modified_utc,
                hidden: self.hidden,
                attributes: self.attributes,
                created_utc: self.created_utc,
                content: self.content.as_ref(),
            }
        }
    }
}
