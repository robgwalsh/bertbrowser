#include "fixture.h"

#include <set>

using namespace bb;
using namespace bb::ui;

namespace {

void expect_rect(const Rect& r, float l, float t, float rr, float b, const char* what) {
    const bool ok = std::fabs(r.l - l) < 0.01f && std::fabs(r.t - t) < 0.01f && std::fabs(r.r - rr) < 0.01f &&
                    std::fabs(r.b - b) < 0.01f;
    if (!ok) {
        std::printf("  %s: got {%g %g %g %g} want {%g %g %g %g}\n", what, r.l, r.t, r.r, r.b, l, t, rr, b);
    }
    CHECK(ok);
}

// A tree provider over a generated tree: 100 top-level nodes with 50 children each, 20 grandchildren
// under each child. Ids encode the path. Records which labels were asked for.
class GenTree : public TreeProvider {
public:
    size_t child_count(uint64_t node) const override {
        if (node == 0) return 100;
        if (node < 1000) return 50;
        if (node < 1000000) return 20;
        return 0;
    }
    uint64_t child_id(uint64_t node, size_t i) const override {
        if (node == 0) return i + 1;
        if (node < 1000) return node * 1000 + i + 1;
        return node * 1000 + i + 1;
    }
    std::wstring label(uint64_t node) const override {
        asked.insert(node);
        return L"node " + std::to_wstring(node);
    }
    mutable std::set<uint64_t> asked;
};

}  // namespace

TEST(stack_panel_layout_numbers) {
    Rig rig(200, 300);
    auto root = std::make_unique<StackPanel>(Orientation::Vertical, 0.0f);
    auto* a = root->emplace<Panel>();
    a->height = 30;
    auto* b = root->emplace<Panel>();
    b->flex = 1;
    auto* c = root->emplace<Panel>();
    c->height = 20;
    rig.set(std::move(root));
    expect_rect(a->bounds, 0, 0, 200, 30, "a");
    expect_rect(b->bounds, 0, 30, 200, 280, "b");
    expect_rect(c->bounds, 0, 280, 200, 300, "c");
}

TEST(stack_panel_horizontal_flex_and_spacing) {
    Rig rig(400, 100);
    auto root = std::make_unique<StackPanel>(Orientation::Horizontal, 10.0f);
    auto* a = root->emplace<Panel>();
    a->width = 50;
    auto* b = root->emplace<Panel>();
    b->flex = 1;
    auto* c = root->emplace<Panel>();
    c->flex = 3;
    rig.set(std::move(root));
    // leftover = 400 - 50 - 2*10 = 330, shared 1:3
    expect_rect(a->bounds, 0, 0, 50, 100, "a");
    expect_rect(b->bounds, 60, 0, 60 + 82.5f, 100, "b");
    expect_rect(c->bounds, 152.5f, 0, 400, 100, "c");
}

TEST(dock_panel_layout_numbers) {
    Rig rig(500, 400);
    auto root = std::make_unique<DockPanel>();
    auto* top = root->emplace<Panel>();
    top->dock = Dock::Top;
    top->height = 40;
    auto* left = root->emplace<Panel>();
    left->dock = Dock::Left;
    left->width = 120;
    auto* bottom = root->emplace<Panel>();
    bottom->dock = Dock::Bottom;
    bottom->height = 24;
    auto* fill = root->emplace<Panel>();
    rig.set(std::move(root));
    expect_rect(top->bounds, 0, 0, 500, 40, "top");
    expect_rect(left->bounds, 0, 40, 120, 400, "left");
    expect_rect(bottom->bounds, 120, 376, 500, 400, "bottom");
    expect_rect(fill->bounds, 120, 40, 500, 376, "fill");
}

TEST(measure_uses_text_metrics) {
    Rig rig(300, 100);
    auto root = std::make_unique<StackPanel>(Orientation::Vertical, 0.0f);
    auto* text = root->emplace<TextBlock>(L"Hello world");
    auto* button = root->emplace<Button>(L"OK");
    auto* box = root->emplace<CheckBox>(L"Remember");
    rig.set(std::move(root));
    CHECK(text->desired().w > 40 && text->desired().h >= 14);
    CHECK_NEAR(button->desired().h, 28.0, 0.01);
    CHECK(button->desired().w >= 28);
    CHECK(box->desired().w > CheckBox::kBox + CheckBox::kGap);
}

