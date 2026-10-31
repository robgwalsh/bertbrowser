#include "fixture.h"
#include "ui/controls/pickers.h"
#include "ui/collections/tile_view.h"

#include <algorithm>
#include <map>
#include <set>

using namespace bb;
using namespace bb::ui;

namespace {

class FakeRows : public RowProvider {
public:
    explicit FakeRows(size_t n) : n_(n), sel_(n, 0) {}
    size_t row_count() const override { return n_; }
    uint64_t row_id(size_t row) const override {
        ++id_calls;
        return 1000 + row;
    }
    std::wstring cell_text(size_t row, int) const override {
        ++text_calls;
        return L"IMG_" + std::to_wstring(row) + L".jpg";
    }
    bool selected(size_t row) const override { return row < n_ && sel_[row]; }
    void select(SelectOp op, size_t first, size_t last) override {
        ++select_calls;
        switch (op) {
            case SelectOp::Clear: std::fill(sel_.begin(), sel_.end(), static_cast<char>(0)); break;
            case SelectOp::Replace:
                std::fill(sel_.begin(), sel_.end(), static_cast<char>(0));
                [[fallthrough]];
            case SelectOp::Add:
                for (size_t i = first; i <= last && i < n_; ++i) sel_[i] = static_cast<char>(1);
                break;
            case SelectOp::Toggle:
                for (size_t i = first; i <= last && i < n_; ++i) sel_[i] = static_cast<char>(sel_[i] ? 0 : 1);
                break;
        }
    }
    size_t selected_count() const { return static_cast<size_t>(std::count(sel_.begin(), sel_.end(), static_cast<char>(1))); }

    size_t n_;
    std::vector<char> sel_;
    mutable size_t id_calls = 0, text_calls = 0;
    int select_calls = 0;
};

ThumbPtr solid(uint32_t rgb, uint32_t w = 64, uint32_t h = 64) {
    auto img = std::make_shared<ThumbImage>();
    img->width = w;
    img->height = h;
    img->bgra.resize(static_cast<size_t>(w) * h * 4);
    for (size_t i = 0; i < static_cast<size_t>(w) * h; ++i) {
        img->bgra[i * 4 + 0] = static_cast<uint8_t>(rgb & 0xFF);
        img->bgra[i * 4 + 1] = static_cast<uint8_t>((rgb >> 8) & 0xFF);
        img->bgra[i * 4 + 2] = static_cast<uint8_t>((rgb >> 16) & 0xFF);
        img->bgra[i * 4 + 3] = 0xFF;
    }
    return img;
}

// Something that reads as a picture: a sky-to-ground gradient in the row's hue, a sun, and a diagonal.
ThumbPtr picture(size_t row, uint32_t w, uint32_t h) {
    auto img = std::make_shared<ThumbImage>();
    img->width = w;
    img->height = h;
    img->bgra.resize(static_cast<size_t>(w) * h * 4);
    const float hue = std::fmod(static_cast<float>(row) * 47.0f, 360.0f);
    for (uint32_t y = 0; y < h; ++y) {
        for (uint32_t x = 0; x < w; ++x) {
            const float fy = static_cast<float>(y) / static_cast<float>(h);
            uint32_t rgb = hsv_to_rgb({hue, 0.35f + 0.5f * fy, 1.0f - 0.55f * fy});
            const float dx = static_cast<float>(x) - static_cast<float>(w) * 0.72f, dy = static_cast<float>(y) - static_cast<float>(h) * 0.28f;
            if (dx * dx + dy * dy < static_cast<float>(h * h) * 0.02f) rgb = 0xFFF2B0;
            if (fy > 0.62f && std::abs(static_cast<int>(x) - static_cast<int>(y) * 2 + static_cast<int>(w) / 3) < 4) rgb = 0x203040;
            uint8_t* px = &img->bgra[(static_cast<size_t>(y) * w + x) * 4];
            px[0] = static_cast<uint8_t>(rgb & 0xFF);
            px[1] = static_cast<uint8_t>((rgb >> 8) & 0xFF);
            px[2] = static_cast<uint8_t>((rgb >> 16) & 0xFF);
            px[3] = 0xFF;
        }
    }
    return img;
}

class FakeThumbs : public ThumbnailProvider {
public:
    struct Req {
        uint64_t id;
        size_t row;
        uint32_t px;
    };
    void request(uint64_t id, size_t row, uint32_t px, ThumbnailSink& s) override {
        sink = &s;
        requests.push_back({id, row, px});
        if (auto_answer && !(skip.count(row))) {
            s.deliver(id, make ? make(row) : solid(0x3060C0));
        } else {
            pending.push_back(id);
        }
    }
    void cancel(uint64_t id) override {
        cancelled.push_back(id);
        std::erase(pending, id);
    }
    void answer(uint64_t id, ThumbPtr img) {
        std::erase(pending, id);
        sink->deliver(id, std::move(img));
    }
    // Answers whatever is waiting until nothing is.
    void drain(const std::function<ThumbPtr(uint64_t)>& f) {
        while (!pending.empty()) answer(pending.front(), f(pending.front()));
    }

