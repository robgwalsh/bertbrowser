// The window-feature verbs, continued: saved searches and workspaces, columns, fixtures and the index banner.
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
#include "ui/popups/rich_tip.h"


namespace bbh {

namespace core = bb::core;
namespace ui = bb::ui;
namespace views = bb::views;
using namespace ui_util;

void Runner::install_ui_saved() {
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
}

void Runner::install_ui_columns() {
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
    // What the header itself does when a column is dragged or a gripper let go: the list's own columns
    // change and it says so, and the view hands that to the core. `columns move|width` skips the view.
    auto header_edit = [this](const std::wstring& id, const std::function<void(std::vector<ui::Column>&, size_t)>& edit) {
        views::TabView* tv = s_.view().active_view();
        const auto ids = tv->column_ids();
        std::vector<ui::Column> cols = tv->list()->columns();
        size_t at = cols.size();
        for (size_t i = 0; i < cols.size(); ++i) {
            const size_t k = static_cast<size_t>(cols[i].id);
            if (k < ids.size() && core::iequals(ids[k], id)) at = i;
        }
        if (at == cols.size()) throw AssertionError("the header has no '" + to_utf8(id) + "' column.");
        edit(cols, at);
        tv->list()->set_columns(cols);
        if (tv->list()->on_columns_changed) tv->list()->on_columns_changed();
        s_.settle();
        tv->reload_columns();
    };
    verbs_[L"header-drag"] = [this, header_edit](const std::wstring& r) {
        const auto [id, tail] = split_first(require(r, "header-drag"));
        const size_t to = static_cast<size_t>(std::max(0, number(tail, "header-drag")));
        header_edit(id, [to](std::vector<ui::Column>& cols, size_t at) {
            const ui::Column c = cols[at];
            cols.erase(cols.begin() + static_cast<std::ptrdiff_t>(at));
            cols.insert(cols.begin() + static_cast<std::ptrdiff_t>(std::min(to, cols.size())), c);
        });
    };
    verbs_[L"header-resize"] = [this, header_edit](const std::wstring& r) {
        const auto [id, tail] = split_first(require(r, "header-resize"));
        const float w = static_cast<float>(number(tail, "header-resize"));
        header_edit(id, [w](std::vector<ui::Column>& cols, size_t at) { cols[at].width = w; });
    };
    verbs_[L"assert-column-width"] = [this](const std::wstring& r) {
        const auto [id, tail] = split_first(require(r, "assert-column-width"));
        const int wanted = number(tail, "assert-column-width");
        views::TabView* tv = s_.view().active_view();
        tv->reload_columns();
        const auto ids = tv->column_ids();
        for (const ui::Column& c : tv->list()->columns()) {
            const size_t k = static_cast<size_t>(c.id);
            if (k >= ids.size() || !core::iequals(ids[k], id)) continue;
            if (std::abs(static_cast<int>(std::lround(c.width)) - wanted) > 1) {
                throw AssertionError("the '" + to_utf8(id) + "' column is " + std::to_string(std::lround(c.width)) + " wide, not " + std::to_string(wanted) + ".");
            }
            return;
        }
        throw AssertionError("there is no '" + to_utf8(id) + "' column.");
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
}

void Runner::install_ui_fixtures() {
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
}

void Runner::install_ui_banner() {
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
    // A running index helper with canned resource use, or none: what the title-bar button and the Search index
    // page's rows follow. Only the harness's index can be posed.
    verbs_[L"pose-indexer"] = [this](const std::wstring& r) {
        const std::wstring state = lower(require(r, "pose-indexer"));
        if (state != L"running" && state != L"off") throw FormatError("pose-indexer wants running or off, got '" + to_utf8(state) + "'.");
        core::check(bb_harness_pose_indexer(s_.core().app(), core::s(state)), "pose-indexer");
        s_.settle();
        s_.view().sync_overlays();
    };
    // pick open|save <folder or file> [name=<file name>] [filter=<*.a;*.b>] [title=<words>] -- another program
    // asks the user to choose a file, as its command line would (`--pick ... --reply-pipe ...`): the same
    // parser and the same request path as a second launch. The reply pipe is the run's recorder.
    verbs_[L"pick"] = [this](const std::wstring& r) {
        const std::vector<std::wstring> parts = words(require(r, "pick"));
        if (parts.size() < 2) throw FormatError("pick wants open or save, then a folder or file to start at.");
        static int picks = 0;
        std::wstring lines = L"--pick\n" + parts[0] + L"\n--reply-pipe\nharness-pick-" + std::to_wstring(++picks) + L"\n";
        std::wstring start = parts[1];
        for (size_t i = 2; i < parts.size(); ++i) {
            const size_t eq = parts[i].find(L'=');
            const std::wstring key = eq == std::wstring::npos ? L"" : lower(parts[i].substr(0, eq));
            if (key == L"name" || key == L"filter" || key == L"title") lines += L"--pick-" + key + L"\n" + parts[i].substr(eq + 1) + L"\n";
            else start += L" " + parts[i];  // a path with a space in it
        }
        lines += sandbox_.resolve(start).wstring();
        core::check(bb_harness_launch_args(s_.core().app(), core::s(lines)), "pick");
        s_.settle();
    };
    // The program that asked stops waiting (it was closed, or its own Cancel was pressed).
    verbs_[L"pose-asker-gone"] = [this](const std::wstring&) {
        core::check(bb_harness_pose_askers_gone(s_.core().app()), "pose-asker-gone");
        Sleep(600);  // the tab looks every 400 ms
        s_.settle();
    };
    // assert-reply waiting|cancelled|<path> -- what the last program that asked has received.
    verbs_[L"assert-reply"] = [this](const std::wstring& r) {
        const std::wstring want = require(r, "assert-reply");
        const bb::json::Value replies = bb::json::parse(s_.core().state_json())["replies"];
        if (replies.size() == 0) throw AssertionError("nothing has asked this run to choose a file.");
        const bb::json::Value& last = replies[replies.size() - 1];
        const std::string state = last["state"].str();
        const std::wstring got = state == "answered" ? core::wide(last["path"].str()) : core::wide(state);
        const std::wstring expected = (lower(want) == L"waiting" || lower(want) == L"cancelled") ? lower(want) : sandbox_.resolve(want).wstring();
        if (!core::iequals(got, expected)) throw AssertionError("the asker has `" + to_utf8(got) + "`, not `" + to_utf8(expected) + "`.");
    };
    // BertCut, the video editor a video is handed to, installed or not. A run starts with it absent whatever
    // this machine has, so the Edit button and its command are the same in every run.
    verbs_[L"pose-installed"] = [this](const std::wstring& r) {
        const std::wstring what = lower(require(r, "pose-installed"));
        if (what != L"bertcut" && what != L"none") throw FormatError("pose-installed wants bertcut or none, got '" + to_utf8(what) + "'.");
        core::check(bb_harness_pose_installed(s_.core().app(), core::s(what)), "pose-installed");
        s_.settle();
    };
    // locate-bertcut <path> -- answers "Where is BertCut?" as Windows' picker would have, with a sandbox file: a run
    // has nobody to answer the real one. The answer is kept and the interrupted Edit runs again.
    verbs_[L"locate-bertcut"] = [this](const std::wstring& r) {
        s_.view().bertcut_located(sandbox_.resolve(require(r, "locate-bertcut")).wstring());
        s_.settle();
    };
    // Shows a widget's tip as resting the pointer on it would (a run posts no mouse input), for a `shot`.
    verbs_[L"tip"] = [this](const std::wstring& r) {
        const std::wstring name = require(r, "tip");
        s_.view().refresh_tooltips();
        bb::ui::Widget* w = s_.view().find_named(to_utf8(name));
        if (!w) throw AssertionError("there is no element named " + to_utf8(name) + ".");
        if (!w->shown()) throw AssertionError(to_utf8(name) + " is not shown, so it cannot be hovered.");
        bb::ui::PopupLayer* layer = s_.view().popup_layer();
        if (!layer) throw AssertionError("the window has no popup layer.");
        layer->show_tip_for(w);
        if (!layer->tip_visible()) throw AssertionError(to_utf8(name) + " has no tip to show.");
    };
    // The tip showing contains this text (a plain tip's text, or any line of a rich one).
    verbs_[L"assert-tip"] = [this](const std::wstring& r) {
        const std::wstring expected = require(r, "assert-tip");
        bb::ui::PopupLayer* layer = s_.view().popup_layer();
        if (!layer || !layer->tip_visible()) throw AssertionError("no tip is showing.");
        std::wstring all = layer->tip_text();
        if (auto* rich = dynamic_cast<bb::ui::RichTip*>(layer->tip_content())) {
            const auto& c = rich->content();
            all = c.title + L" | " + c.subtitle + L" | ";
            for (const auto& row : c.rows) all += row.label + L": " + row.value + (row.detail.empty() ? L"" : L" " + row.detail) + L" | ";
            all += c.footer;
        }
        if (!core::icontains(all, expected)) throw AssertionError("the tip says \"" + to_utf8(all) + "\", which does not contain '" + to_utf8(expected) + "'.");
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
}
}  // namespace bbh
