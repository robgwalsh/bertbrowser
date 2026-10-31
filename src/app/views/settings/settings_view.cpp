#include "views/settings/settings_view.h"
#include "views/dialogs/file_pickers.h"

#include "gfx/icons.gen.h"
#include "views/main/main_view.h"
#include "views/dialogs/ops_card.h"
#include "views/tools/tool_widgets.h"

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

// A row that turns the wheel over it into steps: the Columns page's width ("Scroll a width to change it"), Ctrl
// for one DIP at a time. Spec: SettingsView.xaml.cs (ColumnWidth_Wheel).
class WheelRow : public StackPanel {
public:
    WheelRow() : StackPanel(Orientation::Horizontal, 8.0f) {}
    std::function<void(int notches, bool fine)> on_step;
    bool on_wheel(Point, int delta, Mods m) override {
        if (!on_step || delta == 0) return false;
        on_step(delta > 0 ? std::max(1, delta / 120) : std::min(-1, delta / 120), m.ctrl);
        return true;
    }
};

}  // namespace

// The "leave anyway?" card, over the whole page: a scrim that takes every click and one card centred on it. It
// is in-tree, not the dialog layer's, because the page it asks about is itself a layer of the main window.
class LeaveLayer : public Widget {
public:
    LeaveLayer() {
        style.bg = tok::Scrim;
        flags.visible = false;
    }
    void show_card(std::unique_ptr<Widget> card) {
        hide_card();
        card_ = add(std::move(card));
        set_visible(true);
        request_layout();
        invalidate();
    }
    void hide_card() {
        if (!card_) return;
        remove_child(card_);
        card_ = nullptr;
        set_visible(false);
        invalidate();
    }
    bool has_card() const { return card_ != nullptr; }
    Size measure(Size avail) override { return avail; }
    void arrange(Rect r) override {
        bounds = r;
        if (!card_) return;
        const Size d = card_->measure_tree({card_->width >= 0 ? card_->width : r.width() - 80, r.height() - 40});
        const float w = card_->width >= 0 ? card_->width : d.w;
        card_->arrange(Rect::xywh(r.l + (r.width() - w) / 2, r.t + std::max(20.0f, (r.height() - d.h) / 3), w, d.h));
    }
    bool on_mouse_down(const MouseEvent&) override { return true; }
    bool on_wheel(Point, int, Mods) override { return true; }

private:
    Widget* card_ = nullptr;
};

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

    // The title, and the way back to the folders on the right, where the Settings button that opened the page
    // sits. Spec: SettingsView.xaml (the title strip).
    auto* strip = card_->emplace<DockPanel>();
    strip->name = "SettingsTitle";
    strip->dock = Dock::Top;
    strip->height = 44;
    strip->pad = 8;
    strip->style.bg = tok::SurfaceBackground;
    auto* close = strip->emplace<Button>();
    close->name = "SettingsClose";
    close->dock = Dock::Right;
    close->icon = &icons::Dismiss;
    close->width = 32;
    close->style.bg = tok::SurfaceBackground;  // flat, as a toolbar button, until the pointer is on it
    close->style.border = tok::SurfaceBackground;
    close->tooltip = L"Close settings (Esc)";
    close->on_click = [this] { go_back(); };
    auto* title = strip->emplace<TextBlock>(L"Settings");
    title->style.font.size = 16.0f;
    title->style.font.weight = 600;
    title->vcenter = true;
    title->wrap = true;
    auto* rule = card_->emplace<Widget>();
    rule->dock = Dock::Top;
    rule->height = 1;
    rule->style.bg = tok::BorderSubtle;

    auto* nav = card_->emplace<StackPanel>(Orientation::Vertical, 6.0f);
    nav->dock = Dock::Left;
    nav->width = 220;
    nav->pad = 10;
    nav->style.bg = tok::SurfaceRaised;
    nav_ = nav;

    // Stays up for as long as the problem does: a list with a missing field is held unwritten until it is
    // fixed, and only leaving the page takes the person to it. Spec: SettingsView.xaml (the problem strip).
    problem_strip_ = card_->emplace<StackPanel>(Orientation::Horizontal, 8.0f);
    problem_strip_->name = "SettingsProblem";
    problem_strip_->dock = Dock::Top;
    problem_strip_->pad = 10;
    problem_strip_->style.bg = tok::ErrorBackground;
    problem_ = problem_strip_->emplace<TextBlock>();
    problem_->style.fg = tok::ErrorForeground;
    problem_->wrap = true;
    problem_->flex = 1;
    problem_strip_->set_visible(false);

    scroll_ = card_->emplace<ScrollViewer>();
    scroll_->dock = Dock::Fill;
    scroll_->name = "SettingsScroll";
    body_ = scroll_->emplace<StackPanel>(Orientation::Vertical, 10.0f);
    body_->pad = 24;
    body_->name = "SettingsBody";

    auto leave = std::make_unique<LeaveLayer>();
    leave_ = leave.get();
    add(std::move(leave));
}

void SettingsView::arrange(Rect r) {
    bounds = r;
    const float w = std::min(1280.0f, std::max(320.0f, r.width() - 48));
    const float h = std::max(200.0f, r.height() - 48);
    const float x = r.l + (r.width() - w) / 2, y = r.t + (r.height() - h) / 2;
    card_->measure_tree({w, h});
    card_->arrange(Rect::xywh(x, y, w, h));
    leave_->measure_tree({r.width(), r.height()});
    leave_->arrange(r);
}

void SettingsView::paint(PaintContext& pc) {
    Widget::paint(pc);
}

bool SettingsView::on_mouse_down(const MouseEvent& e) {
    // A click on the dimmed folders means "back to them", as Esc does. Spec: MainWindow.SettingsScrim_MouseDown.
    if (!card_->bounds.contains(e.pt)) go_back();
    return true;
}

// Back to the folders, unless something on the page could not be saved and the person would rather stay and
// finish it: the core refuses and has already brought the page that holds it to the front. A scripted run
// reports the refusal in the strip instead of putting a card up. Spec: MainWindow.CloseSettings.
void SettingsView::go_back(std::function<void()> then) {
    if (leave_->has_card()) return;
    try {
        core::check(bb_settings_page_close(core_.app()), "close settings");
        if (then) then();
    } catch (const core::CoreError& e) {
        const std::wstring why = core::message_of(e);
        // The core has brought the offending page up; if it is this one, show the entry on it now.
        reveal_problem_ = true;
        signature_.clear();
        rebuild_if_changed(false);
        if (!main_.shows_dialogs()) {
            refused_ = why;
            show_problem(L"");
            return;
        }
        ask_leave(why, std::move(then));
    }
}

void SettingsView::ask_leave(const std::wstring& why, std::function<void()> then) {
    ops::Card c = ops::make_card(L"Settings", 460);
    auto* head = f::row(*c.body, 12);
    auto* glyph = head->emplace<Icon>(icons::find("Warning"), 22.0f);
    glyph->style.fg = tok::WarningForeground;
    auto* words = f::column(*head, 8);
    words->flex = 1;
    f::text(*words, why, tok::TextPrimary, 13.5f)->wrap = true;
    f::text(*words, L"Leave anyway? That entry and the rest of its list won\u2019t be saved.", tok::TextPrimary, 13.5f)->wrap = true;
    // Cancel is the default, so Enter stays and finishes the entry. The handlers are deferred: they free the card
    // the button is on.
    Button* cancel = ops::add_button(c, L"Cancel", [this] { main_.defer([this] { leave_->hide_card(); }); });
    cancel->name = "SettingsLeaveCancel";
    cancel->is_default = true;
    ops::add_button(c, L"Leave", [this, then = std::move(then)] {
        main_.defer([this, then] {
            leave_->hide_card();
            leave_unsaved(then);
        });
    }, true)->name = "SettingsLeaveConfirm";
    leave_->show_card(std::move(c.root));
    if (root()) root()->set_focus(cancel);
}

// The lists the core is holding are dropped by writing back the ones it still has in force (a valid list
// clears its hold), and the page's own copies of them are forgotten so they are read again.
void SettingsView::leave_unsaved(const std::function<void()>& then) {
    for (const char* key : {"newFileTypes", "customCommands"}) {
        BbOwnedStr o{};
        if (bb_settings_get(core_.app(), core::s(core::wide(key)), &o) == BB_OK) set_raw(key, core::utf8(core::take(o)));
    }
    new_types_loaded_ = false;
    pending_commands_ = json::Value();
    reveal_problem_ = false;
    command_editor_ = false;
    command_editing_.clear();
    refused_.clear();
    try {
        core::check(bb_settings_page_close(core_.app()), "close settings");
        if (then) then();
    } catch (const core::CoreError& e) {
        refused_ = core::message_of(e);
        show_problem(L"");
    }
}

void SettingsView::customise_colours() {
    main_.defer([this] { go_back([this] { main_.open_dialog(L"theme-editor"); }); });
}

