// Spike S7, now over the real bb-ffi v1 surface: a Rust staticlib linked into an MSVC /MT executable
// with the boundary rules proven. Events arrive through a sink on Rust threads and are only enqueued;
// strings cross both ways with clear ownership; a listing is a refcounted snapshot read by views; an
// error, a stale id and a panic each come back as a status with a readable message.
#include <windows.h>

#include <chrono>
#include <condition_variable>
#include <cstdio>
#include <filesystem>
#include <fstream>
#include <mutex>
#include <string>
#include <vector>

#include "bbcore.h"

namespace {

namespace fs = std::filesystem;

struct Queue {
    std::mutex mutex;
    std::condition_variable ready;
    std::vector<BbEvent*> events;
};

// The sink: runs on a Rust thread, enqueues, returns. Nothing else.
extern "C" void sink(void* ctx, BbEvent* evt) {
    auto* queue = static_cast<Queue*>(ctx);
    std::lock_guard<std::mutex> lock(queue->mutex);
    queue->events.push_back(evt);
    queue->ready.notify_one();
}

std::wstring view(BbStr s) {
    return std::wstring(reinterpret_cast<const wchar_t*>(s.ptr), s.len);
}

BbStr str(const std::wstring& s) {
    return BbStr{reinterpret_cast<const uint16_t*>(s.data()), static_cast<uint32_t>(s.size())};
}

std::wstring take(BbOwnedStr s) {
    std::wstring text = s.ptr ? std::wstring(reinterpret_cast<const wchar_t*>(s.ptr), s.len) : L"";
    bb_str_free(s);
    return text;
}

std::wstring last_error() {
    BbStr s{};
    bb_last_error(&s);
    return view(s);
}

int fail(const char* what) {
    std::printf("S7 FAIL: %s\n", what);
    return 1;
}

}  // namespace

int main() {
    std::printf("bbcore %ls\n", view(bb_version()).c_str());

    // A throwaway tree and data directory.
    const fs::path base = fs::temp_directory_path() / (L"bb-s7-" + std::to_wstring(GetCurrentProcessId()));
    const fs::path tree = base / L"tree";
    const fs::path data = base / L"data";
    fs::create_directories(tree / L"Documents");
    for (const wchar_t* name : {L"notes.txt", L"photo.jpg", L"report.md"}) {
        std::ofstream(tree / name, std::ios::binary) << "x";
    }

    // 1. Create the app over the harness seams, install the sink, open a tab.
    const std::wstring data_dir = data.wstring();
    BbAppConfig cfg{};
    cfg.data_dir = str(data_dir);
    cfg.fakes = BB_FAKES_HARNESS;
    cfg.search_debounce_ms = 0;
    BbApp* app = nullptr;
    if (bb_app_create(&cfg, &app) != BB_OK) {
        return fail("bb_app_create refused");
    }
    Queue queue;
    bb_app_set_event_sink(app, sink, &queue);
    const std::wstring tree_path = tree.wstring();
    uint64_t tab = 0;
    if (bb_app_initialize(app, str(tree_path), &tab) != BB_OK || tab == 0) {
        return fail("bb_app_initialize refused");
    }

    // 2. The listing arrives as an event from a Rust thread; the settle predicate goes quiet.
    {
        std::unique_lock<std::mutex> lock(queue.mutex);
        auto listed = [&] {
            for (const BbEvent* e : queue.events) {
                if (e->kind == BB_EVT_LISTING_CHANGED && e->id == tab && e->a > 0) {
                    return true;
                }
            }
            return false;
        };
        if (!queue.ready.wait_for(lock, std::chrono::seconds(10), listed)) {
            return fail("no ListingChanged event arrived");
        }
    }
    for (int i = 0; i < 1000 && bb_app_is_busy(app); ++i) {
        Sleep(5);
    }
    if (bb_app_is_busy(app)) {
        return fail("the app never went quiet");
    }

    BbTabState state{};
    if (bb_tab_state(app, tab, &state) != BB_OK || state.count != 4) {
        return fail("wrong row count");
    }
    const BbListing* listing = bb_tab_listing(app, tab);
    if (!listing || bb_listing_len(listing) != 4) {
        return fail("wrong listing length");
    }
    BbRow row{};
    if (bb_listing_row(listing, 0, &row) != BB_OK || view(row.name) != L"Documents" ||
        !(row.flags & BB_ROW_DIRECTORY) || row.size != -1) {
        return fail("first row should be the unmeasured Documents folder");
    }
    BbStr cell{};
    if (bb_listing_cell(listing, 2, 0, &cell) != BB_OK || view(cell) != L"photo.jpg") {
        return fail("cell text is wrong");
    }
    bb_listing_release(listing);

    // 3. Strings in and out, with the caller freeing what it was given.
    BbOwnedStr json{};
    if (bb_settings_get(app, str(L"showHiddenItems"), &json) != BB_OK || take(json) != L"false") {
        return fail("settings did not round-trip");
    }
    BbOwnedStr path{};
    if (bb_tab_text(app, tab, BB_TEXT_PATH, &path) != BB_OK || take(path) != tree_path) {
        return fail("tab path is wrong");
    }

    // 4. An error, a stale id and a panic each come back as a status with a message, and nothing
    //    unwinds.
    if (bb_tab_navigate(app, tab, str(L"Q:\\no\\such\\folder")) != BB_ERR ||
        last_error().rfind(L"Folder not found", 0) != 0) {
        return fail("BB_ERR did not carry its message");
    }
    if (bb_tab_navigate(app, 424242, str(tree_path)) != BB_STALE) {
        return fail("a stale tab should answer BB_STALE");
    }
    if (bb_selftest_panic() != BB_PANIC || last_error() != L"selftest: deliberate panic") {
        return fail("BB_PANIC did not carry its message");
    }

    // 5. Tear down: every queued event is ours to free.
    bb_app_destroy(app);
    size_t count = 0;
    {
        std::lock_guard<std::mutex> lock(queue.mutex);
        count = queue.events.size();
        for (BbEvent* evt : queue.events) {
            bb_event_free(evt);
        }
    }
    std::error_code ignored;
    fs::remove_all(base, ignored);

    std::printf("S7 OK: %zu events, listing snapshot, strings both ways, error/stale/panic contained\n", count);
    return 0;
}