    ThumbnailSink* sink = nullptr;
    std::vector<Req> requests;
    std::vector<uint64_t> pending, cancelled;
    bool auto_answer = false;
    std::set<size_t> skip;
    std::function<ThumbPtr(size_t)> make;
};

struct TileRig {
    ThemeScope theme;
    Rig rig;
    FakeRows rows;
    FakeThumbs thumbs;
    TileView* view = nullptr;

    TileRig(size_t n, int w = 800, int h = 600, bool with_thumbs = true) : rig(w, h), rows(n) {
        auto v = std::make_unique<TileView>();
        view = rig.set(std::move(v));
        view->set_provider(&rows);
        if (with_thumbs) view->set_thumbnails(&thumbs);
        rig.relayout();
    }
    Point center(size_t row) const {
        const Rect r = view->tile_rect(row);
        return {(r.l + r.r) / 2, (r.t + r.b) / 2};
    }
};

}  // namespace

// ---- arithmetic -------------------------------------------------------------------------------------

// Folders and files that are not pictures are full-width rows above the grid; the grid starts under them.
TEST(tile_layout_rows_above_the_grid) {
    TileMetrics m;
    m.caption_h = 36;
    TileLayout lay{m, 800, 13};
    lay.leading = 3;
    lay.row_h = 26;
    CHECK_EQ(lay.tiles(), 10);
    CHECK_EQ(lay.rows(), 2);
    CHECK(lay.tile_rect(0) == Rect::xywh(8, 8, 784, 26));
    CHECK(lay.tile_rect(2) == Rect::xywh(8, 60, 784, 26));
    CHECK_NEAR(lay.grid_top(), 8 + 78 + 6, 0.01);
    CHECK(lay.tile_rect(3) == Rect::xywh(8, 92, 120, 160));
    CHECK(lay.tile_rect(9) == Rect::xywh(8, 92 + 166, 120, 160));
    CHECK_NEAR(lay.content_height(), 92 + 160 + 6 + 160 + 8, 0.01);
    CHECK_EQ(lay.index_at({400, 40}), 1);   // a row, anywhere across it
    CHECK_EQ(lay.index_at({50, 100}), 3);   // the first tile
    CHECK(lay.index_at({400, 89}) == TileView::npos);  // the gap between the rows and the grid
    auto v = lay.visible(0, 50);
    CHECK(v.first == 0 && v.second == 2);   // rows only
    v = lay.visible(70, 100);
    CHECK(v.first == 2 && v.second == 9);   // the last row and the first line of tiles
    v = lay.visible(200, 600);
    CHECK(v.first == 3 && v.second == 13);  // tiles only
    // Every row, no grid at all.
    TileLayout rows_only{m, 800, 3};
    rows_only.leading = 3;
    CHECK_NEAR(rows_only.content_height(), 8 + 78 + 8, 0.01);
    CHECK(rows_only.visible(0, 600).second == 3);
}

