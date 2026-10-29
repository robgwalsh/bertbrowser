#include "views/dialogs.h"
#include "views/shell_icons.h"
#include "views/image_view.h"

#include "gfx/icons.gen.h"
#include "ui/pickers.h"
#include "views/command_ui.h"
#include "views/main_view.h"
#include "views/tool_dialogs.h"
#include "views/ops.h"
#include "views/file_pickers.h"
#include "views/shell_props.h"
#include "views/tool_state.h"

#include <windows.h>

#include <algorithm>
#include <cmath>
#include <map>
#include <optional>
#include <cwctype>

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
    head->name = "CardHeader";  // a tool window shows the title in its own caption instead
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
    // The app's own icon beside the name, from the exe's resources (a harness build has none, and shows none).
    auto* head = f::row(*c.body, 16);
    if (HICON icon = static_cast<HICON>(LoadImageW(GetModuleHandleW(nullptr), MAKEINTRESOURCEW(1), IMAGE_ICON, 96, 96, LR_DEFAULTCOLOR))) {
        auto* logo = head->emplace<ImageView>();
        logo->name = "AboutLogo";
        logo->interactive = false;
        logo->checkerboard = false;
        logo->width = 48;
        logo->height = 48;
        logo->style.bg = gfx::kNoToken;
        logo->set_image(icon_picture(icon, 96));
        DestroyIcon(icon);
    }
    auto* words = f::column(*head, 4);
    words->flex = 1;
    f::text(*words, L"BertBrowser", tok::TextPrimary, 22.0f, 600);
    f::text(*words, L"Version " + core::w(bb_version()), tok::TextSecondary)->name = "AboutVersion";
    // Which update feed this copy follows. Spec: Views/AboutDialog.
    f::text(*words, bb_is_unstable() ? L"Unstable channel" : L"Stable channel", tok::TextSecondary)->name = "AboutChannel";
    f::note(*c.body, L"A file browser for Windows with a global search that reads the NTFS master file table and folder sizes that are measured once and remembered.");
    add_button(c, L"Close", cx.close, true);
    return std::move(c.root);
}

// New folder / New file / New <type>: a name, planned again on every keystroke so a bad or taken one is refused
// before anything is written, with Create only live for a name that will work. A type from the New submenu
// carries its extension, added when the name typed has none. Spec: Views/NewItemDialog.
std::unique_ptr<Panel> new_item(DialogContext& cx, bool file) {
    BbApp* app = cx.main.core().app();
    const uint64_t tab = cx.main.active_tab();
    // From the tree's menu, the folder right-clicked; otherwise (empty) the folder the tab shows.
    const std::wstring dir = cx.main.dialog_folder();
    const auto [ext, label] = cx.main.new_file_template();
    const std::wstring what = !file ? L"folder" : label.empty() ? L"file" : label;
    Card c = make_card(L"New " + what, 460);
    BbOwnedStr sugg{};
    std::wstring name = file ? L"New file" + ext : L"New folder";
    if (bb_new_item_suggest(app, tab, core::s(dir), file ? 1 : 0, core::s(ext), &sugg) == BB_OK) name = core::take(sugg);
    auto* edit = labelled_edit(*c.body, (L"Name for the new " + what + L":").c_str(), name);
    edit->name = "NewItemName";
    // The part that gets typed over, leaving a file's extension alone.
    const size_t dot = file ? name.rfind(L'.') : std::wstring::npos;
    edit->select(0, dot == std::wstring::npos || dot == 0 ? name.size() : dot);
    auto* where = f::note(*c.body, L"");
    where->name = "NewItemWhere";
    auto* problem = f::text(*c.body, L"", tok::ErrorForeground, 13.0f);
    problem->name = "NewItemProblem";
    problem->wrap = true;
    Button* create = add_button(c, L"Create", nullptr, true);
    create->name = "NewItemCreate";
    add_button(c, L"Cancel", cx.close);
    auto typed = [edit, file, ext = ext] {
        std::wstring t = edit->text();
        // Only a name with no extension of its own gets the type's: "notes." has none. Spec:
        // NewItemDialog (Path.GetExtension(typed).Length > 0).
        const size_t dot = t.find_last_of(L'.');
        const size_t slash = t.find_last_of(L"\\/");
        const bool has_extension = dot != std::wstring::npos && dot + 1 < t.size() && (slash == std::wstring::npos || dot > slash);
        if (file && !ext.empty() && !has_extension) t += ext;
        return t;
    };
    auto replan = [app, tab, dir, file, typed, where, problem, create] {
        const json::Value plan = try_model([&](BbOwnedStr* o) { return bb_new_item_plan(app, tab, core::s(dir), file ? 1 : 0, core::s(typed()), o); });
        const bool ok = !plan.is_null() && plan["ok"].boolean();
        where->set_text(plan.is_null() ? L"" : L"In: " + core::wide(plan["directory"].str()));
        problem->set_text(ok || plan.is_null() ? L"" : core::wide(plan["rejected"].str()));
        create->set_enabled(ok);
        where->invalidate();
        problem->invalidate();
        create->invalidate();
    };
    replan();
    edit->on_changed = [replan](const std::wstring&) { replan(); };
    create->on_click = [cx, typed, file, dir, replan, create] {
        replan();  // the disk may have changed while the dialog sat open
        if (!create->flags.enabled) return;
        uint64_t op = 0;
        bb_new_item(cx.main.core().app(), cx.main.active_tab(), core::s(dir), file ? 1 : 0, core::s(typed()), &op);
        cx.close();
    };
    edit->on_commit = [create] {
        if (create->flags.enabled && create->on_click) create->on_click();
    };
    return std::move(c.root);
}

