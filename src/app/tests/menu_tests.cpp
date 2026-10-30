#include "fixture.h"
#include "gfx/icons.gen.h"
#include "platform/popup_window.h"
#include "ui/menu_bar.h"

#include <string>
#include <vector>

using namespace bb;
using namespace bb::ui;

namespace {

// A window whose whole client area is a button (so "did the click reach what is underneath" is a
// question about one counter) with a PopupLayer over it.
struct MenuRig {
    ThemeScope theme;
    Rig rig{900, 600};
    DockPanel* content = nullptr;
    PopupLayer* layer = nullptr;
    Button* under = nullptr;
    MenuBar* bar = nullptr;
    int under_clicks = 0;
    std::vector<std::string> log;

    explicit MenuRig(bool with_bar = false) {
        auto root = std::make_unique<DockPanel>();
        if (with_bar) {
            bar = root->emplace<MenuBar>();
            bar->dock = Dock::Top;
        }
        under = root->emplace<Button>(L"Underneath");
        under->on_click = [this] { ++under_clicks; };
        content = rig.set(std::move(root));
        layer = PopupLayer::install(*content);
        rig.relayout();
    }

    // 0 New, 1 Open, 2 ---, 3 Recent >, 4 Save (disabled), 5 ---, 6 Exit
    MenuItems file_menu() {
        MenuItems m;
        auto add = [&](const wchar_t* text, const char* tag, const wchar_t* gesture) {
            m.push_back(MenuItem::make(text, [this, tag] { log.push_back(tag); }, gesture));
        };
        add(L"&New", "new", L"Ctrl+N");
        add(L"&Open", "open", L"Ctrl+O");
        m.push_back(MenuItem::separator());
        MenuItem recent = MenuItem::make(L"&Recent");
        for (const char* tag : {"r1", "r2", "r3"}) {
            recent.sub.push_back(MenuItem::make(std::wstring(L"Folder ") + std::wstring(tag, tag + 2), [this, tag] { log.push_back(tag); }));
        }
        m.push_back(std::move(recent));
        MenuItem save = MenuItem::make(L"&Save", [this] { log.push_back("save"); }, L"Ctrl+S");
        save.enabled = false;
        m.push_back(std::move(save));
        m.push_back(MenuItem::separator());
        add(L"E&xit", "exit", L"Alt+F4");
        return m;
    }

    Point item_center(size_t level, int index) {
        const auto& lv = layer->menus().levels()[level];
        const Rect r = lv.panel->item_rect(index).offset(lv.rect.l, lv.rect.t);
        return {(r.l + r.r) / 2, (r.t + r.b) / 2};
    }
    void open(const MenuItems& items, MenuOpen o = {}) {
        o.anchor = Rect{40, 30, 100, 30};
        layer->open_menu(items, std::move(o));
    }
    void wait(unsigned ms) {
        rig.now += ms;
        rig.root.tick();
    }
};

}  // namespace

// ---- pure rules --------------------------------------------------------------------------------------

TEST(menu_labels_and_access_keys) {
    MenuLabel a = parse_menu_label(L"&Open");
    CHECK_TEXT(a.display, L"Open");
    CHECK_EQ(a.key_index, 0);
    CHECK_EQ(a.key, L'o');
    MenuLabel b = parse_menu_label(L"Save &As");
    CHECK_TEXT(b.display, L"Save As");
    CHECK_EQ(b.key_index, 5);
    CHECK_EQ(b.key, L'a');
    MenuLabel c = parse_menu_label(L"Fish && Chips");
    CHECK_TEXT(c.display, L"Fish & Chips");
    CHECK_EQ(c.key_index, -1);
    MenuLabel d = parse_menu_label(L"Ben &&&Jerry");
    CHECK_TEXT(d.display, L"Ben &Jerry");
    CHECK_EQ(d.key, L'j');
    MenuLabel e = parse_menu_label(L"Trailing&");
    CHECK_TEXT(e.display, L"Trailing");
    CHECK_EQ(e.key_index, -1);
    CHECK_EQ(parse_menu_label(L"&X &Y").key, L'x');  // the first marker wins
}

