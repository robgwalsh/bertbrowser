#include "fixture.h"

#include <algorithm>
#include <set>

using namespace bb;
using namespace bb::ui;

namespace {

// A details-grid provider over `n` generated rows. Records which rows the widget asked about, so a test
// can assert that painting touches the visible range and nothing else.
class GridProvider : public RowProvider {
public:
    explicit GridProvider(size_t n) : n_(n) {}
    size_t row_count() const override { return n_; }
    uint64_t row_id(size_t row) const override {
        touched.insert(row);
        return 1000 + row;
    }
    std::wstring cell_text(size_t row, int col) const override {
        touched.insert(row);
        wchar_t buf[64];
        switch (col) {
            case 0: swprintf_s(buf, L"holiday_photo_%05zu.jpg", row); break;
            case 1: swprintf_s(buf, L"%zu KB", (row * 37) % 9000 + 12); break;
            case 2: swprintf_s(buf, L"JPEG image"); break;
            default: swprintf_s(buf, L"2026-10-%02zu 14:%02zu", row % 28 + 1, row % 60); break;
        }
        return buf;
    }
    bool selected(size_t row) const override {
        touched.insert(row);
        return std::binary_search(sel_.begin(), sel_.end(), row) || chosen_.count(row) != 0;
    }
    void select(SelectOp op, size_t first, size_t last) override {
        ++select_calls;
        ops.push_back({op, first, last});
        switch (op) {
            case SelectOp::Clear: chosen_.clear(); break;
            case SelectOp::Replace:
                chosen_.clear();
                for (size_t r = first; r <= last; ++r) chosen_.insert(r);  // test provider; a real one keeps ranges
                break;
            case SelectOp::Add:
                for (size_t r = first; r <= last; ++r) chosen_.insert(r);
                break;
            case SelectOp::Toggle:
                for (size_t r = first; r <= last; ++r) {
                    if (!chosen_.erase(r)) chosen_.insert(r);
                }
                break;
        }
    }
    bool is_selected(size_t r) const { return chosen_.count(r) != 0; }
    size_t selected_count() const { return chosen_.size(); }

    struct Op {
        SelectOp op;
        size_t first, last;
    };
    mutable std::set<size_t> touched;
    int select_calls = 0;
    std::vector<Op> ops;

private:
    size_t n_;
    std::vector<size_t> sel_;
    std::set<size_t> chosen_;
};

std::vector<Column> grid_columns() {
    return {{0, L"Name", 260, 32, false, true},
            {1, L"Size", 90, 32, true, true},
            {2, L"Type", 120, 32, false, true},
            {3, L"Date modified", 160, 32, false, true}};
}

struct GridRig {
    Rig rig{900, 500};
    GridProvider provider{10000};
    ListView* list = nullptr;

    GridRig() {
        auto lv = std::make_unique<ListView>();
        lv->set_columns(grid_columns());
        list = rig.set(std::move(lv));
        list->set_provider(&provider);
        rig.relayout();
    }
    float row_y(size_t r) const { return list->row_rect(r).t + list->row_height / 2; }
};

}  // namespace

TEST(grid_geometry_and_visible_range) {
    GridRig g;
    // 500 high, a 26 header, no horizontal bar (630 < 888): 474 for rows = 21.5 rows of 22.
    CHECK_NEAR(g.list->body_rect().height(), 474.0, 0.01);
    const auto [first, last] = g.list->visible_range();
    CHECK_EQ(first, 0);
    CHECK_EQ(last, 22);
    CHECK_NEAR(g.list->body_rect().width(), 900.0 - ScrollBar::kThickness, 0.01);  // vertical bar present
    CHECK_NEAR(g.list->content_width(), 630.0, 0.01);
    CHECK_EQ(g.list->row_at({50, g.list->body_rect().t + 5}), 0);
    CHECK_EQ(g.list->row_at({50, g.list->body_rect().t + 22 * 3 + 1}), 3);
    CHECK(g.list->row_at({700, g.list->body_rect().t + 5}) == ListView::npos);  // right of the columns
    CHECK(g.list->row_at({50, 5}) == ListView::npos);                          // header
}