TEST(hit_testing_z_order_and_visibility) {
    Rig rig(200, 200);
    auto root = std::make_unique<Panel>();
    auto* under = root->emplace<Panel>();
    under->name = "under";
    auto* over = root->emplace<Panel>();  // later sibling paints and hits on top
    over->name = "over";
    over->width = 80;
    over->height = 80;
    rig.set(std::move(root));
    over->bounds = Rect{10, 10, 90, 90};  // as a container would place it
    CHECK(rig.root.hit_test({50, 50}) == over);
    CHECK(rig.root.hit_test({150, 150}) == under);
    over->set_visible(false);
    CHECK(rig.root.hit_test({50, 50}) == under);
    CHECK(rig.root.hit_test({500, 500}) == nullptr);
    CHECK(rig.root.find("under") == under);
}

TEST(invalidation_unions_into_the_root_dirty_rect) {
    Rig rig(400, 200);
    auto root = std::make_unique<StackPanel>(Orientation::Horizontal, 20.0f);
    auto* a = root->emplace<Button>(L"One");
    auto* b = root->emplace<Button>(L"Two");
    auto* c = root->emplace<Button>(L"Three");
    rig.set(std::move(root));
    rig.render();
    CHECK(rig.root.dirty().empty());
    a->invalidate();
    CHECK(rig.root.dirty() == a->bounds);
    c->invalidate();
    CHECK(rig.root.dirty() == a->bounds.united(c->bounds));
    CHECK(!rig.root.dirty().intersects(b->bounds) || rig.root.dirty().contains({b->bounds.l + 1, b->bounds.t + 1}));
    const uint64_t paints = rig.root.paint_count();
    rig.render();
    CHECK_EQ(rig.root.paint_count(), paints + 1);
    CHECK(rig.root.dirty().empty());
    CHECK(rig.win.render() == S_FALSE);  // nothing dirty: nothing painted, nothing presented
}

TEST(repaint_touches_only_the_dirty_rect) {
    Rig rig(300, 100);
    auto root = std::make_unique<StackPanel>(Orientation::Horizontal, 0.0f);
    auto* left = root->emplace<Panel>();
    left->width = 150;
    left->style.bg = tok::Accent;
    auto* right = root->emplace<Panel>();
    right->flex = 1;
    right->style.bg = tok::ControlBg;
    rig.set(std::move(root));
    rig.render();
    const uint32_t before_left = rig.bitmap().pixel(50, 50);
    left->style.bg = tok::Marquee;  // changed, but never invalidated...
    right->style.bg = tok::SelectionBg;
    right->invalidate();  // ...so only the right half is repainted
    rig.render();
    CHECK_EQ(rig.bitmap().pixel(50, 50), before_left);
    CHECK(rig.bitmap().pixel(200, 50) != 0 && rig.bitmap().pixel(200, 50) != before_left);
}

TEST(tab_focus_traversal) {
    Rig rig(400, 200);
    auto root = std::make_unique<StackPanel>(Orientation::Vertical, 4.0f);
    auto* b1 = root->emplace<Button>(L"One");
    auto* e = root->emplace<Edit>();
    auto* hidden = root->emplace<Button>(L"Hidden");
    hidden->set_visible(false);
    auto* b2 = root->emplace<Button>(L"Two");
    auto* off = root->emplace<Button>(L"Off");
    off->set_enabled(false);
    auto* label = root->emplace<TextBlock>(L"not focusable");
    rig.set(std::move(root));

    CHECK(rig.root.focused() == nullptr);
    rig.key(VK_TAB);
    CHECK(rig.root.focused() == b1);
    rig.key(VK_TAB);
    CHECK(rig.root.focused() == e);
    rig.key(VK_TAB);
    CHECK(rig.root.focused() == b2);  // skips the hidden and the disabled
    rig.key(VK_TAB);
    CHECK(rig.root.focused() == b1);  // wraps
    rig.key(VK_TAB, true);
    CHECK(rig.root.focused() == b2);  // Shift+Tab goes back, wrapping the other way
    rig.key(VK_TAB, true);
    CHECK(rig.root.focused() == e);
    rig.click(Rig::center(label));
    CHECK(rig.root.focused() == e);  // clicking something that cannot focus leaves focus alone
    rig.click(Rig::center(b1));
    CHECK(rig.root.focused() == b1);
    (void)hidden;
}