TEST(menu_selection_helpers) {
    MenuRig m;
    const MenuItems items = m.file_menu();
    CHECK_EQ(menu_first_selectable(items), 0);
    CHECK_EQ(menu_last_selectable(items), 6);
    CHECK_EQ(menu_next_selectable(items, 1, 1), 3);   // skips the separator
    CHECK_EQ(menu_next_selectable(items, 3, 1), 6);   // skips the disabled item and the separator
    CHECK_EQ(menu_next_selectable(items, 6, 1), 0);   // wraps
    CHECK_EQ(menu_next_selectable(items, 0, -1), 6);
    CHECK_EQ(menu_next_selectable({}, -1, 1), -1);
    const auto hits = menu_access_matches(items, L'O');
    CHECK_EQ(hits.size(), 1);
    CHECK_EQ(hits[0], 1);
    CHECK_EQ(menu_access_matches(items, L's').size(), 0);  // Save is disabled
}

TEST(popup_placement_flips_and_clamps) {
    const Rect work{0, 0, 800, 600};
    // Below the anchor, left edges aligned.
    Rect r = place_popup({200, 150}, Rect{100, 20, 160, 40}, PopupSide::Below, work);
    CHECK(r == Rect::xywh(100, 40, 200, 150));
    // Too far right: pushed back inside.
    r = place_popup({200, 150}, Rect{700, 20, 760, 40}, PopupSide::Below, work);
    CHECK_NEAR(r.r, 800, 0.01);
    // No room below: flips above the anchor.
    r = place_popup({200, 150}, Rect{100, 500, 160, 520}, PopupSide::Below, work);
    CHECK_NEAR(r.b, 500, 0.01);
    // No room either side: pinned to the bottom edge.
    r = place_popup({200, 450}, Rect{100, 200, 160, 220}, PopupSide::Below, work);
    CHECK_NEAR(r.b, 600, 0.01);
    // A submenu goes beside its parent item, overlapping by 2.
    r = place_popup({160, 100}, Rect{100, 60, 300, 86}, PopupSide::Beside, work);
    CHECK_NEAR(r.l, 298, 0.01);
    CHECK_NEAR(r.t, 55, 0.01);
    // ...and flips to the left when it would leave the work area.
    r = place_popup({160, 100}, Rect{600, 60, 780, 86}, PopupSide::Beside, work);
    CHECK_NEAR(r.r, 602, 0.01);
    // Bigger than the work area: pinned to its top-left, never off the top.
    r = place_popup({900, 700}, Rect{100, 20, 160, 40}, PopupSide::Below, work);
    CHECK_NEAR(r.l, 0, 0.01);
    CHECK_NEAR(r.t, 0, 0.01);
}

// ---- opening, hovering, invoking ---------------------------------------------------------------------

TEST(menu_opens_modal_and_swallows_clicks) {
    MenuRig m;
    const MenuItems items = m.file_menu();
    m.open(items);
    CHECK(m.layer->menu_open());
    CHECK(m.rig.root.modal() == m.layer);
    CHECK_EQ(m.layer->menus().levels().size(), 1);
    const Rect r = m.layer->menus().levels()[0].rect;
    CHECK_NEAR(r.l, 40, 0.01);
    CHECK_NEAR(r.t, 30, 0.01);  // below the anchor's bottom edge
    CHECK(r.width() >= 120 && r.height() > 100);
    // A click outside closes the menu and does not reach the button underneath.
    m.rig.click({700, 500});
    CHECK(!m.layer->menu_open());
    CHECK(m.rig.root.modal() == nullptr);
    CHECK_EQ(m.under_clicks, 0);
    CHECK(m.log.empty());
    // With the menu gone the window is ordinary again.
    m.rig.click({700, 500});
    CHECK_EQ(m.under_clicks, 1);
}