TEST(tile_layout_arithmetic) {
    TileMetrics m;
    m.caption_h = 36;  // round numbers below
    TileLayout lay{m, 800, 100};
    CHECK_EQ(lay.columns(), 6);
    CHECK_EQ(lay.rows(), 17);
    CHECK_NEAR(lay.content_height(), 2832, 0.01);
    CHECK(lay.tile_rect(0) == Rect::xywh(8, 8, 120, 160));
    CHECK(lay.tile_rect(7) == Rect::xywh(134, 174, 120, 160));
    CHECK(lay.image_rect(0) == Rect::xywh(16, 16, 104, 104));
    CHECK(lay.caption_rect(0) == Rect::xywh(16, 124, 104, 36));
    auto v = lay.visible(0, 600);
    CHECK(v.first == 0 && v.second == 24);
    v = lay.visible(500, 600);
    CHECK(v.first == 12 && v.second == 42);
    v = lay.visible(lay.content_height() - 600, 600);
    CHECK(v.second == 100);  // clamped to the count
    CHECK_EQ(lay.index_at({50, 50}), 0);
    CHECK_EQ(lay.index_at({140, 180}), 7);
    CHECK(lay.index_at({130, 50}) == TileView::npos);  // in the gap between columns
    CHECK(lay.index_at({50, 170}) == TileView::npos);  // in the gap between rows
    CHECK(lay.index_at({795, 50}) == TileView::npos);  // right of the last column
    CHECK(lay.index_at({5, 5}) == TileView::npos);     // in the margin
    CHECK(lay.index_at({8 + 4 * 126 + 10, 8 + 16 * 166 + 10}) == TileView::npos);  // past the last tile
    CHECK_EQ(lay.index_at({8 + 3 * 126 + 10, 8 + 16 * 166 + 10}), 99);
    // Degenerate: nothing, and a width too narrow for one column still has one.
    TileLayout none{TileMetrics{}, 800, 0};
    CHECK_EQ(none.rows(), 0);
    CHECK_NEAR(none.content_height(), 0, 0.001);
    CHECK(none.visible(0, 600).second == 0);
    TileLayout narrow{TileMetrics{}, 50, 10};
    CHECK_EQ(narrow.columns(), 1);
    // Wider windows fit more columns, exactly at the boundary.
    TileLayout w{TileMetrics{}, 2 * 8 + 6 * 120 + 5 * 6, 100};
    CHECK_EQ(w.columns(), 6);
    w.width -= 1;
    CHECK_EQ(w.columns(), 5);
}

TEST(tile_view_touches_only_realized_rows_of_a_huge_list) {
    TileRig t(100000, 800, 600, false);
    t.rig.render();
    const auto r = t.view->realized();
    CHECK(r.first == 0 && r.second >= 18 && r.second <= 30);
    CHECK_EQ(t.view->painted_tiles(), r.second - r.first);
    CHECK(t.rows.text_calls <= 30);  // captions asked for the painted tiles and no others
    CHECK(t.rows.id_calls < 120);
    // Scroll to the middle: the range moves, the work does not grow.
    t.rows.text_calls = t.rows.id_calls = 0;
    t.view->scroll_to_row(50000);
    t.rig.render();
    const auto m = t.view->realized();
    CHECK(m.first <= 50000 && 50000 < m.second);
    CHECK(m.second - m.first <= 30);
    CHECK(t.rows.text_calls <= 30);
    CHECK(t.rows.id_calls < 120);
    CHECK(t.view->tile_rect(50000).intersects(t.view->bounds));
    // The scroll range is the whole arithmetic height, and the end is reachable.
    t.view->set_scroll_y(1.0e9f);
    const auto e = t.view->realized();
    CHECK(e.second == 100000);
}

