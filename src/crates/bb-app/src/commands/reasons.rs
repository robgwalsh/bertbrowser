//! The registry's answer for a command (`registry_why_not`), one small method per catalogue area.

use bb_commands::commands::FileVerb;
use bb_core::transfer::TransferVerb;

use super::{Ctx, NO_OTHER_PANE, WHILE_COMPARING, drive_root};
use crate::app::Inner;
use crate::browse::tabs::parent_path;

fn s(t: &str) -> Option<String> {
    Some(t.to_string())
}

/// The reasons several commands share, each `Some` when the command cannot run.
struct Shared<'a>(&'a Ctx);

impl Shared<'_> {
    fn no_rows(&self) -> Option<String> {
        (self.0.row_count == 0).then(|| "There is nothing here to select.".to_string())
    }
    fn no_selection(&self) -> Option<String> {
        (self.0.selected == 0).then(|| "Nothing is selected.".to_string())
    }
    fn multi_tab(&self) -> Option<String> {
        (self.0.tabs_in_pane < 2).then(|| "This is the only tab in the pane.".to_string())
    }
    fn no_other(&self) -> Option<String> {
        self.0.other.is_none().then(|| NO_OTHER_PANE.to_string())
    }
    fn while_comparing(&self) -> Option<String> {
        self.0.comparing.then(|| WHILE_COMPARING.to_string())
    }
    fn no_preview(&self) -> Option<String> {
        (self.0.side_pane != 1).then(|| "The preview pane is closed.".to_string())
    }
    fn only_pane(&self) -> Option<String> {
        (self.0.pane_count < 2).then(|| "This is the only pane.".to_string())
    }
    fn comparing_only(&self) -> Option<String> {
        (!self.0.comparing).then(|| "Only while two panes are being compared.".to_string())
    }
    fn transferring(&self) -> Option<String> {
        (!self.0.transferring).then(|| "Nothing is being transferred.".to_string())
    }
}

impl Inner {
    /// The registry's answer for the command itself.
    pub(crate) fn registry_why_not(&self, ctx: &Ctx, id: &str) -> Option<String> {
        if !ctx.has_tab {
            return Some("There is no tab.".to_string());
        }
        if let Some(rest) = id.strip_prefix("custom:") {
            let _ = rest;
            return self.custom_command_reason(ctx, id);
        }
        let shared = Shared(ctx);
        match id.split_once('.')?.0 {
            "nav" => Self::why_not_nav(ctx, id),
            "tab" => Self::why_not_tab(ctx, &shared, id),
            "pane" => self.why_not_pane(ctx, &shared, id),
            "select" => Self::why_not_select(&shared, id),
            "file" => self.why_not_file(ctx, id),
            "edit" => self.why_not_edit(ctx, id),
            "view" => Self::why_not_view(ctx, id),
            "search" => Self::why_not_search(ctx, id),
            "tools" => Self::why_not_tools(ctx, &shared, id),
            "preview" => Self::why_not_preview(ctx, &shared, id),
            "app" => Self::why_not_app(ctx, id),
            _ => None,
        }
    }

    fn why_not_nav(ctx: &Ctx, id: &str) -> Option<String> {
        match id {
            "nav.back" => (!ctx.can_back).then(|| "There is nowhere to go back to.".to_string()),
            "nav.forward" => {
                (!ctx.can_forward).then(|| "There is nowhere to go forward to.".to_string())
            }
            "nav.up" => (ctx.path.is_empty() || parent_path(&ctx.path).is_none())
                .then(|| "This is already the top.".to_string()),
            "nav.drive-root" => match drive_root(&ctx.path) {
                Some(root)
                    if !root
                        .trim_end_matches('\\')
                        .eq_ignore_ascii_case(ctx.path.trim_end_matches('\\')) =>
                {
                    None
                }
                _ => s("This is already the top of the drive."),
            },
            "nav.containing-folder" => {
                if !ctx.is_flattened {
                    s(
                        "Only in search results or a flat view \u{2014} elsewhere the folder is the one showing.",
                    )
                } else if ctx.in_archive {
                    s("Not available inside an archive.")
                } else if ctx.selected == 1 {
                    None
                } else {
                    s("Select one item.")
                }
            }
            _ => None,
        }
    }