TEST(menu_hover_selects_and_click_invokes) {
    MenuRig m;
    const MenuItems items = m.file_menu();
    MenuClose closed_with = MenuClose::Escape;
    bool closed = false;
    MenuOpen o;
    o.on_closed = [&](MenuClose why) {
        closed = true;
        closed_with = why;
    };
    m.open(items, std::move(o));
    m.rig.move(m.item_center(0, 1));
    CHECK_EQ(m.layer->menus().selected(0), 1);
    m.rig.move(m.item_center(0, 0));
    CHECK_EQ(m.layer->menus().selected(0), 0);
    // A separator keeps the selection; a disabled item clears it.
    m.rig.move(m.item_center(0, 2));
    CHECK_EQ(m.layer->menus().selected(0), 0);
    m.rig.move(m.item_center(0, 4));
    CHECK_EQ(m.layer->menus().selected(0), -1);
    // Clicking a disabled item does nothing and leaves the menu up.
    m.rig.click(m.item_center(0, 4));
    CHECK(m.layer->menu_open());
    CHECK(m.log.empty());
    // Clicking an enabled item runs it, after the menu has closed.
    m.rig.click(m.item_center(0, 1));
    CHECK(!m.layer->menu_open());
    CHECK(closed);
    CHECK(closed_with == MenuClose::Invoked);
    CHECK_EQ(m.log.size(), 1);
    CHECK(m.log[0] == "open");
    CHECK_EQ(m.under_clicks, 0);
}

TEST(menu_submenu_opens_after_hover_delay_and_closes_on_leaving) {
    MenuRig m;
    const MenuItems items = m.file_menu();
    m.open(items);
    m.rig.move(m.item_center(0, 3));  // Recent >
    CHECK_EQ(m.layer->menus().levels().size(), 1);  // not yet: the pointer may be passing through
    m.wait(100);
    CHECK_EQ(m.layer->menus().levels().size(), 1);
    m.wait(200);
    CHECK_EQ(m.layer->menus().levels().size(), 2);
    const auto& l0 = m.layer->menus().levels()[0];
    const auto& l1 = m.layer->menus().levels()[1];
    CHECK_NEAR(l1.rect.l, l0.rect.r - 2, 0.01);
    // Into the submenu and click an item of it.
    m.rig.move(m.item_center(1, 2));
    CHECK_EQ(m.layer->menus().selected(1), 2);
    // Back to a plain parent item: the submenu goes away at once.
    m.rig.move(m.item_center(0, 1));
    CHECK_EQ(m.layer->menus().levels().size(), 1);
    // Clicking a submenu item invokes it and closes everything.
    m.rig.move(m.item_center(0, 3));
    m.wait(300);
    CHECK_EQ(m.layer->menus().levels().size(), 2);
    m.rig.click(m.item_center(1, 1));
    CHECK(!m.layer->menu_open());
    CHECK_EQ(m.log.size(), 1);
    CHECK(m.log[0] == "r2");
}

TEST(menu_click_on_submenu_item_opens_it_immediately) {
    MenuRig m;
    const MenuItems items = m.file_menu();
    m.open(items);
    m.rig.click(m.item_center(0, 3));
    CHECK(m.layer->menu_open());
    CHECK_EQ(m.layer->menus().levels().size(), 2);
    CHECK(m.log.empty());
}

TEST(menu_press_drag_release_invokes_like_a_menu_bar) {
    MenuRig m;
    const MenuItems items = m.file_menu();
    MenuOpen o;
    o.button_down = true;  // opened by a press that is still held
    m.open(items, std::move(o));
    const Point p = m.item_center(0, 0);
    m.rig.move(p);
    m.rig.up(p);
    CHECK(!m.layer->menu_open());
    CHECK_EQ(m.log.size(), 1);
    CHECK(m.log[0] == "new");
}

// ---- keyboard -----------------------------------------------------------------------------------------