TEST(button_click_semantics) {
    Rig rig(300, 100);
    auto row = std::make_unique<StackPanel>(Orientation::Horizontal, 0.0f);
    auto* b = row->emplace<Button>(L"Press");
    b->width = 100;
    rig.set(std::move(row));
    int n = 0;
    b->on_click = [&] { ++n; };
    rig.click(Rig::center(b));
    CHECK_EQ(n, 1);
    // Press, drag off, release: not a click.
    rig.down(Rig::center(b));
    CHECK(b->pressed());
    rig.move({250, 80});
    rig.up({250, 80});
    CHECK_EQ(n, 1);
    // Keyboard.
    rig.root.set_focus(b);
    rig.key(VK_SPACE);
    rig.key(VK_RETURN);
    CHECK_EQ(n, 3);
    b->set_enabled(false);
    rig.click(Rig::center(b));
    rig.key(VK_SPACE);
    CHECK_EQ(n, 3);
}

TEST(checkbox_toggles) {
    Rig rig(300, 100);
    auto* c = rig.set(std::make_unique<CheckBox>(L"Show hidden files"));
    std::vector<bool> seen;
    c->on_toggle = [&](bool v) { seen.push_back(v); };
    rig.click(Rig::center(c));
    CHECK(c->checked());
    rig.key(VK_SPACE);
    CHECK(!c->checked());
    CHECK_EQ(seen.size(), 2);
    CHECK(seen[0] && !seen[1]);
    c->set_checked(true);  // from code: no callback
    CHECK_EQ(seen.size(), 2);
    rig.render();
    CHECK(!rig.bitmap().is_flat());
}

TEST(icon_geometry_from_data) {
    static const float square[] = {IconMove, 2, 2, IconLine, 18, 2, IconLine, 18, 18, IconLine, 2, 18, IconClose};
    static const IconData data{20.0f, square, sizeof(square) / sizeof(float)};
    Rig rig(120, 120);
    auto root = std::make_unique<Panel>();
    auto* icon = root->emplace<Icon>(&data, 100.0f);
    icon->style.fg = tok::Accent;
    icon->width = icon->height = 100;
    rig.set(std::move(root));
    rig.render();
    const uint32_t accent = Env::get().res->palette.argb(tok::Accent);
    const Rect b = icon->bounds;
    CHECK_EQ(rig.bitmap().pixel(static_cast<UINT>((b.l + b.r) / 2), static_cast<UINT>((b.t + b.b) / 2)), accent);
    // The square spans 2..18 of 20: a 5% margin of the 100 px box is empty.
    CHECK(rig.bitmap().pixel(static_cast<UINT>(b.l + 2), static_cast<UINT>(b.t + 2)) != accent);
}

TEST(scroll_viewer_scrolls_clamps_and_drags) {
    Rig rig(300, 300);
    auto sv = std::make_unique<ScrollViewer>();
    auto* content = sv->emplace<StackPanel>(Orientation::Vertical, 0.0f);
    for (int i = 0; i < 50; ++i) {
        auto* row = content->emplace<Panel>();
        row->height = 20;
        row->name = "row" + std::to_string(i);
    }
    auto* viewer = rig.set(std::move(sv));
    CHECK_NEAR(viewer->extent().h, 1000.0, 0.01);
    const float vw = viewer->viewport().width();
    CHECK_NEAR(vw, 300.0 - ScrollBar::kThickness, 0.01);  // the bar takes its strip
    CHECK_NEAR(content->bounds.width(), vw, 0.01);

    rig.root.wheel({100, 100}, -120);  // one notch down = 3 lines of 16
    CHECK_NEAR(viewer->offset_y(), 48.0, 0.01);
    expect_rect(content->bounds, 0, -48, vw, 952, "content after wheel");
    rig.root.wheel({100, 100}, 120);
    rig.root.wheel({100, 100}, 120);
    CHECK_NEAR(viewer->offset_y(), 0.0, 0.01);  // clamped at the top
    viewer->scroll_to(0, 99999);
    CHECK_NEAR(viewer->offset_y(), 700.0, 0.01);  // extent 1000 - viewport 300
    viewer->scroll_to(0, 0);

    // Only the viewport hits content; the bar strip belongs to the viewer.
    CHECK(rig.root.hit_test({50, 25})->name == "row1");
    CHECK(rig.root.hit_test({296, 100}) == viewer);

    // Drag the thumb: it is 300 * 300/1000 = 90 tall; moving it 100 px moves 100/(300-90) of the range.
    const float bar_x = 300 - ScrollBar::kThickness / 2;
    rig.down({bar_x, 20});
    rig.move({bar_x, 120});
    rig.up({bar_x, 120});
    CHECK_NEAR(viewer->offset_y(), 100.0 / 210.0 * 700.0, 1.0);
    // Clicking the track pages.
    viewer->scroll_to(0, 0);
    rig.click({bar_x, 250});
    CHECK_NEAR(viewer->offset_y(), 300.0, 0.01);
    rig.render();
    CHECK(!rig.bitmap().is_flat());
}

