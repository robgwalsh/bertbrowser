// A typed, exception-throwing face on bbcore.h for the views and the harness. Every call is made from
// the UI thread (the rule of the FFI); results arrive as events.
#pragma once
#include "bbcore.h"

#include <cstdint>
#include <memory>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

namespace bb::core {

// ---- strings: UTF-16 across the boundary, UTF-8 in JSON and on the console ----
inline std::wstring w(BbStr s) {
    return s.ptr ? std::wstring(reinterpret_cast<const wchar_t*>(s.ptr), s.len) : std::wstring();
}
inline BbStr s(const std::wstring& v) {
    return BbStr{reinterpret_cast<const uint16_t*>(v.data()), static_cast<uint32_t>(v.size())};
}
// Takes ownership of an owned string from Rust (frees it). Empty for none.
std::wstring take(BbOwnedStr o);
std::wstring last_error();
std::string utf8(const std::wstring& v);
std::wstring wide(const std::string& v);
std::vector<std::wstring> split(const std::wstring& text, wchar_t sep);
bool iequals(const std::wstring& a, const std::wstring& b);
bool icontains(const std::wstring& haystack, const std::wstring& needle);

// A call that came back with something other than BB_OK.
class CoreError : public std::runtime_error {
public:
    CoreError(BbStatus status, const std::string& what) : std::runtime_error(what), status(status) {}
    BbStatus status;
};
void check(BbStatus st, const char* what);

// A refcounted, immutable snapshot of a tab's rows. Views it hands out are valid while this lives.
class Listing {
public:
    explicit Listing(const BbListing* adopted) : l_(adopted) {}  // takes the caller's reference
    ~Listing();
    Listing(const Listing&) = delete;
    Listing& operator=(const Listing&) = delete;

    size_t size() const;
    uint64_t version() const;
    bool row(size_t i, BbRow& out) const;
    std::wstring name(size_t i) const;
    std::wstring cell(size_t i, uint32_t column) const;

private:
    const BbListing* l_;
};

struct ColumnInfo {
    std::wstring id, header;
    float width = 120;
    int cell = -1;
    bool right = false, sortable = true;
};

struct Crumb {
    std::wstring name, path;
};

class Core {
public:
    explicit Core(BbApp* app) : app_(app) {}
    BbApp* app() const { return app_; }

    BbTabState tab_state(uint64_t tab);
    // Empty for none.
    std::wstring tab_text(uint64_t tab, BbTabText which);
    std::shared_ptr<Listing> listing(uint64_t tab);
    std::vector<ColumnInfo> columns(uint64_t tab);
    std::vector<Crumb> breadcrumb(uint64_t tab);
    std::vector<uint32_t> selection(uint64_t tab);
    std::vector<std::wstring> selection_paths(uint64_t tab);
    std::string state_json();
    std::vector<uint32_t> palette();
    bool busy() const { return bb_app_is_busy(app_); }

    void navigate(uint64_t tab, const std::wstring& path);
    void select(uint64_t tab, BbSelectOp op, const std::vector<uint32_t>& idx);
    // Which command owns this chord, or empty.
    std::wstring dispatch(uint32_t modifiers, const std::wstring& key, BbFocusKind focus, uint32_t flags);

private:
    BbApp* app_;
};

}  // namespace bb::core
