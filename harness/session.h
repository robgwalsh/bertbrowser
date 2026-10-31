// The window under test (tools/BertBrowser.Harness/UiSession.cs): the real MainView over the real core
// built on the harness fakes, in a window parked offscreen that draws into a bitmap on WARP. Nothing here
// can reach the user's screen or take the foreground.
#pragma once
#include "options.h"

#include "gfx/resources.h"
#include "platform/application.h"
#include "platform/window.h"
#include "views/bridge/core.h"
#include "views/main/main_view.h"

#include <atomic>
#include <memory>
#include <thread>

namespace bbh {

// Hands the foreground back if this process ever holds it (the parked window is disabled and
// non-activating, so this is the belt to that braces).
class ForegroundGuard {
public:
    explicit ForegroundGuard(HWND owner);
    ~ForegroundGuard();
    int corrections() const { return corrections_.load(); }

private:
    HWND owner_;
    std::atomic<bool> stop_{false};
    std::atomic<int> corrections_{0};
    std::thread thread_;
};

class Session {
public:
    static std::unique_ptr<Session> start(const Options& options);
    ~Session();
    Session(const Session&) = delete;
    Session& operator=(const Session&) = delete;

    const Options& options() const { return options_; }
    bb::core::Core& core() { return *core_; }
    bb::views::MainView& view() { return *view_; }
    bb::platform::Window& window() { return *window_; }
    bb::gfx::Resources& resources() { return *res_; }
    uint64_t tab() { return view_->active_tab(); }
    int foreground_corrections() const { return guard_ ? guard_->corrections() : 0; }
    double startup_ms() const { return startup_ms_; }

    // Handles everything the core has posted to the window, until nothing is left. Returns how many
    // messages were dispatched.
    int pump();
    bool busy();
    // Waits for the core to go quiet (and stay quiet for `quiet_ms`), handling events as they arrive.
    void settle(int quiet_ms = 0);
    // Whether `settle` also waits for the icon workers (on by default, so a `shot` shows the real pictures). A
    // timed verb turns it off: a row draws its glyph until its icon arrives, so the icons are not part of how
    // long the window took to answer.
    void set_wait_for_icons(bool wait) { wait_for_icons_ = wait; }
    bool wait_for_icons() const { return wait_for_icons_; }

private:
    explicit Session(const Options& o) : options_(o) {}

    Options options_;
    // Destruction runs bottom to top: the view lets go of the core's sink before the window and the app die.
    std::unique_ptr<bb::gfx::GfxDevice> dev_;
    std::unique_ptr<bb::gfx::Resources> res_;
    std::unique_ptr<bb::platform::Window> window_;
    BbApp* app_ = nullptr;
    std::unique_ptr<bb::core::Core> core_;
    std::unique_ptr<bb::views::MainView> view_;
    std::unique_ptr<ForegroundGuard> guard_;
    double startup_ms_ = 0;
    bool wait_for_icons_ = true;
};

}  // namespace bbh