// Add a column: the catalogue's columns and every property this machine reports, searched as it is typed. A
// column taken leaves the list at once, and Enter in the box adds the first match, so search-Enter-type-again
// is one session. Spec: Views/ColumnAddPanel.
// `defaults` is the Columns page's: the set new tabs start from rather than this tab's.
std::unique_ptr<Panel> columns_dialog(DialogContext& cx, bool defaults) {
    Card c = make_card(defaults ? L"Add a column to new tabs" : L"Add a column", 520);
    cx.main.ensure_machine_properties();
    BbApp* app = cx.main.core().app();
    const uint64_t tab = defaults ? 0 : cx.main.active_tab();
    auto* search = f::field(*c.body, L"", -1, nullptr, L"Search columns and properties");
    search->width = -1;
    search->name = "ColumnSearch";
    auto* list = c.body->emplace<f::ItemList>();
    list->name = "ColumnCandidates";
    list->height = 360;
    auto* status = f::text(*c.body, L"", tok::TextMuted, 12.5f);
    status->name = "ColumnStatus";
    status->wrap = true;
    auto ids = std::make_shared<std::vector<std::wstring>>();
    auto refill = [app, tab, search, list, status, ids] {
        const json::Value st = try_model([&](BbOwnedStr* o) { return bb_columns_candidates(app, tab, core::s(search->text()), o); });
        std::vector<f::ListEntry> entries;
        ids->clear();
        for (size_t g = 0; g < st["groups"].size(); ++g) {
            f::ListEntry h;
            h.heading = true;
            h.text = core::wide(st["groups"][g]["title"].str());
            entries.push_back(std::move(h));
            ids->push_back(L"");
            const json::Value& items = st["groups"][g]["items"];
            for (size_t i = 0; i < items.size(); ++i) {
                f::ListEntry e;
                e.text = core::wide(items[i]["header"].str());
                e.detail = core::wide(items[i]["detail"].str());
                entries.push_back(std::move(e));
                ids->push_back(core::wide(items[i]["id"].str()));
            }
        }
        list->set_entries(std::move(entries));
        status->set_text(st["full"].boolean()     ? L"That is all " + std::to_wstring(st["max"].i64()) + L" columns. Remove one to add another."
                         : st["loading"].boolean() ? L"Reading the rest of the property system\u2026"
                         : st["empty"].boolean()   ? L"No property matches that."
                                                   : L"");
        status->invalidate();
    };
    auto add = [app, tab, ids, refill](int i) {
        if (i < 0 || static_cast<size_t>(i) >= ids->size() || (*ids)[static_cast<size_t>(i)].empty()) return;
        if (tab == 0) bb_columns_defaults_do(app, core::s(L"add"), core::s((*ids)[static_cast<size_t>(i)]));
        else bb_columns_do(app, tab, core::s(L"add"), core::s((*ids)[static_cast<size_t>(i)]));
        refill();  // the column just taken leaves the list
    };
    search->on_changed = [refill](const std::wstring&) { refill(); };
    search->on_commit = [ids, add] {
        for (size_t i = 0; i < ids->size(); ++i) {
            if (!(*ids)[i].empty()) {
                add(static_cast<int>(i));
                return;
            }
        }
    };
    list->on_activate = add;
    refill();
    add_button(c, L"Add", [list, add] { add(list->selected); }, true)->name = "ColumnAdd";
    add_button(c, L"Close", cx.close);
    return std::move(c.root);
}

// The folder a tree menu's Properties is about, read from disk: the tree holds names, not sizes or dates.
std::vector<SelectedRow> folder_row(const std::wstring& folder) {
    SelectedRow r;
    r.path = folder;
    r.dir = true;
    const size_t cut = folder.find_last_of(L'\\');
    r.name = folder.size() <= 3 || cut == std::wstring::npos ? folder : folder.substr(cut + 1);
    WIN32_FILE_ATTRIBUTE_DATA data{};
    if (GetFileAttributesExW(folder.c_str(), GetFileExInfoStandard, &data)) {
        auto ticks = [](const FILETIME& t) { return static_cast<int64_t>((static_cast<uint64_t>(t.dwHighDateTime) << 32) | t.dwLowDateTime); };
        r.modified = ticks(data.ftLastWriteTime);
        r.created = ticks(data.ftCreationTime);
    }
    return {r};
}