    fn why_not_tab(ctx: &Ctx, shared: &Shared, id: &str) -> Option<String> {
        match id {
            "tab.close-others" | "tab.next" | "tab.previous" => shared.multi_tab(),
            "tab.reopen" => (ctx.closed_in_pane == 0)
                .then(|| "No tab has been closed in this pane.".to_string()),
            "tab.move-to-new-pane" => (ctx.tabs_in_pane < 2).then(|| {
                "This is the pane's only tab \u{2014} it already has a pane to itself.".to_string()
            }),
            "tab.close-right" => (ctx.tab_index + 1 >= ctx.tabs_in_pane)
                .then(|| "There are no tabs to the right.".to_string()),
            "tab.move-left" => {
                (ctx.tab_index == 0).then(|| "This is already the first tab.".to_string())
            }
            "tab.move-right" => (ctx.tab_index + 1 >= ctx.tabs_in_pane)
                .then(|| "This is already the last tab.".to_string()),
            "tab.move-to-other-pane" => shared.no_other().or_else(|| shared.while_comparing()),
            _ => None,
        }
    }

    fn why_not_pane(&self, ctx: &Ctx, shared: &Shared, id: &str) -> Option<String> {
        match id {
            "pane.close" | "pane.next" | "pane.previous" => shared.only_pane(),
            "pane.swap" => shared.no_other().or_else(|| shared.while_comparing()),
            "pane.equalise" => shared.no_other(),
            "pane.open-here-in-other" => ctx
                .path
                .is_empty()
                .then(|| "There is no folder here to open.".to_string()),
            "pane.open-selected-in-other" => ctx
                .single_folder
                .is_none()
                .then(|| "Select one folder.".to_string()),
            "pane.copy-to-other" => self.send_to_other_reason(ctx, TransferVerb::Copy),
            "pane.move-to-other" => self.send_to_other_reason(ctx, TransferVerb::Move),
            _ => None,
        }
    }

    fn why_not_select(shared: &Shared, id: &str) -> Option<String> {
        match id {
            "select.all" | "select.invert" | "select.pattern" | "select.files"
            | "select.folders" => shared.no_rows(),
            "select.none" | "select.unpattern" => shared.no_selection(),
            _ => None,
        }
    }

    fn why_not_file(&self, ctx: &Ctx, id: &str) -> Option<String> {
        match id {
            "file.open" => self.verb_reason(ctx, FileVerb::Open),
            "file.run-as-admin" => self.verb_reason(ctx, FileVerb::RunAsAdmin),
            "file.open-new-tab" => self.verb_reason(ctx, FileVerb::OpenInNewTab),
            "file.open-pane-right" | "file.open-pane-below" => {
                self.verb_reason(ctx, FileVerb::OpenInNewPane)
            }
            "file.open-terminal" => self.verb_reason(ctx, FileVerb::OpenInTerminal),
            "file.open-vscode" => self.verb_reason(ctx, FileVerb::OpenInVSCode),
            "file.edit-in-bertcut" => self.verb_reason(ctx, FileVerb::EditInBertCut),
            "file.new-folder" | "file.new-file" => self.verb_reason(ctx, FileVerb::New),
            "file.rename" => self.verb_reason(ctx, FileVerb::Rename),
            "file.delete" => self.verb_reason(ctx, FileVerb::Delete),
            "file.delete-permanently" => self.verb_reason(
                ctx,
                if ctx.in_archive {
                    FileVerb::Delete
                } else {
                    FileVerb::DeletePermanently
                },
            ),
            "file.properties" => self.verb_reason(ctx, FileVerb::Properties),
            "file.compress" => self.verb_reason(ctx, FileVerb::Compress),
            "file.extract-here" => self.verb_reason(ctx, FileVerb::ExtractHere),
            "file.extract-to" => self.verb_reason(ctx, FileVerb::ExtractTo),
            "file.unlock-archive" => self.verb_reason(ctx, FileVerb::UnlockArchive),
            "file.bookmark" => self.verb_reason(ctx, FileVerb::Bookmark),
            "file.checksum" => self.verb_reason(ctx, FileVerb::Checksum),
            "file.verify-checksums" => self.verb_reason(ctx, FileVerb::VerifyChecksums),
            "file.compare-files" => self.verb_reason(ctx, FileVerb::CompareFiles),
            "file.settle-by-content" => self.verb_reason(ctx, FileVerb::SettleByContent),
            "file.create-shortcut" => self
                .verb_reason(ctx, FileVerb::Copy)
                .or_else(|| self.verb_reason(ctx, FileVerb::New)),
            "file.copy-to" => self.verb_reason(ctx, FileVerb::Copy),
            "file.move-to" => self.verb_reason(ctx, FileVerb::Cut),
            _ => None,
        }
    }