TEST(grid_paint_touches_only_visible_rows) {
    GridRig g;
    g.provider.touched.clear();
    g.rig.root.invalidate_all();
    g.rig.render();
    const auto [first, last] = g.list->visible_range();
    const size_t visible = last - first;
    CHECK_EQ(visible, 22);
    CHECK(g.provider.touched.size() <= visible + 2);  // the contract: at most visible + 2 rows asked for
    CHECK_EQ(g.list->painted_rows(), visible);
    CHECK(*g.provider.touched.rbegin() < 23);

    // Scrolled deep into the list, it is still only the rows on screen.
    g.list->set_scroll_y(22.0f * 5000);
    g.provider.touched.clear();
    g.rig.render();
    const auto [f2, l2] = g.list->visible_range();
    CHECK_EQ(f2, 5000);
    CHECK_EQ(l2, 5022);
    CHECK(g.provider.touched.size() <= 24);
    CHECK(*g.provider.touched.begin() >= 5000);
    CHECK(*g.provider.touched.rbegin() <= 5023);

    // A dirty rect over two rows repaints two or three rows, not the page.
    g.provider.touched.clear();
    const Rect r = g.list->row_rect(5010);
    g.rig.root.invalidate(Rect{r.l, r.t + 2, r.l + 100, r.b + 2});
    g.rig.render();
    CHECK(g.provider.touched.size() <= 3);
    CHECK(!g.rig.bitmap().is_flat());
}

TEST(grid_layout_cache_is_used_on_repaint) {
    GridRig g;
    g.rig.render();
    auto& lru = Env::get().res->layouts;
    const size_t misses = lru.misses();
    g.rig.root.invalidate_all();
    g.rig.render();
    CHECK_EQ(lru.misses(), misses);  // the same cells hit the (row_id, column, width) cache
}

TEST(grid_wheel_and_scroll_bar) {
    GridRig g;
    g.rig.root.wheel({200, 200}, -120);
    CHECK_NEAR(g.list->scroll_y(), 66.0, 0.01);  // one notch = 3 rows
    g.rig.root.wheel({200, 200}, 120);
    g.rig.root.wheel({200, 200}, 120);
    CHECK_NEAR(g.list->scroll_y(), 0.0, 0.01);
    // Drag the thumb a third of the way down the track.
    const float bar_x = 900 - ScrollBar::kThickness / 2;
    const float top = g.list->body_rect().t;
    g.rig.down({bar_x, top + 10});
    g.rig.move({bar_x, top + 10 + 100});
    g.rig.up({bar_x, top + 110});
    CHECK(g.list->scroll_y() > 22.0f * 1000);  // 100 px of ~450 px of track: thousands of rows
    const float max = 22.0f * 10000 - 474.0f;
    g.rig.root.wheel({200, 200}, -120 * 100000);
    CHECK_NEAR(g.list->scroll_y(), max, 0.5);  // clamped at the end
    const auto [first, last] = g.list->visible_range();
    CHECK_EQ(last, 10000);
    CHECK(first > 9970);
}

