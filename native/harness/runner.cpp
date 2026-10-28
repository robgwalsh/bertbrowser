#include "runner.h"

#include <windows.h>
#include <psapi.h>

#include <algorithm>
#include <charconv>
#include <cmath>
#include <chrono>
#include <cwctype>
#include <fstream>
#include <set>

#include "views/folder_tree.h"
#include "views/json.h"

namespace bbh {

namespace core = bb::core;

namespace {

using bb::core::iequals;
using bb::core::icontains;
using Clock = std::chrono::steady_clock;

const std::set<std::wstring>& all_verbs() {
    static const std::set<std::wstring> verbs = {
#include "all_verbs.inc"
    };
    return verbs;
}

std::string narrow(const std::wstring& w) { return to_utf8(w); }

std::wstring trim(const std::wstring& s) {
    const size_t b = s.find_first_not_of(L" \t\r\n");
    if (b == std::wstring::npos) return {};
    return s.substr(b, s.find_last_not_of(L" \t\r\n") - b + 1);
}

std::string quote(const std::string& text) {
    std::string out = "\"";
    for (const char c : text) {
        switch (c) {
            case '\\': out += "\\\\"; break;
            case '"': out += "\\\""; break;
            case '\n': out += "\\n"; break;
            case '\r': out += "\\r"; break;
            case '\t': out += "\\t"; break;
            default: out += c; break;
        }
    }
    return out + "\"";
}
std::string quote(const std::wstring& text) { return quote(narrow(text)); }
std::string boolean(bool v) { return v ? "true" : "false"; }

std::string shortest(double v) {
    char buf[64];
    const auto r = std::to_chars(buf, buf + sizeof(buf), v);
    return std::string(buf, r.ptr);
}

std::string fixed1(double v) {
    char buf[64];
    snprintf(buf, sizeof(buf), "%.1f", v);
    return buf;
}

}  // namespace

Runner::Runner(Session& session, const Options& options) : s_(session), o_(options), sandbox_(options) {
    install();
    install_asserts();
    install_bench();
    install_ui();
    install_panes();
    install_tools();
}

// ---- the loop ---------------------------------------------------------------------------------------

int Runner::run() {
    bool failed = false;
    if (!o_.bench_out.empty()) record_time(L"startup.hosted", s_.startup_ms());

    for (const std::wstring& raw : o_.commands) {
        const std::wstring line = trim(raw);
        if (line.empty() || line[0] == L'#') continue;
        try {
            execute(line);
            say(L"OK " + line);
        } catch (const std::exception& e) {
            say(L"FAIL " + line + L" — " + from_utf8(e.what()));
            failed = true;
            if (!o_.keep_going) break;
        }
    }
    return failed ? Exit::Failed : Exit::Ok;
}

void Runner::execute(const std::wstring& line) {
    auto [verb, rest] = split_first(line);
    std::transform(verb.begin(), verb.end(), verb.begin(), [](wchar_t c) { return static_cast<wchar_t>(std::towlower(c)); });
    const auto it = verbs_.find(verb);
    if (it != verbs_.end()) {
        it->second(rest);
        return;
    }
    if (all_verbs().count(verb)) {
        throw NotImplemented("'" + narrow(verb) + "' is not implemented in the native harness yet (the feature it drives is not built).");
    }
    throw FormatError("'" + narrow(verb) + "' is not a command. Run with --help for the list.");
}

// ---- helpers ----------------------------------------------------------------------------------------

std::pair<std::wstring, std::wstring> Runner::split_first(const std::wstring& line) {
    const size_t space = line.find(L' ');
    if (space == std::wstring::npos) return {line, L""};
    return {line.substr(0, space), trim(line.substr(space + 1))};
}

std::pair<std::wstring, std::wstring> Runner::split_on(const std::wstring& line, const std::wstring& sep, const char* verb) {
    const std::wstring padded = L" " + line;
    const std::wstring needle = L" " + sep + L" ";
    size_t at = std::wstring::npos;
    for (size_t pos = 0; (pos = padded.find(needle, pos)) != std::wstring::npos; ++pos) at = pos;
    if (at == std::wstring::npos) {
        throw FormatError(std::string(verb) + " wants '<names> to <folder>', or 'to <folder>' for the selection.");
    }
    return {trim(padded.substr(0, at)), trim(padded.substr(at + sep.size() + 2))};
}

std::wstring Runner::require(const std::wstring& rest, const char* verb) {
    if (rest.empty()) throw FormatError(std::string(verb) + " needs an argument.");
    if (rest.size() > 1 && rest.front() == L'"' && rest.back() == L'"') return rest.substr(1, rest.size() - 2);
    return rest;
}

int Runner::number(const std::wstring& text, const char* verb) {
    const std::wstring t = trim(text);
    wchar_t* end = nullptr;
    const long v = std::wcstol(t.c_str(), &end, 10);
    if (t.empty() || !end || *end != 0) throw FormatError(std::string(verb) + " needs a number, got '" + narrow(text) + "'.");
    return static_cast<int>(v);
}

double Runner::fraction(const std::wstring& text, const char* verb) {
    const std::wstring t = trim(text);
    wchar_t* end = nullptr;
    const double v = std::wcstod(t.c_str(), &end);
    if (t.empty() || !end || *end != 0 || v < 0 || v > 1) {
        throw FormatError(std::string(verb) + " needs a number from 0 to 1, got '" + narrow(text) + "'.");
    }
    return v;
}

bool Runner::on_off(const std::wstring& text, const char* verb) {
    std::wstring t = trim(text);
    std::transform(t.begin(), t.end(), t.begin(), [](wchar_t c) { return static_cast<wchar_t>(std::towlower(c)); });
    if (t == L"on" || t == L"true" || t == L"yes") return true;
    if (t == L"off" || t == L"false" || t == L"no") return false;
    throw FormatError(std::string(verb) + " wants 'on' or 'off', got '" + narrow(t) + "'.");
}

std::vector<std::wstring> Runner::csv(const std::wstring& text) {
    std::vector<std::wstring> out;
    for (const std::wstring& part : bb::core::split(text, L',')) {
        const std::wstring t = trim(part);
        if (!t.empty()) out.push_back(t);
    }
    return out;
}

std::vector<std::wstring> Runner::row_names() {
    std::vector<std::wstring> out;
    if (auto l = s_.core().listing(tab())) {
        for (size_t i = 0; i < l->size(); ++i) out.push_back(l->name(i));
    }
    return out;
}

static std::string join_names(const std::vector<std::wstring>& names) {
    std::string out;
    for (size_t i = 0; i < names.size(); ++i) out += (i ? ", " : "") + narrow(names[i]);
    return out;
}

size_t Runner::row_index(const std::wstring& name) {
    if (auto l = s_.core().listing(tab())) {
        for (size_t i = 0; i < l->size(); ++i) {
            if (iequals(l->name(i), name)) return i;
        }
    }
    throw AssertionError("There is no row named '" + narrow(name) + "'. The list holds: " + join_names(row_names()));
}

std::wstring Runner::row_path(const std::wstring& name) {
    auto l = s_.core().listing(tab());
    BbRow r{};
    if (!l || !l->row(row_index(name), r)) throw AssertionError("There is no row named '" + narrow(name) + "'.");
    return bb::core::w(r.full_path);
}

std::vector<std::wstring> Runner::sources_for(const std::wstring& names, const char* verb) {
    std::vector<std::wstring> paths;
    if (names.empty()) {
        paths = s_.core().selection_paths(tab());
        if (paths.empty()) throw AssertionError("Nothing is selected; use 'select <name>' first.");
    } else {
        for (const std::wstring& n : csv(names)) paths.push_back(row_path(n));
    }
    for (std::wstring& p : paths) p = sandbox_.require_inside(p, verb).wstring();
    return paths;
}

void Runner::invoke_settled(const std::function<void()>& fn) {
    fn();
    s_.settle();
}

void Runner::await_op(uint64_t op) {
    const auto start = Clock::now();
    BbOpInfo info{};
    std::wstring message;
    for (;;) {
        s_.pump();
        BbOwnedStr m{};
        core::check(bb_op_state(s_.core().app(), op, &info, &m), "bb_op_state");
        message = core::take(m);
        if (info.phase == BB_OP_DONE) break;
        if (std::chrono::duration_cast<std::chrono::milliseconds>(Clock::now() - start).count() > o_.busy_timeout_ms) {
            throw TimeoutError("The operation did not finish within " + std::to_string(o_.busy_timeout_ms) + " ms.");
        }
        Sleep(1);
    }
    s_.settle();
    if (!info.ok) throw AssertionError(message.empty() ? "the operation failed." : narrow(message));
}

void Runner::transfer(const std::wstring& rest, bool move) {
    const char* verb = move ? "move" : "copy";
    const auto [names, dest] = split_on(rest, L"to", verb);
    const std::vector<std::wstring> srcs = sources_for(names, move ? "Move" : "Copy");
    const fs::path target = sandbox_.require_inside(dest, move ? "Move" : "Copy");
    if (!fs::is_directory(target)) throw AssertionError("There is no folder at '" + narrow(target.wstring()) + "'.");
    std::vector<BbStr> views;
    for (const auto& p : srcs) views.push_back(core::s(p));
    const std::wstring t = target.wstring();
    uint64_t op = 0;
    core::check((move ? bb_op_move : bb_op_copy)(s_.core().app(), views.data(), static_cast<uint32_t>(views.size()), core::s(t), &op), verb);
    await_op(op);
}

void Runner::erase(const std::wstring& rest, BbDeleteMode mode) {
    const std::vector<std::wstring> srcs = sources_for(rest.empty() ? L"" : rest, "delete");
    std::vector<BbStr> views;
    for (const auto& p : srcs) views.push_back(core::s(p));
    uint64_t op = 0;
    core::check(bb_op_delete(s_.core().app(), views.data(), static_cast<uint32_t>(views.size()), mode, &op), "delete");
    await_op(op);
}

void Runner::undo_step(int steps) {
    BbUndoState st{};
    core::check(bb_undo_state(s_.core().app(), &st), "bb_undo_state");
    if (steps > 0 && !st.can_undo) throw AssertionError("There is nothing to undo.");
    if (steps < 0 && !st.can_redo) throw AssertionError("There is nothing to redo.");
    uint64_t op = 0;
    core::check(bb_undo_step(s_.core().app(), steps, &op), steps > 0 ? "undo" : "redo");
    await_op(op);
}

void Runner::select_names(const std::wstring& names) {
    std::vector<uint32_t> idx;
    for (const std::wstring& n : csv(names)) idx.push_back(static_cast<uint32_t>(row_index(n)));
    s_.core().select(tab(), idx.empty() ? BB_SELECT_CLEAR : BB_SELECT_REPLACE, idx);
    s_.settle();
}

void Runner::search(const std::wstring& rest, bool global) {
    const std::wstring query = require(rest, global ? "gsearch" : "search");
    if (global && !o_.index_declined) {
        throw AssertionError(
            "Whole-PC search reads the MFT index, which this run did not build. The native harness has no index service "
            "yet; use 'search', which walks the current folder.");
    }
    s_.view().active_view()->type_search(query, global);
    s_.settle();
    // A query the parser rejects never starts a search; otherwise wait the search out.
    const auto start = Clock::now();
    while (s_.core().tab_state(tab()).is_search_running &&
           std::chrono::duration_cast<std::chrono::milliseconds>(Clock::now() - start).count() < o_.busy_timeout_ms) {
        s_.pump();
        Sleep(2);
    }
    s_.settle();
}

void Runner::split_pane(const std::wstring& rest) {
    const auto [direction, tail] = split_first(rest);
    std::wstring d = direction;
    std::transform(d.begin(), d.end(), d.begin(), [](wchar_t c) { return static_cast<wchar_t>(std::towlower(c)); });
    BbOrientation o;
    if (d == L"right") o = BB_VERTICAL;
    else if (d == L"down" || d == L"below") o = BB_HORIZONTAL;
    else throw FormatError("split wants 'right' or 'down', got '" + narrow(direction) + "'.");
    const std::wstring path = tail.empty() ? s_.core().tab_text(tab(), BB_TEXT_PATH) : sandbox_.resolve(tail).wstring();
    uint64_t pane = 0;
    core::check(bb_pane_split(s_.core().app(), 0, o, core::s(path), &pane), "split");
    s_.settle();
}

void Runner::shot(const std::wstring& rest) {
    auto [name, element] = split_first(rest);
    std::wstring file = name.empty() ? (shots_ + 1 < 10 ? L"0" : L"") + std::to_wstring(shots_ + 1) + L"-shot.png" : name;
    ++shots_;
    if (!name.empty() && !(file.size() >= 4 && _wcsicmp(file.c_str() + file.size() - 4, L".png") == 0)) file += L".png";
    const fs::path path = fs::path(file).is_absolute() ? fs::path(file) : o_.out_dir / file;
    if (path.has_parent_path()) fs::create_directories(path.parent_path());

    s_.window().render();
    auto* bitmap = s_.window().bitmap();
    if (!bitmap) throw std::runtime_error("the window has no bitmap surface to capture");
    RECT region{};
    const RECT* crop = nullptr;
    if (!element.empty()) {
        bb::ui::Widget* w = s_.view().find_named(narrow(element));
        if (!w) throw FormatError("There is no element named '" + narrow(element) + "' in the window.");
        const float k = bitmap->scale();
        region = {static_cast<LONG>(std::floor(w->bounds.l * k)), static_cast<LONG>(std::floor(w->bounds.t * k)),
                  static_cast<LONG>(std::ceil(w->bounds.r * k)), static_cast<LONG>(std::ceil(w->bounds.b * k))};
        if (region.right <= region.left || region.bottom <= region.top) {
            throw std::runtime_error(narrow(element) + " has no visible size to capture; it is probably collapsed.");
        }
        crop = &region;
    }
    if (bitmap->is_flat()) throw AssertionError(narrow(path.wstring()) + " is a single flat colour — the window rendered nothing.");
    if (!bitmap->write_png(path.wstring(), crop)) throw std::runtime_error("could not write " + narrow(path.wstring()));
    say("SHOT " + narrow(path.wstring()));
    if (o_.verbose) {
        say("# " + std::to_string(bitmap->width_px()) + "x" + std::to_string(bitmap->height_px()) +
            (element.empty() ? "" : " of " + narrow(element)));
    }
}

std::string Runner::state_json() {
    const bb::json::Value st = bb::json::parse(s_.core().state_json());
    const uint64_t active = static_cast<uint64_t>(st["activeTab"].i64());
    bb::json::Value none;
    const bb::json::Value* t = &none;
    for (size_t i = 0; i < st["tabs"].size(); ++i) {
        if (static_cast<uint64_t>(st["tabs"][i]["id"].i64()) == active) t = &st["tabs"][i];
    }
    const bb::json::Value& tab = *t;
    std::string cols;
    {
        const auto columns = s_.core().columns(active);
        for (size_t i = 0; i < columns.size(); ++i) cols += (i ? ", " : "") + narrow(columns[i].id);
    }
    const int64_t side = tab["sidePane"].i64();
    bb::views::PreviewPane* preview = s_.view().active_view() ? s_.view().active_view()->preview() : nullptr;
    const auto titles = s_.view().tab_titles_in_active_pane();
    std::wstring hidden_text;
    {
        BbOwnedStr o{};
        if (bb_settings_get(s_.core().app(), core::s(L"showHiddenItems"), &o) == BB_OK) hidden_text = core::take(o);
    }
    char scale[32];
    snprintf(scale, sizeof(scale), "%.15g", std::round(tab["thumbnailScale"].num() * 100) / 100);
    const auto& tr = st["transfer"];

    struct Field {
        const char* name;
        std::string value;
    };
    const std::vector<Field> f = {
        {"path", quote(tab["path"].str())},
        {"items", std::to_string(tab["count"].i64())},
        {"selected", std::to_string(tab["selected"].i64())},
        {"flattened", boolean(tab["flattened"].boolean())},
        {"flat", boolean(tab["flatView"].boolean())},
        {"notice", quote(tab["notice"].str())},
        {"columns", quote(cols)},
        {"insideArchive", boolean(tab["insideArchive"].boolean())},
        {"search", quote(tab["search"].str())},
        {"globalSearch", boolean(tab["globalSearch"].boolean())},
        {"savedSearches", quote(std::string())},
        {"tabs", std::to_string(s_.view().tab_count_in_active_pane())},
        {"tabTitles", quote(join_names(titles))},
        {"panes", std::to_string(s_.view().pane_count())},
        {"hidden", boolean(trim(hidden_text) == L"true")},
        {"thumbnails", scale},
        {"preview", boolean(side == 1)},
        {"previewState", quote(preview ? preview->state_name() : std::string("none"))},
        {"previewMode", quote(std::string(preview && preview->mode() == 1 ? "Text" : preview && preview->mode() == 2 ? "Hex" : "Auto"))},
        {"previewTitle", quote(preview ? preview->title() : std::wstring())},
        {"previewMessage", quote(preview ? preview->message() : std::wstring())},
        {"previewFooter", quote(preview ? preview->footer() : std::wstring())},
        {"previewPaneWidth", preview ? shortest(static_cast<double>(preview->bounds.width())) : std::string("0")},
        {"sidePane", quote(std::string(side == 1 ? "Preview" : side == 2 ? "Metadata" : "None"))},
        {"canUndo", boolean(st["canUndo"].boolean())},
        {"undo", quote(st["undo"].str())},
        {"canRedo", boolean(st["canRedo"].boolean())},
        {"redo", quote(st["redo"].str())},
        {"history", std::to_string(st["undoEntries"].i64())},
        {"historyCursor", "0"},
        {"historyReleased", "0"},
        {"foregroundCorrections", std::to_string(s_.foreground_corrections())},
        {"selection", quote(std::string())},
        {"status", quote(tab["status"].str())},
        {"indexing", quote(std::string())},
        {"indexingCanRetry", "false"},
        {"isTransferring", boolean(st["transferring"].boolean())},
        {"transferHeadline", quote(tr["current"].str())},
        {"transferDetail", quote(std::string())},
        {"transferBytes", std::to_string(tr["bytes"].i64())},
        {"transferBytesTotal", std::to_string(tr["total"].i64())},
        {"transferIndeterminate", "false"},
        {"queueJobs", "0"},
        {"queueWaiting", quote(std::string())},
        {"queuePaused", boolean(st["paused"].boolean())},
        {"queueKinds", quote(std::string())},
    };
    std::string out = "{";
    for (size_t i = 0; i < f.size(); ++i) out += (i ? "," : "") + quote(std::string(f[i].name)) + ":" + f[i].value;
    return out + "}";
}

// ---- verbs ------------------------------------------------------------------------------------------

void Runner::install() {
    verbs_[L"echo"] = [this](const std::wstring& r) { say(r); };
    verbs_[L"sleep"] = [this](const std::wstring& r) {
        const auto start = Clock::now();
        const int ms = number(r, "sleep");
        while (std::chrono::duration_cast<std::chrono::milliseconds>(Clock::now() - start).count() < ms) {
            s_.pump();
            Sleep(2);
        }
    };
    verbs_[L"settle"] = [this](const std::wstring& r) { s_.settle(r.empty() ? 0 : number(r, "settle")); };

    // ---- fixtures ----
    verbs_[L"tree"] = [this](const std::wstring& r) { say(L"# tree: " + sandbox_.populate(r.empty() ? L"." : r).wstring()); };
    verbs_[L"mkdir"] = [this](const std::wstring& r) { fs::create_directories(sandbox_.require_inside(require(r, "mkdir"), "mkdir")); };
    verbs_[L"write"] = [this](const std::wstring& r) {
        const auto [path, tail] = split_first(r);
        const int bytes = tail.empty() ? 64 : number(tail, "write");
        Sandbox::write(sandbox_.require_inside(require(path, "write"), "write"), static_cast<size_t>(std::max(0, bytes)));
    };
    verbs_[L"write-text"] = [this](const std::wstring& r) {
        const auto [path, tail] = split_first(r);
        const fs::path full = sandbox_.require_inside(require(path, "write-text"), "write-text");
        if (full.has_parent_path()) fs::create_directories(full.parent_path());
        std::wstring text;
        for (wchar_t c : tail) text += c == L'|' ? L'\n' : c;
        std::ofstream out(full, std::ios::binary | std::ios::trunc);
        const std::string bytes = narrow(text) + "\n";
        out.write(bytes.data(), static_cast<std::streamsize>(bytes.size()));
    };
    verbs_[L"write-binary"] = [this](const std::wstring& r) {
        std::vector<std::wstring> parts;
        for (const auto& p : bb::core::split(require(r, "write-binary"), L' ')) {
            if (!p.empty()) parts.push_back(p);
        }
        if (parts.size() < 2 || parts.size() > 3) throw FormatError("write-binary wants <path> <bytes> [differ-at].");
        const fs::path full = sandbox_.require_inside(parts[0], "write-binary");
        const int count = number(parts[1], "write-binary");
        std::string bytes(static_cast<size_t>(std::max(0, count)), '\0');
        for (int i = 0; i < count; ++i) bytes[static_cast<size_t>(i)] = static_cast<char>((i * 7 + (i / 251)) % 256);
        if (parts.size() == 3) {
            const int at = number(parts[2], "write-binary");
            if (at >= 0 && at < count) bytes[static_cast<size_t>(at)] = static_cast<char>(bytes[static_cast<size_t>(at)] ^ 0xFF);
        }
        if (full.has_parent_path()) fs::create_directories(full.parent_path());
        std::ofstream out(full, std::ios::binary | std::ios::trunc);
        out.write(bytes.data(), static_cast<std::streamsize>(bytes.size()));
    };
    verbs_[L"sandbox"] = [this](const std::wstring&) { say(sandbox_.root().wstring()); };

    // ---- navigation ----
    verbs_[L"go"] = [this](const std::wstring& r) {
        const fs::path path = sandbox_.resolve(require(r, "go"));
        std::error_code ec;
        if (!fs::is_directory(path, ec)) throw AssertionError("There is no folder at '" + narrow(path.wstring()) + "'.");
        s_.core().navigate(tab(), path.wstring());
        s_.settle();
    };
    verbs_[L"up"] = [this](const std::wstring&) { invoke_settled([&] { core::check(bb_tab_up(s_.core().app(), tab()), "up"); }); };
    verbs_[L"back"] = [this](const std::wstring&) { invoke_settled([&] { core::check(bb_tab_back(s_.core().app(), tab()), "back"); }); };
    verbs_[L"forward"] = [this](const std::wstring&) { invoke_settled([&] { core::check(bb_tab_forward(s_.core().app(), tab()), "forward"); }); };
    verbs_[L"refresh"] = [this](const std::wstring&) { invoke_settled([&] { core::check(bb_tab_refresh(s_.core().app(), tab()), "refresh"); }); };
    verbs_[L"enter"] = [this](const std::wstring& r) {
        const std::wstring name = require(r, "enter");
        const size_t index = row_index(name);
        BbRow row{};
        auto l = s_.core().listing(tab());
        l->row(index, row);
        const bool dir = (row.flags & BB_ROW_DIRECTORY) != 0;
        const bool archive = [&] {
            for (const wchar_t* ext : {L".zip", L".7z", L".rar", L".tar", L".gz", L".tgz", L".bz2", L".xz"}) {
                const size_t n = wcslen(ext);
                if (name.size() >= n && _wcsicmp(name.c_str() + name.size() - n, ext) == 0) return true;
            }
            return false;
        }();
        if (!dir && !archive) {
            throw std::runtime_error("'" + narrow(name) + "' is a file, and this run starts no programs. Use 'go' for folders, or "
                                     "assert on the status bar if you meant to check the refusal.");
        }
        core::check(bb_tab_open_item(s_.core().app(), tab(), static_cast<uint32_t>(index)), "enter");
        s_.settle();
    };
    verbs_[L"tree-click"] = [this](const std::wstring& r) {
        const fs::path path = sandbox_.resolve(require(r, "tree-click"));
        auto& provider = s_.view().folder_tree();
        const auto chain = provider.chain_to(path.wstring());
        if (chain.empty() || _wcsicmp(provider.path_of(chain.back()).c_str(), path.c_str()) != 0) {
            throw AssertionError("The tree has no row for '" + narrow(path.wstring()) + "' showing; navigate there or to its parent first.");
        }
        auto* tv = s_.view().tree_view();
        tv->select(chain.back());
        s_.core().navigate(tab(), provider.path_of(chain.back()));
        s_.settle(200);
    };
    verbs_[L"tree-expand"] = [this](const std::wstring& r) {
        std::wstring arg = require(r, "tree-expand");
        bool expand = true;
        if (arg.size() > 4 && iequals(arg.substr(arg.size() - 4), L" off")) { expand = false; arg = trim(arg.substr(0, arg.size() - 4)); }
        else if (arg.size() > 3 && iequals(arg.substr(arg.size() - 3), L" on")) { arg = trim(arg.substr(0, arg.size() - 3)); }
        const fs::path path = sandbox_.resolve(arg);
        auto& provider = s_.view().folder_tree();
        const auto chain = provider.chain_to(path.wstring());
        if (chain.empty() || _wcsicmp(provider.path_of(chain.back()).c_str(), path.c_str()) != 0) {
            throw AssertionError("The tree has no row for '" + narrow(path.wstring()) + "' showing; navigate there or to its parent first.");
        }
        if (expand) s_.view().tree_view()->expand(chain.back());
        else s_.view().tree_view()->collapse(chain.back());
        s_.settle(200);
    };

    // ---- selection ----
    verbs_[L"select"] = [this](const std::wstring& r) { select_names(require(r, "select")); };
    verbs_[L"select-all"] = [this](const std::wstring&) {
        core::check(bb_tab_select(s_.core().app(), tab(), BB_SELECT_ALL, nullptr, 0), "select-all");
        s_.settle();
    };
    verbs_[L"deselect"] = [this](const std::wstring&) {
        core::check(bb_tab_select(s_.core().app(), tab(), BB_SELECT_CLEAR, nullptr, 0), "deselect");
        s_.settle();
    };

    // ---- tabs and panes ----
    verbs_[L"newtab"] = [this](const std::wstring& r) {
        const std::wstring path = r.empty() ? s_.core().tab_text(tab(), BB_TEXT_PATH) : sandbox_.resolve(r).wstring();
        uint64_t t = 0;
        core::check(bb_tab_open(s_.core().app(), 0, core::s(path), true, &t), "newtab");
        s_.settle();
    };
    verbs_[L"closetab"] = [this](const std::wstring&) {
        core::check(bb_tab_close(s_.core().app(), tab()), "closetab");
        s_.settle();
    };
    verbs_[L"tab"] = [this](const std::wstring& r) {
        const int index = number(r, "tab");
        const auto ids = s_.view().tab_ids_in_active_pane();
        if (index < 1 || static_cast<size_t>(index) > ids.size()) {
            throw AssertionError("This pane has " + std::to_string(ids.size()) + " tab(s); there is no tab " + std::to_string(index) + ".");
        }
        core::check(bb_tab_activate(s_.core().app(), ids[static_cast<size_t>(index) - 1]), "tab");
        s_.settle();
    };
    verbs_[L"split"] = [this](const std::wstring& r) { split_pane(r); };
    verbs_[L"closepane"] = [this](const std::wstring&) {
        core::check(bb_pane_close(s_.core().app(), s_.view().active_pane()), "closepane");
        s_.settle();
    };
    verbs_[L"pane"] = [this](const std::wstring& r) {
        const int index = number(r, "pane");
        const auto ids = s_.view().pane_ids();
        if (index < 1 || static_cast<size_t>(index) > ids.size()) {
            throw AssertionError("There are " + std::to_string(ids.size()) + " pane(s); there is no pane " + std::to_string(index) + ".");
        }
        core::check(bb_pane_activate(s_.core().app(), ids[static_cast<size_t>(index) - 1]), "pane");
        s_.settle();
    };

    // ---- searching, flat, hidden ----
    verbs_[L"search"] = [this](const std::wstring& r) { search(r, false); };
    verbs_[L"gsearch"] = [this](const std::wstring& r) { search(r, true); };
    verbs_[L"clear-search"] = [this](const std::wstring&) {
        const bool global = s_.core().tab_state(tab()).is_global_search;
        s_.view().active_view()->type_search(L"", global);
        s_.settle();
    };
    verbs_[L"flat"] = [this](const std::wstring& r) {
        const bool on = on_off(r, "flat");
        core::check(bb_tab_set_flat(s_.core().app(), tab(), on), "flat");
        s_.settle();
    };
    verbs_[L"hidden"] = [this](const std::wstring& r) {
        const bool on = on_off(r, "hidden");
        core::check(bb_settings_set(s_.core().app(), core::s(L"showHiddenItems"), core::s(on ? L"true" : L"false")), "hidden");
        s_.settle();
    };

    // ---- writing ----
    verbs_[L"rename"] = [this](const std::wstring& r) {
        const std::wstring pattern = require(r, "rename");
        const std::vector<std::wstring> srcs = sources_for(L"", "rename");
        std::vector<BbStr> views;
        for (const auto& p : srcs) views.push_back(core::s(p));
        uint64_t op = 0;
        core::check(bb_op_rename(s_.core().app(), views.data(), static_cast<uint32_t>(views.size()), core::s(pattern), &op), "rename");
        await_op(op);
    };
    verbs_[L"delete"] = [this](const std::wstring& r) { erase(r, BB_DELETE_RECYCLE); };
    verbs_[L"delete-permanent"] = [this](const std::wstring& r) { erase(r, BB_DELETE_PERMANENT); };
    verbs_[L"move"] = [this](const std::wstring& r) { transfer(r, true); };
    verbs_[L"copy"] = [this](const std::wstring& r) { transfer(r, false); };
    verbs_[L"undo"] = [this](const std::wstring&) { undo_step(1); };
    verbs_[L"redo"] = [this](const std::wstring&) { undo_step(-1); };

    // ---- reading back ----
    verbs_[L"shot"] = [this](const std::wstring& r) { shot(r); };
    verbs_[L"state"] = [this](const std::wstring&) { say("STATE " + state_json()); };
    verbs_[L"rows"] = [this](const std::wstring&) { say("ROWS " + join_names(row_names())); };
}

void Runner::install_asserts() {
    verbs_[L"assert-count"] = [this](const std::wstring& r) {
        const int expected = number(r, "assert-count");
        const int actual = static_cast<int>(s_.core().tab_state(tab()).count);
        if (actual != expected) {
            throw AssertionError("expected " + std::to_string(expected) + " row(s), got " + std::to_string(actual) + ": " + join_names(row_names()));
        }
    };
    auto row_assert = [this](const std::wstring& r, bool expected) {
        const std::wstring name = require(r, expected ? "assert-row" : "assert-no-row");
        bool present = false;
        for (const auto& n : row_names()) present = present || iequals(n, name);
        if (present != expected) {
            throw AssertionError(expected ? "'" + narrow(name) + "' is not in the list: " + join_names(row_names())
                                          : "'" + narrow(name) + "' is still in the list.");
        }
    };
    verbs_[L"assert-row"] = [row_assert](const std::wstring& r) { row_assert(r, true); };
    verbs_[L"assert-no-row"] = [row_assert](const std::wstring& r) { row_assert(r, false); };
    verbs_[L"assert-selected"] = [this](const std::wstring& r) {
        const int expected = number(r, "assert-selected");
        const int actual = static_cast<int>(s_.core().tab_state(tab()).selected);
        if (actual != expected) {
            throw AssertionError("expected " + std::to_string(expected) + " selected item(s), got " + std::to_string(actual) + ".");
        }
    };
    verbs_[L"assert-tabs"] = [this](const std::wstring& r) {
        const int expected = number(r, "assert-tabs");
        const int actual = static_cast<int>(s_.view().tab_count_in_active_pane());
        if (actual != expected) {
            throw AssertionError("expected " + std::to_string(expected) + " tab(s) in this pane, got " + std::to_string(actual) + ".");
        }
    };
    verbs_[L"assert-panes"] = [this](const std::wstring& r) {
        const int expected = number(r, "assert-panes");
        const int actual = static_cast<int>(s_.view().pane_count());
        if (actual != expected) {
            throw AssertionError("expected " + std::to_string(expected) + " pane(s), got " + std::to_string(actual) + ".");
        }
    };
    auto flattened = [this](bool expected) {
        const bool actual = s_.core().tab_state(tab()).is_flattened;
        if (actual != expected) {
            throw AssertionError(expected ? "the list is a normal directory listing, not a flattened one." : "the list is still flattened.");
        }
    };
    verbs_[L"assert-flattened"] = [flattened](const std::wstring&) { flattened(true); };
    verbs_[L"assert-not-flattened"] = [flattened](const std::wstring&) { flattened(false); };
    verbs_[L"assert-flat"] = [this](const std::wstring& r) {
        const bool actual = s_.core().tab_state(tab()).is_flat_view;
        const bool expected = trim(r).empty() || on_off(r, "assert-flat");
        if (actual != expected) {
            throw AssertionError(std::string("expected the flat view to be ") + (expected ? "on" : "off") + ", got " + (actual ? "on" : "off") + ".");
        }
    };
    verbs_[L"assert-not-flat"] = [this](const std::wstring&) {
        if (s_.core().tab_state(tab()).is_flat_view) throw AssertionError("expected the flat view to be off, got on.");
    };
    auto inside_archive = [this](bool expected) {
        const bb::json::Value st = bb::json::parse(s_.core().state_json());
        const uint64_t active = static_cast<uint64_t>(st["activeTab"].i64());
        bool actual = false;
        for (size_t i = 0; i < st["tabs"].size(); ++i) {
            if (static_cast<uint64_t>(st["tabs"][i]["id"].i64()) == active) actual = st["tabs"][i]["insideArchive"].boolean();
        }
        if (actual != expected) {
            throw AssertionError(expected ? "the tab is not inside an archive." : "the tab is inside an archive.");
        }
    };
    verbs_[L"assert-inside-archive"] = [inside_archive](const std::wstring&) { inside_archive(true); };
    verbs_[L"assert-not-inside-archive"] = [inside_archive](const std::wstring&) { inside_archive(false); };
    verbs_[L"assert-exists"] = [this](const std::wstring& r) {
        const fs::path p = sandbox_.resolve(require(r, "assert-exists"));
        std::error_code ec;
        if (!fs::exists(p, ec)) throw AssertionError("there is nothing at '" + narrow(p.wstring()) + "'.");
    };
    verbs_[L"assert-missing"] = [this](const std::wstring& r) {
        const fs::path p = sandbox_.resolve(require(r, "assert-missing"));
        std::error_code ec;
        if (fs::exists(p, ec)) throw AssertionError("'" + narrow(p.wstring()) + "' is still there.");
    };
    verbs_[L"assert-path"] = [this](const std::wstring& r) {
        const std::wstring expected = require(r, "assert-path");
        const std::wstring actual = s_.core().tab_text(tab(), BB_TEXT_PATH);
        if (!icontains(actual, expected)) throw AssertionError("expected the path to contain '" + narrow(expected) + "', got '" + narrow(actual) + "'.");
    };
    verbs_[L"assert-status"] = [this](const std::wstring& r) {
        const std::wstring expected = require(r, "assert-status");
        const std::wstring actual = s_.core().tab_text(tab(), BB_TEXT_STATUS);
        if (!icontains(actual, expected)) throw AssertionError("expected the status to contain '" + narrow(expected) + "', got '" + narrow(actual) + "'.");
    };
    verbs_[L"assert-error"] = [this](const std::wstring& r) {
        const std::wstring actual = s_.core().tab_text(tab(), BB_TEXT_ERROR);
        const bool has = s_.core().tab_state(tab()).has_error;
        if (trim(r).empty()) {
            if (has) throw AssertionError("expected no error banner, got '" + narrow(actual) + "'.");
            return;
        }
        if (!has) throw AssertionError("expected an error banner containing '" + narrow(r) + "', there was none.");
        if (!icontains(actual, r)) throw AssertionError("expected the error banner to contain '" + narrow(r) + "', got '" + narrow(actual) + "'.");
    };
    auto can_undo = [this](bool expected, bool redo) {
        BbUndoState st{};
        core::check(bb_undo_state(s_.core().app(), &st), "bb_undo_state");
        const bool actual = redo ? st.can_redo : st.can_undo;
        const char* what = redo ? "redo" : "undo";
        if (actual != expected) {
            throw AssertionError(expected ? std::string("there is nothing to ") + what + "." : std::string("there is still something to ") + what + ".");
        }
    };
    verbs_[L"assert-can-undo"] = [can_undo](const std::wstring&) { can_undo(true, false); };
    verbs_[L"assert-cannot-undo"] = [can_undo](const std::wstring&) { can_undo(false, false); };
    verbs_[L"assert-can-redo"] = [can_undo](const std::wstring&) { can_undo(true, true); };
    verbs_[L"assert-cannot-redo"] = [can_undo](const std::wstring&) { can_undo(false, true); };
    auto visibility = [this](const std::wstring& r, bool expected) {
        const std::wstring name = require(r, expected ? "assert-visible" : "assert-hidden");
        bb::ui::Widget* w = s_.view().find_named(narrow(name));
        const bool actual = w && w->flags.visible;
        if (actual != expected) {
            throw AssertionError("expected " + narrow(name) + " to be " + (expected ? "visible" : "not visible") + ", it is not.");
        }
    };
    verbs_[L"assert-visible"] = [visibility](const std::wstring& r) { visibility(r, true); };
    verbs_[L"assert-hidden"] = [visibility](const std::wstring& r) { visibility(r, false); };
    verbs_[L"assert-not-launched"] = [this](const std::wstring&) {
        const bb::json::Value st = bb::json::parse(s_.core().state_json());
        const int64_t n = st["launches"].i64();
        if (n > 0) throw AssertionError("the run tried to start " + std::to_string(n) + " program(s) (all refused).");
    };
}

}  // namespace bbh
