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

namespace {

std::wstring key_name(unsigned vk) {
    if ((vk >= 'A' && vk <= 'Z') || (vk >= '0' && vk <= '9')) return std::wstring(1, static_cast<wchar_t>(vk));
    if (vk >= VK_F1 && vk <= VK_F24) return L"F" + std::to_wstring(vk - VK_F1 + 1);
    if (vk >= VK_NUMPAD0 && vk <= VK_NUMPAD9) return L"Num" + std::to_wstring(vk - VK_NUMPAD0);
    switch (vk) {
        case VK_BACK: return L"Backspace";
        case VK_DELETE: return L"Del";
        case VK_RETURN: return L"Enter";
        case VK_ESCAPE: return L"Esc";
        case VK_TAB: return L"Tab";
        case VK_SPACE: return L"Space";
        case VK_LEFT: return L"Left";
        case VK_RIGHT: return L"Right";
        case VK_UP: return L"Up";
        case VK_DOWN: return L"Down";
        case VK_HOME: return L"Home";
        case VK_END: return L"End";
        case VK_PRIOR: return L"PageUp";
        case VK_NEXT: return L"PageDown";
        case VK_INSERT: return L"Insert";
        case VK_APPS: return L"Apps";
        case VK_ADD: return L"Num+";
        case VK_SUBTRACT: return L"Num-";
        case VK_MULTIPLY: return L"Num*";
        case VK_DIVIDE: return L"Num/";
        case VK_DECIMAL: return L"Num.";
        case VK_OEM_COMMA: return L",";
        case VK_OEM_PERIOD: return L".";
        case VK_OEM_2: return L"/";
        case VK_OEM_1: return L";";
        case VK_OEM_7: return L"'";
        case VK_OEM_4: return L"[";
        case VK_OEM_6: return L"]";
        case VK_OEM_5: return L"\\";
        case VK_OEM_MINUS: return L"-";
        case VK_OEM_PLUS: return L"=";
        case VK_OEM_3: return L"`";
        default: return L"";
    }
}

}  // namespace

std::wstring chord_key_name(unsigned vk) { return key_name(vk); }

BbFocusKind MainView::focus_kind() const {
    Widget* f = window_.root().focused();
    if (const auto* edit = dynamic_cast<Edit*>(f)) return edit->read_only ? BB_FOCUS_TEXT_READ_ONLY : BB_FOCUS_TEXT_EDITABLE;
    if (!f || dynamic_cast<ListView*>(f) || dynamic_cast<TileView*>(f)) return BB_FOCUS_FILE_LIST;
    return BB_FOCUS_OTHER;
}

uint32_t MainView::focus_flags() {
    uint32_t flags = 0;
    if (try_model([&](BbOwnedStr* o) { return bb_settings_page_state(core_.app(), o); })["open"].boolean()) {
        flags |= BB_FOCUS_SETTINGS_OPEN;
        if (try_model([&](BbOwnedStr* o) { return bb_keys_page(core_.app(), o); })["recording"].boolean()) flags |= BB_FOCUS_RECORDING;
    }
    if (try_model([&](BbOwnedStr* o) { return bb_palette_state(core_.app(), o); })["open"].boolean()) flags |= BB_FOCUS_PALETTE_OPEN;
    if (dialog_open_) flags |= BB_FOCUS_CONTENT_BLOCKED;
    if (popup_ && popup_->menu_open()) flags |= BB_FOCUS_IN_POPUP;
    return flags;
}

bool MainView::on_preview_key(const KeyEvent& e) {
    if (dialog_key(e)) return true;
    if (menu_bar_ && menu_bar_->preview_key(e)) return true;
    if (palette_ && palette_->preview_key(e)) return true;
    if (settings_ && settings_->is_open() && settings_->preview_key(e)) return true;
    const std::wstring key = key_name(e.vk);
    if (key.empty()) return false;
    const uint32_t mods = (e.mods.ctrl ? 1u : 0u) | (e.mods.alt ? 2u : 0u) | (e.mods.shift ? 4u : 0u);
    const json::Value r = try_model([&](BbOwnedStr* o) {
        return bb_key_press(core_.app(), BbKeyChord{mods, core::s(key)}, BbFocusState{focus_kind(), focus_flags()}, false, o);
    });
    if (!r["claimed"].boolean()) return false;
    // A claimed chord is the command's even when it cannot run now.
    if (r["result"].str() == "ui") host_command(core::wide(r["command"].str()));
    if (r["result"].str() == "ran") after_command(core::wide(r["command"].str()));
    return true;
}

