// A top-level window that hosts a widget tree: window class, message handling, per-monitor v2 DPI
// (layout stays in DIPs), the timer behind UiRoot::tick, and the custom chrome proved in spike S8.
//
// `parked` is the harness mode: created at -32000,-32000 with WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
// shown without activation, disabled, drawing into a BitmapSurface. Nothing about it can reach the
// user's screen or take the foreground.
#pragma once
#include "gfx/resources.h"
#include "gfx/surface.h"
#include "ui/widget.h"

#include <functional>
#include <memory>
#include <string>

namespace bb::platform {

struct WindowOptions {
    std::wstring title = L"BertBrowser";
    int width_dip = 1100;
    int height_dip = 700;
    bool parked = false;
    // The real window (hardware surface) put out of sight and never activated: a benchmark launch
    // (BERTBROWSER_STARTUP_TRACE). Unlike `parked` it is a normal window in every other way.
    bool offscreen = false;
    bool custom_chrome = true;  // no caption, DWM resize frame kept; widgets with nc_hit make the bar
    bool activate = true;       // show() takes the foreground (never when parked)
    unsigned dpi_override = 0;  // 0 = the monitor's; the harness pins 96 so captures do not depend on the desktop
    bool quit_on_destroy = false;  // WM_DESTROY posts WM_QUIT (the main window)
};

class Window {
public:
    Window(gfx::Resources& res, const WindowOptions& options);
    ~Window();
    Window(const Window&) = delete;
    Window& operator=(const Window&) = delete;

    bool ok() const { return hwnd_ != nullptr && surface_ != nullptr; }
    HWND hwnd() const { return hwnd_; }
    ui::UiRoot& root() { return root_; }
    gfx::Surface& surface() { return *surface_; }
    // Non-null when the window draws into a bitmap (parked): the capture path.
    gfx::BitmapSurface* bitmap() { return bitmap_; }
    unsigned dpi() const { return dpi_; }
    const WindowOptions& options() const { return options_; }

    void show();
    // Lay out if asked and paint the dirty region now. Recovers from a device loss.
    HRESULT render();
    // Frame colours for DWM (dark mode, border, caption, caption text); unsupported attributes are ignored.
    void set_frame_colors(bool dark, COLORREF border, COLORREF caption, COLORREF text);
    // Where the tree sits in window DIPs <-> client pixels.
    POINT to_px(ui::Point p) const;
    ui::Point to_dip(int x, int y) const;

    std::function<void()> on_close;  // WM_CLOSE; default destroys the window
    // Called once, after the first paint has reached the surface (the content has been rendered).
    std::function<void()> on_first_paint;
    // Windows' light/dark, accent or high-contrast setting moved (WM_SETTINGCHANGE and friends).
    std::function<void()> on_system_settings_changed;
    // What is on the clipboard changed (WM_CLIPBOARDUPDATE, once the window listens for it).
    std::function<void()> on_clipboard_changed;
    // Application messages (WM_APP..0xBFFF), seen before anything else; true = handled. How the core's
    // events reach the UI thread: its sink PostMessages here and this handler drains them.
    std::function<bool(UINT msg, WPARAM wp, LPARAM lp)> on_message;

private:
    static void register_class();
    static LRESULT CALLBACK static_proc(HWND, UINT, WPARAM, LPARAM);
    LRESULT handle(UINT msg, WPARAM wp, LPARAM lp);
    LRESULT nc_hit_test(LPARAM lp);
    void on_size();
    void create_surface();
    void recover();
    void position_ime();
    void update_tick_timer();
    ui::Mods mouse_mods(WPARAM wp) const;
    bool mouse_button(UINT msg, WPARAM wp, LPARAM lp);

    gfx::Resources& res_;
    WindowOptions options_;
    HWND hwnd_ = nullptr;
    unsigned dpi_ = 96;
    ui::UiRoot root_;
    std::unique_ptr<gfx::Surface> surface_;
    gfx::BitmapSurface* bitmap_ = nullptr;
    bool tracking_leave_ = false;
    bool tick_running_ = false;
    bool reenabling_ = false;
    bool in_render_ = false;
};

}  // namespace bb::platform
