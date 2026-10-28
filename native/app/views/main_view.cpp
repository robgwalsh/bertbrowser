#include "views/main_view.h"

#include "views/folder_tree.h"

#include <dwmapi.h>

#include <algorithm>
#include <cmath>

namespace bb::views {

using namespace bb::ui;

namespace {

// A split whose first child gets `fraction` of the space the first time it is laid out; after that the
// user's drag is the truth.
class WeightedSplitter : public Splitter {
public:
    WeightedSplitter(Orientation o, float fraction) : Splitter(o), fraction_(fraction) {}
    void arrange(Rect r) override {
        if (!placed_ && !r.empty()) {
            const float total = orientation == Orientation::Horizontal ? r.width() : r.height();
            set_position(fraction_ * (total - thickness));
            placed_ = true;
        }
        Splitter::arrange(r);
    }

private:
    float fraction_;
    bool placed_ = false;
};

std::wstring key_name(unsigned vk) {
    if ((vk >= 'A' && vk <= 'Z') || (vk >= '0' && vk <= '9')) return std::wstring(1, static_cast<wchar_t>(vk));
    if (vk >= VK_F1 && vk <= VK_F24) return L"F" + std::to_wstring(vk - VK_F1 + 1);
    if (vk >= VK_NUMPAD0 && vk <= VK_NUMPAD9) return L"Num" + std::to_wstring(vk - VK_NUMPAD0);
    switch (vk) {
        case VK_BACK: return L"Backspace";
        case VK_DELETE: return L"Del";
        case VK_RETURN: return L"Enter";
        case VK_ESCAPE: return L"Esc";
        case VK_TAB: return L"Tab";
        case VK_SPACE: return L"Space";
        case VK_LEFT: return L"Left";
        case VK_RIGHT: return L"Right";
        case VK_UP: return L"Up";
        case VK_DOWN: return L"Down";
        case VK_HOME: return L"Home";
        case VK_END: return L"End";
        case VK_PRIOR: return L"PageUp";
        case VK_NEXT: return L"PageDown";
        case VK_INSERT: return L"Insert";
        case VK_ADD: return L"Num+";
        case VK_SUBTRACT: return L"Num-";
        case VK_MULTIPLY: return L"Num*";
        case VK_DIVIDE: return L"Num/";
        case VK_DECIMAL: return L"Num.";
        case VK_OEM_COMMA: return L",";
        case VK_OEM_PERIOD: return L".";
        case VK_OEM_2: return L"/";
        case VK_OEM_1: return L";";
        case VK_OEM_7: return L"'";
        case VK_OEM_4: return L"[";
        case VK_OEM_6: return L"]";
        case VK_OEM_5: return L"\\";
        case VK_OEM_MINUS: return L"-";
        case VK_OEM_PLUS: return L"=";
        case VK_OEM_3: return L"`";
        default: return L"";
    }
}

const json::Value& pane_state(const json::Value& panes, uint64_t id) {
    static const json::Value none;
    for (size_t i = 0; i < panes.size(); ++i) {
        if (static_cast<uint64_t>(panes[i]["id"].i64()) == id) return panes[i];
    }
    return none;
}

std::string signature(const json::Value& node, const json::Value& panes) {
    if (node["kind"].str() == "pane") {
        const uint64_t id = static_cast<uint64_t>(node["pane"].i64());
        const json::Value& p = pane_state(panes, id);
        std::string s = "P" + std::to_string(id) + "[";
        for (size_t i = 0; i < p["tabs"].size(); ++i) s += std::to_string(p["tabs"][i].i64()) + ",";
        return s + "|" + std::to_string(p["activeTab"].i64(-1)) + "]";
    }
    std::string s = "S" + node["orientation"].str() + "(";
    const json::Value& kids = node["children"];
    for (size_t i = 0; i < kids.size(); ++i) s += signature(kids[i], panes) + ";";
    return s + ")";
}

}  // namespace

// ---- ListAdapter -------------------------------------------------------------------------------------

void ListAdapter::reset(std::shared_ptr<core::Listing> l) {
    listing_ = std::move(l);
    reload_selection();
}

void ListAdapter::reload_selection() {
    sel_.assign(row_count(), 0);
    for (const uint32_t i : core_.selection(tab_)) {
        if (i < sel_.size()) sel_[i] = 1;
    }
}

uint64_t ListAdapter::row_id(size_t row) const {
    BbRow r{};
    return listing_ && listing_->row(row, r) ? r.row_id : row;
}

std::wstring ListAdapter::cell_text(size_t row, int column_id) const {
    if (!listing_ || column_id < 0 || static_cast<size_t>(column_id) >= cells_.size()) return {};
    const int cell = cells_[static_cast<size_t>(column_id)];
    return cell < 0 ? std::wstring() : listing_->cell(row, static_cast<uint32_t>(cell));
}

void ListAdapter::select(SelectOp op, size_t first, size_t last) {
    if (op == SelectOp::Clear) {
        core_.select(tab_, BB_SELECT_CLEAR, {});
    } else {
        std::vector<uint32_t> idx;
        const size_t n = row_count();
        for (size_t i = first; i <= last && i < n; ++i) idx.push_back(static_cast<uint32_t>(i));
        core_.select(tab_, op == SelectOp::Replace ? BB_SELECT_REPLACE : op == SelectOp::Add ? BB_SELECT_ADD : BB_SELECT_TOGGLE,
                     idx);
    }
    reload_selection();
}

// ---- TabView -----------------------------------------------------------------------------------------

TabView::TabView(MainView& main, core::Core& core, uint64_t tab)
    : main_(main), core_(core), tab_(tab), adapter_(core, tab) {
    style.bg = tok::ListBackground;
    BbApp* app = core.app();

    auto nav = std::make_unique<StackPanel>(Orientation::Horizontal, 6.0f);
    nav->pad = 6;
    nav->height = 42;
    nav->dock = Dock::Top;
    nav->name = "NavBar";
    nav->style.bg = tok::ToolbarBackground;
    nav_ = nav.get();
    auto button = [&](const wchar_t* glyph, std::function<void()> fn) {
        auto b = std::make_unique<Button>(glyph);
        b->width = 34;
        b->on_click = std::move(fn);
        return nav_->add(std::move(b));
    };
    back_ = button(L"←", [this, app] { bb_tab_back(app, tab_); });
    forward_ = button(L"→", [this, app] { bb_tab_forward(app, tab_); });
    up_ = button(L"↑", [this, app] { bb_tab_up(app, tab_); });
    refresh_ = button(L"↻", [this, app] { bb_tab_refresh(app, tab_); });
    back_->name = "BackButton";
    forward_->name = "ForwardButton";
    up_->name = "UpButton";
    refresh_->name = "RefreshButton";

    path_bar_ = nav_->emplace<Panel>();
    path_bar_->flex = 1;
    path_bar_->name = "PathBar";
    crumbs_ = path_bar_->emplace<BreadcrumbBar>();
    crumbs_->name = "Breadcrumb";
    crumbs_->on_navigate = [this](const std::wstring& p) { core_.navigate(tab_, p); };
    crumbs_->on_edit = [this] { begin_path_edit(); };
    path_ = path_bar_->emplace<PathEdit>();
    path_->name = "PathBox";
    path_->set_visible(false);
    path_->on_commit = [this] {
        const std::wstring target = path_->text();
        end_path_edit();
        try {
            core_.navigate(tab_, target);
        } catch (const core::CoreError&) {
        }
    };
    path_->on_cancel = [this] { end_path_edit(); };
    path_->on_blur = [this] { end_path_edit(); };

    search_ = nav_->emplace<Edit>();
    search_->name = "SearchBox";
    search_->width = 200;
    search_->placeholder = L"Search";
    search_->on_changed = [this](const std::wstring&) { on_search_changed(false); };
    gsearch_ = nav_->emplace<Edit>();
    gsearch_->name = "GlobalSearchBox";
    gsearch_->width = 200;
    gsearch_->placeholder = L"Search this PC";
    gsearch_->on_changed = [this](const std::wstring&) { on_search_changed(true); };
    add(std::move(nav));

    auto banner = std::make_unique<Panel>();
    banner->pad = 4;
    banner->height = 28;
    banner->dock = Dock::Top;
    banner->name = "ErrorBanner";
    banner->style.bg = tok::ErrorBackground;
    banner_ = banner->emplace<TextBlock>();
    banner_->style.fg = tok::ErrorForeground;
    banner->set_visible(false);
    banner_panel_ = add(std::move(banner));

    auto body = std::make_unique<Panel>();
    body->dock = Dock::Fill;
    body_ = add(std::move(body));
    list_ = body_->emplace<ListView>();
    list_->name = "FileListView";
    list_->set_provider(&adapter_);
    empty_ = body_->emplace<Overlay>();
    empty_->name = "EmptyMessage";
    empty_->set_visible(false);

    list_->on_sort = [this, app](int column, bool) {
        if (column < 0 || static_cast<size_t>(column) >= columns_.size()) return;
        bb_tab_set_sort(app, tab_, core::s(columns_[static_cast<size_t>(column)].id));
    };
    list_->on_activate = [this, app](size_t row) { bb_tab_open_item(app, tab_, static_cast<uint32_t>(row)); };
    list_->on_typeahead = [this](const std::wstring& prefix) {
        const auto& l = adapter_.listing();
        if (!l) return;
        for (size_t i = 0; i < l->size(); ++i) {
            if (core::iequals(l->name(i).substr(0, prefix.size()), prefix)) {
                list_->select_row(i);
                return;
            }
        }
    };
}

void TabView::sync_columns() {
    const auto cols = core_.columns(tab_);
    std::vector<int> cells;
    for (const auto& c : cols) cells.push_back(c.cell);
    adapter_.set_cells(cells);
    bool same = cols.size() == columns_.size();
    for (size_t i = 0; same && i < cols.size(); ++i) same = cols[i].id == columns_[i].id;
    columns_ = cols;
    if (same) return;
    std::vector<Column> out;
    for (size_t i = 0; i < cols.size(); ++i) {
        Column c;
        c.id = static_cast<int>(i);
        c.title = cols[i].header;
        c.width = cols[i].width;
        c.right_align = cols[i].right;
        c.sortable = cols[i].sortable;
        out.push_back(std::move(c));
    }
    list_->set_columns(std::move(out));
}

void TabView::reload_listing() {
    adapter_.reset(core_.listing(tab_));
    sync_columns();
    list_->refresh();
    const bool none = adapter_.row_count() == 0;
    std::wstring message = none ? core_.tab_text(tab_, BB_TEXT_EMPTY) : std::wstring();
    empty_->set_message(std::move(message));
}

void TabView::reload_state() {
    BbTabState st{};
    try {
        st = core_.tab_state(tab_);
    } catch (const core::CoreError&) {
        return;
    }
    back_->set_enabled(st.can_back);
    forward_->set_enabled(st.can_forward);
    up_->set_enabled(st.can_up);

    const std::wstring path = core_.tab_text(tab_, BB_TEXT_PATH);
    if (path != path_text_) {
        path_text_ = path;
        crumbs_->set_crumbs(core_.breadcrumb(tab_));
        if (!path_->flags.visible) path_->set_text(path);
        list_->set_provider(&adapter_);  // a different place: back to the top, no cursor
        main_.tab_navigated(tab_, path);
    }

    // The box that holds the active search shows it; typing in the other one is left alone.
    if (!search_->focused() && !gsearch_->focused()) {
        const std::wstring text = core_.tab_text(tab_, BB_TEXT_SEARCH);
        Edit* active = st.is_global_search ? gsearch_ : search_;
        Edit* other = st.is_global_search ? search_ : gsearch_;
        suppress_ = true;
        if (active->text() != text) active->set_text(text);
        if (!other->text().empty()) other->set_text(L"");
        suppress_ = false;
    }

    const std::wstring error = core_.tab_text(tab_, BB_TEXT_ERROR);
    banner_->set_text(error);
    banner_panel_->set_visible(!error.empty());

    adapter_.reload_selection();
    const std::wstring sort = core_.tab_text(tab_, BB_TEXT_SORT_BY);
    for (size_t i = 0; i < columns_.size(); ++i) {
        if (columns_[i].id == sort) list_->set_sort(static_cast<int>(i), !st.sort_descending);
    }
    list_->invalidate();
}

void TabView::on_search_changed(bool global) {
    if (suppress_) return;
    Edit* box = global ? gsearch_ : search_;
    bb_tab_set_search(core_.app(), tab_, core::s(box->text()), global);
}

void TabView::type_search(const std::wstring& text, bool global) {
    Edit* box = global ? gsearch_ : search_;
    Edit* other = global ? search_ : gsearch_;
    suppress_ = true;
    box->set_text(text);
    if (!text.empty()) other->set_text(L"");
    suppress_ = false;
    core::check(bb_tab_set_search(core_.app(), tab_, core::s(text), global), "search");
}

void TabView::begin_path_edit() {
    crumbs_->set_visible(false);
    path_->set_visible(true);
    path_->set_text(path_text_);
    root()->set_focus(path_);
    path_->select(0, path_text_.size());
    path_bar_->request_layout();
}

void TabView::end_path_edit() {
    if (!path_->flags.visible) return;
    path_->set_visible(false);
    crumbs_->set_visible(true);
    path_bar_->request_layout();
}

// ---- PaneView ----------------------------------------------------------------------------------------

bool PaneView::on_mouse_down(const MouseEvent&) {
    if (pane_ != main_.active_pane()) main_.activate_pane(pane_);
    return false;
}

// ---- MainView ----------------------------------------------------------------------------------------

MainView::MainView(core::Core& core, platform::Window& window)
    : core_(core), window_(window), tree_provider_(std::make_unique<FolderTreeProvider>()) {
    auto root = std::make_unique<DockPanel>();
    root->style.bg = tok::WindowBackground;
    title_ = root->emplace<TitleBar>();
    title_->name = "TitleBarRoot";
    title_->dock = Dock::Top;
    status_ = root->emplace<StatusBar>();
    status_->name = "StatusBar";
    status_->dock = Dock::Bottom;
    main_split_ = root->emplace<Splitter>(Orientation::Horizontal);
    main_split_->dock = Dock::Fill;
    main_split_->name = "MainSplitter";
    main_split_->set_position(240);
    tree_ = main_split_->emplace<TreeView>();
    tree_->name = "FolderTree";
    tree_->set_provider(tree_provider_.get());
    pane_host_ = main_split_->emplace<Panel>();
    pane_host_->name = "PaneHostSite";

    tree_->on_select = [this](uint64_t id) {
        if (revealing_ || !active_tab_) return;
        const std::wstring path = tree_provider_->path_of(id);
        if (!path.empty()) {
            try {
                core_.navigate(active_tab_, path);
            } catch (const core::CoreError&) {
            }
        }
    };
    window_.root().set_content(std::move(root));

    hwnd_ = window_.hwnd();
    window_.on_message = [this](UINT msg, WPARAM wp, LPARAM lp) { return on_window_message(msg, wp, lp); };
    window_.root().on_preview_key = [this](const KeyEvent& e) { return on_preview_key(e); };
    bb_app_set_event_sink(core_.app(), &MainView::sink, this);
}

MainView::~MainView() {
    bb_app_set_event_sink(core_.app(), nullptr, nullptr);
    window_.on_message = nullptr;
    window_.root().on_preview_key = nullptr;
}

void MainView::sink(void* ctx, BbEvent* evt) {
    // A Rust thread: enqueue and return. The window drains it on the UI thread.
    auto* self = static_cast<MainView*>(ctx);
    HWND hwnd = self->hwnd_.load();
    if (!hwnd || !PostMessageW(hwnd, WM_BB_EVENT, 0, reinterpret_cast<LPARAM>(evt))) {
        bb_event_free(evt);
    }
}

bool MainView::on_window_message(UINT msg, WPARAM, LPARAM lp) {
    if (msg != WM_BB_EVENT) return false;
    auto* evt = reinterpret_cast<BbEvent*>(lp);
    try {
        handle(*evt);
    } catch (const std::exception&) {
        // A view that cannot read the core right now repaints on the next event.
    }
    ++handled_;
    bb_event_free(evt);
    return true;
}

void MainView::handle(const BbEvent& e) {
    switch (e.kind) {
        case BB_EVT_LISTING_CHANGED: {
            TabView* tv = tab_view(e.id);
            if (!tv) {
                sync_layout();
                tv = tab_view(e.id);
            }
            if (tv) tv->reload_listing();
            refresh_status();
            break;
        }
        case BB_EVT_TAB_CHANGED: {
            TabView* tv = tab_view(e.id);
            if (!tv) {
                sync_layout();
            } else {
                tv->reload_state();
                sync_layout();
            }
            refresh_status();
            break;
        }
        case BB_EVT_LAYOUT_CHANGED:
        case BB_EVT_PANE_CHANGED:
            sync_layout();
            break;
        case BB_EVT_SETTINGS_CHANGED: {
            const std::wstring key = core::w(e.text);
            if (core::icontains(key, L"theme")) apply_theme();
            break;
        }
        case BB_EVT_UNDO_CHANGED:
        case BB_EVT_OP_FINISHED:
        case BB_EVT_BUSY_CHANGED:
            refresh_status();
            break;
    }
}

void MainView::start(const std::wstring& path) {
    apply_theme();
    uint64_t tab = 0;
    core::check(bb_app_initialize(core_.app(), core::s(path), &tab), "initialize");
    sync_layout();
}

void MainView::apply_theme() {
    const std::vector<uint32_t> argb = core_.palette();
    if (argb.size() != gfx::tok::Count) {
        throw std::runtime_error("the core's palette has " + std::to_string(argb.size()) + " colours; the views expect " +
                                 std::to_string(gfx::tok::Count));
    }
    window_.root().resources().palette.set_colors(argb);
    auto rgb = [&](gfx::Token t) {
        const uint32_t v = argb[t];
        return RGB((v >> 16) & 0xFF, (v >> 8) & 0xFF, v & 0xFF);
    };
    const uint32_t bg = argb[gfx::tok::WindowBackground];
    const int luma = (((bg >> 16) & 0xFF) * 299 + ((bg >> 8) & 0xFF) * 587 + (bg & 0xFF) * 114) / 1000;
    window_.set_frame_colors(luma < 128, rgb(gfx::tok::TitleBarBorder), rgb(gfx::tok::TitleBarBackground),
                             rgb(gfx::tok::TitleBarForeground));
    window_.root().invalidate_all();
}

TabView* MainView::tab_view(uint64_t tab) {
    const auto it = by_tab_.find(tab);
    return it == by_tab_.end() ? nullptr : it->second;
}

size_t MainView::tab_count_in_active_pane() const {
    for (const PaneView* pv : pane_views_) {
        if (pv->pane() == active_pane_) return pv->strip->tabs().size();
    }
    return 0;
}

std::vector<std::wstring> MainView::tab_titles_in_active_pane() const {
    std::vector<std::wstring> out;
    for (const PaneView* pv : pane_views_) {
        if (pv->pane() != active_pane_) continue;
        for (const TabEntry& t : pv->strip->tabs()) out.push_back(t.title);
    }
    return out;
}

std::vector<uint64_t> MainView::tab_ids_in_active_pane() const {
    std::vector<uint64_t> out;
    for (const PaneView* pv : pane_views_) {
        if (pv->pane() != active_pane_) continue;
        for (const TabEntry& t : pv->strip->tabs()) out.push_back(t.id);
    }
    return out;
}

std::vector<uint64_t> MainView::pane_ids() const {
    std::vector<uint64_t> out;
    for (const PaneView* pv : pane_views_) out.push_back(pv->pane());
    return out;
}

Widget* MainView::find_named(const std::string& name) {
    if (TabView* tv = active_view()) {
        if (tv->name == name) return tv;
        if (Widget* w = tv->find(name)) return w;
    }
    return window_.root().find(name);
}

void MainView::tab_navigated(uint64_t tab, const std::wstring& path) {
    if (tab == active_tab_) reveal_in_tree(path);
}

void MainView::reveal_in_tree(const std::wstring& path) {
    if (path == revealed_ || path.empty()) return;
    revealed_ = path;
    const std::vector<uint64_t> chain = tree_provider_->chain_to(path);
    if (chain.empty()) return;
    revealing_ = true;
    for (size_t i = 0; i + 1 < chain.size(); ++i) tree_->expand(chain[i]);
    tree_->select(chain.back());
    revealing_ = false;
}

bool MainView::run_command(const std::wstring& id) {
    return bb_command_run(core_.app(), core::s(id)) == BB_OK;
}

void MainView::activate_tab(uint64_t tab) { bb_tab_activate(core_.app(), tab); }
void MainView::close_tab(uint64_t tab) { bb_tab_close(core_.app(), tab); }
void MainView::activate_pane(uint64_t pane) { bb_pane_activate(core_.app(), pane); }

bool MainView::on_preview_key(const KeyEvent& e) {
    const std::wstring key = key_name(e.vk);
    if (key.empty()) return false;
    const uint32_t mods = (e.mods.ctrl ? 1u : 0u) | (e.mods.alt ? 2u : 0u) | (e.mods.shift ? 4u : 0u);
    BbFocusKind kind = BB_FOCUS_OTHER;
    Widget* f = window_.root().focused();
    if (const auto* edit = dynamic_cast<Edit*>(f)) {
        kind = edit->read_only ? BB_FOCUS_TEXT_READ_ONLY : BB_FOCUS_TEXT_EDITABLE;
    } else if (!f || dynamic_cast<ListView*>(f)) {
        kind = BB_FOCUS_FILE_LIST;
    }
    const std::wstring command = core_.dispatch(mods, key, kind, 0);
    if (command.empty()) return false;
    // A claimed chord is the command's even when it cannot run now.
    run_command(command);
    return true;
}

void MainView::refresh_status() {
    if (!active_tab_) return;
    status_->set_text(core_.tab_text(active_tab_, BB_TEXT_STATUS), L"");
}

void MainView::sync_layout() {
    const json::Value st = json::parse(core_.state_json());
    active_tab_ = static_cast<uint64_t>(st["activeTab"].i64());
    active_pane_ = static_cast<uint64_t>(st["activePane"].i64());
    const std::string sig = signature(st["layout"], st["panes"]) + "@" + std::to_string(active_pane_) + "/" +
                            std::to_string(active_tab_);
    if (sig != layout_sig_) {
        layout_sig_ = sig;
        rebuild_layout(st);
    }
    // Titles move with navigation, so the strips are refreshed whether or not the shape changed.
    for (PaneView* pv : pane_views_) {
        const json::Value& p = pane_state(st["panes"], pv->pane());
        std::vector<TabEntry> entries;
        for (size_t i = 0; i < p["tabs"].size(); ++i) {
            const uint64_t id = static_cast<uint64_t>(p["tabs"][i].i64());
            entries.push_back({id, core_.tab_text(id, BB_TEXT_TITLE), id == static_cast<uint64_t>(p["activeTab"].i64(-1))});
        }
        pv->strip->set_tabs(std::move(entries), pv->pane() == active_pane_);
    }
    if (active_tab_) {
        reveal_in_tree(core_.tab_text(active_tab_, BB_TEXT_PATH));
    }
    refresh_status();
}

void MainView::rebuild_layout(const json::Value& st) {
    // Tab views outlive the layout: park them, drop the old shape, build the new one around them.
    for (PaneView* pv : pane_views_) {
        if (pv->content) {
            const uint64_t tab = pv->content->tab();
            parked_[tab] = std::unique_ptr<TabView>(static_cast<TabView*>(pv->remove_child(pv->content).release()));
            pv->content = nullptr;
        }
    }
    pane_views_.clear();
    if (layout_root_) {
        pane_host_->remove_child(layout_root_);
        layout_root_ = nullptr;
    }
    // A tab the core no longer has goes away with its view.
    std::vector<uint64_t> alive;
    for (size_t i = 0; i < st["tabs"].size(); ++i) alive.push_back(static_cast<uint64_t>(st["tabs"][i]["id"].i64()));
    for (auto it = parked_.begin(); it != parked_.end();) {
        if (std::find(alive.begin(), alive.end(), it->first) == alive.end()) {
            by_tab_.erase(it->first);
            it = parked_.erase(it);
        } else {
            ++it;
        }
    }
    std::unique_ptr<Widget> built = build_node(st["layout"], st["panes"]);
    if (built) layout_root_ = pane_host_->add(std::move(built));
    pane_host_->request_layout();
}

std::unique_ptr<Widget> MainView::build_node(const json::Value& node, const json::Value& panes) {
    if (node["kind"].str() == "pane") {
        const uint64_t id = static_cast<uint64_t>(node["pane"].i64());
        auto pv = std::make_unique<PaneView>(*this, id);
        pv->name = "Pane" + std::to_string(id);
        pv->strip = pv->emplace<TabStrip>();
        pv->strip->name = "TabHost";
        pv->strip->on_activate = [this](uint64_t t) { activate_tab(t); };
        pv->strip->on_close = [this](uint64_t t) { close_tab(t); };
        const json::Value& p = pane_state(panes, id);
        if (!p["activeTab"].is_null()) {
            const uint64_t tab = static_cast<uint64_t>(p["activeTab"].i64());
            std::unique_ptr<TabView> view;
            if (auto it = parked_.find(tab); it != parked_.end()) {
                view = std::move(it->second);
                parked_.erase(it);
            } else {
                view = std::make_unique<TabView>(*this, core_, tab);
                by_tab_[tab] = view.get();
                view->reload_listing();
                view->reload_state();
            }
            view->flex = 1;
            pv->content = pv->add(std::move(view));
        }
        pane_views_.push_back(pv.get());
        return pv;
    }
    const json::Value& kids = node["children"];
    if (kids.size() == 0) return nullptr;
    const Orientation o = node["orientation"].str() == "vertical" ? Orientation::Horizontal : Orientation::Vertical;
    // Right-nested: [a, b, c] is a | (b | c), each first child weighted against the rest.
    std::function<std::unique_ptr<Widget>(size_t)> chain = [&](size_t i) -> std::unique_ptr<Widget> {
        std::unique_ptr<Widget> first = build_node(kids[i], panes);
        if (i + 1 >= kids.size()) return first;
        float rest = 0;
        for (size_t k = i + 1; k < kids.size(); ++k) rest += static_cast<float>(kids[k]["weight"].num(1));
        const float mine = static_cast<float>(kids[i]["weight"].num(1));
        auto split = std::make_unique<WeightedSplitter>(o, mine / std::max(0.0001f, mine + rest));
        split->add_child(std::move(first));
        split->add_child(chain(i + 1));
        return split;
    };
    return chain(0);
}

}  // namespace bb::views