bool MainView::execute_command(const std::wstring& id, std::wstring* reason, bool* ui) {
    bool needs_ui = false;
    const BbStatus st = bb_command_execute(core_.app(), core::s(id), &needs_ui);
    if (st != BB_OK) {
        if (reason) *reason = core::last_error();
        return false;
    }
    if (needs_ui) {
        const bool done = host_command(id);
        if (ui) *ui = !done;  // true: it wanted a person (a dialog) and nothing was done
    } else {
        after_command(id);
    }
    return true;
}

// A command that changed which pane or tab is active, or made a new one, leaves the keyboard on that
// tab's list, as the original's PaneFocusRequested did - otherwise a rebuilt layout leaves it nowhere and
// the next arrow key goes unheard until a click. Deferred, so it lands after the layout the command
// changed has been rebuilt from the events already posted.
void MainView::after_command(const std::wstring& id) {
    const bool moves_focus = (id.rfind(L"tab.", 0) == 0 || id.rfind(L"pane.", 0) == 0 || id == L"file.open-pane-right" ||
                              id == L"file.open-pane-below") &&
                             id != L"pane.focus-tree" && id != L"pane.open-here-in-other" && id != L"pane.copy-to-other" &&
                             id != L"pane.move-to-other";
    if (!moves_focus) return;
    defer([this] {
        if (!dialog_open_) focus_file_list();
    });
}

