// Popups as windows of their own: a borderless, owned, never-activating Win32 window drawn by D2D, one
// per menu level (or tooltip). Input never reaches them - the PopupLayer holds the UiRoot modal, and with
// it the OS mouse capture - so they are display only. TrackPopupMenuEx is not used anywhere.
//
// Parked (the harness and the tests): the window is created at -32000,-32000 plus its offset in the owner
// and draws into a BitmapSurface, so nothing can reach a screen and the pixels can be read back.
#pragma once
#include "gfx/resources.h"
#include "gfx/surface.h"
#include "ui/popups/popup.h"

#include <memory>
#include <vector>

namespace bb::platform {

class PopupWindow {
public:
    // `screen` is the window's rectangle in screen pixels.
    PopupWindow(gfx::Resources& res, HWND owner, bool parked, unsigned dpi, std::unique_ptr<ui::Widget> content,
                RECT screen);
    ~PopupWindow();
    PopupWindow(const PopupWindow&) = delete;
    PopupWindow& operator=(const PopupWindow&) = delete;

    bool ok() const { return hwnd_ != nullptr && surface_ != nullptr; }
    HWND hwnd() const { return hwnd_; }
    ui::Widget* content() const { return content_; }
    ui::UiRoot& root() { return root_; }
    gfx::BitmapSurface* bitmap() { return bitmap_; }  // non-null when parked
    RECT screen_rect() const;
    HRESULT render();

private:
    static void register_class();
    static LRESULT CALLBACK static_proc(HWND, UINT, WPARAM, LPARAM);

    gfx::Resources& res_;
    HWND hwnd_ = nullptr;
    bool parked_;
    unsigned dpi_;
    ui::UiRoot root_;
    ui::Widget* content_ = nullptr;
    std::unique_ptr<gfx::Surface> surface_;
    gfx::BitmapSurface* bitmap_ = nullptr;
    bool in_render_ = false;
};

// Shows the PopupLayer's popups in PopupWindows. Rectangles are the owner's client DIPs.
class WindowPresenter : public ui::PopupPresenter {
public:
    WindowPresenter(gfx::Resources& res, HWND owner, bool parked, unsigned dpi)
        : res_(res), owner_(owner), parked_(parked), dpi_(dpi) {}

    // Overrides the monitor's work area (owner DIPs). A parked presenter always uses one: its owner sits
    // off screen, and the monitor's rectangle relative to that would put a menu on the real desktop.
    void set_work_area(ui::Rect r) {
        work_ = r;
        has_work_ = true;
    }
    const std::vector<std::unique_ptr<PopupWindow>>& windows() const { return windows_; }
    PopupWindow* window_of(const ui::Widget* content) const;

    ui::Rect work_area() override;
    ui::Widget* show(std::unique_ptr<ui::Widget> content, ui::Rect rect) override;
    void hide(ui::Widget* content) override;
    void start_timer(unsigned) override {}
    void stop_timer() override {}

private:
    gfx::Resources& res_;
    HWND owner_;
    bool parked_;
    unsigned dpi_;
    ui::Rect work_;
    bool has_work_ = false;
    std::vector<std::unique_ptr<PopupWindow>> windows_;
};

}  // namespace bb::platform