TEST(grid_click_ctrl_shift_selection) {
    GridRig g;
    const float x = 60;
    g.rig.click({x, g.row_y(3)});
    CHECK(g.provider.is_selected(3));
    CHECK_EQ(g.provider.selected_count(), 1);
    CHECK(g.provider.ops.back().op == SelectOp::Replace);
    CHECK(g.rig.root.focused() == g.list);
    g.rig.click({x, g.row_y(5)}, {false, true, false});  // ctrl
    CHECK(g.provider.ops.back().op == SelectOp::Toggle);
    CHECK(g.provider.is_selected(3) && g.provider.is_selected(5));
    g.rig.click({x, g.row_y(9)}, {true, false, false});  // shift: from the anchor (5)
    CHECK(g.provider.ops.back().op == SelectOp::Replace);
    CHECK_EQ(g.provider.ops.back().first, 5);
    CHECK_EQ(g.provider.ops.back().last, 9);
    CHECK_EQ(g.provider.selected_count(), 5);
    int activated = -1;
    g.list->on_activate = [&](size_t r) { activated = static_cast<int>(r); };
    g.rig.down({x, g.row_y(7)}, {}, 2);
    g.rig.up({x, g.row_y(7)});
    CHECK_EQ(activated, 7);
}

TEST(grid_keyboard_moves) {
    GridRig g;
    g.rig.root.set_focus(g.list);
    g.rig.key(VK_DOWN);
    CHECK_EQ(g.list->cursor(), 0);
    g.rig.key(VK_DOWN);
    g.rig.key(VK_DOWN);
    CHECK_EQ(g.list->cursor(), 2);
    CHECK(g.provider.is_selected(2) && g.provider.selected_count() == 1);
    g.rig.key(VK_DOWN, true);  // shift extends from the anchor
    CHECK(g.provider.is_selected(2) && g.provider.is_selected(3));
    g.rig.key(VK_UP);
    CHECK_EQ(g.list->cursor(), 2);
    g.rig.key(VK_NEXT);  // page down: one viewport of rows (21 whole rows)
    CHECK_EQ(g.list->cursor(), 2 + 21);
    g.rig.key(VK_END);
    CHECK_EQ(g.list->cursor(), 9999);
    CHECK_EQ(g.list->visible_range().second, 10000);  // scrolled into view
    g.rig.key(VK_HOME);
    CHECK_EQ(g.list->cursor(), 0);
    CHECK_NEAR(g.list->scroll_y(), 0.0, 0.01);
    const int calls = g.provider.select_calls;
    g.rig.key('A', false, true);
    CHECK_EQ(g.provider.select_calls, calls + 1);  // select all is one bulk call, not 10,000
    CHECK(g.provider.ops.back().op == SelectOp::Replace && g.provider.ops.back().last == 9999);
    // Ctrl+arrow moves the cursor and leaves the selection alone.
    g.rig.key(VK_DOWN, false, true);
    CHECK_EQ(g.provider.select_calls, calls + 1);
    CHECK_EQ(g.list->cursor(), 1);
    int activated = -1;
    g.list->on_activate = [&](size_t r) { activated = static_cast<int>(r); };
    g.rig.key(VK_RETURN);
    CHECK_EQ(activated, 1);
}

TEST(grid_marquee_selects_with_one_call_per_range) {
    GridRig g;
    // Empty space: right of the 630 DIP of columns.
    const float x0 = 700;
    g.rig.down({x0, g.list->body_rect().t + 74});
    CHECK(g.provider.ops.back().op == SelectOp::Clear);
    const int calls = g.provider.select_calls;
    g.rig.move({x0 + 30, g.list->body_rect().t + 174});
    g.rig.move({x0 + 40, g.list->body_rect().t + 176});  // same rows: no new call
    g.rig.up({x0 + 40, g.list->body_rect().t + 176});
    // Content y 74 -> row 3; 176 -> row 8. Columns end at 630, so the marquee at x>=700 touches no row:
    CHECK_EQ(g.provider.selected_count(), 0);
    CHECK_EQ(g.provider.select_calls, calls);

    // Start in empty space left of... a marquee that begins right of the columns and sweeps left over them.
    g.rig.down({x0, g.list->body_rect().t + 74});
    g.rig.move({400, g.list->body_rect().t + 174});
    g.rig.move({410, g.list->body_rect().t + 176});
    g.rig.up({410, g.list->body_rect().t + 176});
    CHECK_EQ(g.provider.ops.back().first, 3);
    CHECK_EQ(g.provider.ops.back().last, 8);
    CHECK(g.provider.ops.back().op == SelectOp::Replace);
    CHECK_EQ(g.provider.selected_count(), 6);
    int replaces = 0;
    for (const auto& o : g.provider.ops) {
        if (o.op == SelectOp::Replace && o.first == 3 && o.last == 8) ++replaces;
    }
    CHECK_EQ(replaces, 1);  // one bulk call for the range, though the mouse moved twice
    g.rig.render();         // the marquee is gone after mouse up
}

