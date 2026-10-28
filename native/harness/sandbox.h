// The throwaway tree a run browses, and the fence around everything that writes
// (tools/BertBrowser.Harness/Sandbox.cs).
#pragma once
#include "options.h"

#include <stdexcept>

namespace bbh {

// A command asserted something that is not so, or asked for something that cannot be done.
class AssertionError : public std::runtime_error {
public:
    using std::runtime_error::runtime_error;
};
// A command that is badly written (the C# FormatException).
class FormatError : public std::runtime_error {
public:
    using std::runtime_error::runtime_error;
};
class TimeoutError : public std::runtime_error {
public:
    using std::runtime_error::runtime_error;
};
// A verb the C# harness has that this one does not yet. Always a failure, never a quiet pass.
class NotImplemented : public std::runtime_error {
public:
    using std::runtime_error::runtime_error;
};

class Sandbox {
public:
    explicit Sandbox(const Options& o) : root_(o.sandbox_dir), allow_outside_(o.allow_outside) {}

    const fs::path& root() const { return root_; }
    // Absolute as given, otherwise relative to the sandbox.
    fs::path resolve(const std::wstring& path) const;
    // Refuses a path the run has no business writing to.
    fs::path require_inside(const std::wstring& path, const std::string& verb) const;
    // The standard tree; deterministic names, sizes and times. Returns its root.
    fs::path populate(const std::wstring& relative) const;

    static void write(const fs::path& path, size_t bytes);
    static void stamp(const fs::path& root);

private:
    fs::path root_;
    bool allow_outside_;
};

}  // namespace bbh
