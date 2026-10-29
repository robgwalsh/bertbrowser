// Bench verbs that wait rather than measure (tools/BertBrowser.Harness/ScriptRunner.Bench.cs): settle-thumbnails.
// `time`, `repeat`, `mem`, `assert-mem` and `bench-fixture` live in bench.cpp.
#include "runner.h"

#include <chrono>

namespace bbh {

// Settle does not wait for pictures on its own in the C# harness, so a timing that ends in a tile view
// would stop the clock before the shell had answered. The native session's settle already gives the
// generator a bounded wait; this verb is the strict form: it keeps pumping until every tile has been
// handed its picture, or fails once the busy timeout is spent, so "tiles on" means "every visible tile
// asked and heard back" and a stalled generator is an error rather than a fast number.
void Runner::settle_thumbnails() {
    s_.settle();
    const auto start = std::chrono::steady_clock::now();
    while (s_.view().thumbnails_pending() > 0) {
        const auto waited = std::chrono::duration_cast<std::chrono::milliseconds>(std::chrono::steady_clock::now() - start).count();
        if (waited > o_.busy_timeout_ms) {
            throw TimeoutError("Thumbnails were still pending after " + std::to_string(o_.busy_timeout_ms) + " ms.");
        }
        s_.pump();
        Sleep(2);
    }
    s_.pump();
    s_.window().render();
}

void Runner::install_bench_settle() {
    verbs_[L"settle-thumbnails"] = [this](const std::wstring&) { settle_thumbnails(); };
}

}  // namespace bbh
