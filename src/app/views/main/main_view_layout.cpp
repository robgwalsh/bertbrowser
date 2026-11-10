#include "views/tools/tool_dialogs.h"
#include "views/tools/tool_state.h"
#include "views/main/main_view.h"

#include "gfx/icons.gen.h"
#include "views/main/folder_tree.h"
#include "views/panes/media_pane.h"
#include "views/bridge/ops.h"
#include "views/dialogs/ops_card.h"
#include "views/dialogs/ops_menus.h"
#include "views/panes/property_host.h"
#include "views/shell/shell_props.h"
#include "views/tools/tool_widgets.h"

#include <dwmapi.h>
#include <shobjidl.h>

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

// The strip's buttons and its menu act on the pane they sit in, so that pane is made active first.
void MainView::wire_tab_strip(TabStrip& strip, uint64_t pane) {
    BbApp* app = core_.app();
    strip.close_tip = gesture_tip(app, L"Close tab", L"tab.close");
    strip.on_move = [app](uint64_t tab, uint32_t index) { bb_tab_move(app, tab, index); };
    strip.on_new_tab = [this, pane] {
        activate_pane(pane);
        execute_command(L"tab.new");
    };
    strip.on_split = [this, pane](bool right) {
        activate_pane(pane);
        execute_command(right ? L"pane.split-right" : L"pane.split-below");
    };
    strip.on_context = [this](uint64_t tab, Point at) { show_context_menu(tab_menu(tab), at); };
    auto tip = [app](const wchar_t* text, const wchar_t* command) {
        BbOwnedStr o{};
        const std::wstring g = bb_keymap_gesture(app, core::s(command), &o) == BB_OK ? core::take(o) : std::wstring();
        return g.empty() ? std::wstring(text) : std::wstring(text) + L" (" + g + L")";
    };
    strip.new_tab_button()->tooltip = tip(L"New tab here", L"tab.new");
    strip.split_right_button()->tooltip = tip(L"Split pane to the right", L"pane.split-right");
    strip.split_below_button()->tooltip = tip(L"Split pane below", L"pane.split-below");
}

MenuItems MainView::tab_menu(uint64_t tab) {
    // The commands act on the active tab, so the one right-clicked becomes it.
    activate_tab(tab);
    std::vector<BarRow> rows;
    const std::pair<const char*, const wchar_t*> items[] = {{"tab.duplicate", L"&Duplicate tab"},
                                                            {"tab.close-others", L"Close &other tabs"},
                                                            {"tab.move-to-new-pane", L"&Move to new pane"},
                                                            {"-", nullptr},
                                                            {"tab.close", L"&Close tab"}};
    for (const auto& [id, caption] : items) {
        BarRow r;
        if (std::string(id) == "-") {
            r.kind = "separator";
            rows.push_back(std::move(r));
            continue;
        }
        const json::Value st = try_model([&](BbOwnedStr* o) { return bb_command_state(core_.app(), core::s(core::wide(id)), o); });
        if (st.is_null()) continue;
        r.kind = "command";
        r.id = id;
        r.icon = st["icon"].str();
        r.name = core::wide(st["name"].str());
        r.caption = caption;
        r.gesture = core::wide(st["gesture"].str());
        r.enabled = st["whyNot"].is_null();
        r.reason = core::wide(st["whyNot"].str());
        rows.push_back(std::move(r));
    }
    return build_menu_items(menu_env(), rows);
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
            entries.push_back({id, core_.tab_text(id, BB_TEXT_TITLE), id == static_cast<uint64_t>(p["activeTab"].i64(-1)),
                               core_.tab_text(id, BB_TEXT_PATH)});
        }
        pv->strip->set_tabs(std::move(entries), pv->pane() == active_pane_);
    }
    if (active_tab_) {
        reveal_in_tree(core_.tab_text(active_tab_, BB_TEXT_PATH));
    }
    sync_title_search();
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
    std::unique_ptr<Widget> built = build_node(st["layout"], st["panes"], {});
    if (built) layout_root_ = pane_host_->add(std::move(built));
    pane_host_->request_layout();
}

std::unique_ptr<Widget> MainView::build_node(const json::Value& node, const json::Value& panes, const std::vector<uint32_t>& path) {
    if (node["kind"].str() == "pane") {
        const uint64_t id = static_cast<uint64_t>(node["pane"].i64());
        auto pv = std::make_unique<PaneView>(*this, id);
        pv->name = "Pane" + std::to_string(id);
        pv->strip = pv->emplace<TabStrip>();
        pv->strip->name = "TabHost";
        pv->strip->on_activate = [this](uint64_t t) { activate_tab(t); };
        pv->strip->on_close = [this](uint64_t t) { close_tab(t); };
        wire_tab_strip(*pv->strip, id);
        const json::Value& p = pane_state(panes, id);
        if (!p["activeTab"].is_null()) {
            const uint64_t tab = static_cast<uint64_t>(p["activeTab"].i64());
            std::unique_ptr<TabView> view;
            if (auto it = parked_.find(tab); it != parked_.end()) {
                view = std::move(it->second);
                parked_.erase(it);
            } else {
                view = std::make_unique<TabView>(*this, core_, tab);
                view->set_thumbnail_source(thumb_source_);
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
    // Right-nested: [a, b, c] is a | (b | c), each first child weighted against the rest. A drag of any bar in
    // the chain writes every child's size back as its weight. Spec: PaneLayoutHost (MinPaneSize 160,
    // WriteBackWeights).
    constexpr float kMinPane = 160.0f;
    auto bars = std::make_shared<std::vector<Splitter*>>();
    auto write_back = [this, bars, path, o] {
        if (bars->empty()) return;
        std::vector<double> sizes;
        for (Splitter* s : *bars) sizes.push_back(s->position());
        const Splitter* last = bars->back();
        const float extent = o == Orientation::Horizontal ? last->bounds.width() : last->bounds.height();
        sizes.push_back(std::max(1.0f, extent - last->position() - last->thickness));
        bb_layout_set_weights(core_.app(), path.data(), static_cast<uint32_t>(path.size()), sizes.data(), static_cast<uint32_t>(sizes.size()));
    };
    std::function<std::unique_ptr<Widget>(size_t)> chain = [&](size_t i) -> std::unique_ptr<Widget> {
        std::vector<uint32_t> child_path = path;
        child_path.push_back(static_cast<uint32_t>(i));
        std::unique_ptr<Widget> first = build_node(kids[i], panes, child_path);
        if (i + 1 >= kids.size()) return first;
        float rest = 0;
        for (size_t k = i + 1; k < kids.size(); ++k) rest += static_cast<float>(kids[k]["weight"].num(1));
        const float mine = static_cast<float>(kids[i]["weight"].num(1));
        auto split = std::make_unique<WeightedSplitter>(o, mine / std::max(0.0001f, mine + rest));
        const float after = static_cast<float>(kids.size() - i - 1);
        split->min_first = kMinPane;
        split->min_second = kMinPane * after + split->thickness * (after - 1);
        split->on_released = write_back;
        bars->push_back(split.get());
        split->add_child(std::move(first));
        split->add_child(chain(i + 1));
        return split;
    };
    return chain(0);
}


}  // namespace bb::views
