// The widgets the tool windows share: a table of text cells with a header (the change timeline, the sync
// preview, the duplicate finder, the disk-usage list), the two-sided line diff of the file compare, and
// the treemap. Each paints what it is given; none knows about the core.
#pragma once
#include "ui/scroll.h"
#include "ui/widget.h"

#include <functional>
#include <string>
#include <vector>

namespace bb::views::tools {

using namespace bb::ui;

struct TableColumn {
    std::wstring title;
    float width = 100.0f;  // fixed, unless flex
    bool flex = false;     // shares what the fixed columns leave
    bool right = false;    // right-aligned
    bool mono = false;
};

struct TableRow {
    std::vector<std::wstring> cells;
    bool muted = false;
    bool heading = false;
    Token fg = gfx::kNoToken;  // overrides the text colour of the first cell
    Token tint = gfx::kNoToken;  // a faint fill behind the row
    float bar = -1.0f;         // 0..1 draws a thin bar under the row; negative draws none (an unknown share)
    bool bar_slot = false;     // reserve the bar's strip even when there is none to draw
};

// Rows of text cells under a header; one selected; a scroll bar when it overflows.
class TableList : public Widget {
public:
    TableList() {
        flags.focusable = true;
        flags.tab_stop = true;
        style.bg = tok::InputBackground;
        style.border = tok::BorderSubtle;
    }
    std::vector<TableColumn> columns;
    std::vector<TableRow> rows;
    bool show_header = true;
    float row_height = 24.0f;
    int selected = -1;
    std::wstring empty_text;  // drawn in the body when there are no rows
    std::function<void(int)> on_select;
    std::function<void(int)> on_activate;
    // A row with a tick box in its first column: a click there, or Space, toggles it rather than activating
    // the row, which a double-click still does.
    std::function<void(int)> on_toggle;

    void set_rows(std::vector<TableRow> r);
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
    float header_height() const { return show_header ? 24.0f : 0.0f; }
    float body_top() const { return bounds.t + header_height(); }
    float row_step(const TableRow& r) const { return r.bar_slot || r.bar >= 0 ? row_height + 6.0f : row_height; }
    void layout_bar();
    int row_at(Point p) const;
    std::vector<float> column_x(float total) const;
    ScrollBar v_{true};
};

// One row of the diff: the line number and text on each side, and what happened between them.
struct DiffLine {
    int op = 0;  // 0 equal, 1 delete, 2 insert, 3 replace
    std::wstring ln, lt, rn, rt;
};

// Two columns of numbered lines, changed ones tinted, scrolling together.
class DiffView : public Widget {
public:
    DiffView() {
        style.bg = tok::InputBackground;
        style.border = tok::BorderSubtle;
        flags.focusable = true;
    }
    void set_lines(std::vector<DiffLine> lines);
    void scroll_to_row(int row);
    int first_visible_row() const { return static_cast<int>(v_.offset() / kRow); }
    Size measure(Size) override { return fixed_or({400.0f, 200.0f}); }
    void arrange(Rect r) override;
    void paint(PaintContext& pc) override;
    bool on_mouse_down(const MouseEvent& e) override;
    bool on_mouse_move(const MouseEvent& e) override;
    bool on_mouse_up(const MouseEvent& e) override;
    bool on_wheel(Point p, int delta, Mods m) override;
    static constexpr float kRow = 19.0f;

private:
    void layout_bar();
    std::vector<DiffLine> lines_;
    ScrollBar v_{true};
};

struct TreemapTile {
    std::wstring name;
    std::wstring size_text;
    double weight = 0;
    bool folded = false;  // the "N smaller items" tile: not drillable
};

struct TreemapCell {
    size_t index = 0;
    float x = 0, y = 0, w = 0, h = 0;
};

// The squarified treemap of a folder's measured children. The layout itself is the core's
// (TreemapLayout); this lays the cells out whenever its size changes and paints them.
class TreemapCanvas : public Widget {
public:
    // weights, width, height -> cells
    std::function<std::vector<TreemapCell>(const std::vector<double>&, float, float)> layouter;
    std::vector<uint32_t> fills;  // 0xAARRGGBB, rotated by index
    std::function<void(size_t)> on_tile;  // a click on a tile (index into tiles())

    void set_tiles(std::vector<TreemapTile> tiles);
    const std::vector<TreemapTile>& tiles() const { return tiles_; }
    const std::vector<TreemapCell>& cells() const { return cells_; }
    Size measure(Size) override { return fixed_or({200.0f, 120.0f}); }
    void arrange(Rect r) override;
    void paint(PaintContext& pc) override;
    bool on_mouse_down(const MouseEvent& e) override;
    bool on_mouse_move(const MouseEvent& e) override;
    void on_mouse_leave() override;

private:
    int tile_at(Point p) const;
    void relayout();
    std::vector<TreemapTile> tiles_;
    std::vector<TreemapCell> cells_;
    int hovered_ = -1;
};

}  // namespace bb::views::tools