// Esc and the Keyboard page's recorder, taken before the focused control sees the key (the main view asks
// first). While the page listens the next key is the answer and nothing else sees it; an offer to reassign
// is declined by Esc rather than leaving the page with the question open; otherwise a plain Esc goes back.
// Spec: SettingsView.xaml.cs (View_KeyDown, OnPreviewKeyDown).
bool SettingsView::preview_key(const KeyEvent& e) {
    if (!open_) return false;
    // While the "leave anyway?" card is up, Esc is its Cancel and everything else is the card's.
    if (leave_->has_card()) {
        if (e.vk != VK_ESCAPE || e.mods.ctrl || e.mods.alt || e.mods.shift) return false;
        main_.defer([this] { leave_->hide_card(); });
        return true;
    }
    const bool modifier = e.vk == VK_SHIFT || e.vk == VK_CONTROL || e.vk == VK_MENU || e.vk == VK_LWIN || e.vk == VK_RWIN;
    const bool bare = !e.mods.ctrl && !e.mods.alt && !e.mods.shift;
    if (nudge_ && e.mods.alt && !e.mods.ctrl && !e.mods.shift && (e.vk == VK_UP || e.vk == VK_DOWN) && root() && root()->focused() == nudge_for_) {
        // A copy: moving a row rebuilds the page, which drops the function that is running.
        const auto nudge = nudge_;
        nudge(e.vk == VK_UP ? -1 : 1);
        return true;
    }
    if (page_ == "keyboard") {
        const json::Value m = try_model([&](BbOwnedStr* o) { return bb_keys_page(core_.app(), o); });
        auto keys_do = [&](const std::wstring& verb, const std::wstring& arg = L"") {
            try {
                core::check(bb_keys_do(core_.app(), core::s(verb), core::s(arg)), "keys");
            } catch (const core::CoreError&) {
            }
        };
        if (m["canReassign"].boolean() && e.vk == VK_ESCAPE && bare) {
            keys_do(L"cancel");
            return true;
        }
        if (m["capturing"].boolean()) {
            // Alt+F4 still closes the window: it can never be a shortcut.
            if (e.vk == VK_F4 && e.mods.alt && !e.mods.ctrl && !e.mods.shift) return false;
            if (e.vk == VK_ESCAPE && bare) {
                keys_do(L"cancel");
                return true;
            }
            if (modifier) return true;  // a modifier on its way down is not a chord yet
            const std::wstring key = chord_key_name(e.vk);
            if (key.empty()) return true;
            keys_do(L"press", std::wstring(e.mods.ctrl ? L"Ctrl+" : L"") + (e.mods.alt ? L"Alt+" : L"") + (e.mods.shift ? L"Shift+" : L"") + key);
            return true;
        }
    }
    if (e.vk == VK_ESCAPE && bare) {
        go_back();
        return true;
    }
    return false;
}

// ---- reading the core --------------------------------------------------------------------------------------

json::Value SettingsView::get(const char* key) {
    return try_model([&](BbOwnedStr* o) { return bb_settings_get(core_.app(), core::s(core::wide(key)), o); });
}

void SettingsView::set_raw(const char* key, const std::string& json_value) {
    try {
        core::check(bb_settings_set(core_.app(), core::s(core::wide(key)), core::s(core::wide(json_value))), key);
        refused_.clear();
    } catch (const core::CoreError& e) {
        refused_ = core::wide(e.what());
    }
    const json::Value st = try_model([&](BbOwnedStr* o) { return bb_settings_page_state(core_.app(), o); });
    show_problem(core::wide(st["problem"].str()));
}