TEST(menu_keyboard_navigation) {
    MenuRig m;
    const MenuItems items = m.file_menu();
    m.open(items);
    CHECK_EQ(m.layer->menus().selected(0), -1);
    m.rig.key(VK_DOWN);
    CHECK_EQ(m.layer->menus().selected(0), 0);
    m.rig.key(VK_DOWN);
    CHECK_EQ(m.layer->menus().selected(0), 1);
    m.rig.key(VK_DOWN);
    CHECK_EQ(m.layer->menus().selected(0), 3);  // over the separator
    m.rig.key(VK_DOWN);
    CHECK_EQ(m.layer->menus().selected(0), 6);  // over the disabled item and the next separator
    m.rig.key(VK_DOWN);
    CHECK_EQ(m.layer->menus().selected(0), 0);  // wraps
    m.rig.key(VK_UP);
    CHECK_EQ(m.layer->menus().selected(0), 6);
    m.rig.key(VK_HOME);
    CHECK_EQ(m.layer->menus().selected(0), 0);
    m.rig.key(VK_END);
    CHECK_EQ(m.layer->menus().selected(0), 6);
    // Right opens a submenu with its first item selected; Left closes it.
    m.rig.key(VK_UP);
    CHECK_EQ(m.layer->menus().selected(0), 3);
    m.rig.key(VK_RIGHT);
    CHECK_EQ(m.layer->menus().levels().size(), 2);
    CHECK_EQ(m.layer->menus().selected(1), 0);
    m.rig.key(VK_DOWN);
    CHECK_EQ(m.layer->menus().selected(1), 1);
    m.rig.key(VK_LEFT);
    CHECK_EQ(m.layer->menus().levels().size(), 1);
    // Enter on a leaf runs it.
    m.rig.key(VK_HOME);
    m.rig.key(VK_RETURN);
    CHECK(!m.layer->menu_open());
    CHECK_EQ(m.log.size(), 1);
    CHECK(m.log[0] == "new");
}

TEST(menu_escape_steps_back_then_closes) {
    MenuRig m;
    const MenuItems items = m.file_menu();
    MenuClose why = MenuClose::Invoked;
    MenuOpen o;
    o.on_closed = [&](MenuClose w) { why = w; };
    m.open(items, std::move(o));
    m.rig.key(VK_DOWN);
    m.rig.key(VK_DOWN);
    m.rig.key(VK_DOWN);
    m.rig.key(VK_RIGHT);
    CHECK_EQ(m.layer->menus().levels().size(), 2);
    m.rig.key(VK_ESCAPE);
    CHECK_EQ(m.layer->menus().levels().size(), 1);
    CHECK(m.layer->menu_open());
    m.rig.key(VK_ESCAPE);
    CHECK(!m.layer->menu_open());
    CHECK(why == MenuClose::Escape);
    CHECK(m.rig.root.modal() == nullptr);
}

TEST(menu_access_keys_invoke_select_and_open) {
    MenuRig m;
    const MenuItems items = m.file_menu();
    m.open(items);
    m.rig.type(L"s");  // Save is disabled: no match, the menu stays
    CHECK(m.layer->menu_open());
    m.rig.type(L"R");  // case-insensitive; a submenu opens with its first item selected
    CHECK_EQ(m.layer->menus().levels().size(), 2);
    CHECK_EQ(m.layer->menus().selected(1), 0);
    m.rig.type(L"x");  // not an access key of the submenu's items
    CHECK(m.layer->menu_open());
    m.rig.key(VK_LEFT);
    m.rig.type(L"x");
    CHECK(!m.layer->menu_open());
    CHECK_EQ(m.log.size(), 1);
    CHECK(m.log[0] == "exit");
}

TEST(menu_shared_access_key_cycles_before_invoking) {
    MenuRig m;
    MenuItems items;
    for (const char* tag : {"a1", "b", "a2", "a3"}) {
        const bool a = tag[0] == 'a';
        items.push_back(MenuItem::make(a ? L"&Alpha" : L"&Beta", [&m, tag] { m.log.push_back(tag); }));
    }
    m.open(items);
    m.rig.type(L"a");
    CHECK_EQ(m.layer->menus().selected(0), 0);
    CHECK(m.layer->menu_open());
    m.rig.type(L"a");
    CHECK_EQ(m.layer->menus().selected(0), 2);
    m.rig.type(L"a");
    CHECK_EQ(m.layer->menus().selected(0), 3);
    m.rig.type(L"a");
    CHECK_EQ(m.layer->menus().selected(0), 0);  // wraps round the matches
    m.rig.key(VK_RETURN);
    CHECK(m.log.size() == 1 && m.log[0] == "a1");
}

