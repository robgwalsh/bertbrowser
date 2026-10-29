// The tab strip: tabs with a title, optional icon and a close button, a "+" button, drag to reorder.
// The strip owns only the presentation: closing asks (on_close) and the owner removes the tab.
#pragma once
#include "ui/controls.h"

namespace bb::ui {

struct Tab {
    std::wstring title;
    uint64_t id = 0;  // the owner's key; survives reordering
    const IconData* icon = nullptr;
    bool closable = true;
};

class TabStrip : public Widget {
public:
    static constexpr float kHeight = 34.0f;
    static constexpr float kMinTab = 80.0f, kMaxTab = 220.0f;
    static constexpr float kCloseSize = 18.0f;
    static constexpr float kNewWidth = 34.0f;
    static constexpr float kDragThreshold = 4.0f;

    TabStrip() {
        flags.focusable = true;
        flags.tab_stop = true;
    }

    bool show_new_button = true;

    int add(Tab t, bool select = true);
    void remove(int index);        // selection moves to a neighbour when the selected tab goes
    void move(int from, int to);   // reorder in place; the selection follows its tab
    void select(int index);        // no callback
    void set_title(int index, std::wstring title);
    int count() const { return static_cast<int>(tabs_.size()); }
    const Tab& tab(int i) const { return tabs_[static_cast<size_t>(i)]; }
    int selected() const { return selected_; }
    int index_of_id(uint64_t id) const;

    std::function<void(int)> on_select;    // a click or a key chose a tab
    std::function<void(int)> on_close;     // close button or middle click
    std::function<void(int, int)> on_reorder;  // a drag finished: from, to
    std::function<void()> on_new;

    // Geometry. Tabs share the strip: each is as wide as it can be up to kMaxTab, shrinking to kMinTab and
    // then running off the right edge (clipped).
    float tab_width() const;
    Rect tab_rect(int i) const;
    Rect close_rect(int i) const;
    Rect new_rect() const;
    int tab_at(Point p) const;  // -1 when none
    bool reordering() const { return reordering_; }

    Size measure(Size avail) override;
    void paint(PaintContext& pc) override;
    bool on_mouse_down(const MouseEvent& e) override;
    bool on_mouse_move(const MouseEvent& e) override;
    bool on_mouse_up(const MouseEvent& e) override;
    void on_mouse_leave() override;
    bool on_key_down(const KeyEvent& e) override;
    void on_focus(bool) override { invalidate(); }

private:
    std::vector<Tab> tabs_;
    std::vector<ComPtr<ID2D1PathGeometry>> icon_geo_;
    std::vector<uint32_t> icon_gen_;
    int selected_ = -1;
    int hover_tab_ = -1;
    bool hover_close_ = false, hover_new_ = false;
    int press_tab_ = -1;
    bool press_close_ = false, press_new_ = false;
    bool reordering_ = false;
    int drag_from_ = -1;
    float press_x_ = 0;
};

}  // namespace bb::ui
