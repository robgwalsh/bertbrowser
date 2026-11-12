#include "views/tools/tool_dialogs.h"
#include "views/tools/tool_state.h"
#include "views/main/main_view.h"

#include "gfx/icons.gen.h"
#include "views/main/folder_tree.h"
#include "views/panes/media_pane.h"
#include "views/bridge/ops.h"
#include "views/dialogs/ops_card.h"
#include "views/dialogs/ops_menus.h"
#include "views/panes/property_host.h"
#include "views/shell/shell_props.h"
#include "views/tools/tool_widgets.h"

#include <dwmapi.h>
#include <shobjidl.h>

#include <algorithm>
#include <cmath>

namespace bb::views {

using namespace bb::ui;

void MainView::sink(void* ctx, BbEvent* evt) {
    // A Rust thread: enqueue and return. The window drains it on the UI thread.
    auto* self = static_cast<MainView*>(ctx);
    HWND hwnd = self->hwnd_.load();
    if (!hwnd || !PostMessageW(hwnd, WM_BB_EVENT, 0, reinterpret_cast<LPARAM>(evt))) {
        bb_event_free(evt);
    }
}

bool MainView::on_window_message(UINT msg, WPARAM wp, LPARAM lp) {
    if (msg == WM_BB_BARS) {
        bars_pending_ = false;
        try {
            refresh_bars();
        } catch (const std::exception&) {
        }
        return true;
    }
    if (msg == WM_BB_DEFER) {
        auto run = std::move(deferred_);
        deferred_.clear();
        for (auto& fn : run) fn();
        return true;
    }
    if (msg == WM_BB_ICONS) {
        icons_->drain();
        return true;
    }
    if (msg == WM_BB_TREE) {
        if (tree_provider_ && tree_provider_->drain() && tree_) tree_->refresh();
        return true;
    }
    if (msg == WM_BB_THUMBS) {
        for (auto& kv : by_tab_) kv.second->drain_thumbnails();
        return true;
    }
    if (msg == WM_BB_PREVIEW) {
        deliver_preview(static_cast<uint64_t>(wp), reinterpret_cast<PreviewResult*>(lp));
        return true;
    }
    if (msg != WM_BB_EVENT) return false;
    auto* evt = reinterpret_cast<BbEvent*>(lp);
    try {
        handle(*evt);
    } catch (const std::exception&) {
        // A view that cannot read the core right now repaints on the next event.
    }
    ++handled_;
    bb_event_free(evt);
    return true;
}

void MainView::deliver_preview(uint64_t tab, PreviewResult* raw) {
    std::unique_ptr<PreviewResult> result(raw);
    TabView* tv = tab_view(tab);
    if (!tv || !tv->preview()) {
        // The tab went away (or is parked) while its answer was on the way.
        if (result->gif) bb_gif_close(core_.app(), static_cast<uint64_t>(result->gif));
        return;
    }
    tv->preview()->apply(std::move(result));
}

void MainView::handle(const BbEvent& e) {
    switch (e.kind) {
        case BB_EVT_LISTING_CHANGED: {
            TabView* tv = tab_view(e.id);
            if (!tv) {
                sync_layout();
                tv = tab_view(e.id);
            }
            if (tv) tv->reload_listing();
            refresh_status();
            break;
        }
        case BB_EVT_TAB_CHANGED: {
            TabView* tv = tab_view(e.id);
            if (!tv) {
                sync_layout();
            } else {
                tv->reload_state();
                sync_layout();
            }
            refresh_status();
            break;
        }
        case BB_EVT_LAYOUT_CHANGED:
        case BB_EVT_PANE_CHANGED:
            sync_layout();
            break;
        case BB_EVT_SETTINGS_CHANGED: {
            const std::wstring key = core::w(e.text);
            if (core::icontains(key, L"theme")) apply_theme();
            if (key == L"showHiddenItems") sync_tree_hidden();
            if (key == L"scrollSpeedMultiplier") sync_wheel_speed();
            if (key == L"iconSize") sync_icon_size();
            if (key == L"tileAspectRatio") {
                for (auto& kv : by_tab_) kv.second->reload_state();
            }
            // The page redraws only what moved (its signature), so this is cheap; it is how an import that
            // finished off the UI thread, or a problem with a list just typed, reaches the page.
            if (settings_ && settings_->is_open()) settings_->sync();
            break;
        }
        case BB_EVT_OP_FINISHED:
            // A cut is one-shot once something has moved, even if not everything did; a paste that moved
            // nothing leaves it to try elsewhere. Spec: ShellViewModel (Completed.Count > 0).
            if (cut_paste_op_ && e.id == cut_paste_op_) {
                const bool moved = e.a != 0 || std::any_of(cut_paste_paths_.begin(), cut_paste_paths_.end(), [](const std::wstring& p) {
                                       return GetFileAttributesW(p.c_str()) == INVALID_FILE_ATTRIBUTES;
                                   });
                if (moved && clipboard_seq() == cut_paste_seq_) {
                    if (headless_dialogs_) {
                        run_clipboard_.reset();
                        ++run_clipboard_seq_;
                    } else {
                        file_clipboard::clear();
                    }
                    push_clipboard_flag();
                }
                cut_paste_op_ = 0;
            }
            // An operation may have made, renamed, moved or deleted a folder: the open branches of the tree read
            // their sub-folders again. Spec: ShellViewModel.RefreshAfterCreateAsync (the tree's half).
            if (tree_ && tree_provider_ && tree_provider_->reload_where([this](uint64_t id) { return tree_->is_expanded(id); })) {
                tree_->refresh();
            }
            duplicates_settle_prune(*this);
            [[fallthrough]];
        case BB_EVT_UNDO_CHANGED:
        case BB_EVT_BUSY_CHANGED:
            if (e.kind != BB_EVT_BUSY_CHANGED && on_undo_changed) on_undo_changed();
            refresh_status();
            break;
        case BB_EVT_PROPERTIES_NEEDED:
            supply_properties(e.id);
            break;
        case BB_EVT_MODEL_CHANGED:
            // The helper's two-second usage tick: the button's tip and the Settings rows, nothing else - and not
            // the bars either, since what the helper costs never decides whether a command can run.
            if (e.a == 25) {
                refresh_index_usage();
                return;
            }
            // A request, not a state: Edit in BertCut could not find BertCut, and nothing else is stale.
            if (e.a == 26) {
                locate_bertcut();
                return;
            }
            if (e.a == 2) apply_theme();
            if (e.a == 10) {
                for (auto& kv : by_tab_) kv.second->reload_columns();
            }
            if (e.a == 4) refresh_tooltips();
            if (e.a == 11) compare_banner_sync(browser_->find("CompareBanner"));
            if (e.a == 14) sync_transfer();
            if (e.a == 15) sync_questions(L"conflicts");
            if (e.a == 16) sync_questions(L"elevation");
            if (e.a == 17) sync_questions(L"confirm");
            if (e.a == 18) sync_questions(L"notice");
            if (e.a == 19 && on_delete_survey) on_delete_survey();
            if (e.a == 20) sync_duplicates();
            if (e.a == 9) refresh_index_windows();
            if (e.a == 21) refresh_tool(L"checksum-show");
            if (e.a == 22) refresh_tool(L"disk-usage-show");
            if (e.a == 23) refresh_tool(L"compare-files-show");
            if (e.a == 24) sync_devices(false);
            if (e.a == 13) {
                for (auto& kv : by_tab_) {
                    if (kv.second->metadata()) kv.second->metadata()->refresh();
                }
            }
            if (e.a == 1 || e.a == 2 || e.a == 4 || e.a == 5 || e.a == 6 || e.a == 7 || e.a == 8 || e.a == 9 || e.a == 10) sync_overlays();
            break;
        default:
            break;
    }
    // Whatever moved may have changed what a command can do now: the bars catch up once, at idle.
    if (!bars_pending_ && hwnd_.load()) {
        bars_pending_ = true;
        PostMessageW(hwnd_.load(), WM_BB_BARS, 0, 0);
    }
}


}  // namespace bb::views