void SettingsView::show_problem(const std::wstring& core_problem) {
    const std::wstring text = core_problem.empty() ? refused_ : core_problem;
    if (problem_->text() != text) problem_->set_text(text);
    if (problem_strip_->flags.visible != !text.empty()) {
        problem_strip_->set_visible(!text.empty());
        card_->request_layout();
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

ComboBox* SettingsView::choice_row(Widget& p, const wchar_t* label, const char* key, std::vector<std::pair<std::wstring, std::string>> choices,
                                   const char* when_unset) {
    auto* row = f::row(p);
    f::row_label(*row, label, 320);
    const json::Value v = get(key);
    // The stored value, as JSON text the choices can be compared with; unset is the default it stands for.
    std::string now = v.is_null() && when_unset ? when_unset : "";
    if (v.type == json::Value::Type::String) now = "\"" + v.str() + "\"";
    else if (v.type == json::Value::Type::Number) {
        char buf[32];
        snprintf(buf, sizeof(buf), "%g", v.num());
        now = buf;
    }
    int at = -1;
    for (size_t i = 0; i < choices.size(); ++i) {
        if (choices[i].second == now) at = static_cast<int>(i);
    }
    if (at < 0 && !now.empty()) {
        std::wstring shown = core::wide(now);
        if (shown.size() > 1 && shown.front() == L'"') shown = shown.substr(1, shown.size() - 2);
        choices.push_back({shown + L" (current)", now});
        at = static_cast<int>(choices.size()) - 1;
    }
    std::vector<std::wstring> labels;
    for (const auto& c : choices) labels.push_back(c.first);
    auto* box = f::combo(*row, labels, at, 200, [this, key, choices](int i) {
        if (i >= 0 && static_cast<size_t>(i) < choices.size()) set_raw(key, choices[static_cast<size_t>(i)].second);
    });
    box->automation_id = key;
    return box;
}

std::wstring SettingsView::gesture_or(const wchar_t* command, const wchar_t* fallback) {
    BbOwnedStr o{};
    std::wstring g;
    if (bb_keymap_gesture(core_.app(), core::s(command), &o) == BB_OK) g = core::take(o);
    return g.empty() ? std::wstring(fallback) : g;
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
        leave_->hide_card();
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
        // The New items page keeps its half-typed list across pages while the core is holding it unwritten, as the
        // original's view model did; otherwise it reads the saved list again.
        if (!was || st["problem"].str().empty()) new_types_loaded_ = false;
        refused_.clear();
        for (size_t i = 0; i < nav_->children().size(); ++i) {
            if (auto* list = dynamic_cast<f::ItemList*>(nav_->children()[i].get())) {
                for (size_t k = 0; k < pages_.size(); ++k) {
                    if (pages_[k]["id"].str() == page_) list->select(static_cast<int>(k));
                }
            }
        }
        signature_.clear();
    }
    show_problem(core::wide(st["problem"].str()));
    rebuild_if_changed(false);
    request_layout();
    invalidate();
}

void SettingsView::build_nav(const json::Value& pages) {
    while (!nav_->children().empty()) nav_->remove_child(nav_->children().front().get());
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
        sig += text_of([&](BbOwnedStr* o) { return bb_columns_defaults(app, o); });
    } else if (page_ == "search-index") {
        sig += text_of([&](BbOwnedStr* o) { return bb_index_state(app, o); });
    } else if (page_ == "new-items") {
        const json::Value st = try_model([&](BbOwnedStr* o) { return bb_new_types_state(app, o); });
        sig += (st["importing"].boolean() ? "importing|" : "idle|") + st["status"].str();
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
    nudge_ = nullptr;
    nudge_for_ = nullptr;
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
    bool_row(p, L"Show hidden items", "showHiddenItems");
    f::note(p, L"Show hidden files and folders in listings and search results");
    bool_row(p, L"Open archives as folders", "enterArchivesOnDoubleClick");
    f::note(p, L"Double-clicking a zip, 7z, tar or rar walks into it. With this off they open in whichever program owns them \u2014 and the breadcrumb, Enter and typing the path still go in.");
    f::check(p, L"Middle-click a drive/device to open a new panel", get("drivesOpenTarget").str() == "NewPanel",
             [this](bool v) { set_raw("drivesOpenTarget", v ? "\"NewPanel\"" : "\"NewTab\""); });
    f::note(p, L"Middle-clicking a drive or device in the DRIVES & DEVICES sidebar opens it in a new panel instead of a new tab. A plain click always opens a new tab.");
    const bool restore = get("restoreLastSession").boolean();
    f::check(p, L"Restore last session on startup", restore, [this](bool v) {
        set_bool("restoreLastSession", v);
        signature_.clear();  // the folder box below follows it
    });
    f::note(p, L"When off, the app always opens a single tab at the folder below instead of wherever the panes were left.");
    auto* row = f::row(p);
    const json::Value path = get("startupDefaultPath");
    // Trimmed as it is kept: a path with a stray space on the end is not the folder it looks like.
    auto trimmed = [](std::wstring t) {
        while (!t.empty() && iswspace(t.back())) t.pop_back();
        while (!t.empty() && iswspace(t.front())) t.erase(t.begin());
        return t;
    };
    auto* e = f::field(*row, core::wide(path.str()), 380, [this, trimmed](const std::wstring& t) { set_text("startupDefaultPath", trimmed(t), true); },
                       L"The folder BertBrowser opens in");
    e->automation_id = "startupDefaultPath";
    e->set_enabled(!restore);
    auto* browse = f::button(*row, L"Browse\u2026", [this, e, trimmed] {
        if (!main_.shows_dialogs()) return;
        PickPath pick;
        pick.title = L"Choose a startup folder";
        pick.folder = e->text();
        if (const auto chosen = pick_folder(main_.window().hwnd(), pick)) {
            e->set_text(*chosen);
            set_text("startupDefaultPath", trimmed(*chosen), true);
        }
    });
    browse->set_enabled(!restore);
    choice_row(p, L"Vertical scroll speed", "scrollSpeedMultiplier",
               {{L"1\u00d7 (normal)", "1"}, {L"1.5\u00d7", "1.5"}, {L"2\u00d7", "2"}, {L"2.5\u00d7", "2.5"}, {L"3\u00d7", "3"}, {L"3.5\u00d7", "3.5"},
                {L"4\u00d7", "4"}, {L"4.5\u00d7", "4.5"}, {L"5\u00d7", "5"}, {L"5.5\u00d7", "5.5"}, {L"6\u00d7", "6"}});
    choice_row(p, L"Tile aspect ratio", "tileAspectRatio",
               {{L"1:1", "\"1:1\""}, {L"4:3", "\"4:3\""}, {L"3:2", "\"3:2\""}, {L"16:10", "\"16:10\""}, {L"16:9", "\"16:9\""}, {L"3:4", "\"3:4\""},
                {L"2:3", "\"2:3\""}});
    f::gap(p, 6);
    // Who opens folders is Windows' setting, so it is read from Windows each time rather than kept here, and a
    // scripted run never touches the machine's registry. Spec: SettingsViewModel.OnOpenFoldersHereChanged.
    if (main_.scripted()) {
        f::check(p, L"Open folders in BertBrowser instead of File Explorer", false, nullptr)->set_enabled(false);
        f::check(p, L"Win+E opens BertBrowser", false, nullptr)->set_enabled(false);
        f::note(p, L"(Not read in a scripted run: it is the machine's registry.)");
        return;
    }
    // A registered program's bare name, for naming it rather than quietly taking its place.
    const auto program_name = [](const std::string& path) {
        std::wstring name = core::wide(path);
        const size_t slash = name.find_last_of(L"\\/");
        if (slash != std::wstring::npos) name = name.substr(slash + 1);
        const size_t dot = name.rfind(L'.');
        if (dot != std::wstring::npos && dot > 0) name = name.substr(0, dot);
        return name.empty() ? std::wstring(L"Another program") : name;
    };
    BbOwnedStr o{};
    json::Value handler;
    if (bb_folder_handler_state(&o) == BB_OK) handler = json::parse(core::utf8(core::take(o)));
    const std::string state = handler["state"].str();
    const bool ours = state == "RegisteredToThisApp" || state == "RegisteredToThisAppStale";
    f::check(p, L"Open folders in BertBrowser instead of File Explorer", ours, [this](bool v) {
        folder_handler_warning_ = bb_folder_handler_set(v) == BB_OK
                                      ? L""
                                      : v ? L"Windows would not let BertBrowser register as the folder handler."
                                          : L"Windows would not let BertBrowser hand folders back to File Explorer.";
        signature_.clear();
        sync();
    })->name = "OpenFoldersHere";
    f::note(p, L"This PC and Control Panel keep opening in File Explorer, and so does a zip opened from Explorer \u2014 BertBrowser browses archives in its own window. So does anything that runs explorer.exe by name \u2014 the taskbar icon, and some apps' \"Show in folder\".");
    std::wstring warning = folder_handler_warning_;
    if (warning.empty() && state == "RegisteredToAnotherApp") {
        warning = program_name(handler["other"].str()) + L" currently opens folders. Turning this on will replace it.";
    }
    if (!warning.empty()) f::text(p, warning, tok::WarningForeground, 13.0f)->wrap = true;

    f::gap(p, 6);
    BbOwnedStr win_e_out{};
    json::Value win_e;
    if (bb_win_e_handler_state(&win_e_out) == BB_OK) win_e = json::parse(core::utf8(core::take(win_e_out)));
    const std::string win_e_state = win_e["state"].str();
    const bool win_e_ours = win_e_state == "RegisteredToThisApp" || win_e_state == "RegisteredToThisAppStale";
    f::check(p, L"Win+E opens BertBrowser", win_e_ours, [this](bool v) {
        win_e_handler_warning_ = bb_win_e_handler_set(v) == BB_OK
                                     ? L""
                                     : v ? L"Windows would not let BertBrowser take over Win+E."
                                         : L"Windows would not let BertBrowser hand Win+E back to File Explorer.";
        signature_.clear();
        sync();
    })->name = "OpenWinEHere";
    f::note(p, L"So does running explorer.exe with no folder; naming a folder still opens File Explorer. If BertBrowser is already open, its window comes forward. Windows has no setting for this, so it relies on an undocumented registry key that an update could stop honouring.");
    std::wstring win_e_warning = win_e_handler_warning_;
    if (win_e_warning.empty() && win_e_state == "RegisteredToAnotherApp") {
        win_e_warning = program_name(win_e["other"].str()) + L" currently opens on Win+E. Turning this on will replace it.";
    }
    if (!win_e_warning.empty()) f::text(p, win_e_warning, tok::WarningForeground, 13.0f)->wrap = true;
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
    f::note(p, L"Switches theme when Windows does, without a restart. Turning on Windows high contrast switches to High Contrast Dark whatever is picked below, and turning it off comes back here.");
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
        // Light themes for the light slot and dark for the dark. Spec: SystemThemeRules.SlotChoices.
        auto slot_list = [&](const json::Value& allowed, const std::string& held, std::vector<std::wstring>* labels, std::vector<std::string>* keys) {
            int at = -1;
            for (size_t k = 0; k < allowed.size(); ++k) {
                for (size_t i = 0; i < available.size(); ++i) {
                    if (available[i]["id"].str() != allowed[k].str()) continue;
                    labels->push_back(names[i]);
                    keys->push_back(ids[i]);
                    if (ids[i] == held) at = static_cast<int>(keys->size()) - 1;
                }
            }
            return at;
        };
        std::vector<std::wstring> light_names, dark_names;
        std::vector<std::string> light_ids, dark_ids;
        const int light_at = slot_list(th["lightChoices"], th["lightSlot"].str(), &light_names, &light_ids);
        const int dark_at = slot_list(th["darkChoices"], th["darkSlot"].str(), &dark_names, &dark_ids);
        (void)light;
        (void)dark;
        auto* c1 = f::combo(*r1, light_names, light_at, 240, [this, light_ids](int i) {
            BbOwnedStr o{};
            if (i >= 0 && bb_theme_do(core_.app(), core::s(L"slot"), core::s(L"light " + core::wide(light_ids[static_cast<size_t>(i)])), &o) == BB_OK) bb_str_free(o);
        });
        c1->name = "LightThemeCombo";
        f::button(*r1, L"Customise colours…", [this] { customise_colours(); })->tooltip =
            L"Edit any colour in the theme currently in use and save it as your own";
        auto* r2 = f::row(p);
        f::row_label(*r2, L"Dark theme", 160);
        auto* c2 = f::combo(*r2, dark_names, dark_at, 240, [this, dark_ids](int i) {
            BbOwnedStr o{};
            if (i >= 0 && bb_theme_do(core_.app(), core::s(L"slot"), core::s(L"dark " + core::wide(dark_ids[static_cast<size_t>(i)])), &o) == BB_OK) bb_str_free(o);
        });
        c2->name = "DarkThemeCombo";
        f::button(*r2, L"Customise colours…", [this] { customise_colours(); })->tooltip =
            L"Edit any colour in the theme currently in use and save it as your own";
    } else {
        auto* r = f::row(p);
        f::row_label(*r, L"Theme", 160);
        auto* c = f::combo(*r, names, current, 240, [this, ids](int i) {
            BbOwnedStr o{};
            if (i >= 0 && bb_theme_do(core_.app(), core::s(L"select"), core::s(core::wide(ids[static_cast<size_t>(i)])), &o) == BB_OK) bb_str_free(o);
        });
        c->name = "ThemeCombo";
        f::button(*r, L"Customise colours…", [this] { customise_colours(); })->tooltip =
            L"Edit any colour in this theme and save it as your own";
    }
    (void)theme_do;
    for (size_t i = 0; i < th["issues"].size(); ++i) f::text(p, core::wide(th["issues"][i].str()), tok::WarningForeground, 13.0f)->wrap = true;
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
    f::note(p, L"Each tab keeps its own preview pane, so one pane can preview while another shows a full-width list. This is what a new tab starts with; " +
                   gesture_or(L"view.preview", L"Show preview") + L" toggles the one in front of you.");
    // Kept inside the bounds the readers were written for. Spec: SettingsViewModel.Apply (Clamp KB*1024, 4 KB..64 MB).
    const std::vector<std::pair<std::wstring, std::string>> sizes = {
        {L"64 KB", "65536"},     {L"128 KB", "131072"},   {L"256 KB", "262144"},   {L"512 KB", "524288"},
        {L"1 MB", "1048576"},    {L"2 MB", "2097152"},    {L"4 MB", "4194304"},    {L"8 MB", "8388608"}};
    choice_row(p, L"Read at most", "previewTextMaxBytes", sizes);
    f::note(p, L"Trim text after this amount.");
    choice_row(p, L"Search inside files up to", "searchContentMaxBytes", sizes);
    f::note(p, L"How much of each file content: reads. A file longer than this is searched to here and no further, and the status bar says so rather than reporting it as a miss.");
    f::note(p, L"Previews don't hold file locks!");
}

