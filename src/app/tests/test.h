// A minimal test harness: TEST(name) registers a case, CHECK(cond) records a failure and carries on.
#pragma once
#include <cmath>
#include <cstdio>
#include <string>
#include <vector>

namespace t {

struct Case {
    const char* name;
    void (*fn)();
};
inline std::vector<Case>& cases() {
    static std::vector<Case> c;
    return c;
}
inline int& failures() {
    static int n = 0;
    return n;
}
inline long& checks() {
    static long n = 0;
    return n;
}
inline const char*& current() {
    static const char* n = "";
    return n;
}
struct Reg {
    Reg(const char* name, void (*fn)()) { cases().push_back({name, fn}); }
};
inline bool check(bool ok, const char* expr, const char* file, int line) {
    ++checks();
    if (!ok) {
        ++failures();
        std::printf("FAIL [%s] %s:%d: %s\n", current(), file, line, expr);
    }
    return ok;
}
inline bool check_near(double a, double b, double eps, const char* expr, const char* file, int line) {
    ++checks();
    const bool ok = std::fabs(a - b) <= eps;
    if (!ok) {
        ++failures();
        std::printf("FAIL [%s] %s:%d: %s (%g vs %g)\n", current(), file, line, expr, a, b);
    }
    return ok;
}
inline bool check_eq(long long a, long long b, const char* expr, const char* file, int line) {
    ++checks();
    const bool ok = a == b;
    if (!ok) {
        ++failures();
        std::printf("FAIL [%s] %s:%d: %s (%lld vs %lld)\n", current(), file, line, expr, a, b);
    }
    return ok;
}
inline std::string narrow(const std::wstring& s) {
    std::string out;
    for (wchar_t c : s) out += c < 0x80 ? static_cast<char>(c) : '?';
    return out;
}
inline bool check_text(const std::wstring& a, const std::wstring& b, const char* expr, const char* file, int line) {
    ++checks();
    const bool ok = a == b;
    if (!ok) {
        ++failures();
        std::printf("FAIL [%s] %s:%d: %s (\"%s\" vs \"%s\")\n", current(), file, line, expr, narrow(a).c_str(),
                    narrow(b).c_str());
    }
    return ok;
}

}  // namespace t

#define TEST(name)                          \
    static void name();                     \
    static t::Reg reg_##name(#name, &name); \
    static void name()
#define CHECK(c) t::check(static_cast<bool>(c), #c, __FILE__, __LINE__)
#define CHECK_EQ(a, b) t::check_eq(static_cast<long long>(a), static_cast<long long>(b), #a " == " #b, __FILE__, __LINE__)
#define CHECK_NEAR(a, b, eps) t::check_near((a), (b), (eps), #a " ~ " #b, __FILE__, __LINE__)
#define CHECK_TEXT(a, b) t::check_text((a), (b), #a " == " #b, __FILE__, __LINE__)
