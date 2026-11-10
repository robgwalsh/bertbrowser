// The window-feature verbs: menu bar and toolbars, commands and keys, the palette, settings, columns,
// themes, saved searches, the index banner, dialogs. Each does what the C# ScriptRunner's verb does, over
// the same core models the views draw from, and asserts on what the views are showing.
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

struct Runner::UiState {
    ui::MenuItems last_menu;
};


void Runner::shot_menu(const ui::MenuItems& items, const std::wstring& name) {
    auto bitmap = ui::render_menu_detached(s_.resources(), items);
    if (!bitmap) throw std::runtime_error("the menu could not be rendered");
    write_shot(*bitmap, name);
}

void Runner::write_shot(bb::gfx::BitmapSurface& bitmap, const std::wstring& name) {
    ++shots_;
    std::wstring file = name;
    if (file.size() < 4 || _wcsicmp(file.c_str() + file.size() - 4, L".png") != 0) file += L".png";
    const fs::path path = fs::path(file).is_absolute() ? fs::path(file) : o_.out_dir / file;
    if (path.has_parent_path()) fs::create_directories(path.parent_path());
    if (bitmap.is_flat()) throw AssertionError(to_utf8(path.wstring()) + " is a single flat colour - it rendered nothing.");
    if (!bitmap.write_png(path.wstring(), nullptr)) throw std::runtime_error("could not write " + to_utf8(path.wstring()));
    say("SHOT " + to_utf8(path.wstring()));
}

void Runner::install_ui() {
    ui_ = std::make_shared<UiState>();
    install_ui_dump();
    install_ui_bars();
    install_ui_menus();
    install_ui_keys();
    install_ui_settings();
    install_ui_palette();
    install_ui_saved();
    install_ui_columns();
    install_ui_fixtures();
    install_ui_banner();
    install_ui_session();
    install_ui_themes();
    install_ui_dialogs();
    install_ui_compare();
}

void Runner::install_ui_dump() {
    // A native-only aid, not in the C# harness: `dump <model> [arg]` writes the JSON a model call answers to
    // <out>/dump.json (commands, palette, keys, settings, columns, saved, theme, index, tiles, cands, or a
    // bb_bars_query name), for writing views against.
    verbs_[L"dump"] = [this](const std::wstring& r) {
        auto [what, arg] = split_first(r);
        BbOwnedStr out{};
        BbApp* a = s_.core().app();
        BbStatus st = BB_ERR;
        if (what == L"commands") st = bb_command_list(a, &out);
        else if (what == L"palette") st = bb_palette_state(a, &out);
        else if (what == L"keys") st = bb_keys_page(a, &out);
        else if (what == L"settings") st = bb_settings_page_state(a, &out);
        else if (what == L"columns") st = bb_columns_state(a, tab(), &out);
        else if (what == L"saved") st = bb_saved_state(a, &out);
        else if (what == L"theme") st = bb_theme_state(a, &out);
        else if (what == L"index") st = bb_index_state(a, &out);
        else if (what == L"tiles") st = bb_tab_tile_state(a, tab(), &out);
        else if (what == L"cands") st = bb_columns_candidates(a, tab(), core::s(arg), &out);
        else st = bb_bars_query(a, core::s(what), core::s(arg), &out);
        core::check(st, "dump");
        const std::string text = to_utf8(core::take(out));
        fs::create_directories(o_.out_dir);
        std::ofstream(o_.out_dir / "dump.json", std::ios::binary | std::ios::trunc) << text;
        say("DUMP " + text.substr(0, 200));
    };
}