// Properties: what the selection is, its Read-only and Hidden attributes to change (a box the items disagree on
// starts mixed and is left alone unless touched), the folder totals the MFT pass recorded, and for one file the
// shell's own list. Spec: Views/PropertiesDialog, ViewModels/PropertiesViewModel.
std::unique_ptr<Panel> properties(DialogContext& cx) {
    std::vector<std::wstring> paths;
    if (!cx.main.dialog_paths().empty()) {
        paths = cx.main.dialog_paths();  // a rebuild after Apply: the same items, even if the list no longer shows them
    } else if (!cx.main.dialog_folder().empty()) {
        paths.push_back(cx.main.dialog_folder());
    } else {
        for (const auto& r : cx.main.selected_rows()) paths.push_back(r.path);
        if (paths.empty()) paths.push_back(cx.main.core().tab_text(cx.main.active_tab(), BB_TEXT_PATH));
    }
    MainView* main = &cx.main;
    main->properties_paths() = paths;
    std::wstring error;
    json::Value m = try_tool_call(cx.main, "properties.model", "{\"paths\":" + json_array(paths) + "}", &error);
    Card c = make_card(m.is_null() ? L"Properties" : core::wide(m["title"].str()), 520);
    if (m.is_null()) {
        f::text(*c.body, error, tok::ErrorForeground, 13.0f)->wrap = true;
        add_button(c, L"Close", cx.close, true);
        return std::move(c.root);
    }
    // Values are read-only text boxes, so a path can be selected and copied. Spec: PropertiesDialog.xaml
    // (PropValue).
    auto value_box = [](Widget& parent, const std::wstring& value, bool multiline) {
        auto* e = parent.emplace<Edit>(multiline);
        e->plain = true;
        e->read_only = true;
        e->set_text(value);
        e->select(0, 0);
        if (!multiline) e->height = 22;  // a line of text, not a box
        return e;
    };
    auto line = [&](Widget& parent, const wchar_t* label, const std::wstring& value) {
        if (value.empty()) return;
        auto* r = f::row(parent, 8);
        f::row_label(*r, label, 110)->style.fg = tok::TextSecondary;
        value_box(*r, value, false)->flex = 1;
    };
    auto when = [](const json::Value& v) { return v.is_null() ? std::wstring() : format_file_time(v.i64()); };
    const bool single = m["single"].boolean();
    if (single) {
        // The item's own icon beside its name. Spec: PropertiesDialog.xaml (the header), PropertiesViewModel.Icon.
        const std::wstring path = paths.front();
        const DWORD attrs = GetFileAttributesW(path.c_str());
        const bool dir = attrs != INVALID_FILE_ATTRIBUTES && (attrs & FILE_ATTRIBUTE_DIRECTORY) != 0;
        ShellIcons* shell = &cx.main.shell_icons();
        auto* head = f::row(*c.body, 10);
        head->emplace<f::Picture>([shell, path, dir](uint32_t px) {
            return path.size() == 3 ? shell->drive(path, px) : shell->file(path, dir, dir ? FILE_ATTRIBUTE_DIRECTORY : 0, px);
        }, 32.0f)->width = 32;
        auto* title = f::text(*head, core::wide(m["name"].str()), tok::TextPrimary, 15.0f, 600);
        title->vcenter = true;
        title->flex = 1;
    }
    line(*c.body, L"Name", core::wide(m["name"].str()));
    line(*c.body, L"Type", core::wide(m["typeName"].str()));
    line(*c.body, single ? L"Location" : L"Where", core::wide(m["location"].str()));
    if (!single) {
        // Every name, in a box that scrolls past six or so. Spec: PropertiesDialog.xaml (Selected items).
        std::wstring names;
        for (size_t i = 0; i < m["itemNames"].size(); ++i) names += (i ? L"\n" : L"") + core::wide(m["itemNames"][i].str());
        if (!names.empty()) {
            f::text(*c.body, L"Selected items", tok::TextSecondary, 13.0f);
            auto* box = value_box(*c.body, names, true);
            box->name = "PropertiesItemNames";
            box->plain = false;
            box->height = std::min(120.0f, 10.0f + 20.0f * static_cast<float>(m["itemNames"].size()));
        }
    }
    line(*c.body, L"Size", m["size"].is_null() ? L"" : core::wide(m["size"].str()));
    if (single) {
        line(*c.body, L"Created", when(m["created"]));
        line(*c.body, L"Modified", when(m["modified"]));
        line(*c.body, L"Accessed", when(m["accessed"]));
    }

    // Folder contents: the recorded totals, or a plain "not indexed yet" - never a zero.
    if (m["hasDirectories"].boolean()) {
        f::gap(*c.body, 4);
        f::text(*c.body, L"Folder contents", tok::TextSecondary, 13.0f, 600);
        if (!m["folderSize"].is_null()) {
            line(*c.body, L"Size", core::wide(m["folderSize"].str()));
            line(*c.body, L"Contains", core::wide(m["folderCounts"].str()));
            f::note(*c.body, core::wide(m["sizeComputed"].str()));
        } else {
            f::note(*c.body, L"Not indexed yet \u2014 the size appears once the search index has measured it.");
        }
    }

    // Attributes, applied by the button and only to the boxes that changed.
    f::gap(*c.body, 4);
    f::text(*c.body, L"Attributes", tok::TextSecondary, 13.0f, 600);
    auto want_ro = std::make_shared<std::optional<bool>>();
    auto want_hidden = std::make_shared<std::optional<bool>>();
    Button* apply = nullptr;
    auto set_box = [](CheckBox* box, const json::Value& v) {
        if (v.is_null()) box->set_indeterminate(true);
        else box->set_checked(v.boolean());
    };
    auto* attrs = f::row(*c.body, 16);
    auto* ro = f::check(*attrs, L"Read-only", false, nullptr);
    ro->name = "PropertiesReadOnly";
    set_box(ro, m["readOnly"]);
    auto* hid = f::check(*attrs, L"Hidden", false, nullptr);
    hid->name = "PropertiesHidden";
    set_box(hid, m["hidden"]);
    f::note(*c.body, core::wide(m["attributesHint"].str()));
    if (!single) {
        ro->three_state = hid->three_state = true;
        f::note(*c.body, L"A filled box means the selected items differ; click it through to a checked or cleared state to apply that to all of them.")->wrap = true;
    }
    auto* problem = f::text(*c.body, m["error"].is_null() ? L"" : core::wide(m["error"].str()), tok::ErrorForeground, 13.0f);
    problem->name = "PropertiesError";
    problem->wrap = true;
    ro->set_enabled(!m["missing"].boolean());
    hid->set_enabled(!m["missing"].boolean());

    // Details: the shell's own list, for one file it may open. Read after the dialog is up (a handler can be
    // slow), and kept for the path while it is.
    const std::wstring details = m["detailsPath"].is_null() ? std::wstring() : core::wide(m["detailsPath"].str());
    if (single && !details.empty()) {
        f::gap(*c.body, 4);
        f::text(*c.body, L"Details", tok::TextSecondary, 13.0f, 600);
        auto& cache = cx.main.properties_details();
        if (cache.first != details) {
            f::note(*c.body, L"Reading\u2026")->name = "PropertiesDetailsLoading";
            cx.main.defer([main, details] {
                auto& slot = main->properties_details();
                if (slot.first == details) return;
                slot = {details, read_all_properties(details)};
                if (main->dialog_up() && main->dialog_kind() == L"properties") main->reopen_on(L"properties", main->properties_paths());
            });
        } else if (cache.second.empty()) {
            f::note(*c.body, L"Windows has no further details for this file.");
        } else {
            auto* scroll = c.body->emplace<ScrollViewer>();
            scroll->height = std::min<float>(220.0f, 8.0f + 22.0f * static_cast<float>(cache.second.size()));
            auto* list = scroll->emplace<StackPanel>(Orientation::Vertical, 2.0f);
            list->name = "PropertiesDetails";
            for (const auto& [name, value] : cache.second) line(*list, name.c_str(), value);
        }
    }

    apply = add_button(c, L"Apply", nullptr, false);
    apply->name = "PropertiesApply";
    apply->set_enabled(false);
    // Apply is live once a box differs from what the items have.
    const json::Value start_ro = m["readOnly"], start_hidden = m["hidden"];
    auto refresh = [apply, want_ro, want_hidden, start_ro, start_hidden] {
        auto differs = [](const std::optional<bool>& want, const json::Value& had) {
            return want.has_value() && (had.is_null() || had.boolean() != *want);
        };
        apply->set_enabled(differs(*want_ro, start_ro) || differs(*want_hidden, start_hidden));
        apply->invalidate();
    };
    // Back to mixed is back to leaving them alone.
    ro->on_toggle = [want_ro, refresh, ro](bool on) {
        *want_ro = ro->indeterminate() ? std::nullopt : std::optional<bool>(on);
        refresh();
    };
    hid->on_toggle = [want_hidden, refresh, hid](bool on) {
        *want_hidden = hid->indeterminate() ? std::nullopt : std::optional<bool>(on);
        refresh();
    };
    apply->on_click = [main, paths, want_ro, want_hidden] {
        std::string args = "{\"paths\":" + json_array(paths);
        if (*want_ro) args += std::string(",\"readOnly\":") + (**want_ro ? "true" : "false");
        if (*want_hidden) args += std::string(",\"hidden\":") + (**want_hidden ? "true" : "false");
        args += "}";
        try_tool_call(*main, "properties.apply", args);
        main->properties_details().first.clear();  // the shell's list includes the attributes just changed
        main->reopen_on(L"properties", paths);  // what is on disk now, and any failure said in the card
    };
    add_button(c, L"Close", cx.close, true);
    return std::move(c.root);
}