void SettingsView::page_search_index(Widget& p) {
    f::heading(p, L"Search index");
    const json::Value st = try_model([&](BbOwnedStr* o) { return bb_index_state(core_.app(), o); });
    f::note(p, L"Searching the whole PC reads the NTFS master file table, which needs administrator rights. A small helper does that "
               L"and keeps running when BertBrowser closes.");
    auto* r = f::row(p);
    const std::wstring helper = core::wide(st["helperText"].str());
    auto* t = f::text(*r, L"Background indexer: " + (helper.empty() ? core::wide(st["status"].str()) : helper));
    t->flex = 1;
    t->name = "IndexerStatus";
    auto* start = f::button(*r, L"Start", [this] {
        bb_index_do(core_.app(), core::s(L"start"));
        signature_.clear();
    });
    start->set_enabled(st["canStart"].boolean());
    start->name = "IndexerStart";
    // Only with a session to say goodbye down; one that is not answering cannot be reached from here.
    auto* stop = f::button(*r, L"Stop", [this] {
        bb_index_do(core_.app(), core::s(L"stop"));
        signature_.clear();
    });
    stop->set_enabled(st["canStop"].boolean());
    stop->name = "IndexerStop";
    if (!st["canStop"].boolean() && helper == L"Running, but not responding.") stop->tooltip = L"The helper is not answering, so it cannot be stopped from here.";
    f::gap(p, 6);
    bool_row(p, L"Start the indexer when BertBrowser launches", "startIndexerAtLaunch");
    // The scheduler is the state: read, not remembered. Each change is one elevation prompt.
    auto* logon = f::check(p, L"Start the indexer automatically when I sign in", st["autostart"].boolean(), [this](bool v) {
        bb_index_do(core_.app(), core::s(v ? L"autostart-on" : L"autostart-off"));
    });
    logon->name = "IndexerAtSignIn";
    logon->set_enabled(st["autostartCanChoose"].boolean());
    if (!st["autostartNote"].str().empty()) f::text(p, core::wide(st["autostartNote"].str()), tok::WarningForeground, 13.0f)->wrap = true;
}

void SettingsView::page_history(Widget& p) {
    f::heading(p, L"History");
    f::note(p, L"Features that remember what you did are off until you turn them on here, and each has a way to clear it.");
    f::text(p, L"What changed", tok::TextSecondary, 14.0f, 600);
    bool_row(p, L"Record file changes", "recordFileChanges");
    f::note(p, L"Keeps a log of every file created, changed, deleted or renamed on every indexed drive, so the \"What changed\" window (" +
                   gesture_or(L"tools.changes", L"What changed here") +
                   L") can show what an installer or a build just did. The log lives in BertBrowser's own database in your profile folder, readable by anything that can read your files \u2014 which is why it is off unless you turn it on. Turning it off deletes what was kept, straight away.");
    // Nothing is kept while recording is off, so there is nothing to choose how long to keep.
    choice_row(p, L"Keep changes for", "fileChangeRetentionHours", {{L"1 hour", "1"}, {L"6 hours", "6"}, {L"24 hours", "24"}, {L"7 days", "168"}})
        ->set_enabled(get("recordFileChanges").boolean());
    f::note(p, L"Older changes are dropped as new ones arrive. Whatever you pick, the log never grows past half a million rows.");
    BbOwnedStr out{};
    int64_t recorded = 0;
    if (bb_tools(core_.app(), core::s(L"changes.count"), core::s(L"{}"), &out) == BB_OK) recorded = json::parse(core::utf8(core::take(out)))["count"].i64();
    auto* r = f::row(p, 12);
    auto* clear = f::button(*r, L"Clear history now", [this] {
        // The recorded changes, gone; recording itself stays as the switch above has it.
        BbOwnedStr o{};
        if (bb_tools(core_.app(), core::s(L"changes.clear"), core::s(L"{}"), &o) == BB_OK) bb_str_free(o);
        signature_.clear();
        sync();
    });
    clear->name = "ClearChangeLog";
    clear->set_enabled(recorded > 0);
    // Thousands grouped, as the original's N0 had it.
    std::wstring digits = std::to_wstring(recorded);
    for (int at = static_cast<int>(digits.size()) - 3; at > 0; at -= 3) digits.insert(static_cast<size_t>(at), L",");
    f::text(*r, recorded == 1 ? L"1 change recorded" : digits + L" changes recorded", tok::TextMuted, 13.0f)->name = "ChangeLogCount";
    f::note(p, L"Deletes the log straight away.");
    f::gap(p, 6);
    f::text(p, L"Undo", tok::TextSecondary, 14.0f, 600);
    choice_row(p, L"Keep up to", "undoMaxSteps", {{L"10 actions", "10"}, {L"25 actions", "25"}, {L"50 actions", "50"}, {L"100 actions", "100"}, {L"200 actions", "200"}},
               "50");
    choice_row(p, L"Hold up to this much set-aside data", "undoMaxHeldGigabytes",
               {{L"1 GB", "1"}, {L"5 GB", "5"}, {L"10 GB", "10"}, {L"25 GB", "25"}, {L"50 GB", "50"}, {L"100 GB", "100"}}, "10");
    f::note(p, L"Undo history lasts for this session only; " + gesture_or(L"edit.undo-history", L"Undo history") +
                   L" shows it. To make undo possible, BertBrowser holds on to what you replace or delete into its own holding folder, and an edited archive's original. When the history goes past either limit, the oldest actions are released and what they held is removed for good. The same happens when you clear the history or close BertBrowser. The action you would undo next is never released, however large it is. Items sent to the Recycle Bin stay there either way.");
    auto* clear_undo = f::button(p, L"Clear the undo history now", [this] { main_.open_dialog(L"clear-undo"); });
    clear_undo->name = "ClearUndoHistory";
    clear_undo->width = 220;
}

void SettingsView::load_new_types() {
    const json::Value st = try_model([&](BbOwnedStr* o) { return bb_new_types_state(core_.app(), o); });
    new_types_.clear();
    for (size_t i = 0; i < st["types"].size(); ++i) {
        const json::Value& t = st["types"][i];
        new_types_.push_back({core::wide(t["label"].str()), core::wide(t["extension"].str()), core::wide(t["templatePath"].str()),
                              t["enabled"].is_null() || t["enabled"].boolean()});
    }
    if (new_type_selected_ >= static_cast<int>(new_types_.size())) new_type_selected_ = static_cast<int>(new_types_.size()) - 1;
    new_types_loaded_ = true;
}

void SettingsView::write_new_types() {
    std::string out = "[";
    for (size_t i = 0; i < new_types_.size(); ++i) {
        const NewType& t = new_types_[i];
        if (i) out += ",";
        out += "{\"label\":" + json_quote(t.label) + ",\"extension\":" + json_quote(t.extension) +
               ",\"templatePath\":" + (t.template_path.empty() ? std::string("null") : json_quote(t.template_path)) +
               ",\"enabled\":" + (t.enabled ? "true" : "false") + "}";
    }
    set_raw("newFileTypes", out + "]");
}

void SettingsView::refresh_new_type_row(int i) {
    auto* list = dynamic_cast<f::ItemList*>(new_type_list_);
    if (!list || i < 0 || i >= static_cast<int>(new_types_.size())) return;
    const NewType& t = new_types_[static_cast<size_t>(i)];
    f::ListEntry e;
    e.text = t.label.empty() ? L"(no name)" : t.label;
    e.detail = t.extension + (t.enabled ? L"" : L"  \u00b7  off");
    e.muted = !t.enabled;
    list->update_entry(i, std::move(e));
}