// ---- thumbnails -------------------------------------------------------------------------------------

TEST(tile_thumbnails_are_gated_and_placeholders_show_until_ready) {
    TileRig t(1000);
    CHECK_EQ(t.thumbs.requests.size(), 4);  // the four-slot gate, in visible order
    CHECK_EQ(t.view->in_flight(), 4);
    for (size_t i = 0; i < 4; ++i) {
        CHECK_EQ(t.thumbs.requests[i].row, i);
        CHECK_EQ(t.thumbs.requests[i].id, 1000 + i);
        CHECK_EQ(t.thumbs.requests[i].px, 104);
    }
    t.rig.render();
    const Rect slot0 = TileLayout{t.view->metrics, 788, 1000}.image_rect(0);
    const UINT cx = static_cast<UINT>(slot0.l + t.view->bounds.l + 52), cy = static_cast<UINT>(slot0.t + t.view->bounds.t + 52);
    const uint32_t placeholder = Env::get().res->palette.argb(tok::ThumbnailTileBackground) & 0xFFFFFF;
    CHECK_EQ(t.rig.bitmap().pixel(cx, cy) & 0xFFFFFF, placeholder);
    // One arrives: the slot shows it, and a freed slot asks for the next missing tile.
    t.thumbs.answer(1000, solid(0xE02020));
    CHECK_EQ(t.view->ready_count(), 1);
    CHECK(t.view->has_image(1000));
    CHECK_EQ(t.thumbs.requests.size(), 5);
    CHECK_EQ(t.view->in_flight(), 4);
    t.rig.render();
    CHECK_EQ(t.rig.bitmap().pixel(cx, cy) & 0xFFFFFF, 0xE02020u);
    // "No thumbnail exists" keeps the placeholder and is not asked about again.
    t.thumbs.answer(1001, nullptr);
    CHECK(!t.view->has_image(1001));
    const size_t asked = t.thumbs.requests.size();
    t.thumbs.drain([](uint64_t) { return solid(0x2040E0); });
    const auto r = t.view->realized();
    CHECK_EQ(t.thumbs.requests.size(), r.second - r.first);  // every realized tile exactly once
    CHECK(t.thumbs.requests.size() > asked);
    CHECK_EQ(t.view->ready_count(), r.second - r.first - 1);
    CHECK_EQ(t.view->in_flight(), 0);
    t.rig.render();
    const Rect slot1 = TileLayout{t.view->metrics, 788, 1000}.image_rect(1);
    CHECK_EQ(t.rig.bitmap().pixel(static_cast<UINT>(slot1.l + 52 + t.view->bounds.l), static_cast<UINT>(slot1.t + 52 + t.view->bounds.t)) & 0xFFFFFF,
             placeholder);
    // Images are drawn at their own size, centred, never stretched: a 64px image in a 104px slot.
    CHECK_EQ(t.rig.bitmap().pixel(cx + 25, cy) & 0xFFFFFF, 0xE02020u);
    CHECK_EQ(t.rig.bitmap().pixel(cx + 45, cy) & 0xFFFFFF, placeholder);
}