// The theme editor: every colour of the current theme, searchable, each changed live as the picker is dragged -
// the window repaints behind it, which is why it is a window of its own rather than a dialog over the list it
// is judged against. Changes are overrides on the theme until kept (Keep) or saved as a theme of the user's own;
// the built-ins are never edited. Spec: Views/ThemeEditorWindow, ViewModels/AppearanceViewModel.
std::unique_ptr<Panel> theme_editor(DialogContext& cx) {
    Card c = make_card(L"Customise colours", 830);
    MainView* main = &cx.main;
    BbApp* app = cx.main.core().app();
    ToolsUi& ui = tools_ui(cx.main);
    const json::Value th = try_model([&](BbOwnedStr* o) { return bb_theme_state(app, o); });
    const json::Value tokens = try_model([&](BbOwnedStr* o) { return bb_theme_tokens(app, o); });
    auto theme_do = [app](const wchar_t* verb, const std::wstring& arg) {
        BbOwnedStr out{};
        const BbStatus st = bb_theme_do(app, core::s(verb), core::s(arg), &out);
        const std::wstring text = st == BB_OK ? core::take(out) : core::last_error();
        return std::make_pair(st == BB_OK, text);
    };

    // Which theme.
    auto* top = f::row(*c.body, 8);
    f::text(*top, L"Theme", tok::TextSecondary, 13.0f);
    std::vector<std::wstring> names;
    std::vector<std::wstring> ids;
    int current = 0;
    for (size_t i = 0; i < th["available"].size(); ++i) {
        const json::Value& t = th["available"][i];
        names.push_back(core::wide(t["name"].str()));
        ids.push_back(core::wide(t["id"].str()));
        if (t["current"].boolean()) current = static_cast<int>(i);
    }
    f::combo(*top, names, current, 260, [main, ids, theme_do](int i) {
        if (i < 0 || static_cast<size_t>(i) >= ids.size()) return;
        theme_do(L"select", ids[static_cast<size_t>(i)]);
        tools_ui(*main).theme_token.clear();
        main->open_dialog(L"theme-editor");
    })->name = "ThemePicker";

    // Search, and the main colours or all of them.
    auto* filter = f::row(*c.body, 10);
    auto* search = f::field(*filter, ui.theme_search, 420, nullptr, L"Search colours");
    search->name = "ThemeSearch";
    auto* all = f::check(*filter, L"All colours", ui.theme_all, nullptr);
    all->name = "ThemeAllColours";
    all->tooltip = L"Show every colour the theme defines, not just the main ones";

    auto* cols = f::row(*c.body, 14);
    cols->height = 380;
    auto* list = cols->emplace<f::ItemList>();
    list->name = "ThemeTokens";
    list->width = 400;
    auto* right = cols->emplace<StackPanel>(Orientation::Vertical, 8.0f);
    right->width = 330;
    auto* which = f::text(*right, L"", tok::TextPrimary, 13.5f, 600);
    which->name = "ThemeTokenName";
    auto* about = f::text(*right, L"", tok::TextMuted, 12.5f);
    about->wrap = true;
    auto* picker = right->emplace<ColorPicker>();
    picker->name = "ThemeColour";
    auto* hex_row = f::row(*right, 8);
    auto* hex = f::field(*hex_row, L"", 110, nullptr);
    hex->name = "ThemeHex";
    auto* reset_one = f::button(*hex_row, L"Reset colour", nullptr, 120);
    reset_one->name = "ThemeResetToken";

    // The rows shown, filtered here, so typing in the search box never rebuilds the window under the caret.
    struct Shown {
        std::vector<std::string> keys;  // per entry; empty for a group heading
        std::map<std::string, uint32_t> argb;
        std::map<std::string, bool> overridden;
        std::map<std::string, std::pair<std::wstring, std::wstring>> words;  // name, description
    };
    auto shown = std::make_shared<Shown>();
    for (size_t i = 0; i < tokens.size(); ++i) {
        const json::Value& t = tokens[i];
        shown->argb[t["key"].str()] = static_cast<uint32_t>(t["argb"].num());
        shown->overridden[t["key"].str()] = t["overridden"].boolean();
        shown->words[t["key"].str()] = {core::wide(t["name"].str()), core::wide(t["description"].str())};
    }
    auto entry_of = [shown](const std::string& key) {
        f::ListEntry e;
        e.text = shown->words[key].first;
        char text[16];
        const uint32_t argb = shown->argb[key];
        snprintf(text, sizeof(text), "#%06X", argb & 0xFFFFFF);
        e.detail = core::wide(text) + (shown->overridden[key] ? L"  \u2022 changed" : L"");
        e.swatch_on = true;
        e.swatch = argb;
        return e;
    };
    auto fill = [list, shown, tokens, entry_of, main]() {
        const ToolsUi& st = tools_ui(*main);
        std::wstring needle = st.theme_search;
        for (auto& ch : needle) ch = static_cast<wchar_t>(std::towlower(ch));
        auto has = [&](const std::wstring& hay) {
            std::wstring low = hay;
            for (auto& ch : low) ch = static_cast<wchar_t>(std::towlower(ch));
            return needle.empty() || low.find(needle) != std::wstring::npos;
        };
        std::vector<f::ListEntry> entries;
        shown->keys.clear();
        std::string group;
        int select = -1;
        for (size_t i = 0; i < tokens.size(); ++i) {
            const json::Value& t = tokens[i];
            const std::string key = t["key"].str();
            if (!st.theme_all && !t["core"].boolean() && needle.empty()) continue;
            if (!has(shown->words[key].first) && !has(shown->words[key].second) && !has(core::wide(key))) continue;
            if (t["group"].str() != group) {
                group = t["group"].str();
                f::ListEntry h;
                h.heading = true;
                h.text = core::wide(group);
                entries.push_back(std::move(h));
                shown->keys.emplace_back();
            }
            if (key == st.theme_token) select = static_cast<int>(entries.size());
            entries.push_back(entry_of(key));
            shown->keys.push_back(key);
        }
        if (entries.empty()) {
            f::ListEntry none;
            none.text = L"No colour matches that.";
            none.muted = true;
            entries.push_back(std::move(none));
            shown->keys.emplace_back();
        }
        list->set_entries(std::move(entries), select);
    };

    // The colour being edited, in the picker and the box beside it.
    auto show_token = [shown, which, about, picker, hex, reset_one, main](const std::string& key) {
        tools_ui(*main).theme_token = key;
        const bool any = !key.empty() && shown->argb.count(key);
        which->set_text(any ? shown->words[key].first : L"Pick a colour in the list");
        about->set_text(any ? shown->words[key].second : L"");
        picker->set_enabled(any);
        hex->set_enabled(any);
        reset_one->set_enabled(any && shown->overridden[key]);
        if (any) {
            picker->set_argb(shown->argb[key]);
            hex->set_text(picker->hex_text());
        }
        which->invalidate();
        about->invalidate();
    };
    // A change: the override reaches the running app at once (the palette is re-pushed), and this row's swatch
    // follows without the list being rebuilt under the drag.
    auto revert_slot = std::make_shared<Button*>(nullptr);  // the footer's, made below; an edit makes it live
    auto set_colour = [shown, list, entry_of, theme_do, hex, reset_one, main, revert_slot](const std::wstring& text) {
        const std::string key = tools_ui(*main).theme_token;
        if (key.empty()) return;
        if (!theme_do(L"override", core::wide(key) + L" " + text).first) return;
        shown->argb[key] = ColorPicker::parse_hex(text).value_or(0xFF000000u);
        shown->overridden[key] = true;
        reset_one->set_enabled(true);
        if (*revert_slot) (*revert_slot)->set_enabled(true);
        for (size_t i = 0; i < shown->keys.size(); ++i) {
            if (shown->keys[i] == key) list->update_entry(static_cast<int>(i), entry_of(key));
        }
        (void)hex;
    };
    list->on_select = [shown, show_token](int i) {
        if (i >= 0 && static_cast<size_t>(i) < shown->keys.size() && !shown->keys[static_cast<size_t>(i)].empty()) show_token(shown->keys[static_cast<size_t>(i)]);
    };
    picker->on_change = [picker, hex, set_colour](uint32_t, bool) {
        hex->set_text(picker->hex_text());
        set_colour(picker->hex_text());
    };
    // Applied as it is typed, once it is a colour; anything else is marked rather than ignored.
    // Spec: ColorPicker.HexBox_TextChanged.
    hex->tooltip = L"#RRGGBB or #AARRGGBB";
    auto from_hex = [hex, picker, set_colour](bool commit) {
        const std::wstring t = hex->text();
        const auto v = ColorPicker::parse_hex(t);
        hex->style.border = v || t.empty() ? kNoToken : tok::ErrorBorder;
        hex->invalidate();
        if (!v) return;
        if (picker->argb() == *v && !commit) return;
        picker->set_argb(*v);
        set_colour(picker->hex_text());
    };
    hex->on_changed = [from_hex](const std::wstring&) { from_hex(false); };
    hex->on_commit = [from_hex] { from_hex(true); };
    reset_one->on_click = [main, theme_do] {
        const std::string key = tools_ui(*main).theme_token;
        if (key.empty()) return;
        theme_do(L"clear-override", core::wide(key));
        tools_ui(*main).theme_status = L"Colour reset.";
        main->open_dialog(L"theme-editor");
    };
    search->on_changed = [main, fill](const std::wstring& text) {
        tools_ui(*main).theme_search = text;
        fill();
    };
    all->on_toggle = [main, fill](bool on) {
        tools_ui(*main).theme_all = on;
        fill();
    };
    fill();
    show_token(ui.theme_token);

    auto* status = f::text(*c.body, ui.theme_status.empty() ? core::wide(th["name"].str()) + L" \u2014 changes are kept as your own version of the theme; the built-in is never edited." : ui.theme_status,
                           tok::TextSecondary, 13.0f);
    status->name = "ThemeStatus";
    status->wrap = true;

    auto say_and_rebuild = [main](std::wstring said) {
        tools_ui(*main).theme_status = std::move(said);
        main->open_dialog(L"theme-editor");
    };
    auto owner = [main]() {
        ToolWindow* w = main->tool_window(L"theme-editor");
        return w ? w->window().hwnd() : main->window().hwnd();
    };
    add_button(c, L"Import\u2026", [main, theme_do, say_and_rebuild, owner] {
        if (!main->shows_dialogs()) return;
        PickPath pick;
        pick.title = L"Import a theme";
        pick.filters = {{L"Theme files (*.json)", L"*.json"}, {L"All files (*.*)", L"*.*"}};
        const auto path = pick_open_file(owner(), pick);
        if (!path) return;
        const auto [ok, said] = theme_do(L"import", *path);
        say_and_rebuild(ok ? L"Imported the theme." : said);
    })->name = "ThemeImport";
    add_button(c, L"Export…", [main, th, theme_do, say_and_rebuild, owner] {
        if (!main->shows_dialogs()) return;
        PickPath pick;
        pick.title = L"Export the current theme";
        pick.file_name = core::wide(th["name"].str()) + L".json";
        pick.default_ext = L"json";
        pick.filters = {{L"Theme files (*.json)", L"*.json"}};
        const auto path = pick_save_file(owner(), pick);
        if (!path) return;
        const auto [ok, said] = theme_do(L"export", *path);
        say_and_rebuild(ok ? L"Theme exported." : said);
    })->name = "ThemeExport";
    add_button(c, L"Save as\u2026", [main, theme_do, say_and_rebuild, owner] {
        if (!main->shows_dialogs()) return;
        // The file name doubles as the theme's name, which saves asking for it twice.
        PickPath pick;
        pick.title = L"Save theme as";
        pick.file_name = L"My theme.json";
        pick.default_ext = L"json";
        pick.filters = {{L"Theme files (*.json)", L"*.json"}};
        const auto path = pick_save_file(owner(), pick);
        if (!path) return;
        std::wstring name = path->substr(path->find_last_of(L"\\/") + 1);
        if (const size_t dot = name.rfind(L'.'); dot != std::wstring::npos) name = name.substr(0, dot);
        const auto [ok, said] = theme_do(L"save-as", name);
        say_and_rebuild(ok ? L"Saved as \"" + name + L"\"." : said);
    })->name = "ThemeSaveAs";
    add_button(c, L"Reset all", [theme_do, say_and_rebuild] {
        theme_do(L"reset-all", L"");
        say_and_rebuild(L"All colours reset to the theme's defaults.");
    })->name = "ThemeResetAll";
    auto* revert = add_button(c, L"Revert", [theme_do, say_and_rebuild] {
        theme_do(L"revert", L"");
        say_and_rebuild(L"Unsaved colour changes discarded.");
    });
    revert->name = "ThemeRevert";
    revert->tooltip = L"Discard changes made since they were last kept";
    revert->set_enabled(th["dirty"].boolean());
    *revert_slot = revert;
    auto* keep = add_button(c, L"Keep", [theme_do, say_and_rebuild] {
        const auto [ok, said] = theme_do(L"persist", L"");
        say_and_rebuild(ok ? L"Colours saved." : said);
    }, true);
    keep->name = "ThemeKeep";
    keep->tooltip = L"Save these colours";
    add_button(c, L"Close", cx.close);
    return std::move(c.root);
}


