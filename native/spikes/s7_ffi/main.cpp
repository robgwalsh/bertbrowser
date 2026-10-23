// Spike S7: a Rust staticlib linked into an MSVC /MT executable with the three boundary rules proven:
// events arrive through a sink on a Rust thread and are only enqueued; strings cross both ways with
// clear ownership; an error and a panic each come back as a status with a readable message.
#include <windows.h>

#include <condition_variable>
#include <cstdio>
#include <mutex>
#include <string>
#include <vector>

#include "bbcore.h"

namespace {

struct Queue {
    std::mutex mutex;
    std::condition_variable ready;
    std::vector<BbEvent*> events;
    bool done = false;
};

// The sink: runs on the Rust thread, enqueues, returns. Nothing else.
extern "C" void sink(void* ctx, BbEvent* evt) {
    auto* queue = static_cast<Queue*>(ctx);
    std::lock_guard<std::mutex> lock(queue->mutex);
    queue->events.push_back(evt);
    if (evt->kind == 2) {
        queue->done = true;
    }
    queue->ready.notify_one();
}

std::wstring view(BbStr s) {
    return std::wstring(reinterpret_cast<const wchar_t*>(s.ptr), s.len);
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

    // 1. Events from a Rust thread, enqueued by the sink, consumed here, freed here.
    Queue queue;
    const uint32_t count = 1000;
    if (bb_spike_emit(sink, &queue, count) != BB_OK) {
        return fail("bb_spike_emit refused");
    }
    {
        std::unique_lock<std::mutex> lock(queue.mutex);
        if (!queue.ready.wait_for(lock, std::chrono::seconds(10), [&] { return queue.done; })) {
            return fail("timed out waiting for the done event");
        }
    }
    if (queue.events.size() != count + 1) {
        return fail("wrong number of events");
    }
    for (uint32_t i = 0; i < count; ++i) {
        BbEvent* evt = queue.events[i];
        if (evt->kind != 1 || evt->id != i || view(evt->text) != L"event " + std::to_wstring(i)) {
            return fail("event out of order or mislabelled");
        }
    }
    if (view(queue.events[count]->text) != L"done") {
        return fail("final event is not done");
    }
    for (BbEvent* evt : queue.events) {
        bb_event_free(evt);
    }

    // 2. Strings in and out, with the caller freeing what it was given.
    const std::wstring input = L"bert browser é";
    BbOwnedStr upper{};
    if (bb_spike_upper(BbStr{reinterpret_cast<const uint16_t*>(input.data()),
                             static_cast<uint32_t>(input.size())},
                       &upper) != BB_OK) {
        return fail("bb_spike_upper refused");
    }
    const std::wstring got(reinterpret_cast<const wchar_t*>(upper.ptr), upper.len);
    bb_str_free(upper);
    if (got != L"BERT BROWSER É") {
        return fail("upper-casing did not round-trip");
    }

    // 3. An error and a panic each come back as a status with a message, and nothing unwinds.
    if (bb_spike_fail() != BB_ERR || last_error() != L"spike: deliberate failure") {
        return fail("BB_ERR did not carry its message");
    }
    if (bb_spike_panic() != BB_PANIC || last_error() != L"spike: deliberate panic") {
        return fail("BB_PANIC did not carry its message");
    }

    std::printf("S7 OK: %u events, strings both ways, error and panic contained\n", count);
    return 0;
}
