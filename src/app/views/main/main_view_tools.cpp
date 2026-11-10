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

Button* find_button(Widget& w, bool Button::*flag) {
    if (auto* b = dynamic_cast<Button*>(&w); b && b->*flag && b->flags.enabled && b->shown()) return b;
    for (const auto& c : w.children()) {
        if (Button* found = find_button(*c, flag)) return found;
    }
    return nullptr;
}

// The card's default button: the one marked default, else the accent-filled one, when it can be pressed.
Button* primary_button(Widget& w) {
    if (Button* b = find_button(w, &Button::is_default)) return b;
    return find_button(w, &Button::primary);
}

// What has the keyboard when a card opens: its first field (with whatever selection the card gave it),
// else its default button. Spec: each dialog's Loaded handler.
Widget* first_focus(Widget& w) {
    if (!w.shown() || !w.flags.enabled) return nullptr;
    if (auto* e = dynamic_cast<Edit*>(&w); e && e->flags.focusable && !e->read_only) return e;
    for (const auto& c : w.children()) {
        if (Widget* found = first_focus(*c)) return found;
    }
    return nullptr;
}

}  // namespace

// A dialog's keys, as a WPF dialog's IsCancel and IsDefault buttons had them: Esc closes it (closing is
// "no" wherever a question is waiting), Enter presses the default button unless the keyboard is in
// something that takes Enter itself. The search syntax card closes on any key. Spec: ThemedWindow.
bool MainView::dialog_key(const KeyEvent& e) {
    if (!dialog_open_ || !dialogs_ || (popup_ && popup_->menu_open())) return false;
    const bool plain = !e.mods.ctrl && !e.mods.alt && !e.mods.shift;
    const bool modifier = e.vk == VK_SHIFT || e.vk == VK_CONTROL || e.vk == VK_MENU || e.vk == VK_LWIN || e.vk == VK_RWIN;
    if (dialog_kind_ == L"search-syntax" && !modifier) {
        close_dialog();
        return true;
    }
    if (e.vk == VK_ESCAPE && plain) {
        close_dialog();
        return true;
    }
    if (e.vk == VK_RETURN && plain) {
        Widget* focused = window_.root().focused();
        if (auto* edit = dynamic_cast<Edit*>(focused); edit && (edit->multiline || edit->on_commit)) return false;
        if (dynamic_cast<Button*>(focused)) return false;
        if (focused && dynamic_cast<forms::ItemList*>(focused) && static_cast<forms::ItemList*>(focused)->on_activate) return false;
        if (Button* b = primary_button(*dialogs_)) {
            if (b->on_click) b->on_click();
            return true;
        }
    }
    return false;
}

namespace {

// One window per tool: disk usage pointed somewhere else, or checksums verifying instead of hashing, is the
// same window showing something else.
std::wstring tool_key(const std::wstring& kind) {
    if (kind == L"disk-usage-at") return L"disk-usage";
    if (kind == L"checksum-verify" || kind == L"checksum-show") return L"checksum";
    // A window's "-show" kind rebuilds it from the model it already has.
    if (kind.size() > 5 && kind.compare(kind.size() - 5, 5, L"-show") == 0) return kind.substr(0, kind.size() - 5);
    return kind;
}

}  // namespace

bool MainView::is_tool_window(const std::wstring& kind) {
    static const wchar_t* const kinds[] = {L"disk-usage",   L"disk-usage-at", L"disk-usage-show", L"duplicates", L"changes",      L"checksum",
                                           L"checksum-verify", L"checksum-show", L"compare-files", L"compare-files-show",
                                           L"changes-show", L"undo-history", L"transfer",
                                           L"theme-editor"};
    for (const wchar_t* k : kinds) {
        if (kind == k) return true;
    }
    return false;
}

ToolWindow* MainView::tool_window(const std::wstring& kind) {
    const auto it = tools_.find(tool_key(kind));
    return it == tools_.end() ? nullptr : it->second.get();
}

void MainView::close_tool(const std::wstring& kind) {
    const std::wstring key = tool_key(kind);
    if (key == L"changes") {
        KillTimer(window_.hwnd(), kChangesTimer);
        changes_signature_.clear();
    }
    tools_.erase(key);
}

