// Spike S8: custom window chrome without WPF's WindowChrome. The app draws its own title bar, so the
// window must: drop the standard caption but keep the DWM resize frame (WM_NCCALCSIZE); answer
// WM_NCHITTEST with HTCAPTION for the bar, HTMAXBUTTON for the maximise button (which is what makes
// Windows 11 offer snap layouts), HTTOP and the corners for the edge we took over; clamp a maximised
// window to the work area (WM_GETMINMAXINFO), or the frame hangs off the screen and covers the
// taskbar; undo the Win32-level disable a modal child applies to its owner (WM_ENABLE), so the
// title bar keeps working under a dialog as ThemedWindow.cs does; tell DWM the frame colours
// (attributes 20, 33 to 36); and load the icon at the window's DPI.
//
// Everything is checked by sending the messages to a parked, non-activating window and reading the
// answers, so nothing appears on screen. What this cannot check is how it looks; that waits for the
// first real window at M5.
#include <windows.h>

#include <dwmapi.h>
#include <shellapi.h>
#include <windowsx.h>

#include <cstdio>
#include <string>

namespace {

int failures = 0;

void check(bool ok, const char* what) {
    if (!ok) {
        ++failures;
        std::printf("S8 FAIL: %s\n", what);
    }
}

constexpr int kTitleDip = 32;
constexpr int kButtonDip = 46;

struct Chrome {
    int enable_messages = 0;
    bool reenabling = false;
};

int scale(int dip, UINT dpi) { return MulDiv(dip, static_cast<int>(dpi), 96); }

int frame_x(UINT dpi) { return GetSystemMetricsForDpi(SM_CXFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi); }
int frame_y(UINT dpi) { return GetSystemMetricsForDpi(SM_CYFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi); }

// The caption buttons, in client coordinates: close at the right edge, then maximise, then minimise.
RECT button_rect(HWND hwnd, int index_from_right) {
    RECT client{};
    GetClientRect(hwnd, &client);
    const UINT dpi = GetDpiForWindow(hwnd);
    const int w = scale(kButtonDip, dpi);
    const int h = scale(kTitleDip, dpi);
    const int top = IsZoomed(hwnd) ? frame_y(dpi) : 0;
    return RECT{client.right - w * (index_from_right + 1), top, client.right - w * index_from_right, top + h};
}

LRESULT hit_test(HWND hwnd, LPARAM lp) {
    const LRESULT def = DefWindowProcW(hwnd, WM_NCHITTEST, 0, lp);
    if (def != HTCLIENT) {
        return def;  // the left, right and bottom resize frames are still DefWindowProc's
    }
    POINT pt{GET_X_LPARAM(lp), GET_Y_LPARAM(lp)};
    ScreenToClient(hwnd, &pt);
    RECT client{};
    GetClientRect(hwnd, &client);
    const UINT dpi = GetDpiForWindow(hwnd);
    if (!IsZoomed(hwnd) && pt.y < frame_y(dpi)) {
        // The top frame is ours now, since WM_NCCALCSIZE gave that strip to the client area.
        if (pt.x < frame_x(dpi)) return HTTOPLEFT;
        if (pt.x >= client.right - frame_x(dpi)) return HTTOPRIGHT;
        return HTTOP;
    }
    const int top = IsZoomed(hwnd) ? frame_y(dpi) : 0;
    if (pt.y < top + scale(kTitleDip, dpi)) {
        const RECT close = button_rect(hwnd, 0), max = button_rect(hwnd, 1), min = button_rect(hwnd, 2);
        if (PtInRect(&close, pt)) return HTCLOSE;
        if (PtInRect(&max, pt)) return HTMAXBUTTON;
        if (PtInRect(&min, pt)) return HTMINBUTTON;
        return HTCAPTION;
    }
    return HTCLIENT;
}

void clamp_to_work_area(HWND hwnd, LPARAM lp) {
    const HMONITOR monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    MONITORINFO info{sizeof(info)};
    if (!monitor || !GetMonitorInfoW(monitor, &info)) return;
    auto* mm = reinterpret_cast<MINMAXINFO*>(lp);
    mm->ptMaxPosition.x = info.rcWork.left - info.rcMonitor.left;
    mm->ptMaxPosition.y = info.rcWork.top - info.rcMonitor.top;
    mm->ptMaxSize.x = info.rcWork.right - info.rcWork.left;
    mm->ptMaxSize.y = info.rcWork.bottom - info.rcWork.top;
    mm->ptMaxTrackSize = mm->ptMaxSize;
}

LRESULT CALLBACK wnd_proc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp) {
    auto* chrome = reinterpret_cast<Chrome*>(GetWindowLongPtrW(hwnd, GWLP_USERDATA));
    switch (msg) {
        case WM_NCCALCSIZE:
            if (wp) {
                // Keep DefWindowProc's side and bottom frames (DWM draws the resize borders and the
                // shadow for them) and take the top back, so there is no caption. Maximised, the
                // frame would hang off the monitor, so inset the top by the frame height instead.
                auto* params = reinterpret_cast<NCCALCSIZE_PARAMS*>(lp);
                const LONG top = params->rgrc[0].top;
                DefWindowProcW(hwnd, msg, wp, lp);
                params->rgrc[0].top = top + (IsZoomed(hwnd) ? frame_y(GetDpiForWindow(hwnd)) : 0);
                return 0;
            }
            break;
        case WM_NCHITTEST:
            return hit_test(hwnd, lp);
        case WM_GETMINMAXINFO:
            clamp_to_work_area(hwnd, lp);
            return 0;
        case WM_NCLBUTTONDOWN:
            if (wp == HTMAXBUTTON) return 0;  // or a caption drag starts on the button
            break;
        case WM_NCLBUTTONUP:
            if (wp == HTMAXBUTTON) {
                ShowWindow(hwnd, IsZoomed(hwnd) ? SW_RESTORE : SW_MAXIMIZE);
                return 0;
            }
            break;
        case WM_ENABLE:
            // A modal child disables its owner at the Win32 level; undo that at once so the title
            // bar stays draggable and closable. The content block is the app's own business.
            if (chrome) {
                ++chrome->enable_messages;
                if (wp == FALSE && !chrome->reenabling) {
                    chrome->reenabling = true;
                    EnableWindow(hwnd, TRUE);
                    chrome->reenabling = false;
                }
            }
            break;
        default:
            break;
    }
    return DefWindowProcW(hwnd, msg, wp, lp);
}

