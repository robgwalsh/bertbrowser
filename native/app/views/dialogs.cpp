#include "views/dialogs.h"

#include "gfx/icons.gen.h"
#include "ui/pickers.h"
#include "views/command_ui.h"
#include "views/main_view.h"
#include "views/ops.h"

#include <windows.h>

#include <algorithm>
#include <cmath>
#include <map>

namespace bb::views {

using namespace bb::ui;
namespace f = forms;

// ---- formatting ----------------------------------------------------------------------------------------

std::wstring format_bytes(int64_t bytes) {
    wchar_t buf[48];
    const double b = static_cast<double>(bytes);
    if (bytes >= (1LL << 30)) swprintf_s(buf, L"%.3f GB", b / (1 << 30));
    else if (bytes >= (1LL << 20)) swprintf_s(buf, L"%.1f MB", b / (1 << 20));
    else if (bytes >= (1LL << 10)) swprintf_s(buf, L"%.1f KB", b / (1 << 10));
    else swprintf_s(buf, L"%lld B", static_cast<long long>(bytes));
    return buf;
}

std::wstring format_file_time(int64_t ticks) {
    if (ticks <= 0) return {};
    FILETIME ft{static_cast<DWORD>(ticks & 0xFFFFFFFF), static_cast<DWORD>(ticks >> 32)};
    SYSTEMTIME utc{}, local{};
    if (!FileTimeToSystemTime(&ft, &utc) || !SystemTimeToTzSpecificLocalTime(nullptr, &utc, &local)) return {};
    wchar_t buf[32];
    swprintf_s(buf, L"%04d-%02d-%02d %02d:%02d", local.wYear, local.wMonth, local.wDay, local.wHour, local.wMinute);
    return buf;
}

namespace {

struct Card {
    std::unique_ptr<Panel> root;
    StackPanel* body = nullptr;
    StackPanel* buttons = nullptr;
};

Card make_card(const std::wstring& title, float width) {
    Card c;
    c.root = std::make_unique<Panel>();
    c.root->pad = 1;
    c.root->width = width;
    c.root->style.bg = tok::OverlayBorder;
    auto* dock = c.root->emplace<DockPanel>();
    dock->style.bg = tok::OverlayBackground;
    auto* head = dock->emplace<Panel>();
    head->dock = Dock::Top;
    head->pad = 14;
    auto* t = head->emplace<TextBlock>(title);
    t->style.font.size = 16.0f;
    t->style.font.weight = 600;
    auto* foot = dock->emplace<StackPanel>(Orientation::Horizontal, 8.0f);
    foot->dock = Dock::Bottom;
    foot->pad = 12;
    foot->style.bg = tok::SurfaceBackground;
    auto* spacer = foot->emplace<Widget>();
    spacer->flex = 1;
    c.buttons = foot;
    c.body = dock->emplace<StackPanel>(Orientation::Vertical, 8.0f);
    c.body->dock = Dock::Fill;
    c.body->pad = 16;
    return c;
}

Button* add_button(Card& c, const wchar_t* label, std::function<void()> fn, bool primary = false) {
    auto* b = f::button(*c.buttons, label, std::move(fn), 104.0f);
    b->primary = primary;
    return b;
}

TextBlock* mono(Widget& parent, const std::wstring& t, Token fg = tok::TextPrimary, float size = 13.0f) {
    auto* tb = f::text(parent, t, fg, size);
    tb->style.font.family = gfx::Family::Mono;
    return tb;
}

Edit* labelled_edit(Widget& parent, const wchar_t* label, const std::wstring& value, const wchar_t* placeholder = L"") {
    f::text(parent, label, tok::TextSecondary, 13.0f);
    auto* e = f::field(parent, value, -1, nullptr, placeholder);
    e->width = -1;
    return e;
}

std::wstring parent_dir(const std::wstring& path) {
    const size_t cut = path.find_last_of(L"\\/");
    return cut == std::wstring::npos ? std::wstring() : path.substr(0, cut);
}

std::wstring join_names(const std::vector<SelectedRow>& rows, size_t limit) {
    std::wstring out;
    for (size_t i = 0; i < rows.size() && i < limit; ++i) out += (i ? L"\n" : L"") + rows[i].name;
    if (rows.size() > limit) out += L"\n\u2026and " + std::to_wstring(rows.size() - limit) + L" more";
    return out;
}

// ---- the builders --------------------------------------------------------------------------------------

using Builder = std::unique_ptr<Panel> (*)(DialogContext&);

std::unique_ptr<Panel> about(DialogContext& cx) {
    Card c = make_card(L"About BertBrowser", 420);
    auto* name = f::text(*c.body, L"BertBrowser", tok::TextPrimary, 22.0f, 600);
    (void)name;
    f::text(*c.body, L"Version " + core::w(bb_version()), tok::TextSecondary);
    f::note(*c.body, L"A file browser for Windows with a global search that reads the NTFS master file table and folder sizes that are measured once and remembered.");
    add_button(c, L"Close", cx.close, true);
    return std::move(c.root);
}

std::unique_ptr<Panel> new_item(DialogContext& cx, bool file) {
    Card c = make_card(file ? L"New file" : L"New folder", 460);
    BbApp* app = cx.main.core().app();
    const uint64_t tab = cx.main.active_tab();
    BbOwnedStr sugg{};
    std::wstring name = file ? L"New file.txt" : L"New folder";
    if (bb_new_item_suggest(app, tab, file ? 1 : 0, core::s(std::wstring(file ? L".txt" : L"")), &sugg) == BB_OK) name = core::take(sugg);
    if (file) {
        const json::Value types = try_model([&](BbOwnedStr* o) { return bb_new_file_types(app, o); });
        std::vector<std::wstring> labels;
        for (size_t i = 0; i < types.size(); ++i) labels.push_back(core::wide(types[i]["label"].str()));
        if (!labels.empty()) {
            f::text(*c.body, L"Type", tok::TextSecondary, 13.0f);
            f::combo(*c.body, labels, 0, 280, nullptr)->name = "NewFileType";
        }
    }
    auto* edit = labelled_edit(*c.body, L"Name", name);
    edit->name = "NewItemName";
    const json::Value plan = try_model([&](BbOwnedStr* o) { return bb_new_item_plan(app, tab, file ? 1 : 0, core::s(name), o); });
    const std::wstring target = core::wide(plan["target"].str());
    f::note(*c.body, plan["ok"].boolean(true) || plan.is_null() ? L"It will be made in " + core::wide(plan["directory"].str())
                                                                : core::wide(plan["rejected"].str()));
    add_button(c, L"Create", [cx, edit, file] {
        uint64_t op = 0;
        bb_new_item(cx.main.core().app(), cx.main.active_tab(), file ? 1 : 0, core::s(edit->text()), &op);
        cx.close();
    }, true);
    add_button(c, L"Cancel", cx.close);
    return std::move(c.root);
}

std::unique_ptr<Panel> rename_dialog(DialogContext& cx) {
    Card c = make_card(L"Rename", 460);
    const auto rows = cx.main.selected_rows();
    const bool many = rows.size() > 1;
    std::wstring text = rows.empty() ? L"" : rows[0].name;
    if (many) {
        const size_t dot = text.find_last_of(L'.');
        text = dot == std::wstring::npos || dot == 0 ? text : text.substr(0, dot);
    }
    auto* edit = labelled_edit(*c.body, many ? L"New name for all of them" : L"New name", text);
    edit->name = "RenameName";
    if (!rows.empty() && !many) {
        const size_t dot = text.find_last_of(L'.');
        edit->select(0, dot == std::wstring::npos || dot == 0 ? text.size() : dot);
    }
    f::note(*c.body, many ? std::to_wstring(rows.size()) + L" items are numbered: each keeps its own extension, so \"Holiday\" gives \"Holiday 1.txt\", \"Holiday 2.md\"."
                          : L"The extension stays unless you change it.");
    add_button(c, L"Rename", [cx, edit] {
        std::vector<std::wstring> paths;
        for (const auto& r : cx.main.selected_rows()) paths.push_back(r.path);
        std::vector<BbStr> views;
        for (const auto& p : paths) views.push_back(core::s(p));
        uint64_t op = 0;
        bb_op_rename(cx.main.core().app(), views.data(), static_cast<uint32_t>(views.size()), core::s(edit->text()), &op);
        cx.close();
    }, true);
    add_button(c, L"Advanced\u2026", [cx] {
        cx.close();
        cx.main.open_dialog(L"rename-advanced");
    });
    add_button(c, L"Cancel", cx.close);
    return std::move(c.root);
}

std::unique_ptr<Panel> rename_advanced(DialogContext& cx) {
    Card c = make_card(L"Rename (advanced)", 560);
    const auto rows = cx.main.selected_rows();
    labelled_edit(*c.body, L"Find", L"IMG_", L"Text to find in each name");
    labelled_edit(*c.body, L"Replace with", L"Holiday", L"What to put there instead");
    labelled_edit(*c.body, L"Template", L"{base} {n:000}{ext}", L"{base} {n} {ext} {date:yyyy-MM-dd}");
    auto* opts = f::row(*c.body, 16);
    f::check(*opts, L"Regular expression", false, nullptr);
    f::check(*opts, L"Match case", false, nullptr);
    auto* nums = f::row(*c.body, 8);
    f::text(*nums, L"Number from", tok::TextSecondary, 13.0f);
    f::field(*nums, L"1", 60, nullptr);
    f::text(*nums, L"by", tok::TextSecondary, 13.0f);
    f::field(*nums, L"1", 60, nullptr);
    f::text(*nums, L"Apply to", tok::TextSecondary, 13.0f);
    f::combo(*nums, {L"Name only", L"Name and extension", L"Extension only"}, 0, 170, nullptr);
    f::text(*c.body, L"Preview", tok::TextSecondary, 13.0f);
    auto* list = c.body->emplace<f::ItemList>();
    list->height = 120;
    std::vector<f::ListEntry> entries;
    int n = 1;
    for (const auto& r : rows) {
        f::ListEntry e;
        e.text = r.name;
        const size_t dot = r.name.find_last_of(L'.');
        e.detail = L"\u2192  Holiday " + std::to_wstring(n++) + (dot == std::wstring::npos ? L"" : r.name.substr(dot));
        entries.push_back(std::move(e));
    }
    list->set_entries(std::move(entries));
    add_button(c, L"Rename", cx.close, true);
    add_button(c, L"Cancel", cx.close);
    return std::move(c.root);
}

std::unique_ptr<Panel> delete_dialog(DialogContext& cx) {
    const auto rows = cx.main.selected_rows();
    Card c = make_card(rows.size() == 1 ? L"Delete \"" + rows[0].name + L"\"?" : L"Delete " + std::to_wstring(rows.size()) + L" items?", 460);
    auto* names = f::text(*c.body, join_names(rows, 8), tok::TextSecondary, 13.0f);
    names->wrap = true;
    f::text(*c.body, L"What should happen to " + std::wstring(rows.size() == 1 ? L"it?" : L"them?"));
    auto* rg = f::radios(*c.body, {L"Move to the Recycle Bin (you can get it back)", L"Delete permanently (no way back)"}, 0, nullptr);
    rg->name = "DeleteMode";
    add_button(c, L"Delete", [cx, rg] {
        std::vector<std::wstring> paths;
        for (const auto& r : cx.main.selected_rows()) paths.push_back(r.path);
        std::vector<BbStr> views;
        for (const auto& p : paths) views.push_back(core::s(p));
        uint64_t op = 0;
        bb_op_delete(cx.main.core().app(), views.data(), static_cast<uint32_t>(views.size()), rg->selected() == 1 ? BB_DELETE_PERMANENT : BB_DELETE_RECYCLE, &op);
        cx.close();
    }, true);
    add_button(c, L"Cancel", cx.close);
    return std::move(c.root);
}

std::unique_ptr<Panel> columns_dialog(DialogContext& cx) {
    Card c = make_card(L"Add a column", 520);
    cx.main.ensure_machine_properties();
    BbApp* app = cx.main.core().app();
    const uint64_t tab = cx.main.active_tab();
    auto* search = f::field(*c.body, L"", -1, nullptr, L"Search columns and properties");
    search->width = -1;
    search->name = "ColumnSearch";
    const json::Value st = try_model([&](BbOwnedStr* o) { return bb_columns_candidates(app, tab, core::s(std::wstring()), o); });
    auto* list = c.body->emplace<f::ItemList>();
    list->name = "ColumnCandidates";
    list->height = 360;
    std::vector<f::ListEntry> entries;
    std::vector<std::wstring> ids;
    for (size_t g = 0; g < st["groups"].size(); ++g) {
        f::ListEntry h;
        h.heading = true;
        h.text = core::wide(st["groups"][g]["title"].str());
        entries.push_back(std::move(h));
        ids.push_back(L"");
        const json::Value& items = st["groups"][g]["items"];
        for (size_t i = 0; i < items.size(); ++i) {
            f::ListEntry e;
            e.text = core::wide(items[i]["header"].str());
            e.detail = core::wide(items[i]["detail"].str());
            entries.push_back(std::move(e));
            ids.push_back(core::wide(items[i]["id"].str()));
        }
    }
    if (st["loading"].boolean() || !st["full"].boolean()) {
        f::ListEntry e;
        e.text = L"Reading what Windows can show\u2026";
        e.muted = true;
        entries.push_back(std::move(e));
        ids.push_back(L"");
    }
    list->set_entries(std::move(entries));
    list->on_activate = [cx, ids](int i) {
        if (i < 0 || static_cast<size_t>(i) >= ids.size() || ids[static_cast<size_t>(i)].empty()) return;
        bb_columns_do(cx.main.core().app(), cx.main.active_tab(), core::s(L"add"), core::s(ids[static_cast<size_t>(i)]));
    };
    add_button(c, L"Add", [list, ids, cx] {
        if (list->selected >= 0 && static_cast<size_t>(list->selected) < ids.size() && !ids[static_cast<size_t>(list->selected)].empty()) {
            bb_columns_do(cx.main.core().app(), cx.main.active_tab(), core::s(L"add"), core::s(ids[static_cast<size_t>(list->selected)]));
        }
    }, true);
    add_button(c, L"Close", cx.close);
    return std::move(c.root);
}

std::unique_ptr<Panel> properties(DialogContext& cx) {
    const auto rows = cx.main.selected_rows();
    Card c = make_card(rows.size() == 1 ? rows[0].name + L" properties" : L"Properties", 460);
    auto line = [&](const wchar_t* label, const std::wstring& value) {
        if (value.empty()) return;
        auto* r = f::row(*c.body, 8);
        f::row_label(*r, label, 110)->style.fg = tok::TextSecondary;
        auto* v = f::text(*r, value);
        v->flex = 1;
    };
    if (rows.empty()) {
        line(L"Folder", cx.main.core().tab_text(cx.main.active_tab(), BB_TEXT_PATH));
    } else if (rows.size() == 1) {
        const auto& r = rows[0];
        line(L"Name", r.name);
        line(L"Type", r.dir ? L"File folder" : L"File");
        line(L"Location", parent_dir(r.path));
        line(L"Size", r.size >= 0 ? format_bytes(r.size) + L" (" + std::to_wstring(r.size) + L" bytes)" : L"");
        line(L"Modified", format_file_time(r.modified));
        line(L"Created", format_file_time(r.created));
    } else {
        int64_t total = 0;
        size_t files = 0, dirs = 0;
        for (const auto& r : rows) {
            if (r.dir) ++dirs;
            else {
                ++files;
                total += std::max<int64_t>(0, r.size);
            }
        }
        line(L"Selected", std::to_wstring(rows.size()) + L" items");
        line(L"Contains", std::to_wstring(files) + L" files, " + std::to_wstring(dirs) + L" folders");
        line(L"Size", format_bytes(total));
        line(L"Location", parent_dir(rows[0].path));
    }
    add_button(c, L"Close", cx.close, true);
    return std::move(c.root);
}

std::unique_ptr<Panel> theme_editor(DialogContext& cx) {
    Card c = make_card(L"Customise colours", 760);
    BbApp* app = cx.main.core().app();
    const json::Value th = try_model([&](BbOwnedStr* o) { return bb_theme_state(app, o); });
    const json::Value tokens = try_model([&](BbOwnedStr* o) { return bb_theme_tokens(app, o); });
    f::text(*c.body, core::wide(th["name"].str()) + L" \u2014 changes are kept as your own version of the theme; the built-in is never edited.", tok::TextSecondary, 13.0f);
    auto* cols = f::row(*c.body, 14);
    cols->height = 380;
    auto* list = cols->emplace<f::ItemList>();
    list->name = "ThemeTokens";
    list->width = 360;
    std::vector<f::ListEntry> entries;
    std::string group;
    uint32_t first_color = 0xFF202020;
    int first_row = -1;
    for (size_t i = 0; i < tokens.size(); ++i) {
        const json::Value& t = tokens[i];
        if (t["group"].str() != group) {
            group = t["group"].str();
            f::ListEntry h;
            h.heading = true;
            h.text = core::wide(group);
            entries.push_back(std::move(h));
        }
        f::ListEntry e;
        e.text = core::wide(t["name"].str());
        char hex[16];
        const uint32_t argb = static_cast<uint32_t>(t["argb"].num());
        snprintf(hex, sizeof(hex), "#%06X", argb & 0xFFFFFF);
        e.detail = core::wide(hex) + (t["overridden"].boolean() ? L"  \u2022" : L"");
        e.swatch_on = true;
        e.swatch = argb;
        if (first_row < 0) {
            first_row = static_cast<int>(entries.size());
            first_color = argb;
        }
        entries.push_back(std::move(e));
    }
    list->set_entries(std::move(entries), first_row);
    auto* right = cols->emplace<StackPanel>(Orientation::Vertical, 8.0f);
    right->width = 310;
    auto* picker = right->emplace<ColorPicker>();
    picker->set_rgb(first_color & 0xFFFFFF);
    auto* hex = f::field(*right, L"", 120, nullptr);
    hex->set_text(picker->hex_text());
    picker->on_change = [hex, picker](uint32_t, bool) { hex->set_text(picker->hex_text()); };
    f::note(*right, L"Pick a colour in the list, then change it here. The window repaints as you drag.");
    add_button(c, L"Save as\u2026", cx.close);
    add_button(c, L"Revert", cx.close);
    add_button(c, L"Reset all", cx.close);
    add_button(c, L"Close", cx.close, true);
    return std::move(c.root);
}

std::unique_ptr<Panel> duplicates(DialogContext& cx) {
    Card c = make_card(L"Duplicate files", 720);
    DuplicatesState& d = cx.main.duplicates();
    if (!d.has) {
        f::note(*c.body, L"Nothing has been scanned yet. Find duplicates looks under the folder you are in.");
    } else {
        const json::Value& groups = d.result["groups"];
        f::text(*c.body, std::to_wstring(groups.size()) + L" groups \u2014 " + format_bytes(static_cast<int64_t>(d.result["wastedBytes"].num())) + L" could be reclaimed.", tok::TextSecondary, 13.0f);
        auto* list = c.body->emplace<f::ItemList>();
        list->name = "DuplicateRows";
        list->height = 320;
        std::vector<f::ListEntry> entries;
        for (size_t g = 0; g < groups.size(); ++g) {
            f::ListEntry h;
            h.heading = true;
            h.text = std::to_wstring(groups[g]["files"].size()) + L" copies of " + format_bytes(static_cast<int64_t>(groups[g]["sizeBytes"].num()));
            entries.push_back(std::move(h));
            for (size_t i = 0; i < groups[g]["files"].size(); ++i) {
                const json::Value& file = groups[g]["files"][i];
                f::ListEntry e;
                const bool ticked = d.ticked.count({g, i}) > 0;
                e.text = std::wstring(ticked ? L"\u2611  " : L"\u2610  ") + core::wide(file["name"].str());
                e.detail = core::wide(file["dir"].str().empty() ? file["path"].str() : file["dir"].str());
                e.muted = !ticked;
                entries.push_back(std::move(e));
            }
        }
        list->set_entries(std::move(entries));
    }
    add_button(c, L"Remove ticked", cx.close)->set_enabled(d.has && !d.ticked.empty());
    add_button(c, L"Close", cx.close, true);
    return std::move(c.root);
}

std::unique_ptr<Panel> search_syntax(DialogContext& cx) {
    Card c = make_card(L"Search syntax", 640);
    const json::Value sections = try_model([&](BbOwnedStr* o) { return bb_search_syntax_help(o); });
    auto* scroll = c.body->emplace<ScrollViewer>();
    scroll->height = 420;
    auto* col = scroll->emplace<StackPanel>(Orientation::Vertical, 4.0f);
    for (size_t s = 0; s < sections.size(); ++s) {
        f::text(*col, core::wide(sections[s]["title"].str()), tok::TextPrimary, 14.0f, 600);
        const std::wstring blurb = core::wide(sections[s]["blurb"].str());
        if (!blurb.empty()) f::note(*col, blurb);
        for (size_t i = 0; i < sections[s]["entries"].size(); ++i) {
            auto* r = f::row(*col, 12);
            auto* ex = mono(*r, core::wide(sections[s]["entries"][i]["example"].str()), tok::TextLink);
            ex->width = 230;
            auto* m = f::text(*r, core::wide(sections[s]["entries"][i]["meaning"].str()), tok::TextSecondary, 13.0f);
            m->flex = 1;
            m->wrap = true;
        }
        f::gap(*col, 6);
    }
    add_button(c, L"Close", cx.close, true);
    return std::move(c.root);
}

std::unique_ptr<Panel> compress(DialogContext& cx) {
    Card c = make_card(L"Compress", 480);
    const auto rows = cx.main.selected_rows();
    f::text(*c.body, rows.empty() ? L"The whole folder" : join_names(rows, 4), tok::TextSecondary, 13.0f)->wrap = true;
    labelled_edit(*c.body, L"Archive name", rows.size() == 1 ? rows[0].name + L".zip" : L"Archive.zip");
    f::text(*c.body, L"Format", tok::TextSecondary, 13.0f);
    f::combo(*c.body, {L"Zip", L"7z", L"Tar", L"Tar.gz"}, 0, 200, nullptr);
    f::text(*c.body, L"Compression", tok::TextSecondary, 13.0f);
    f::radios(*c.body, {L"Fastest", L"Normal", L"Smallest"}, 1, nullptr);
    f::text(*c.body, L"Where", tok::TextSecondary, 13.0f);
    auto* r = f::row(*c.body);
    auto* dest = f::field(*r, L"", 330, nullptr);
    dest->set_text(rows.empty() ? cx.main.core().tab_text(cx.main.active_tab(), BB_TEXT_PATH) : parent_dir(rows[0].path));
    f::button(*r, L"Browse\u2026", nullptr);
    add_button(c, L"Compress", cx.close, true);
    add_button(c, L"Cancel", cx.close);
    return std::move(c.root);
}

std::unique_ptr<Panel> extract(DialogContext& cx) {
    Card c = make_card(L"Extract", 480);
    const auto rows = cx.main.selected_rows();
    f::text(*c.body, rows.empty() ? L"The archive" : rows[0].name, tok::TextSecondary, 13.0f);
    f::text(*c.body, L"Extract to", tok::TextSecondary, 13.0f);
    f::radios(*c.body, {L"A folder named after the archive", L"This folder"}, 0, nullptr);
    f::text(*c.body, L"Destination", tok::TextSecondary, 13.0f);
    auto* r = f::row(*c.body);
    auto* dest = f::field(*r, L"", 330, nullptr);
    dest->set_text(rows.empty() ? cx.main.core().tab_text(cx.main.active_tab(), BB_TEXT_PATH) : parent_dir(rows[0].path));
    f::button(*r, L"Browse\u2026", nullptr);
    f::text(*c.body, L"If a file is already there", tok::TextSecondary, 13.0f);
    f::combo(*c.body, {L"Ask me each time", L"Skip it", L"Replace it", L"Keep both"}, 0, 240, nullptr);
    add_button(c, L"Extract", cx.close, true);
    add_button(c, L"Cancel", cx.close);
    return std::move(c.root);
}

std::unique_ptr<Panel> archive_password(DialogContext& cx) {
    Card c = make_card(L"This archive is protected", 440);
    f::note(*c.body, L"Enter the password to open it. It is kept for this session only.");
    auto* e = labelled_edit(*c.body, L"Password", L"\u2022\u2022\u2022\u2022\u2022\u2022\u2022\u2022");
    e->select(0, 0);
    f::check(*c.body, L"Show the password", false, nullptr);
    add_button(c, L"Unlock", cx.close, true);
    add_button(c, L"Cancel", cx.close);
    return std::move(c.root);
}

std::unique_ptr<Panel> elevation(DialogContext& cx) {
    Card c = make_card(L"Administrator permission needed", 500);
    auto* head = f::row(*c.body, 12);
    auto* shield = head->emplace<Icon>(icons::find("Shield"), 28.0f);
    shield->style.fg = tok::WarningForeground;
    auto* t = f::text(*head, L"Windows will not let this program change these items.", tok::WarningForeground, 15.0f, 600);
    t->wrap = true;
    t->flex = 1;
    f::note(*c.body, L"BertBrowser can ask for administrator rights for this one step only. Nothing else is run as administrator.");
    auto* list = c.body->emplace<f::ItemList>();
    list->height = 96;
    std::vector<f::ListEntry> entries;
    for (const auto& r : cx.main.selected_rows()) {
        f::ListEntry e;
        e.text = r.name;
        e.detail = parent_dir(r.path);
        entries.push_back(std::move(e));
    }
    if (entries.empty()) {
        f::ListEntry e;
        e.text = L"hosts";
        e.detail = L"C:\\Windows\\System32\\drivers\\etc";
        entries.push_back(std::move(e));
    }
    list->set_entries(std::move(entries));
    add_button(c, L"Continue", cx.close, true);
    add_button(c, L"Cancel", cx.close);
    return std::move(c.root);
}

std::unique_ptr<Panel> saved_search(DialogContext& cx) {
    Card c = make_card(L"Save this search", 480);
    BbApp* app = cx.main.core().app();
    const uint64_t tab = cx.main.active_tab();
    BbOwnedStr nm{};
    std::wstring name = L"Search";
    if (bb_saved_default_name(app, tab, false, &nm) == BB_OK) name = core::take(nm);
    auto* name_edit = labelled_edit(*c.body, L"Name", name);
    name_edit->name = "SavedSearchName";
    const std::wstring query = cx.main.core().tab_text(tab, BB_TEXT_SEARCH);
    auto* q = labelled_edit(*c.body, L"Search for", query);
    q->read_only = true;
    f::text(*c.body, L"Where to run it", tok::TextSecondary, 13.0f);
    auto* rg = f::radios(*c.body, {L"Wherever I am when I run it", L"Only in " + cx.main.core().tab_text(tab, BB_TEXT_PATH), L"Everywhere on this PC"}, 0, nullptr);
    rg->name = "SavedSearchScope";
    add_button(c, L"Save", [cx, name_edit, rg] {
        const wchar_t* scope[] = {L"current", L"folder", L"pc"};
        bb_saved_do(cx.main.core().app(), cx.main.active_tab(), core::s(L"save-search"), core::s(name_edit->text() + L" " + scope[rg->selected() < 0 ? 0 : rg->selected()]));
        cx.close();
    }, true);
    add_button(c, L"Cancel", cx.close);
    return std::move(c.root);
}

std::unique_ptr<Panel> select_pattern(DialogContext& cx) {
    Card c = make_card(L"Select by pattern", 420);
    labelled_edit(*c.body, L"Pattern", L"*.txt", L"*.jpg  report*  ?.md");
    f::note(*c.body, L"* matches any run of characters, ? matches one. Several patterns can be separated with a semicolon.");
    add_button(c, L"Select", cx.close, true);
    add_button(c, L"Cancel", cx.close);
    return std::move(c.root);
}

std::unique_ptr<Panel> flat_large(DialogContext& cx) {
    Card c = make_card(L"Flat view", 520);
    const std::wstring path = cx.main.core().tab_text(cx.main.active_tab(), BB_TEXT_PATH);
    f::text(*c.body, L"\"" + path + L"\" holds 120,000 files. A flat view lists the first 50,000 of them, which will take a moment.")->wrap = true;
    add_button(c, L"Show 50,000", cx.close, true);
    add_button(c, L"Cancel", cx.close);
    return std::move(c.root);
}

std::unique_ptr<Panel> saved_command_bars(DialogContext& cx) {
    Card c = make_card(L"Saved menus and toolbars", 460);
    const json::Value st = try_model([&](BbOwnedStr* o) { return bb_bars_query(cx.main.core().app(), core::s(L"state"), core::s(std::wstring()), o); });
    auto* list = c.body->emplace<f::ItemList>();
    list->height = 200;
    std::vector<f::ListEntry> entries;
    for (size_t i = 0; i < st["saved"].size(); ++i) {
        f::ListEntry e;
        e.text = core::wide(st["saved"][i]["name"].str());
        e.detail = core::wide(st["saved"][i]["shape"].str());
        entries.push_back(std::move(e));
    }
    if (entries.empty()) {
        f::ListEntry e;
        e.text = L"Nothing saved yet";
        e.muted = true;
        entries.push_back(std::move(e));
    }
    list->set_entries(std::move(entries));
    add_button(c, L"Close", cx.close, true);
    return std::move(c.root);
}

const std::map<std::wstring, Builder>& builders() {
    static const std::map<std::wstring, Builder> table = {
        {L"about", about},
        {L"new-folder", [](DialogContext& cx) { return new_item(cx, false); }},
        {L"new-file", [](DialogContext& cx) { return new_item(cx, true); }},
        {L"rename", rename_dialog},
        {L"rename-advanced", rename_advanced},
        {L"delete", delete_dialog},
        {L"columns", columns_dialog},
        {L"properties", properties},
        {L"theme-editor", theme_editor},
        {L"duplicates", duplicates},
        {L"search-syntax", search_syntax},
        {L"compress", compress},
        {L"extract", extract},
        {L"archive-password", archive_password},
        {L"elevation", elevation},
        {L"saved-search", saved_search},
        {L"select-pattern", select_pattern},
        {L"flat-large", flat_large},
        {L"saved-command-bars", saved_command_bars},
    };
    return table;
}

// The dialogs that live in their own files (views/ops_*.cpp); they win over a kind of the same name above.
const std::map<std::wstring, DialogBuilder>& extra_builders() {
    static const std::map<std::wstring, DialogBuilder> table = [] {
        std::map<std::wstring, DialogBuilder> t;
        ops::add_dialogs(t);
        return t;
    }();
    return table;
}

}  // namespace

bool dialog_exists(const std::wstring& kind) { return extra_builders().count(kind) > 0 || builders().count(kind) > 0; }

std::vector<std::wstring> dialog_kinds() {
    std::vector<std::wstring> out;
    for (const auto& kv : builders()) out.push_back(kv.first);
    for (const auto& kv : extra_builders()) {
        if (!builders().count(kv.first)) out.push_back(kv.first);
    }
    return out;
}

std::unique_ptr<Widget> build_dialog(const std::wstring& kind, DialogContext ctx) {
    if (const auto it = extra_builders().find(kind); it != extra_builders().end()) return it->second(ctx);
    const auto it = builders().find(kind);
    if (it == builders().end()) return nullptr;
    return it->second(ctx);
}

std::unique_ptr<gfx::BitmapSurface> render_dialog_detached(gfx::Resources& res, std::unique_ptr<Widget> card) {
    constexpr float kMargin = 24.0f;
    UiRoot root(res);
    root.caret_blink = false;
    auto* content = root.set_content(std::make_unique<StackPanel>(Orientation::Vertical, 0.0f));
    content->pad = kMargin;
    content->style.bg = tok::WindowBackground;
    Widget* c = content->add(std::move(card));
    const float w = c->width >= 0 ? c->width : 480.0f;
    const Size d = c->measure_tree({w, kInf});
    const Size size{w + 2 * kMargin, d.h + 2 * kMargin};
    root.layout(size);
    root.invalidate_all();
    HRESULT hr = S_OK;
    auto surface = gfx::BitmapSurface::create(res.dev, static_cast<UINT>(std::ceil(size.w)), static_cast<UINT>(std::ceil(size.h)), 96.0f, &hr);
    if (!surface) return nullptr;
    root.paint(*surface);
    return surface;
}

// ---- the layer ------------------------------------------------------------------------------------------

DialogLayer::DialogLayer() {
    style.bg = tok::Scrim;
    flags.visible = false;
}

void DialogLayer::show(std::unique_ptr<Widget> card) {
    close();
    card_ = add(std::move(card));
    set_visible(true);
    request_layout();
    invalidate();
}

void DialogLayer::close() {
    if (!card_) return;
    remove_child(card_);
    card_ = nullptr;
    set_visible(false);
    invalidate();
    if (on_closed) on_closed();
}

void DialogLayer::arrange(Rect r) {
    bounds = r;
    if (!card_) return;
    const Size d = card_->measure_tree({card_->width >= 0 ? card_->width : r.width() - 80, r.height() - 40});
    const float w = card_->width >= 0 ? card_->width : d.w;
    card_->arrange(Rect::xywh(r.l + (r.width() - w) / 2, r.t + std::max(20.0f, (r.height() - d.h) / 3), w, d.h));
}

}  // namespace bb::views