// ---- rendering ---------------------------------------------------------------------------------------

TEST(menu_renders_in_tree_with_icons_checks_and_states) {
    MenuRig m;
    MenuItems items = m.file_menu();
    items[0].icon = &icons::NewFile;
    items[1].icon = &icons::Open;
    items[1].checked = true;  // an icon with a check: framed
    items[3].icon = &icons::Folder;
    items[4].icon = &icons::Save;  // disabled
    items[6].checked = true;       // a plain check mark
    MenuItem radio = MenuItem::make(L"&Radio on");
    radio.checked = radio.radio = true;
    items.insert(items.begin() + 6, std::move(radio));
    m.open(items);
    m.rig.move(m.item_center(0, 1));
    m.rig.render();
    auto& bmp = m.rig.bitmap();
    const MenuController::Level lv = m.layer->menus().levels()[0];  // a copy: opening the submenu reallocates
    const uint32_t menu_bg = Env::get().res->palette.argb(tok::MenuBackground) & 0xFFFFFF;
    const uint32_t hover_bg = Env::get().res->palette.argb(tok::MenuHoverBackground) & 0xFFFFFF;
    const Point unsel = m.item_center(0, 0), sel = m.item_center(0, 1);
    CHECK_EQ(bmp.pixel(static_cast<UINT>(lv.rect.l + 6), static_cast<UINT>(unsel.y)) & 0xFFFFFF, menu_bg);
    CHECK_EQ(bmp.pixel(static_cast<UINT>(lv.rect.l + 6), static_cast<UINT>(sel.y)) & 0xFFFFFF, hover_bg);
    // Outside the menu is the window, not the menu colour.
    CHECK(bmp.pixel(600, 400) != bmp.pixel(static_cast<UINT>(lv.rect.l + 6), static_cast<UINT>(unsel.y)));
    // Open the submenu too, and photograph the lot.
    m.rig.move(m.item_center(0, 3));
    m.wait(300);
    CHECK_EQ(m.layer->menus().levels().size(), 2);
    m.rig.render();
    const Rect u = lv.rect.united(m.layer->menus().levels()[1].rect).inflate(10);
    const RECT px{static_cast<LONG>(u.l), static_cast<LONG>(u.t), static_cast<LONG>(u.r), static_cast<LONG>(u.b)};
    CHECK(bmp.write_png(exe_dir() + L"menu_open.png", &px));
}

TEST(menu_renders_detached_without_any_window) {
    MenuRig m;
    MenuItems items = m.file_menu();
    items[0].icon = &icons::NewFile;
    items[1].icon = &icons::Open;
    items[3].icon = &icons::Folder;
    items[4].icon = &icons::Save;
    items[6].checked = true;
    auto surf = render_menu_detached(*Env::get().res, items, {3, 1});
    CHECK(surf != nullptr);
    if (!surf) return;
    CHECK(surf->width_px() > 200 && surf->width_px() < 700);
    CHECK(surf->height_px() > 150 && surf->height_px() < 450);
    CHECK(surf->count_non_background() > 5000);
    CHECK(surf->write_png(exe_dir() + L"menu_detached.png"));
    // The same menu at 200% DPI is twice the pixels.
    auto big = render_menu_detached(*Env::get().res, items, {3, 1}, 192.0f);
    CHECK(big != nullptr);
    if (big) CHECK(std::abs(static_cast<int>(big->width_px()) - 2 * static_cast<int>(surf->width_px())) <= 2);
}

// ---- popup windows ------------------------------------------------------------------------------------

