// The window-feature verbs, continued: commands and keys, the settings page and the command palette.
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

void Runner::install_ui_keys() {
    // ---- commands and keys ----
    auto chord_and_focus = [](const std::wstring& rest, uint32_t* mods, std::wstring* key, BbFocusKind* focus) {
        BbFocusKind f = BB_FOCUS_FILE_LIST;
        std::wstring chord = rest;
        const size_t at = lower(rest).find(L" in ");
        if (at != std::wstring::npos) {
            chord = rest.substr(0, at);
            f = focus_word(rest.substr(at + 4));
        }
        if (!parse_chord(chord, mods, key)) throw FormatError("'" + to_utf8(chord) + "' is not a key chord.");
        *focus = f;
    };
    verbs_[L"key"] = [this, chord_and_focus](const std::wstring& r) {
        uint32_t mods;
        std::wstring key;
        BbFocusKind focus;
        chord_and_focus(require(r, "key"), &mods, &key, &focus);
        const auto res = views::model([&](BbOwnedStr* o) {
            return bb_key_press(s_.core().app(), BbKeyChord{mods, core::s(key)}, BbFocusState{focus, s_.view().focus_flags()}, false, o);
        });
        if (!res["claimed"].boolean()) {
            say("KEY " + to_utf8(r) + " -> nothing");
            return;
        }
        const std::string cmd = res["command"].str();
        say("KEY " + to_utf8(r) + " -> " + cmd);
        const std::string result = res["result"].str();
        if (result == "unavailable") say("UNAVAILABLE " + cmd + " - " + res["reason"].str());
        else if (result == "ui" && !s_.view().host_command(core::wide(cmd))) say("WOULD-RUN " + cmd);
        s_.settle();
    };
    // window-key <chord> -- posts the key to the window, as the keyboard would: down and up, with its
    // modifiers held. Unlike `key` it goes through the window's own handling first (a dialog's Esc and
    // Enter, a focused box), not straight to the dispatcher.
    verbs_[L"window-key"] = [this](const std::wstring& r) {
        uint32_t mods = 0;
        std::wstring key;
        if (!parse_chord(require(r, "window-key"), &mods, &key)) throw FormatError("'" + to_utf8(r) + "' is not a key chord.");
        unsigned vk = 0;
        for (unsigned v = 1; v < 256 && !vk; ++v) {
            if (bb::views::chord_key_name(v) == key) vk = v;
        }
        if (!vk) throw FormatError("'" + to_utf8(key) + "' is not a key the window knows.");
        const HWND hwnd = s_.view().window().hwnd();
        const std::vector<unsigned> held = [&] {
            std::vector<unsigned> h;
            if (mods & 1) h.push_back(VK_CONTROL);
            if (mods & 2) h.push_back(VK_MENU);
            if (mods & 4) h.push_back(VK_SHIFT);
            return h;
        }();
        for (const unsigned m : held) PostMessageW(hwnd, m == VK_MENU ? WM_SYSKEYDOWN : WM_KEYDOWN, m, 0);
        PostMessageW(hwnd, (mods & 2) ? WM_SYSKEYDOWN : WM_KEYDOWN, vk, 0);
        PostMessageW(hwnd, (mods & 2) ? WM_SYSKEYUP : WM_KEYUP, vk, 0xC0000000);
        for (auto it = held.rbegin(); it != held.rend(); ++it) PostMessageW(hwnd, *it == VK_MENU ? WM_SYSKEYUP : WM_KEYUP, *it, 0xC0000000);
        s_.pump();
        s_.settle();
    };
    verbs_[L"assert-key"] = [this](const std::wstring& r) {
        const auto parts = words(require(r, "assert-key"));
        if (parts.size() != 3) throw FormatError("assert-key wants <chord> <list|text|readonly|other> <command id|none>.");
        uint32_t mods;
        std::wstring key;
        if (!parse_chord(parts[0], &mods, &key)) throw FormatError("'" + to_utf8(parts[0]) + "' is not a key chord.");
        const std::wstring actual = [&] {
            const std::wstring c = s_.core().dispatch(mods, key, focus_word(parts[1]), s_.view().focus_flags());
            return c.empty() ? std::wstring(L"none") : c;
        }();
        if (!core::iequals(actual, parts[2])) {
            throw AssertionError(to_utf8(parts[0]) + " with focus in '" + to_utf8(parts[1]) + "' runs " + to_utf8(actual) + ", not " + to_utf8(parts[2]) + ".");
        }
    };
    verbs_[L"run"] = [this](const std::wstring& r) {
        const std::wstring id = require(r, "run");
        BbOwnedStr state{};
        if (bb_command_state(s_.core().app(), core::s(id), &state) != BB_OK) throw FormatError("'" + to_utf8(id) + "' is not a command.");
        bb_str_free(state);
        std::wstring reason;
        bool wanted_person = false;
        if (!s_.view().execute_command(id, &reason, &wanted_person)) {
            say("UNAVAILABLE " + to_utf8(id) + " - " + to_utf8(reason));
            return;
        }
        if (wanted_person) say("WOULD-RUN " + to_utf8(id));
        s_.settle();
    };
    verbs_[L"assert-command"] = [this](const std::wstring& r) {
        auto [id, tail] = split_first(require(r, "assert-command"));
        auto [expected, text] = split_first(tail);
        const std::wstring e = lower(expected);
        if (e != L"available" && e != L"unavailable") throw FormatError("assert-command wants <id> available|unavailable [reason].");
        BbOwnedStr o{};
        core::check(bb_command_why_not(s_.core().app(), core::s(id), &o), "assert-command");
        const std::wstring reason = core::take(o);
        const bool available = reason.empty();
        if (e == L"available" && !available) throw AssertionError(to_utf8(id) + " is unavailable: " + to_utf8(reason));
        if (e == L"unavailable" && available) throw AssertionError(to_utf8(id) + " is available.");
        if (e == L"unavailable" && !text.empty() && !core::icontains(reason, text)) {
            throw AssertionError(to_utf8(id) + " is unavailable because \"" + to_utf8(reason) + "\", which does not mention '" + to_utf8(text) + "'.");
        }
    };
    verbs_[L"bind"] = [this](const std::wstring& r) {
        auto [id, tail] = split_first(require(r, "bind"));
        if (tail.empty()) throw FormatError("bind wants <command id> <chord>... or <command id> none.");
        std::vector<std::wstring> chords;
        if (lower(tail) != L"none") chords = words(tail);
        std::vector<BbStr> views_;
        for (const auto& c : chords) views_.push_back(core::s(c));
        core::check(bb_keymap_set_chords(s_.core().app(), core::s(id), views_.data(), static_cast<uint32_t>(views_.size())), "bind");
        s_.settle();
    };
    verbs_[L"unbind"] = [this](const std::wstring& r) {
        core::check(bb_keymap_reset(s_.core().app(), core::s(r)), "unbind");
        s_.settle();
    };
    verbs_[L"assert-gesture"] = [this](const std::wstring& r) {
        auto [id, expected] = split_first(require(r, "assert-gesture"));
        BbOwnedStr o{};
        core::check(bb_keymap_gesture(s_.core().app(), core::s(id), &o), "assert-gesture");
        const std::wstring actual = core::take(o);
        const std::wstring want = lower(expected) == L"none" ? std::wstring() : expected;
        if (!core::iequals(actual, want)) throw AssertionError(to_utf8(id) + " shows '" + to_utf8(actual) + "', not '" + to_utf8(want) + "'.");
    };
    verbs_[L"assert-tooltip"] = [this](const std::wstring& r) {
        auto [name, text] = split_first(require(r, "assert-tooltip"));
        s_.view().refresh_tooltips();
        bb::ui::Widget* w = s_.view().find_named(to_utf8(name));
        if (!w) throw AssertionError("there is no element named " + to_utf8(name) + ".");
        if (!core::icontains(w->tooltip, text)) {
            throw AssertionError(to_utf8(name) + "'s tooltip is \"" + to_utf8(w->tooltip) + "\", which does not contain '" + to_utf8(text) + "'.");
        }
    };
}

