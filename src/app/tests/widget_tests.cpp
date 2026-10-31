#include "fixture.h"
#include "gfx/icons.gen.h"
#include "ui/controls/inputs.h"
#include "ui/popups/menu_bar.h"
#include "ui/controls/pickers.h"
#include "ui/controls/tabs.h"

#include <map>
#include <string>
#include <utility>
#include <vector>

using namespace bb;
using namespace bb::ui;

namespace {

// Children keep the rectangle they were placed at, through any number of layouts (a popup opening asks for
// one); a child that was never placed fills the canvas.
class Canvas : public Widget {
public:
    void place(Widget* w, Rect r) {
        placed_[w] = r;
        w->measure_tree({r.width(), r.height()});
        w->arrange(r);
    }
    Size measure(Size) override { return {}; }
    void arrange(Rect r) override {
        bounds = r;
        for (auto& c : children()) {
            const auto it = placed_.find(c.get());
            if (it != placed_.end()) {
                c->measure_tree({it->second.width(), it->second.height()});
                c->arrange(it->second);
            } else {
                c->arrange(r);
            }
        }
    }

private:
    std::map<Widget*, Rect> placed_;
};

// A window with widgets placed by hand: add() them, finish() (which puts the popup layer on top), then
// place() each at an exact rectangle.
struct Stage {
    ThemeScope theme;
    Rig rig;
    Canvas* root = nullptr;
    PopupLayer* layer = nullptr;

    explicit Stage(int w = 600, int h = 400) : rig(w, h) { root = rig.set(std::make_unique<Canvas>()); }
    template <class T, class... A>
    T* add(A&&... args) {
        return root->emplace<T>(std::forward<A>(args)...);
    }
    void finish() {
        layer = PopupLayer::install(*root);
        rig.relayout();
    }
    void place(Widget* w, Rect r) { root->place(w, r); }
    static Point mid(Rect r) { return {(r.l + r.r) / 2, (r.t + r.b) / 2}; }
};

bool near_colour(uint32_t argb, int r, int g, int b, int tol) {
    const int pr = static_cast<int>((argb >> 16) & 0xFF), pg = static_cast<int>((argb >> 8) & 0xFF), pb = static_cast<int>(argb & 0xFF);
    return std::abs(pr - r) <= tol && std::abs(pg - g) <= tol && std::abs(pb - b) <= tol;
}

}  // namespace

// ---- slider -------------------------------------------------------------------------------------------

TEST(slider_press_anywhere_then_drag) {
    Stage st;
    auto* s = st.add<Slider>();
    st.finish();
    st.place(s, Rect::xywh(20, 20, 200, 24));
    std::vector<std::pair<double, bool>> events;
    s->on_change = [&](double v, bool dragging) { events.emplace_back(v, dragging); };
    CHECK_NEAR(s->track().l, 27, 0.01);
    CHECK_NEAR(s->track().r, 213, 0.01);
    CHECK_NEAR(s->thumb().l + Slider::kThumb / 2, 27, 0.01);  // value 0: at the left end
    // Press well away from the thumb: the value goes there, and the drag carries on from it.
    st.rig.down({120, 32});
    CHECK_NEAR(s->value(), 50, 0.01);
    CHECK(s->dragging());
    CHECK_NEAR(s->thumb().l + Slider::kThumb / 2, 120, 0.01);
    st.rig.move({213, 90});  // the pointer wandered off the track vertically: still dragging
    CHECK_NEAR(s->value(), 100, 0.01);
    st.rig.move({500, 90});  // clamped, unchanged: no event
    st.rig.up({500, 90});
    CHECK(!s->dragging());
    CHECK_EQ(events.size(), 3);
    CHECK(events[0].first == 50 && events[0].second);
    CHECK(events[1].first == 100 && events[1].second);
    CHECK(events[2].first == 100 && !events[2].second);  // the release
    // Keys.
    s->on_change = [&](double v, bool d) { events.emplace_back(v, d); };
    events.clear();
    st.rig.key(VK_LEFT);
    CHECK_NEAR(s->value(), 99, 0.01);
    st.rig.key(VK_PRIOR);
    CHECK_NEAR(s->value(), 100, 0.01);
    st.rig.key(VK_HOME);
    CHECK_NEAR(s->value(), 0, 0.01);
    st.rig.key(VK_NEXT);
    CHECK_NEAR(s->value(), 0, 0.01);
    st.rig.key(VK_END);
    CHECK_NEAR(s->value(), 100, 0.01);
    CHECK(events.size() == 5 && !events.back().second);
    // set_value snaps and does not call back.
    events.clear();
    s->step = 10;
    s->set_value(47);
    CHECK_NEAR(s->value(), 50, 0.01);
    CHECK(events.empty());
}