TEST(grid_column_resize) {
    GridRig g;
    int changed = 0;
    g.list->on_columns_changed = [&] { ++changed; };
    const float edge = g.list->column_header_rect(0).r;  // 260
    CHECK(g.list->cursor_at({edge, 10}) == Cursor::SizeWE);
    CHECK(g.list->cursor_at({edge + 40, 10}) == Cursor::Arrow);
    g.rig.down({edge, 10});
    g.rig.move({edge + 60, 10});
    CHECK_NEAR(g.list->columns()[0].width, 320.0, 0.01);
    CHECK_NEAR(g.list->content_width(), 690.0, 0.01);
    g.rig.up({edge + 60, 10});
    CHECK_EQ(changed, 1);
    CHECK_EQ(g.provider.select_calls, 0);  // not a sort, not a selection
    // Down to nothing stops at the minimum.
    g.rig.down({g.list->column_header_rect(0).r, 10});
    g.rig.move({0, 10});
    g.rig.up({0, 10});
    CHECK_NEAR(g.list->columns()[0].width, 32.0, 0.01);
    // Wider than the view brings a horizontal bar.
    g.rig.down({g.list->column_header_rect(0).r, 10});
    g.rig.move({1200, 10});
    g.rig.up({1200, 10});
    CHECK(g.list->content_width() > 888);
    CHECK(g.list->body_rect().height() < 474.0f);  // the bar took a strip off the bottom
    g.rig.root.wheel({200, 200}, -120, {true, false, false});  // shift+wheel scrolls sideways
    CHECK(g.list->scroll_x() > 0);
}

TEST(grid_column_reorder_and_sort) {
    GridRig g;
    int changed = 0;
    g.list->on_columns_changed = [&] { ++changed; };
    // Drag "Size" (second) over the left half of "Name".
    const Rect size = g.list->column_header_rect(1);
    g.rig.down({(size.l + size.r) / 2, 10});
    g.rig.move({(size.l + size.r) / 2 - 10, 10});  // under the 4 DIP threshold? no: 10 > 4, it is a drag
    g.rig.move({30, 10});
    g.rig.up({30, 10});
    CHECK_EQ(changed, 1);
    CHECK_EQ(g.list->columns()[0].id, 1);
    CHECK_EQ(g.list->columns()[1].id, 0);
    CHECK_EQ(g.list->columns()[2].id, 2);
    int sorted_id = -1;
    bool sorted_asc = false;
    g.list->on_sort = [&](int id, bool asc) {
        sorted_id = id;
        sorted_asc = asc;
    };
    // A plain click on "Type" sorts by it, again reverses.
    const Rect type = g.list->column_header_rect(2);
    g.rig.click({(type.l + type.r) / 2, 10});
    CHECK_EQ(sorted_id, 2);
    CHECK(sorted_asc);
    CHECK_EQ(g.list->sort_column(), 2);
    g.rig.click({(type.l + type.r) / 2, 10});
    CHECK(!sorted_asc);
    CHECK_EQ(changed, 1);  // sorting is not a layout change
    // Cells follow the column id after the reorder.
    g.rig.render();
    CHECK(!g.rig.bitmap().is_flat());
}

