//! One file per C# test file, one test per C# test method. Theories iterate their rows and name the
//! failing row in the assertion message, so `tools/parity/tests.ps1` can match names one to one.

mod access_denied_tests;
mod animation_timeline_tests;
mod app_paths_tests;
mod app_settings_tests;
mod archive_aware_file_system_service_tests;
mod archive_creator_tests;
mod archive_edit_tests;
mod archive_index_builder_tests;
mod archive_listing_tests;
mod archive_path_tests;
mod archive_reader_tests;
mod archive_search_tests;
mod aspect_ratio_tests;
mod bench_compare_tests;
mod bench_markdown_tests;
mod bench_results_json_tests;
mod bookmark_repository_tests;
mod bookmark_service_tests;
mod built_in_menu_items_tests;
mod change_log_repository_tests;
mod change_log_rules_tests;
mod change_recorder_tests;
mod checksum_algorithms_tests;
mod checksum_compare_tests;
mod checksum_file_tests;
mod checksum_path_tests;
mod checksum_runner_tests;
mod checksum_verify_tests;
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
mod compare_equality_tests;
mod compare_refusal_tests;
mod compare_rules_tests;
mod content_reader_tests;
mod content_scanner_tests;
mod content_search_agreement_tests;
mod content_settlement_tests;
mod content_snippet_tests;
mod content_term_tests;
mod date_shorthand_tests;
mod default_keymap_parity_tests;
mod delete_executor_tests;
mod delete_planner_tests;
mod delete_surveyor_tests;
mod dir_size_repository_tests;
mod directory_removal_tests;
mod disk_usage_in_archive_tests;
mod disk_usage_rules_tests;
mod disk_usage_service_tests;
mod drag_out_contract_tests;
mod drop_in_contract_tests;
mod duplicate_rules_tests;
mod duplicate_scanner_tests;
mod elevated_retry_tests;
mod elevation_host_tests;
mod elevation_protocol_tests;
mod encrypted_archive_tests;
mod executable_path_tests;
mod extract_executor_tests;
mod file_compare_plan_tests;
mod file_content_comparer_tests;
mod file_digester_tests;
mod file_list_diff_tests;
mod file_system_file_hasher_tests;
mod file_verb_rules_tests;
mod flat_view_rules_tests;
mod folder_compare_agreement_tests;
mod folder_comparer_tests;
mod folder_handler_registration_tests;
mod foreground_raise_rules_tests;
mod fs_index_repository_subtree_tests;
mod fs_index_repository_tests;
mod gif_decoder_tests;
mod gif_frame_compositor_tests;
mod go_to_rules_tests;
mod hex_preview_reader_tests;
mod image_zoom_tests;
mod index_crawler_tests;
mod index_protocol_tests;
mod index_watcher_apply_tests;
mod indexer_arguments_tests;
mod indexer_auto_start_task_tests;
mod indexer_banner_rules_tests;
mod indexer_presence_lock_tests;
mod instance_endpoint_tests;
mod key_chord_tests;
mod keymap_rules_tests;
mod layout_tree_tests;
mod legacy_data_tests;
mod line_channel_tests;
mod machine_key_tests;
mod media_playlist_tests;
mod menu_layout_rules_tests;
mod metadata_codec_tests;
mod metadata_edit_tests;
mod metadata_formats_tests;
mod metadata_read_rules_tests;
mod mft_directory_size_builder_tests;
mod mft_index_client_tests;
mod mft_index_host_tests;
mod mft_index_state_tests;
mod mft_path_builder_tests;
mod navigation_request_tests;
mod new_item_executor_tests;
mod new_item_planner_tests;
mod ntfs_parsing_tests;
mod palette_rules_tests;
mod pause_gate_tests;
mod performance_doc_tests;
mod pipe_identity_tests;
mod preview_classifier_tests;
mod preview_metadata_tests;
mod property_fallback_tests;
mod rate_formatter_tests;
mod redo_plan_tests;
mod relative_time_tests;
mod rename_executor_tests;
mod rename_pattern_tests;
mod rename_planner_tests;
mod rename_rule_tests;
mod rename_tag_tests;
mod right_drop_menu_rules_tests;
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
mod shell_new_import_tests;
mod shell_open_command_tests;
mod shortcut_executor_tests;
mod shortcut_planner_tests;
mod size_text_tests;
mod startup_trace_json_tests;
mod static_verb_command_tests;
mod subtree_listing_tests;
mod sync_overwrite_tests;
mod sync_planner_tests;
mod sync_runner_tests;
mod syntax_tokenizer_tests;
mod system_theme_rules_tests;
mod text_differ_tests;
mod text_preview_reader_tests;
mod theme_catalog_tests;
mod theme_color_tests;
mod theme_id_tests;
mod theme_json_tests;
mod theme_resolver_tests;
mod theme_token_tests;
mod timestamp_tests;
mod transfer_estimator_tests;
mod transfer_executor_tests;
mod transfer_merge_expander_tests;
mod transfer_planner_tests;
mod transfer_queue_rules_tests;
mod transfer_rate_tests;
mod transfer_round_trip_tests;
mod treemap_layout_tests;
mod treemap_palette_tests;
mod undo_navigator_tests;
mod undo_record_tests;
mod undo_stack_tests;
mod usn_record_parser_tests;
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