bool MainView::host_command(const std::wstring& id) {
    if (id == L"nav.address-bar") {
        if (TabView* tv = active_view()) tv->begin_path_edit();
        return true;
    }
    // The side pane's own: the core only decides whether they may run (a pane is open, something plays).
    if (id == L"preview.fit-width" || id == L"preview.play" || id == L"preview.play-next" || id == L"preview.fullscreen") {
        TabView* tv = active_view();
        if (!tv || !tv->preview()) return false;
        if (id == L"preview.fit-width") tv->fit_side_width();
        else if (id == L"preview.play-next") tv->select_next_media();
        else if (id == L"preview.play") tv->preview()->media()->toggle_play();
        else tv->preview()->media()->set_full_screen(!tv->preview()->media()->full_screen());
        return true;
    }
    // The operations the core runs but only the host can start: they need the selection or a confirmation.
    if (id == L"edit.clear-undo-history") {
        if (!shows_dialogs()) return false;
        open_dialog(L"clear-undo");
        return true;
    }
    if (id == L"tools.transfer-cancel") {
        ops::run(core_, "cancel-running");
        return true;
    }
    if (id == L"file.create-shortcut") {
        std::vector<std::wstring> paths;
        for (const auto& r : selected_rows()) paths.push_back(r.path);
        if (paths.empty()) return false;
        ops::run(core_, "shortcut", "{\"sources\":" + ops::qa(paths) + ",\"directory\":" + ops::q(core_.tab_text(active_tab_, BB_TEXT_PATH)) + "}");
        return true;
    }
    // Commands that start another program for the user: the core decides what from the selection and
    // starts it through the one launcher (the harness's refuses and records).
    if (id == L"file.run-as-admin" || id == L"file.open-terminal" || id == L"file.open-vscode" || id.rfind(L"custom:", 0) == 0) {
        return bb_launch_command(core_.app(), core::s(id)) == BB_OK;
    }
    // Commands that put a dialog up. A run with no one to answer reports them instead.
    static const std::pair<const wchar_t*, const wchar_t*> dialogs[] = {
        {L"file.new-folder", L"new-folder"}, {L"file.new-file", L"new-file"},       {L"file.rename", L"rename"},
        {L"file.delete", L"delete"},         {L"file.delete-permanently", L"delete-permanently"},
        {L"file.properties", L"properties"}, {L"file.compress", L"compress"},
        {L"file.extract-to", L"extract"},    {L"file.unlock-archive", L"archive-password"},
        {L"app.about", L"about"},            {L"search.syntax", L"search-syntax"},  {L"search.save", L"saved-search"},
        {L"select.pattern", L"select-pattern"}, {L"select.unpattern", L"deselect-pattern"},
        {L"view.add-column", L"columns"},    {L"tools.transfer-details", L"transfer"}, {L"edit.undo-history", L"undo-history"},
        {L"app.customise-theme", L"theme-editor"},
    };
    for (const auto& [command, kind] : dialogs) {
        if (id != command) continue;
        if (!shows_dialogs()) return false;
        open_dialog(kind);
        return true;
    }
    // Extract here asks nothing: it goes where ExtractRules says, keeping both on a clash. Spec: ExtractAsync(askWhere: false).
    if (id == L"file.extract-here") {
        const ops::ExtractSubject subject = ops::extract_subject(*this);
        if (subject.archive.empty()) return false;
        // A refused plan is a warning card, as Extract to's is. Spec: DirectoryTabView.ExtractAsync.
        try {
            ops::run_extract(*this, subject, ops::extract_destination(*this, subject.archive), false);
        } catch (const std::exception& e) {
            show_message(L"Extract", core::message_of(e));
        }
        return true;
    }
    if (id == L"tools.duplicates" && shows_dialogs()) {
        open_duplicates(core_.tab_text(active_tab_, BB_TEXT_PATH));
        return true;
    }
    if (id == L"edit.copy" || id == L"edit.cut") {
        std::vector<std::wstring> paths;
        for (const auto& r : selected_rows()) paths.push_back(r.path);
        if (paths.empty()) return false;
        const bool cut = id == L"edit.cut";
        const std::wstring said = clipboard_put(paths, cut)
                                      ? std::to_wstring(paths.size()) + L" item(s) " + (cut ? L"cut" : L"copied")
                                      : L"Could not copy - the clipboard is in use by another program.";
        bb_status_set(core_.app(), core::s(said));
        return true;
    }
    if (id == L"edit.paste" || id == L"edit.paste-shortcut") return paste(id == L"edit.paste-shortcut");
    if (id == L"file.copy-to" || id == L"file.move-to") return transfer_to_chosen(id == L"file.move-to");
    if (id == L"app.exit") {
        // A run has no window of the person's to close.
        if (headless_dialogs_) return false;
        PostMessageW(window_.hwnd(), WM_CLOSE, 0, 0);
        return true;
    }
    if (id == L"pane.focus-tree") {
        window_.root().set_focus(tree_);
        return true;
    }
    if (id == L"pane.focus-list") return focus_file_list();
    if (id == L"search.folder" || id == L"search.pc") {
        TabView* tv = active_view();
        if (!tv) return false;
        if (id == L"search.folder") tv->reveal_search(false);  // the box is a button until it is wanted
        Widget* target = id == L"search.folder" ? static_cast<Widget*>(tv->search_box()) : static_cast<Widget*>(tv->global_search_box());
        if (!target) return false;
        window_.root().set_focus(target);
        // A search box takes focus with its text selected, so typing replaces it.
        if (auto* edit = dynamic_cast<Edit*>(target)) edit->select(0, edit->text().size());
        return true;
    }
    if (id == L"app.save-workspace" || id == L"app.save-command-bars") {
        if (!shows_dialogs()) return false;
        open_dialog(id == L"app.save-workspace" ? L"save-workspace" : L"save-command-bars");
        return true;
    }
    if (id == L"edit.copy-path" || id == L"edit.copy-name" || id == L"edit.copy-folder-path") {
        std::vector<std::wstring> paths;
        if (id == L"edit.copy-folder-path") paths.push_back(core_.tab_text(active_tab_, BB_TEXT_PATH));
        else {
            for (const auto& r : selected_rows()) paths.push_back(id == L"edit.copy-name" ? r.name : r.path);
        }
        if (paths.empty() || paths.front().empty()) return false;
        // "Copy as path" quotes each one, as Explorer's does; this folder's own path is copied bare.
        copy_text(paths, id == L"edit.copy-path", id == L"edit.copy-folder-path");
        return true;
    }
    return tool_host_command(*this, id);
}

void MainView::set_headless_dialogs(bool on) {
    headless_dialogs_ = on;
    push_clipboard_flag();
}

uint32_t MainView::clipboard_seq() const { return headless_dialogs_ ? run_clipboard_seq_ : GetClipboardSequenceNumber(); }

bool MainView::clipboard_put(const std::vector<std::wstring>& paths, bool cut) {
    if (headless_dialogs_) {
        run_clipboard_ = ClipboardFiles{paths, cut};
        ++run_clipboard_seq_;
        push_clipboard_flag();
        return true;
    }
    return file_clipboard::put(paths, cut);  // WM_CLIPBOARDUPDATE pushes the flag
}

std::optional<ClipboardFiles> MainView::clipboard_get() { return headless_dialogs_ ? run_clipboard_ : file_clipboard::get(); }

void MainView::push_clipboard_flag() {
    const bool has = headless_dialogs_ ? run_clipboard_.has_value() : file_clipboard::has_files();
    bb_ui_set_flag(core_.app(), core::s(L"clipboardHasFiles"), has);
}

