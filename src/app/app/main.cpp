// The product's entry point: the real window on the hardware device. UI work is checked through
// bb-harness (offscreen), never by launching this.
//
// Startup order (spec: App.xaml.cs): the installer's hooks first, because a hook exits the process and
// must not be gated behind anything; then the per-user instance claim, so a second launch that only
// passes its arguments on costs no device, no database and no window; then the data directory, the core
// and the window; and once there is a window, the listener for other launches and the background work.
// A launch with BERTBROWSER_STARTUP_TRACE set is a measurement: the window is parked offscreen and
// the core skips everything that acts on the machine (see bb_launch_serve).
#include <windows.h>
#include <dwmapi.h>
#include <shellapi.h>

#include <memory>
#include <string>
#include <thread>

#include "gfx/resources.h"
#include "platform/application.h"
#include "platform/window.h"
#include "views/bridge/core.h"
#include "views/main/main_view.h"

namespace {

std::wstring env(const wchar_t* name) {
    wchar_t buf[MAX_PATH * 2];
    const DWORD n = GetEnvironmentVariableW(name, buf, static_cast<DWORD>(std::size(buf)));
    return n > 0 && n < std::size(buf) ? std::wstring(buf, n) : std::wstring();
}

// Where the app's data lives: ~\.bertbrowser, or BERTBROWSER_DATA_DIR (the Rust AppPaths rule).
std::wstring data_dir() {
    std::wstring dir = env(L"BERTBROWSER_DATA_DIR");
    if (!dir.empty()) return dir;
    return env(L"USERPROFILE") + L"\\.bertbrowser";
}

void set_cloaked(HWND hwnd, bool on) {
    const BOOL value = on ? TRUE : FALSE;
    DwmSetWindowAttribute(hwnd, DWMWA_CLOAK, &value, sizeof(value));
}

int fail(const wchar_t* what) {
    MessageBoxW(nullptr, what, L"BertBrowser", MB_OK | MB_ICONERROR);
    return 2;
}

void mark(const wchar_t* name) {
    bb_trace_mark(bb::core::s(std::wstring(name)));
}

// Puts the window back where it was last closed: the size when it is a sane one, the place only when it is still
// on a screen (a monitor unplugged since must not strand it), and whether to open maximised (the answer).
// Bounds are kept in DIPs, so a change of scaling keeps the window's size as the user sees it. Spec:
// MainWindow.ApplyWindowSettings.
bool restore_placement(HWND hwnd, BbApp* app) {
    BbWindowPlacement saved{};
    if (bb_window_placement(app, &saved) != BB_OK || !saved.known) return false;
    WINDOWPLACEMENT wp{sizeof(wp)};
    if (!GetWindowPlacement(hwnd, &wp)) return saved.maximized;
    const double scale = GetDpiForWindow(hwnd) / 96.0;
    RECT r = wp.rcNormalPosition;
    if (saved.width > 200 && saved.height > 150) {
        r.right = r.left + static_cast<LONG>(saved.width * scale);
        r.bottom = r.top + static_cast<LONG>(saved.height * scale);
    }
    const LONG left = static_cast<LONG>(saved.left * scale), top = static_cast<LONG>(saved.top * scale);
    const int vx = GetSystemMetrics(SM_XVIRTUALSCREEN), vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
    const int vw = GetSystemMetrics(SM_CXVIRTUALSCREEN), vh = GetSystemMetrics(SM_CYVIRTUALSCREEN);
    const LONG margin = static_cast<LONG>(100 * scale);
    if (left > vx - margin && top > vy - margin && left < vx + vw - margin && top < vy + vh - margin) {
        OffsetRect(&r, left - r.left, top - r.top);
    }
    wp.rcNormalPosition = r;
    wp.showCmd = SW_HIDE;  // shown by the caller, maximised or not
    SetWindowPlacement(hwnd, &wp);
    return saved.maximized;
}

// What closing remembers: the restored bounds even when maximised, so un-maximising next time lands where it
// was. Spec: MainWindow.SaveWindowSettings (RestoreBounds).
void save_placement(HWND hwnd, BbApp* app) {
    WINDOWPLACEMENT wp{sizeof(wp)};
    if (!GetWindowPlacement(hwnd, &wp)) return;
    const double scale = GetDpiForWindow(hwnd) / 96.0;
    const RECT& r = wp.rcNormalPosition;
    BbWindowPlacement p{};
    p.known = true;
    p.left = r.left / scale;
    p.top = r.top / scale;
    p.width = (r.right - r.left) / scale;
    p.height = (r.bottom - r.top) / scale;
    p.maximized = IsZoomed(hwnd) != FALSE;
    bb_window_placement_set(app, &p);
}

// The command line as UTF-16 pointers, program name first. Freed with LocalFree.
struct Args {
    int argc = 0;
    LPWSTR* argv = nullptr;
    Args() : argv(CommandLineToArgvW(GetCommandLineW(), &argc)) {}
    ~Args() {
        if (argv) LocalFree(argv);
    }
    Args(const Args&) = delete;
    Args& operator=(const Args&) = delete;
    const uint16_t* const* wide() const { return reinterpret_cast<const uint16_t* const*>(argv); }
};

}  // namespace