std::vector<std::wstring> MainView::open_tool_windows() const {
    std::vector<std::wstring> out;
    for (const auto& kv : tools_) out.push_back(kv.first);
    return out;
}

void MainView::style_tool_window(ToolWindow& w) {
    const std::vector<uint32_t> argb = core_.palette();
    if (argb.size() != gfx::tok::Count) return;
    auto rgb = [&](gfx::Token t) {
        const uint32_t v = argb[t];
        return RGB((v >> 16) & 0xFF, (v >> 8) & 0xFF, v & 0xFF);
    };
    const uint32_t bg = argb[gfx::tok::WindowBackground];
    const int luma = (((bg >> 16) & 0xFF) * 299 + ((bg >> 8) & 0xFF) * 587 + (bg & 0xFF) * 114) / 1000;
    w.apply_theme(luma < 128, rgb(gfx::tok::TitleBarBorder), rgb(gfx::tok::TitleBarBackground), rgb(gfx::tok::TitleBarForeground));
}

// Compare files steps through the differences with F3 / Shift+F3 and Alt+Down / Alt+Up. Spec:
// FileCompareWindow.xaml (InputBindings).
bool MainView::tool_window_key(const std::wstring& key, const KeyEvent& e) {
    if (key != L"compare-files" || e.mods.ctrl) return false;
    int dir = 0;
    if (e.vk == VK_F3 && !e.mods.alt) dir = e.mods.shift ? -1 : 1;
    else if (e.mods.alt && !e.mods.shift && (e.vk == VK_DOWN || e.vk == VK_UP)) dir = e.vk == VK_DOWN ? 1 : -1;
    if (!dir) return false;
    try_tool_call(*this, "filecompare.step", "{\"direction\":" + std::to_string(dir) + "}");
    defer([this] { open_dialog(L"compare-files-show"); });
    return true;
}

void MainView::show_message(std::wstring title, std::wstring text) {
    message_ = {std::move(title), std::move(text)};
    defer([this] { open_dialog(L"message"); });
}

// Rebuilds a tool window that is already open, from something other than the person (progress, the index,
// a timer): the same as opening it again, minus taking the foreground.
void MainView::refresh_tool(const std::wstring& kind) {
    if (!tool_window(tool_key(kind))) return;
    refreshing_tool_ = true;
    open_dialog(kind);
    refreshing_tool_ = false;
}

void MainView::open_dialog(const std::wstring& kind) {
    if (!shows_dialogs() || !dialog_exists(kind)) return;
    if (is_tool_window(kind)) {
        const std::wstring key = tool_key(kind);
        auto card = build_dialog(kind, DialogContext{*this, [this, key] { defer([this, key] { close_tool(key); }); }});
        if (!card) return;
        if (key == L"theme-editor") theme_editor_id_ = try_model([&](BbOwnedStr* o) { return bb_theme_state(core_.app(), o); })["id"].str();
        if (ToolWindow* open = tool_window(key)) {
            open->replace(std::move(card));
            // Brought forward when somebody asked for it; a refresh behind their back never activates it.
            if (!refreshing_tool_) open->focus();
            return;
        }
        auto w = std::make_unique<ToolWindow>(*this, key, std::move(card));
        ToolWindow* raw = w.get();
        tools_[key] = std::move(w);
        raw->show();
        if (key == L"changes") SetTimer(window_.hwnd(), kChangesTimer, 2000, nullptr);
        return;
    }
    // A card rebuilt as itself (a tick in the sync preview, the live run) keeps its lists' places and what
    // had the keyboard; anything else starts fresh.
    auto base = [](std::wstring k) {
        for (const wchar_t* suffix : {L"-edit", L"-live"}) {
            const size_t n = wcslen(suffix);
            if (k.size() > n && k.compare(k.size() - n, n, suffix) == 0) k.resize(k.size() - n);
        }
        return k;
    };
    const bool again = dialog_open_ && base(dialog_kind_) == base(kind);
    tools::ListPlaces places;
    std::string focused_name;
    Widget* kept_focus = focus_before_dialog_;
    if (again) {
        places = tools::save_places(*dialogs_);
        if (Widget* f = window_.root().focused(); f && dialogs_->holds(f)) focused_name = f->name;
    }
    close_dialog();
    auto card = build_dialog(kind, DialogContext{*this, [this] { close_dialog(); }});
    if (!card) return;
    dialog_open_ = true;
    dialog_kind_ = kind;
    bb_ui_set_dialog_open(core_.app(), true);
    Widget* before = window_.root().focused();
    if (again) focus_before_dialog_ = kept_focus;
    else if (!dialogs_->holds(before)) focus_before_dialog_ = before;
    // The column picker is a popup where it was asked for (the pointer, or under a button); with no place to go
    // (a key, the palette) it sits under the title bar's left edge, over the list.
    if (kind == L"columns" || kind == L"default-columns") {
        const Rect area = dialogs_->bounds;
        dialogs_->show_popup(std::move(card), dialog_anchor_.value_or(Point{area.l + 40, area.t + 40}));
    } else {
        dialogs_->show(std::move(card));
    }
    if (again) tools::restore_places(*dialogs_, places);
    Widget* named = focused_name.empty() ? nullptr : dialogs_->find(focused_name);
    if (named) window_.root().set_focus(named);
    else if (Widget* f = first_focus(*dialogs_)) window_.root().set_focus(f);
    else if (Button* b = primary_button(*dialogs_)) window_.root().set_focus(b);
}