// The file types on the New submenu, in menu order, and an editor for the one selected; Windows' own
// list can be imported into it. Spec: SettingsView.xaml (NewItems), SettingsViewModel (NewFileTypes).
void SettingsView::page_new_items(Widget& p) {
    const json::Value st = try_model([&](BbOwnedStr* o) { return bb_new_types_state(core_.app(), o); });
    const bool importing = st["importing"].boolean();
    // An import that has finished brought new rows; anything else is the page's own copy.
    if (!new_types_loaded_ || (new_types_importing_ && !importing)) load_new_types();
    if (!importing) new_types_importing_ = false;

    f::heading(p, L"New file types");
    f::note(p, L"These appear on the New submenu of the file list and folder tree, in this order. A type with no template file makes an empty file. New \u25b8 Folder is always offered and is not listed here.");
    auto* cols = f::row(p, 12);
    auto* left = f::column(*cols, 6);
    left->width = 240;
    auto* list = left->emplace<f::ItemList>();
    list->name = "NewTypeRows";
    list->height = 300;
    new_type_list_ = list;
    std::vector<f::ListEntry> entries;
    for (size_t i = 0; i < new_types_.size(); ++i) entries.emplace_back();
    list->set_entries(std::move(entries), new_type_selected_);
    for (size_t i = 0; i < new_types_.size(); ++i) refresh_new_type_row(static_cast<int>(i));
    list->on_select = [this](int i) {
        new_type_selected_ = i;
        main_.defer([this] { build_page(); });
    };
    auto* buttons = f::row(*left, 6);
    const int sel = new_type_selected_;
    const bool has = sel >= 0 && sel < static_cast<int>(new_types_.size());
    f::button(*buttons, L"Add", [this] {
        new_types_.push_back({L"New type", L".txt", L"", true});  // Spec: AddNewFileType
        new_type_selected_ = static_cast<int>(new_types_.size()) - 1;
        write_new_types();
        main_.defer([this] { build_page(); });
    })->name = "NewTypeAdd";
    auto* remove = f::button(*buttons, L"Remove", [this] {
        if (new_type_selected_ < 0 || new_type_selected_ >= static_cast<int>(new_types_.size())) return;
        new_types_.erase(new_types_.begin() + new_type_selected_);
        new_type_selected_ = std::min(new_type_selected_, static_cast<int>(new_types_.size()) - 1);
        write_new_types();
        main_.defer([this] { build_page(); });
    });
    remove->name = "NewTypeRemove";
    remove->set_enabled(has);
    auto move = [this](int by) {
        const int to = new_type_selected_ + by;
        if (new_type_selected_ < 0 || to < 0 || to >= static_cast<int>(new_types_.size())) return;
        std::swap(new_types_[static_cast<size_t>(new_type_selected_)], new_types_[static_cast<size_t>(to)]);
        new_type_selected_ = to;
        write_new_types();
        main_.defer([this] { build_page(); });
    };
    auto* up = f::button(*buttons, L"\u2191", [move] { move(-1); }, 32);
    up->name = "NewTypeUp";
    up->tooltip = L"Move up";
    up->set_enabled(has && sel > 0);
    auto* down = f::button(*buttons, L"\u2193", [move] { move(1); }, 32);
    down->name = "NewTypeDown";
    down->tooltip = L"Move down";
    down->set_enabled(has && sel + 1 < static_cast<int>(new_types_.size()));

    // The editor for the selected type; off while nothing is selected.
    auto* editor = f::column(*cols, 8);
    editor->flex = 1;
    const NewType blank;
    const NewType& t = has ? new_types_[static_cast<size_t>(sel)] : blank;
    auto edit_row = [&](const wchar_t* label, const std::wstring& value, const char* id, std::function<void(NewType&, const std::wstring&)> apply) {
        auto* r = f::row(*editor);
        f::row_label(*r, label, 90);
        auto* e = f::field(*r, value, 300, [this, apply](const std::wstring& v) {
            if (new_type_selected_ < 0 || new_type_selected_ >= static_cast<int>(new_types_.size())) return;
            apply(new_types_[static_cast<size_t>(new_type_selected_)], v);
            refresh_new_type_row(new_type_selected_);
            write_new_types();
        });
        e->name = id;
        e->automation_id = id;
        e->set_enabled(has);
        return r;
    };
    edit_row(L"Name:", t.label, "NewTypeName", [](NewType& n, const std::wstring& v) { n.label = v; });
    edit_row(L"Extension:", t.extension, "NewTypeExtension", [](NewType& n, const std::wstring& v) { n.extension = v; });
    auto* template_row = edit_row(L"Template:", t.template_path, "NewTypeTemplate", [](NewType& n, const std::wstring& v) { n.template_path = v; });
    auto* browse = f::button(*template_row, L"\u2026", [this] {
        if (new_type_selected_ < 0 || new_type_selected_ >= static_cast<int>(new_types_.size())) return;
        PickPath pick;
        pick.title = L"Choose a template file";
        if (const auto chosen = pick_open_file(main_.window().hwnd(), pick)) {
            new_types_[static_cast<size_t>(new_type_selected_)].template_path = *chosen;
            write_new_types();
            main_.defer([this] { build_page(); });
        }
    }, 32);
    browse->tooltip = L"Browse for a template file";
    browse->set_enabled(has && !main_.scripted());
    f::note(*editor, L"Leave the template blank to create an empty file.");
    auto* shown = f::check(*editor, L"Show this type on the New menu", t.enabled, [this](bool v) {
        if (new_type_selected_ < 0 || new_type_selected_ >= static_cast<int>(new_types_.size())) return;
        new_types_[static_cast<size_t>(new_type_selected_)].enabled = v;
        refresh_new_type_row(new_type_selected_);
        write_new_types();
    });
    shown->name = "NewTypeEnabled";
    shown->set_enabled(has);
    f::gap(*editor, 8);
    auto* import = f::button(*editor, importing ? L"Reading what Windows offers\u2026" : L"Import from Windows\u2026", [this] {
        new_types_importing_ = true;  // read the list again once it is done, however quickly that is
        bb_new_types_import(core_.app());
    });
    import->name = "NewTypeImport";
    import->width = 240;
    import->set_enabled(!importing);
    f::note(*editor, L"Adds the file types Windows offers in its own New menu.");
    const std::wstring status = core::wide(st["status"].str());
    if (!status.empty()) f::text(*editor, status, tok::TextSecondary, 13.0f)->name = "NewTypeImportStatus";
}

// The default columns a new tab starts from (a tab that was arranged by hand keeps its own). Name is first
// and stays. Spec: SettingsView.xaml (Columns), SettingsViewModel (Columns).
void SettingsView::page_columns(Widget& p) {
    BbApp* app = core_.app();
    auto edit = [this, app](const std::wstring& verb, const std::wstring& arg) {
        bb_columns_defaults_do(app, core::s(verb), core::s(arg));
        main_.defer([this] { build_page(); });
    };
    f::heading(p, L"Columns");
    f::note(p, L"The columns a new tab starts with, in order. A tab whose columns you have arranged by hand keeps its own; right-click a header in the list to change just that tab.");
    const json::Value st = try_model([&](BbOwnedStr* o) { return bb_columns_defaults(app, o); });
    const json::Value& cols = st["columns"];
    if (column_selected_ >= static_cast<int>(cols.size())) column_selected_ = static_cast<int>(cols.size()) - 1;
    auto* list = p.emplace<f::ItemList>();
    list->name = "ColumnRows";
    list->row_height = 30;
    list->height = 30.0f * static_cast<float>(std::max<size_t>(4, std::min<size_t>(10, cols.size()))) + 4;
    std::vector<f::ListEntry> entries;
    std::vector<std::wstring> ids;
    std::vector<bool> movable;
    for (size_t i = 0; i < cols.size(); ++i) {
        f::ListEntry e;
        e.text = core::wide(cols[i]["header"].str());
        // A property column says which property it is beside its width, as the C# row does.
        const std::wstring id = core::wide(cols[i]["id"].str());
        const std::wstring px = std::to_wstring(static_cast<int>(std::lround(cols[i]["width"].num()))) + L" px";
        e.detail = id.find(L'.') != std::wstring::npos ? id + L"   " + px : px;
        entries.push_back(std::move(e));
        ids.push_back(id);
        movable.push_back(cols[i]["removable"].boolean());
    }
    list->set_entries(std::move(entries), column_selected_);
    list->automation_id = "ColumnRows";
    list->on_select = [this](int i) {
        column_selected_ = i;
        main_.defer([this] { build_page(); });
    };
    // Dragged to a new place, or Alt+Up/Down: Name is first and stays there. Spec: SettingsView.xaml.cs
    // (ColumnList drag, ColumnList_KeyDown).
    list->can_drag = [movable](int i) { return i >= 0 && static_cast<size_t>(i) < movable.size() && movable[static_cast<size_t>(i)]; };
    list->on_reorder = [this, edit, ids](int from, int to) {
        if (from < 0 || static_cast<size_t>(from) >= ids.size()) return;
        to = std::max(1, to);
        column_selected_ = to;
        edit(L"move", ids[static_cast<size_t>(from)] + L" " + std::to_wstring(to));
    };
    nudge_ = [this, edit, ids, movable](int d) {
        const int at = column_selected_, to = at + d;
        if (at < 1 || static_cast<size_t>(at) >= ids.size() || !movable[static_cast<size_t>(at)] || to < 1 || static_cast<size_t>(to) >= ids.size()) return;
        column_selected_ = to;
        edit(L"move", ids[static_cast<size_t>(at)] + L" " + std::to_wstring(to));
    };
    nudge_for_ = list;
    if (dragging_pose_) list->set_drop_line(2);  // the line a row drag draws, placed rather than dragged
    const int sel = column_selected_;
    const bool has = sel >= 0 && sel < static_cast<int>(ids.size());
    const std::wstring id = has ? ids[static_cast<size_t>(sel)] : std::wstring();
    const bool removable = has && cols[static_cast<size_t>(sel)]["removable"].boolean();
    list->on_key = [this, edit, id, removable, sel](unsigned vk) {
        if (vk == VK_DELETE && removable) {
            edit(L"remove", id);
            return true;
        }
        return false;
    };
    auto* r = f::row(p);
    auto* add = f::button(*r, L"Add column…", nullptr);  // a popup under the button, as C#'s
    add->on_click = [this, add] { main_.open_dialog_at(L"default-columns", Point{add->bounds.l, add->bounds.b + 2}); };
    add->name = "ColumnAdd";
    add->icon = &icons::Add;
    auto* remove = f::button(*r, L"Remove", [edit, id] { edit(L"remove", id); });
    remove->name = "ColumnRemove";
    remove->set_enabled(removable);
    // Name is first whichever way it is asked, so it neither moves nor has anything moved above it.
    auto* up = f::button(*r, L"\u2191", [this, edit, id, sel] {
        column_selected_ = sel - 1;
        edit(L"move", id + L" " + std::to_wstring(sel - 1));
    }, 32);
    up->tooltip = L"Move left";
    up->set_enabled(removable && sel > 1);
    auto* down = f::button(*r, L"\u2193", [this, edit, id, sel] {
        column_selected_ = sel + 1;
        edit(L"move", id + L" " + std::to_wstring(sel + 1));
    }, 32);
    down->tooltip = L"Move right";
    down->set_enabled(removable && sel + 1 < static_cast<int>(ids.size()));
    f::button(*r, L"Reset", [this, edit] {
        column_selected_ = 0;
        edit(L"reset", L"");
    })->name = "ColumnReset";
    f::note(p, L"Drag to reorder (or Alt+\u2191/\u2193). Scroll a width to change it; Ctrl+scroll by one.");
    if (has) {
        auto* wr = p.emplace<WheelRow>();
        wr->on_step = [edit, id](int notches, bool fine) {
            edit(L"step-width", id + L" " + std::to_wstring(notches) + (fine ? L" fine" : L""));
        };
        f::row_label(*wr, L"Width of \u201c" + core::wide(cols[static_cast<size_t>(sel)]["header"].str()) + L"\u201d", 220);
        auto* box = f::field(*wr, std::to_wstring(static_cast<int>(std::lround(cols[static_cast<size_t>(sel)]["width"].num()))), 80,
                             nullptr);
        box->name = "ColumnWidth";
        box->automation_id = "ColumnWidth";
        box->on_commit = [box, edit, id] {
            const std::wstring t = box->text();
            if (!t.empty() && std::all_of(t.begin(), t.end(), [](wchar_t c) { return c >= L'0' && c <= L'9'; })) edit(L"width", id + L" " + t);
        };
        f::text(*wr, L"px  (Enter to apply)", tok::TextMuted, 13.0f);
    }
}