TEST(menu_in_owned_popup_windows_parked) {
    MenuRig m;
    platform::WindowPresenter wp(*Env::get().res, m.rig.win.hwnd(), true, m.rig.win.dpi());
    m.layer->set_presenter(&wp);
    const MenuItems items = m.file_menu();
    m.open(items);
    CHECK_EQ(wp.windows().size(), 1);
    CHECK(m.rig.root.modal() == m.layer);
    // The popup is a window of its own, owned by the main one, nowhere near a screen.
    platform::PopupWindow* w = wp.windows()[0].get();
    CHECK(w->ok());
    CHECK(GetWindow(w->hwnd(), GW_OWNER) == m.rig.win.hwnd());
    CHECK((GetWindowLongW(w->hwnd(), GWL_EXSTYLE) & WS_EX_NOACTIVATE) != 0);
    const RECT sr = w->screen_rect();
    CHECK(sr.left < -20000 && sr.top < -20000);
    CHECK(w->bitmap() && w->bitmap()->count_non_background() > 2000);
    // Input goes through the layer, so a hover in owner coordinates drives the popup's panel.
    m.rig.move(m.item_center(0, 3));
    CHECK_EQ(m.layer->menus().selected(0), 3);
    m.wait(300);
    CHECK_EQ(wp.windows().size(), 2);
    CHECK_EQ(m.layer->menus().levels().size(), 2);
    CHECK(wp.windows()[1]->bitmap()->count_non_background() > 1000);
    // Placement is in owner space even though the window is not: the submenu abuts the parent.
    CHECK_NEAR(m.layer->menus().levels()[1].rect.l, m.layer->menus().levels()[0].rect.r - 2, 0.01);
    wp.windows()[0]->bitmap()->write_png(exe_dir() + L"popup_window.png");
    m.rig.click(m.item_center(1, 0));
    CHECK(wp.windows().empty());
    CHECK(!m.layer->menu_open());
    CHECK_EQ(m.log.size(), 1);
    m.layer->set_presenter(nullptr);
}

// ---- tooltips -----------------------------------------------------------------------------------------

TEST(tooltip_appears_after_rest_and_hides_on_leave_or_press) {
    MenuRig m;
    m.under->tooltip = L"Does the underneath thing";
    CHECK(!m.layer->tip_visible());
    m.rig.move({300, 200});
    m.wait(200);
    CHECK(!m.layer->tip_visible());
    m.wait(400);
    CHECK(m.layer->tip_visible());
    CHECK_TEXT(m.layer->tip_text(), L"Does the underneath thing");
    const Rect tr = m.layer->tip_rect();
    CHECK(tr.l >= 299 && tr.t >= 200 && tr.r <= 900 && tr.b <= 600);
    m.rig.render();
    CHECK(m.rig.bitmap().pixel(static_cast<UINT>(tr.l + 1), static_cast<UINT>(tr.t + 1)) !=
          m.rig.bitmap().pixel(static_cast<UINT>(tr.l - 5), static_cast<UINT>(tr.t - 5)));
    // A press dismisses it and it does not come back until the pointer moves to something again.
    m.rig.down({300, 200});
    CHECK(!m.layer->tip_visible());
    m.rig.up({300, 200});
    m.wait(1000);
    CHECK(!m.layer->tip_visible());
    // Moving away before the delay means no tooltip.
    m.rig.move({310, 210});
    m.rig.root.mouse_leave();
    m.wait(1000);
    CHECK(!m.layer->tip_visible());
    // Nothing is shown over an open menu.
    const MenuItems items = m.file_menu();
    m.open(items);
    m.wait(1000);
    CHECK(!m.layer->tip_visible());
}

// ---- the menu bar -------------------------------------------------------------------------------------

namespace {

struct BarRig : MenuRig {
    MenuItems file, edit, view;
    std::vector<bool> mode_log;
    int opened = 0;