void Runner::install_ui_settings() {
    // ---- the settings page ----
    verbs_[L"settings"] = [this](const std::wstring& r) {
        auto [page, tail] = split_first(r);
        BbApp* a = s_.core().app();
        if (page.empty()) {
            throw FormatError("settings needs a page (general, appearance, preview, search-index, history, new-items, columns, "
                              "saved-searches, workspaces, context-menu, command-bars, keyboard) or close.");
        }
        const std::wstring p = lower(page);
        auto need_open = [this](const char* what) {
            s_.view().sync_overlays();
            if (!s_.view().settings()->is_open()) throw AssertionError(std::string(what) + " needs the settings page open.");
        };
        if (p == L"tick") {
            need_open("settings tick");
            const size_t split = tail.rfind(L' ');
            const std::wstring state = split == std::wstring::npos ? L"" : tail.substr(split + 1);
            if (state != L"on" && state != L"off") throw FormatError("settings tick wants a label and on|off, e.g. 'settings tick Show hidden items on'.");
            const std::wstring label = trim(tail.substr(0, split));
            bb::ui::CheckBox* box = s_.view().settings()->find_check(label);
            if (!box) throw AssertionError("There is no box labelled '" + to_utf8(label) + "' on this page.");
            box->set_checked(state == L"on");
            if (box->on_toggle) box->on_toggle(state == L"on");
            s_.settle();
            return;
        }
        // settings search-audit -- every page has something the search box finds, and shows every label the
        // box would point at. Opens each page; the last one stays open.
        if (p == L"search-audit") {
            BbOwnedStr index_out{}, state_out{};
            core::check(bb_settings_search_index(a, &index_out), "settings search-audit");
            core::check(bb_settings_page_state(a, &state_out), "settings search-audit");
            const auto index = bb::json::parse(to_utf8(core::take(index_out)));
            const auto state = bb::json::parse(to_utf8(core::take(state_out)));
            std::vector<std::string> problems;
            for (size_t i = 0; i < state["pages"].size(); ++i) {
                const std::string id = state["pages"][i]["id"].str();
                core::check(bb_settings_page_open(a, core::s(core::wide(id))), "settings search-audit");
                s_.settle();
                s_.view().sync_overlays();
                size_t found = 0;
                for (size_t k = 0; k < index.size(); ++k) {
                    if (index[k]["page"].str() != id) continue;
                    ++found;
                    if (!s_.view().settings()->shows_words(core::wide(index[k]["label"].str()))) problems.push_back(id + ": '" + index[k]["label"].str() + "'");
                }
                if (!found) problems.push_back(id + ": nothing indexed");
            }
            if (!problems.empty()) {
                std::string all;
                for (const std::string& problem : problems) all += (all.empty() ? "" : ", ") + problem;
                throw AssertionError("The settings search index does not match the pages: " + all);
            }
            return;
        }
        if (p == L"key") {
            need_open("settings key");
            auto [verb, arg] = split_first(tail);
            const std::wstring v = lower(verb);
            if (v == L"press") {
                const auto m = views::model([&](BbOwnedStr* o) { return bb_keys_page(a, o); });
                if (!m["capturing"].boolean() && !m["recording"].boolean()) throw AssertionError("The Keyboard page is not listening for a key.");
            }
            static const std::vector<std::wstring> known = {L"select", L"filter", L"change", L"add", L"remove", L"reset", L"reset-all", L"reassign", L"find", L"cancel", L"press"};
            if (std::find(known.begin(), known.end(), v) == known.end()) {
                throw FormatError("settings key does not know '" + to_utf8(verb) + "'. Try: select, filter, change, add, press, reassign, remove, reset, reset-all, find, cancel.");
            }
            core::check(bb_keys_do(a, core::s(v), core::s(require(v == L"select" || v == L"press" ? arg : arg.empty() ? L" " : arg, "settings key"))), "settings key");
            s_.settle();
            s_.view().sync_overlays();
            return;
        }
        if (p == L"bars") {
            need_open("settings bars");
            auto [verb, arg] = split_first(tail);
            const std::wstring v = lower(verb);
            if (v == L"rows") {
                say("BAR-ROWS " + join(s_.view().settings()->bar_rows(), " | "));
                return;
            }
            if (v == L"choices") {
                say("BAR-CHOICES " + join(s_.view().settings()->bar_choices(), " | "));
                return;
            }
            std::wstring a2 = arg;
            if (v == L"select" && lower(arg) == L"menu-bar") a2 = L"menu-bar";
            core::check(bb_barpage_do(a, core::s(v), core::s(a2)), "settings bars");
            s_.settle();
            s_.view().sync_overlays();
            return;
        }
        if (p == L"close") {
            core::check(bb_settings_page_close(a), "settings close");
            s_.settle();
            s_.view().sync_overlays();
            return;
        }
        // A page by name.
        BbOwnedStr st{};
        core::check(bb_settings_page_state(a, &st), "settings");
        const auto state = bb::json::parse(to_utf8(core::take(st)));
        bool known = false;
        for (size_t i = 0; i < state["pages"].size(); ++i) known = known || core::wide(state["pages"][i]["id"].str()) == p;
        if (!known) throw FormatError("'" + to_utf8(page) + "' is not a settings page.");
        if (state["open"].boolean()) bb_settings_page_close(a);
        core::check(bb_settings_page_open(a, core::s(p)), "settings");
        s_.settle();
        s_.view().sync_overlays();
        if (lower(tail) == L"dragging") s_.view().settings()->pose_column_drag(true);
        else s_.view().settings()->pose_column_drag(false);
        if (lower(tail) == L"new-command") s_.view().settings()->open_new_command();
        s_.settle();
    };
    verbs_[L"assert-bar-rows"] = [this](const std::wstring& r) {
        s_.view().sync_overlays();
        if (!s_.view().settings()->is_open()) throw AssertionError("assert-bar-rows needs the settings page open.");
        const auto expected = bar_split(require(r, "assert-bar-rows"));
        const auto actual = s_.view().settings()->bar_rows();
        bool same = expected.size() == actual.size();
        for (size_t i = 0; same && i < actual.size(); ++i) same = core::iequals(actual[i], expected[i]);
        if (!same) throw AssertionError("The page lists " + join(actual, " | ") + ", not " + join(expected, " | ") + ".");
    };
    auto bar_choice = [this](const std::wstring& r, bool expected) {
        s_.view().sync_overlays();
        const std::wstring name = require(r, expected ? "assert-bar-choice" : "assert-no-bar-choice");
        if (!s_.view().settings()->is_open()) throw AssertionError("This needs the settings page open.");
        bool offered = false;
        for (const auto& c : s_.view().settings()->bar_choices()) offered = offered || core::iequals(c, name);
        if (offered != expected) throw AssertionError(expected ? "The picker does not offer '" + to_utf8(name) + "'." : "The picker offers '" + to_utf8(name) + "'.");
    };
    verbs_[L"assert-bar-choice"] = [bar_choice](const std::wstring& r) { bar_choice(r, true); };
    verbs_[L"assert-no-bar-choice"] = [bar_choice](const std::wstring& r) { bar_choice(r, false); };
    verbs_[L"assert-key-recorder"] = [this](const std::wstring& r) {
        const std::wstring text = require(r, "assert-key-recorder");
        const std::wstring actual = s_.view().settings()->recorder_text();
        if (!core::icontains(actual, text)) throw AssertionError("the recorder says \"" + to_utf8(actual) + "\", which does not contain '" + to_utf8(text) + "'.");
    };
    verbs_[L"assert-key-row"] = [this](const std::wstring& r) {
        const std::wstring rest = require(r, "assert-key-row");
        const size_t split = rest.rfind(L' ');
        if (split == std::wstring::npos) throw FormatError("assert-key-row wants <command name> <chords|none|absent>.");
        const std::wstring name = trim(rest.substr(0, split)), expected = trim(rest.substr(split + 1));
        std::vector<std::wstring> chords;
        const bool found = s_.view().settings()->key_row(name, &chords);
        if (lower(expected) == L"absent") {
            if (found) throw AssertionError("'" + to_utf8(name) + "' is listed, and should not be.");
            return;
        }
        if (!found) throw AssertionError("the Keyboard page lists no command called '" + to_utf8(name) + "'.");
        std::wstring actual;
        for (size_t i = 0; i < chords.size(); ++i) actual += (i ? L"," : L"") + chords[i];
        if (actual.empty()) actual = L"none";
        if (!core::iequals(actual, expected)) throw AssertionError("'" + to_utf8(name) + "' shows " + to_utf8(actual) + ", not " + to_utf8(expected) + ".");
    };
}