TEST(splitter_drag_and_clamp) {
    Rig rig(600, 300);
    auto sp = std::make_unique<Splitter>(Orientation::Horizontal);
    auto* first = sp->emplace<Panel>();
    auto* second = sp->emplace<Panel>();
    first->style.bg = tok::ControlBg;
    second->style.bg = tok::RowAlt;
    sp->set_position(200);
    auto* s = rig.set(std::move(sp));
    expect_rect(first->bounds, 0, 0, 200, 300, "first");
    expect_rect(second->bounds, 205, 0, 600, 300, "second");
    CHECK(rig.root.hit_test({202, 100}) == s);  // the bar is the splitter's
    CHECK(rig.root.hit_test({100, 100}) == first);
    CHECK(s->cursor_at({202, 100}) == Cursor::SizeWE);
    float moved = 0;
    s->on_moved = [&](float p) { moved = p; };
    rig.down({202, 100});
    rig.move({302, 100});
    CHECK_NEAR(s->position(), 300.0, 0.01);
    CHECK_NEAR(moved, 300.0, 0.01);
    rig.move({5, 100});
    CHECK_NEAR(s->position(), s->min_first, 0.01);  // clamped
    rig.move({900, 100});
    CHECK_NEAR(s->position(), 600.0 - 5 - s->min_second, 0.01);
    rig.up({900, 100});
    rig.relayout();
    expect_rect(first->bounds, 0, 0, 555, 300, "first after drag");
    // A vertical splitter divides top from bottom.
    Rig rig2(300, 400);
    auto sp2 = std::make_unique<Splitter>(Orientation::Vertical);
    auto* top = sp2->emplace<Panel>();
    auto* bottom = sp2->emplace<Panel>();
    sp2->set_position(100);
    rig2.set(std::move(sp2));
    expect_rect(top->bounds, 0, 0, 300, 100, "top");
    expect_rect(bottom->bounds, 0, 105, 300, 400, "bottom");
}

TEST(tree_view_expand_collapse_and_virtualisation) {
    Rig rig(300, 400);
    GenTree provider;
    auto* tree = rig.set(std::make_unique<TreeView>());
    tree->set_provider(&provider);
    CHECK_EQ(tree->visible_count(), 100);
    // Expanding nodes adds rows.
    tree->expand(1);
    CHECK_EQ(tree->visible_count(), 150);
    CHECK_EQ(tree->id_at_row(1), 1001);
    tree->expand(1001);
    CHECK_EQ(tree->visible_count(), 170);
    tree->collapse(1);
    CHECK_EQ(tree->visible_count(), 100);
    tree->expand(1);  // the grandchildren's expansion is remembered
    CHECK_EQ(tree->visible_count(), 170);

    // Paint asks for the visible rows only: 400 / 22 = 19 rows.
    provider.asked.clear();
    rig.render();
    const auto [first, last] = tree->visible_range();
    CHECK_EQ(first, 0);
    CHECK_EQ(last, 19);
    CHECK(provider.asked.size() <= 19 + 2);
    CHECK_EQ(tree->painted_rows(), 19);

    // Keyboard: Down, Right expands, Right again steps in, Left steps out, Left collapses.
    tree->select(2);
    CHECK_EQ(tree->selected_id(), 2);
    rig.root.set_focus(tree);
    rig.key(VK_RIGHT);
    CHECK(tree->is_expanded(2));
    rig.key(VK_RIGHT);
    CHECK_EQ(tree->selected_id(), 2001);
    rig.key(VK_LEFT);
    CHECK_EQ(tree->selected_id(), 2);
    rig.key(VK_LEFT);
    CHECK(!tree->is_expanded(2));
    rig.key(VK_END);
    CHECK_EQ(tree->selected_id(), 100);  // the last visible row
    CHECK(tree->scroll_y() > 0);          // scrolled to keep it on screen

    // Mouse: a click on the chevron toggles, a click on the label selects.
    tree->collapse(1);
    tree->set_scroll_y(0);
    bool toggled = false;
    tree->on_toggle = [&](uint64_t, bool) { toggled = true; };
    const Rect r5 = tree->row_rect(tree->row_of(5));  // node 5 is row 4 now
    rig.click({r5.l + 4 + 8, (r5.t + r5.b) / 2});
    CHECK(toggled);
    CHECK(tree->is_expanded(5));
    rig.click({r5.l + 100, (r5.t + r5.b) / 2});
    CHECK_EQ(tree->selected_id(), 5);
    rig.root.wheel({100, 100}, -120);
    CHECK_NEAR(tree->scroll_y(), 66.0, 0.01);
}