std::unique_ptr<Panel> search_syntax(DialogContext& cx) {
    // Sized to what it says rather than a box that scrolls: a reference card is read whole. Spec:
    // SearchSyntaxDialog.xaml (SizeToContent).
    Card c = make_card(L"Search syntax", 640);
    const json::Value sections = try_model([&](BbOwnedStr* o) { return bb_search_syntax_help(o); });
    auto* col = c.body->emplace<StackPanel>(Orientation::Vertical, 4.0f);
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
    // The hint takes the place of a Close button: with any key or click closing it, a button would be the
    // least likely way anyone shuts it.
    auto* hint = f::text(*c.body, L"Click anywhere or press any key to close", tok::TextMuted, 11.0f);
    hint->name = "SearchSyntaxHint";
    c.buttons->set_visible(false);
    (void)cx;
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

// Save search: a name, the query (editable - saving is a chance to tidy it), and where it runs: wherever the
// tab is when it is run, pinned to this folder, or the whole PC. Checked as it is typed; a name already saved
// is replaced, and the button says so. Spec: Views/SavedSearchDialog.
std::unique_ptr<Panel> saved_search(DialogContext& cx) {
    // Editing one already saved: its own values, its name may stay as it is but may not take another's.
    const std::wstring editing = cx.main.editing_saved();
    Card c = make_card(editing.empty() ? L"Save search" : L"Edit saved search", 480);
    MainView* main = &cx.main;
    const json::Value seed = editing.empty() ? try_tool_call(cx.main, "search.save-seed")
                                             : try_tool_call(cx.main, "search.edit-seed", "{\"name\":" + json_quote(editing) + "}");
    auto* name = labelled_edit(*c.body, L"Name", core::wide(seed["name"].str()));
    name->name = "SavedSearchName";
    auto* query = labelled_edit(*c.body, L"Search for", core::wide(seed["query"].str()));
    query->name = "SavedSearchQuery";
    query->tooltip = L"Filters: ext: size: dm: path: is: re: in: content:, plus OR and !";
    const std::wstring folder = seed["folder"].is_null() ? std::wstring() : core::wide(seed["folder"].str());
    f::text(*c.body, L"Where it runs", tok::TextSecondary, 13.0f);
    const int start = seed["scope"].str() == "pc" ? 2 : seed["scope"].str() == "folder" && !seed["folder"].is_null() ? 1 : 0;
    auto* rg = f::radios(*c.body, {L"Wherever I am when I run it", L"Only in " + (folder.empty() ? std::wstring(L"(no folder to pin)") : folder), L"Everywhere on this PC"}, start, nullptr);
    rg->name = "SavedSearchScope";
    auto* problem = f::text(*c.body, L"", tok::ErrorForeground, 13.0f);
    problem->name = "SavedSearchProblem";
    problem->wrap = true;
    Button* save = add_button(c, L"Save", nullptr, true);
    save->name = "SavedSearchSave";
    add_button(c, L"Cancel", cx.close);
    auto args = [name, query, rg, folder, editing] {
        const wchar_t* scope[] = {L"current", L"folder", L"pc"};
        const int at = rg->selected() < 0 ? 0 : rg->selected();
        return "{\"name\":" + json_quote(name->text()) + ",\"query\":" + json_quote(query->text()) + ",\"scope\":" + json_quote(scope[at]) +
               ",\"folder\":" + (folder.empty() ? std::string("null") : json_quote(folder)) +
               ",\"previous\":" + (editing.empty() ? std::string("null") : json_quote(editing)) + "}";
    };
    auto check = [main, args, problem, save, rg, folder] {
        // Pinning needs a folder; with none, the choice falls back to "wherever I am".
        if (folder.empty() && rg->selected() == 1) rg->set_selected(0);
        const json::Value r = try_tool_call(*main, "search.save-check", args());
        const bool ok = r["problem"].is_null();
        problem->set_text(ok ? L"" : core::wide(r["problem"].str()));
        problem->invalidate();
        save->text = ok && r["replacing"].boolean() ? L"Replace" : L"Save";
        save->set_enabled(ok);
        save->invalidate();
    };
    name->on_changed = [check](const std::wstring&) { check(); };
    query->on_changed = [check](const std::wstring&) { check(); };
    rg->on_change = [check](int) { check(); };
    save->on_click = [main, args, name, close = cx.close] {
        std::wstring error;
        if (try_tool_call(*main, "search.save", args(), &error).is_null()) return;  // the strip already says why
        bb_status_set(main->core().app(), core::s(L"Saved the search \"" + name->text() + L"\""));
        close();
    };
    name->on_commit = [save] {
        if (save->flags.enabled && save->on_click) save->on_click();
    };
    check();
    return std::move(c.root);
}

// Renaming a saved workspace: one name, checked as it is typed; it may keep its own name but not take another's.
// Spec: Views/SavedWorkspaceDialog (editingName).
std::unique_ptr<Panel> rename_workspace(DialogContext& cx) {
    const std::wstring old = cx.main.editing_saved();
    Card c = make_card(L"Rename workspace", 460);
    MainView* main = &cx.main;
    auto* edit = labelled_edit(*c.body, L"Name", old);
    edit->name = "RenameWorkspaceName";
    edit->select(0, old.size());
    auto* note = f::text(*c.body, L"", tok::ErrorForeground, 13.0f);
    note->name = "RenameWorkspaceProblem";
    note->wrap = true;
    Button* ok = add_button(c, L"Rename", nullptr, true);
    ok->name = "RenameWorkspaceOk";
    add_button(c, L"Cancel", cx.close);
    auto check = [main, old, edit, note, ok] {
        const json::Value r = try_tool_call(*main, "workspace.rename-check", "{\"old\":" + json_quote(old) + ",\"name\":" + json_quote(edit->text()) + "}");
        note->set_text(r["problem"].is_null() ? L"" : core::wide(r["problem"].str()));
        ok->set_enabled(r["problem"].is_null());
        note->invalidate();
        ok->invalidate();
    };
    check();
    edit->on_changed = [check](const std::wstring&) { check(); };
    ok->on_click = [main, old, edit, ok, close = cx.close] {
        if (!ok->flags.enabled) return;
        if (bb_saved_do(main->core().app(), 0, core::s(L"rename-workspace"), core::s(old + L" to " + edit->text())) != BB_OK) return;
        close();
    };
    edit->on_commit = [ok] {
        if (ok->on_click) ok->on_click();
    };
    return std::move(c.root);
}

// Select / deselect by pattern: the search box's language, judged against the rows on screen, with the
// count said as it is typed so the answer is known before anything is selected. Spec: Views/SelectPatternDialog.
std::unique_ptr<Panel> select_pattern(DialogContext& cx, bool select) {
    Card c = make_card(select ? L"Select by pattern" : L"Deselect by pattern", 420);
    BbApp* app = cx.main.core().app();
    const uint64_t tab = cx.main.active_tab();
    auto* edit = labelled_edit(*c.body, select ? L"Select the items here that match" : L"Deselect the items here that match", L"",
                               L"*.jpg  report*  size:>1mb");
    edit->name = "SelectPattern";
    auto* result = f::text(*c.body, L"", tok::TextSecondary, 13.0f);
    result->name = "SelectPatternResult";
    result->wrap = true;
    f::note(*c.body, L"The search box's own language: *.jpg \u00b7 ext:png;gif \u00b7 size:>1mb \u00b7 dm:today \u00b7 is:folder \u00b7 re:^IMG_\\d+ \u00b7 and OR, ! and brackets.")->wrap = true;
    Button* ok = add_button(c, select ? L"Select" : L"Deselect", nullptr, true);
    ok->name = "SelectPatternOk";
    add_button(c, L"Cancel", cx.close)->name = "SelectPatternCancel";
    auto ask = [app, tab, select](const std::wstring& pattern, bool apply) {
        return try_model([&](BbOwnedStr* o) { return bb_select_pattern(app, tab, core::s(pattern), select, apply, o); });
    };
    auto refresh = [ask, result, ok](const std::wstring& pattern) {
        const json::Value st = ask(pattern, false);
        result->set_text(core::wide(st["summary"].str()));
        result->style.fg = st["problem"].is_null() ? tok::TextSecondary : tok::ErrorForeground;
        result->invalidate();
        ok->set_enabled(st["usable"].boolean());
        ok->invalidate();
    };
    refresh(L"");
    edit->on_changed = refresh;
    ok->on_click = [cx, ask, edit] {
        if (!ask(edit->text(), true)["usable"].boolean()) return;
        cx.close();
        cx.main.focus_file_list();
    };
    edit->on_commit = [ok] {
        if (ok->flags.enabled && ok->on_click) ok->on_click();
    };
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

// Save workspace / Save menus and toolbars: one name, checked as it is typed. A name already saved is
// replaced, and the button says so. Spec: Views/SavedWorkspaceDialog.
std::unique_ptr<Panel> save_named(DialogContext& cx, bool workspace) {
    Card c = make_card(workspace ? L"Save workspace" : L"Save menus and toolbars", 460);
    BbApp* app = cx.main.core().app();
    const std::wstring kind = workspace ? L"workspace" : L"command-bars";
    auto check = [app, kind](const std::wstring& name) {
        return try_model([&](BbOwnedStr* o) { return bb_save_name(app, core::s(kind), core::s(name), o); });
    };
    auto* edit = labelled_edit(*c.body, L"Name", core::wide(check(L"")["default"].str()));
    edit->name = "SaveName";
    const std::wstring about = workspace ? L"The panes and tabs as they are now, to switch back to by name."
                                         : L"The menu bar and toolbars as they are now, to switch back to by name.";
    auto* note = f::note(*c.body, about);
    Button* save = add_button(c, L"Save", nullptr, true);
    add_button(c, L"Cancel", cx.close);
    auto refresh = [check, note, save, about](const std::wstring& name) {
        const json::Value st = check(name);
        const bool ok = st["problem"].is_null();
        const bool replacing = ok && st["taken"].boolean();
        note->set_text(!ok ? core::wide(st["problem"].str()) : replacing ? L"This replaces the one already saved under this name." : about);
        save->text = replacing ? L"Replace" : L"Save";
        save->set_enabled(ok);
        save->invalidate();
    };
    refresh(edit->text());
    edit->on_changed = refresh;
    save->on_click = [cx, app, edit, workspace] {
        const std::wstring name = edit->text();
        const BbStatus st = workspace ? bb_saved_do(app, 0, core::s(L"save-workspace"), core::s(name))
                                      : bb_bars_do(app, core::s(L"save"), core::s(name));
        if (st != BB_OK) return;  // the name was refused: the note already says why
        const std::wstring said = workspace ? L"Saved the workspace \"" + name + L"\"" : L"Saved the menus and toolbars as \"" + name + L"\"";
        bb_status_set(app, core::s(said));
        cx.close();
    };
    edit->on_commit = [save] {
        if (save->flags.enabled && save->on_click) save->on_click();
    };
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
        {L"columns", [](DialogContext& cx) { return columns_dialog(cx, false); }},
        {L"default-columns", [](DialogContext& cx) { return columns_dialog(cx, true); }},
        {L"properties", properties},
        {L"theme-editor", theme_editor},
        {L"search-syntax", search_syntax},
        {L"elevation", elevation},
        {L"saved-search", saved_search},
        {L"rename-workspace", rename_workspace},
        {L"select-pattern", [](DialogContext& cx) { return select_pattern(cx, true); }},
        {L"deselect-pattern", [](DialogContext& cx) { return select_pattern(cx, false); }},
        {L"flat-large", flat_large},
        {L"saved-command-bars", saved_command_bars},
        {L"save-workspace", [](DialogContext& cx) { return save_named(cx, true); }},
        {L"save-command-bars", [](DialogContext& cx) { return save_named(cx, false); }},
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

bool dialog_exists(const std::wstring& kind) {
    return builders().count(kind) > 0 || extra_builders().count(kind) > 0 || tool_dialog_exists(kind);
}

std::vector<std::wstring> dialog_kinds() {
    std::vector<std::wstring> out;
    for (const auto& kv : builders()) out.push_back(kv.first);
    for (const auto& kv : extra_builders()) {
        if (!builders().count(kv.first)) out.push_back(kv.first);
    }
    for (const auto& kind : tool_dialog_kinds()) out.push_back(kind);
    std::sort(out.begin(), out.end());
    return out;
}

std::unique_ptr<Widget> build_dialog(const std::wstring& kind, DialogContext ctx) {
    if (const auto it = extra_builders().find(kind); it != extra_builders().end()) return it->second(ctx);
    const auto it = builders().find(kind);
    if (it == builders().end()) return tool_build_dialog(kind, ctx);
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
