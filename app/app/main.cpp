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
#include <shellapi.h>

#include <memory>
#include <string>

#include "gfx/resources.h"
#include "platform/application.h"
#include "platform/window.h"
#include "views/core.h"
#include "views/main_view.h"

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

int fail(const wchar_t* what) {
    MessageBoxW(nullptr, what, L"BertBrowser", MB_OK | MB_ICONERROR);
    return 2;
}

void mark(const wchar_t* name) {
    bb_trace_mark(bb::core::s(std::wstring(name)));
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

        HRESULT hr = S_OK;
        auto dev = gfx::GfxDevice::create(gfx::DeviceOptions{.warp = false, .video_support = true}, &hr);
        if (!dev) dev = gfx::GfxDevice::create(gfx::DeviceOptions{.warp = true}, &hr);
        if (!dev) {
            bb_launch_free(launch);
            return fail(L"No Direct3D device is available.");
        }
        gfx::Resources res(*dev);

        BbAppConfig cfg{};
        cfg.data_dir = core::s(dir);
        cfg.fakes = BB_FAKES_NONE;
        cfg.search_debounce_ms = UINT32_MAX;
        if (bb_app_create(&cfg, &app) != BB_OK) {
            const std::wstring why = core::last_error();
            bb_launch_free(launch);
            return fail(why.c_str());
        }

        {
            core::Core core(app);
            platform::WindowOptions options;
            options.quit_on_destroy = true;
            // A measurement parks the real window out of sight and never activates it.
            options.offscreen = bb_trace_enabled();
            options.activate = !options.offscreen;
            platform::Window window(res, options);
            if (!window.ok()) {
                error = L"Could not create the window.";
            } else {
                // The first thing the window draws is the content having been rendered.
                window.on_first_paint = [] { mark(L"contentRendered"); };
                views::MainView view(core, window);
                try {
                    // The folder the command line named, else the user's profile.
                    const BbStr named = bb_launch_start_path(launch);
                    std::wstring start(reinterpret_cast<const wchar_t*>(named.ptr), named.len);
                    if (start.empty()) {
                        wchar_t profile[MAX_PATH];
                        const DWORD n = GetEnvironmentVariableW(L"USERPROFILE", profile, MAX_PATH);
                        start = n ? std::wstring(profile, n) : std::wstring(L"C:\\");
                    }
                    view.start(start);
                    view.enable_file_drag_drop();
                    mark(L"windowBuilt");
                    window.show();
                    mark(L"windowShown");
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
