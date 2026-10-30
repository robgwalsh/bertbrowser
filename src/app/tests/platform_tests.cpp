#include "fixture.h"

#include <windowsx.h>

using namespace bb;
using namespace bb::ui;

namespace {
LRESULT send_hit(HWND hwnd, int client_x, int client_y) {
    POINT pt{client_x, client_y};
    ClientToScreen(hwnd, &pt);
    return SendMessageW(hwnd, WM_NCHITTEST, 0, MAKELPARAM(static_cast<short>(pt.x), static_cast<short>(pt.y)));
}
}  // namespace

TEST(parked_window_never_reaches_the_screen) {
    const HWND before = GetForegroundWindow();
    Rig rig(640, 400);
    HWND hwnd = rig.win.hwnd();
    const LONG ex = GetWindowLongW(hwnd, GWL_EXSTYLE);
    CHECK((ex & WS_EX_TOOLWINDOW) != 0);
    CHECK((ex & WS_EX_NOACTIVATE) != 0);
    CHECK(IsWindowEnabled(hwnd) == FALSE);
    RECT wr{}, cr{};
    GetWindowRect(hwnd, &wr);
    GetClientRect(hwnd, &cr);
    CHECK_EQ(wr.left, -32000);
    CHECK_EQ(wr.top, -32000);
    CHECK_EQ(cr.right, 640);  // the client area is exactly the asked size in pixels
    CHECK_EQ(cr.bottom, 400);
    CHECK(rig.win.bitmap() != nullptr);
    CHECK_EQ(rig.win.dpi(), 96);
    CHECK(GetForegroundWindow() == before);
    // Layout is in DIPs: the surface is 640x400 DIPs at 96 dpi.
    CHECK_NEAR(rig.win.surface().width_dip(), 640.0, 0.01);
}

TEST(chrome_hit_tests_follow_widgets) {
    Rig rig(800, 500);
    auto dock = std::make_unique<DockPanel>();
    auto* bar = dock->emplace<DockPanel>();
    bar->dock = Dock::Top;
    bar->height = 32;
    bar->nc_hit = HTCAPTION;
    auto* close = bar->emplace<Button>(L"x");
    close->dock = Dock::Right;
    close->width = 46;
    close->nc_hit = HTCLOSE;
    auto* max = bar->emplace<Button>(L"o");
    max->dock = Dock::Right;
    max->width = 46;
    max->nc_hit = HTMAXBUTTON;
    auto* body = dock->emplace<Panel>();
    rig.set(std::move(dock));

    HWND hwnd = rig.win.hwnd();
    CHECK_EQ(send_hit(hwnd, 300, 16), HTCAPTION);
    CHECK_EQ(send_hit(hwnd, static_cast<int>(Rig::center(max).x), 16), HTMAXBUTTON);  // snap layouts need this
    CHECK_EQ(send_hit(hwnd, static_cast<int>(Rig::center(close).x), 16), HTCLOSE);
    CHECK_EQ(send_hit(hwnd, 400, 1), HTTOP);        // the top strip is ours: WM_NCCALCSIZE gave it to the client
    CHECK_EQ(send_hit(hwnd, 1, 1), HTTOPLEFT);
    CHECK_EQ(send_hit(hwnd, 798, 1), HTTOPRIGHT);
    CHECK_EQ(send_hit(hwnd, 400, 300), HTCLIENT);   // body: no role
    (void)body;
    RECT win{};
    GetWindowRect(hwnd, &win);
    const LPARAM edge = MAKELPARAM(static_cast<short>(win.left + 2), static_cast<short>((win.top + win.bottom) / 2));
    CHECK_EQ(SendMessageW(hwnd, WM_NCHITTEST, 0, edge), HTLEFT);  // side frames remain DefWindowProc's

    // The caption has no standard frame: the client area starts at the window's top edge.
    POINT origin{0, 0};
    ClientToScreen(hwnd, &origin);
    CHECK_EQ(origin.y, win.top);

    // Maximise is clamped to the work area, not the monitor.
    MINMAXINFO mm{};
    SendMessageW(hwnd, WM_GETMINMAXINFO, 0, reinterpret_cast<LPARAM>(&mm));
    MONITORINFO info{sizeof(info)};
    GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &info);
    CHECK_EQ(mm.ptMaxSize.x, info.rcWork.right - info.rcWork.left);
    CHECK_EQ(mm.ptMaxSize.y, info.rcWork.bottom - info.rcWork.top);

    // A click on the close button asks the window to close; the parked window stays where it is.
    bool closed = false;
    rig.win.on_close = [&] { closed = true; };
    SendMessageW(hwnd, WM_NCLBUTTONUP, HTCLOSE, 0);
    MSG m;
    while (PeekMessageW(&m, hwnd, WM_CLOSE, WM_CLOSE, PM_REMOVE)) DispatchMessageW(&m);
    CHECK(closed);
    SendMessageW(hwnd, WM_NCLBUTTONUP, HTMAXBUTTON, 0);
    CHECK(IsZoomed(hwnd) == FALSE);  // never maximised: that would put it on the screen
}