TEST(slider_vertical_has_minimum_at_the_bottom) {
    Stage st;
    auto* s = st.add<Slider>(Orientation::Vertical);
    st.finish();
    st.place(s, Rect::xywh(40, 20, 24, 160));
    CHECK_NEAR(s->track().t, 27, 0.01);
    CHECK_NEAR(s->track().b, 173, 0.01);
    st.rig.down({52, 173});
    CHECK_NEAR(s->value(), 0, 0.01);
    st.rig.move({52, 27});
    CHECK_NEAR(s->value(), 100, 0.01);
    st.rig.move({52, 100});
    CHECK_NEAR(s->value(), 50, 0.01);
    st.rig.up({52, 100});
    s->min = -1;
    s->max = 1;
    s->step = 0.5;
    st.rig.click({52, 173.0f - 146.0f * 0.7f});
    CHECK_NEAR(s->value(), 0.5, 1e-9);  // 0.7 of the way up, snapped to the half
}

// ---- progress -----------------------------------------------------------------------------------------

TEST(progress_bar_fill_and_indeterminate_sweep) {
    Stage st;
    auto* p = st.add<ProgressBar>();
    st.finish();
    st.place(p, Rect::xywh(20, 20, 200, 8));
    p->set_value(0.25);
    CHECK_NEAR(p->filled().r, 70, 0.01);
    p->set_value(7);
    CHECK_NEAR(p->value(), 1, 0.0001);
    CHECK_NEAR(p->filled().r, 220, 0.01);
    p->set_value(0.5);
    st.rig.render();
    const uint32_t track = Env::get().res->palette.argb(tok::ProgressTrack) & 0xFFFFFF;
    const uint32_t fill = Env::get().res->palette.argb(tok::ProgressIndicator) & 0xFFFFFF;
    CHECK_EQ(st.rig.bitmap().pixel(60, 24) & 0xFFFFFF, fill);
    CHECK_EQ(st.rig.bitmap().pixel(200, 24) & 0xFFFFFF, track);
    // Indeterminate: a segment 30% long, always inside the track, moving with the clock, ticked by a timer.
    CHECK(!st.rig.root.has_timers());
    p->set_indeterminate(true);
    st.rig.render();
    CHECK(st.rig.root.has_timers());
    float last_l = -1;
    bool moved = false;
    for (uint64_t t : {0u, 400u, 800u, 1200u, 1500u}) {
        st.rig.now = 100000 - (100000 % ProgressBar::kPeriodMs) + t;
        const Rect f = p->filled();
        CHECK(f.l >= 20 - 0.01 && f.r <= 220 + 0.01);
        CHECK(f.width() <= 60.01);
        if (t > 0 && f.l != last_l) moved = true;
        last_l = f.l;
    }
    CHECK(moved);
    st.rig.now = 100000 - (100000 % ProgressBar::kPeriodMs) + 800;  // halfway: 20 - 60 + 0.5 * 260
    CHECK_NEAR(p->filled().l, 90, 0.01);
    CHECK_NEAR(p->filled().r, 150, 0.01);
    p->set_indeterminate(false);
    CHECK(!st.rig.root.has_timers());
}

// ---- radio group ----------------------------------------------------------------------------------------