TEST(tile_thumbnail_retention_follows_the_realized_rows) {
    TileRig t(1000);
    CHECK_EQ(t.thumbs.pending.size(), 4);
    const std::vector<uint64_t> first_four = t.thumbs.pending;
    // Scrolling away before they arrive cancels them and asks for the new rows instead.
    t.view->set_scroll_y(20000);
    CHECK_EQ(t.thumbs.cancelled.size(), 4);
    for (uint64_t id : first_four) CHECK(std::find(t.thumbs.cancelled.begin(), t.thumbs.cancelled.end(), id) != t.thumbs.cancelled.end());
    CHECK_EQ(t.view->in_flight(), 4);
    CHECK_EQ(t.view->cached(), 4);
    // An answer that arrives late for a row that is gone is dropped, not cached.
    t.thumbs.sink->deliver(first_four[0], solid(0x00FF00));
    CHECK_EQ(t.view->ready_count(), 0);
    CHECK_EQ(t.view->in_flight(), 4);
    // Memory stays bounded by the screen however far the list is scrolled and however much is delivered.
    size_t most = 0;
    for (float y = 0; y < 60000; y += 1500) {
        t.view->set_scroll_y(y);
        t.thumbs.drain([](uint64_t) { return solid(0x808080); });
        const auto r = t.view->realized();
        most = std::max(most, t.view->cached());
        CHECK(t.view->cached() <= r.second - r.first);
    }
    CHECK(most > 10);
    // Keeping a row of tiles beyond the screen is a setting, off by default.
    t.view->set_scroll_y(5000);
    t.thumbs.drain([](uint64_t) { return solid(0x808080); });
    const auto before = t.view->realized();
    const uint64_t scrolled_off = 1000 + before.first;
    t.view->set_scroll_y(5000 + 166);  // one row of tiles
    CHECK(!t.view->has_image(scrolled_off));
    t.view->retain_extra_rows = 1;
    t.view->set_scroll_y(5000);
    t.thumbs.drain([](uint64_t) { return solid(0x808080); });
    t.view->set_scroll_y(5000 + 166);
    CHECK(t.view->has_image(scrolled_off));
}

TEST(tile_provider_may_answer_inside_request) {
    TileRig t(200, 800, 600, false);
    t.thumbs.auto_answer = true;
    t.view->set_thumbnails(&t.thumbs);
    const auto r = t.view->realized();
    CHECK_EQ(t.view->ready_count(), r.second - r.first);  // no gate stall, no recursion trouble
    CHECK_EQ(t.view->in_flight(), 0);
    t.rig.render();
    CHECK(t.rig.bitmap().count_non_background() > 20000);
}

// ---- selection and keys -------------------------------------------------------------------------------

TEST(tile_view_selection_uses_bulk_calls) {
    TileRig t(100, 800, 600, false);
    t.rig.render();
    t.rig.click(t.center(5));
    CHECK_EQ(t.rows.select_calls, 1);
    CHECK(t.rows.selected(5));
    CHECK_EQ(t.rows.selected_count(), 1);
    CHECK_EQ(t.view->cursor(), 5);
    t.rig.click(t.center(7), Mods{false, true, false});
    CHECK_EQ(t.rows.select_calls, 2);
    CHECK(t.rows.selected(5) && t.rows.selected(7));
    t.rig.click(t.center(12), Mods{true, false, false});  // shift: anchor 7 to 12, in one call
    CHECK_EQ(t.rows.select_calls, 3);
    CHECK_EQ(t.rows.selected_count(), 6);
    CHECK(!t.rows.selected(5) && t.rows.selected(7) && t.rows.selected(12));
    // Empty space clears (the gap between two tiles).
    const Rect a = t.view->tile_rect(0);
    t.rig.click({a.r + 2, a.t + 20});
    CHECK_EQ(t.rows.selected_count(), 0);
    // Double click activates.
    size_t activated = TileView::npos;
    t.view->on_activate = [&](size_t row) { activated = row; };
    t.rig.down(t.center(9), {}, 2);
    t.rig.up(t.center(9));
    CHECK_EQ(activated, 9);
}