TEST(grid_type_ahead_hook_and_timeout) {
    GridRig g;
    g.rig.root.set_focus(g.list);
    std::vector<std::wstring> seen;
    g.list->on_typeahead = [&](const std::wstring& s) { seen.push_back(s); };
    g.rig.type(L"h");
    g.rig.now += 300;
    g.rig.type(L"o");
    g.rig.now += 701;  // past 700 ms: the buffer starts over
    g.rig.type(L"x");
    g.rig.now += 100;
    g.rig.type(L" ");
    CHECK_EQ(seen.size(), 4);
    CHECK_TEXT(seen[0], L"h");
    CHECK_TEXT(seen[1], L"ho");
    CHECK_TEXT(seen[2], L"x");
    CHECK_TEXT(seen[3], L"x ");
    g.list->select_row(4321);  // what a hook calls to jump
    CHECK_EQ(g.list->cursor(), 4321);
    CHECK(g.provider.is_selected(4321));
    CHECK(g.list->visible_range().first <= 4321 && g.list->visible_range().second > 4321);
}

TEST(grid_png_of_ten_thousand_rows) {
    GridRig g;
    g.list->set_sort(0, true);
    g.list->set_scroll_y(22.0f * 1200);
    g.rig.click({60, g.row_y(1203)});
    g.rig.click({60, g.row_y(1205)}, {true, false, false});
    g.rig.click({60, g.row_y(1209)}, {false, true, false});
    g.rig.root.set_focus(g.list);
    g.provider.touched.clear();
    g.rig.root.invalidate_all();
    g.rig.render();
    CHECK(g.provider.touched.size() <= 22 + 2);
    CHECK(g.rig.bitmap().count_non_background() > 20000);
    const std::wstring path = exe_dir() + L"details_grid_10k.png";
    CHECK(g.rig.bitmap().write_png(path));
}

TEST(gallery_png) {
    Rig rig(900, 560);
    auto root = std::make_unique<DockPanel>();
    auto* bar = root->emplace<StackPanel>(Orientation::Horizontal, 8.0f);
    bar->dock = Dock::Top;
    bar->height = 44;
    bar->pad = 8;
    bar->nc_hit = HTCAPTION;
    bar->style.bg = tok::HeaderBg;
    auto* ok = bar->emplace<Button>(L"Copy");
    ok->primary = true;
    bar->emplace<Button>(L"Move");
    auto* off = bar->emplace<Button>(L"Delete");
    off->set_enabled(false);
    auto* search = bar->emplace<Edit>();
    search->placeholder = L"Search";
    search->flex = 1;
    auto* cb = bar->emplace<CheckBox>(L"Hidden files");
    cb->set_checked(true);
    auto* split = root->emplace<Splitter>(Orientation::Horizontal);
    split->set_position(220);
    auto* tree = split->emplace<TreeView>();
    auto* list = split->emplace<ListView>();
    static GridProvider provider(2500);
    class FolderTree : public TreeProvider {
    public:
        size_t child_count(uint64_t n) const override { return n == 0 ? 4 : (n < 10 ? 3 : (n < 100 ? 2 : 0)); }
        uint64_t child_id(uint64_t n, size_t i) const override { return n == 0 ? i + 1 : n * 10 + i + 1; }
        std::wstring label(uint64_t n) const override {
            static const wchar_t* top[] = {L"Desktop", L"Documents", L"Pictures", L"Downloads"};
            return n >= 1 && n <= 4 ? top[n - 1] : L"Folder " + std::to_wstring(n);
        }
    };
    static FolderTree folders;
    tree->set_provider(&folders);
    tree->expand(3);
    tree->expand(31);
    tree->select(32);
    list->set_columns(grid_columns());
    list->set_provider(&provider);
    list->set_sort(0, true);
    rig.set(std::move(root));
    provider.select(SelectOp::Replace, 2, 4);
    rig.root.set_focus(list);
    rig.render();
    CHECK(rig.bitmap().count_non_background() > 20000);
    CHECK(rig.bitmap().write_png(exe_dir() + L"widget_gallery.png"));
}
