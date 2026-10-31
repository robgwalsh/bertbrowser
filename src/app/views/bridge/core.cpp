#include "views/bridge/core.h"

#include <windows.h>

#include <algorithm>
#include <cwctype>

namespace bb::core {

std::wstring take(BbOwnedStr o) {
    std::wstring text = o.ptr ? std::wstring(reinterpret_cast<const wchar_t*>(o.ptr), o.len) : std::wstring();
    bb_str_free(o);
    return text;
}

std::wstring last_error() {
    BbStr e{};
    bb_last_error(&e);
    return w(e);
}

std::string utf8(const std::wstring& v) {
    if (v.empty()) return {};
    const int n = WideCharToMultiByte(CP_UTF8, 0, v.data(), static_cast<int>(v.size()), nullptr, 0, nullptr, nullptr);
    std::string out(static_cast<size_t>(n), '\0');
    WideCharToMultiByte(CP_UTF8, 0, v.data(), static_cast<int>(v.size()), out.data(), n, nullptr, nullptr);
    return out;
}

std::wstring wide(const std::string& v) {
    if (v.empty()) return {};
    const int n = MultiByteToWideChar(CP_UTF8, 0, v.data(), static_cast<int>(v.size()), nullptr, 0);
    std::wstring out(static_cast<size_t>(n), L'\0');
    MultiByteToWideChar(CP_UTF8, 0, v.data(), static_cast<int>(v.size()), out.data(), n);
    return out;
}

std::vector<std::wstring> split(const std::wstring& text, wchar_t sep) {
    std::vector<std::wstring> out;
    size_t start = 0;
    while (start <= text.size()) {
        const size_t at = text.find(sep, start);
        if (at == std::wstring::npos) {
            out.push_back(text.substr(start));
            break;
        }
        out.push_back(text.substr(start, at - start));
        start = at + 1;
    }
    return out;
}

bool iequals(const std::wstring& a, const std::wstring& b) {
    return a.size() == b.size() &&
           std::equal(a.begin(), a.end(), b.begin(), [](wchar_t x, wchar_t y) { return std::towlower(x) == std::towlower(y); });
}

bool icontains(const std::wstring& haystack, const std::wstring& needle) {
    if (needle.empty()) return true;
    return std::search(haystack.begin(), haystack.end(), needle.begin(), needle.end(),
                       [](wchar_t x, wchar_t y) { return std::towlower(x) == std::towlower(y); }) != haystack.end();
}

void check(BbStatus st, const char* what) {
    if (st == BB_OK) return;
    const std::string said = utf8(last_error());
    throw CoreError(st, std::string(what) + ": " + said, said);
}

std::wstring message_of(const std::exception& e) {
    if (const auto* c = dynamic_cast<const CoreError*>(&e); c && !c->detail.empty()) return wide(c->detail);
    return wide(e.what());
}

Listing::~Listing() {
    if (l_) bb_listing_release(l_);
}
size_t Listing::size() const { return l_ ? bb_listing_len(l_) : 0; }
uint64_t Listing::version() const { return l_ ? bb_listing_version(l_) : 0; }
bool Listing::row(size_t i, BbRow& out) const {
    return l_ && bb_listing_row(l_, static_cast<uint32_t>(i), &out) == BB_OK;
}
std::wstring Listing::name(size_t i) const {
    BbRow r{};
    return row(i, r) ? w(r.name) : std::wstring();
}
std::wstring Listing::cell(size_t i, uint32_t column) const {
    BbStr t{};
    if (!l_ || bb_listing_cell(l_, static_cast<uint32_t>(i), column, &t) != BB_OK) return {};
    return w(t);
}

std::wstring Listing::size_tip(size_t i) const {
    BbOwnedStr o{};
    if (bb_listing_size_tip(l_, static_cast<uint32_t>(i), &o) != BB_OK) return {};
    return take(o);
}

int Listing::match(size_t i, uint32_t* start, uint32_t* length) const {
    int32_t line = 0;
    if (bb_listing_match(l_, static_cast<uint32_t>(i), &line, start, length) != BB_OK) return 0;
    return line;
}

BbTabState Core::tab_state(uint64_t tab) {
    BbTabState st{};
    check(bb_tab_state(app_, tab, &st), "bb_tab_state");
    return st;
}

std::wstring Core::tab_text(uint64_t tab, BbTabText which) {
    BbOwnedStr o{};
    if (bb_tab_text(app_, tab, which, &o) != BB_OK) return {};
    return take(o);
}

std::shared_ptr<Listing> Core::listing(uint64_t tab) {
    const BbListing* l = bb_tab_listing(app_, tab);
    return l ? std::make_shared<Listing>(l) : nullptr;
}

std::vector<ColumnInfo> Core::columns(uint64_t tab) {
    BbOwnedStr o{};
    std::vector<ColumnInfo> out;
    if (bb_tab_columns(app_, tab, &o) != BB_OK) return out;
    for (const std::wstring& line : split(take(o), L'\n')) {
        const auto f = split(line, L'\t');
        if (f.size() < 6) continue;
        ColumnInfo c;
        c.id = f[0];
        c.header = f[1];
        c.width = static_cast<float>(_wtof(f[2].c_str()));
        c.cell = _wtoi(f[3].c_str());
        c.right = f[4] == L"1";
        c.sortable = f[5] == L"1";
        out.push_back(std::move(c));
    }
    return out;
}

std::vector<Crumb> Core::breadcrumb(uint64_t tab) {
    BbOwnedStr o{};
    std::vector<Crumb> out;
    if (bb_tab_breadcrumb(app_, tab, &o) != BB_OK) return out;
    for (const std::wstring& line : split(take(o), L'\n')) {
        const auto f = split(line, L'\t');
        if (f.size() >= 2) out.push_back({f[0], f[1]});
    }
    return out;
}

std::vector<uint32_t> Core::selection(uint64_t tab) {
    uint32_t total = 0;
    if (bb_tab_selection(app_, tab, nullptr, 0, &total) != BB_OK || total == 0) return {};
    std::vector<uint32_t> idx(total);
    uint32_t got = 0;
    if (bb_tab_selection(app_, tab, idx.data(), total, &got) != BB_OK) return {};
    idx.resize(std::min(got, total));
    return idx;
}

std::vector<std::wstring> Core::selection_paths(uint64_t tab) {
    BbOwnedStr o{};
    if (bb_tab_selection_paths(app_, tab, &o) != BB_OK) return {};
    const std::wstring text = take(o);
    std::vector<std::wstring> out;
    for (std::wstring& line : split(text, L'\n')) {
        while (!line.empty() && line.back() == L'\r') line.pop_back();
        if (!line.empty()) out.push_back(std::move(line));
    }
    return out;
}

std::string Core::state_json() {
    BbOwnedStr o{};
    check(bb_app_state_json(app_, &o), "bb_app_state_json");
    return utf8(take(o));
}

std::vector<uint32_t> Core::palette() {
    uint32_t total = 0;
    check(bb_theme_palette(app_, nullptr, 0, &total), "bb_theme_palette");
    std::vector<uint32_t> argb(total);
    uint32_t got = 0;
    check(bb_theme_palette(app_, argb.data(), total, &got), "bb_theme_palette");
    argb.resize(std::min(got, total));
    return argb;
}

void Core::navigate(uint64_t tab, const std::wstring& path) { check(bb_tab_navigate(app_, tab, s(path)), "navigate"); }

void Core::select(uint64_t tab, BbSelectOp op, const std::vector<uint32_t>& idx) {
    check(bb_tab_select(app_, tab, op, idx.empty() ? nullptr : idx.data(), static_cast<uint32_t>(idx.size())), "select");
}

std::wstring Core::dispatch(uint32_t modifiers, const std::wstring& key, BbFocusKind focus, uint32_t flags) {
    BbOwnedStr o{};
    const BbStatus st = bb_key_dispatch(app_, BbKeyChord{modifiers, s(key)}, BbFocusState{focus, flags}, &o);
    if (st != BB_OK) return {};
    return take(o);
}

}  // namespace bb::core