TEST(radio_group_click_keys_and_disabled_items) {
    Stage st;
    auto* g = st.add<RadioGroup>();
    g->set_items({L"Details", L"List", L"Tiles"});
    st.finish();
    st.place(g, Rect::xywh(20, 20, 160, 78));
    CHECK(g->item_rect(1) == Rect::xywh(20, 46, 160, 26));
    CHECK_EQ(g->item_at({30, 60}), 1);
    CHECK_EQ(g->item_at({30, 120}), -1);
    std::vector<int> changes;
    g->on_change = [&](int i) { changes.push_back(i); };
    CHECK_EQ(g->selected(), -1);
    st.rig.click({30, 85});
    CHECK_EQ(g->selected(), 2);
    st.rig.click({30, 85});  // choosing the chosen one again is not a change
    CHECK_EQ(changes.size(), 1);
    g->set_item_enabled(0, false);
    st.rig.click({30, 30});
    CHECK_EQ(g->selected(), 2);
    st.rig.key(VK_DOWN);  // wraps past the disabled first item
    CHECK_EQ(g->selected(), 1);
    st.rig.key(VK_UP);    // and again, backwards
    CHECK_EQ(g->selected(), 2);
    st.rig.key(VK_UP);
    CHECK_EQ(g->selected(), 1);
    CHECK(changes.size() == 4 && changes.back() == 1);
    g->set_selected(2);  // no callback
    CHECK_EQ(changes.size(), 4);
}

TEST(radio_group_horizontal_lays_out_in_a_row) {
    Stage st;
    auto* g = st.add<RadioGroup>();
    g->orientation = Orientation::Horizontal;
    g->set_items({L"One", L"Two", L"Three"});
    st.finish();
    const Size sz = g->measure_tree({1000, 100});
    CHECK(sz.w > 120 && sz.h == RadioGroup::kRow);
    st.place(g, Rect::xywh(10, 10, sz.w, sz.h));
    CHECK_NEAR(g->item_rect(1).l, g->item_rect(0).r, 0.01);
    CHECK_NEAR(g->item_rect(2).l, g->item_rect(1).r, 0.01);
    st.rig.click(Stage::mid(g->item_rect(1)));
    CHECK_EQ(g->selected(), 1);
}

// ---- combo box ------------------------------------------------------------------------------------------

TEST(combo_box_type_ahead_rule) {
    const std::vector<std::wstring> items{L"Apple", L"Banana", L"Avocado", L""};
    CHECK_EQ(ComboBox::next_starting_with(items, -1, L'a'), 0);
    CHECK_EQ(ComboBox::next_starting_with(items, 0, L'A'), 2);
    CHECK_EQ(ComboBox::next_starting_with(items, 2, L'a'), 0);
    CHECK_EQ(ComboBox::next_starting_with(items, 0, L'z'), -1);
}

TEST(combo_box_opens_its_list_and_chooses) {
    Stage st;
    auto* c = st.add<ComboBox>();
    c->set_items({L"Details", L"List", L"Tiles & icons", L"Content"});
    c->set_selected(1);
    st.finish();
    st.place(c, Rect::xywh(20, 20, 200, 28));
    std::vector<int> changes;
    c->on_change = [&](int i) { changes.push_back(i); };
    st.rig.click(Stage::mid(c->bounds));
    CHECK(c->list_open());
    CHECK(st.layer->menu_open());
    const auto& lv = st.layer->menus().levels()[0];
    CHECK_EQ(lv.items->size(), 4);
    CHECK_NEAR(lv.rect.l, 20, 0.01);
    CHECK_NEAR(lv.rect.t, 48, 0.01);
    CHECK(lv.rect.width() >= 200);
    CHECK((*lv.items)[1].checked && !(*lv.items)[0].checked);
    CHECK_EQ(st.layer->menus().selected(0), 1);  // opens on the current choice
    // An ampersand in an item is literal.
    CHECK_TEXT(parse_menu_label((*lv.items)[2].text).display, L"Tiles & icons");
    const Rect r2 = lv.panel->item_rect(2).offset(lv.rect.l, lv.rect.t);
    st.rig.click(Stage::mid(r2));
    CHECK(!c->list_open());
    CHECK(!st.layer->menu_open());
    CHECK_EQ(c->selected(), 2);
    CHECK(changes.size() == 1 && changes[0] == 2);
    // Escape closes without choosing.
    st.rig.key(VK_F4);
    CHECK(c->list_open());
    st.rig.key(VK_ESCAPE);
    CHECK(!c->list_open());
    CHECK_EQ(c->selected(), 2);
    // Closed, the keys change the choice; Alt+Down opens.
    st.rig.key(VK_UP);
    CHECK_EQ(c->selected(), 1);
    st.rig.key(VK_END);
    CHECK_EQ(c->selected(), 3);
    st.rig.key(VK_DOWN);
    CHECK_EQ(c->selected(), 3);
    st.rig.type(L"d");
    CHECK_EQ(c->selected(), 0);
    CHECK(!c->list_open());
    st.rig.root.key_down(VK_MENU);
    st.rig.root.key_down(VK_DOWN);
    st.rig.root.key_up(VK_DOWN);
    st.rig.root.key_up(VK_MENU);
    CHECK(c->list_open());
    // Pressing Enter on the highlighted item chooses it.
    st.rig.key(VK_DOWN);
    st.rig.key(VK_RETURN);
    CHECK_EQ(c->selected(), 1);
    CHECK(!c->list_open());
}