LRESULT send_hit(HWND hwnd, int client_x, int client_y) {
    POINT pt{client_x, client_y};
    ClientToScreen(hwnd, &pt);
    return SendMessageW(hwnd, WM_NCHITTEST, 0, MAKELPARAM(static_cast<short>(pt.x), static_cast<short>(pt.y)));
}

}  // namespace

int wmain() {
    const HWND foreground_before = GetForegroundWindow();
    SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

    WNDCLASSW wc{};
    wc.lpfnWndProc = wnd_proc;
    wc.hInstance = GetModuleHandleW(nullptr);
    wc.lpszClassName = L"BbSpikeS8";
    wc.hCursor = LoadCursorW(nullptr, IDC_ARROW);
    RegisterClassW(&wc);
    Chrome chrome;
    const HWND hwnd = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, wc.lpszClassName, L"s8",
                                      WS_OVERLAPPEDWINDOW, -32000, -32000, 800, 500, nullptr, nullptr, wc.hInstance,
                                      nullptr);
    check(hwnd != nullptr, "CreateWindowExW");
    SetWindowLongPtrW(hwnd, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(&chrome));
    // The frame change has to be applied once the handler is in place.
    SetWindowPos(hwnd, nullptr, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
    ShowWindow(hwnd, SW_SHOWNOACTIVATE);

    const UINT dpi = GetDpiForWindow(hwnd);
    std::printf("S8 dpi %u, frame %dx%d px\n", dpi, frame_x(dpi), frame_y(dpi));

    // 1. No caption: the client area starts at the window's top edge, the side frames remain.
    RECT win{}, client{};
    GetWindowRect(hwnd, &win);
    GetClientRect(hwnd, &client);
    POINT origin{0, 0};
    ClientToScreen(hwnd, &origin);
    check(origin.y == win.top, "client area begins at the top of the window (caption removed)");
    check(origin.x == win.left + frame_x(dpi), "left resize frame kept");
    check(client.bottom == (win.bottom - win.top) - frame_y(dpi), "bottom resize frame kept");
    std::printf("S8 window %ldx%ld, client %ldx%ld\n", win.right - win.left, win.bottom - win.top, client.right,
                client.bottom);

    // 2. Hit testing, as Windows will ask it.
    const int title_mid = scale(kTitleDip, dpi) / 2;
    check(send_hit(hwnd, client.right / 2, title_mid) == HTCAPTION, "middle of the bar is HTCAPTION");
    const RECT max = button_rect(hwnd, 1);
    check(send_hit(hwnd, (max.left + max.right) / 2, (max.top + max.bottom) / 2) == HTMAXBUTTON,
          "maximise button is HTMAXBUTTON (snap layouts)");
    const RECT close = button_rect(hwnd, 0);
    check(send_hit(hwnd, (close.left + close.right) / 2, title_mid) == HTCLOSE, "close button is HTCLOSE");
    const RECT min = button_rect(hwnd, 2);
    check(send_hit(hwnd, (min.left + min.right) / 2, title_mid) == HTMINBUTTON, "minimise button is HTMINBUTTON");
    check(send_hit(hwnd, client.right / 2, 1) == HTTOP, "top strip is HTTOP");
    check(send_hit(hwnd, 1, 1) == HTTOPLEFT, "top-left corner is HTTOPLEFT");
    check(send_hit(hwnd, client.right - 2, 1) == HTTOPRIGHT, "top-right corner is HTTOPRIGHT");
    check(send_hit(hwnd, client.right / 2, client.bottom / 2) == HTCLIENT, "content is HTCLIENT");
    {
        POINT edge{win.left + 2, (win.top + win.bottom) / 2};
        const LRESULT r = SendMessageW(hwnd, WM_NCHITTEST, 0, MAKELPARAM(static_cast<short>(edge.x), static_cast<short>(edge.y)));
        check(r == HTLEFT, "left frame is still HTLEFT from DefWindowProc");
    }

    // 3. Maximise clamps to the work area and keeps the bar reachable.
    {
        MINMAXINFO mm{};
        SendMessageW(hwnd, WM_GETMINMAXINFO, 0, reinterpret_cast<LPARAM>(&mm));
        MONITORINFO info{sizeof(info)};
        GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &info);
        check(mm.ptMaxSize.x == info.rcWork.right - info.rcWork.left && mm.ptMaxSize.y == info.rcWork.bottom - info.rcWork.top,
              "maximised size is the work area, not the monitor");
        check(mm.ptMaxPosition.x == info.rcWork.left - info.rcMonitor.left && mm.ptMaxPosition.y == info.rcWork.top - info.rcMonitor.top,
              "maximised position is the work area's corner");
    }
    SendMessageW(hwnd, WM_NCLBUTTONDOWN, HTMAXBUTTON, 0);
    SendMessageW(hwnd, WM_NCLBUTTONUP, HTMAXBUTTON, 0);
    check(IsZoomed(hwnd) != 0, "a non-client click on the maximise button maximises");
    {
        RECT z{}, zc{};
        GetWindowRect(hwnd, &z);
        GetClientRect(hwnd, &zc);
        POINT zo{0, 0};
        ClientToScreen(hwnd, &zo);
        check(zo.y == z.top + frame_y(dpi), "maximised, the top frame is inset so the bar is on screen");
        check(send_hit(hwnd, zc.right / 2, frame_y(dpi) + title_mid) == HTCAPTION, "maximised bar is still HTCAPTION");
        check(send_hit(hwnd, zc.right / 2, 1) == HTCAPTION, "maximised, the top strip is the bar, not a resize edge");
    }
    SendMessageW(hwnd, WM_NCLBUTTONUP, HTMAXBUTTON, 0);
    check(IsZoomed(hwnd) == 0, "a second click restores");

    // 4. A modal child's EnableWindow(owner, FALSE) is undone at once.
    EnableWindow(hwnd, FALSE);
    check(IsWindowEnabled(hwnd) != 0, "WM_ENABLE(FALSE) is reversed so the title bar stays live");
    check(chrome.enable_messages >= 2, "the re-enable went through WM_ENABLE once more and did not recurse");

    // 5. DWM frame attributes. 20 and 33 are Windows 10/11; 34 to 36 are Windows 11 and may be
    // refused on an older build, which is reported rather than failed.
    {
        const BOOL dark = TRUE;
        const HRESULT h20 = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &dark, sizeof(dark));
        const DWM_WINDOW_CORNER_PREFERENCE corner = DWMWCP_ROUND;
        const HRESULT h33 = DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &corner, sizeof(corner));
        const COLORREF border = RGB(0x3c, 0x3c, 0x3c), caption = RGB(0x1e, 0x1e, 0x1e), text = RGB(0xe0, 0xe0, 0xe0);
        const HRESULT h34 = DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR, &border, sizeof(border));
        const HRESULT h35 = DwmSetWindowAttribute(hwnd, DWMWA_CAPTION_COLOR, &caption, sizeof(caption));
        const HRESULT h36 = DwmSetWindowAttribute(hwnd, DWMWA_TEXT_COLOR, &text, sizeof(text));
        check(SUCCEEDED(h20), "DWMWA_USE_IMMERSIVE_DARK_MODE accepted");
        check(SUCCEEDED(h33), "DWMWA_WINDOW_CORNER_PREFERENCE accepted");
        std::printf("S8 dwm: dark 0x%08lx, corners 0x%08lx, border 0x%08lx, caption 0x%08lx, text 0x%08lx\n",
                    static_cast<unsigned long>(h20), static_cast<unsigned long>(h33), static_cast<unsigned long>(h34),
                    static_cast<unsigned long>(h35), static_cast<unsigned long>(h36));
    }

    // 6. The icon at the window's DPI, for the taskbar and the bar.
    {
        HICON icon_small = nullptr, icon_big = nullptr;
        const int cx = GetSystemMetricsForDpi(SM_CXSMICON, dpi), cy = GetSystemMetricsForDpi(SM_CYSMICON, dpi);
        // LoadImageW with an explicit size picks the best frame and scales; LoadIconWithScaleDown
        // would need the ComCtl32 v6 manifest the real app carries and a spike does not.
        icon_small = static_cast<HICON>(LoadImageW(nullptr, IDI_APPLICATION, IMAGE_ICON, cx, cy, LR_SHARED));
        icon_big = static_cast<HICON>(LoadImageW(nullptr, IDI_APPLICATION, IMAGE_ICON, GetSystemMetricsForDpi(SM_CXICON, dpi),
                                                 GetSystemMetricsForDpi(SM_CYICON, dpi), LR_SHARED));
        check(icon_small != nullptr, "small icon scaled for the DPI");
        check(icon_big != nullptr, "large icon scaled for the DPI");
        SendMessageW(hwnd, WM_SETICON, ICON_SMALL, reinterpret_cast<LPARAM>(icon_small));
        SendMessageW(hwnd, WM_SETICON, ICON_BIG, reinterpret_cast<LPARAM>(icon_big));
        check(reinterpret_cast<HICON>(SendMessageW(hwnd, WM_GETICON, ICON_SMALL, 0)) == icon_small, "WM_SETICON took");
    }

    DestroyWindow(hwnd);
    check(GetForegroundWindow() == foreground_before, "foreground untouched");
    if (failures) {
        std::printf("S8 FAILED: %d check(s)\n", failures);
        return 1;
    }
    std::printf("S8 OK: caption removed with the resize frame kept, hit tests (incl. HTMAXBUTTON), work-area clamp, WM_ENABLE undo, DWM attributes, per-DPI icon\n");
    return 0;
}
