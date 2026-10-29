// The overlay every popup goes through: the last child of the window's content, covering it. It is the
// in-tree PopupPresenter (menus and tooltips drawn over the content, no window), and it is the single
// receiver of input while a menu is open: it takes the UiRoot modal, so mouse, wheel and keys reach it
// wherever the pointer is - which is also why a menu drawn in separate popup windows (WindowPresenter)
// needs no input handling of its own. Hover tooltips live here too (Widget::tooltip).
#pragma once
#include "ui/menu.h"

namespace bb::ui {

struct MenuOpen {
    Rect anchor;
    PopupSide side = PopupSide::Below;
    float min_width = 0;
    bool select_first = false;
    bool button_down = false;  // opened by a press that is still held (release over an item invokes it)
    std::function<void(const MenuItem&)> on_invoke;
    std::function<void(MenuClose)> on_closed;
    // A press outside every level. Return true when handled (the menu bar switching headings); false closes.
    std::function<bool(Point)> on_outside_press;
    std::function<void(Point)> on_outside_move;  // the pointer is over no level (a bar hovering another heading)
    std::function<void(bool right)> on_edge;
};

class PopupLayer : public Widget, public PopupPresenter {
public:
    static constexpr int kMenuTimer = 1, kTipTimer = 2;
    static constexpr unsigned kTipDelayMs = 500;

    explicit PopupLayer(gfx::Resources& res);
    ~PopupLayer() override;

    // Adds a layer as `host`'s last child (host must already be in a UiRoot) and registers it with the root,
    // wiring tooltips. Call again after replacing the content.
    static PopupLayer* install(Widget& host);
    static PopupLayer* of(const Widget& w);

    // Use another presenter (WindowPresenter) for menu levels and tips; nullptr goes back to in-tree.
    void set_presenter(PopupPresenter* p) { external_ = p; }
    MenuController& menus() { return menus_; }

    void open_menu(const MenuItems& items, MenuOpen opt);
    bool menu_open() const { return menus_.is_open(); }
    void close_menu(MenuClose why = MenuClose::Dismissed) { menus_.close(why); }

    // Tooltips. Shown by hovering a widget with `tooltip` text; these are for direct use and tests.
    void show_tip(const std::wstring& text, Point pointer);
    void hide_tip();
    bool tip_visible() const { return tip_ != nullptr; }
    const std::wstring& tip_text() const { return tip_text_; }
    Rect tip_rect() const { return tip_rect_; }

    // PopupPresenter, in-tree unless an external presenter is set.
    Rect work_area() override;
    Widget* show(std::unique_ptr<Widget> content, Rect rect) override;
    void hide(Widget* content) override;
    void start_timer(unsigned ms) override;
    void stop_timer() override;

    // Widget
    Size measure(Size) override { return {}; }
    void arrange(Rect r) override;
    bool on_mouse_down(const MouseEvent& e) override;
    bool on_mouse_up(const MouseEvent& e) override;
    bool on_mouse_move(const MouseEvent& e) override;
    bool on_wheel(Point, int, Mods) override { return true; }
    bool on_key_down(const KeyEvent& e) override;
    bool on_char(wchar_t c) override;
    void on_timer(int id) override;
    void on_modal_cancel() override;

private:
    void hover_changed(Widget* w);
    // A menu item's tooltip: the item the pointer rests on, and what it will say once the delay is up.
    const MenuItem* tip_item_ = nullptr;
    std::wstring menu_tip_;

    gfx::Resources& res_;
    PopupPresenter* external_ = nullptr;
    struct Shown {
        Widget* w;
        Rect rect;
    };
    std::vector<Shown> shown_;  // before menus_: its destructor hides the levels through us
    MenuController menus_;
    MenuOpen opt_;
    std::function<void(const MenuItem&)> pending_invoke_;
    Widget* tip_target_ = nullptr;
    Widget* tip_ = nullptr;
    std::wstring tip_text_;
    Rect tip_rect_;
};

// The text bubble of a tooltip.
class TooltipBubble : public Widget {
public:
    TooltipBubble(gfx::Resources& res, std::wstring text);
    Size natural_size() const { return natural_; }
    Size measure(Size) override { return natural_; }
    void paint(PaintContext& pc) override;

private:
    gfx::Resources& res_;
    std::wstring text_;
    Size natural_;
};

// A menu photographed without a window: opens `items` below a point near the top-left of a scratch
// UiRoot, follows `path` (the selected index at each level, opening submenus on the way) and paints
// every open level into a new BitmapSurface sized to fit. This is what the harness uses for `menu main <Menu>`.
std::unique_ptr<gfx::BitmapSurface> render_menu_detached(gfx::Resources& res, const MenuItems& items,
                                                         const std::vector<int>& path = {}, float dpi = 96.0f);

}  // namespace bb::ui
