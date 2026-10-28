// The measuring verbs (tools/BertBrowser.Harness/ScriptRunner.Bench.cs): time, repeat, mem, assert-mem,
// bench-fixture. The JSON lines BertBrowser.Bench reads keep the C# names and shapes.
#include "runner.h"

#include <windows.h>
#include <psapi.h>

#include <charconv>
#include <chrono>
#include <fstream>

namespace bbh {

namespace {
using Clock = std::chrono::steady_clock;

std::string json_string(const std::string& s) {
    std::string out = "\"";
    for (const char c : s) {
        if (c == '"' || c == '\\') out += '\\';
        out += c;
    }
    return out + "\"";
}

std::string shortest(double v) {
    char buf[64];
    const auto r = std::to_chars(buf, buf + sizeof(buf), v);
    return std::string(buf, r.ptr);
}

std::wstring trim_ws(const std::wstring& s) {
    const size_t b = s.find_first_not_of(L" \t");
    if (b == std::wstring::npos) return {};
    return s.substr(b, s.find_last_not_of(L" \t") - b + 1);
}

std::vector<std::wstring> steps_of(const std::wstring& text) {
    std::vector<std::wstring> out;
    std::wstring part;
    for (wchar_t c : text + L";") {
        if (c == L';') {
            const std::wstring t = trim_ws(part);
            if (!t.empty()) out.push_back(t);
            part.clear();
        } else {
            part += c;
        }
    }
    return out;
}
}  // namespace

void Runner::append_line(const std::string& json) {
    if (o_.bench_out.empty()) return;
    const fs::path p(o_.bench_out);
    if (p.has_parent_path()) fs::create_directories(p.parent_path());
    std::ofstream out(p, std::ios::binary | std::ios::app);
    out << json << "\n";
}

void Runner::record_time(const std::wstring& name, double ms) {
    char one[64];
    snprintf(one, sizeof(one), "%.1f", ms);
    say("BENCH " + to_utf8(name) + " " + one);
    append_line("{\"kind\":\"time\",\"name\":" + json_string(to_utf8(name)) + ",\"ms\":" + shortest(ms) + "}");
}

void Runner::time_verb(const std::wstring& rest) {
    const auto [name, inner] = split_first(require(rest, "time"));
    if (inner.empty()) throw FormatError("time wants '<name> <verb> [args]'.");
    const std::vector<std::wstring> steps = steps_of(inner);
    if (steps.empty()) throw FormatError("time has nothing to run.");
    const auto start = Clock::now();
    for (const std::wstring& step : steps) execute(step);
    const double ms = std::chrono::duration<double, std::milli>(Clock::now() - start).count();
    record_time(name, ms);
}

Runner::MemorySample Runner::read_memory() {
    // What is held, not what is about to be: let the core's own queued events land first.
    s_.pump();
    // The managed harness collects fully before it reads; the native analogue is dropping what is only
    // cached. The layout LRU is bounded (4096 layouts, tens of MB of DirectWrite objects) and fills to its
    // cap whatever the folder, so left in it would read as growth in a soak that is not one.
    s_.resources().layouts.clear();
    MemorySample m;
    HEAP_SUMMARY hs{};
    hs.cb = sizeof(hs);
    if (HeapSummary(GetProcessHeap(), 0, &hs)) m.managed = hs.cbAllocated;  // the native heap in use, our analogue of the managed heap
    PROCESS_MEMORY_COUNTERS_EX pmc{};
    pmc.cb = sizeof(pmc);
    if (GetProcessMemoryInfo(GetCurrentProcess(), reinterpret_cast<PROCESS_MEMORY_COUNTERS*>(&pmc), sizeof(pmc))) {
        m.working_set = pmc.WorkingSetSize;
        m.private_bytes = pmc.PrivateUsage;
        m.peak_working_set = pmc.PeakWorkingSetSize;
    }
    if (auto l = s_.core().listing(tab())) m.items = static_cast<int64_t>(l->size());
    m.realized = -1;
    if (bb::views::TabView* tv = s_.view().active_view()) {
        const auto [first, last] = tv->list()->visible_range();
        m.realized = static_cast<int64_t>(last - first);
    }
    m.retained_thumbnails = 0;  // no thumbnails are built yet
    return m;
}

void Runner::mem_verb(const std::wstring& rest) {
    const std::wstring name = rest.empty() ? L"mem" : rest;
    const MemorySample m = read_memory();
    memory_[name] = m;
    const std::string body = "{\"kind\":\"mem\",\"name\":" + json_string(to_utf8(name)) +
                             ",\"managedBytes\":" + std::to_string(m.managed) +
                             ",\"workingSetBytes\":" + std::to_string(m.working_set) +
                             ",\"privateBytes\":" + std::to_string(m.private_bytes) +
                             ",\"peakWorkingSetBytes\":" + std::to_string(m.peak_working_set) +
                             ",\"items\":" + std::to_string(m.items) + ",\"realized\":" + std::to_string(m.realized) +
                             ",\"retainedThumbnails\":" + std::to_string(m.retained_thumbnails) + "}";
    say("MEM " + body);
    append_line(body);
}

void Runner::assert_mem(const std::wstring& rest) {
    const char* usage = "assert-mem wants '<managed|private|workingset> under <mb> [since <name>]'.";
    std::vector<std::wstring> parts;
    for (const auto& p : bb::core::split(require(rest, "assert-mem"), L' ')) {
        if (!p.empty()) parts.push_back(p);
    }
    if (!(parts.size() == 3 || parts.size() == 5) || !bb::core::iequals(parts[1], L"under")) throw FormatError(usage);
    if (parts.size() == 5 && !bb::core::iequals(parts[3], L"since")) throw FormatError(usage);

    const int limit_mb = number(parts[2], "assert-mem");
    const MemorySample now = read_memory();
    std::wstring kind = parts[0];
    for (auto& c : kind) c = static_cast<wchar_t>(towlower(c));
    uint64_t MemorySample::*field;
    const char* what;
    if (kind == L"managed") { field = &MemorySample::managed; what = "native heap"; }
    else if (kind == L"private") { field = &MemorySample::private_bytes; what = "private bytes"; }
    else if (kind == L"workingset") { field = &MemorySample::working_set; what = "working set"; }
    else throw FormatError("assert-mem: expected 'managed', 'private' or 'workingset', got '" + to_utf8(parts[0]) + "'.");

    constexpr double kMb = 1024.0 * 1024.0;
    char buf[160];
    if (parts.size() == 3) {
        const double mb = static_cast<double>(now.*field) / kMb;
        if (mb >= limit_mb) {
            snprintf(buf, sizeof(buf), "expected the %s under %d MB, got %.1f MB.", what, limit_mb, mb);
            throw AssertionError(buf);
        }
        return;
    }
    const auto it = memory_.find(parts[4]);
    if (it == memory_.end()) throw FormatError("assert-mem: no 'mem " + to_utf8(parts[4]) + "' has run yet.");
    const double grown = (static_cast<double>(now.*field) - static_cast<double>(it->second.*field)) / kMb;
    if (grown >= limit_mb) {
        snprintf(buf, sizeof(buf), "expected the %s to have grown by under %d MB since '", what, limit_mb);
        char tail[96];
        snprintf(tail, sizeof(tail), "', it grew %.1f MB (to %.1f MB).", grown, static_cast<double>(now.*field) / kMb);
        throw AssertionError(std::string(buf) + to_utf8(parts[4]) + tail);
    }
}

void Runner::bench_fixture(const std::wstring& rest) {
    std::vector<std::wstring> parts;
    for (const auto& p : bb::core::split(require(rest, "bench-fixture"), L' ')) {
        if (!p.empty()) parts.push_back(p);
    }
    if (parts.size() < 2 || parts.size() > 3) throw FormatError("bench-fixture wants '<files> <dirs> [dir]'.");
    const int files = number(parts[0], "bench-fixture");
    const int dirs = number(parts[1], "bench-fixture");
    const fs::path root = sandbox_.require_inside(parts.size() == 3 ? parts[2] : L"Bench", "bench-fixture");
    fs::create_directories(root);

    static const wchar_t* cycle[] = {L".jpg", L".png", L".mp4", L".txt", L".cs", L".json", L".xml", L".txt"};
    for (int d = 0; d < dirs; ++d) {
        wchar_t name[16];
        swprintf_s(name, L"D%03d", d);
        fs::create_directories(root / name);
    }
    for (int i = 0; i < files; ++i) {
        wchar_t file[32];
        swprintf_s(file, L"f%05d%ls", i, cycle[i % 8]);
        fs::path folder = root;
        if (dirs != 0) {
            wchar_t dn[16];
            swprintf_s(dn, L"D%03d", i % dirs);
            folder = root / dn;
        }
        HANDLE h = CreateFileW((folder / file).c_str(), GENERIC_WRITE, 0, nullptr, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
        if (h == INVALID_HANDLE_VALUE) throw std::runtime_error("cannot create " + to_utf8((folder / file).wstring()));
        CloseHandle(h);
    }
    Sandbox::stamp(root);
    say("# bench fixture: " + std::to_string(files) + " files in " + std::to_string(std::max(dirs, 1)) + " folder(s) under " +
        to_utf8(root.wstring()));
}

void Runner::install_bench() {
    verbs_[L"time"] = [this](const std::wstring& r) { time_verb(r); };
    verbs_[L"repeat"] = [this](const std::wstring& r) {
        const auto [count, inner] = split_first(require(r, "repeat"));
        const int laps = number(count, "repeat");
        const std::vector<std::wstring> steps = steps_of(inner);
        if (laps < 1 || steps.empty()) throw FormatError("repeat wants '<n> <verb> [args][; <verb> [args]…]'.");
        for (int lap = 0; lap < laps; ++lap) {
            for (const auto& step : steps) execute(step);
        }
    };
    verbs_[L"mem"] = [this](const std::wstring& r) { mem_verb(r); };
    verbs_[L"assert-mem"] = [this](const std::wstring& r) { assert_mem(r); };
    verbs_[L"bench-fixture"] = [this](const std::wstring& r) { bench_fixture(r); };
    install_bench_settle();
}

}  // namespace bbh