// ---- tab strip ------------------------------------------------------------------------------------------

namespace {
struct TabRig {
    Stage st;
    TabStrip* tabs = nullptr;
    std::vector<std::string> log;
    TabRig() {
        tabs = st.add<TabStrip>();
        st.finish();
        st.place(tabs, Rect::xywh(0, 0, 600, 34));
        for (uint64_t i = 1; i <= 4; ++i) tabs->add(Tab{L"Tab " + std::to_wstring(i), i * 10, nullptr, true}, i == 1);
        tabs->on_select = [this](int i) { log.push_back("select " + std::to_string(i)); };
        tabs->on_close = [this](int i) { log.push_back("close " + std::to_string(i)); };
        tabs->on_reorder = [this](int a, int b) { log.push_back("reorder " + std::to_string(a) + ">" + std::to_string(b)); };
        tabs->on_new = [this] { log.push_back("new"); };
    }
};
}  // namespace

TEST(tab_strip_widths_share_the_strip_within_limits) {
    TabRig t;
    CHECK_NEAR(t.tabs->tab_width(), (600 - TabStrip::kNewWidth) / 4, 0.01);
    CHECK_NEAR(t.tabs->tab_rect(1).l, t.tabs->tab_width(), 0.01);
    CHECK_NEAR(t.tabs->new_rect().l, 600 - TabStrip::kNewWidth, 0.01);
    for (uint64_t i = 5; i <= 12; ++i) t.tabs->add(Tab{L"More", i * 10, nullptr, true}, false);
    CHECK_NEAR(t.tabs->tab_width(), TabStrip::kMinTab, 0.01);  // never narrower than the minimum
    while (t.tabs->count() > 1) t.tabs->remove(1);
    CHECK_NEAR(t.tabs->tab_width(), TabStrip::kMaxTab, 0.01);  // nor wider than the maximum
    CHECK_NEAR(t.tabs->new_rect().l, TabStrip::kMaxTab, 0.01);
}

TEST(tab_strip_select_close_new_and_middle_click) {
    TabRig t;
    CHECK_EQ(t.tabs->selected(), 0);
    t.st.rig.click(Stage::mid(t.tabs->tab_rect(2)));
    CHECK_EQ(t.tabs->selected(), 2);
    CHECK(t.log.size() == 1 && t.log[0] == "select 2");
    t.st.rig.click(Stage::mid(t.tabs->tab_rect(2)));  // the selected tab again: not a change
    CHECK_EQ(t.log.size(), 1);
    // The close button asks; the owner removes.
    t.st.rig.click(Stage::mid(t.tabs->close_rect(2)));
    CHECK(t.log.size() == 2 && t.log[1] == "close 2");
    CHECK_EQ(t.tabs->count(), 4);
    t.tabs->remove(2);
    CHECK_EQ(t.tabs->count(), 3);
    CHECK_EQ(t.tabs->selected(), 2);  // the tab that slid into its place
    t.tabs->remove(2);
    CHECK_EQ(t.tabs->selected(), 1);  // the last tab went: its left neighbour
    // Middle click closes any closable tab, selected or not.
    t.log.clear();
    const Point p = Stage::mid(t.tabs->tab_rect(0));
    t.st.rig.root.mouse_down(p, MouseButton::Middle);
    t.st.rig.root.mouse_up(p, MouseButton::Middle);
    CHECK(t.log.size() == 1 && t.log[0] == "close 0");
    // A tab that is not closable has no close button: a press where it would be just selects the tab.
    t.tabs->add(Tab{L"Pinned", 99, nullptr, false});
    const int pinned = t.tabs->count() - 1;
    t.st.rig.click(Stage::mid(t.tabs->tab_rect(0)));
    t.log.clear();
    t.st.rig.click(Stage::mid(t.tabs->close_rect(pinned)));
    CHECK(t.log.size() == 1 && t.log[0] == "select 2");
    // The + button.
    t.log.clear();
    t.st.rig.click(Stage::mid(t.tabs->new_rect()));
    CHECK(t.log.size() == 1 && t.log[0] == "new");
}

