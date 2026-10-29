// The window-feature verbs: menu bar and toolbars, commands and keys, the palette, settings, columns,
// themes, saved searches, the index banner, dialogs. Each does what the C# ScriptRunner's verb does, over
// the same core models the views draw from, and asserts on what the views are showing.
#include "runner.h"

#include <windows.h>

#include <algorithm>
#include <chrono>
#include <cwctype>
#include <fstream>

#include "views/command_ui.h"
#include "views/ops_menus.h"
#include "views/dialogs.h"
#include "views/json.h"
#include "views/model.h"

namespace bbh {

namespace core = bb::core;
namespace ui = bb::ui;
namespace views = bb::views;

struct Runner::UiState {
    ui::MenuItems last_menu;
};

namespace {

std::wstring lower(std::wstring s) {
    for (auto& c : s) c = static_cast<wchar_t>(std::towlower(c));
    return s;
}

std::wstring trim(const std::wstring& s) {
    const size_t b = s.find_first_not_of(L" \t\r\n");
    if (b == std::wstring::npos) return {};
    return s.substr(b, s.find_last_not_of(L" \t\r\n") - b + 1);
}

std::vector<std::wstring> words(const std::wstring& s) {
    std::vector<std::wstring> out;
    std::wstring cur;
    for (wchar_t c : s) {
        if (c == L' ') {
            if (!cur.empty()) out.push_back(cur);
            cur.clear();
        } else {
            cur += c;
        }
    }
    if (!cur.empty()) out.push_back(cur);
    return out;
}

std::vector<std::wstring> bar_split(const std::wstring& s) {
    std::vector<std::wstring> out;
    for (const std::wstring& p : core::split(s, L'|')) {
        const std::wstring t = trim(p);
        if (!t.empty()) out.push_back(t);
    }
    return out;
}

std::string join(const std::vector<std::wstring>& v, const char* sep) {
    std::string out;
    for (size_t i = 0; i < v.size(); ++i) out += (i ? sep : "") + to_utf8(v[i]);
    return out;
}

// "Ctrl+Shift+F5" -> modifiers (1 Ctrl, 2 Alt, 4 Shift) and the key's name.
bool parse_chord(const std::wstring& text, uint32_t* mods, std::wstring* key) {
    std::vector<std::wstring> parts = core::split(trim(text), L'+');
    *mods = 0;
    if (parts.empty()) return false;
    std::wstring k = parts.back();
    parts.pop_back();
    if (k.empty() && !parts.empty()) {  // "Num+": the key is the plus itself
        k = parts.back() + L"+";
        parts.pop_back();
    }
    for (const std::wstring& p : parts) {
        const std::wstring m = lower(p);
        if (m == L"ctrl") *mods |= 1;
        else if (m == L"alt") *mods |= 2;
        else if (m == L"shift") *mods |= 4;
        else return false;
    }
    if (k.size() == 1) k[0] = static_cast<wchar_t>(std::towupper(k[0]));
    *key = k;
    return !k.empty();
}

BbFocusKind focus_word(const std::wstring& w) {
    const std::wstring f = lower(trim(w));
    if (f == L"list") return BB_FOCUS_FILE_LIST;
    if (f == L"text") return BB_FOCUS_TEXT_EDITABLE;
    if (f == L"readonly") return BB_FOCUS_TEXT_READ_ONLY;
    if (f == L"other" || f == L"tree") return BB_FOCUS_OTHER;
    throw FormatError("Focus is one of list, text, readonly or other, not '" + to_utf8(w) + "'.");
}

void collect_headers(const ui::MenuItems& items, std::vector<std::wstring>* headers,
                     std::vector<std::pair<std::wstring, std::wstring>>* gestures) {
    for (const ui::MenuItem& m : items) {
        if (m.is_separator) continue;
        const std::wstring text = ui::parse_menu_label(m.text).display;
        headers->push_back(text);
        gestures->push_back({text, m.gesture});
        collect_headers(m.sub, headers, gestures);
    }
}

// ---- fixtures: a PNG and a zip written by hand, so two runs make byte-identical files ----

uint32_t crc32_of(const uint8_t* data, size_t n, uint32_t crc = 0) {
    static uint32_t table[256];
    static bool made = false;
    if (!made) {
        for (uint32_t i = 0; i < 256; ++i) {
            uint32_t c = i;
            for (int k = 0; k < 8; ++k) c = c & 1 ? 0xEDB88320u ^ (c >> 1) : c >> 1;
            table[i] = c;
        }
        made = true;
    }
    crc = ~crc;
    for (size_t i = 0; i < n; ++i) crc = table[(crc ^ data[i]) & 0xFF] ^ (crc >> 8);
    return ~crc;
}

void put_be32(std::string& s, uint32_t v) {
    for (int i = 3; i >= 0; --i) s += static_cast<char>(v >> (8 * i) & 0xFF);
}
void put_le16(std::string& s, uint32_t v) {
    s += static_cast<char>(v & 0xFF);
    s += static_cast<char>(v >> 8 & 0xFF);
}
void put_le32(std::string& s, uint32_t v) {
    for (int i = 0; i < 4; ++i) s += static_cast<char>(v >> (8 * i) & 0xFF);
}

void png_chunk(std::string& out, const char* type, const std::string& data) {
    put_be32(out, static_cast<uint32_t>(data.size()));
    std::string body = std::string(type, 4) + data;
    out += body;
    put_be32(out, crc32_of(reinterpret_cast<const uint8_t*>(body.data()), body.size()));
}

// 8-bit RGBA, filter 0, zlib "stored" blocks.
void write_png_rgba(const fs::path& path, uint32_t w, uint32_t h, const std::vector<uint8_t>& rgba) {
    std::string raw;
    for (uint32_t y = 0; y < h; ++y) {
        raw += '\0';
        raw.append(reinterpret_cast<const char*>(&rgba[static_cast<size_t>(y) * w * 4]), static_cast<size_t>(w) * 4);
    }
    std::string z = "\x78\x01";
    for (size_t pos = 0; pos < raw.size(); pos += 65535) {
        const size_t n = std::min<size_t>(65535, raw.size() - pos);
        z += pos + n >= raw.size() ? '\x01' : '\x00';
        put_le16(z, static_cast<uint32_t>(n));
        put_le16(z, static_cast<uint32_t>(~n & 0xFFFF));
        z.append(raw, pos, n);
    }
    uint32_t a = 1, b = 0;
    for (const unsigned char c : raw) {
        a = (a + c) % 65521;
        b = (b + a) % 65521;
    }
    put_be32(z, b << 16 | a);
    std::string out = "\x89PNG\r\n\x1a\n";
    std::string ihdr;
    put_be32(ihdr, w);
    put_be32(ihdr, h);
    ihdr += std::string("\x08\x06\x00\x00\x00", 5);
    png_chunk(out, "IHDR", ihdr);
    png_chunk(out, "IDAT", z);
    png_chunk(out, "IEND", "");
    std::ofstream(path, std::ios::binary | std::ios::trunc).write(out.data(), static_cast<std::streamsize>(out.size()));
}

void write_zip(const fs::path& path, const std::vector<std::pair<std::string, std::string>>& entries) {
    std::string out, central;
    for (const auto& [name, data] : entries) {
        const uint32_t crc = crc32_of(reinterpret_cast<const uint8_t*>(data.data()), data.size());
        const uint32_t offset = static_cast<uint32_t>(out.size());
        put_le32(out, 0x04034b50);
        put_le16(out, 20);
        put_le16(out, 0);
        put_le16(out, 0);
        put_le16(out, 0x6000);  // a fixed time and date
        put_le16(out, 0x5821);
        put_le32(out, crc);
        put_le32(out, static_cast<uint32_t>(data.size()));
        put_le32(out, static_cast<uint32_t>(data.size()));
        put_le16(out, static_cast<uint32_t>(name.size()));
        put_le16(out, 0);
        out += name;
        out += data;
        put_le32(central, 0x02014b50);
        put_le16(central, 20);
        put_le16(central, 20);
        put_le16(central, 0);
        put_le16(central, 0);
        put_le16(central, 0x6000);
        put_le16(central, 0x5821);
        put_le32(central, crc);
        put_le32(central, static_cast<uint32_t>(data.size()));
        put_le32(central, static_cast<uint32_t>(data.size()));
        put_le16(central, static_cast<uint32_t>(name.size()));
        put_le16(central, 0);
        put_le16(central, 0);
        put_le16(central, 0);
        put_le16(central, 0);
        put_le32(central, 0);
        put_le32(central, offset);
        central += name;
    }
    const uint32_t cd_offset = static_cast<uint32_t>(out.size());
    out += central;
    put_le32(out, 0x06054b50);
    put_le16(out, 0);
    put_le16(out, 0);
    put_le16(out, static_cast<uint32_t>(entries.size()));
    put_le16(out, static_cast<uint32_t>(entries.size()));
    put_le32(out, static_cast<uint32_t>(central.size()));
    put_le32(out, cd_offset);
    put_le16(out, 0);
    std::ofstream(path, std::ios::binary | std::ios::trunc).write(out.data(), static_cast<std::streamsize>(out.size()));
}

void write_text(const fs::path& path, const std::string& text, bool bom = false) {
    fs::create_directories(path.parent_path());
    std::ofstream f(path, std::ios::binary | std::ios::trunc);
    if (bom) f << "\xEF\xBB\xBF";
    f << text;
}

}  // namespace

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
        } else if (!s_.view().build_named_menu(k, &items)) {
            throw FormatError("'" + to_utf8(kind) + "' is not a menu. Try: main <Menu>, columns, files, background.");
        }
        if (file.empty()) file = tail.empty() ? L"menu-" + k : tail;
        ui_->last_menu = std::move(items);
        shot_menu(ui_->last_menu, file);
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

    // ---- saved searches and workspaces ----
    auto saved_do = [this](const std::wstring& verb, const std::wstring& arg, bool with_tab) {
        core::check(bb_saved_do(s_.core().app(), with_tab ? tab() : 0, core::s(verb), core::s(arg)), to_utf8(verb).c_str());
    };
    auto wait_search = [this] {
        const auto start = std::chrono::steady_clock::now();
        s_.settle();
        while (s_.core().tab_state(tab()).is_search_running &&
               std::chrono::duration_cast<std::chrono::milliseconds>(std::chrono::steady_clock::now() - start).count() < o_.busy_timeout_ms) {
            s_.pump();
            Sleep(2);
        }
        s_.settle();
    };
    verbs_[L"save-search"] = [this, saved_do](const std::wstring& r) {
        saved_do(L"save-search", require(r, "save-search"), true);
        s_.settle();
        s_.view().sync_overlays();
    };
    verbs_[L"run-saved"] = [this, saved_do, wait_search](const std::wstring& r) {
        std::wstring text = require(r, "run-saved");
        bool new_tab = false;
        if (text.size() > 7 && core::iequals(text.substr(text.size() - 7), L" newtab")) {
            new_tab = true;
            text = trim(text.substr(0, text.size() - 7));
        }
        saved_do(new_tab ? L"run-search-new-tab" : L"run-search", text, true);
        wait_search();
    };
    verbs_[L"rename-saved"] = [this, saved_do](const std::wstring& r) {
        saved_do(L"rename-search", require(r, "rename-saved"), false);
        s_.settle();
        s_.view().sync_overlays();
    };
    verbs_[L"remove-saved"] = [this, saved_do](const std::wstring& r) {
        saved_do(L"remove-search", require(r, "remove-saved"), false);
        s_.settle();
        s_.view().sync_overlays();
    };
    auto saved_names = [this](const char* what) {
        const auto st = views::model([&](BbOwnedStr* o) { return bb_saved_state(s_.core().app(), o); });
        std::vector<std::wstring> out;
        for (size_t i = 0; i < st[what].size(); ++i) {
            const auto& e = st[what][i];
            out.push_back(core::wide(e["name"].str()) + L" (" + core::wide(e["scopeText"].str().empty() ? e["shape"].str() : e["scopeText"].str()) + L")");
        }
        return out;
    };
    verbs_[L"saved-searches"] = [saved_names](const std::wstring&) { say("SAVED " + join(saved_names("searches"), ", ")); };
    // The sidebar's Bookmarks section as it shows now (hidden targets left out unless hidden items are shown).
    verbs_[L"assert-bookmarks"] = [this](const std::wstring& r) {
        const std::wstring rest = require(r, "assert-bookmarks");
        const std::vector<std::wstring> wanted = core::iequals(rest, L"none") ? std::vector<std::wstring>{} : bar_split(rest);
        s_.view().sync_overlays();
        std::vector<std::wstring> shown;
        for (const std::wstring& p : s_.view().sidebar()->bookmarks()) shown.push_back(fs::path(p).filename().wstring());
        bool same = shown.size() == wanted.size();
        for (size_t i = 0; same && i < shown.size(); ++i) same = core::iequals(shown[i], wanted[i]);
        if (!same) throw AssertionError("The bookmarks are: " + (shown.empty() ? std::string("none") : join(shown, " | ")) + ".");
    };
    verbs_[L"workspaces"] = [saved_names](const std::wstring&) { say("WORKSPACES " + join(saved_names("workspaces"), ", ")); };
    auto saved_assert = [this](const std::wstring& r, bool expected, const char* what) {
        const std::wstring name = require(r, expected ? "assert-saved-search" : "assert-no-saved-search");
        const auto st = views::model([&](BbOwnedStr* o) { return bb_saved_state(s_.core().app(), o); });
        bool present = false;
        std::vector<std::wstring> all;
        for (size_t i = 0; i < st[what].size(); ++i) {
            all.push_back(core::wide(st[what][i]["name"].str()));
            present = present || core::iequals(all.back(), name);
        }
        if (present != expected) {
            throw AssertionError(expected ? "'" + to_utf8(name) + "' is not a saved search. Saved: " + join(all, ", ") : "'" + to_utf8(name) + "' is still a saved search.");
        }
    };
    verbs_[L"assert-saved-search"] = [saved_assert](const std::wstring& r) { saved_assert(r, true, "searches"); };
    verbs_[L"assert-no-saved-search"] = [saved_assert](const std::wstring& r) { saved_assert(r, false, "searches"); };
    verbs_[L"save-workspace"] = [this, saved_do](const std::wstring& r) {
        saved_do(L"save-workspace", require(r, "save-workspace"), false);
        s_.settle();
        s_.view().sync_overlays();
    };
    verbs_[L"switch-workspace"] = [this, saved_do](const std::wstring& r) {
        saved_do(L"switch-workspace", require(r, "switch-workspace"), false);
        s_.settle();
    };
    verbs_[L"rename-workspace"] = [this, saved_do](const std::wstring& r) {
        saved_do(L"rename-workspace", require(r, "rename-workspace"), false);
        s_.settle();
    };
    verbs_[L"remove-workspace"] = [this, saved_do](const std::wstring& r) {
        saved_do(L"remove-workspace", require(r, "remove-workspace"), false);
        s_.settle();
    };

    // ---- columns, sorting, the list and tile views ----
    auto column_summary = [this] {
        std::wstring s;
        for (const auto& id : s_.view().active_view()->column_ids()) s += (s.empty() ? L"" : L", ") + id;
        return to_utf8(s);
    };
    verbs_[L"columns"] = [this, column_summary](const std::wstring& r) {
        auto [verb, tail] = split_first(r);
        const std::wstring v = lower(verb);
        BbApp* a = s_.core().app();
        if (v.empty()) {
            say("COLUMNS " + column_summary());
            return;
        }
        if (v != L"add" && v != L"remove" && v != L"move" && v != L"width" && v != L"reset" && v != L"default") {
            throw FormatError("columns wants add, remove, move, width, reset or default, got '" + to_utf8(verb) + "'.");
        }
        core::check(bb_columns_do(a, tab(), core::s(v), core::s(tail)), "columns");
        s_.settle();
        s_.view().active_view()->reload_columns();
    };
    verbs_[L"assert-columns"] = [this](const std::wstring& r) {
        s_.view().active_view()->reload_columns();
        std::vector<std::wstring> wanted;
        for (const std::wstring& p : core::split(require(r, "assert-columns"), L',')) {
            if (!trim(p).empty()) wanted.push_back(trim(p));
        }
        const auto actual = s_.view().active_view()->column_ids();
        bool same = wanted.size() == actual.size();
        for (size_t i = 0; same && i < actual.size(); ++i) same = core::iequals(wanted[i], actual[i]);
        if (!same) throw AssertionError("expected the columns to be [" + join(wanted, ", ") + "], they are [" + join(actual, ", ") + "].");
    };
    auto column_assert = [this, column_summary](const std::wstring& r, bool expected) {
        s_.view().active_view()->reload_columns();
        const std::wstring id = require(r, expected ? "assert-column" : "assert-no-column");
        bool present = false;
        for (const auto& c : s_.view().active_view()->column_ids()) present = present || core::iequals(c, id);
        if (present != expected) {
            throw AssertionError(expected ? "there is no '" + to_utf8(id) + "' column: " + column_summary()
                                          : "the '" + to_utf8(id) + "' column is still there: " + column_summary());
        }
    };
    verbs_[L"assert-column"] = [column_assert](const std::wstring& r) { column_assert(r, true); };
    verbs_[L"assert-no-column"] = [column_assert](const std::wstring& r) { column_assert(r, false); };
    verbs_[L"sort"] = [this](const std::wstring& r) {
        std::wstring id = require(r, "sort");
        if (lower(id) == L"date") id = L"Modified";
        core::check(bb_tab_set_sort(s_.core().app(), tab(), core::s(id)), "sort");
        s_.settle();
    };
    verbs_[L"thumbnails"] = [this](const std::wstring& r) {
        const double scale = fraction(r, "thumbnails");
        core::check(bb_tab_set_thumbnail_scale(s_.core().app(), tab(), scale), "thumbnails");
        s_.settle(120);
    };
    verbs_[L"preview"] = [this](const std::wstring& r) {
        const bool on = on_off(r, "preview");
        core::check(bb_tab_set_side_pane(s_.core().app(), tab(), on ? 1 : 0), "preview");
        s_.settle(400);
    };
    verbs_[L"list-scroll"] = [this](const std::wstring& r) {
        s_.view().active_view()->scroll_to_offset(static_cast<float>(_wtof(require(r, "list-scroll").c_str())));
        s_.settle();
    };
    verbs_[L"reveal"] = [this](const std::wstring& r) {
        s_.view().active_view()->reveal_row(row_index(require(r, "reveal")));
        s_.settle();
    };
    verbs_[L"assert-realized"] = [this](const std::wstring& r) {
        auto [op, tail] = split_first(r);
        const int want = number(require(tail, "assert-realized"), "assert-realized");
        const int actual = static_cast<int>(s_.view().active_view()->realized_rows());
        const std::wstring o = lower(op);
        bool ok;
        if (o == L"under" || o == L"<") ok = actual < want;
        else if (o == L"over" || o == L">") ok = actual > want;
        else throw FormatError("assert-realized: expected 'under' or 'over', got '" + to_utf8(op) + "'.");
        if (!ok) throw AssertionError("expected the list to have " + to_utf8(op) + " " + std::to_string(want) + " realized row(s), got " + std::to_string(actual) + ".");
    };
    verbs_[L"assert-header-menu"] = [this](const std::wstring& r) {
        const std::wstring want = lower(require(r, "assert-header-menu"));
        if (want != L"columns" && want != L"files") throw FormatError("assert-header-menu wants columns or files, got '" + to_utf8(want) + "'.");
        const bool columns = s_.view().active_view()->header_opens_column_menu();
        if ((want == L"columns") != columns) {
            throw AssertionError(std::string("a right-click past the last column would open the '") + (columns ? "columns" : "files") + "' menu, expected '" + to_utf8(want) + "'.");
        }
    };
    verbs_[L"assert-metadata"] = [this](const std::wstring& r) {
        auto [name, tail] = split_first(r);
        auto [canonical, expected] = split_first(tail);
        if (canonical.empty() || expected.empty()) throw FormatError("assert-metadata wants a row, a canonical name and the text to find.");
        s_.settle(100);
        const size_t row = row_index(name);
        const std::wstring actual = s_.view().active_view()->cell_by_id(row, canonical);
        std::wstring want = expected;
        if (want.size() > 1 && want.front() == L'"' && want.back() == L'"') want = want.substr(1, want.size() - 2);
        if (!core::icontains(actual, want)) {
            throw AssertionError("the '" + to_utf8(canonical) + "' cell of '" + to_utf8(name) + "' reads '" + to_utf8(actual) + "', expected it to contain '" + to_utf8(expected) + "'.");
        }
    };

    // ---- fixtures ----
    verbs_[L"many-fixture"] = [this](const std::wstring& r) {
        auto [count_text, dir] = split_first(r);
        const int count = count_text.empty() ? 400 : number(count_text, "many-fixture");
        const fs::path root = sandbox_.require_inside(dir.empty() ? L"Many" : dir, "many-fixture");
        for (int i = 0; i < count; ++i) {
            char buf[64];
            snprintf(buf, sizeof(buf), "Batch%02d", i / 100);
            const fs::path sub = root / buf;
            fs::create_directories(sub);
            snprintf(buf, sizeof(buf), i % 4 == 3 ? "note%04d.txt" : "clip%04d.mp4", i);
            Sandbox::write(sub / buf, 64);
        }
        Sandbox::stamp(root);
        say("# many fixture: " + std::to_string(count) + " files under " + to_utf8(root.wstring()));
    };
    verbs_[L"content-fixture"] = [this](const std::wstring& r) {
        const fs::path root = sandbox_.require_inside(r.empty() ? L"Content" : r, "content-fixture");
        write_text(root / "haystack.txt", "first line\nthe annual report is late\nthird line\n");
        write_text(root / "decoy.txt", "annual sales figures\nand a separate report\n");
        write_text(root / "code.cs", "public class Thing\n{\n    // the annual report goes here\n}\n");
        write_text(root / "annual report.md", "this file is named for it but never says the words\n");
        {
            const std::wstring text = L"the annual report, in two-byte characters\n";
            std::ofstream f(root / "utf16.txt", std::ios::binary | std::ios::trunc);
            f.write(reinterpret_cast<const char*>(text.data()), static_cast<std::streamsize>(text.size() * 2));
        }
        std::string opaque(2048, '\0');
        const std::string needle = "the annual report";
        opaque.replace(16, needle.size(), needle);
        for (size_t i = 64; i < opaque.size(); ++i) opaque[i] = static_cast<char>(i % 251);
        write_text(root / "opaque.bin", opaque);
        Sandbox::stamp(root);
        say("# content fixture: " + to_utf8(root.wstring()));
    };
    verbs_[L"preview-fixture"] = [this](const std::wstring& r) {
        const fs::path root = sandbox_.require_inside(r.empty() ? L"Preview" : r, "preview-fixture");
        fs::create_directories(root);
        std::vector<uint8_t> px(64 * 64 * 4);
        for (int y = 0; y < 64; ++y) {
            for (int x = 0; x < 64; ++x) {
                uint8_t* p = &px[(static_cast<size_t>(y) * 64 + x) * 4];
                const bool inside = (x - 32) * (x - 32) + (y - 32) * (y - 32) < 20 * 20;
                p[0] = 200;
                p[1] = static_cast<uint8_t>(y * 4);
                p[2] = static_cast<uint8_t>(x * 4);
                p[3] = inside ? 0 : 255;
            }
        }
        write_png_rgba(root / "sample.png", 64, 64, px);
        write_zip(root / "sample.zip", {{"readme.txt", "This archive is listed, never extracted.\n"},
                                        {"src/app.js", "console.log('hello');\n"},
                                        {"src/lib/util.js", "export const one = 1;\n"},
                                        {"docs/guide.md", "# Guide\n\nNothing here is opened.\n"}});
        write_text(root / "Sample.cs",
                   "using System;\n\nnamespace Sample;\n\n/// <summary>A little of everything the tokenizer has an opinion about.</summary>\npublic static class Greeter\n{\n"
                   "    private const int Attempts = 3;      // a number and a line comment\n"
                   "    private const string Url = \"https://example.com/not-a-comment\";\n\n    /* a block comment\n       over two lines */\n"
                   "    public static void Greet(string name)\n    {\n        for (var i = 0; i < Attempts; i++)\n"
                   "            Console.WriteLine($\"Hello, {name}! 0xFF is {0xFF}, and it's fine.\");\n    }\n}\n");
        write_text(root / "sample.md", "# Sample document\n\nOrdinary paragraph text, which stays plain.\n\n- one\n- two\n\n```\ncode\n```\n");
        Sandbox::write(root / "plain.txt", 400);
        write_text(root / "app.exe.manifest",
                   "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<assembly manifestVersion=\"1.0\" xmlns=\"urn:schemas-microsoft-com:asm.v1\">\n"
                   "  <assemblyIdentity version=\"1.0.0.0\" name=\"sample.app\"/>\n</assembly>\n", true);
        write_text(root / "mystery.qqq", "Not a format anyone has heard of,\nand plainly readable anyway.\n");
        std::string opaque(2048, '\0');
        for (size_t i = 0; i < opaque.size(); ++i) opaque[i] = static_cast<char>(1 + i % 0x1F);
        write_text(root / "opaque.qqq", opaque);
        Sandbox::stamp(root);
        say("# preview fixture: " + to_utf8(root.wstring()));
    };
    auto new_item = [this](const std::wstring& r, uint32_t kind) {
        const std::wstring name = require(r, kind == 0 ? "newfolder" : "newfile");
        uint64_t op = 0;
        core::check(bb_new_item(s_.core().app(), tab(), core::s(L""), kind, core::s(name), &op), "new item");
        await_op(op);
    };
    verbs_[L"newfolder"] = [new_item](const std::wstring& r) { new_item(r, 0); };
    verbs_[L"newfile"] = [new_item](const std::wstring& r) { new_item(r, 1); };

    // ---- the search index banner and line ----
    verbs_[L"assert-banner"] = [this](const std::wstring& r) {
        const std::wstring expected = require(r, "assert-banner");
        s_.view().sync_overlays();
        const auto b = s_.view().indexer_banner();
        if (expected == L"none") {
            if (b.shown) throw AssertionError("expected no indexer banner; it says '" + to_utf8(b.message) + "'.");
            return;
        }
        if (expected == L"start") {
            if (!b.shown || !b.can_start) {
                throw AssertionError(std::string("expected the banner to offer to start the indexer; shown=") + (b.shown ? "true" : "false") +
                                     ", canStart=" + (b.can_start ? "true" : "false") + ".");
            }
            return;
        }
        if (!b.shown) throw AssertionError("expected an indexer banner containing '" + to_utf8(expected) + "'; there is none.");
        if (!core::icontains(b.message, expected)) throw AssertionError("expected the banner to contain '" + to_utf8(expected) + "', got '" + to_utf8(b.message) + "'.");
    };
    verbs_[L"assert-indexing"] = [this](const std::wstring& r) {
        const std::wstring expected = require(r, "assert-indexing");
        s_.view().sync_overlays();
        const std::wstring status = s_.view().indexing_status();
        const bool can_retry = s_.view().indexing_can_retry();
        if (expected == L"retry" || expected == L"no-retry") {
            if ((expected == L"retry") != can_retry) {
                throw AssertionError(std::string("expected ") + (expected == L"retry" ? "a retry to be offered" : "no retry to be offered") + "; the index line is '" + to_utf8(status) + "'.");
            }
            return;
        }
        if (!core::icontains(status, expected)) throw AssertionError("expected the index line to contain '" + to_utf8(expected) + "', got '" + to_utf8(status) + "'.");
    };
    verbs_[L"dismiss-banner"] = [this](const std::wstring&) {
        core::check(bb_index_do(s_.core().app(), core::s(L"dismiss-banner")), "dismiss-banner");
        s_.settle();
        s_.view().sync_overlays();
    };
    verbs_[L"start-indexer"] = [this](const std::wstring&) {
        s_.view().sync_overlays();
        const std::wstring before = s_.view().indexing_status() + (s_.view().indexer_banner().shown ? L"|banner" : L"");
        core::check(bb_index_do(s_.core().app(), core::s(L"start")), "start-indexer");
        const auto start = std::chrono::steady_clock::now();
        while (std::chrono::duration_cast<std::chrono::milliseconds>(std::chrono::steady_clock::now() - start).count() < 10000) {
            s_.settle();
            s_.view().sync_overlays();
            const std::wstring now = s_.view().indexing_status() + (s_.view().indexer_banner().shown ? L"|banner" : L"");
            if (now != before) return;
            Sleep(20);
        }
    };

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
        auto bitmap = s_.view().render_dialog(k);
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
        BbOwnedStr o{};
        core::check(bb_duplicates_scan(s_.core().app(), core::s(path.wstring()), 1, &o), "duplicates");
        auto& d = s_.view().duplicates();
        d.result = bb::json::parse(to_utf8(core::take(o)));
        d.has = true;
        d.ticked.clear();
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
