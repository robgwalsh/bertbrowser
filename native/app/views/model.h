// The core's models come back as JSON text; this reads one into a json::Value. Every call is made from
// the UI thread, like the rest of the FFI.
#pragma once
#include "views/core.h"
#include "views/json.h"

namespace bb::views {

// Runs a call that fills a BbOwnedStr and parses what it wrote. Throws CoreError when the call fails.
template <class F>
json::Value model(F&& call, const char* what = "model") {
    BbOwnedStr out{};
    core::check(call(&out), what);
    const std::string text = core::utf8(core::take(out));
    return text.empty() ? json::Value{} : json::parse(text);
}

// As model(), but a failed call is a null value rather than an exception (a view repainting from events).
template <class F>
json::Value try_model(F&& call) {
    try {
        return model(std::forward<F>(call));
    } catch (const std::exception&) {
        return {};
    }
}

inline std::wstring wstr(const json::Value& v) { return core::wide(v.str()); }

}  // namespace bb::views
