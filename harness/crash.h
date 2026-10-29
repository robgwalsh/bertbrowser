// Crash diagnostics for the harness: an unhandled-exception filter that writes a minidump into the
// run's --out directory and prints `CRASH <code> at <module>+<offset>` plus a symbolised stack.
#pragma once

#include <filesystem>

namespace bbh {

// Installs the filter. `dump_dir` is where crash.dmp goes; call before any other thread starts.
void install_crash_handler(const std::filesystem::path& dump_dir);

}  // namespace bbh