TEST(tab_strip_drag_reorders_and_selection_follows) {
    TabRig t;
    const float w = t.tabs->tab_width();
    const Point a = Stage::mid(t.tabs->tab_rect(0));
    t.st.rig.down(a);
    t.st.rig.move({a.x + 3, a.y});  // under the threshold: still a click
    CHECK(!t.tabs->reordering());
    t.st.rig.move({a.x + w * 2, a.y});
    CHECK(t.tabs->reordering());
    CHECK(t.tabs->tab(2).id == 10);  // the first tab, now third
    CHECK(t.tabs->tab(0).id == 20);
    CHECK_EQ(t.tabs->selected(), 2);  // it was the selected one; the selection travelled with it
    t.st.rig.move({a.x + w * 3, a.y});
    CHECK(t.tabs->tab(3).id == 10);
    t.st.rig.up({a.x + w * 3, a.y});
    CHECK(!t.tabs->reordering());
    CHECK(t.log.size() == 1 && t.log[0] == "reorder 0>3");  // one report, for the whole gesture
    CHECK_EQ(t.tabs->index_of_id(10), 3);
    CHECK_EQ(t.tabs->selected(), 3);
    // A drag that comes back to where it started reports nothing.
    t.log.clear();
    const Point b = Stage::mid(t.tabs->tab_rect(1));
    t.st.rig.down(b);
    t.st.rig.move({b.x + w, b.y});
    t.st.rig.move({b.x, b.y});
    t.st.rig.up({b.x, b.y});
    CHECK(t.log.size() == 1 && t.log[0] == "select 1");
    // move() keeps the selection on its tab.
    t.tabs->select(1);
    t.tabs->move(0, 3);
    CHECK_EQ(t.tabs->selected(), 0);
    t.tabs->move(3, 0);
    CHECK_EQ(t.tabs->selected(), 1);
}

TEST(tab_strip_arrow_keys_select_neighbours) {
    TabRig t;
    t.st.rig.root.set_focus(t.tabs);
    t.st.rig.key(VK_RIGHT);
    t.st.rig.key(VK_RIGHT);
    CHECK_EQ(t.tabs->selected(), 2);
    t.st.rig.key(VK_END);
    t.st.rig.key(VK_RIGHT);
    CHECK_EQ(t.tabs->selected(), 3);
    t.st.rig.key(VK_HOME);
    CHECK_EQ(t.tabs->selected(), 0);
    CHECK_EQ(t.log.size(), 4);  // End then Right is one change, not two
}

// ---- calendar ---------------------------------------------------------------------------------------------