    BarRig() : MenuRig(true) {
        file = file_menu();
        edit.push_back(MenuItem::make(L"&Undo", [this] { log.push_back("undo"); }));
        edit.push_back(MenuItem::make(L"&Redo", [this] { log.push_back("redo"); }));
        view.push_back(MenuItem::make(L"&Thumbnails", [this] { log.push_back("thumbs"); }));
        MenuBarEntry f{L"&File", file, [this](MenuItems&) { ++opened; }};
        bar->set_menus({f, {L"&Edit", edit, {}}, {L"&View", view, {}}});
        bar->on_mode_changed = [this](bool on) { mode_log.push_back(on); };
        bar->hook(rig.root);
        rig.root.on_preview_key = [this](const KeyEvent& e) { return bar->preview_key(e); };
        rig.relayout();
    }
    Point heading(int i) {
        const Rect r = bar->heading_rect(i);
        return {(r.l + r.r) / 2, (r.t + r.b) / 2};
    }
    void tap_alt() {
        rig.root.key_down(VK_MENU);
        rig.root.key_up(VK_MENU);
    }
};

}  // namespace

TEST(menu_bar_layout_and_click_opens_dropdown) {
    BarRig b;
    CHECK(b.bar->heading_rect(0).width() > 30);
    CHECK_NEAR(b.bar->heading_rect(1).l, b.bar->heading_rect(0).r, 0.01);
    CHECK_EQ(b.bar->heading_at(b.heading(2)), 2);
    CHECK_EQ(b.bar->heading_at({700, 300}), -1);
    CHECK(!b.bar->in_mode());
    b.rig.click(b.heading(0));
    CHECK(b.bar->dropdown_open());
    CHECK_EQ(b.bar->open_index(), 0);
    CHECK_EQ(b.opened, 1);  // built when the heading opened
    CHECK(b.bar->in_mode());
    CHECK(b.layer->menu_open());
    // The dropdown hangs from the heading.
    const Rect hr = b.bar->heading_rect(0);
    const Rect dr = b.layer->menus().levels()[0].rect;
    CHECK_NEAR(dr.l, hr.l, 0.01);
    CHECK_NEAR(dr.t, hr.b, 0.01);
    // Hovering another heading while one is open switches to it.
    b.rig.move(b.heading(1));
    CHECK_EQ(b.bar->open_index(), 1);
    CHECK_EQ(b.layer->menus().levels()[0].items->size(), 2);
    // Pressing the open heading again closes the menu and leaves menu mode.
    b.rig.click(b.heading(1));
    CHECK(!b.layer->menu_open());
    CHECK(!b.bar->in_mode());
    CHECK(b.rig.root.modal() == nullptr);
    CHECK_EQ(b.under_clicks, 0);
    // Choosing an item runs it and leaves menu mode.
    b.rig.click(b.heading(2));
    CHECK(b.layer->menu_open());
    b.rig.click(b.item_center(0, 0));
    CHECK(b.log.size() == 1 && b.log[0] == "thumbs");
    CHECK(!b.bar->in_mode());
    CHECK(!b.mode_log.empty() && !b.mode_log.back());
}

TEST(menu_bar_press_drag_release_picks_an_item) {
    BarRig b;
    b.rig.down(b.heading(0));
    CHECK(b.layer->menu_open());
    const Point p = b.item_center(0, 1);
    b.rig.move(p);
    b.rig.up(p);
    CHECK(!b.layer->menu_open());
    CHECK(b.log.size() == 1 && b.log[0] == "open");
}

TEST(menu_bar_alt_tap_enters_mode_and_keys_drive_it) {
    BarRig b;
    b.tap_alt();
    CHECK(b.bar->in_mode());
    CHECK_EQ(b.bar->active(), 0);
    CHECK(!b.bar->dropdown_open());
    CHECK(b.rig.root.modal() == b.bar);
    b.rig.key(VK_RIGHT);
    CHECK_EQ(b.bar->active(), 1);
    b.rig.key(VK_RIGHT);
    b.rig.key(VK_RIGHT);
    CHECK_EQ(b.bar->active(), 0);  // wraps
    b.rig.key(VK_LEFT);
    CHECK_EQ(b.bar->active(), 2);
    b.rig.key(VK_DOWN);  // opens the active heading with its first item selected
    CHECK_EQ(b.bar->open_index(), 2);
    CHECK_EQ(b.layer->menus().selected(0), 0);
    // Left / Right at the root level of a dropdown move to the neighbouring heading, still open.
    b.rig.key(VK_RIGHT);
    CHECK_EQ(b.bar->open_index(), 0);
    CHECK_EQ(b.layer->menus().selected(0), 0);
    b.rig.key(VK_LEFT);
    CHECK_EQ(b.bar->open_index(), 2);
    // Escape in the dropdown steps back to the bar, still in mode; Escape again leaves it.
    b.rig.key(VK_ESCAPE);
    CHECK(!b.layer->menu_open());
    CHECK(b.bar->in_mode());
    CHECK(b.rig.root.modal() == b.bar);
    CHECK_EQ(b.bar->active(), 2);
    b.rig.key(VK_ESCAPE);
    CHECK(!b.bar->in_mode());
    CHECK(b.rig.root.modal() == nullptr);
    CHECK_EQ(b.mode_log.size(), 2);
}

