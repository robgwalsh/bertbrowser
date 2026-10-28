#include "platform/window.h"

#include <dwmapi.h>
#include <imm.h>
#include <windowsx.h>

#include <algorithm>
#include <cmath>

namespace bb::platform {

namespace {

constexpr wchar_t kClassName[] = L"BertBrowserWindow";
constexpr UINT_PTR kTickTimer = 1;
constexpr UINT kTickMs = 16;

int frame_x(UINT dpi) { return GetSystemMetricsForDpi(SM_CXFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi); }
int frame_y(UINT dpi) { return GetSystemMetricsForDpi(SM_CYFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi); }

LPCWSTR cursor_id(ui::Cursor c) {
    switch (c) {
        case ui::Cursor::IBeam: return IDC_IBEAM;
        case ui::Cursor::SizeWE: return IDC_SIZEWE;
        case ui::Cursor::SizeNS: return IDC_SIZENS;
        case ui::Cursor::Hand: return IDC_HAND;
        default: return IDC_ARROW;
    }
}

}  // namespace

void Window::register_class() {
    static bool done = false;
    if (done) return;
    WNDCLASSEXW wc{};
    wc.cbSize = sizeof(wc);
    wc.style = CS_DBLCLKS;
    wc.lpfnWndProc = &Window::static_proc;
    wc.hInstance = GetModuleHandleW(nullptr);
    wc.hCursor = LoadCursorW(nullptr, IDC_ARROW);
    wc.lpszClassName = kClassName;
    RegisterClassExW(&wc);
    done = true;
}

Window::Window(gfx::Resources& res, const WindowOptions& options) : res_(res), options_(options), root_(res) {
    register_class();
    const DWORD ex = options_.parked ? (WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE) : 0;
    const UINT guess = options_.dpi_override ? options_.dpi_override : GetDpiForSystem();
    const int w = MulDiv(options_.width_dip, static_cast<int>(guess), 96);
    const int h = MulDiv(options_.height_dip, static_cast<int>(guess), 96);
    const int x = options_.parked ? -32000 : CW_USEDEFAULT;
    const int y = options_.parked ? -32000 : CW_USEDEFAULT;
    hwnd_ = CreateWindowExW(ex, kClassName, options_.title.c_str(), WS_OVERLAPPEDWINDOW, x, y, w, h, nullptr, nullptr,
                            GetModuleHandleW(nullptr), this);
    if (!hwnd_) {
        return;
    }
    dpi_ = options_.dpi_override ? options_.dpi_override : GetDpiForWindow(hwnd_);
    root_.clipboard = ui::Clipboard::system(hwnd_);
    root_.on_invalidate = [this] {
        if (hwnd_ && !in_render_) InvalidateRect(hwnd_, nullptr, FALSE);
    };
    root_.on_timers_changed = [this] { update_tick_timer(); };
    // An open menu owns the mouse the way a Win32 menu does: the OS capture follows the modal, so a click
    // anywhere (outside this window too) reaches the menu, which closes itself.
    root_.on_modal_changed = [this](bool on) {
        if (options_.parked || !hwnd_) return;
        if (on) {
            SetCapture(hwnd_);
        } else if (GetCapture() == hwnd_) {
            ReleaseCapture();
        }
    };

    if (options_.custom_chrome) {
        SetWindowPos(hwnd_, nullptr, 0, 0, 0, 0,
                     SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
        set_frame_colors(true, RGB(0x3c, 0x3c, 0x3c), RGB(0x1e, 0x1e, 0x1e), RGB(0xe0, 0xe0, 0xe0));
    }
    // Make the client area exactly the asked size in pixels (the frame differs by chrome mode and DPI).
    const int want_w = MulDiv(options_.width_dip, static_cast<int>(dpi_), 96);
    const int want_h = MulDiv(options_.height_dip, static_cast<int>(dpi_), 96);
    RECT win{}, client{};
    GetWindowRect(hwnd_, &win);
    GetClientRect(hwnd_, &client);
    SetWindowPos(hwnd_, nullptr, 0, 0, (win.right - win.left) + (want_w - client.right),
                 (win.bottom - win.top) + (want_h - client.bottom), SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
    create_surface();
}

Window::~Window() {
    if (hwnd_) {
        SetWindowLongPtrW(hwnd_, GWLP_USERDATA, 0);
        if (IsWindow(hwnd_)) DestroyWindow(hwnd_);
    }
}

void Window::create_surface() {
    surface_.reset();
    bitmap_ = nullptr;
    RECT rc{};
    GetClientRect(hwnd_, &rc);
    const UINT w = static_cast<UINT>(std::max<LONG>(rc.right, 1));
    const UINT h = static_cast<UINT>(std::max<LONG>(rc.bottom, 1));
    if (options_.parked) {
        auto b = gfx::BitmapSurface::create(res_.dev, w, h, static_cast<float>(dpi_));
        bitmap_ = b.get();
        surface_ = std::move(b);
    } else {
        surface_ = gfx::SwapChainSurface::create(res_.dev, hwnd_, w, h, static_cast<float>(dpi_));
    }
    if (surface_) {
        root_.layout({surface_->width_dip(), surface_->height_dip()});
    }
}

void Window::recover() {
    res_.dev.recreate();
    res_.on_device_recreated();
    create_surface();
    root_.invalidate_all();
}

void Window::show() {
    if (options_.parked) {
        ShowWindow(hwnd_, SW_SHOWNOACTIVATE);
        EnableWindow(hwnd_, FALSE);  // a disabled window cannot be activated by anything
    } else {
        ShowWindow(hwnd_, options_.activate ? SW_SHOW : SW_SHOWNOACTIVATE);
    }
}

HRESULT Window::render() {
    if (!surface_) return E_FAIL;
    in_render_ = true;
    HRESULT hr = root_.paint(*surface_);
    in_render_ = false;
    if (gfx::GfxDevice::is_loss(hr)) {
        recover();
        in_render_ = true;
        hr = root_.paint(*surface_);
        in_render_ = false;
    }
    return hr;
}

void Window::set_frame_colors(bool dark, COLORREF border, COLORREF caption, COLORREF text) {
    const BOOL d = dark ? TRUE : FALSE;
    DwmSetWindowAttribute(hwnd_, DWMWA_USE_IMMERSIVE_DARK_MODE, &d, sizeof(d));
    const DWM_WINDOW_CORNER_PREFERENCE corner = DWMWCP_ROUND;
    DwmSetWindowAttribute(hwnd_, DWMWA_WINDOW_CORNER_PREFERENCE, &corner, sizeof(corner));
    DwmSetWindowAttribute(hwnd_, DWMWA_BORDER_COLOR, &border, sizeof(border));
    DwmSetWindowAttribute(hwnd_, DWMWA_CAPTION_COLOR, &caption, sizeof(caption));
    DwmSetWindowAttribute(hwnd_, DWMWA_TEXT_COLOR, &text, sizeof(text));
}

POINT Window::to_px(ui::Point p) const {
    const float s = static_cast<float>(dpi_) / 96.0f;
    return {static_cast<LONG>(std::lround(p.x * s)), static_cast<LONG>(std::lround(p.y * s))};
}

ui::Point Window::to_dip(int x, int y) const {
    const float s = 96.0f / static_cast<float>(dpi_);
    return {static_cast<float>(x) * s, static_cast<float>(y) * s};
}

void Window::on_size() {
    if (!surface_ || IsIconic(hwnd_)) return;
    RECT rc{};
    GetClientRect(hwnd_, &rc);
    const UINT w = static_cast<UINT>(std::max<LONG>(rc.right, 1));
    const UINT h = static_cast<UINT>(std::max<LONG>(rc.bottom, 1));
    if (w != surface_->width_px() || h != surface_->height_px()) {
        if (gfx::GfxDevice::is_loss(surface_->resize(w, h))) {
            recover();
            return;
        }
    }
    root_.layout({surface_->width_dip(), surface_->height_dip()});
}

void Window::update_tick_timer() {
    if (!hwnd_) return;
    const bool want = root_.has_timers();
    if (want && !tick_running_) {
        SetTimer(hwnd_, kTickTimer, kTickMs, nullptr);
        tick_running_ = true;
    } else if (!want && tick_running_) {
        KillTimer(hwnd_, kTickTimer);
        tick_running_ = false;
    }
}

void Window::position_ime() {
    ui::Rect r;
    if (!root_.ime_caret_rect(&r)) return;
    const HIMC himc = ImmGetContext(hwnd_);
    if (!himc) return;
    const POINT tl = to_px({r.l, r.t}), br = to_px({r.r, r.b});
    COMPOSITIONFORM cf{};
    cf.dwStyle = CFS_POINT;
    cf.ptCurrentPos = tl;
    ImmSetCompositionWindow(himc, &cf);
    CANDIDATEFORM cand{};
    cand.dwStyle = CFS_EXCLUDE;
    cand.ptCurrentPos = tl;
    cand.rcArea = {tl.x, tl.y, br.x, br.y};
    ImmSetCandidateWindow(himc, &cand);
    ImmReleaseContext(hwnd_, himc);
}

ui::Mods Window::mouse_mods(WPARAM wp) const {
    return {(wp & MK_SHIFT) != 0, (wp & MK_CONTROL) != 0, false};
}

bool Window::mouse_button(UINT msg, WPARAM wp, LPARAM lp) {
    const ui::Point p = to_dip(GET_X_LPARAM(lp), GET_Y_LPARAM(lp));
    const ui::Mods m = mouse_mods(wp);
    using ui::MouseButton;
    MouseButton b = MouseButton::Left;
    bool down = true;
    int clicks = 1;
    switch (msg) {
        case WM_LBUTTONDBLCLK: clicks = 2; break;
        case WM_LBUTTONUP: down = false; break;
        case WM_RBUTTONDOWN: b = MouseButton::Right; break;
        case WM_RBUTTONDBLCLK: b = MouseButton::Right; clicks = 2; break;
        case WM_RBUTTONUP: b = MouseButton::Right; down = false; break;
        case WM_MBUTTONDOWN: b = MouseButton::Middle; break;
        case WM_MBUTTONDBLCLK: b = MouseButton::Middle; clicks = 2; break;
        case WM_MBUTTONUP: b = MouseButton::Middle; down = false; break;
        default: break;
    }
    if (down) {
        if (!options_.parked) SetCapture(hwnd_);
        return root_.mouse_down(p, b, m, clicks);
    }
    const bool handled = root_.mouse_up(p, b, m);
    if (!options_.parked && !root_.modal() && (wp & (MK_LBUTTON | MK_RBUTTON | MK_MBUTTON)) == 0 &&
        GetCapture() == hwnd_) {
        ReleaseCapture();
    }
    return handled;
}

LRESULT Window::nc_hit_test(LPARAM lp) {
    const LRESULT def = DefWindowProcW(hwnd_, WM_NCHITTEST, 0, lp);
    if (def != HTCLIENT) {
        return def;  // the left, right and bottom resize frames are DefWindowProc's
    }
    POINT pt{GET_X_LPARAM(lp), GET_Y_LPARAM(lp)};
    ScreenToClient(hwnd_, &pt);
    RECT client{};
    GetClientRect(hwnd_, &client);
    if (!IsZoomed(hwnd_) && pt.y < frame_y(dpi_)) {
        // The top frame is ours now: WM_NCCALCSIZE gave that strip to the client area.
        if (pt.x < frame_x(dpi_)) return HTTOPLEFT;
        if (pt.x >= client.right - frame_x(dpi_)) return HTTOPRIGHT;
        return HTTOP;
    }
    ui::Widget* w = root_.hit_test(to_dip(pt.x, pt.y));
    for (; w; w = w->parent()) {
        if (w->nc_hit != 0) return w->nc_hit;
    }
    return HTCLIENT;
}

LRESULT CALLBACK Window::static_proc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp) {
    if (msg == WM_NCCREATE) {
        const auto* cs = reinterpret_cast<const CREATESTRUCTW*>(lp);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(cs->lpCreateParams));
        auto* self = static_cast<Window*>(cs->lpCreateParams);
        self->hwnd_ = hwnd;
    }
    auto* self = reinterpret_cast<Window*>(GetWindowLongPtrW(hwnd, GWLP_USERDATA));
    if (!self) {
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    return self->handle(msg, wp, lp);
}

LRESULT Window::handle(UINT msg, WPARAM wp, LPARAM lp) {
    const bool chrome = options_.custom_chrome;
    if (on_message && msg >= WM_APP && msg < 0xC000 && on_message(msg, wp, lp)) {
        return 0;
    }
    switch (msg) {
        case WM_NCCALCSIZE:
            if (chrome && wp) {
                // Keep the side and bottom frames (DWM draws the resize borders and the shadow for them)
                // and give the top back to the client, so there is no caption. Maximised, the frame would
                // hang off the monitor, so inset the top by the frame height.
                auto* params = reinterpret_cast<NCCALCSIZE_PARAMS*>(lp);
                const LONG top = params->rgrc[0].top;
                DefWindowProcW(hwnd_, msg, wp, lp);
                params->rgrc[0].top = top + (IsZoomed(hwnd_) ? frame_y(GetDpiForWindow(hwnd_)) : 0);
                return 0;
            }
            break;
        case WM_NCHITTEST:
            if (chrome) return nc_hit_test(lp);
            break;
        case WM_GETMINMAXINFO:
            if (chrome) {
                const HMONITOR monitor = MonitorFromWindow(hwnd_, MONITOR_DEFAULTTONEAREST);
                MONITORINFO info{sizeof(info)};
                if (monitor && GetMonitorInfoW(monitor, &info)) {
                    auto* mm = reinterpret_cast<MINMAXINFO*>(lp);
                    mm->ptMaxPosition.x = info.rcWork.left - info.rcMonitor.left;
                    mm->ptMaxPosition.y = info.rcWork.top - info.rcMonitor.top;
                    mm->ptMaxSize.x = info.rcWork.right - info.rcWork.left;
                    mm->ptMaxSize.y = info.rcWork.bottom - info.rcWork.top;
                    mm->ptMaxTrackSize = mm->ptMaxSize;
                }
                return 0;
            }
            break;
        case WM_NCLBUTTONDOWN:
            if (chrome && (wp == HTMAXBUTTON || wp == HTMINBUTTON || wp == HTCLOSE)) return 0;
            break;
        case WM_NCLBUTTONUP:
            // A parked window must never be maximised or minimised: maximising moves it onto the screen.
            if (chrome && wp == HTMAXBUTTON) {
                if (!options_.parked) ShowWindow(hwnd_, IsZoomed(hwnd_) ? SW_RESTORE : SW_MAXIMIZE);
                return 0;
            }
            if (chrome && wp == HTMINBUTTON) {
                if (!options_.parked) ShowWindow(hwnd_, SW_MINIMIZE);
                return 0;
            }
            if (chrome && wp == HTCLOSE) {
                PostMessageW(hwnd_, WM_CLOSE, 0, 0);
                return 0;
            }
            break;
        case WM_NCMOUSEMOVE: {
            POINT pt{GET_X_LPARAM(lp), GET_Y_LPARAM(lp)};
            ScreenToClient(hwnd_, &pt);
            root_.mouse_move(to_dip(pt.x, pt.y));
            TRACKMOUSEEVENT t{sizeof(t), TME_LEAVE | TME_NONCLIENT, hwnd_, 0};
            TrackMouseEvent(&t);
            break;
        }
        case WM_NCMOUSELEAVE:
            root_.mouse_leave();
            break;
        case WM_ENABLE:
            // A modal child disables its owner at the Win32 level; undo that at once so the title bar
            // stays draggable and closable. The content block is the app's own business. (Not for a
            // parked window: its disabled state is the point.)
            if (chrome && !options_.parked && wp == FALSE && !reenabling_) {
                reenabling_ = true;
                EnableWindow(hwnd_, TRUE);
                reenabling_ = false;
            }
            break;
        case WM_SIZE:
            if (wp != SIZE_MINIMIZED) on_size();
            return 0;
        case WM_SETTINGCHANGE:
        case WM_SYSCOLORCHANGE:
        case WM_THEMECHANGED:
            if (on_system_settings_changed) on_system_settings_changed();
            break;
        case WM_DPICHANGED:
            if (!options_.dpi_override) {
                dpi_ = HIWORD(wp);
                if (surface_) surface_->set_dpi(static_cast<float>(dpi_));
                const auto* r = reinterpret_cast<const RECT*>(lp);
                SetWindowPos(hwnd_, nullptr, r->left, r->top, r->right - r->left, r->bottom - r->top,
                             SWP_NOZORDER | SWP_NOACTIVATE);
                on_size();
            }
            return 0;
        case WM_ERASEBKGND:
            return 1;
        case WM_PAINT: {
            PAINTSTRUCT ps;
            BeginPaint(hwnd_, &ps);
            EndPaint(hwnd_, &ps);
            render();
            return 0;
        }
        case WM_SETCURSOR:
            if (LOWORD(lp) == HTCLIENT) {
                SetCursor(LoadCursorW(nullptr, cursor_id(root_.cursor())));
                return TRUE;
            }
            break;
        case WM_MOUSEMOVE: {
            if (!tracking_leave_) {
                TRACKMOUSEEVENT t{sizeof(t), TME_LEAVE, hwnd_, 0};
                tracking_leave_ = TrackMouseEvent(&t) != 0;
            }
            root_.mouse_move(to_dip(GET_X_LPARAM(lp), GET_Y_LPARAM(lp)), mouse_mods(wp));
            return 0;
        }
        case WM_MOUSELEAVE:
            tracking_leave_ = false;
            root_.mouse_leave();
            return 0;
        case WM_LBUTTONDOWN:
        case WM_LBUTTONDBLCLK:
        case WM_LBUTTONUP:
        case WM_RBUTTONDOWN:
        case WM_RBUTTONDBLCLK:
        case WM_RBUTTONUP:
        case WM_MBUTTONDOWN:
        case WM_MBUTTONDBLCLK:
        case WM_MBUTTONUP:
            mouse_button(msg, wp, lp);
            return 0;
        case WM_MOUSEWHEEL: {
            POINT pt{GET_X_LPARAM(lp), GET_Y_LPARAM(lp)};
            ScreenToClient(hwnd_, &pt);
            root_.wheel(to_dip(pt.x, pt.y), GET_WHEEL_DELTA_WPARAM(wp), mouse_mods(GET_KEYSTATE_WPARAM(wp)));
            return 0;
        }
        case WM_KEYDOWN: {
            const bool handled = root_.key_down(static_cast<unsigned>(wp));
            position_ime();
            if (handled) return 0;
            break;
        }
        case WM_SYSKEYDOWN: {
            const bool handled = root_.key_down(static_cast<unsigned>(wp));
            if (handled && wp != VK_MENU && wp != VK_F10) return 0;
            break;
        }
        case WM_KEYUP:
        case WM_SYSKEYUP:
            root_.key_up(static_cast<unsigned>(wp));
            if (msg == WM_KEYUP) return 0;
            break;
        case WM_CHAR:
            root_.char_input(static_cast<wchar_t>(wp));
            position_ime();
            return 0;
        case WM_CANCELMODE:
            root_.cancel_modal();
            break;
        case WM_ACTIVATE:
            if (LOWORD(wp) == WA_INACTIVE) root_.cancel_modal();
            break;
        case WM_SETFOCUS:
        case WM_KILLFOCUS:
            root_.set_mods({});  // a key released while elsewhere never reached us
            break;
        case WM_TIMER:
            if (wp == kTickTimer) {
                root_.tick();
                return 0;
            }
            break;
        case WM_IME_SETCONTEXT:
            // We draw the composition ourselves; the IME keeps its candidate list.
            return DefWindowProcW(hwnd_, msg, wp, lp & ~static_cast<LPARAM>(ISC_SHOWUICOMPOSITIONWINDOW));
        case WM_IME_STARTCOMPOSITION:
            root_.ime_composition(L"", 0);
            position_ime();
            return 0;
        case WM_IME_COMPOSITION: {
            const HIMC himc = ImmGetContext(hwnd_);
            if (himc) {
                auto read = [&](DWORD what) {
                    const LONG bytes = ImmGetCompositionStringW(himc, what, nullptr, 0);
                    std::wstring s;
                    if (bytes > 0) {
                        s.resize(static_cast<size_t>(bytes) / sizeof(wchar_t));
                        ImmGetCompositionStringW(himc, what, s.data(), static_cast<DWORD>(bytes));
                    }
                    return s;
                };
                if (lp & GCS_RESULTSTR) root_.ime_commit(read(GCS_RESULTSTR));
                if (lp & GCS_COMPSTR) {
                    const LONG cursor = ImmGetCompositionStringW(himc, GCS_CURSORPOS, nullptr, 0);
                    root_.ime_composition(read(GCS_COMPSTR), cursor > 0 ? static_cast<size_t>(cursor) : 0);
                }
                ImmReleaseContext(hwnd_, himc);
            }
            position_ime();
            return 0;
        }
        case WM_IME_ENDCOMPOSITION:
            root_.ime_end();
            return 0;
        case WM_CLOSE:
            if (on_close) {
                on_close();
            } else {
                DestroyWindow(hwnd_);
            }
            return 0;
        case WM_DESTROY:
            if (tick_running_) {
                KillTimer(hwnd_, kTickTimer);
                tick_running_ = false;
            }
            if (options_.quit_on_destroy) PostQuitMessage(0);
            break;
        case WM_NCDESTROY: {
            const LRESULT r = DefWindowProcW(hwnd_, msg, wp, lp);
            SetWindowLongPtrW(hwnd_, GWLP_USERDATA, 0);
            hwnd_ = nullptr;
            return r;
        }
        default:
            break;
    }
    return DefWindowProcW(hwnd_, msg, wp, lp);
}

}  // namespace bb::platform