bool MainView::paste(bool shortcut) {
    const std::wstring here = core_.tab_text(active_tab_, BB_TEXT_PATH);
    if (here.empty()) return false;
    const std::optional<ClipboardFiles> clip = clipboard_get();
    if (!clip || clip->paths.empty()) {
        bb_status_set(core_.app(), core::s(L"There are no files on the clipboard."));
        return true;
    }
    // A paste is a drop by another name: the one planner and executor, with progress, conflicts and undo.
    const uint64_t op = transfer_into(core_, clip->paths, here, shortcut ? "shortcut" : clip->cut ? "paste-move" : "paste-copy");
    if (!shortcut && clip->cut && op) {
        cut_paste_op_ = op;
        cut_paste_seq_ = clipboard_seq();
        cut_paste_paths_ = clip->paths;
    }
    return true;
}

bool MainView::transfer_to_chosen(bool move) {
    std::vector<std::wstring> sources;
    for (const auto& r : selected_rows()) sources.push_back(r.path);
    // A run cannot answer Windows' folder picker, so it reports the command instead.
    if (sources.empty() || headless_dialogs_) return false;
    IFileOpenDialog* dialog = nullptr;
    if (FAILED(CoCreateInstance(CLSID_FileOpenDialog, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&dialog)))) return false;
    DWORD options = 0;
    dialog->GetOptions(&options);
    dialog->SetOptions(options | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST);
    dialog->SetTitle(move ? L"Move to folder" : L"Copy to folder");
    const std::wstring here = core_.tab_text(active_tab_, BB_TEXT_PATH);
    IShellItem* start = nullptr;
    if (!here.empty() && SUCCEEDED(SHCreateItemFromParsingName(here.c_str(), nullptr, IID_PPV_ARGS(&start)))) {
        dialog->SetFolder(start);
        start->Release();
    }
    std::wstring chosen;
    if (SUCCEEDED(dialog->Show(window_.hwnd()))) {
        IShellItem* item = nullptr;
        if (SUCCEEDED(dialog->GetResult(&item))) {
            PWSTR path = nullptr;
            if (SUCCEEDED(item->GetDisplayName(SIGDN_FILESYSPATH, &path))) {
                chosen = path;
                CoTaskMemFree(path);
            }
            item->Release();
        }
    }
    dialog->Release();
    if (!chosen.empty()) transfer_into(core_, sources, chosen, move ? "move" : "copy");
    return true;
}

void MainView::open_duplicates(const std::wstring& folder) {
    // Pointed at the folder (none: this PC) and waiting for Scan: a scan reads real files, so it is asked for.
    const std::string root = folder.empty() ? "null" : json_quote(folder);
    try_tool_call(*this, "duplicates.load", "{\"root\":" + root + "}");
    duplicates_.ticked.clear();
    sync_duplicates();
    open_dialog(L"duplicates");
}

// The index moved (a volume finished, the helper started or stopped): the windows whose answer comes from it
// ask again. Duplicates scans again only when its answer was not believable yet; a finished scan is not
// thrown away for an index that was already enough. Spec: DiskUsageViewModel / ChangeTimelineViewModel /
// DuplicatesViewModel .OnIndexRefreshed and .OnStatusChanged.
void MainView::refresh_index_windows() {
    refresh_tool(L"disk-usage-at");
    if (tool_window(L"changes")) refresh_changes(true);
    if (tool_window(L"duplicates")) {
        const json::Value& m = duplicates_.result;
        if (m["hasScanned"].boolean() && !m["scanning"].boolean() && m["availability"].str() != "Ready") {
            const std::string root = m["root"].is_null() ? "null" : json_quote(core::wide(m["root"].str()));
            try_tool_call(*this, "duplicates.scan", "{\"root\":" + root + "}");
        } else {
            sync_duplicates();
        }
    }
}

// What changed slides with the clock ("in the last hour"), so its window reads the log again every two
// seconds while it is open - and is rebuilt only when what it would show is different, so a list somebody
// is reading does not jump under them. Spec: ChangeTimelineWindow (its DispatcherTimer).
void MainView::refresh_changes(bool force) {
    if (!tool_window(L"changes")) return;
    // The clock tick asks the log's stamp, which is cheap; the query runs when that moved, when recording was
    // switched, or every thirty seconds so "2 min ago" keeps up. Spec: ChangeTimelineViewModel.OnTick.
    {
        const json::Value st = try_tool_call(*this, "changes.stamp");
        const std::string stamp = st["stamp"].str() + (st["recording"].boolean() ? "|on" : "|off");
        if (!force && stamp == changes_stamp_ && ++changes_ticks_ < 15) return;
        changes_stamp_ = stamp;
    }
    changes_ticks_ = 0;
    std::wstring error;
    const json::Value m = try_tool_call(*this, "changes.model", "", &error);
    if (m.is_null()) return;
    std::string sig = m["summary"].str() + "|" + m["markText"].str() + "|" + m["message"].str() + "|" + std::to_string(m["rows"].size());
    for (size_t i = 0; i < m["rows"].size() && i < 64; ++i) sig += "|" + m["rows"][i]["path"].str() + m["rows"][i]["when"].str();
    if (!force && sig == changes_signature_) return;
    changes_signature_ = sig;
    refresh_tool(L"changes-show");
}

