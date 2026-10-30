#include "platform/popup_window.h"

#include <algorithm>
#include <cmath>

namespace bb::platform {

namespace {
constexpr wchar_t kClassName[] = L"BertBrowserPopup";
}

void PopupWindow::register_class() {
    static bool done = false;
    if (done) return;
    WNDCLASSEXW wc{};
    wc.cbSize = sizeof(wc);
    wc.style = CS_DROPSHADOW;
    wc.lpfnWndProc = &PopupWindow::static_proc;
    wc.hInstance = GetModuleHandleW(nullptr);
    wc.hCursor = LoadCursorW(nullptr, IDC_ARROW);
    wc.lpszClassName = kClassName;
    RegisterClassExW(&wc);
    done = true;
}

LRESULT CALLBACK PopupWindow::static_proc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp) {
    switch (msg) {
        case WM_NCCREATE:
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(reinterpret_cast<CREATESTRUCTW*>(lp)->lpCreateParams));
            break;
        case WM_MOUSEACTIVATE:
            return MA_NOACTIVATE;
        case WM_ERASEBKGND:
            return 1;
        case WM_PAINT: {
            PAINTSTRUCT ps;
            BeginPaint(hwnd, &ps);
            EndPaint(hwnd, &ps);
            if (auto* self = reinterpret_cast<PopupWindow*>(GetWindowLongPtrW(hwnd, GWLP_USERDATA))) self->render();
            return 0;
        }
        default:
            break;
    }
    return DefWindowProcW(hwnd, msg, wp, lp);
}

PopupWindow::PopupWindow(gfx::Resources& res, HWND owner, bool parked, unsigned dpi, std::unique_ptr<ui::Widget> content,
                         RECT screen)
    : res_(res), parked_(parked), dpi_(dpi), root_(res) {
    register_class();
    const DWORD ex = WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
    hwnd_ = CreateWindowExW(ex, kClassName, L"", WS_POPUP, screen.left, screen.top, screen.right - screen.left,
                            screen.bottom - screen.top, owner, nullptr, GetModuleHandleW(nullptr), this);
    if (!hwnd_) return;
    const UINT w = static_cast<UINT>(std::max<LONG>(screen.right - screen.left, 1));
    const UINT h = static_cast<UINT>(std::max<LONG>(screen.bottom - screen.top, 1));
    if (parked_) {
        auto b = gfx::BitmapSurface::create(res_.dev, w, h, static_cast<float>(dpi_));
        bitmap_ = b.get();
        surface_ = std::move(b);
    } else {
        surface_ = gfx::SwapChainSurface::create(res_.dev, hwnd_, w, h, static_cast<float>(dpi_));
    }
    content_ = root_.set_content(std::move(content));
    root_.on_invalidate = [this] {
        if (hwnd_ && !in_render_ && !parked_) InvalidateRect(hwnd_, nullptr, FALSE);
    };
    ShowWindow(hwnd_, SW_SHOWNOACTIVATE);  // an owned, no-activate tool window: never takes the foreground
    if (surface_) {
        root_.layout({surface_->width_dip(), surface_->height_dip()});
        render();
    }
}

PopupWindow::~PopupWindow() {
    if (hwnd_) {
        SetWindowLongPtrW(hwnd_, GWLP_USERDATA, 0);
        if (IsWindow(hwnd_)) DestroyWindow(hwnd_);
    }
}

RECT PopupWindow::screen_rect() const {
    RECT r{};
    if (hwnd_) GetWindowRect(hwnd_, &r);
    return r;
}

HRESULT PopupWindow::render() {
    if (!surface_) return E_FAIL;
    if (!parked_ && surface_->generation() != res_.dev.generation()) {
        // The device moved on under an open window (WARP to hardware): its swap chain belongs to the old one.
        RECT rc{};
        GetClientRect(hwnd_, &rc);
        const UINT w = static_cast<UINT>(std::max<LONG>(rc.right, 1));
        const UINT h = static_cast<UINT>(std::max<LONG>(rc.bottom, 1));
        surface_.reset();
        surface_ = gfx::SwapChainSurface::create(res_.dev, hwnd_, w, h, static_cast<float>(dpi_));
        if (!surface_) return E_FAIL;
        root_.layout({surface_->width_dip(), surface_->height_dip()});
        root_.invalidate_all();
    }
    in_render_ = true;
    const HRESULT hr = root_.paint(*surface_);
    in_render_ = false;
    return hr;
}

// ---- WindowPresenter --------------------------------------------------------------------------------

ui::Rect WindowPresenter::work_area() {
    if (has_work_) return work_;
    RECT client{};
    GetClientRect(owner_, &client);
    const float s = 96.0f / static_cast<float>(dpi_);
    if (parked_) return ui::Rect{0, 0, static_cast<float>(client.right) * s, static_cast<float>(client.bottom) * s};
    POINT origin{0, 0};
    ClientToScreen(owner_, &origin);
    MONITORINFO mi{sizeof(mi)};
    if (!GetMonitorInfoW(MonitorFromWindow(owner_, MONITOR_DEFAULTTONEAREST), &mi)) {
        return ui::Rect{0, 0, static_cast<float>(client.right) * s, static_cast<float>(client.bottom) * s};
    }
    return ui::Rect{static_cast<float>(mi.rcWork.left - origin.x) * s, static_cast<float>(mi.rcWork.top - origin.y) * s,
                    static_cast<float>(mi.rcWork.right - origin.x) * s, static_cast<float>(mi.rcWork.bottom - origin.y) * s};
}

ui::Widget* WindowPresenter::show(std::unique_ptr<ui::Widget> content, ui::Rect rect) {
    POINT origin{0, 0};
    if (parked_) {
        origin = {-32000, -32000};  // never relative to anything that could be on a screen
    } else {
        ClientToScreen(owner_, &origin);
    }
    const float s = static_cast<float>(dpi_) / 96.0f;
    RECT px{origin.x + static_cast<LONG>(std::lround(rect.l * s)), origin.y + static_cast<LONG>(std::lround(rect.t * s)),
            origin.x + static_cast<LONG>(std::lround(rect.r * s)), origin.y + static_cast<LONG>(std::lround(rect.b * s))};
    ui::Widget* raw = content.get();
    auto w = std::make_unique<PopupWindow>(res_, owner_, parked_, dpi_, std::move(content), px);
    if (!w->ok()) return nullptr;
    windows_.push_back(std::move(w));
    return raw;
}

void WindowPresenter::hide(ui::Widget* content) {
    std::erase_if(windows_, [&](const std::unique_ptr<PopupWindow>& w) { return w->content() == content; });
}

PopupWindow* WindowPresenter::window_of(const ui::Widget* content) const {
    for (auto& w : windows_) {
        if (w->content() == content) return w.get();
    }
    return nullptr;
}

}  // namespace bb::platform
