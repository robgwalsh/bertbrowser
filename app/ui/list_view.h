// The virtualized details grid. Rows come from a RowProvider and only the visible range is ever asked
// for: paint (and everything else) touches rows [first, last) plus nothing. Column headers carry the sort
// indicator and can be resized and reordered; a drag from empty space is a marquee that selects with one
// bulk provider call; typing within 700 ms builds a prefix handed to a hook.
#pragma once
#include "ui/controls.h"
#include "ui/controls.h"
#include "ui/scroll.h"

#include <unordered_map>

#include <unordered_map>

namespace bb::ui {

enum class SelectOp { Replace, Add, Toggle, Clear };

// What the Rust side fills (through a thin adapter): count, ids, text, selection. `select` is one bulk
// call per gesture over the inclusive row range [first, last]; Clear ignores the range.
class RowProvider {
public:
    virtual ~RowProvider() = default;
    virtual size_t row_count() const = 0;
    virtual uint64_t row_id(size_t row) const = 0;
    virtual std::wstring cell_text(size_t row, int column_id) const = 0;
    virtual bool selected(size_t row) const = 0;
    virtual void select(SelectOp op, size_t first, size_t last) = 0;
    // A picture drawn before the first column's text (the file's own icon), `px` pixels square; `faded` draws it
    // at half strength (a hidden file). Null falls back to `row_icon`.
    virtual ThumbPtr row_image(size_t /*row*/, uint32_t /*px*/, bool* /*faded*/) const { return nullptr; }
    // An outline icon drawn there instead, or null for none.
    virtual const IconData* row_icon(size_t /*row*/) const { return nullptr; }
    // A faint fill behind the row (what a folder comparison says about it), or kNoToken for none.
    virtual Token row_tint(size_t /*row*/) const { return kNoToken; }
};

struct Column {
    int id = 0;  // stable: the provider is asked by id, so reordering never confuses it
    std::wstring title;
    float width = 120.0f;
    float min_width = 32.0f;
    bool right_align = false;
    bool sortable = true;
};

class ListView : public Widget {
public:
    static constexpr size_t npos = static_cast<size_t>(-1);
    static constexpr unsigned kTypeAheadMs = 700;

    ListView();

    float row_height = 22.0f;
    float header_height = 26.0f;
    bool show_header = true;

    // Hooks.
    std::function<void(int column_id, bool ascending)> on_sort;
    std::function<void(size_t row)> on_activate;       // Enter, double click
    std::function<void()> on_columns_changed;          // resize or reorder finished
    std::function<void(const std::wstring&)> on_typeahead;  // the prefix typed so far; call select_row
    std::function<void(Point)> on_header_menu;               // a right click on the header strip
    std::function<void(Point, size_t row)> on_context;       // a right click on a row (npos: on empty space)
    // A press on a row moved far enough to be a drag, with the button that started it. The selection change a
    // press on a selected row would make is held back until release, so a multi-selection can be dragged.
    std::function<void(MouseButton)> on_drag_start;
    size_t drop_row = npos;  // the folder row a drag is hovering over (outlined)

    void set_provider(RowProvider* p);
    RowProvider* provider() const { return provider_; }
    void set_columns(std::vector<Column> cols);
    const std::vector<Column>& columns() const { return columns_; }
    void set_sort(int column_id, bool ascending);
    int sort_column() const { return sort_column_; }
    bool sort_ascending() const { return sort_ascending_; }
    void refresh();  // the provider's rows changed: re-measure, clamp, repaint

    size_t cursor() const { return cursor_; }
    void select_row(size_t row);  // cursor + selection + scroll into view
    void scroll_to_row(size_t row);
    void set_scroll_y(float y);
    float scroll_y() const { return v_.offset(); }
    float scroll_x() const { return h_.offset(); }

    // Geometry, window DIPs.
    Rect header_rect() const;
    Rect body_rect() const { return body_; }  // the rows' viewport
    std::pair<size_t, size_t> visible_range() const;  // [first, last)
    Rect row_rect(size_t row) const;
    Rect column_header_rect(size_t index) const;
    size_t row_at(Point p) const;  // npos when not over a row
    float content_width() const;
    size_t painted_rows() const { return painted_rows_; }  // by the last paint

    Size measure(Size avail) override;
    void arrange(Rect r) override;
    void paint(PaintContext& pc) override;
    bool on_mouse_down(const MouseEvent& e) override;
    bool on_mouse_move(const MouseEvent& e) override;
    bool on_mouse_up(const MouseEvent& e) override;
    bool on_wheel(Point p, int delta, Mods m) override;
    bool on_key_down(const KeyEvent& e) override;
    bool on_char(wchar_t c) override;
    void on_focus(bool) override { invalidate(); }
    Cursor cursor_at(Point p) const override;

private:
    void layout_bars();
    int resize_zone(Point p) const;  // column index whose right edge is under p, or -1
    int header_index_at(float x) const;
    size_t clamp_row(long long r) const;
    void move_cursor(size_t row, const Mods& m);
    void update_marquee(Point p);
    void paint_header(PaintContext& pc);
    size_t count() const { return provider_ ? provider_->row_count() : 0; }

    RowProvider* provider_ = nullptr;
    std::vector<Column> columns_;
    ScrollBar v_, h_;
    Rect body_;
    int sort_column_ = -1;
    bool sort_ascending_ = true;
    size_t cursor_ = npos, anchor_ = npos;
    size_t painted_rows_ = 0;

    // Gesture state.
    bool bar_drag_ = false;
    int resize_col_ = -1;
    float resize_x_ = 0, resize_w_ = 0;
    int press_col_ = -1;
    float press_x_ = 0;
    bool reordering_ = false;
    int drop_index_ = -1;
    bool marquee_ = false, marquee_add_ = false;
    // A press on a row that may become a drag.
    bool drag_armed_ = false;
    Point drag_origin_;
    MouseButton drag_button_ = MouseButton::Left;
    size_t deferred_row_ = npos;
    bool context_pending_ = false;
    Point context_point_;
    size_t context_row_ = npos;
    Point marquee_origin_;  // in content coordinates
    Rect marquee_rect_;     // window DIPs, for painting
    size_t marquee_first_ = npos, marquee_last_ = npos;
    std::wstring typed_;
    uint64_t typed_at_ = 0;
    struct IconCache {
        ComPtr<ID2D1PathGeometry> geometry;
        uint32_t generation = 0;
    };
    std::unordered_map<const IconData*, IconCache> icon_cache_;
    ImageCache images_;
};

}  // namespace bb::ui