TEST(calendar_date_maths) {
    using namespace calendar;
    CHECK_EQ(days_from_civil({1970, 1, 1}), 0);
    CHECK_EQ(days_from_civil({2000, 3, 1}), 11017);
    CHECK_EQ(weekday({1970, 1, 1}), 4);    // Thursday
    CHECK_EQ(weekday({2026, 10, 1}), 4);   // Thursday
    CHECK_EQ(weekday({2026, 10, 28}), 3);  // Wednesday
    CHECK_EQ(days_in_month(2024, 2), 29);
    CHECK_EQ(days_in_month(2100, 2), 28);
    CHECK_EQ(days_in_month(2000, 2), 29);
    CHECK_EQ(days_in_month(2026, 12), 31);
    int bad = 0;
    for (int z = -800000; z < 800000; z += 37) {
        if (days_from_civil(civil_from_days(z)) != z) ++bad;
    }
    CHECK_EQ(bad, 0);
    CHECK(add_months({2026, 1, 31}, 1) == (Date{2026, 2, 28}));
    CHECK(add_months({2026, 1, 15}, -1) == (Date{2025, 12, 15}));
    CHECK(add_months({2026, 12, 5}, 1) == (Date{2027, 1, 5}));
    CHECK(add_months({2024, 2, 29}, 12) == (Date{2025, 2, 28}));
    CHECK(add_months({2026, 3, 10}, -27) == (Date{2023, 12, 10}));
    CHECK(add_days({2026, 12, 31}, 1) == (Date{2027, 1, 1}));
    // October 2026 starts on a Thursday: three leading days of September for a Monday-first week, four for Sunday.
    CHECK(cell(2026, 10, 0, 0, 1) == (Date{2026, 9, 28}));
    CHECK(cell(2026, 10, 0, 3, 1) == (Date{2026, 10, 1}));
    CHECK(cell(2026, 10, 0, 0, 0) == (Date{2026, 9, 27}));
    CHECK(cell(2026, 10, 5, 6, 1) == (Date{2026, 11, 8}));
}

TEST(calendar_clicks_and_keys) {
    Stage st;
    auto* c = st.add<Calendar>();
    st.finish();
    st.place(c, Rect::xywh(10, 10, 254, 258));
    c->first_day_of_week = 1;
    c->today = {2026, 10, 28};
    c->set_selected({2026, 10, 15});
    CHECK_EQ(c->shown_month(), 10);
    std::vector<Date> picked;
    std::vector<std::pair<int, int>> months;
    c->on_select = [&](Date d) { picked.push_back(d); };
    c->on_month_changed = [&](int y, int m) { months.emplace_back(y, m); };
    st.rig.click(Stage::mid(c->cell_rect(0, 3)));
    CHECK(picked.size() == 1 && picked[0] == (Date{2026, 10, 1}));
    CHECK(c->selected() == (Date{2026, 10, 1}));
    // A day of the neighbouring month selects it and turns to its month.
    st.rig.click(Stage::mid(c->cell_rect(0, 0)));
    CHECK(c->selected() == (Date{2026, 9, 28}));
    CHECK_EQ(c->shown_month(), 9);
    CHECK(months.size() == 1 && months[0] == std::make_pair(2026, 9));
    st.rig.click(Stage::mid(c->next_rect()));
    st.rig.click(Stage::mid(c->next_rect()));
    CHECK(c->shown_year() == 2026 && c->shown_month() == 11);
    st.rig.click(Stage::mid(c->prev_rect()));
    st.rig.click(Stage::mid(c->prev_rect()));
    st.rig.click(Stage::mid(c->prev_rect()));
    CHECK(c->shown_year() == 2026 && c->shown_month() == 8);
    // Keys move the selection and the shown month follows it.
    c->set_selected({2026, 10, 31});
    st.rig.root.set_focus(c);
    st.rig.key(VK_RIGHT);
    CHECK(c->selected() == (Date{2026, 11, 1}));
    CHECK_EQ(c->shown_month(), 11);
    st.rig.key(VK_UP);
    CHECK(c->selected() == (Date{2026, 10, 25}));
    st.rig.key(VK_NEXT);
    CHECK(c->selected() == (Date{2026, 11, 25}));
    st.rig.key(VK_END);
    CHECK(c->selected() == (Date{2026, 11, 30}));
    st.rig.key(VK_HOME);
    CHECK(c->selected() == (Date{2026, 11, 1}));
    st.rig.key(VK_PRIOR, false, true);  // Ctrl+PageUp: a year back
    CHECK(c->selected() == (Date{2025, 11, 1}));
}

// ---- colour picker ----------------------------------------------------------------------------------------

