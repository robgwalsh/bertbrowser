#include "fixture.h"

#include <cstring>

int wmain(int argc, wchar_t** argv) {
    const HWND foreground_before = GetForegroundWindow();
    std::string filter;
    if (argc > 1) {
        filter = t::narrow(argv[1]);
    }
    Env::get();
    int ran = 0;
    for (const t::Case& c : t::cases()) {
        if (!filter.empty() && std::strstr(c.name, filter.c_str()) == nullptr) {
            continue;
        }
        t::current() = c.name;
        const int before = t::failures();
        c.fn();
        std::printf("%s %s\n", t::failures() == before ? "ok  " : "FAIL", c.name);
        ++ran;
    }
    // Nothing may have taken the foreground from whatever the user was doing.
    t::current() = "(process)";
    CHECK(GetForegroundWindow() == foreground_before);
    if (t::failures()) {
        std::printf("bbapp_tests FAILED: %d of %ld checks failed, %d cases\n", t::failures(), t::checks(), ran);
        return 1;
    }
    std::printf("bbapp_tests OK: %ld checks, %d cases\n", t::checks(), ran);
    return 0;
}
