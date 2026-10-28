// bb-harness: the offscreen UI harness. A separate exe over the same views as the product (never a mode
// of BertBrowser.exe), with every hazard seam faked, a parked window and a bitmap surface on WARP.
//   bb-harness.exe --script tools/ui/smoke.bbs
// Exit codes: 0 ok, 1 a command failed, 2 the environment, 3 the watchdog, 64 usage.
#include <windows.h>

#include <atomic>
#include <chrono>
#include <thread>

#include "options.h"
#include "runner.h"
#include "session.h"

namespace {

void remove_scratch_state(const bbh::Options& o) {
    // The database keeps a WAL and the index watcher patches it while files move; retry for a few
    // seconds, then report rather than fail.
    for (int attempt = 0;; ++attempt) {
        std::error_code ec;
        if (!bbh::fs::exists(o.state_dir, ec)) return;
        bbh::fs::remove_all(o.state_dir, ec);
        if (!ec) return;
        if (attempt >= 29) {
            bbh::say("# could not remove the scratch state directory: " + ec.message());
            return;
        }
        Sleep(200);
    }
}

}  // namespace

int wmain(int argc, wchar_t** argv) {
    SetConsoleOutputCP(CP_UTF8);
    int exit_code = 0;
    auto options = bbh::parse_options(argc, argv, exit_code);
    if (!options) return exit_code;
    const bbh::Options& o = *options;

    // Nothing here has a window the user could find and close, so a run that wedged would sit in the
    // background for ever. The watchdog is what makes an invisible process safe to start.
    std::atomic<bool> finished{false};
    std::thread watchdog([&] {
        const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(o.timeout_seconds);
        while (!finished.load()) {
            if (std::chrono::steady_clock::now() >= deadline) {
                bbh::say("FAIL watchdog — nothing finished within " + std::to_string(o.timeout_seconds) + "s.");
                std::fflush(stdout);
                ExitProcess(bbh::Exit::Timeout);
            }
            Sleep(50);
        }
    });
    watchdog.detach();

    std::error_code ec;
    bbh::fs::create_directories(o.out_dir, ec);
    bbh::say(L"# out:     " + o.out_dir.wstring());
    bbh::say(L"# sandbox: " + o.sandbox_dir.wstring());
    bbh::say(L"# state:   " + o.state_dir.wstring());

    int code = bbh::Exit::Environment;
    try {
        {
            auto session = bbh::Session::start(o);
            bbh::Runner runner(*session, o);
            code = runner.run();
        }  // the window and the core close here, before the scratch state is swept
    } catch (const std::exception& e) {
        bbh::say(std::string("FAIL harness — ") + e.what());
        code = bbh::Exit::Environment;
    }
    if (!o.keep_state) remove_scratch_state(o);
    finished = true;
    std::fflush(stdout);
    return code;
}