void Runner::install_ui_palette() {
    // ---- the command palette ----
    auto palette_state = [this] {
        return views::model([&](BbOwnedStr* o) { return bb_palette_state(s_.core().app(), o); });
    };
    auto palette_rows = [palette_state] {
        std::vector<std::wstring> out;
        const auto st = palette_state();
        for (size_t i = 0; i < st["rows"].size(); ++i) {
            const auto& r = st["rows"][i];
            const std::string kind = r["kind"].str();
            out.push_back(kind == "heading" ? L"# " + core::wide(r["text"].str()) : kind == "category" ? core::wide(r["token"].str()) : core::wide(r["text"].str()));
        }
        return out;
    };
    verbs_[L"palette"] = [this, palette_state, palette_rows](const std::wstring& r) {
        auto [verb, tail] = split_first(r);
        const std::wstring v = lower(verb);
        BbApp* a = s_.core().app();
        auto pdo = [&](const wchar_t* vb, const std::wstring& arg) {
            BbOwnedStr o{};
            core::check(bb_palette_do(a, core::s(vb), core::s(arg), &o), "palette");
            return core::take(o);
        };
        if (v.empty() || v == L"open") {
            if (palette_state()["open"].boolean()) pdo(L"close", L"");
            core::check(bb_palette_open(a, core::s(tail)), "palette open");
        } else if (v == L"close") {
            pdo(L"close", L"");
        } else if (v == L"type") {
            pdo(L"type", tail);
        } else if (v == L"clear") {
            pdo(L"type", L"");
        } else if (v == L"down") {
            pdo(L"down", tail.empty() ? L"1" : std::to_wstring(number(tail, "palette down")));
        } else if (v == L"up") {
            pdo(L"up", tail.empty() ? L"1" : std::to_wstring(number(tail, "palette up")));
        } else if (v == L"tab") {
            pdo(L"complete", L"");
        } else if (v == L"pin") {
            pdo(L"pin", L"");
        } else if (v == L"shortcut") {
            pdo(L"shortcut", L"");
        } else if (v == L"pick") {
            const auto st = palette_state();
            std::string id = "nothing";
            for (size_t i = 0; i < st["rows"].size(); ++i) {
                if (st["rows"][i]["selected"].boolean()) id = st["rows"][i]["id"].str().empty() ? st["rows"][i]["text"].str() : st["rows"][i]["id"].str();
            }
            const auto res = bb::json::parse(to_utf8(pdo(L"pick", L"")));
            if (res["ui"].boolean() && !s_.view().host_command(core::wide(res["command"].str()))) {
                say("WOULD-RUN " + res["command"].str());
            } else {
                say("PICK " + id);
            }
        } else if (v == L"rows") {
            say("PALETTE " + join(palette_rows(), " | "));
            return;
        } else {
            throw FormatError("palette does not know '" + to_utf8(verb) + "'. Try: open, type, clear, down, up, tab, pin, shortcut, pick, rows, close.");
        }
        s_.settle();
        s_.view().sync_overlays();
    };
    auto palette_row_assert = [palette_rows](const std::wstring& r, bool expected) {
        const std::wstring text = require(r, expected ? "assert-palette-row" : "assert-no-palette-row");
        const auto rows = palette_rows();
        bool actual = false;
        for (const auto& x : rows) actual = actual || core::icontains(x, text);
        if (actual != expected) {
            throw AssertionError(expected ? "no palette row contains '" + to_utf8(text) + "'. Rows: " + join(rows, " | ")
                                          : "a palette row contains '" + to_utf8(text) + "', and none should.");
        }
    };
    verbs_[L"assert-palette-row"] = [palette_row_assert](const std::wstring& r) { palette_row_assert(r, true); };
    verbs_[L"assert-no-palette-row"] = [palette_row_assert](const std::wstring& r) { palette_row_assert(r, false); };
    verbs_[L"assert-palette-order"] = [palette_rows](const std::wstring& r) {
        const auto wanted = bar_split(require(r, "assert-palette-order"));
        const auto rows = palette_rows();
        size_t at = 0;
        for (const auto& want : wanted) {
            size_t found = rows.size();
            for (size_t i = at; i < rows.size(); ++i) {
                if (core::icontains(rows[i], want)) {
                    found = i;
                    break;
                }
            }
            if (found == rows.size()) throw AssertionError("'" + to_utf8(want) + "' does not come after the rows before it. Rows: " + join(rows, " | "));
            at = found + 1;
        }
    };
    verbs_[L"assert-palette-selected"] = [palette_state](const std::wstring& r) {
        const std::wstring text = require(r, "assert-palette-selected");
        const auto st = palette_state();
        std::wstring selected;
        for (size_t i = 0; i < st["rows"].size(); ++i) {
            if (st["rows"][i]["selected"].boolean()) selected = core::wide(st["rows"][i]["text"].str());
        }
        if (!core::icontains(selected, text)) throw AssertionError("the selected row is '" + to_utf8(selected) + "', not '" + to_utf8(text) + "'.");
    };
    verbs_[L"assert-palette-hint"] = [palette_state](const std::wstring& r) {
        const std::wstring text = require(r, "assert-palette-hint");
        const std::wstring hint = core::wide(palette_state()["hint"].str());
        if (!core::icontains(hint, text)) throw AssertionError("the palette's hint is \"" + to_utf8(hint) + "\", which does not contain '" + to_utf8(text) + "'.");
    };
    verbs_[L"assert-palette-open"] = [palette_state](const std::wstring&) {
        if (!palette_state()["open"].boolean()) throw AssertionError("the palette is not open.");
    };
    verbs_[L"assert-palette-closed"] = [palette_state](const std::wstring&) {
        if (palette_state()["open"].boolean()) throw AssertionError("the palette is open.");
    };
}
}  // namespace bbh