void Runner::install_ui_bars() {
    // ---- command-bars ----
    verbs_[L"command-bars"] = [this](const std::wstring& r) {
        auto [verb, tail] = split_first(require(r, "command-bars"));
        BbApp* a = s_.core().app();
        const std::wstring v = lower(verb);
        if (v == L"reset") {
            core::check(bb_bars_do(a, core::s(L"reset"), core::s(L"")), "command-bars reset");
        } else if (v == L"toolbar") {
            auto [which, items] = split_first(require(tail, "command-bars toolbar"));
            bool text = false;
            if (items == L"text" || items.rfind(L"text ", 0) == 0) {
                text = true;
                items = items.size() > 4 ? items.substr(5) : L"";
            }
            core::check(bb_bars_set_toolbar(a, core::s(which), core::s(items), text ? 1 : 0), "command-bars toolbar");
        } else if (v == L"menu") {
            auto [name, items] = split_first(require(tail, "command-bars menu"));
            std::wstring caption = name;
            for (auto& c : caption) if (c == L'_') c = L' ';
            core::check(bb_bars_do(a, core::s(L"menu"), core::s(caption + L" " + items)), "command-bars menu");
        } else if (v == L"save" || v == L"apply" || v == L"remove") {
            core::check(bb_bars_do(a, core::s(v), core::s(require(tail, "command-bars"))), ("command-bars " + to_utf8(v)).c_str());
        } else if (v == L"rename") {
            core::check(bb_bars_do(a, core::s(L"rename"), core::s(require(tail, "command-bars rename"))), "command-bars rename");
        } else if (v == L"saved") {
            const auto st = views::model([&](BbOwnedStr* o) { return bb_bars_query(a, core::s(L"state"), core::s(L""), o); });
            std::string names;
            for (size_t i = 0; i < st["saved"].size(); ++i) names += (i ? ", " : "") + st["saved"][i]["name"].str();
            say("SAVED-BARS " + names);
            return;
        } else {
            throw FormatError("'" + to_utf8(verb) + "' is not a command-bars verb. Try: reset, toolbar, menu, save, apply, rename, remove, saved.");
        }
        s_.settle();
    };

    auto toolbar_named = [this](const std::wstring& which) -> views::CommandToolbar* {
        s_.view().refresh_bars();
        const std::wstring w = lower(which);
        if (w == L"title") return s_.view().title_toolbar();
        if (w.rfind(L"row", 0) == 0) {
            const int n = _wtoi(w.c_str() + 3);
            const auto& rows = s_.view().row_toolbars();
            if (n >= 1 && static_cast<size_t>(n) <= rows.size()) return rows[static_cast<size_t>(n) - 1];
            throw AssertionError("There is no toolbar " + to_utf8(which) + ".");
        }
        throw FormatError("'" + to_utf8(which) + "' is not a toolbar. Say title, or row1, row2....");
    };

    verbs_[L"assert-menu-bar"] = [this](const std::wstring& r) {
        s_.view().refresh_bars();
        const auto expected = bar_split(require(r, "assert-menu-bar"));
        const auto actual = s_.view().menu_bar_captions();
        bool same = expected.size() == actual.size();
        for (size_t i = 0; same && i < actual.size(); ++i) same = core::iequals(actual[i], expected[i]);
        if (!same) throw AssertionError("The menu bar reads " + join(actual, " | ") + ", not " + join(expected, " | ") + ".");
    };
    verbs_[L"assert-bars"] = [this](const std::wstring& r) {
        const std::wstring expected = require(r, "assert-bars");
        const auto st = views::model([&](BbOwnedStr* o) { return bb_bars_query(s_.core().app(), core::s(L"state"), core::s(L""), o); });
        const std::wstring actual = st["isDefault"].boolean() ? L"default" : st["name"].is_null() ? L"custom" : core::wide(st["name"].str());
        if (!core::iequals(actual, expected)) {
            throw AssertionError("The menus and toolbars are '" + to_utf8(actual) + "', not '" + to_utf8(expected) + "'.");
        }
    };
    verbs_[L"assert-toolbar"] = [this, toolbar_named](const std::wstring& r) {
        auto [which, tail] = split_first(require(r, "assert-toolbar"));
        std::vector<std::wstring> expected = words(tail);
        int overflow = 0;
        if (!expected.empty() && expected.back().size() > 1 && expected.back()[0] == L'+') {
            overflow = _wtoi(expected.back().c_str() + 1);
            expected.pop_back();
        }
        views::CommandToolbar* tb = toolbar_named(which);
        std::vector<std::wstring> shown;
        for (size_t i = 0; i < tb->visible_count(); ++i) shown.push_back(core::wide(views::bar_token(tb->items()[i].row)));
        const int hidden = static_cast<int>(tb->items().size() - tb->visible_count());
        if (shown != expected || hidden != overflow) {
            throw AssertionError("Toolbar " + to_utf8(which) + " shows [" + join(shown, " ") + "] with " + std::to_string(hidden) +
                                 " overflowed, not [" + join(expected, " ") + "] with " + std::to_string(overflow) + ".");
        }
    };
    // Clicks a toolbar button (by its token, as assert-toolbar names it) through the toolbar's own mouse
    // handling; a dropdown it opens becomes the last menu, for assert-menu-item.
    verbs_[L"toolbar-click"] = [this, toolbar_named](const std::wstring& r) {
        auto [which, token] = split_first(require(r, "toolbar-click"));
        views::CommandToolbar* tb = toolbar_named(which);
        size_t at = tb->items().size();
        for (size_t i = 0; i < tb->visible_count(); ++i) {
            if (core::wide(views::bar_token(tb->items()[i].row)) == trim(token)) at = i;
        }
        if (at == tb->items().size()) throw AssertionError("Toolbar " + to_utf8(which) + " shows no " + to_utf8(token) + ".");
        const ui::Rect rc = tb->item_rect(at);
        ui::MouseEvent e;
        e.pt = {rc.l + 6, (rc.t + rc.b) / 2};
        const ui::MenuItems* before = s_.view().floating_menu();
        tb->on_mouse_down(e);
        tb->on_mouse_up(e);
        s_.settle();
        const ui::MenuItems* menu = s_.view().floating_menu();
        if (menu && menu != before) ui_->last_menu = *menu;
    };
    verbs_[L"assert-menu-open"] = [this](const std::wstring&) {
        if (!s_.view().menu_open()) throw AssertionError("expected a menu to be open, and none is.");
    };
    verbs_[L"assert-menu-closed"] = [this](const std::wstring&) {
        if (s_.view().menu_open()) throw AssertionError("expected no menu to be open, and one is.");
    };
    // The widget with the keyboard focus is the one named (or, with "none", nothing has it).
    verbs_[L"assert-focus"] = [this](const std::wstring& r) {
        const std::wstring name = require(r, "assert-focus");
        const ui::Widget* f = s_.view().window().root().focused();
        if (lower(name) == L"none") {
            if (f) throw AssertionError("expected nothing focused, got '" + f->name + "'.");
            return;
        }
        const ui::Widget* want = s_.view().find_named(to_utf8(name));
        if (!want) throw FormatError("There is no widget named '" + to_utf8(name) + "' in the window.");
        if (f != want) throw AssertionError("expected '" + to_utf8(name) + "' to have the focus, got '" + (f ? f->name : std::string("nothing")) + "'.");
    };
    verbs_[L"assert-toolbar-item"] = [this, toolbar_named](const std::wstring& r) {
        const auto parts = words(require(r, "assert-toolbar-item"));
        if (parts.size() != 3) {
            throw FormatError("assert-toolbar-item wants <title|rowN> <command id> <enabled|disabled|checked|unchecked|text|icon>.");
        }
        views::CommandToolbar* tb = toolbar_named(parts[0]);
        const views::CommandToolbar::Item* item = nullptr;
        for (const auto& it : tb->items()) {
            if (core::wide(it.row.id) == parts[1]) item = &it;
        }
        if (!item) throw AssertionError("Toolbar " + to_utf8(parts[0]) + " has no " + to_utf8(parts[1]) + ".");
        const views::BarRow& row = item->row;
        const std::wstring state = lower(parts[2]);
        bool ok;
        if (state == L"enabled") ok = row.enabled;
        else if (state == L"disabled") ok = !row.enabled;
        else if (state == L"checked") ok = row.checked;
        else if (state == L"unchecked") ok = !row.checked;
        else if (state == L"text") ok = tb->shows_text() || row.show_text;
        else if (state == L"icon") ok = !(tb->shows_text() || row.show_text);
        else throw FormatError("'" + to_utf8(state) + "' is not a state.");
        if (!ok) {
            throw AssertionError(to_utf8(parts[1]) + " on toolbar " + to_utf8(parts[0]) + " is not " + to_utf8(parts[2]) +
                                 " (enabled=" + (row.enabled ? "true" : "false") + ", checked=" + (row.checked ? "true" : "false") +
                                 ", reason=" + (row.reason.empty() ? "none" : to_utf8(row.reason)) + ").");
        }
    };
}

