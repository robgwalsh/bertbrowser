// The menu bar: a row of headings whose dropdowns are menus shown through the window's PopupLayer.
//
// Menu mode is the keyboard state - Alt tapped alone, F10, or Alt+letter. While it is on and no dropdown is
// up, the bar is the UiRoot modal (headings highlight with Left/Right, Down/Enter open one, a letter picks
// one by its access key, Esc leaves); while a dropdown is up the PopupLayer is the modal and hands Left/Right
// at the edge back to the bar. The view decides when mode is allowed (`allows_mode`: not under a dialog, the
// palette or the key recorder), which is how it stands the bar down.
#pragma once
#include "ui/popups/popup_layer.h"

namespace bb::ui {

struct MenuBarEntry {
    std::wstring text;  // "&File"
    MenuItems items;
    // Fills `items` just before the dropdown opens (a menu is built when its heading opens).
    std::function<void(MenuItems&)> on_opening;
};

class MenuBar : public Widget {
public:
    static constexpr float kHeadingPad = 10.0f;

    MenuBar();

    void set_menus(std::vector<MenuBarEntry> menus);
    std::vector<MenuBarEntry>& menus() { return menus_; }
    Rect heading_rect(int i) const;
    int heading_at(Point p) const;  // -1 when none

    // ---- state ----
    int active() const { return active_; }             // the highlighted heading, or -1
    bool in_mode() const { return mode_; }              // keyboard menu mode (a dropdown counts)
    int open_index() const { return open_; }            // the heading whose dropdown is up, or -1
    bool dropdown_open() const { return open_ >= 0; }

    void open(int index, bool select_first = false, bool button_down = false);
    void enter_mode();
    void exit_mode();

    // ---- what the view hooks ----
    std::function<bool()> allows_mode;       // null = always; false stands Alt / F10 / Alt+letter down
    std::function<void(bool)> on_mode_changed;
    // Wires Alt-tap to `root`. The view's key dispatcher must also call preview_key() first.
    void hook(UiRoot& root);
    // F10 and Alt+letter. true = consumed. Any other key cancels a pending Alt tap.
    bool preview_key(const KeyEvent& e);
    // Alt went down / up by itself (hook() calls it).
    void alt_changed(bool down);

    Size measure(Size avail) override;
    void paint(PaintContext& pc) override;
    bool on_mouse_down(const MouseEvent& e) override;
    bool on_mouse_up(const MouseEvent& e) override;
    bool on_mouse_move(const MouseEvent& e) override;
    void on_mouse_leave() override;
    bool on_key_down(const KeyEvent& e) override;
    bool on_char(wchar_t c) override;
    void on_modal_cancel() override;

private:
    bool allowed() const { return !allows_mode || allows_mode(); }
    void on_dropdown_closed(MenuClose why);
    void step(int dir, bool reopen);

    std::vector<MenuBarEntry> menus_;
    std::vector<float> lefts_;  // heading i spans [lefts_[i], lefts_[i + 1]) from bounds.l
    int active_ = -1, hover_ = -1, open_ = -1;
    bool mode_ = false;
    bool alt_pending_ = false;
    bool switching_ = false;
};

}  // namespace bb::ui