TEST(tile_view_keyboard_moves_by_tile_and_by_row) {
    TileRig t(100, 800, 600, false);
    t.rig.root.set_focus(t.view);
    t.view->select_row(0);
    const size_t cols = static_cast<size_t>(t.view->layout().columns());
    CHECK_EQ(cols, 6);
    t.rig.key(VK_RIGHT);
    CHECK_EQ(t.view->cursor(), 1);
    t.rig.key(VK_DOWN);
    CHECK_EQ(t.view->cursor(), 1 + cols);
    t.rig.key(VK_LEFT);
    CHECK_EQ(t.view->cursor(), cols);
    CHECK_EQ(t.rows.selected_count(), 1);
    CHECK(t.rows.selected(cols));
    t.rig.key(VK_END);
    CHECK_EQ(t.view->cursor(), 99);
    CHECK(t.view->tile_rect(99).intersects(t.view->bounds));  // scrolled into view
    t.rig.key(VK_HOME);
    CHECK_EQ(t.view->cursor(), 0);
    CHECK_NEAR(t.view->scroll_y(), 0, 0.01);
    // Shift extends from the anchor, one bulk call per step.
    const int calls = t.rows.select_calls;
    t.rig.key(VK_RIGHT, true);
    t.rig.key(VK_RIGHT, true);
    t.rig.key(VK_DOWN, true);
    CHECK_EQ(t.rows.select_calls, calls + 3);
    CHECK_EQ(t.rows.selected_count(), cols + 3);  // 0 .. 8
    // Ctrl+arrow moves the cursor without touching the selection.
    const size_t selected = t.rows.selected_count();
    t.rig.key(VK_RIGHT, false, true);
    CHECK_EQ(t.rows.selected_count(), selected);
    // Ctrl+A, Escape (which leaves the selection alone, as the details list does), Enter.
    t.rig.key('A', false, true);
    CHECK_EQ(t.rows.selected_count(), 100);
    t.rig.key(VK_ESCAPE);
    CHECK_EQ(t.rows.selected_count(), 100);
    size_t activated = TileView::npos;
    t.view->on_activate = [&](size_t row) { activated = row; };
    t.rig.key(VK_RETURN);
    CHECK(activated == t.view->cursor());
    // Page keys move by whole screens of rows.
    t.view->select_row(0);
    t.rig.key(VK_NEXT);
    CHECK(t.view->cursor() >= 3 * cols);
    CHECK(t.view->tile_rect(t.view->cursor()).intersects(t.view->bounds));
}

TEST(tile_view_scrolling_by_wheel_and_bar) {
    TileRig t(1000, 800, 600, false);
    t.rig.render();
    CHECK(t.view->scroll_y() == 0);
    t.rig.root.wheel({400, 300}, -120);
    CHECK_NEAR(t.view->scroll_y(), ui::wheel_rows(-120) * (t.view->metrics.tile_h() + t.view->metrics.gap) * 0.2, 0.01);
    t.rig.root.wheel({400, 300}, 240);
    CHECK_NEAR(t.view->scroll_y(), 0, 0.01);  // clamped at the top
    // Drag the bar's thumb.
    t.rig.down({794, 20});  // on the thumb at the top
    t.rig.move({794, 320});
    t.rig.up({794, 320});
    CHECK(t.view->scroll_y() > 1000);
    CHECK(t.view->realized().first > 30);
    // The width available for tiles excludes the bar once there is one.
    CHECK_NEAR(t.view->layout().width, 788, 0.01);
}

// ---- the photograph ----------------------------------------------------------------------------------

TEST(tile_grid_png) {
    TileRig t(60, 820, 540, false);
    t.thumbs.auto_answer = true;
    t.thumbs.skip = {7, 8, 9};  // still loading: placeholders
    t.thumbs.make = [](size_t row) { return row % 5 == 3 ? picture(row, 104, 70) : picture(row, 104, 104); };
    t.view->set_thumbnails(&t.thumbs);
    t.view->select_row(2);
    t.rows.select(SelectOp::Add, 3, 3);
    t.rows.select(SelectOp::Add, 10, 11);
    t.rig.root.set_focus(t.view);
    t.rig.render();
    CHECK(t.rig.bitmap().count_non_background() > 40000);
    CHECK(t.rig.bitmap().write_png(exe_dir() + L"tile_grid.png"));
}
