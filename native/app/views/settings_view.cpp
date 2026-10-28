#include "views/settings_view.h"

#include "gfx/icons.gen.h"
#include "views/main_view.h"

#include <windows.h>

#include <algorithm>
#include <cmath>

namespace bb::views {

using namespace bb::ui;
namespace f = forms;

namespace {

std::string json_quote(const std::wstring& w) {
    const std::string s = core::utf8(w);
    std::string out = "\"";
    for (const char c : s) {
        switch (c) {
            case '\\': out += "\\\\"; break;
            case '"': out += "\\\""; break;
            case '\n': out += "\\n"; break;
            case '\t': out += "\\t"; break;
            default: out += c; break;
        }
    }
    return out + "\"";
}

std::wstring join_chords(const json::Value& chords) {
    std::wstring out;
    for (size_t i = 0; i < chords.size(); ++i) out += (i ? L", " : L"") + core::wide(chords[i].str());
    return out;
}

Widget* find_by_automation(Widget& w, const std::string& id) {
    if (w.automation_id == id) return &w;
    for (const auto& c : w.children()) {
        if (Widget* r = find_by_automation(*c, id)) return r;
    }
    return nullptr;
}

CheckBox* find_checkbox(Widget& w, const std::wstring& label) {
    if (auto* c = dynamic_cast<CheckBox*>(&w)) {
        if (core::iequals(c->text, label)) return c;
    }
    for (const auto& ch : w.children()) {
        if (CheckBox* r = find_checkbox(*ch, label)) return r;
    }
    return nullptr;
}

}  // namespace

SettingsView::SettingsView(MainView& main, core::Core& core) : main_(main), core_(core) {
    style.bg = tok::Scrim;
    flags.visible = false;
    build_chrome();
}

void SettingsView::build_chrome() {
    auto card = std::make_unique<DockPanel>();
    card->name = "SettingsCard";
    card->style.bg = tok::SurfaceBackground;
    card->style.border = tok::BorderDefault;
    card_ = add(std::move(card));

    auto* nav = card_->emplace<StackPanel>(Orientation::Vertical, 6.0f);
    nav->dock = Dock::Left;
    nav->width = 220;
    nav->pad = 10;
    nav->style.bg = tok::SurfaceRaised;
    nav_ = nav;

    problem_ = card_->emplace<TextBlock>();
    problem_->dock = Dock::Top;
    problem_->style.fg = tok::ErrorForeground;
    problem_->style.bg = tok::ErrorBackground;
    problem_->wrap = true;
    problem_->set_visible(false);

    scroll_ = card_->emplace<ScrollViewer>();
    scroll_->dock = Dock::Fill;
    scroll_->name = "SettingsScroll";
    body_ = scroll_->emplace<StackPanel>(Orientation::Vertical, 10.0f);
    body_->pad = 24;
    body_->name = "SettingsBody";
}

void SettingsView::arrange(Rect r) {
    bounds = r;
    const float w = std::min(1024.0f, std::max(320.0f, r.width() - 48));
    const float h = std::max(200.0f, r.height() - 48);
    const float x = r.l + (r.width() - w) / 2, y = r.t + (r.height() - h) / 2;
    card_->measure_tree({w, h});
    card_->arrange(Rect::xywh(x, y, w, h));
}

void SettingsView::paint(PaintContext& pc) {
    Widget::paint(pc);
}

bool SettingsView::on_mouse_down(const MouseEvent& e) {
    if (!card_->bounds.contains(e.pt)) {
        try {
            core::check(bb_settings_page_close(core_.app()), "close settings");
        } catch (const core::CoreError&) {
        }
    }
    return true;
}

// ---- reading the core --------------------------------------------------------------------------------------

json::Value SettingsView::get(const char* key) {
    return try_model([&](BbOwnedStr* o) { return bb_settings_get(core_.app(), core::s(core::wide(key)), o); });
}

void SettingsView::set_raw(const char* key, const std::string& json_value) {
    try {
        core::check(bb_settings_set(core_.app(), core::s(core::wide(key)), core::s(core::wide(json_value))), key);
        problem_->set_visible(false);
    } catch (const core::CoreError& e) {
        problem_->set_text(core::wide(e.what()));
        problem_->set_visible(true);
    }
}

void SettingsView::set_text(const char* key, const std::wstring& v, bool null_if_empty) {
    set_raw(key, v.empty() && null_if_empty ? "null" : json_quote(v));
}

void SettingsView::bool_row(Widget& p, const wchar_t* label, const char* key) {
    f::check(p, label, get(key).boolean(), [this, key](bool v) { set_bool(key, v); });
}

void SettingsView::number_row(Widget& p, const wchar_t* label, const char* key, const wchar_t* unit) {
    auto* row = f::row(p);
    f::row_label(*row, label, 320);
    const json::Value v = get(key);
    std::wstring text = v.is_null() ? L"" : core::wide(std::to_string(static_cast<long long>(v.num())));
    if (v.type == json::Value::Type::Number && v.num() != std::floor(v.num())) {
        char buf[32];
        snprintf(buf, sizeof(buf), "%g", v.num());
        text = core::wide(buf);
    }
    auto* e = f::field(*row, text, 120, [this, key](const std::wstring& t) {
        if (t.empty()) return;
        wchar_t* end = nullptr;
        const double d = std::wcstod(t.c_str(), &end);
        if (end && *end == 0) set_raw(key, core::utf8(t));
        (void)d;
    });
    e->automation_id = key;
    if (*unit) f::text(*row, unit, tok::TextMuted, 13.0f);
}

// ---- sync ----------------------------------------------------------------------------------------------------

void SettingsView::sync() {
    const json::Value st = try_model([&](BbOwnedStr* o) { return bb_settings_page_state(core_.app(), o); });
    const bool now_open = st["open"].boolean();
    const std::string page = st["page"].str();
    const bool was = open_;
    open_ = now_open;
    flags.visible = now_open;
    if (!now_open) {
        if (was) {
            request_layout();
            invalidate();
        }
        page_.clear();
        signature_.clear();
        return;
    }
    if (pages_.size() != st["pages"].size() || !was) {
        pages_ = st["pages"];
        build_nav(pages_);
    }
    if (page != page_) {
        page_ = page;
        for (size_t i = 0; i < nav_->children().size(); ++i) {
            if (auto* list = dynamic_cast<f::ItemList*>(nav_->children()[i].get())) {
                for (size_t k = 0; k < pages_.size(); ++k) {
                    if (pages_[k]["id"].str() == page_) list->select(static_cast<int>(k));
                }
            }
        }
        signature_.clear();
    }
    const std::string problem = st["problem"].str();
    if (!problem.empty()) {
        problem_->set_text(core::wide(problem));
        problem_->set_visible(true);
    }
    rebuild_if_changed(false);
    request_layout();
    invalidate();
}

void SettingsView::build_nav(const json::Value& pages) {
    while (!nav_->children().empty()) nav_->remove_child(nav_->children().front().get());
    auto* back = f::button(*nav_, L"←  Back", [this] {
        try {
            core::check(bb_settings_page_close(core_.app()), "close settings");
        } catch (const core::CoreError& e) {
            problem_->set_text(core::wide(e.what()));
            problem_->set_visible(true);
        }
    });
    back->name = "SettingsBack";
    auto* list = nav_->emplace<f::ItemList>();
    list->name = "SettingsPages";
    list->flex = 1;
    list->style.bg = gfx::kNoToken;
    list->style.border = gfx::kNoToken;
    std::vector<f::ListEntry> entries;
    for (size_t i = 0; i < pages.size(); ++i) {
        f::ListEntry e;
        e.text = core::wide(pages[i]["name"].str());
        e.icon = icon_for(pages[i]["icon"].str());
        entries.push_back(std::move(e));
    }
    list->row_height = 30;
    list->set_entries(std::move(entries));
    list->on_select = [this](int i) {
        if (i < 0 || static_cast<size_t>(i) >= pages_.size()) return;
        bb_settings_page_open(core_.app(), core::s(core::wide(pages_[static_cast<size_t>(i)]["id"].str())));
    };
}

void SettingsView::rebuild_if_changed(bool force) {
    // The signature is what the page will show: the models it draws from, as text.
    std::string sig = page_;
    auto add = [&](const json::Value& v) {
        sig += "|";
        sig += std::to_string(v.size());
    };
    (void)add;
    BbApp* app = core_.app();
    auto text_of = [&](auto call) {
        BbOwnedStr o{};
        if (call(&o) != BB_OK) return std::string();
        return core::utf8(core::take(o));
    };
    if (page_ == "command-bars") {
        sig += text_of([&](BbOwnedStr* o) { return bb_bars_query(app, core::s(L"page"), core::s(L""), o); });
    } else if (page_ == "keyboard") {
        sig += text_of([&](BbOwnedStr* o) { return bb_keys_page(app, o); });
    } else if (page_ == "appearance") {
        sig += text_of([&](BbOwnedStr* o) { return bb_theme_state(app, o); });
    } else if (page_ == "saved-searches" || page_ == "workspaces") {
        sig += text_of([&](BbOwnedStr* o) { return bb_saved_state(app, o); });
    } else if (page_ == "columns") {
        sig += text_of([&](BbOwnedStr* o) { return bb_columns_state(app, main_.active_tab(), o); });
    } else if (page_ == "search-index") {
        sig += text_of([&](BbOwnedStr* o) { return bb_index_state(app, o); });
    }
    if (!force && sig == signature_) return;
    signature_ = sig;
    build_page();
}

void SettingsView::build_page() {
    // Keep a box somebody is typing in: the page is rebuilt under it whenever a value it shows moves.
    std::string focus_id;
    size_t caret = 0;
    if (Widget* fw = root() ? root()->focused() : nullptr) {
        if (body_->is_ancestor_of(fw) && !fw->automation_id.empty()) {
            focus_id = fw->automation_id;
            if (auto* e = dynamic_cast<Edit*>(fw)) caret = e->caret();
        }
    }
    const float keep_scroll = scroll_ ? scroll_->offset_y() : 0.0f;
    while (!body_->children().empty()) body_->remove_child(body_->children().front().get());
    if (page_ == "general") page_general(*body_);
    else if (page_ == "appearance") page_appearance(*body_);
    else if (page_ == "preview") page_preview(*body_);
    else if (page_ == "search-index") page_search_index(*body_);
    else if (page_ == "history") page_history(*body_);
    else if (page_ == "new-items") page_new_items(*body_);
    else if (page_ == "columns") page_columns(*body_);
    else if (page_ == "saved-searches") page_saved(*body_, false);
    else if (page_ == "workspaces") page_saved(*body_, true);
    else if (page_ == "context-menu") page_context_menu(*body_);
    else if (page_ == "command-bars") page_bars(*body_);
    else if (page_ == "keyboard") page_keys(*body_);
    request_layout();
    if (root()) {
        if (!focus_id.empty()) {
            if (Widget* nw = find_by_automation(*body_, focus_id)) {
                root()->set_focus(nw);
                if (auto* e = dynamic_cast<Edit*>(nw)) e->select(caret, caret);
            }
        }
        root()->layout(root()->size());
    }
    if (scroll_ && keep_scroll > 0) scroll_->scroll_to(0, keep_scroll);
}

CheckBox* SettingsView::find_check(const std::wstring& label) { return find_checkbox(*body_, label); }

void SettingsView::pose_column_drag(bool on) {
    dragging_pose_ = on;
    signature_.clear();
    sync();
}

// ---- pages -----------------------------------------------------------------------------------------------------

void SettingsView::page_general(Widget& p) {
    f::heading(p, L"General");
    f::text(p, L"Browsing", tok::TextSecondary, 14.0f, 600);
    bool_row(p, L"Show hidden items", "showHiddenItems");
    bool_row(p, L"Open archives as folders", "enterArchivesOnDoubleClick");
    f::check(p, L"Middle-click a drive/device to open a new panel", get("drivesOpenTarget").str() == "NewPanel",
             [this](bool v) { set_raw("drivesOpenTarget", v ? "\"NewPanel\"" : "\"NewTab\""); });
    number_row(p, L"Scroll speed (1 is normal)", "scrollSpeedMultiplier");
    f::gap(p, 6);
    f::text(p, L"Startup", tok::TextSecondary, 14.0f, 600);
    bool_row(p, L"Restore last session on startup", "restoreLastSession");
    auto* row = f::row(p);
    f::row_label(*row, L"Open this folder when there is no session", 320);
    const json::Value path = get("startupDefaultPath");
    auto* e = f::field(*row, core::wide(path.str()), 280, [this](const std::wstring& t) { set_text("startupDefaultPath", t, true); },
                       L"The folder BertBrowser opens in");
    e->automation_id = "startupDefaultPath";
    f::button(*row, L"Browse…", [] {});
    f::gap(p, 6);
    f::text(p, L"Windows", tok::TextSecondary, 14.0f, 600);
    f::note(p, L"BertBrowser can open folders in place of File Explorer. The setting is changed in Windows, so nothing here moves until you choose it.");
    f::check(p, L"Open folders in BertBrowser instead of File Explorer", false, [](bool) {});
}

void SettingsView::page_appearance(Widget& p) {
    f::heading(p, L"Appearance");
    const json::Value th = try_model([&](BbOwnedStr* o) { return bb_theme_state(core_.app(), o); });
    const bool following = th["following"].boolean();
    auto theme_do = [this](const std::wstring& verb, const std::wstring& arg = L"") {
        BbOwnedStr o{};
        const BbStatus st = bb_theme_do(core_.app(), core::s(verb + (arg.empty() ? L"" : L" " + arg)), core::s(L""), &o);
        if (st == BB_OK) bb_str_free(o);
    };
    auto* follow = f::check(p, L"Match Windows light/dark", following, [this](bool v) {
        BbOwnedStr o{};
        if (bb_theme_do(core_.app(), core::s(L"follow"), core::s(v ? L"on" : L"off"), &o) == BB_OK) bb_str_free(o);
    });
    follow->name = "FollowSystemTheme";
    const json::Value& available = th["available"];
    std::vector<std::wstring> names;
    std::vector<std::string> ids;
    int current = -1, light = -1, dark = -1;
    for (size_t i = 0; i < available.size(); ++i) {
        names.push_back(core::wide(available[i]["name"].str()));
        ids.push_back(available[i]["id"].str());
        if (available[i]["id"].str() == th["id"].str()) current = static_cast<int>(i);
        if (available[i]["id"].str() == th["lightSlot"].str()) light = static_cast<int>(i);
        if (available[i]["id"].str() == th["darkSlot"].str()) dark = static_cast<int>(i);
    }
    if (following) {
        const bool os_dark = th["appearance"]["dark"].boolean();
        const bool hc = th["appearance"]["highContrast"].boolean();
        f::note(p, hc ? L"Windows is using a high contrast theme, so that is what BertBrowser shows. Your light and dark choices come back when it is off."
                      : os_dark ? L"Windows is set to dark, so BertBrowser is showing the dark theme below."
                                : L"Windows is set to light, so BertBrowser is showing the light theme below.");
        auto* r1 = f::row(p);
        f::row_label(*r1, L"Light theme", 160);
        auto* c1 = f::combo(*r1, names, light, 240, [this, ids](int i) {
            BbOwnedStr o{};
            if (i >= 0 && bb_theme_do(core_.app(), core::s(L"slot"), core::s(L"light " + core::wide(ids[static_cast<size_t>(i)])), &o) == BB_OK) bb_str_free(o);
        });
        c1->name = "LightThemeCombo";
        f::button(*r1, L"Customise colours…", [this] { main_.open_dialog(L"theme-editor"); });
        auto* r2 = f::row(p);
        f::row_label(*r2, L"Dark theme", 160);
        auto* c2 = f::combo(*r2, names, dark, 240, [this, ids](int i) {
            BbOwnedStr o{};
            if (i >= 0 && bb_theme_do(core_.app(), core::s(L"slot"), core::s(L"dark " + core::wide(ids[static_cast<size_t>(i)])), &o) == BB_OK) bb_str_free(o);
        });
        c2->name = "DarkThemeCombo";
        f::button(*r2, L"Customise colours…", [this] { main_.open_dialog(L"theme-editor"); });
    } else {
        auto* r = f::row(p);
        f::row_label(*r, L"Theme", 160);
        auto* c = f::combo(*r, names, current, 240, [this, ids](int i) {
            BbOwnedStr o{};
            if (i >= 0 && bb_theme_do(core_.app(), core::s(L"select"), core::s(core::wide(ids[static_cast<size_t>(i)])), &o) == BB_OK) bb_str_free(o);
        });
        c->name = "ThemeCombo";
        f::button(*r, L"Customise colours…", [this] { main_.open_dialog(L"theme-editor"); });
    }
    (void)theme_do;
    f::gap(p, 6);
    f::text(p, L"Preview", tok::TextSecondary, 14.0f, 600);
    // A strip of the colours the theme is made of, so a change is seen without leaving the page.
    auto* swatches = f::row(p, 4);
    swatches->height = 28;
    static const Token toks[] = {tok::WindowBackground, tok::SurfaceBackground, tok::SurfaceRaised, tok::ListSelectedBackground,
                                 tok::AccentBackground, tok::TextPrimary, tok::TextMuted, tok::ErrorForeground, tok::WarningForeground};
    for (Token t : toks) {
        auto* w = swatches->emplace<Widget>();
        w->width = 40;
        w->style.bg = t;
        w->style.border = tok::BorderDefault;
    }
}

void SettingsView::page_preview(Widget& p) {
    f::heading(p, L"Preview");
    bool_row(p, L"Show the preview pane in new tabs", "showPreviewPane");
    bool_row(p, L"Play the next item automatically", "previewAutoAdvance");
    bool_row(p, L"Loop what is playing", "previewLoop");
    bool_row(p, L"Start playing as soon as an item is selected", "previewAutoPlay");
    number_row(p, L"Largest text file shown in full", "previewTextMaxBytes", L"bytes");
    auto* r = f::row(p);
    f::row_label(*r, L"Thumbnail shape (width:height)", 320);
    f::field(*r, core::wide(get("tileAspectRatio").str()), 120, [this](const std::wstring& t) { set_text("tileAspectRatio", t); });
}

void SettingsView::page_search_index(Widget& p) {
    f::heading(p, L"Search index");
    const json::Value st = try_model([&](BbOwnedStr* o) { return bb_index_state(core_.app(), o); });
    f::note(p, L"Searching the whole PC reads the NTFS master file table, which needs administrator rights. A small helper does that "
               L"and keeps running when BertBrowser closes.");
    auto* r = f::row(p);
    const std::string status = st["status"].str();
    auto* t = f::text(*r, status.empty() ? L"The search index is not running." : core::wide(status));
    t->flex = 1;
    auto* start = f::button(*r, L"Start", [this] { bb_index_do(core_.app(), core::s(L"start")); });
    start->set_enabled(st["canStart"].boolean());
    auto* stop = f::button(*r, L"Stop", [] {});
    stop->set_enabled(false);
    f::gap(p, 6);
    bool_row(p, L"Start the indexer when BertBrowser launches", "startIndexerAtLaunch");
    number_row(p, L"Search inside files up to", "searchContentMaxBytes", L"bytes");
    number_row(p, L"Duplicate finder skips files smaller than", "duplicateMinSizeBytes", L"bytes");
    bool_row(p, L"Duplicate finder skips system folders", "duplicateSkipSystemFolders");
}

void SettingsView::page_history(Widget& p) {
    f::heading(p, L"History");
    f::note(p, L"Features that remember what you did are off until you turn them on here, and each has a way to clear it.");
    f::text(p, L"What changed", tok::TextSecondary, 14.0f, 600);
    bool_row(p, L"Record file changes", "recordFileChanges");
    f::note(p, L"Recording continues while BertBrowser is closed, so the timeline has no holes. Turning it off deletes what was recorded.");
    number_row(p, L"Keep changes for", "fileChangeRetentionHours", L"hours");
    f::button(p, L"Clear history now", [this] {
        // The recorded changes, gone; recording itself stays as the switch above has it.
        BbOwnedStr out{};
        const std::wstring verb = L"changes.clear", args = L"{}";
        if (bb_tools(core_.app(), core::s(verb), core::s(args), &out) == BB_OK) bb_str_free(out);
    });
    f::gap(p, 6);
    f::text(p, L"Undo", tok::TextSecondary, 14.0f, 600);
    f::note(p, L"Undo lasts for this session. The budget is released oldest first; the next undo is never released.");
    number_row(p, L"Steps to remember", "undoMaxSteps");
    number_row(p, L"Most data to hold", "undoMaxHeldGigabytes", L"GB");
    auto* clear = f::button(p, L"Clear the undo history now", [this] { bb_undo_clear(core_.app()); });
    clear->name = "ClearUndoHistory";
    clear->width = 220;
}

void SettingsView::page_new_items(Widget& p) {
    f::heading(p, L"New items");
    f::note(p, L"The file types the New menu offers, besides a folder and an empty file.");
    const json::Value types = try_model([&](BbOwnedStr* o) { return bb_new_file_types(core_.app(), o); });
    auto* list = p.emplace<f::ItemList>();
    list->height = 260;
    std::vector<f::ListEntry> entries;
    for (size_t i = 0; i < types.size(); ++i) {
        f::ListEntry e;
        e.text = core::wide(types[i]["label"].str());
        e.detail = core::wide(types[i]["extension"].str());
        entries.push_back(std::move(e));
    }
    list->set_entries(std::move(entries));
    auto* r = f::row(p);
    f::button(*r, L"Add", [] {});
    f::button(*r, L"Remove", [] {});
    f::button(*r, L"Import from Windows…", [] {});
}

void SettingsView::page_columns(Widget& p) {
    f::heading(p, L"Columns");
    f::note(p, L"The columns the file list shows. Name is always first. Drag a header in the list to reorder; right-click a header to add or remove.");
    const json::Value st = try_model([&](BbOwnedStr* o) { return bb_columns_state(core_.app(), main_.active_tab(), o); });
    auto* list = p.emplace<f::ItemList>();
    list->name = "ColumnRows";
    list->row_height = 34;
    list->height = 34.0f * static_cast<float>(std::max<size_t>(3, std::min<size_t>(10, st["columns"].size()))) + 4;
    std::vector<f::ListEntry> entries;
    for (size_t i = 0; i < st["columns"].size(); ++i) {
        const json::Value& c = st["columns"][i];
        if (c["injected"].boolean()) continue;
        f::ListEntry e;
        e.text = L"⋮⋮  " + core::wide(c["header"].str());
        std::wstring detail = core::wide(c["id"].str());
        if (c["property"].boolean()) detail += L"    " + core::wide(c["canonical"].str());
        e.detail = detail + L"    " + std::to_wstring(static_cast<int>(c["width"].num())) + L" px";
        entries.push_back(std::move(e));
    }
    list->set_entries(std::move(entries));
    if (dragging_pose_) {
        // The line a header drag leaves between two columns, placed rather than dragged.
        auto* line = p.emplace<Widget>();
        line->height = 2;
        line->style.bg = tok::AccentBackground;
    }
    auto* r = f::row(p);
    f::button(*r, L"Reset", [this] { bb_columns_do(core_.app(), main_.active_tab(), core::s(L"reset"), core::s(L"")); });
    f::button(*r, L"Use for new tabs", [this] { bb_columns_do(core_.app(), main_.active_tab(), core::s(L"default"), core::s(L"")); });
}

void SettingsView::page_saved(Widget& p, bool workspaces) {
    f::heading(p, workspaces ? L"Workspaces" : L"Saved searches");
    f::note(p, workspaces ? L"A workspace is the arrangement of panes and tabs, saved under a name." : L"A saved search keeps a query under a name, to run again in one click.");
    const json::Value st = try_model([&](BbOwnedStr* o) { return bb_saved_state(core_.app(), o); });
    const char* key = workspaces ? "workspacesPlacement" : "savedSearchesPlacement";
    const std::string cur = st[workspaces ? "workspacesPlacement" : "savedSearchesPlacement"].str();
    f::text(p, L"Where to offer them", tok::TextSecondary, 14.0f, 600);
    auto* rg = f::radios(p, {L"In the sidebar", L"In the title bar, beside the app name", L"Nowhere"},
                         cur == "sidebar" ? 0 : cur == "titlebar" || cur == "title-bar" || cur == "titleBar" ? 1 : 2, [this, key](int i) {
                             const char* v[] = {"\"sidebar\"", "\"titleBar\"", "\"hidden\""};
                             const char* alt[] = {"\"Sidebar\"", "\"TitleBar\"", "\"Hidden\""};
                             try {
                                 core::check(bb_settings_set(core_.app(), core::s(core::wide(key)), core::s(core::wide(v[i]))), key);
                             } catch (const core::CoreError&) {
                                 set_raw(key, alt[i]);
                             }
                         });
    rg->name = workspaces ? "WorkspacesPlacement" : "SavedSearchesPlacement";
    f::gap(p, 6);
    f::text(p, workspaces ? L"Your workspaces" : L"Your saved searches", tok::TextSecondary, 14.0f, 600);
    const json::Value& items = st[workspaces ? "workspaces" : "searches"];
    auto* list = p.emplace<f::ItemList>();
    list->name = workspaces ? "WorkspaceList" : "SavedSearchList";
    list->height = 220;
    std::vector<f::ListEntry> entries;
    for (size_t i = 0; i < items.size(); ++i) {
        f::ListEntry e;
        e.text = core::wide(items[i]["name"].str());
        e.detail = core::wide(workspaces ? items[i]["shape"].str() : items[i]["query"].str());
        entries.push_back(std::move(e));
    }
    if (entries.empty()) {
        f::ListEntry e;
        e.text = workspaces ? L"No saved workspaces yet" : L"No saved searches yet";
        e.muted = true;
        entries.push_back(std::move(e));
    }
    list->set_entries(std::move(entries));
}

// ---- the Menus and toolbars page ------------------------------------------------------------------------------------

void SettingsView::page_bars(Widget& p) {
    BbApp* app = core_.app();
    auto bars_do = [app](const std::wstring& verb, const std::wstring& arg = L"") {
        try {
            core::check(bb_barpage_do(app, core::s(verb), core::s(arg)), "bars");
        } catch (const core::CoreError&) {
        }
    };
    const json::Value m = try_model([&](BbOwnedStr* o) { return bb_bars_query(app, core::s(L"page"), core::s(L""), o); });
    bars_model_ = m;
    f::heading(p, L"Menus and toolbars");
    auto* top = f::row(p);
    auto* status = f::text(*top, core::wide(m["status"].str()), tok::TextMuted, 13.0f);
    status->flex = 1;
    f::button(*top, L"Save as…", [] {});
    f::button(*top, L"Reset to default", [bars_do] { bars_do(L"reset"); });

    auto* cols = f::row(p, 14);
    cols->height = 520;

    // -- which bar, and the saved arrangements
    auto* left = cols->emplace<StackPanel>(Orientation::Vertical, 6.0f);
    left->width = 200;
    f::text(*left, L"Edit", tok::TextSecondary, 13.0f, 600);
    auto* targets = left->emplace<f::ItemList>();
    targets->name = "BarTargets";
    targets->height = 30.0f * static_cast<float>(std::max<size_t>(2, m["targets"].size())) + 4;
    std::vector<f::ListEntry> te;
    int tsel = 0;
    std::vector<std::wstring> tkeys;
    for (size_t i = 0; i < m["targets"].size(); ++i) {
        f::ListEntry e;
        e.text = core::wide(m["targets"][i]["name"].str());
        e.icon = icon_for(m["targets"][i]["icon"].str());
        te.push_back(std::move(e));
        tkeys.push_back(core::wide(m["targets"][i]["key"].str()));
        if (m["targets"][i]["key"].str() == m["target"]["key"].str()) tsel = static_cast<int>(i);
    }
    targets->row_height = 30;
    targets->set_entries(std::move(te), tsel);
    targets->on_select = [bars_do, tkeys](int i) {
        if (i >= 0 && static_cast<size_t>(i) < tkeys.size()) bars_do(L"select", tkeys[static_cast<size_t>(i)] == L"menus" ? L"menu-bar" : tkeys[static_cast<size_t>(i)]);
    };
    f::text(*left, L"Saved arrangements", tok::TextSecondary, 13.0f, 600);
    auto* saved = left->emplace<f::ItemList>();
    saved->flex = 1;
    std::vector<f::ListEntry> se;
    std::vector<std::wstring> snames;
    int ssel = -1;
    for (size_t i = 0; i < m["saved"].size(); ++i) {
        f::ListEntry e;
        e.text = core::wide(m["saved"][i]["name"].str());
        e.detail = core::wide(m["saved"][i]["shape"].str());
        se.push_back(std::move(e));
        snames.push_back(core::wide(m["saved"][i]["name"].str()));
        if (m["saved"][i]["name"].str() == m["selectedSaved"].str()) ssel = static_cast<int>(i);
    }
    if (se.empty()) {
        f::ListEntry e;
        e.text = L"Nothing saved yet";
        e.muted = true;
        se.push_back(std::move(e));
    }
    saved->set_entries(std::move(se), ssel);
    saved->on_select = [bars_do, snames](int i) {
        if (i >= 0 && static_cast<size_t>(i) < snames.size()) bars_do(L"select-saved", snames[static_cast<size_t>(i)]);
    };
    auto* sr = f::row(*left, 6);
    f::button(*sr, L"Use", [bars_do, snames, ssel] { if (ssel >= 0) bars_do(L"apply-saved", snames[static_cast<size_t>(ssel)]); });
    f::button(*sr, L"Remove", [bars_do, snames, ssel] { if (ssel >= 0) bars_do(L"remove-saved", snames[static_cast<size_t>(ssel)]); });

    // -- what is on the bar
    auto* mid = cols->emplace<StackPanel>(Orientation::Vertical, 6.0f);
    mid->flex = 1;
    auto* loc = f::row(*mid);
    if (m["canUp"].boolean()) f::button(*loc, L"←", [bars_do] { bars_do(L"up"); });
    f::text(*loc, core::wide(m["location"].str()), tok::TextPrimary, 14.0f, 600);
    auto* rows = mid->emplace<f::ItemList>();
    rows->name = "BarRows";
    rows->flex = 1;
    std::vector<f::ListEntry> re;
    int rsel = -1;
    for (size_t i = 0; i < m["rows"].size(); ++i) {
        const json::Value& r = m["rows"][i];
        f::ListEntry e;
        e.separator = r["separator"].boolean();
        e.text = e.separator ? L"" : core::wide(r["name"].str());
        e.detail = core::wide(r["detail"].str());
        e.icon = icon_for(r["icon"].str());
        re.push_back(std::move(e));
        if (r["selected"].boolean()) rsel = static_cast<int>(i);
    }
    rows->set_entries(std::move(re), rsel);
    rows->on_select = [bars_do](int i) { bars_do(L"pick", std::to_wstring(i)); };
    rows->on_activate = [bars_do](int i) { bars_do(L"open", std::to_wstring(i)); };
    auto* rb = f::row(*mid, 6);
    f::button(*rb, L"Open", [bars_do] { bars_do(L"open"); });
    f::button(*rb, L"↑", [bars_do] { bars_do(L"nudge", L"up"); });
    f::button(*rb, L"↓", [bars_do] { bars_do(L"nudge", L"down"); });
    f::button(*rb, L"Remove", [bars_do] { bars_do(L"remove"); });
    f::button(*rb, L"Separator", [bars_do] { bars_do(L"add-separator"); });
    auto* addm = f::button(*rb, core::wide(m["addSubmenuLabel"].is_null() ? "Menu" : m["addSubmenuLabel"].str()), [bars_do] { bars_do(L"add-menu"); });
    addm->set_enabled(m["canAdd"].boolean() || true);
    if (m["canEditCaption"].boolean()) {
        auto* cr = f::row(*mid, 6);
        f::row_label(*cr, L"Name", 60);
        auto* ce = f::field(*cr, core::wide(m["caption"].str()), 240, [bars_do](const std::wstring& t) { bars_do(L"name", t); });
        ce->automation_id = "BarCaption";
        f::note(*mid, core::wide(m["captionHint"].str()));
    }
    if (m["isToolbar"].boolean()) {
        f::check(*mid, L"Show names beside the icons on this toolbar", m["showsText"].boolean(),
                 [bars_do](bool v) { bars_do(L"show-text", v ? L"on" : L"off"); });
    }

    // -- the picker
    auto* right = cols->emplace<StackPanel>(Orientation::Vertical, 6.0f);
    right->width = 250;
    f::text(*right, L"Add", tok::TextSecondary, 13.0f, 600);
    auto* filter = f::field(*right, core::wide(m["filter"].str()), -1, [bars_do](const std::wstring& t) { bars_do(L"filter", t); }, L"Find a command");
    filter->automation_id = "BarFilter";
    filter->width = -1;
    f::check(*right, L"Only what is not placed anywhere", m["unplacedOnly"].boolean(),
             [bars_do](bool v) { bars_do(L"unplaced", v ? L"on" : L"off"); });
    auto* choices = right->emplace<f::ItemList>();
    choices->name = "BarChoices";
    choices->flex = 1;
    std::vector<f::ListEntry> ce;
    std::vector<std::wstring> cids;
    for (size_t i = 0; i < m["choices"].size(); ++i) {
        const json::Value& c = m["choices"][i];
        f::ListEntry e;
        e.heading = c["heading"].boolean();
        e.text = core::wide(c["name"].str());
        e.detail = core::wide(c["placedText"].str());
        e.icon = icon_for(c["icon"].str());
        ce.push_back(std::move(e));
        cids.push_back(core::wide(c["id"].str().empty() ? c["name"].str() : c["id"].str()));
    }
    choices->set_entries(std::move(ce));
    choices->on_activate = [bars_do, cids](int i) { if (i >= 0) bars_do(L"add", cids[static_cast<size_t>(i)]); };
    auto* add = f::button(*right, L"Add", [bars_do, choices, cids] {
        if (choices->selected >= 0) bars_do(L"add", cids[static_cast<size_t>(choices->selected)]);
    });
    (void)add;
}

std::vector<std::wstring> SettingsView::bar_rows() const {
    std::vector<std::wstring> out;
    const json::Value m = try_model([&](BbOwnedStr* o) { return bb_bars_query(core_.app(), core::s(L"page"), core::s(L""), o); });
    for (size_t i = 0; i < m["rows"].size(); ++i) {
        out.push_back(m["rows"][i]["separator"].boolean() ? L"-" : core::wide(m["rows"][i]["name"].str()));
    }
    return out;
}

std::vector<std::wstring> SettingsView::bar_choices() const {
    std::vector<std::wstring> out;
    const json::Value m = try_model([&](BbOwnedStr* o) { return bb_bars_query(core_.app(), core::s(L"page"), core::s(L""), o); });
    for (size_t i = 0; i < m["choices"].size(); ++i) {
        if (!m["choices"][i]["heading"].boolean()) out.push_back(core::wide(m["choices"][i]["name"].str()));
    }
    return out;
}

// ---- the Keyboard page ---------------------------------------------------------------------------------------------------

void SettingsView::page_keys(Widget& p) {
    BbApp* app = core_.app();
    auto keys_do = [app](const std::wstring& verb, const std::wstring& arg = L"") {
        try {
            core::check(bb_keys_do(app, core::s(verb), core::s(arg)), "keys");
        } catch (const core::CoreError&) {
        }
    };
    const json::Value m = try_model([&](BbOwnedStr* o) { return bb_keys_page(app, o); });
    keys_model_ = m;
    f::heading(p, L"Keyboard");
    f::note(p, L"Every command and the shortcuts that run it. Pick one, then Change to press a new shortcut for it.");
    auto* top = f::row(p);
    auto* filter = f::field(*top, core::wide(m["filter"].str()), 320, [keys_do](const std::wstring& t) { keys_do(L"filter", t); }, L"Filter by name, category or shortcut");
    filter->automation_id = "KeyFilter";
    f::button(*top, L"Find by key…", [keys_do] { keys_do(L"find"); });
    f::button(*top, L"Reset all", [keys_do] { keys_do(L"reset-all"); });

    auto* list = p.emplace<f::ItemList>();
    list->name = "KeyRows";
    list->height = 340;
    std::vector<f::ListEntry> entries;
    std::vector<std::wstring> ids;
    int sel = -1;
    for (size_t i = 0; i < m["rows"].size(); ++i) {
        const json::Value& r = m["rows"][i];
        f::ListEntry e;
        e.heading = r["heading"].boolean();
        e.text = core::wide(r["name"].str());
        e.icon = e.heading ? nullptr : icon_for(r["icon"].str());
        const std::wstring chords = join_chords(r["chords"]);
        e.detail = e.heading ? L"" : chords.empty() ? L"none" : chords;
        e.muted = !e.heading && chords.empty();
        entries.push_back(std::move(e));
        ids.push_back(core::wide(r["id"].str()));
        if (!r["heading"].boolean() && r["id"].str() == m["selected"].str()) sel = static_cast<int>(i);
    }
    list->set_entries(std::move(entries), sel);
    list->on_select = [keys_do, ids](int i) {
        if (i >= 0 && static_cast<size_t>(i) < ids.size() && !ids[static_cast<size_t>(i)].empty()) keys_do(L"select", ids[static_cast<size_t>(i)]);
    };
    // The recorder: one line saying what is being waited for, or what was refused.
    auto* panel = p.emplace<StackPanel>(Orientation::Vertical, 6.0f);
    panel->name = "KeyRecorderPanel";
    const bool recording = m["recording"].boolean() || m["capturing"].boolean();
    const std::wstring line = core::wide(m["text"].str());
    if (!line.empty()) {
        auto* t = f::text(*panel, line, recording ? tok::AccentBackground : tok::TextPrimary, 14.0f, 600);
        t->name = "KeyRecorderText";
        t->wrap = true;
    }
    auto* br = f::row(*panel, 8);
    if (m["canReassign"].boolean()) f::button(*br, L"Reassign", [keys_do] { keys_do(L"reassign"); });
    if (recording || m["canReassign"].boolean()) {
        f::button(*br, L"Cancel", [keys_do] { keys_do(L"cancel"); });
    } else {
        const bool have = !m["selected"].str().empty();
        f::button(*br, L"Change…", [keys_do] { keys_do(L"change"); })->set_enabled(have);
        f::button(*br, L"Add another…", [keys_do] { keys_do(L"add"); })->set_enabled(have);
        f::button(*br, L"Remove", [keys_do] { keys_do(L"remove"); })->set_enabled(have);
        f::button(*br, L"Reset", [keys_do] { keys_do(L"reset"); })->set_enabled(have);
    }
}

std::wstring SettingsView::recorder_text() const {
    return core::wide(try_model([&](BbOwnedStr* o) { return bb_keys_page(core_.app(), o); })["text"].str());
}

bool SettingsView::key_row(const std::wstring& label, std::vector<std::wstring>* chords) const {
    const json::Value m = try_model([&](BbOwnedStr* o) { return bb_keys_page(core_.app(), o); });
    for (size_t i = 0; i < m["rows"].size(); ++i) {
        const json::Value& r = m["rows"][i];
        if (r["heading"].boolean() || !core::iequals(core::wide(r["name"].str()), label)) continue;
        chords->clear();
        for (size_t k = 0; k < r["chords"].size(); ++k) chords->push_back(core::wide(r["chords"][k].str()));
        return true;
    }
    return false;
}

}  // namespace bb::views