TEST(colour_conversions_round_trip_exactly) {
    CHECK(near_colour(0xFF000000 | hsv_to_rgb({0, 1, 1}), 255, 0, 0, 0));
    const Hsv red = rgb_to_hsv(0xFF0000), green = rgb_to_hsv(0x00FF00), blue = rgb_to_hsv(0x0000FF), grey = rgb_to_hsv(0x808080);
    CHECK_NEAR(red.h, 0, 0.01);
    CHECK_NEAR(green.h, 120, 0.01);
    CHECK_NEAR(blue.h, 240, 0.01);
    CHECK_NEAR(grey.s, 0, 0.0001);
    CHECK_NEAR(grey.v, 128.0 / 255.0, 0.0001);
    int bad = 0;
    for (uint32_t r = 0; r < 256; r += 5) {
        for (uint32_t g = 0; g < 256; g += 5) {
            for (uint32_t b = 0; b < 256; b += 5) {
                const uint32_t rgb = (r << 16) | (g << 8) | b;
                if (hsv_to_rgb(rgb_to_hsv(rgb)) != rgb) ++bad;
            }
        }
    }
    CHECK_EQ(bad, 0);
}

TEST(colour_picker_square_strip_and_drag) {
    Stage st;
    auto* p = st.add<ColorPicker>();
    st.finish();
    st.place(p, Rect::xywh(10, 10, 262, 212));
    std::vector<std::pair<uint32_t, bool>> events;
    p->on_change = [&](uint32_t rgb, bool d) { events.emplace_back(rgb, d); };
    const Rect sq = p->square(), strip = p->strip();
    st.rig.down(Stage::mid(sq));
    CHECK_NEAR(p->hsv().s, 0.5, 0.01);
    CHECK_NEAR(p->hsv().v, 0.5, 0.01);
    st.rig.move({sq.r + 50, sq.b + 50});  // dragged beyond the corner: clamped
    CHECK_NEAR(p->hsv().s, 1, 0.0001);
    CHECK_NEAR(p->hsv().v, 0, 0.0001);
    CHECK_EQ(p->rgb(), 0u);
    st.rig.up({sq.r + 50, sq.b + 50});
    CHECK(!events.empty() && !events.back().second);
    st.rig.click({(strip.l + strip.r) / 2, strip.t + strip.height() / 2});
    CHECK_NEAR(p->hsv().h, 180, 0.5);
    st.rig.click({(strip.l + strip.r) / 2, strip.t});
    CHECK_NEAR(p->hsv().h, 0, 0.01);
    // Setting a grey keeps the hue on show; the hex text follows.
    p->set_hsv({200, 0.5f, 0.5f});
    p->set_rgb(0x808080);
    CHECK_NEAR(p->hsv().h, 200, 0.01);
    p->set_rgb(0x12AB9F);
    CHECK_TEXT(p->hex_text(), L"#12AB9F");
    CHECK_EQ(p->rgb(), 0x12AB9Fu);
    // Keys nudge.
    p->set_hsv({0, 0.5f, 0.5f});
    st.rig.root.set_focus(p);
    st.rig.key(VK_RIGHT);
    CHECK_NEAR(p->hsv().s, 0.51, 0.0001);
    st.rig.key(VK_UP, true);
    CHECK_NEAR(p->hsv().v, 0.55, 0.0001);
}

TEST(colour_picker_paints_the_hsv_square) {
    Stage st;
    auto* p = st.add<ColorPicker>();
    st.finish();
    st.place(p, Rect::xywh(10, 10, 262, 212));
    p->set_rgb(0xFF0000);
    st.rig.render();
    auto& bmp = st.rig.bitmap();
    const Rect sq = p->square();
    // Top-left white, top-right the pure hue, bottom black.
    CHECK(near_colour(bmp.pixel(static_cast<UINT>(sq.l + 2), static_cast<UINT>(sq.t + 2)), 255, 255, 255, 12));
    CHECK(near_colour(bmp.pixel(static_cast<UINT>(sq.r - 3), static_cast<UINT>(sq.t + 2)), 255, 0, 0, 12));
    CHECK(near_colour(bmp.pixel(static_cast<UINT>(sq.l + 2), static_cast<UINT>(sq.b - 3)), 0, 0, 0, 12));
    CHECK(near_colour(bmp.pixel(static_cast<UINT>(sq.r - 3), static_cast<UINT>(sq.b - 3)), 0, 0, 0, 12));
    // The strip runs through the spectrum: top red, a third of the way down green.
    const Rect strip = p->strip();
    const UINT sx = static_cast<UINT>((strip.l + strip.r) / 2);
    CHECK(near_colour(bmp.pixel(sx, static_cast<UINT>(strip.t + strip.height() / 3)), 0, 255, 0, 30));
    CHECK(near_colour(bmp.pixel(sx, static_cast<UINT>(strip.t + strip.height() * 2 / 3)), 0, 0, 255, 30));
    // The swatch shows the colour.
    CHECK(near_colour(bmp.pixel(static_cast<UINT>(p->swatch().l + 10), static_cast<UINT>(p->swatch().t + 10)), 255, 0, 0, 2));
}

