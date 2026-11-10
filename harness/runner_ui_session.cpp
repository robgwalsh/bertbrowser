// The window-feature verbs, continued: layout and tabs, themes, dialogs and the folder comparison.
#include "runner.h"
#include "runner_ui_util.h"

#include <windows.h>

#include <algorithm>
#include <chrono>
#include <cwctype>
#include <filesystem>
#include <fstream>

#include "views/chrome/command_ui.h"
#include "views/dialogs/ops_menus.h"
#include "views/dialogs/dialogs.h"
#include "views/dialogs/forms.h"
#include "views/bridge/json.h"
#include "views/tools/tool_state.h"
#include "views/bridge/model.h"


namespace bbh {

namespace core = bb::core;
namespace ui = bb::ui;
namespace views = bb::views;
using namespace ui_util;

void Runner::install_ui_session() {
    // ---- layout, session, tabs ----
    verbs_[L"session"] = [this](const std::wstring&) {
        bool rebuilt = false;
        core::check(bb_session_roundtrip(s_.core().app(), &rebuilt), "session");
        s_.settle();
        s_.view().sync_layout();
        s_.settle();
    };
    verbs_[L"reopen"] = [this](const std::wstring&) {
        std::wstring reason;
        if (!s_.view().execute_command(L"tab.reopen", &reason)) throw AssertionError("there is no closed tab to reopen: " + to_utf8(reason));
        s_.settle();
    };
    verbs_[L"movetab"] = [this](const std::wstring& r) {
        const auto parts = words(r);
        if (parts.size() != 2) throw AssertionError("movetab takes two tab numbers: movetab <from> <to>.");
        const int from = number(parts[0], "movetab"), to = number(parts[1], "movetab");
        const auto ids = s_.view().tab_ids_in_active_pane();
        const int count = static_cast<int>(ids.size());
        if (from < 1 || from > count || to < 1 || to > count) {
            throw AssertionError("This pane has " + std::to_string(count) + " tab(s); cannot move tab " + std::to_string(from) + " to slot " + std::to_string(to) + ".");
        }
        core::check(bb_tab_move(s_.core().app(), ids[static_cast<size_t>(from) - 1], static_cast<uint32_t>(to - 1)), "movetab");
        s_.settle();
    };
    verbs_[L"tab-dragging"] = [this](const std::wstring& r) {
        const int gap = number(r, "tab-dragging");
        if (!s_.view().pose_tab_drag(gap)) throw AssertionError("This strip has no gap " + std::to_string(gap) + ".");
        s_.window().render();
    };
    verbs_[L"flat-cap"] = [this](const std::wstring& r) {
        const std::wstring text = require(r, "flat-cap");
        const int cap = number(text, "flat-cap");
        if (cap < 1) throw FormatError("flat-cap: expected a positive number, got '" + to_utf8(text) + "'.");
        core::check(bb_flat_set_cap(s_.core().app(), static_cast<uint32_t>(cap)), "flat-cap");
    };
}

void Runner::install_ui_themes() {
    // ---- themes ----
    auto theme_state = [this] { return views::model([&](BbOwnedStr* o) { return bb_theme_state(s_.core().app(), o); }); };
    auto theme_do = [this](const std::wstring& verb, const std::wstring& arg) {
        BbOwnedStr o{};
        core::check(bb_theme_do(s_.core().app(), core::s(verb), core::s(arg), &o), "theme");
        bb_str_free(o);
        s_.settle();
        s_.view().sync_overlays();
    };
    verbs_[L"theme"] = [this, theme_state, theme_do](const std::wstring& r) {
        const std::wstring id = require(r, "theme");
        const auto st = theme_state();
        std::string all;
        bool found = false;
        for (size_t i = 0; i < st["available"].size(); ++i) {
            all += (i ? ", " : "") + st["available"][i]["id"].str();
            found = found || core::iequals(core::wide(st["available"][i]["id"].str()), id);
        }
        if (!found) throw AssertionError("There is no theme '" + to_utf8(id) + "'. Available: " + all);
        theme_do(L"select", id);
    };
    verbs_[L"system-follow"] = [this, theme_do](const std::wstring& r) { theme_do(L"follow", on_off(r, "system-follow") ? L"on" : L"off"); };
    verbs_[L"system-slot"] = [this, theme_do](const std::wstring& r) {
        const auto parts = words(require(r, "system-slot"));
        if (parts.size() != 2 || (lower(parts[0]) != L"light" && lower(parts[0]) != L"dark")) throw FormatError("system-slot wants light|dark and a theme id.");
        theme_do(L"slot", lower(parts[0]) + L" " + parts[1]);
    };
    verbs_[L"system-theme"] = [this, theme_state](const std::wstring& r) {
        const std::wstring w = lower(require(r, "system-theme"));
        bool dark = theme_state()["appearance"]["dark"].boolean();
        bool hc = false;
        if (w == L"dark") dark = true;
        else if (w == L"light") dark = false;
        else if (w == L"high-contrast") hc = true;
        else throw FormatError("system-theme wants dark, light or high-contrast, got '" + to_utf8(w) + "'.");
        core::check(bb_theme_set_system_appearance(s_.core().app(), dark, hc), "system-theme");
        s_.settle();
        s_.view().sync_overlays();
    };
    verbs_[L"assert-theme"] = [theme_state](const std::wstring& r) {
        const std::wstring expected = require(r, "assert-theme");
        const std::wstring actual = core::wide(theme_state()["id"].str());
        if (!core::iequals(actual, expected)) throw AssertionError("expected the theme to be '" + to_utf8(expected) + "', it is '" + to_utf8(actual) + "'.");
    };
    verbs_[L"assert-following"] = [theme_state, this](const std::wstring& r) {
        const bool expected = on_off(require(r, "assert-following"), "assert-following");
        if (theme_state()["following"].boolean() != expected) {
            throw AssertionError(std::string("expected following Windows to be ") + (expected ? "on" : "off") + ".");
        }
    };
    auto placement = [this](const wchar_t* key) {
        return [this, key](const std::wstring& r) {
            const std::wstring v = lower(require(r, "placement"));
            if (v != L"sidebar" && v != L"titlebar" && v != L"hidden") throw FormatError("placement wants sidebar, titlebar or hidden, got '" + to_utf8(v) + "'.");
            const std::wstring json = L"\"" + (v == L"titlebar" ? std::wstring(L"titleBar") : v) + L"\"";
            if (bb_settings_set(s_.core().app(), core::s(std::wstring(key)), core::s(json)) != BB_OK) {
                std::wstring alt = v == L"sidebar" ? L"Sidebar" : v == L"titlebar" ? L"TitleBar" : L"Hidden";
                core::check(bb_settings_set(s_.core().app(), core::s(std::wstring(key)), core::s(L"\"" + alt + L"\"")), "placement");
            }
            s_.settle();
            s_.view().sync_overlays();
        };
    };
    verbs_[L"workspaces-placement"] = placement(L"workspacesPlacement");
    verbs_[L"saved-searches-placement"] = placement(L"savedSearchesPlacement");
}

void Runner::install_ui_dialogs() {
    // ---- dialogs and the transfer strip ----
    verbs_[L"dialog"] = [this](const std::wstring& r) {
        auto [kind, tail] = split_first(r);
        auto [name, ignored] = split_first(tail);
        (void)ignored;
        const std::wstring k = lower(kind);
        if (!bb::views::dialog_exists(k)) {
            throw FormatError("'" + to_utf8(kind) + "' is not a dialog. Try: " + join(bb::views::dialog_kinds(), ", ") + ".");
        }
        s_.settle(250);
        // Checksums read off the thread: building the window starts the run, and the photograph is of
        // what it arrives at.
        // Disk usage reads its folder off the thread too.
        const bool checksum = k == L"checksum" || k == L"checksum-verify";
        const bool disk = k == L"disk-usage" || k == L"disk-usage-at";
        const bool files = k == L"compare-files";
        // The Add column list reads the property system on a worker; the second section is what it proves.
        const bool columns = k == L"columns";
        if (checksum || disk || files || columns) {
            s_.view().render_dialog(k);
            s_.settle(250);
        }
        if (columns) s_.view().ensure_machine_properties();
        auto bitmap = s_.view().render_dialog(checksum ? std::wstring(L"checksum-show")
                                              : disk   ? std::wstring(L"disk-usage-show")
                                              : files  ? std::wstring(L"compare-files-show")
                                                       : k);
        if (!bitmap) throw AssertionError("the " + to_utf8(k) + " dialog could not be rendered.");
        write_shot(*bitmap, name.empty() ? L"dialog-" + k : name);
    };
    // live-dialogs on|off -- a run normally reports a dialog instead of putting it up; on, the next ones really
    // open, so a script can answer them with dialog-type and dialog-press like a person would.
    verbs_[L"live-dialogs"] = [this](const std::wstring& r) {
        s_.view().set_live_dialogs(on_off(r, "live-dialogs"));
    };
    // assert-dialog <kind> | none -- which dialog is up.
    verbs_[L"assert-dialog"] = [this](const std::wstring& r) {
        const std::wstring want = lower(require(r, "assert-dialog"));
        s_.settle();
        const bool up = s_.view().dialog_up();
        const std::wstring kind = up ? s_.view().dialog_kind() : L"none";
        if (kind != want) throw AssertionError("expected the " + to_utf8(want) + " dialog; " + (up ? "the " + to_utf8(kind) + " one is up." : "none is up."));
    };
    // dialog-type <Name> <text> -- replaces the text of the named box in the open dialog, as typing would.
    verbs_[L"dialog-type"] = [this](const std::wstring& r) {
        auto [name, text] = split_first(require(r, "dialog-type"));
        auto* edit = dynamic_cast<bb::ui::Edit*>(s_.view().dialog_widget(to_utf8(name)));
        if (!edit) throw AssertionError("The open dialog has no box named '" + to_utf8(name) + "'.");
        edit->set_text(text);
        if (edit->on_changed) edit->on_changed(edit->text());
        s_.settle();
    };
    // dialog-press <Name> -- presses the named button of the open dialog; refused when it is disabled.
    verbs_[L"dialog-press"] = [this](const std::wstring& r) {
        const std::wstring name = require(r, "dialog-press");
        auto* button = dynamic_cast<bb::ui::Button*>(s_.view().dialog_widget(to_utf8(name)));
        if (!button) throw AssertionError("The open dialog has no button named '" + to_utf8(name) + "'.");
        if (!button->flags.enabled) throw AssertionError("'" + to_utf8(name) + "' is disabled.");
        if (button->on_click) button->on_click();
        s_.settle();
    };
    // assert-window <kind> | none -- which tool windows are open (disk-usage, duplicates, changes, checksum,
    // compare-files, undo-history, transfer, theme-editor). They are windows of their own, so the folders
    // stay live while they are up.
    verbs_[L"assert-window"] = [this](const std::wstring& r) {
        const std::wstring want = lower(require(r, "assert-window"));
        s_.settle();
        const auto open = s_.view().open_tool_windows();
        std::string list;
        for (const auto& k : open) list += (list.empty() ? "" : ", ") + to_utf8(k);
        if (want == L"none") {
            if (!open.empty()) throw AssertionError("expected no tool window; open: " + list + ".");
            return;
        }
        if (!s_.view().tool_window(want)) throw AssertionError("the " + to_utf8(want) + " window is not open" + (list.empty() ? "." : "; open: " + list + "."));
    };
    // window-shot <kind> [name] -- PNG of an open tool window, caption and all.
    verbs_[L"window-shot"] = [this](const std::wstring& r) {
        auto [kind, name] = split_first(require(r, "window-shot"));
        s_.settle(250);
        bb::views::ToolWindow* w = s_.view().tool_window(lower(kind));
        if (!w) throw AssertionError("the " + to_utf8(kind) + " window is not open.");
        auto& win = w->window();
        win.root().invalidate_all();
        win.render();
        if (!win.bitmap()) throw AssertionError("the " + to_utf8(kind) + " window is not drawing into a bitmap.");
        write_shot(*win.bitmap(), name.empty() ? L"window-" + lower(kind) : name);
    };
    // window-close <kind> -- closes a tool window as its caption's close button would.
    verbs_[L"window-close"] = [this](const std::wstring& r) {
        const std::wstring kind = lower(require(r, "window-close"));
        if (!s_.view().tool_window(kind)) throw AssertionError("the " + to_utf8(kind) + " window is not open.");
        PostMessageW(s_.view().tool_window(kind)->window().hwnd(), WM_CLOSE, 0, 0);
        s_.settle();
    };
    // dialog-tick <Name> on|off -- sets a named box of the open dialog (or tool window), as a click would.
    verbs_[L"dialog-tick"] = [this](const std::wstring& r) {
        auto [name, state] = split_first(require(r, "dialog-tick"));
        auto* box = dynamic_cast<bb::ui::CheckBox*>(s_.view().dialog_widget(to_utf8(name)));
        if (!box) throw AssertionError("The open dialog has no box named '" + to_utf8(name) + "'.");
        if (!box->flags.enabled) throw AssertionError("'" + to_utf8(name) + "' is disabled.");
        const bool on = on_off(state, "dialog-tick");
        box->set_checked(on);
        if (box->on_toggle) box->on_toggle(on);
        s_.settle();
    };
    // dialog-select <Name> <text> -- selects the first row of a named list in the open dialog or tool window whose
    // text contains this, as a click would.
    verbs_[L"dialog-select"] = [this](const std::wstring& r) {
        auto [name, want] = split_first(require(r, "dialog-select"));
        auto* list = dynamic_cast<bb::views::forms::ItemList*>(s_.view().dialog_widget(to_utf8(name)));
        if (!list) throw AssertionError("The open dialog has no list named '" + to_utf8(name) + "'.");
        for (size_t i = 0; i < list->entries.size(); ++i) {
            const auto& e = list->entries[i];
            if (e.heading || !core::icontains(e.text, want)) continue;
            list->select(static_cast<int>(i), true);
            s_.settle();
            return;
        }
        throw AssertionError("No row of '" + to_utf8(name) + "' says '" + to_utf8(want) + "'.");
    };
    // dialog-commit <Name> <text> -- types into a named box and presses Enter.
    verbs_[L"dialog-commit"] = [this](const std::wstring& r) {
        auto [name, text] = split_first(require(r, "dialog-commit"));
        auto* edit = dynamic_cast<bb::ui::Edit*>(s_.view().dialog_widget(to_utf8(name)));
        if (!edit) throw AssertionError("The open dialog has no box named '" + to_utf8(name) + "'.");
        edit->set_text(text);
        if (edit->on_changed) edit->on_changed(edit->text());
        if (edit->on_commit) edit->on_commit();
        s_.settle();
    };
    // assert-dialog-text <Name> <text> -- the named text of the open dialog contains this.
    verbs_[L"assert-dialog-text"] = [this](const std::wstring& r) {
        auto [name, want] = split_first(require(r, "assert-dialog-text"));
        auto* text = dynamic_cast<bb::ui::TextBlock*>(s_.view().dialog_widget(to_utf8(name)));
        if (!text) throw AssertionError("The open dialog has no text named '" + to_utf8(name) + "'.");
        if (text->text().find(want) == std::wstring::npos)
            throw AssertionError("'" + to_utf8(name) + "' says '" + to_utf8(text->text()) + "', not '" + to_utf8(want) + "'.");
    };
    verbs_[L"duplicates"] = [this](const std::wstring& r) {
        const fs::path path = r.empty() ? fs::path(s_.core().tab_text(tab(), BB_TEXT_PATH)) : sandbox_.resolve(r);
        std::error_code ec;
        if (!fs::is_directory(path, ec)) throw AssertionError("There is no folder at '" + to_utf8(path.wstring()) + "' to search.");
        // The window's own way in - pointed at the folder, then Scan - with the floor at one byte so the
        // sandbox's small files count. The scan runs off the UI thread; settle waits for it.
        const std::wstring root = core::wide(bb::views::json_quote(path.wstring()));
        auto call = [this](const wchar_t* verb, const std::wstring& args) {
            BbOwnedStr o{};
            core::check(bb_tools(s_.core().app(), core::s(verb), core::s(args), &o), "duplicates");
            bb_str_free(o);
        };
        call(L"duplicates.load", L"{\"root\":" + root + L"}");
        call(L"duplicates.scan", L"{\"root\":" + root + L",\"minSize\":1}");
        s_.settle();
        s_.view().sync_duplicates();
        auto& d = s_.view().duplicates();
        if (d.result["scanning"].boolean()) throw AssertionError("the duplicate scan did not finish.");
        if (o_.verbose) say("# " + std::to_string(d.result["groups"].size()) + " groups, " + std::to_string(static_cast<int64_t>(d.result["wastedBytes"].num())) + " bytes reclaimable");
    };
    verbs_[L"duplicates-keep"] = [this](const std::wstring& r) {
        auto& d = s_.view().duplicates();
        if (!d.has) throw AssertionError("duplicates-keep needs a scan: run 'duplicates' first.");
        const std::string strategy = to_utf8(lower(trim(r)));
        if (strategy != "newest" && strategy != "oldest" && strategy != "shallowest") {
            throw FormatError("'" + strategy + "' is not a keep strategy. Try: newest, oldest, shallowest.");
        }
        d.ticked.clear();
        const auto& groups = d.result["groups"];
        for (size_t g = 0; g < groups.size(); ++g) {
            const size_t keep = static_cast<size_t>(groups[g]["keep"][strategy].i64());
            for (size_t i = 0; i < groups[g]["files"].size(); ++i) {
                if (i != keep) d.ticked.insert({g, i});
            }
        }
        s_.settle();
    };
}

void Runner::install_ui_compare() {
    // ---- compare ----
    verbs_[L"compare"] = [this](const std::wstring&) {
        core::check(bb_compare_toggle(s_.core().app()), "compare");
        s_.settle();
    };
    verbs_[L"compare-end"] = [this](const std::wstring&) {
        const auto st = views::model([&](BbOwnedStr* o) { return bb_command_state(s_.core().app(), core::s(L"tools.compare"), o); });
        if (st["checked"].boolean()) core::check(bb_compare_toggle(s_.core().app()), "compare-end");
        s_.settle();
    };
}
}  // namespace bbh
