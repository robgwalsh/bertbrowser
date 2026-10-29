#include "session.h"

#include "canned_shell_menu.h"

#include <windows.h>

#include <chrono>

#include "sandbox.h"

namespace bbh {

namespace {
using Clock = std::chrono::steady_clock;

int ms_since(Clock::time_point t) {
    return static_cast<int>(std::chrono::duration_cast<std::chrono::milliseconds>(Clock::now() - t).count());
}

std::wstring describe_foreground(HWND h) {
    if (!h) return L"<none>";
    wchar_t title[256]{};
    const int n = GetWindowTextW(h, title, 256);
    wchar_t buf[64];
    swprintf_s(buf, L"0x%llX ", static_cast<unsigned long long>(reinterpret_cast<uintptr_t>(h)));
    return std::wstring(buf) + L"\"" + (n > 0 ? std::wstring(title, static_cast<size_t>(n)) : std::wstring(L"<untitled>")) + L"\"";
}
}  // namespace

ForegroundGuard::ForegroundGuard(HWND owner) : owner_(owner) {
    thread_ = std::thread([this] {
        const DWORD self = GetCurrentProcessId();
        while (!stop_.load()) {
            DWORD pid = 0;
            if (HWND fg = GetForegroundWindow()) GetWindowThreadProcessId(fg, &pid);
            if (pid == self) {
                ++corrections_;
                if (owner_) SetForegroundWindow(owner_);
            }
            Sleep(10);
        }
    });
}

ForegroundGuard::~ForegroundGuard() {
    stop_ = true;
    if (thread_.joinable()) thread_.join();
    if (corrections_ > 0) {
        say("# the window reached the foreground " + std::to_string(corrections_.load()) +
            " time(s); each was handed back within ~10ms.");
    }
}

std::unique_ptr<Session> Session::start(const Options& options) {
    const auto began = Clock::now();
    std::unique_ptr<Session> s(new Session(options));

    // Before anything reads the data directory: a run cannot inherit the user's database or settings.
    SetEnvironmentVariableW(L"BERTBROWSER_DATA_DIR", options.state_dir.c_str());
    fs::create_directories(options.state_dir);
    fs::create_directories(options.sandbox_dir);

    if (options.index) {
        say("# NOTE: the native harness has no index service yet; --index is accepted and ignored.");
    }

    if (!bb::platform::init_process()) throw std::runtime_error("could not initialise COM and DPI awareness");

    // WARP always: no GPU, no driver variance, same code path.
    HRESULT hr = S_OK;
    s->dev_ = bb::gfx::GfxDevice::create(bb::gfx::DeviceOptions{.warp = true}, &hr);
    if (!s->dev_) throw std::runtime_error("no WARP device is available");
    s->res_ = std::make_unique<bb::gfx::Resources>(*s->dev_);

    bb::platform::WindowOptions wo;
    wo.parked = true;
    wo.activate = false;
    wo.dpi_override = 96;  // captures do not depend on the desktop
    wo.width_dip = options.width;
    wo.height_dip = options.height;
    s->window_ = std::make_unique<bb::platform::Window>(*s->res_, wo);
    if (!s->window_->ok()) throw std::runtime_error("could not create the parked window");
    s->window_->root().caret_blink = false;
    // A scripted run never touches the user's clipboard.
    {
        auto clip = std::make_shared<std::wstring>();
        s->window_->root().clipboard = {[clip] { return *clip; }, [clip](const std::wstring& t) { *clip = t; }};
    }

    // Every hazard seam is the core's harness set: nothing is launched, elevation is declined, prompts are
    // recorded and the index is null. Asked for by name, so a missing fake cannot be resolved by accident.
    const std::wstring state = options.state_dir.wstring();
    BbAppConfig cfg{};
    cfg.data_dir = bb::core::s(state);
    cfg.fakes = BB_FAKES_HARNESS;
    cfg.search_debounce_ms = 0;
    BbAppConfigEx ex{};
    ex.base = cfg;
    ex.settings_debounce_ms = UINT32_MAX;  // the page's own debounce
    ex.index_declined = options.index_declined;
    if (bb_app_create_ex(&ex, &s->app_) != BB_OK) {
        throw std::runtime_error("bb_app_create refused: " + bb::core::utf8(bb::core::last_error()));
    }
    s->core_ = std::make_unique<bb::core::Core>(s->app_);

    if (!options.theme_id.empty()) {
        bb::core::check(bb_settings_set(s->app_, bb::core::s(L"followSystemTheme"), bb::core::s(L"false")), "followSystemTheme");
        bb::core::check(bb_settings_set(s->app_, bb::core::s(L"themeId"), bb::core::s(L"\"" + options.theme_id + L"\"")),
                        "themeId");
    }

    const HWND before = GetForegroundWindow();
    if (options.verbose) say(L"# foreground on entry: " + describe_foreground(before));
    s->guard_ = std::make_unique<ForegroundGuard>(before);

    s->view_ = std::make_unique<bb::views::MainView>(*s->core_, *s->window_);
    // Pictures are generated, so a capture never depends on what the machine's shell has cached.
    s->view_->set_thumbnail_source(bb::views::ThumbSource::Generated);
    s->view_->set_headless_dialogs(true);
    s->view_->set_shell_source(std::make_unique<CannedShellMenuSource>());
    // The OS is posed by the script (`system-theme`), never read from this desk.
    s->view_->set_follow_windows(false);
    s->window_->show();
    s->view_->start(options.start_path.empty() ? options.sandbox_dir.wstring() : options.start_path);

    // Wait for the first listing.
    const auto clock = Clock::now();
    while (ms_since(clock) < options.busy_timeout_ms) {
        s->pump();
        if (s->view_->active_tab() && !s->core_->tab_text(s->view_->active_tab(), BB_TEXT_PATH).empty() && !s->busy()) break;
        Sleep(1);
    }
    s->settle();
    s->startup_ms_ = static_cast<double>(std::chrono::duration_cast<std::chrono::microseconds>(Clock::now() - began).count()) / 1000.0;

    const HWND after = GetForegroundWindow();
    if (after != before) say(L"# WARNING: the foreground is " + describe_foreground(after) + L", not where it started.");
    return s;
}

Session::~Session() {
    if (view_) pump();
    guard_.reset();
    view_.reset();
    // The widgets belong to the window's root and call into the core as they go (the preview pane closes its
    // GIF), so they die here, while the core is alive, not when the window does after bb_app_destroy.
    if (window_) window_->root().clear_content();
    core_.reset();
    if (app_) bb_app_destroy(app_);
    res_.reset();
    dev_.reset();
}

int Session::pump() { return bb::platform::pump_messages(); }

bool Session::busy() { return core_->busy() || view_->preview_pending() || view_->tree_pending() > 0; }

void Session::settle(int quiet_ms) {
    const auto clock = Clock::now();
    while (busy() && ms_since(clock) < options_.busy_timeout_ms) {
        pump();
        Sleep(1);
    }
    if (busy()) throw TimeoutError("The window was still busy after " + std::to_string(options_.busy_timeout_ms) + " ms.");

    if (quiet_ms > 0) {
        const auto quiet = Clock::now();
        while (ms_since(quiet) < quiet_ms) {
            pump();
            Sleep(5);
        }
    }
    // The core can finish just before its last events are posted; let those land.
    for (int i = 0; i < 3; ++i) {
        while (pump() > 0) {
        }
        Sleep(2);
    }
    // And one more look: an event handled above may have started more work.
    if (busy()) settle(0);
    window_->render();
    // Tiles ask for their pictures, and rows for a program's or shortcut's own icon, as they are painted; wait
    // for those too, then paint them in.
    const auto pictures = [&] { return view_->thumbnails_pending() + view_->icons_pending(); };
    for (int i = 0; i < 400 && pictures() > 0; ++i) {
        pump();
        Sleep(5);
        window_->render();
    }
    if (pictures() == 0) {
        pump();
        window_->render();
    }
}

}  // namespace bbh