TEST(menu_bar_access_keys_and_alt_letter) {
    BarRig b;
    // Alt+E opens Edit with its first item selected.
    b.rig.root.key_down(VK_MENU);
    b.rig.root.key_down('E');
    b.rig.root.key_up('E');
    b.rig.root.key_up(VK_MENU);  // the Alt release after a chord is not a tap
    CHECK_EQ(b.bar->open_index(), 1);
    CHECK_EQ(b.layer->menus().selected(0), 0);
    // An access key inside the dropdown invokes the item.
    b.rig.type(L"r");
    CHECK(!b.layer->menu_open());
    CHECK(b.log.size() == 1 && b.log[0] == "redo");
    CHECK(!b.bar->in_mode());
    // In bar mode a plain letter picks the heading.
    b.tap_alt();
    b.rig.type(L"v");
    CHECK_EQ(b.bar->open_index(), 2);
    b.rig.key(VK_ESCAPE);
    b.rig.key(VK_ESCAPE);
    CHECK(!b.bar->in_mode());
    // F10 toggles mode; a tap with another key between is not a tap.
    b.rig.key(VK_F10);
    CHECK(b.bar->in_mode());
    b.rig.key(VK_F10);
    CHECK(!b.bar->in_mode());
    b.rig.root.key_down(VK_MENU);
    b.rig.root.key_down(VK_LEFT);
    b.rig.root.key_up(VK_LEFT);
    b.rig.root.key_up(VK_MENU);
    CHECK(!b.bar->in_mode());
    // A second tap while in mode leaves it.
    b.tap_alt();
    CHECK(b.bar->in_mode());
    b.tap_alt();
    CHECK(!b.bar->in_mode());
}

TEST(menu_bar_view_can_stand_mode_down) {
    BarRig b;
    bool allowed = false;
    b.bar->allows_mode = [&] { return allowed; };
    b.tap_alt();
    CHECK(!b.bar->in_mode());
    CHECK(!b.rig.root.on_preview_key(KeyEvent{VK_F10, {}}));  // F10 is left for the view
    b.rig.root.key_down(VK_MENU);
    const bool took = b.rig.root.on_preview_key(KeyEvent{'F', Mods{false, false, true}});
    b.rig.root.key_up('F');
    b.rig.root.key_up(VK_MENU);
    CHECK(!took);
    CHECK(!b.bar->in_mode());
    allowed = true;
    b.tap_alt();
    CHECK(b.bar->in_mode());
    b.bar->exit_mode();
    CHECK(!b.bar->in_mode());
}

TEST(menu_bar_click_elsewhere_leaves_mode_and_lands) {
    BarRig b;
    b.tap_alt();
    CHECK(b.bar->in_mode());
    b.rig.click({600, 400});
    CHECK(!b.bar->in_mode());
    CHECK_EQ(b.under_clicks, 1);  // the click was not eaten
}

TEST(menu_bar_gallery_png) {
    BarRig b;
    b.bar->style.bg = tok::ToolbarBackground;
    b.rig.key(VK_F10);
    b.rig.key(VK_DOWN);
    b.rig.key(VK_DOWN);
    b.rig.render();
    const RECT px{0, 0, 460, 260};
    CHECK(b.rig.bitmap().write_png(exe_dir() + L"menu_bar.png", &px));
}