    fn why_not_edit(&self, ctx: &Ctx, id: &str) -> Option<String> {
        match id {
            "edit.cut" => self.verb_reason(ctx, FileVerb::Cut),
            "edit.copy" => self.verb_reason(ctx, FileVerb::Copy),
            "edit.paste" => self.verb_reason(ctx, FileVerb::Paste),
            "edit.copy-path" => self.verb_reason(ctx, FileVerb::CopyPath),
            "edit.copy-name" => self.verb_reason(ctx, FileVerb::CopyName),
            "edit.copy-folder-path" => ctx
                .path
                .is_empty()
                .then(|| "There is no folder here.".to_string()),
            "edit.paste-shortcut" => self
                .verb_reason(ctx, FileVerb::Paste)
                .or_else(|| self.verb_reason(ctx, FileVerb::New)),
            "edit.undo" => {
                if ctx.transferring {
                    s("An operation is running.")
                } else if ctx.undo.is_none() {
                    s("There is nothing to undo.")
                } else {
                    None
                }
            }
            "edit.redo" => {
                if ctx.transferring {
                    s("An operation is running.")
                } else if ctx.redo.is_none() {
                    s("There is nothing to redo.")
                } else {
                    None
                }
            }
            "edit.clear-undo-history" => {
                (ctx.undo_entries == 0).then(|| "The undo history is already empty.".to_string())
            }
            _ => None,
        }
    }

    fn why_not_view(ctx: &Ctx, id: &str) -> Option<String> {
        match id {
            "view.details" => (ctx.thumbnail_scale <= 0.0)
                .then(|| "This is already the details view.".to_string()),
            "view.thumbnails" => (ctx.thumbnail_scale > 0.0)
                .then(|| "This is already the thumbnail view.".to_string()),
            "view.thumbs-larger" => (ctx.thumbnail_scale >= 1.0)
                .then(|| "The thumbnails are as large as they go.".to_string()),
            "view.thumbs-smaller" => (ctx.thumbnail_scale <= 0.0).then(|| {
                "This is the details view \u{2014} there are no thumbnails to shrink.".to_string()
            }),
            "view.reset-columns" => (!ctx.columns_customized)
                .then(|| "These are already the default columns.".to_string()),
            _ => None,
        }
    }

    fn why_not_search(ctx: &Ctx, id: &str) -> Option<String> {
        match id {
            "search.clear" => ctx
                .search_text
                .is_empty()
                .then(|| "No search is showing.".to_string()),
            "search.stop" => (!ctx.search_running).then(|| "No search is running.".to_string()),
            "search.save" => ctx
                .search_text
                .trim()
                .is_empty()
                .then(|| "There is no search to save.".to_string()),
            _ => None,
        }
    }

    fn why_not_tools(ctx: &Ctx, shared: &Shared, id: &str) -> Option<String> {
        match id {
            "tools.compare-differences" => shared.comparing_only(),
            // Not while a scan is reading: a second one would race it into the same result.
            "tools.compare-rescan" => (!ctx.comparing || ctx.compare_scanning)
                .then(|| "Only while two panes are being compared.".to_string()),
            // A sync is planned from the verdicts, so only from a whole answer that has arrived.
            "tools.compare-sync" => {
                (!ctx.comparing || ctx.compare_scanning || !ctx.compare_can_sync)
                    .then(|| "Only once a comparison has found something to sync.".to_string())
            }
            "tools.indexer-dismiss" => {
                (!ctx.banner_shown).then(|| "The banner is not showing.".to_string())
            }
            "tools.indexer-start" => (!ctx.banner_can_start).then(|| {
                "The search index is already running, or cannot be started here.".to_string()
            }),
            "tools.indexer-retry" => {
                (!ctx.index_can_retry).then(|| "Indexing has not failed.".to_string())
            }
            "tools.transfer-cancel" | "tools.transfer-details" => shared.transferring(),
            "tools.transfer-pause" => {
                if !ctx.transferring {
                    s("Nothing is being transferred.")
                } else if ctx.paused {
                    s("The transfer is already paused.")
                } else {
                    None
                }
            }
            "tools.transfer-resume" => (!ctx.paused).then(|| "No transfer is paused.".to_string()),
            _ => None,
        }
    }

    fn why_not_preview(ctx: &Ctx, shared: &Shared, id: &str) -> Option<String> {
        match id {
            "preview.mode-auto"
            | "preview.mode-raw"
            | "preview.mode-hex"
            | "preview.wrap"
            | "preview.fit"
            | "preview.fit-width"
            | "preview.play-next"
            | "preview.auto-advance"
            | "preview.loop"
            | "preview.autoplay" => shared.no_preview(),
            "preview.play" | "preview.fullscreen" => shared.no_preview().or_else(|| {
                (!ctx.preview_has_media).then(|| "Nothing playable is being previewed.".to_string())
            }),
            _ => None,
        }
    }

    fn why_not_app(ctx: &Ctx, id: &str) -> Option<String> {
        match id {
            "app.bookmark-folder" => {
                if ctx.path.is_empty() {
                    s("There is no folder here to bookmark.")
                } else if ctx.in_archive {
                    s("Not available inside an archive.")
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}