void Runner::install_ui_menus() {
    // ---- menus (photographed, never opened) ----
    verbs_[L"menu"] = [this](const std::wstring& r) {
        auto [kind, tail] = split_first(r);
        auto [name, shot_name] = split_first(tail);
        const std::wstring k = lower(kind);
        ui::MenuItems items;
        std::wstring file;
        if (k == L"main") {
            s_.view().refresh_bars();
            bool found = false;
            items = s_.view().build_main_menu(name, &found);
            if (!found) throw FormatError("The menu bar has no '" + to_utf8(name) + "' menu.");
            file = L"menu-main-" + lower(name) + (shot_name.empty() ? L"" : L"-" + shot_name);
        } else if (k == L"workspace-switcher" || k == L"search-switcher") {
            items = s_.view().switcher_items(k == L"workspace-switcher");
        } else if (k == L"tab") {
            items = s_.view().tab_menu(s_.view().active_tab());
            s_.settle();
        } else if (!s_.view().build_named_menu(k, &items)) {
            throw FormatError("'" + to_utf8(kind) + "' is not a menu. Try: main <Menu>, columns, files, background.");
        }
        if (file.empty()) file = tail.empty() ? L"menu-" + k : tail;
        ui_->last_menu = std::move(items);
        shot_menu(ui_->last_menu, file);
    };
    // Clicks an entry of the last menu photographed, by its text, as choosing it would.
    verbs_[L"menu-run"] = [this](const std::wstring& r) {
        const std::wstring text = require(r, "menu-run");
        std::function<const ui::MenuItem*(const ui::MenuItems&)> find = [&](const ui::MenuItems& items) -> const ui::MenuItem* {
            for (const ui::MenuItem& m : items) {
                if (m.is_separator) continue;
                if (m.is_submenu()) {
                    if (const ui::MenuItem* hit = find(m.sub)) return hit;
                } else if (core::iequals(ui::parse_menu_label(m.text).display, text)) {
                    return &m;
                }
            }
            return nullptr;
        };
        const ui::MenuItem* item = find(ui_->last_menu);
        if (!item) throw AssertionError("The last menu has no '" + to_utf8(text) + "'.");
        if (!item->enabled) throw AssertionError("'" + to_utf8(text) + "' is disabled in the last menu.");
        const auto invoke = item->on_invoke;
        if (invoke) invoke();
        s_.settle();
    };
    // assert-status-bar undo|selection <text>|none -- the undo link or the selection summary, by substring.
    verbs_[L"assert-status-bar"] = [this](const std::wstring& r) {
        const auto [part, want] = split_first(require(r, "assert-status-bar"));
        s_.settle();
        views::StatusBar* bar = s_.view().status_bar();
        const std::wstring p = lower(part);
        if (p != L"undo" && p != L"selection") throw FormatError("assert-status-bar wants undo or selection.");
        const std::wstring actual = p == L"undo" ? bar->undo_text() : bar->selection_text();
        const bool ok = lower(want) == L"none" ? actual.empty() : !actual.empty() && core::icontains(actual, want);
        if (!ok) throw AssertionError("expected the status bar's " + to_utf8(p) + " to be '" + to_utf8(want) + "', it is '" + to_utf8(actual) + "'.");
    };
    // widget-drag <name> <x1> <y1> <x2> <y2> [ctrl|shift] -- a left press, a move and a release on the named
    // widget, at offsets (DIPs) from its top-left corner, through its own mouse handlers: a marquee, a slider.
    verbs_[L"widget-drag"] = [this](const std::wstring& r) {
        const auto [name, tail] = split_first(require(r, "widget-drag"));
        auto nums = core::split(tail, L' ');
        ui::Mods mods;
        if (nums.size() == 5) {
            const std::wstring m = lower(nums.back());
            if (m == L"ctrl") mods.ctrl = true;
            else if (m == L"shift") mods.shift = true;
            else throw FormatError("widget-drag: '" + to_utf8(nums.back()) + "' is not ctrl or shift.");
            nums.pop_back();
        }
        if (nums.size() != 4) throw FormatError("widget-drag wants <name> <x1> <y1> <x2> <y2> [ctrl|shift].");
        ui::Widget* w = s_.view().find_named(to_utf8(name));
        if (!w) throw FormatError("There is no element named '" + to_utf8(name) + "' in the window.");
        auto at = [&](size_t i) { return static_cast<float>(number(nums[i], "widget-drag")); };
        ui::MouseEvent e;
        e.mods = mods;
        e.pt = {w->bounds.l + at(0), w->bounds.t + at(1)};
        w->on_mouse_down(e);
        e.pt = {w->bounds.l + (at(0) + at(2)) / 2, w->bounds.t + (at(1) + at(3)) / 2};
        w->on_mouse_move(e);
        e.pt = {w->bounds.l + at(2), w->bounds.t + at(3)};
        w->on_mouse_move(e);
        w->on_mouse_up(e);
        s_.settle();
    };
    // Types characters into whatever has the keyboard, as WM_CHAR messages the window receives.
    verbs_[L"window-type"] = [this](const std::wstring& r) {
        const std::wstring text = require(r, "window-type");
        const HWND hwnd = s_.view().window().hwnd();
        for (const wchar_t c : text) PostMessageW(hwnd, WM_CHAR, c, 0);
        s_.settle();
    };
    // assert-selection a | b -- exactly these names are selected, in list order.
    verbs_[L"assert-selection"] = [this](const std::wstring& r) {
        const auto wanted = bar_split(require(r, "assert-selection"));
        std::vector<std::wstring> actual;
        for (const auto& p : s_.core().selection_paths(tab())) actual.push_back(std::filesystem::path(p).filename().wstring());
        bool same = wanted.size() == actual.size();
        for (size_t i = 0; same && i < actual.size(); ++i) same = core::iequals(wanted[i], actual[i]);
        if (!same) throw AssertionError("expected the selection to be [" + join(wanted, ", ") + "], it is [" + join(actual, ", ") + "].");
    };
    // assert-cursor <name> -- the active view's cursor is on that row, and the row is scrolled into view.
    verbs_[L"assert-cursor"] = [this](const std::wstring& r) {
        const std::wstring want = require(r, "assert-cursor");
        s_.settle();
        views::TabView* tv = s_.view().active_view();
        const bool tiles = tv->tiles_mode();
        const size_t cursor = tiles ? tv->tiles()->cursor() : tv->list()->cursor();
        const auto listing = s_.core().listing(tab());
        const std::wstring name = cursor == ui::ListView::npos || !listing ? std::wstring(L"(none)") : listing->name(cursor);
        if (!core::iequals(name, want)) throw AssertionError("expected the cursor on '" + to_utf8(want) + "', it is on '" + to_utf8(name) + "'.");
        bool shown;
        if (tiles) {
            const auto [first, last] = tv->tiles()->realized();
            shown = cursor >= first && cursor < last;
        } else {
            const ui::ListView* l = tv->list();
            const float top = static_cast<float>(cursor) * l->row_height;
            shown = top >= l->scroll_y() && top + l->row_height <= l->scroll_y() + l->bounds.height();
        }
        if (!shown) throw AssertionError("the cursor is on '" + to_utf8(want) + "', but that row is scrolled out of view.");
    };
    // field-type <Name> <text> -- replaces the text of a named box in the window (not a dialog), as typing would.
    verbs_[L"field-type"] = [this](const std::wstring& r) {
        const auto [name, text] = split_first(require(r, "field-type"));
        auto* e = dynamic_cast<ui::Edit*>(s_.view().find_named(to_utf8(name)));
        if (!e) throw FormatError("There is no text box named '" + to_utf8(name) + "' in the window.");
        if (!e->flags.enabled) throw AssertionError("'" + to_utf8(name) + "' is disabled.");
        e->set_text(text);
        if (e->on_changed) e->on_changed(text);
        s_.settle();
    };
    // list-pick <ListName> <text> -- selects the first row of a named list in the window whose text contains
    // <text>, as a click on it would.
    verbs_[L"list-pick"] = [this](const std::wstring& r) {
        const auto [name, text] = split_first(require(r, "list-pick"));
        auto* list = dynamic_cast<views::forms::ItemList*>(s_.view().find_named(to_utf8(name)));
        if (!list) throw FormatError("There is no list named '" + to_utf8(name) + "' in the window.");
        for (size_t i = 0; i < list->entries.size(); ++i) {
            if (core::icontains(list->entries[i].text, text)) {
                list->select(static_cast<int>(i), true);
                s_.settle();
                return;
            }
        }
        std::vector<std::wstring> rows;
        for (const auto& e : list->entries) rows.push_back(e.text);
        throw AssertionError("'" + to_utf8(name) + "' has no row with '" + to_utf8(text) + "'. Rows: " + join(rows, " | "));
    };
    // A row's words as a person reads them: a checklist's box glyph is not part of its name.
    auto row_words = [](const std::wstring& t) {
        size_t i = 0;
        while (i < t.size() && (t[i] == L'\u2611' || t[i] == L'\u2610' || t[i] == L' ')) ++i;
        return t.substr(i);
    };
    // The row named `text` (exactly, else the first that contains it), or a failure listing what is there.
    auto find_row = [this, row_words](views::forms::ItemList& list, const std::wstring& name, const std::wstring& text) {
        for (size_t i = 0; i < list.entries.size(); ++i) {
            if (!list.entries[i].heading && core::iequals(row_words(list.entries[i].text), text)) return static_cast<int>(i);
        }
        for (size_t i = 0; i < list.entries.size(); ++i) {
            if (!list.entries[i].heading && core::icontains(list.entries[i].text, text)) return static_cast<int>(i);
        }
        std::vector<std::wstring> rows;
        for (const auto& e : list.entries) rows.push_back(e.text);
        throw AssertionError("'" + to_utf8(name) + "' has no row with '" + to_utf8(text) + "'. Rows: " + join(rows, " | "));
    };
    auto item_list = [this](const std::wstring& name) {
        auto* list = dynamic_cast<views::forms::ItemList*>(s_.view().find_named(to_utf8(name)));
        if (!list) throw FormatError("There is no list named '" + to_utf8(name) + "' in the window.");
        return list;
    };
    // list-drag <List>[><Target>] <gap> <row> -- presses the row, drags it and lets go over the line before row
    // <gap> (0 is the top, the row count the end), of the same list or of <Target>, through the lists' own
    // mouse handlers: a reorder, or a choice dropped exactly where it should go.
    verbs_[L"list-drag"] = [this, find_row, item_list](const std::wstring& r) {
        const auto [names, tail] = split_first(require(r, "list-drag"));
        const auto [gap_text, text] = split_first(tail);
        const size_t cut = names.find(L'>');
        const std::wstring source_name = cut == std::wstring::npos ? names : names.substr(0, cut);
        const std::wstring target_name = cut == std::wstring::npos ? names : names.substr(cut + 1);
        auto* source = item_list(source_name);
        auto* target = item_list(target_name);
        const int gap = number(gap_text, "list-drag");
        const int row = find_row(*source, source_name, text);
        const int n = static_cast<int>(target->entries.size());
        if (gap < 0 || gap > n) throw FormatError("list-drag: '" + to_utf8(target_name) + "' has gaps 0 to " + std::to_string(n) + ".");
        source->scroll_into_view(row);
        const ui::Rect from = source->row_rect(row);
        const float y = gap < n ? target->row_rect(gap).t + 1 : target->row_rect(n - 1).b - 1;
        ui::MouseEvent e;
        e.pt = {(from.l + from.r) / 2, (from.t + from.b) / 2};
        source->on_mouse_down(e);
        e.pt = {(from.l + from.r) / 2, (from.t + from.b) / 2 + 8};
        source->on_mouse_move(e);
        e.pt = {(target->bounds.l + target->bounds.r) / 2, y};
        source->on_mouse_move(e);
        source->on_mouse_up(e);
        s_.settle();
    };
    // list-activate <List> <text> -- the row named <text> is taken, as a click or Enter on it would.
    verbs_[L"list-activate"] = [this, find_row, item_list](const std::wstring& r) {
        const auto [name, text] = split_first(require(r, "list-activate"));
        auto* list = item_list(name);
        const int row = find_row(*list, name, text);
        if (!list->on_activate) throw AssertionError("'" + to_utf8(name) + "' has nothing to do when a row is activated.");
        list->select(row);
        list->on_activate(row);
        s_.settle();
    };
    // assert-list-row <List> <index> <text> -- row <index> of the list reads <text>.
    verbs_[L"assert-list-row"] = [this, row_words, item_list](const std::wstring& r) {
        const auto [name, tail] = split_first(require(r, "assert-list-row"));
        const auto [index_text, text] = split_first(tail);
        s_.settle();
        auto* list = item_list(name);
        const int i = number(index_text, "assert-list-row");
        if (i < 0 || static_cast<size_t>(i) >= list->entries.size())
            throw AssertionError("'" + to_utf8(name) + "' has " + std::to_string(list->entries.size()) + " rows, not a row " + std::to_string(i) + ".");
        const std::wstring actual = row_words(list->entries[static_cast<size_t>(i)].text);
        if (!core::iequals(actual, text)) throw AssertionError("row " + std::to_string(i) + " of '" + to_utf8(name) + "' is '" + to_utf8(actual) + "', expected '" + to_utf8(text) + "'.");
    };
    // Gives the keyboard to an element by its name, as a click into it would.
    verbs_[L"focus"] = [this](const std::wstring& r) {
        const std::wstring name = require(r, "focus");
        ui::Widget* w = s_.view().find_named(to_utf8(name));
        if (!w) throw FormatError("There is no element named '" + to_utf8(name) + "' in the window.");
        s_.view().window().root().set_focus(w);
        s_.settle();
    };
    // Presses a button by its element name, as a click would.
    verbs_[L"press"] = [this](const std::wstring& r) {
        const std::wstring name = require(r, "press");
        auto* b = dynamic_cast<ui::Button*>(s_.view().find_named(to_utf8(name)));
        if (!b) throw FormatError("There is no button named '" + to_utf8(name) + "' in the window.");
        if (!b->flags.enabled) throw AssertionError("'" + to_utf8(name) + "' is disabled.");
        b->click();
        s_.settle();
    };
    // Drags tab <from> along the active pane's strip and lets go over tab <to> (1-based), through the
    // strip's own mouse handling: press, a move past the threshold, release.
    verbs_[L"tab-drag"] = [this](const std::wstring& r) {
        const auto parts = core::split(require(r, "tab-drag"), L' ');
        if (parts.size() != 2) throw FormatError("tab-drag takes two tab numbers: tab-drag <from> <to>.");
        const int from = number(parts[0], "tab-drag"), to = number(parts[1], "tab-drag");
        views::TabStrip* strip = s_.view().active_strip();
        const int n = strip ? static_cast<int>(strip->tabs().size()) : 0;
        if (from < 1 || from > n || to < 1 || to > n) throw AssertionError("tab-drag: this pane has " + std::to_string(n) + " tab(s).");
        const ui::Rect a = strip->tab_rect(static_cast<size_t>(from - 1));
        const ui::Rect b = strip->tab_rect(static_cast<size_t>(to - 1));
        ui::MouseEvent e;
        e.pt = {a.l + 12, (a.t + a.b) / 2};
        strip->on_mouse_down(e);
        // Past the middle of the target, on the side that puts the tab where the target was.
        e.pt = {to > from ? b.r - 4 : b.l + 4, e.pt.y};
        strip->on_mouse_move(e);
        strip->on_mouse_up(e);
        s_.settle();
    };
    verbs_[L"assert-menu-order"] = [this](const std::wstring& r) {
        const auto wanted = bar_split(require(r, "assert-menu-order"));
        std::vector<std::wstring> lines;
        for (const ui::MenuItem& m : ui_->last_menu) lines.push_back(m.is_separator ? L"-" : ui::parse_menu_label(m.text).display);
        int at = -1;
        for (const std::wstring& text : wanted) {
            int next = -1;
            for (size_t i = static_cast<size_t>(at + 1); i < lines.size(); ++i) {
                const bool match = text == L"-" ? lines[i] == L"-" : lines[i] != L"-" && core::icontains(lines[i], text);
                if (match) {
                    next = static_cast<int>(i);
                    break;
                }
            }
            if (next < 0) {
                throw AssertionError("'" + to_utf8(text) + "' does not come after '" + (at < 0 ? std::string("the top") : to_utf8(lines[static_cast<size_t>(at)])) +
                                     "' in the last menu. Lines: " + join(lines, " | "));
            }
            at = next;
        }
    };
    auto menu_item = [this](const std::wstring& r, bool expected) {
        const std::wstring text = require(r, expected ? "assert-menu-item" : "assert-no-menu-item");
        std::vector<std::wstring> headers;
        std::vector<std::pair<std::wstring, std::wstring>> gestures;
        collect_headers(ui_->last_menu, &headers, &gestures);
        bool actual = false;
        for (const auto& h : headers) actual = actual || core::icontains(h, text);
        if (actual != expected) {
            throw AssertionError(expected ? "no item of the last menu contains '" + to_utf8(text) + "'. Items: " + join(headers, " | ")
                                          : "an item of the last menu contains '" + to_utf8(text) + "', and none should.");
        }
    };
    // Runs one entry of the folder tree's menu for a folder, as a right-click on its row then a click on
    // the entry would: the entry acts on that folder, whatever the file list shows.
    verbs_[L"tree-menu-run"] = [this](const std::wstring& r) {
        const std::wstring rest = require(r, "tree-menu-run");
        const size_t on = rest.rfind(L" on ");
        if (on == std::wstring::npos) throw FormatError("tree-menu-run wants <item text> on <folder>.");
        const std::wstring text = trim(rest.substr(0, on));
        const std::wstring folder = sandbox_.resolve(trim(rest.substr(on + 4))).wstring();
        views::ops::ComposedMenu composed = views::ops::compose_context_menu(s_.view(), views::ops::MenuKind::Tree, folder);
        std::function<const ui::MenuItem*(const ui::MenuItems&)> find = [&](const ui::MenuItems& items) -> const ui::MenuItem* {
            for (const ui::MenuItem& m : items) {
                if (m.is_separator) continue;
                if (m.is_submenu()) {
                    if (const ui::MenuItem* hit = find(m.sub)) return hit;
                } else if (core::iequals(ui::parse_menu_label(m.text).display, text)) {
                    return &m;
                }
            }
            return nullptr;
        };
        const ui::MenuItem* item = find(composed.items);
        if (!item) throw AssertionError("The tree's menu for '" + to_utf8(folder) + "' has no '" + to_utf8(text) + "'.");
        if (!item->enabled) throw AssertionError("'" + to_utf8(text) + "' is disabled in the tree's menu.");
        if (item->on_invoke) item->on_invoke();
        s_.settle();
    };
    verbs_[L"assert-menu-item"] = [menu_item](const std::wstring& r) { menu_item(r, true); };
    verbs_[L"assert-no-menu-item"] = [menu_item](const std::wstring& r) { menu_item(r, false); };
    verbs_[L"assert-menu-gesture"] = [this](const std::wstring& r) {
        const std::wstring rest = require(r, "assert-menu-gesture");
        const size_t split = rest.rfind(L' ');
        if (split == std::wstring::npos) throw FormatError("assert-menu-gesture wants <item text> <chord|none>.");
        const std::wstring text = trim(rest.substr(0, split));
        std::wstring expected = trim(rest.substr(split + 1));
        if (lower(expected) == L"none") expected.clear();
        std::vector<std::wstring> headers;
        std::vector<std::pair<std::wstring, std::wstring>> gestures;
        collect_headers(ui_->last_menu, &headers, &gestures);
        for (const auto& [h, g] : gestures) {
            if (!core::icontains(h, text)) continue;
            if (!core::iequals(g, expected)) {
                throw AssertionError("'" + to_utf8(h) + "' shows '" + to_utf8(g) + "', not '" + to_utf8(expected) + "'.");
            }
            return;
        }
        throw AssertionError("no item of the last menu contains '" + to_utf8(text) + "'.");
    };
}

}  // namespace bbh