// ---- the gallery --------------------------------------------------------------------------------------

TEST(widget_set_gallery_png) {
    Stage st(900, 520);
    st.root->style.bg = tok::WindowBackground;
    auto* tabs = st.add<TabStrip>();
    auto* cal = st.add<Calendar>();
    auto* pick = st.add<ColorPicker>();
    auto* slider = st.add<Slider>();
    auto* vslider = st.add<Slider>(Orientation::Vertical);
    auto* bar = st.add<ProgressBar>();
    auto* spin = st.add<ProgressBar>();
    auto* radio = st.add<RadioGroup>();
    auto* combo = st.add<ComboBox>();
    auto* label = st.add<TextBlock>(L"Slider, progress, radio group and combo box");
    radio->set_items({L"Details", L"List", L"Tiles"});
    combo->set_items({L"Name", L"Size", L"Modified", L"Created"});
    combo->set_selected(2);
    st.finish();
    static const IconData* const kIcons[] = {&icons::Folder, &icons::Home, &icons::Download, &icons::Desktop};
    const wchar_t* titles[] = {L"Documents", L"Projects", L"A tab with a rather long title", L"Downloads"};
    for (int i = 0; i < 4; ++i) tabs->add(Tab{titles[i], static_cast<uint64_t>(i + 1), kIcons[i], i != 3}, i == 1);
    st.place(tabs, Rect::xywh(0, 0, 900, 34));
    cal->first_day_of_week = 1;
    cal->today = {2026, 10, 28};
    cal->set_selected({2026, 10, 15});
    st.place(cal, Rect::xywh(16, 52, 254, 258));
    pick->set_rgb(0x2E8BC0);
    st.place(pick, Rect::xywh(290, 52, 262, 212));
    st.place(label, Rect::xywh(580, 52, 300, 20));
    slider->set_value(35);
    st.place(slider, Rect::xywh(580, 82, 240, 24));
    vslider->set_value(70);
    st.place(vslider, Rect::xywh(850, 52, 24, 120));
    bar->set_value(0.62);
    st.place(bar, Rect::xywh(580, 122, 240, 8));
    spin->set_indeterminate(true);
    st.rig.now = 100000 - (100000 % ProgressBar::kPeriodMs) + 700;
    st.place(spin, Rect::xywh(580, 142, 240, 8));
    radio->set_selected(1);
    st.place(radio, Rect::xywh(580, 166, 160, 78));
    st.place(combo, Rect::xywh(580, 262, 200, 28));
    st.rig.root.set_focus(slider);
    st.rig.root.mouse_move({590, 270});
    combo->open_list();  // photographed open, below the combo
    st.layer->show_tip(L"Sort by this column", {400, 300});
    st.rig.render();
    CHECK(st.rig.bitmap().count_non_background() > 30000);
    CHECK(st.layer->menu_open());
    CHECK(st.rig.bitmap().write_png(exe_dir() + L"widget_set.png"));
    st.layer->close_menu();
}

namespace {
struct DeathFlag : Widget {
    bool* flag;
    explicit DeathFlag(bool* f) : flag(f) {}
    ~DeathFlag() override { *flag = true; }
};
}  // namespace

// The tree belongs to the window's root, which a host may outlive the services the widgets call as they go
// (the preview pane closes its GIF through the core): clear_content is how it dies first.
TEST(clear_content_destroys_the_tree_before_the_window) {
    Rig rig(200, 100);
    bool died = false;
    rig.set(std::make_unique<DeathFlag>(&died));
    CHECK(!died);
    rig.root.clear_content();
    CHECK(died);
    CHECK(rig.root.content() == nullptr);
}