void MainView::open_new_file(const std::wstring& extension, const std::wstring& label, const std::wstring& folder) {
    new_file_template_ = {extension, label};
    dialog_folder_ = folder;
    open_dialog(L"new-file");
    dialog_folder_.clear();
    new_file_template_ = {};
}

void MainView::edit_saved_search(const std::wstring& name) {
    editing_saved_ = name;
    open_dialog(L"saved-search");
    editing_saved_.clear();
}

void MainView::rename_workspace(const std::wstring& name) {
    editing_saved_ = name;
    open_dialog(L"rename-workspace");
    editing_saved_.clear();
}

void MainView::reopen_on(const std::wstring& kind, std::vector<std::wstring> paths) {
    dialog_paths_ = std::move(paths);
    open_dialog(kind);
    dialog_paths_.clear();
}

void MainView::ask_delete(std::vector<std::wstring> paths, std::function<void(bool)> after) {
    dialog_paths_ = std::move(paths);
    after_delete_ = std::move(after);
    open_dialog(L"delete");
    dialog_paths_.clear();
    after_delete_ = nullptr;  // taken by the dialog when it was built
}

Widget* MainView::dialog_widget(std::string_view name) {
    if (dialog_open_ && dialogs_) {
        if (Widget* w = dialogs_->find(name)) return w;
    }
    for (auto& kv : tools_) {
        if (Widget* w = kv.second->find(name)) return w;
    }
    return nullptr;
}

bool MainView::focus_file_list() {
    TabView* tv = active_view();
    if (!tv) return false;
    Widget* target = tv->tiles_mode() ? static_cast<Widget*>(tv->tiles()) : static_cast<Widget*>(tv->list());
    if (!target) return false;
    window_.root().set_focus(target);
    return true;
}

void MainView::close_dialog() {
    if (dialogs_ && dialogs_->is_open()) dialogs_->close();
}

std::unique_ptr<gfx::BitmapSurface> MainView::render_dialog(const std::wstring& kind) {
    auto card = build_dialog(kind, DialogContext{*this, [] {}});
    if (!card) return nullptr;
    return render_dialog_detached(window_.root().resources(), std::move(card));
}

std::vector<SelectedRow> MainView::selected_rows() {
    std::vector<SelectedRow> out;
    if (!active_tab_) return out;
    const auto listing = core_.listing(active_tab_);
    if (!listing) return out;
    for (const uint32_t i : core_.selection(active_tab_)) {
        BbRow r{};
        if (!listing->row(i, r)) continue;
        SelectedRow s;
        s.name = core::w(r.name);
        s.path = core::w(r.full_path);
        s.dir = (r.flags & BB_ROW_DIRECTORY) != 0;
        s.size = r.size;
        s.modified = r.modified;
        s.created = r.created;
        out.push_back(std::move(s));
    }
    return out;
}