void MainView::sync_duplicates() {
    std::wstring error;
    json::Value m = try_tool_call(*this, "duplicates.model", "", &error);
    if (m.is_null()) return;
    const int64_t version = m["version"].i64();
    if (version != duplicates_.version) duplicates_.ticked.clear();
    duplicates_.version = version;
    duplicates_.result = std::move(m);
    duplicates_.has = true;
    refresh_tool(L"duplicates");
}

// "Copy as path" quotes every path, as Explorer's does, one per line; a name is bare text. A scripted run
// leaves the clipboard alone (it is the person's, not the run's) and only says what it would have copied.
void MainView::copy_text(const std::vector<std::wstring>& items, bool paths, bool this_folder) {
    std::wstring text;
    for (const auto& item : items) {
        if (!text.empty()) text += L"\r\n";
        text += paths ? L"\"" + item + L"\"" : item;
    }
    bool copied = headless_dialogs_;
    if (!headless_dialogs_ && OpenClipboard(window_.hwnd())) {
        EmptyClipboard();
        const size_t bytes = (text.size() + 1) * sizeof(wchar_t);
        if (HGLOBAL mem = GlobalAlloc(GMEM_MOVEABLE, bytes)) {
            if (void* p = GlobalLock(mem)) {
                memcpy(p, text.c_str(), bytes);
                GlobalUnlock(mem);
                copied = SetClipboardData(CF_UNICODETEXT, mem) != nullptr;
            }
            if (!copied) GlobalFree(mem);
        }
        CloseClipboard();
    }
    const std::wstring noun = paths ? L"path" : L"name";
    const std::wstring said = !copied ? L"Could not copy - the clipboard is in use by another program."
                              : this_folder ? L"Copied this folder's path"
                                            : L"Copied " + std::to_wstring(items.size()) + L" " + noun + (items.size() == 1 ? L"" : L"s");
    bb_status_set(core_.app(), core::s(said));
}

void MainView::open_tree_menu(const std::wstring& folder, Point at) {
    ops::ComposedMenu composed = ops::compose_context_menu(*this, ops::MenuKind::Tree, folder);
    keep_menu_session(std::move(composed.keep));
    show_context_menu(std::move(composed.items), at);
}

void MainView::tree_verb(const std::wstring& id, const std::wstring& folder) {
    BbApp* app = core_.app();
    // The dialogs that read a folder take this one while they are built.
    auto dialog_on = [&](const wchar_t* kind) {
        dialog_folder_ = folder;
        open_dialog(kind);
        dialog_folder_.clear();
    };
    if (id == L"file.new-folder") dialog_on(L"new-folder");
    else if (id == L"file.new-file") dialog_on(L"new-file");
    else if (id == L"file.properties") dialog_on(L"properties");
    else if (id == L"tools.changes") dialog_on(L"changes");
    else if (id == L"tools.disk-usage") {
        tools_ui(*this).disk_root = folder;
        open_dialog(L"disk-usage-at");
    } else if (id == L"tools.duplicates") {
        if (shows_dialogs()) open_duplicates(folder);
    } else if (id == L"file.open-new-tab") {
        // Behind the one in view, as a middle-click does. Spec: ShellViewModel.OpenInNewTab (activate: false).
        uint64_t tab = 0;
        bb_tab_open(app, 0, core::s(folder), false, &tab);
    } else if (id == L"file.open-pane-right" || id == L"file.open-pane-below") {
        uint64_t pane = 0;
        bb_pane_split(app, 0, id == L"file.open-pane-right" ? BB_VERTICAL : BB_HORIZONTAL, core::s(folder), &pane);
    } else if (id == L"file.open-terminal" || id == L"file.open-vscode") {
        bb_launch_command_at(app, core::s(id), core::s(folder));
    } else if (id == L"file.bookmark") {
        bb_saved_do(app, 0, core::s(L"toggle-bookmark"), core::s(folder));
    } else if (id == L"file.delete") {
        dialog_on(L"delete");
    } else if (id == L"edit.copy-path") {
        copy_text({folder}, true, false);
    }
}


}  // namespace bb::views