void SettingsView::page_saved(Widget& p, bool workspaces) {
    f::heading(p, workspaces ? L"Workspaces" : L"Saved searches");
    f::note(p, workspaces ? L"A workspace is your panes and tabs saved under a name, to switch back to in one click. Switching replaces the panes you have open."
                          : L"A saved search keeps a query under a name, to run again in one click.");
    const json::Value st = try_model([&](BbOwnedStr* o) { return bb_saved_state(core_.app(), o); });
    const char* key = workspaces ? "workspacesPlacement" : "savedSearchesPlacement";
    const std::string cur = st[workspaces ? "workspacesPlacement" : "savedSearchesPlacement"].str();
    f::text(p, workspaces ? L"Show the workspace switcher" : L"Show saved searches", tok::TextSecondary, 14.0f, 600);
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
    if (workspaces) f::note(p, L"Nowhere only hides the switcher. Your workspaces are kept, and stay listed below.");
    f::gap(p, 6);
    f::text(p, workspaces ? L"Saved workspaces" : L"Your saved searches", tok::TextSecondary, 14.0f, 600);
    const json::Value& items = st[workspaces ? "workspaces" : "searches"];
    if (items.size() == 0) {
        f::note(p, workspaces ? L"Nothing saved yet. Use the + beside WORKSPACES in the sidebar, or Save current layout in the title bar\u2019s workspace menu."
                              : L"Nothing saved yet. Type a search, then press the save button inside the search box.")
            ->name = workspaces ? "SettingsWorkspacesEmpty" : "SettingsSavedSearchesEmpty";
        return;
    }
    // What it is, when it was made and when it was last used, under the original's column headings.
    // Spec: SettingsView.xaml (SavedSearchesList, WorkspacesList).
    auto* list = p.emplace<tools::TableList>();
    list->name = workspaces ? "WorkspaceList" : "SavedSearchList";
    list->height = 220;
    list->columns = {{L"Name", 100.0f, true},
                     {workspaces ? L"Layout" : L"Searches", 170.0f},
                     {L"Created", 130.0f},
                     {workspaces ? L"Last opened" : L"Last run", 130.0f}};
    std::vector<tools::TableRow> entries;
    std::vector<std::wstring> names, more;
    for (size_t i = 0; i < items.size(); ++i) {
        names.push_back(core::wide(items[i]["name"].str()));
        // Under the list for the selected row, and as the row's tooltip: the query as it runs, or the folders the
        // workspace opens.
        if (workspaces) {
            std::wstring folders;
            for (size_t k = 0; k < items[i]["folders"].size(); ++k) folders += (k ? L"  \u00b7  " : L"") + core::wide(items[i]["folders"][k].str());
            more.push_back(folders);
        } else {
            more.push_back(core::wide(items[i]["query"].str()));
        }
        tools::TableRow row;
        row.cells = {names.back(), core::wide(workspaces ? items[i]["shape"].str() : items[i]["scopeColumn"].str()),
                     core::wide(items[i]["createdText"].str()), core::wide(items[i]["lastUsedText"].str())};
        row.tip = more.back();
        entries.push_back(std::move(row));
    }
    list->set_rows(std::move(entries));
    auto* about = f::text(p, L"", tok::TextSecondary, 13.0f);
    about->name = "SavedAbout";
    list->on_select = [about, more](int i) {
        about->set_text(i >= 0 && static_cast<size_t>(i) < more.size() ? more[static_cast<size_t>(i)] : L"");
    };
    // Edit (a search) or Rename (a workspace) and Delete, for the selected row - the sidebar's menu's own.
    // Enter edits, Del deletes. Spec: SettingsView (Saved searches / Workspaces), ListKeyDown.
    auto* buttons = f::row(p, 8);
    BbApp* app = core_.app();
    auto selected = [list, names]() -> std::wstring {
        return list->selected >= 0 && static_cast<size_t>(list->selected) < names.size() ? names[static_cast<size_t>(list->selected)] : L"";
    };
    auto edit = [this, workspaces, selected] {
        const std::wstring n = selected();
        if (n.empty()) return;
        if (workspaces) main_.rename_workspace(n);
        else main_.edit_saved_search(n);
    };
    auto remove = [this, app, workspaces, selected] {
        const std::wstring n = selected();
        if (n.empty()) return;
        bb_saved_do(app, 0, core::s(workspaces ? L"remove-workspace" : L"remove-search"), core::s(n));
        signature_.clear();
        sync();
    };
    auto* edit_button = f::button(*buttons, workspaces ? L"Rename…" : L"Edit…", edit, 110);
    edit_button->name = workspaces ? "WorkspaceRename" : "SavedSearchEdit";
    edit_button->tooltip = workspaces ? L"Rename (F2)" : L"Edit name, search and scope (F2)";
    auto* delete_button = f::button(*buttons, L"Delete", remove, 90);
    delete_button->name = workspaces ? "WorkspaceDelete" : "SavedSearchDelete";
    delete_button->tooltip = L"Delete (Del)";
    f::note(p, workspaces ? L"Deleting a workspace removes only its name and layout \u2014 no folder or file is touched."
                          : L"Deleting a saved search removes only the search \u2014 no file it found is touched.");
    list->on_activate = [edit](int) { edit(); };
    // Deferred: either one rebuilds the page, which frees this list while its own handler is still running.
    list->on_key = [this, remove, edit](unsigned vk) {
        if (vk == VK_DELETE) main_.defer(remove);
        else if (vk == VK_F2) main_.defer(edit);
        else return false;
        return true;
    };
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
    f::note(p, L"What is on the menu bar and the toolbars, and in what order. Changes show in the title bar above as you make them. Every command stays in the command palette (" +
                   gesture_or(L"app.palette", L"Command palette") + L") whether or not it is placed here.");
    auto* top = f::row(p);
    auto* status = f::text(*top, core::wide(m["status"].str()), tok::TextMuted, 13.0f);
    status->flex = 1;
    auto rebuild = [this] { main_.defer([this] { build_page(); }); };
    auto* save_as = f::button(*top, L"Save as\u2026", [this, rebuild] {
        bar_naming_ = BarNaming::SaveAs;
        bar_name_text_.clear();
        rebuild();
    });
    save_as->name = "BarSaveAs";
    save_as->tooltip = L"Keep this arrangement under a name, to come back to";
    // Asks first when the arrangement in force is saved under no name (the core does).
    auto* reset = f::button(*top, L"Reset to default", [bars_do] { bars_do(L"reset"); });
    reset->name = "BarReset";
    reset->tooltip = L"The menus and toolbar BertBrowser ships with. Saved arrangements are kept.";
    reset->set_enabled(!m["isDefault"].boolean());
    if (bar_naming_ != BarNaming::None) {
        // A name box in place of a dialog: Save keeps the arrangement in force under it (replacing one of that
        // name), Rename renames the saved arrangement selected. Spec: SavedWorkspaceDialog (NamedThing.CommandBars).
        const bool renaming = bar_naming_ == BarNaming::Rename;
        const std::wstring old_name = core::wide(m["selectedSaved"].str());
        auto* nr = f::row(p, 6);
        f::row_label(*nr, renaming ? L"New name for \u201c" + old_name + L"\u201d" : L"Save the menus and toolbars as", 260);
        auto* box = f::field(*nr, bar_name_text_, 220, [this](const std::wstring& t) { bar_name_text_ = t; });
        box->automation_id = "BarSaveName";
        box->name = "BarSaveName";
        auto commit = [this, bars_do, renaming, old_name, rebuild] {
            std::wstring name = bar_name_text_;
            while (!name.empty() && iswspace(name.back())) name.pop_back();
            while (!name.empty() && iswspace(name.front())) name.erase(name.begin());
            if (name.empty()) return;
            bars_do(renaming ? L"rename-saved" : L"save", renaming ? old_name + L" to " + name : name);
            bars_do(L"select-saved", name);  // what was just named is the one the buttons below act on
            bar_naming_ = BarNaming::None;
            rebuild();
        };
        box->on_commit = commit;
        f::button(*nr, renaming ? L"Rename" : L"Save", commit)->name = "BarSaveCommit";
        f::button(*nr, L"Cancel", [this, rebuild] {
            bar_naming_ = BarNaming::None;
            rebuild();
        });
        if (!root() || !root()->focused() || !body_->is_ancestor_of(root()->focused())) {
            main_.defer([this] {
                if (Widget* w = body_->find("BarSaveName")) root()->set_focus(w);
            });
        }
    }

    auto* cols = f::row(p, 14);
    cols->height = 520;

    // -- which bar, and the saved arrangements
    auto* left = cols->emplace<StackPanel>(Orientation::Vertical, 6.0f);
    left->width = 230;
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
        if (i >= 0 && static_cast<size_t>(i) < tkeys.size()) bars_do(L"select", tkeys[static_cast<size_t>(i)]);
    };
    // Toolbar rows come and go here; the menu bar and the title bar always exist.
    auto* trow = f::row(*left, 6);
    auto* add_row = f::button(*trow, L"Add toolbar", [bars_do] { bars_do(L"add-row"); });
    add_row->name = "BarAddRow";
    add_row->tooltip = L"A new toolbar under the title bar";
    auto* remove_row = f::button(*trow, L"Remove", [bars_do] { bars_do(L"remove-row"); });
    remove_row->tooltip = L"Take the selected toolbar row away, with everything on it";
    remove_row->name = "BarRemoveRow";
    remove_row->set_enabled(m["target"]["kind"].str() == "row");
    f::text(*left, L"Saved arrangements", tok::TextSecondary, 13.0f, 600);
    auto* saved = left->emplace<f::ItemList>();
    saved->flex = 1;
    std::vector<f::ListEntry> se;
    std::vector<std::wstring> snames;
    int ssel = -1;
    for (size_t i = 0; i < m["saved"].size(); ++i) {
        f::ListEntry e;
        // The one in force is ticked; its shape is the detail either way.
        e.text = core::wide(m["saved"][i]["name"].str()) + (m["saved"][i]["current"].boolean() ? L"  \u2713" : L"");
        e.detail = core::wide(m["saved"][i]["shape"].str());
        se.push_back(std::move(e));
        snames.push_back(core::wide(m["saved"][i]["name"].str()));
        if (m["saved"][i]["name"].str() == m["selectedSaved"].str()) ssel = static_cast<int>(i);
    }
    const bool none_saved = se.empty();
    saved->set_entries(std::move(se), ssel);
    if (none_saved) {
        f::note(*left, L"Nothing saved yet. Save as\u2026 keeps the arrangement above under a name; View \u25b8 Menus and toolbars and the command palette switch between them.")
            ->name = "SavedBarsEmpty";
    }
    saved->automation_id = "SavedBarsList";
    saved->on_select = [bars_do, snames](int i) {
        if (i >= 0 && static_cast<size_t>(i) < snames.size()) bars_do(L"select-saved", snames[static_cast<size_t>(i)]);
    };
    saved->on_activate = [bars_do, snames](int i) {
        if (i >= 0 && static_cast<size_t>(i) < snames.size()) bars_do(L"apply-saved", snames[static_cast<size_t>(i)]);
    };
    // Enter and a double click use it (above); F2 renames and Del deletes. Spec: SettingsView.xaml.cs (SavedBarsList_KeyDown).
    saved->on_key = [this, bars_do, snames, saved, rebuild](unsigned vk) {
        const int at = saved->selected;
        if (at < 0 || static_cast<size_t>(at) >= snames.size()) return false;
        if (vk == VK_F2) {
            bar_naming_ = BarNaming::Rename;
            bar_name_text_ = snames[static_cast<size_t>(at)];
            rebuild();
            return true;
        }
        if (vk == VK_DELETE) {
            bars_do(L"remove-saved", snames[static_cast<size_t>(at)]);
            return true;
        }
        return false;
    };
    auto* sr = f::row(*left, 6);
    auto* use = f::button(*sr, L"Use", [bars_do, snames, ssel] { if (ssel >= 0) bars_do(L"apply-saved", snames[static_cast<size_t>(ssel)]); });
    use->name = "BarUseSaved";
    use->tooltip = L"Switch the menus and toolbars to this arrangement (Enter)";
    use->set_enabled(ssel >= 0);
    auto* rename = f::button(*sr, L"Rename\u2026", [this, snames, ssel, rebuild] {
        if (ssel < 0) return;
        bar_naming_ = BarNaming::Rename;
        bar_name_text_ = snames[static_cast<size_t>(ssel)];
        rebuild();
    });
    rename->name = "BarRenameSaved";
    rename->tooltip = L"Rename (F2)";
    rename->set_enabled(ssel >= 0);
    auto* forget = f::button(*sr, L"Remove", [bars_do, snames, ssel] { if (ssel >= 0) bars_do(L"remove-saved", snames[static_cast<size_t>(ssel)]); });
    forget->tooltip = L"Delete (Del)";
    forget->set_enabled(ssel >= 0);

    // -- what is on the bar
    auto* mid = cols->emplace<StackPanel>(Orientation::Vertical, 6.0f);
    mid->flex = 1;
    auto* loc = f::row(*mid);
    if (m["canUp"].boolean()) f::button(*loc, L"←", [bars_do] { bars_do(L"up"); })->tooltip = L"Back out of this submenu (Backspace)";
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
        // The note, then the shortcut, as the menu prints them.
        const std::wstring detail = core::wide(r["detail"].str()), gesture = core::wide(r["gesture"].str());
        e.detail = detail + (detail.empty() || gesture.empty() ? L"" : L"   ") + gesture;
        e.icon = icon_for(r["icon"].str());
        re.push_back(std::move(e));
        if (r["selected"].boolean()) rsel = static_cast<int>(i);
    }
    rows->set_entries(std::move(re), rsel);
    rows->on_select = [bars_do](int i) { bars_do(L"pick", std::to_wstring(i)); };
    rows->on_activate = [bars_do](int i) { bars_do(L"open", std::to_wstring(i)); };
    // Alt+Up/Down moves the selected item, Del takes it off, Enter opens a submenu (above) and Backspace backs
    // out of one. Spec: SettingsView.xaml.cs (BarItemsList_KeyDown).
    rows->automation_id = "BarRows";
    nudge_ = [bars_do](int d) { bars_do(L"nudge", d < 0 ? L"up" : L"down"); };
    nudge_for_ = rows;
    // Or dragged to its new place. Spec: SettingsView.xaml.cs (BarItemsList drag).
    rows->on_reorder = [bars_do](int from, int to) { bars_do(L"move", std::to_wstring(from) + L" " + std::to_wstring(to)); };
    const bool can_up = m["canUp"].boolean();
    rows->on_key = [bars_do, can_up](unsigned vk) {
        if (vk == VK_DELETE) {
            bars_do(L"remove");
            return true;
        }
        if (vk == VK_BACK && can_up) {
            bars_do(L"up");
            return true;
        }
        return false;
    };
    auto* rb = f::row(*mid, 6);
    const bool row_is_menu = rsel >= 0 && m["rows"][static_cast<size_t>(rsel)]["menu"].boolean();
    auto* open = f::button(*rb, L"Open", [bars_do, rsel] { if (rsel >= 0) bars_do(L"open", std::to_wstring(rsel)); });
    open->name = "BarOpen";
    open->tooltip = L"Edit what is inside the selected menu (Enter)";
    open->set_enabled(row_is_menu);
    f::button(*rb, L"↑", [bars_do] { bars_do(L"nudge", L"up"); });
    f::button(*rb, L"↓", [bars_do] { bars_do(L"nudge", L"down"); });
    f::button(*rb, L"Remove", [bars_do] { bars_do(L"remove"); })->tooltip = L"Take this off (Del)";
    f::text(*mid, L"Alt+\u2191/\u2193 moves \u00b7 Del removes \u00b7 Enter opens a menu", tok::TextMuted, 11.0f);
    auto* rb2 = f::row(*mid, 6);
    // The menu bar's own level holds menus only, so there is nothing to separate. Spec: CanAddBarSeparator.
    auto* add_sep = f::button(*rb2, L"Add separator", [bars_do] { bars_do(L"add-separator"); });
    add_sep->name = "BarAddSeparator";
    add_sep->set_enabled(m["canAdd"].boolean());
    // At the top of the menu bar only menus can be added: a command needs a menu to be in.
    auto* addm = f::button(*rb2, m["addSubmenuLabel"].str() == "Submenu" ? L"Add submenu" : L"Add menu", [bars_do] { bars_do(L"add-menu"); });
    addm->name = "BarAddMenu";
    addm->tooltip = L"An empty menu to fill: open it, then add commands from the right";
    if (m["canEditCaption"].boolean()) {
        auto* cr = f::row(*mid, 6);
        f::row_label(*cr, L"Name", 60);
        auto* ce = f::field(*cr, core::wide(m["caption"].str()), 240, [bars_do](const std::wstring& t) { bars_do(L"name", t); });
        ce->automation_id = "BarCaption";
        ce->width = -1;
        ce->flex = 1;
        f::note(*mid, core::wide(m["captionHint"].str()));
    }
    if (m["isToolbar"].boolean()) {
        f::check(*mid, L"Show names beside the icons", m["showsText"].boolean(),
                 [bars_do](bool v) { bars_do(L"show-text", v ? L"on" : L"off"); });
        // One button can carry its name when the rest of the toolbar is icons only.
        if (rsel >= 0 && !m["rows"][static_cast<size_t>(rsel)]["separator"].boolean() && !m["showsText"].boolean()) {
            f::check(*mid, L"Always show this button\u2019s name", m["rows"][static_cast<size_t>(rsel)]["showText"].boolean(),
                     [bars_do](bool v) { bars_do(L"row-show-text", v ? L"on" : L"off"); });
        }
    }

    // -- the picker
    auto* right = cols->emplace<StackPanel>(Orientation::Vertical, 6.0f);
    right->width = 240;
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
        // A list is placed by "@id"; a command by its id.
        const std::string id = c["id"].str().empty() ? c["name"].str() : c["id"].str();
        cids.push_back(core::wide(c["kind"].str() == "list" ? "@" + id : id));
    }
    choices->set_entries(std::move(ce));
    const bool can_add = m["canAdd"].boolean();
    choices->on_activate = [bars_do, cids, can_add](int i) { if (i >= 0 && can_add) bars_do(L"add", cids[static_cast<size_t>(i)]); };
    // Dragged onto the list, a choice lands exactly where it is dropped. Spec: SettingsView.xaml.cs (BarChoices drag).
    if (can_add) {
        choices->tooltip = L"Double-click to add after the selected item, or drag onto the list to put it exactly where you want";
        choices->can_drag = [cids](int i) { return i >= 0 && static_cast<size_t>(i) < cids.size() && !cids[static_cast<size_t>(i)].empty(); };
        choices->drag_into = rows;
        rows->on_drop_in = [bars_do, cids](int from, int at) {
            if (from >= 0 && static_cast<size_t>(from) < cids.size()) bars_do(L"add-at", std::to_wstring(at) + L" " + cids[static_cast<size_t>(from)]);
        };
    }
    auto* add = f::button(*right, L"Add", [bars_do, choices, cids] {
        if (choices->selected >= 0) bars_do(L"add", cids[static_cast<size_t>(choices->selected)]);
    });
    add->name = "BarAdd";
    add->set_enabled(can_add);
    if (!can_add) f::note(*right, L"Open a menu to add commands to it; the menu bar itself holds menus.");
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
    f::note(p, L"Every command, and the keys that run it. Select one and choose Change, then press the keys you want. The command palette (" +
                   gesture_or(L"app.palette", L"Command palette") + L") finds any of these by name.");
    auto* top = f::row(p);
    auto* filter = f::field(*top, core::wide(m["filter"].str()), 320, [keys_do](const std::wstring& t) { keys_do(L"filter", t); }, L"Filter by name, category or shortcut");
    filter->automation_id = "KeyFilter";
    auto* find = f::button(*top, L"Find by key…", [keys_do] { keys_do(L"find"); });
    find->name = "KeyFind";
    find->tooltip = L"Press a shortcut to see which command it runs";
    auto* reset_all = f::button(*top, L"Reset all", [keys_do] { keys_do(L"reset-all"); });
    reset_all->name = "KeyResetAll";
    reset_all->set_enabled(m["hasChanged"].boolean());
    reset_all->tooltip = L"Put every command back to the shortcut it ships with";

    // The selected command, which the keys below and the buttons under the list act on.
    const json::Value* chosen = nullptr;
    for (size_t i = 0; i < m["rows"].size(); ++i) {
        if (!m["rows"][i]["heading"].boolean() && m["rows"][i]["id"].str() == m["selected"].str()) chosen = &m["rows"][i];
    }
    const bool recording = m["recording"].boolean() || m["capturing"].boolean();
    const bool have = chosen != nullptr;
    const bool bound = have && (*chosen)["chords"].size() > 0;

    auto* list = p.emplace<f::ItemList>();
    list->name = "KeyRows";
    list->automation_id = "KeyRows";  // the page is rebuilt by every change; focus comes back to the list through this
    list->flex = 1;  // takes whatever height the card leaves under the filter row and above the recorder
    std::vector<f::ListEntry> entries;
    std::vector<std::wstring> ids;
    int sel = -1;
    for (size_t i = 0; i < m["rows"].size(); ++i) {
        const json::Value& r = m["rows"][i];
        f::ListEntry e;
        e.heading = r["heading"].boolean();
        // A dot marks a command whose shortcuts are not the ones it ships with.
        e.text = core::wide(r["name"].str()) + (!e.heading && r["changed"].boolean() ? L"  \u2022" : L"");
        e.icon = e.heading ? nullptr : icon_for(r["icon"].str());
        const std::wstring chords = join_chords(r["chords"]);
        const std::wstring where = core::wide(r["where"].str());
        e.detail = e.heading ? L"" : (chords.empty() ? L"none" : chords) + (where.empty() || chords.empty() ? L"" : L"   " + where);
        e.muted = !e.heading && chords.empty();
        entries.push_back(std::move(e));
        ids.push_back(core::wide(r["id"].str()));
        if (!r["heading"].boolean() && r["id"].str() == m["selected"].str()) sel = static_cast<int>(i);
    }
    list->set_entries(std::move(entries), sel);
    if (sel >= 0) list->scroll_into_view(sel);  // a command chosen from the palette, or after a reassign
    list->on_select = [keys_do, ids](int i) {
        if (i >= 0 && static_cast<size_t>(i) < ids.size() && !ids[static_cast<size_t>(i)].empty()) keys_do(L"select", ids[static_cast<size_t>(i)]);
    };
    // Enter changes the selected command's shortcut (a double click too) and Del removes it, while the
    // recorder is not listening. Spec: SettingsView.xaml.cs (KeyList_KeyDown, KeyList_DoubleClick).
    const bool listening = recording || m["canReassign"].boolean();
    list->on_activate = [keys_do, listening](int) { if (!listening) keys_do(L"change"); };
    list->on_key = [keys_do, listening, bound](unsigned vk) {
        if (vk != VK_DELETE || listening || !bound) return false;
        keys_do(L"remove");
        return true;
    };
    // The recorder: one line saying what is being waited for, or what was refused.
    auto* panel = p.emplace<StackPanel>(Orientation::Vertical, 6.0f);
    panel->name = "KeyRecorderPanel";
    const std::wstring line = core::wide(m["text"].str());
    if (!line.empty()) {
        auto* t = f::text(*panel, line, recording ? tok::AccentBackground : tok::TextPrimary, 14.0f, 600);
        t->name = "KeyRecorderText";
        t->wrap = true;
    }
    if (line.empty() && !chosen && !recording) {
        f::text(*panel, L"Select a command to change its shortcut.", tok::TextSecondary, 14.0f)->name = "KeyRecorderHint";
    }
    // The selected command: what it ships with, so a change can be judged against it.
    if (chosen && !recording) {
        const std::wstring where = core::wide((*chosen)["where"].str());
        auto* d = f::text(*panel, core::wide((*chosen)["defaultText"].str()) + (where.empty() ? L"" : L"; works " + where) + L".", tok::TextMuted, 13.0f);
        d->name = "KeyDefaultText";
    }
    auto* br = f::row(*panel, 8);
    if (m["canReassign"].boolean()) f::button(*br, L"Reassign", [keys_do] { keys_do(L"reassign"); })->name = "KeyReassign";
    if (recording || m["canReassign"].boolean()) {
        f::button(*br, L"Cancel", [keys_do] { keys_do(L"cancel"); })->name = "KeyCancel";
    } else {
        auto* change = f::button(*br, L"Change…", [keys_do] { keys_do(L"change"); });
        change->name = "KeyChange";
        change->tooltip = L"Replace this command’s shortcut (Enter)";
        change->set_enabled(have);
        // Another shortcut only for something that has one already; with none, Change is the way in.
        auto* add = f::button(*br, L"Add another…", [keys_do] { keys_do(L"add"); });
        add->name = "KeyAdd";
        add->tooltip = L"Give this command a second shortcut";
        add->set_enabled(bound);
        auto* remove = f::button(*br, L"Remove", [keys_do] { keys_do(L"remove"); });
        remove->name = "KeyRemove";
        remove->tooltip = L"Leave this command with no shortcut (Del)";
        remove->set_enabled(bound);
        auto* reset = f::button(*br, L"Reset", [keys_do] { keys_do(L"reset"); });
        reset->name = "KeyReset";
        reset->tooltip = L"Put back the shortcut it ships with";
        reset->set_enabled(have && (*chosen)["changed"].boolean());
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
