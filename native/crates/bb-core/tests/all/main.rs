//! One file per C# test file, one test per C# test method. Theories iterate their rows and name the
//! failing row in the assertion message, so `tools/parity/tests.ps1` can match names one to one.

mod access_denied_tests;
mod app_paths_tests;
mod app_settings_tests;
mod archive_path_tests;
mod bench_compare_tests;
mod bench_markdown_tests;
mod bench_results_json_tests;
mod bookmark_repository_tests;
mod bookmark_service_tests;
mod built_in_menu_items_tests;
mod change_log_repository_tests;
mod change_log_rules_tests;
mod change_recorder_tests;
mod column_candidates_tests;
mod column_catalog_tests;
mod column_comparison_tests;
mod column_layout_rules_tests;
mod command_bar_edit_tests;
mod command_bar_rules_tests;
mod command_catalog_tests;
mod command_line_tests;
mod command_matcher_tests;
mod command_template_tests;
mod content_reader_tests;
mod content_scanner_tests;
mod content_search_agreement_tests;
mod content_snippet_tests;
mod content_term_tests;
mod date_shorthand_tests;
mod default_keymap_parity_tests;
mod dir_size_repository_tests;
mod directory_removal_tests;
mod elevation_protocol_tests;
mod executable_path_tests;
mod file_list_diff_tests;
mod file_verb_rules_tests;
mod flat_view_rules_tests;
mod folder_handler_registration_tests;
mod foreground_raise_rules_tests;
mod fs_index_repository_subtree_tests;
mod fs_index_repository_tests;
mod go_to_rules_tests;
mod index_crawler_tests;
mod index_protocol_tests;
mod index_watcher_apply_tests;
mod instance_endpoint_tests;
mod key_chord_tests;
mod keymap_rules_tests;
mod layout_tree_tests;
mod legacy_data_tests;
mod machine_key_tests;
mod menu_layout_rules_tests;
mod navigation_request_tests;
mod palette_rules_tests;
mod performance_doc_tests;
mod rate_formatter_tests;
mod relative_time_tests;
mod run_as_verb_rules_tests;
mod saved_command_bar_rules_tests;
mod saved_search_repository_tests;
mod saved_search_rules_tests;
mod saved_workspace_repository_tests;
mod saved_workspace_rules_tests;
mod search_agreement_tests;
mod search_grammar_tests;
mod search_query_tests;
mod search_service_tests;
mod selection_pattern_tests;
mod session_layout_tests;
mod shell_menu_keys_tests;
mod shell_menu_rules_tests;
mod shell_open_command_tests;
mod size_text_tests;
mod startup_trace_json_tests;
mod static_verb_command_tests;
mod system_theme_rules_tests;
mod theme_catalog_tests;
mod theme_color_tests;
mod theme_id_tests;
mod theme_json_tests;
mod theme_resolver_tests;
mod theme_token_tests;
mod treemap_palette_tests;
mod vs_code_path_tests;

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
