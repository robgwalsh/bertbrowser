// The product's entry point: the real window on the hardware device. UI work is checked through
// bb-harness (offscreen), never by launching this.
#include <windows.h>

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

}  // namespace

int WINAPI wWinMain(HINSTANCE, HINSTANCE, PWSTR, int) {
    using namespace bb;
    if (!platform::init_process()) return fail(L"Could not initialise the process (COM/DPI).");

    HRESULT hr = S_OK;
    auto dev = gfx::GfxDevice::create(gfx::DeviceOptions{.warp = false, .video_support = true}, &hr);
    if (!dev) dev = gfx::GfxDevice::create(gfx::DeviceOptions{.warp = true}, &hr);
    if (!dev) return fail(L"No Direct3D device is available.");
    gfx::Resources res(*dev);

    const std::wstring dir = data_dir();
    BbAppConfig cfg{};
    cfg.data_dir = core::s(dir);
    cfg.fakes = BB_FAKES_NONE;
    cfg.search_debounce_ms = UINT32_MAX;
    BbApp* app = nullptr;
    if (bb_app_create(&cfg, &app) != BB_OK) return fail(core::last_error().c_str());

    int exit_code = 0;
    std::wstring error;
    {
        core::Core core(app);
        platform::WindowOptions options;
        options.quit_on_destroy = true;
        platform::Window window(res, options);
        if (!window.ok()) {
            error = L"Could not create the window.";
        } else {
            views::MainView view(core, window);
            try {
                wchar_t profile[MAX_PATH];
                const DWORD n = GetEnvironmentVariableW(L"USERPROFILE", profile, MAX_PATH);
                view.start(n ? std::wstring(profile, n) : std::wstring(L"C:\\"));
                view.enable_file_drag_drop();
                window.show();
                exit_code = platform::run_message_loop();
            } catch (const std::exception& e) {
                error = core::wide(e.what());
            }
        }
    }
    bb_app_destroy(app);
    if (!error.empty()) return fail(error.c_str());
    return exit_code;
}
