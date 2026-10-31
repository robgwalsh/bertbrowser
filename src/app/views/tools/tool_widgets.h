// The widgets the tool windows share: a table of text cells with a header (the change timeline, the sync
// preview, the duplicate finder, the disk-usage list), the two-sided line diff of the file compare, and
// the treemap. Each paints what it is given; none knows about the core.
#pragma once
#include "ui/controls/controls.h"
#include "ui/core/scroll.h"
#include "ui/core/widget.h"

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
    std::wstring tip;          // the tooltip while the pointer rests on the row
    // The file's icon, drawn at the start of the list's `picture_column`; asked for at paint time because it
    // may still be loading.
    std::function<ThumbPtr(uint32_t px)> picture;
    bool faded = false;  // the picture dimmed (a hidden file)
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
    // Ctrl/Shift-click and Ctrl+A pick more than one row; `picked()` is every one of them, in order.
    bool multi = false;
    int picture_column = -1;  // which column's cells start with the row's picture
    std::vector<int> picked() const;
    std::function<void()> on_copy;  // Ctrl+C
    std::function<bool(unsigned vk)> on_key;  // a plain key the list's own handling has not taken yet; true = handled

    void set_rows(std::vector<TableRow> r);
    float scroll_offset() const { return v_.offset(); }
    void preset_offset(float y) { v_.preset(y); }
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
    void select_only(int i);
    void bring_into_view(int i);
    int row_at(Point p) const;
    std::vector<char> extra_;  // rows picked besides `selected` (multi)
    ImageCache images_;
    int anchor_ = -1;          // where a Shift range starts
    std::vector<float> column_x(float total) const;
    ScrollBar v_{true};
};

// Where each list and scroller of a tree stands, in tree order: a rebuild of the same window or card lays
// the same kinds out in the same order, so the n-th list takes the n-th list's place (and its selection,
// where the rebuild chose none).
struct ListPlaces {
    std::vector<std::pair<float, int>> tables;  // offset, selected
    std::vector<std::pair<float, float>> scrollers;
};
ListPlaces save_places(Widget& tree);
void restore_places(Widget& tree, const ListPlaces& places);

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
    size_t longest_ = 0;  // characters in the longest line of either side; the font is monospaced
    ScrollBar v_{true};
    // Both sides scroll sideways together, so a long line can be read across. Spec: FileCompareWindow.xaml
    // (the diff's horizontal ScrollViewer).
    ScrollBar h_{false};
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
