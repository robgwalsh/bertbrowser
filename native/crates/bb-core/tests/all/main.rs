//! One file per C# test file, one test per C# test method. Theories iterate their rows and name the
//! failing row in the assertion message, so `tools/parity/tests.ps1` can match names one to one.

mod archive_path_tests;
mod bookmark_repository_tests;
mod change_log_repository_tests;
mod change_log_rules_tests;
mod change_recorder_tests;
mod command_catalog_tests;
mod command_line_tests;
mod command_matcher_tests;
mod content_snippet_tests;
mod content_term_tests;
mod date_shorthand_tests;
mod default_keymap_parity_tests;
mod dir_size_repository_tests;
mod file_verb_rules_tests;
mod flat_view_rules_tests;
mod foreground_raise_rules_tests;
mod fs_index_repository_subtree_tests;
mod fs_index_repository_tests;
mod go_to_rules_tests;
mod key_chord_tests;
mod keymap_rules_tests;
mod layout_tree_tests;
mod navigation_request_tests;
mod palette_rules_tests;
mod rate_formatter_tests;
mod relative_time_tests;
mod saved_search_repository_tests;
mod saved_search_rules_tests;
mod saved_workspace_repository_tests;
mod saved_workspace_rules_tests;
mod search_agreement_tests;
mod search_grammar_tests;
mod search_query_tests;
mod selection_pattern_tests;
mod session_layout_tests;
mod size_text_tests;

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