void MainView::sync_transfer() {
    queue_ = try_model([&](BbOwnedStr* o) { return bb_ops_query(core_.app(), core::s(L"queue"), core::s(L""), o); });
    const bool active = queue_["active"].boolean();
    const json::Value& v = queue_["view"];
    transfer_strip_->set_visible(active);
    transfer_head_->set_text(core::wide(v["headline"].str()));
    transfer_detail_->set_text(core::wide(v["detail"].str()));
    const bool paused = queue_["paused"].boolean();
    transfer_bar_->set_indeterminate(active && v["indeterminate"].boolean());
    if (!v["indeterminate"].boolean()) transfer_bar_->set_value(v["fraction"].num());
    transfer_waiting_->set_text(core::wide(queue_["waitingText"].str()));
    transfer_pause_->text = paused ? L"Resume" : L"Pause";
    transfer_pause_->style.font.weight = paused ? 600 : 400;
    transfer_pause_->tooltip = paused ? L"Carry on from where it stopped"
                                      : L"Hold this transfer where it is. Nothing else can be moved, renamed or deleted while it is paused.";
    // A run the queue does not host (a sync carries its own progress) offers neither.
    transfer_pause_->set_visible(queue_["canPause"].boolean() || queue_["canResume"].boolean());
    transfer_cancel_->set_enabled(queue_["canCancel"].boolean());
    browser_->request_layout();
    if (on_transfer_changed) on_transfer_changed();
    if (on_sync_progress) on_sync_progress();
    // The queue window goes when there is nothing left to show in it. Spec: MainWindow (TransferProgress null).
    if (!active && tool_window(L"transfer")) defer([this] { close_tool(L"transfer"); });
}

std::wstring MainView::transfer_text() const {
    if (!queue_["active"].boolean()) return {};
    const json::Value& v = queue_["view"];
    return core::wide(v["headline"].str()) + L"  " + core::wide(v["detail"].str());
}

void MainView::sync_questions(const wchar_t* kind) {
    if (headless_dialogs_) return;
    const json::Value q = try_model([&](BbOwnedStr* o) { return bb_ops_query(core_.app(), core::s(kind), core::s(L""), o); });
    if (q["pending"].is_null() || (dialog_open_ && dialog_kind_ == kind)) return;
    // A question parks a worker until it is answered, so it replaces whatever dialog is up - but never
    // another question, which would be answered "no" by being closed. A notice waits for any dialog, and
    // whatever is left waiting is asked again once that dialog has gone.
    const bool question_up = dialog_open_ && (dialog_kind_ == L"conflicts" || dialog_kind_ == L"elevation" || dialog_kind_ == L"confirm");
    if (question_up || (dialog_open_ && std::wstring(kind) == L"notice")) {
        questions_waiting_ = true;
        return;
    }
    open_dialog(kind);  // closes the one before it
}

void MainView::ensure_machine_properties() {
    if (machine_props_set_) return;
    // Read on a worker: the core says "loading" and lists the curated columns until the rest arrives, and the
    // Add column panel asks again until it has. Spec: ColumnAddPanel.LoadAsync.
    start_machine_property_read();
    machine_props_asked_ = true;
    const std::optional<std::string> list = take_machine_property_list();
    if (!list) return;
    machine_props_set_ = true;
    bb_columns_set_machine_properties(core_.app(), core::s(core::wide(*list)));
}

bool MainView::column_props_pending() {
    // A finished read is handed over here too, so a panel closed before it landed does not leave it waiting.
    if (machine_props_asked_ && !machine_props_set_ && machine_property_read_done()) ensure_machine_properties();
    return machine_props_asked_ && !machine_props_set_;
}

bool MainView::build_named_menu(const std::wstring& kind, MenuItems* out) {
    TabView* tv = active_view();
    if (!tv) return false;
    if (kind == L"columns") *out = tv->build_column_menu();
    else if (kind == L"files") *out = tv->build_files_menu(false);
    else if (kind == L"background") *out = tv->build_files_menu(true);
    else if (kind == L"tree") {
        std::wstring folder = tree_provider_->path_of(tree_->selected_id());
        if (folder.empty()) folder = core_.tab_text(active_tab_, BB_TEXT_PATH);
        ops::ComposedMenu composed = ops::compose_context_menu(*this, ops::MenuKind::Tree, folder);
        keep_menu_session(std::move(composed.keep));
        *out = std::move(composed.items);
    } else if (!ops::build_step_menu(*this, kind, out)) return false;
    return true;
}


}  // namespace bb::views
