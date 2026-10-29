// The virtualized tree (folder tree, settings pages). The provider describes the whole tree by node id
// (0 is the invisible root); the widget keeps the flattened list of expanded nodes and asks for labels
// of the visible rows only.
#pragma once
#include "ui/controls.h"
#include "ui/scroll.h"

#include <unordered_set>

namespace bb::ui {

class TreeProvider {
public:
    virtual ~TreeProvider() = default;
    virtual size_t child_count(uint64_t node) const = 0;
    virtual uint64_t child_id(uint64_t node, size_t index) const = 0;  // never 0
    virtual std::wstring label(uint64_t node) const = 0;
    // Lazy trees answer "maybe" without enumerating. Default: ask for the count.
    virtual bool has_children(uint64_t node) const { return child_count(node) > 0; }
    // A picture before the label, `px` pixels square, or null for none (the settings pages have none).
    virtual ThumbPtr icon(uint64_t /*node*/, uint32_t /*px*/) const { return nullptr; }
    // Draws the icon faded (a hidden folder, as Explorer does).
    virtual bool dimmed(uint64_t /*node*/) const { return false; }
};

class TreeView : public Widget {
public:
    static constexpr size_t npos = static_cast<size_t>(-1);

    TreeView();
    float row_height = 22.0f;
    float indent = 16.0f;

    std::function<void(uint64_t id)> on_select;
    std::function<void(uint64_t id, bool expanded)> on_toggle;  // load children here, then refresh()
    std::function<void(uint64_t id)> on_activate;
    // A right click on a row. It does not select the row: selecting is navigating, and a menu is not.
    std::function<void(uint64_t id, Point at)> on_context;

    void set_provider(TreeProvider* p);
    void refresh();  // the provider's tree changed: re-flatten, keep expansion and selection

    bool is_expanded(uint64_t id) const { return expanded_.count(id) != 0; }
    void expand(uint64_t id);
    void collapse(uint64_t id);
    void toggle(uint64_t id);
    uint64_t selected_id() const { return selected_; }
    void select(uint64_t id);  // selects; scrolls to it if its row is visible in the flattened list

    size_t visible_count() const { return rows_.size(); }  // rows in the flattened list, not on screen
    std::pair<size_t, size_t> visible_range() const;       // [first, last) on screen
    uint64_t id_at_row(size_t row) const { return row < rows_.size() ? rows_[row].id : 0; }
    size_t row_of(uint64_t id) const;
    Rect row_rect(size_t row) const;
    size_t row_at(Point p) const;
    size_t painted_rows() const { return painted_rows_; }
    float scroll_y() const { return v_.offset(); }
    void set_scroll_y(float y);

    Size measure(Size avail) override;
    void arrange(Rect r) override;
    void paint(PaintContext& pc) override;
    bool on_mouse_down(const MouseEvent& e) override;
    bool on_mouse_move(const MouseEvent& e) override;
    bool on_mouse_up(const MouseEvent& e) override;
    bool on_wheel(Point p, int delta, Mods m) override;
    bool on_key_down(const KeyEvent& e) override;
    void on_focus(bool) override { invalidate(); }

private:
    struct Entry {
        uint64_t id;
        int depth;
        size_t parent;  // row of the parent, npos at the top
        bool has_children;
    };
    void rebuild();
    void layout_bar();
    Rect chevron_rect(size_t row) const;
    void select_row(size_t row);

    TreeProvider* provider_ = nullptr;
    std::vector<Entry> rows_;
    std::unordered_set<uint64_t> expanded_;
    uint64_t selected_ = 0;
    ScrollBar v_;
    size_t painted_rows_ = 0;
    bool bar_drag_ = false;
    ImageCache images_;
};

}  // namespace bb::ui