TEST(hidden_window_chrome_and_swap_chain) {
    // Not parked and never shown: a real swap-chain surface and the WM_ENABLE undo, nothing on screen.
    platform::WindowOptions o;
    o.width_dip = 500;
    o.height_dip = 300;
    platform::Window w(*Env::get().res, o);
    CHECK(w.ok());
    CHECK(w.bitmap() == nullptr);
    CHECK(w.dpi() >= 96);
    auto* label = w.root().set_content(std::make_unique<TextBlock>(L"swap chain"));
    w.root().layout({w.surface().width_dip(), w.surface().height_dip()});
    (void)label;
    CHECK(SUCCEEDED(w.render()));
    CHECK_EQ(w.root().dirty().empty(), 1);
    w.root().invalidate(Rect{0, 0, 40, 20});
    CHECK(SUCCEEDED(w.render()));  // a dirty-rect present
    // A modal child disables its owner; the chrome undoes it at once so the title bar stays live.
    EnableWindow(w.hwnd(), FALSE);
    CHECK(IsWindowEnabled(w.hwnd()) != FALSE);
    // DPI round trip.
    const POINT px = w.to_px({100, 50});
    const Point back = w.to_dip(px.x, px.y);
    CHECK_NEAR(back.x, 100.0, 1.0);
    CHECK_NEAR(back.y, 50.0, 1.0);
}

TEST(posted_messages_drive_widgets) {
    Rig rig(500, 300);
    auto stack = std::make_unique<StackPanel>(Orientation::Vertical, 4.0f);
    auto* edit = stack->emplace<Edit>();
    auto* button = stack->emplace<Button>(L"Go");
    button->width = 80;
    rig.set(std::move(stack));
    int clicks = 0;
    button->on_click = [&] { ++clicks; };

    HWND hwnd = rig.win.hwnd();
    const Point pe = Rig::center(edit);
    PostMessageW(hwnd, WM_LBUTTONDOWN, MK_LBUTTON, MAKELPARAM(static_cast<int>(pe.x), static_cast<int>(pe.y)));
    PostMessageW(hwnd, WM_LBUTTONUP, 0, MAKELPARAM(static_cast<int>(pe.x), static_cast<int>(pe.y)));
    for (wchar_t c : std::wstring(L"hi!")) PostMessageW(hwnd, WM_CHAR, c, 0);
    PostMessageW(hwnd, WM_KEYDOWN, VK_LEFT, 0);
    PostMessageW(hwnd, WM_KEYUP, VK_LEFT, 0);
    PostMessageW(hwnd, WM_CHAR, L'X', 0);
    const Point pb = Rig::center(button);
    PostMessageW(hwnd, WM_LBUTTONDOWN, MK_LBUTTON, MAKELPARAM(static_cast<int>(pb.x), static_cast<int>(pb.y)));
    PostMessageW(hwnd, WM_LBUTTONUP, 0, MAKELPARAM(static_cast<int>(pb.x), static_cast<int>(pb.y)));
    platform::pump_messages();
    CHECK_TEXT(edit->text(), L"hiX!");
    CHECK_EQ(clicks, 1);
    CHECK(rig.root.focused() == button);  // clicking the button moved focus to it
}

TEST(timer_service_drives_ticks) {
    Rig rig(300, 100);
    auto* edit = rig.set(std::make_unique<Edit>());
    rig.root.caret_blink = true;
    rig.root.set_focus(edit);  // focus starts the 530 ms blink
    CHECK(rig.root.has_timers());
    rig.render();
    CHECK(rig.root.dirty().empty());
    rig.now += 600;
    rig.root.tick();  // the window's WM_TIMER does this
    CHECK(!rig.root.dirty().empty());  // the caret toggled and asked to be repainted
    rig.root.set_focus(nullptr);
    CHECK(!rig.root.has_timers());  // losing focus stops it
}

TEST(a_window_moves_to_the_hardware_device_when_it_is_ready_and_its_neighbour_follows) {
    // Its own device: adopting one moves the generation, which the shared one's other tests must not see.
    HRESULT hr = E_FAIL;
    auto dev = gfx::GfxDevice::create(gfx::DeviceOptions{.warp = true}, &hr);
    CHECK(dev != nullptr);
    if (!dev) return;
    gfx::Resources res(*dev);
    platform::WindowOptions o;
    o.width_dip = 300;
    o.height_dip = 200;
    platform::Window main(res, o);
    platform::Window neighbour(res, o);  // a tool window: it hears nothing, it only draws on the same device
    CHECK(main.ok() && neighbour.ok());
    main.root().set_content(std::make_unique<TextBlock>(L"on WARP"));
    main.root().layout({main.surface().width_dip(), main.surface().height_dip()});
    CHECK(SUCCEEDED(main.render()));
    const uint32_t warp_generation = dev->generation();

    gfx::HardwareDeviceLoader loader(false);
    bool settled = false;
    bool moved = false;
    main.on_hardware_settled = [&](bool m) {
        settled = true;
        moved = m;
    };
    main.use_hardware_when_ready(loader);
    for (int i = 0; i < 1000 && !settled; ++i) {
        platform::pump_messages();
        Sleep(10);
    }
    CHECK(settled);  // said so whether or not there was a GPU
    if (!moved) return;  // no hardware adapter on this machine: nothing to move to

    CHECK(!dev->is_warp());
    CHECK(dev->generation() != warp_generation);
    CHECK_EQ(main.surface().generation(), dev->generation());
    CHECK(SUCCEEDED(main.render()));
    // The neighbour still holds a swap chain of the old device until it next paints, and then follows.
    CHECK(neighbour.surface().generation() != dev->generation());
    neighbour.root().invalidate_all();
    CHECK(SUCCEEDED(neighbour.render()));
    CHECK_EQ(neighbour.surface().generation(), dev->generation());
}
