// How a run was asked for, and the console it reports on. Same flags, same exit codes as the C# harness
// (tools/BertBrowser.Harness/HarnessOptions.cs).
#pragma once
#include <cstdio>
#include <filesystem>
#include <optional>
#include <string>
#include <vector>

namespace bbh {

namespace fs = std::filesystem;

struct Exit {
    static constexpr int Ok = 0, Failed = 1, Environment = 2, Timeout = 3, Usage = 64;
};

struct Options {
    std::vector<std::wstring> commands;
    fs::path out_dir, sandbox_dir, state_dir;
    bool keep_state = false;
    bool allow_outside = false;
    int timeout_seconds = 120;
    int busy_timeout_ms = 30000;
    bool keep_going = false;
    int width = 1400, height = 900;
    std::wstring theme_id;
    std::wstring start_path;
    bool index = false;
    bool index_declined = false;
    bool verbose = false;
    std::wstring bench_out;
};

// UTF-8 lines on stdout, flushed per line: a watchdog that exits mid-run must not lose what came before.
void say(const std::wstring& line);
void say(const std::string& line);
std::string to_utf8(const std::wstring& w);
std::wstring from_utf8(const std::string& s);

// Parses the command line; on --help or an error prints and returns nullopt with `exit_code` set.
std::optional<Options> parse_options(int argc, wchar_t** argv, int& exit_code);

}  // namespace bbh