int WINAPI wWinMain(HINSTANCE, HINSTANCE, PWSTR, int) {
    using namespace bb;
    Args args;

    // The installer's hooks (--veloapp-*) and a waiting update. Exits the process for a hook.
    if (bb_startup(args.argc, args.wide()) != BB_OK) return fail(core::last_error().c_str());

    const std::wstring dir = data_dir();
    BbLaunch* launch = nullptr;
    if (bb_launch_claim(args.argc, args.wide(), core::s(dir), &launch) != BB_OK) return fail(core::last_error().c_str());
    // Another copy is running and took the request (and, if the shell started us, our right to
    // take the foreground): nothing more to do.
    if (bb_launch_handed_off(launch)) {
        bb_launch_free(launch);
        return 0;
    }

    int exit_code = 0;
    std::wstring error;
    BbApp* app = nullptr;
    {
        if (!platform::init_process()) {
            bb_launch_free(launch);
            return fail(L"Could not initialise the process (COM/DPI).");
        }
        mark(L"comReady");

        // Loading a GPU's driver takes ~200 ms and the first frame does not need it: start on WARP (8 ms), and once
        // the window is up make the hardware device on a thread of its own and move over when it is ready. Setting
        // BERTBROWSER_DEVICE=hardware waits for the GPU first, as the app did before.
        wchar_t device_mode[16] = {};
        GetEnvironmentVariableW(L"BERTBROWSER_DEVICE", device_mode, 16);
        const bool hardware_first = std::wstring(device_mode) == L"hardware";
        HRESULT hr = S_OK;
        std::unique_ptr<gfx::GfxDevice> dev;
        std::unique_ptr<gfx::HardwareDeviceLoader> hardware;
        // WARP and the core have nothing to do with each other, and the device takes the longer: each its own thread,
        // the core on this one because the FFI is the UI thread's. The join hands the device over.
        std::jthread warp_maker;
        if (!hardware_first) {
            warp_maker = std::jthread([&dev, &hr] { dev = gfx::GfxDevice::create(gfx::DeviceOptions{.warp = true}, &hr); });
        }

        BbAppConfig cfg{};
        cfg.data_dir = core::s(dir);
        cfg.fakes = BB_FAKES_NONE;
        cfg.search_debounce_ms = UINT32_MAX;
        if (bb_app_create(&cfg, &app) != BB_OK) {
            const std::wstring why = core::last_error();
            bb_launch_free(launch);
            return fail(why.c_str());
        }
        mark(L"appCreated");

        if (warp_maker.joinable()) warp_maker.join();
        const bool move_to_hardware = dev != nullptr;
        if (!dev) dev = gfx::GfxDevice::create(gfx::DeviceOptions{.warp = false, .video_support = true}, &hr);
        if (!dev) dev = gfx::GfxDevice::create(gfx::DeviceOptions{.warp = true}, &hr);
        if (!dev) {
            bb_app_destroy(app);
            bb_launch_free(launch);
            return fail(L"No Direct3D device is available.");
        }
        gfx::Resources res(*dev);
        mark(L"deviceReady");

        {
            core::Core core(app);
            platform::WindowOptions options;
            options.quit_on_destroy = true;
            // A measurement parks the real window out of sight and never activates it.
            options.offscreen = bb_trace_enabled();
            options.activate = !options.offscreen;
            // Spec: MainWindow.xaml (Width 1200, Height 750, MinWidth 640).
            options.width_dip = 1200;
            options.height_dip = 750;
            options.min_width_dip = 640;
            options.min_height_dip = 320;
            platform::Window window(res, options);
            if (!window.ok()) {
                error = L"Could not create the window.";
            } else {
                mark(L"windowCreated");
                // The first thing the window draws is the content having been rendered.
                window.on_first_paint = [] { mark(L"contentRendered"); };
                views::MainView view(core, window);
                mark(L"mainViewBuilt");
                // The first frame to hold the rows, which need not be the first frame.
                if (bb_trace_enabled()) {
                    window.on_painted = [&view] {
                        if (!view.listing_in_view()) return false;
                        mark(L"listingPainted");
                        return true;
                    };
                }
                // A handler that throws says why in the status bar instead of taking the window with it.
                platform::Window::on_unhandled = [app](const std::exception& e) {
                    bb_status_set(app, core::s(core::message_of(e)));
                };
                try {
                    // The folder the command line named; else last session's panes (or the startup folder, as
                    // Settings says), falling back to the user's profile. Someone who named a folder asked for
                    // it, so reopening the session over it would bury it. Spec: App.OnStartup (start path priority).
                    const BbStr named = bb_launch_start_path(launch);
                    std::wstring start(reinterpret_cast<const wchar_t*>(named.ptr), named.len);
                    const DWORD attrs = start.empty() ? INVALID_FILE_ATTRIBUTES : GetFileAttributesW(start.c_str());
                    const bool asked = attrs != INVALID_FILE_ATTRIBUTES && (attrs & FILE_ATTRIBUTE_DIRECTORY);
                    if (!asked) {
                        wchar_t profile[MAX_PATH];
                        const DWORD n = GetEnvironmentVariableW(L"USERPROFILE", profile, MAX_PATH);
                        start = n ? std::wstring(profile, n) : std::wstring(L"C:\\");
                    }
                    view.start(start, !asked);
                    mark(L"sessionStarted");
                    mark(L"windowBuilt");
                    const bool maximize = !options.offscreen && restore_placement(window.hwnd(), app);
                    if (!options.offscreen) {
                        window.on_close = [&window, app] {
                            window.set_full_screen(false);  // where it was, not the monitor a video covered
                            save_placement(window.hwnd(), app);
                            DestroyWindow(window.hwnd());
                        };
                    }
                    // Maximised, it takes its real size out of sight: painted first and maximised after, the
                    // first frame would be thrown away at once for another of the new size.
                    if (maximize) {
                        set_cloaked(window.hwnd(), true);
                        ShowWindow(window.hwnd(), SW_SHOWMAXIMIZED);
                    }
                    // The rows have usually landed by now, with the event that says so still queued. Painted
                    // without them the window would appear empty and need a second whole frame.
                    view.await_first_listing(25);
                    window.paint_first_frame();  // the window appears with its content, not blank
                    if (maximize) set_cloaked(window.hwnd(), false);
                    else window.show();
                    mark(L"windowShown");
                    // Only now: a driver loading holds locks the first frame would otherwise wait on (the two
                    // device creations share one inside D3D), and nothing on screen is waiting for it.
                    if (move_to_hardware) {
                        window.on_hardware_settled = [](bool moved) {
                            if (moved) mark(L"gfxHardware");
                            mark(L"gfxSettled");
                        };
                        hardware = std::make_unique<gfx::HardwareDeviceLoader>(true);
                        window.use_hardware_when_ready(*hardware);
                    } else {
                        mark(L"gfxSettled");  // drawing on the GPU from the start (or only ever on WARP)
                    }
                    // About 7 ms to register, and nothing can be dropped on a window before it is up. Still ahead
                    // of the message loop, so no gesture can arrive first.
                    view.enable_file_drag_drop();
                    // Other launches are answered from here on, the rest of the command line is
                    // opened, and the background work (or, for a measurement, the trace) starts.
                    if (bb_launch_serve(launch, app, window.hwnd()) != BB_OK) error = core::last_error();
                    if (error.empty()) exit_code = platform::run_message_loop();
                } catch (const std::exception& e) {
                    error = core::wide(e.what());
                }
            }
        }
        bb_app_destroy(app);
    }
    // After the app: the instance claim is the last thing to go.
    bb_launch_free(launch);
    if (!error.empty()) return fail(error.c_str());
    return exit_code;
}
