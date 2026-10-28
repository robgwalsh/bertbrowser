#include "options.h"

#include <windows.h>

#include <chrono>
#include <ctime>
#include <fstream>
#include <iostream>
#include <iterator>
#include <sstream>

namespace bbh {

std::string to_utf8(const std::wstring& w) {
    if (w.empty()) return {};
    const int n = WideCharToMultiByte(CP_UTF8, 0, w.data(), static_cast<int>(w.size()), nullptr, 0, nullptr, nullptr);
    std::string out(static_cast<size_t>(n), '\0');
    WideCharToMultiByte(CP_UTF8, 0, w.data(), static_cast<int>(w.size()), out.data(), n, nullptr, nullptr);
    return out;
}

std::wstring from_utf8(const std::string& s) {
    if (s.empty()) return {};
    const int n = MultiByteToWideChar(CP_UTF8, 0, s.data(), static_cast<int>(s.size()), nullptr, 0);
    std::wstring out(static_cast<size_t>(n), L'\0');
    MultiByteToWideChar(CP_UTF8, 0, s.data(), static_cast<int>(s.size()), out.data(), n);
    return out;
}

void say(const std::string& line) {
    std::fwrite(line.data(), 1, line.size(), stdout);
    std::fputc('\n', stdout);
    std::fflush(stdout);
}

void say(const std::wstring& line) { say(to_utf8(line)); }

namespace {

const char* kUsage = R"(BertBrowser UI harness (native) - drives the real window offscreen, where it cannot take focus
from you or appear over what you are doing.

  --script <path>     run the commands in a file
  -c "<commands>"     run commands given inline, separated by ';'
                      (with neither, commands are read from stdin)

  --out <dir>         where captures go        (default: %TEMP%\bertbrowser-harness\<time>)
  --sandbox <dir>     the tree a run may modify (default: <out>\sandbox)
  --state-dir <dir>   the app's data root       (default: a scratch dir, deleted after)
  --keep-state        keep the state directory, to test restore across two runs
  --allow-outside     let destructive commands act outside the sandbox
  --size <WxH>        window size for captures  (default: 1400x900)
  --theme <id>        start in a theme          (e.g. light-plus, nord)
  --start <path>      where the first tab opens (default: the sandbox)
  --index             build the MFT index in-process (not built in the native harness yet)
  --index-declined    simulate a declined elevation prompt (not built in the native harness yet)
  --timeout <sec>     watchdog, in seconds      (default: 120)
  --busy-timeout <ms> longest allowed listing   (default: 30000)
  --keep-going        do not stop at the first failure
  --bench-out <file>  append `time`/`mem` samples as JSON lines (BertBrowser.Bench ui reads it)
  --verbose
  --help

Every command echoes 'OK <command>'. Captures print 'SHOT <path>' on their own line.)";

std::vector<std::wstring> split_lines(const std::string& text) {
    std::vector<std::wstring> out;
    std::istringstream in(text);
    std::string line;
    while (std::getline(in, line)) {
        if (!line.empty() && line.back() == '\r') line.pop_back();
        out.push_back(from_utf8(line));
    }
    return out;
}

}  // namespace

std::optional<Options> parse_options(int argc, wchar_t** argv, int& exit_code) {
    exit_code = Exit::Ok;
    Options o;
    std::wstring script, inline_cmds;
    bool have_script = false, have_inline = false;
    std::wstring out_dir, sandbox, state;
    try {
        for (int i = 1; i < argc; ++i) {
            const std::wstring a = argv[i];
            auto next = [&](const char* flag) -> std::wstring {
                if (i + 1 >= argc) throw std::invalid_argument(std::string(flag) + " needs a value.");
                return argv[++i];
            };
            if (a == L"--script") { script = next("--script"); have_script = true; }
            else if (a == L"-c" || a == L"--command") { inline_cmds = next("-c"); have_inline = true; }
            else if (a == L"--out") out_dir = next("--out");
            else if (a == L"--sandbox") sandbox = next("--sandbox");
            else if (a == L"--state-dir") state = next("--state-dir");
            else if (a == L"--keep-state") o.keep_state = true;
            else if (a == L"--allow-outside") o.allow_outside = true;
            else if (a == L"--size") {
                const std::wstring v = next("--size");
                const size_t x = v.find_first_of(L"xX");
                if (x == std::wstring::npos) throw std::invalid_argument("--size wants WxH, got '" + to_utf8(v) + "'.");
                o.width = std::stoi(v.substr(0, x));
                o.height = std::stoi(v.substr(x + 1));
            }
            else if (a == L"--theme") o.theme_id = next("--theme");
            else if (a == L"--start") o.start_path = fs::absolute(next("--start")).wstring();
            else if (a == L"--index") o.index = true;
            else if (a == L"--index-declined") o.index_declined = true;
            else if (a == L"--timeout") o.timeout_seconds = std::stoi(next("--timeout"));
            else if (a == L"--busy-timeout") o.busy_timeout_ms = std::stoi(next("--busy-timeout"));
            else if (a == L"--keep-going") o.keep_going = true;
            else if (a == L"--verbose") o.verbose = true;
            else if (a == L"--bench-out") o.bench_out = fs::absolute(next("--bench-out")).wstring();
            else if (a == L"--help" || a == L"-h" || a == L"/?") {
                say(std::string(kUsage));
                return std::nullopt;
            } else {
                say("Unrecognised argument '" + to_utf8(a) + "'.");
                say("");
                say(std::string(kUsage));
                exit_code = Exit::Usage;
                return std::nullopt;
            }
        }
    } catch (const std::exception& e) {
        say(std::string(e.what()));
        exit_code = Exit::Usage;
        return std::nullopt;
    }

    if (have_script) {
        std::ifstream in{fs::path(script), std::ios::binary};
        if (!in) {
            say("No script at '" + to_utf8(script) + "'.");
            exit_code = Exit::Environment;
            return std::nullopt;
        }
        const std::string text((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
        o.commands = split_lines(text);
    } else if (have_inline) {
        std::wstring part;
        for (wchar_t c : inline_cmds + L";") {
            if (c == L';') {
                const size_t b = part.find_first_not_of(L" \t");
                const size_t e = part.find_last_not_of(L" \t");
                if (b != std::wstring::npos) o.commands.push_back(part.substr(b, e - b + 1));
                part.clear();
            } else {
                part += c;
            }
        }
    } else {
        const std::string text((std::istreambuf_iterator<char>(std::cin)), std::istreambuf_iterator<char>());
        o.commands = split_lines(text);
    }

    wchar_t temp[MAX_PATH];
    GetTempPathW(MAX_PATH, temp);
    char stamp[32];
    const std::time_t now = std::time(nullptr);
    std::tm local{};
    localtime_s(&local, &now);
    std::strftime(stamp, sizeof(stamp), "%Y%m%d-%H%M%S", &local);
    const fs::path root = fs::absolute(out_dir.empty() ? fs::path(temp) / L"bertbrowser-harness" / from_utf8(stamp)
                                                       : fs::path(out_dir));
    o.out_dir = root.lexically_normal();
    o.sandbox_dir = fs::absolute(sandbox.empty() ? o.out_dir / L"sandbox" : fs::path(sandbox)).lexically_normal();
    o.state_dir = fs::absolute(state.empty() ? o.out_dir / L"state" : fs::path(state)).lexically_normal();
    o.keep_state = o.keep_state || !state.empty();
    return o;
}

}  // namespace bbh
