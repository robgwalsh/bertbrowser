// The verbs of the operations UI (tools/BertBrowser.Harness/ScriptRunner.cs: the queue, clashes, right-drag,
// shortcuts, archives, elevation, the undo history). Each one drives the core through the same exports the
// product's views call; what a script cannot do - click, drag - is posed through the core's own models, so
// what is photographed is what a person would see.
#include "runner.h"

#include <windows.h>
#include <aclapi.h>
#include <sddl.h>

#include <algorithm>
#include <cwctype>
#include <cstdlib>
#include <fstream>

#include "views/json.h"
#include "views/model.h"
#include "views/file_drag.h"
#include "views/ops.h"
#include "views/shell_menu.h"

namespace bbh {

namespace core = bb::core;
namespace ops = bb::views::ops;

namespace {

using core::iequals;

std::wstring lower(std::wstring s) {
    for (auto& c : s) c = static_cast<wchar_t>(std::towlower(c));
    return s;
}

std::wstring trim(const std::wstring& s) {
    const size_t b = s.find_first_not_of(L" \t\r\n");
    if (b == std::wstring::npos) return {};
    return s.substr(b, s.find_last_not_of(L" \t\r\n") - b + 1);
}

std::wstring slashes(std::wstring s) {
    for (auto& c : s) {
        if (c == L'/') c = L'\\';
    }
    return s;
}

// Folders the run has denied itself deletes in, to be lifted before the sandbox is removed.
struct Denied {
    std::wstring folder;
    PSECURITY_DESCRIPTOR descriptor = nullptr;  // the folder's own, as it was
    PACL dacl = nullptr;                         // inside `descriptor`
};
std::vector<Denied>& denied() {
    static std::vector<Denied> d;
    return d;
}

}  // namespace

// Lifts every denial. Never throws: it runs on the way out, where a failure must not hide the run's result.
void release_denials() {
    for (Denied& d : denied()) {
        SetNamedSecurityInfoW(const_cast<LPWSTR>(d.folder.c_str()), SE_FILE_OBJECT,
                              DACL_SECURITY_INFORMATION | UNPROTECTED_DACL_SECURITY_INFORMATION, nullptr, nullptr, d.dacl, nullptr);
        // The children kept the entry they inherited when they were made; letting them inherit again from the
        // restored folder is what makes the tree removable.
        std::error_code ec;
        for (const auto& e : fs::directory_iterator(d.folder, ec)) {
            SetNamedSecurityInfoW(const_cast<LPWSTR>(e.path().c_str()), SE_FILE_OBJECT,
                                  DACL_SECURITY_INFORMATION | UNPROTECTED_DACL_SECURITY_INFORMATION, nullptr, nullptr, nullptr, nullptr);
        }
        if (d.descriptor) LocalFree(d.descriptor);
    }
    denied().clear();
}

void Runner::install_ops() {
    auto run = [this](const char* verb, const std::string& arg = {}) { return ops::run(s_.core(), verb, arg); };
    auto query = [this](const char* what, const std::string& arg = {}) { return ops::query(s_.core(), what, arg); };
    // An operation a verb started, waited out the way every other is.
    auto started = [this](const bb::json::Value& v) {
        if (!v["op"].is_null()) await_op(static_cast<uint64_t>(v["op"].i64()));
        else s_.settle();
    };
    auto fresh_queue = [this]() -> const bb::json::Value& {
        s_.pump();
        s_.view().sync_transfer();
        return s_.view().queue_model();
    };

    // ---- the queue and the strip ----
    verbs_[L"progress-demo"] = [this, run](const std::wstring& r) {
        const std::wstring arg = lower(trim(r));
        if (!arg.empty() && arg != L"sized" && arg != L"unsized" && arg != L"queued" && arg != L"off") {
            throw FormatError("progress-demo takes nothing, 'sized', 'unsized', 'queued' or 'off'.");
        }
        run("pose-transfer", (arg.empty() ? "sized" : to_utf8(arg)) + "|" + to_utf8((sandbox_.root() / "Archive").wstring()));
        s_.settle();
        s_.pump();
        s_.view().sync_transfer();
    };
    verbs_[L"assert-transfer"] = [this, fresh_queue](const std::wstring& r) {
        const std::wstring expected = require(r, "assert-transfer");
        fresh_queue();
        const std::wstring actual = s_.view().transfer_text();
        if (actual.empty()) throw AssertionError("expected a transfer showing '" + to_utf8(expected) + "', but none is running. Run 'progress-demo' first.");
        if (!core::icontains(actual, expected)) throw AssertionError("expected the transfer to show '" + to_utf8(expected) + "', got '" + to_utf8(actual) + "'.");
    };
    verbs_[L"assert-transfer-indeterminate"] = [this, fresh_queue](const std::wstring&) {
        const auto& q = fresh_queue();
        if (!q["active"].boolean()) throw AssertionError("no transfer is running. Run 'progress-demo unsized' first.");
        if (!q["view"]["indeterminate"].boolean()) {
            throw AssertionError("the transfer bar is determinate, but its byte total was never established - a bar pinned at zero reads as a stall rather than as an unmeasured volume.");
        }
    };
    verbs_[L"pause"] = [this, run, fresh_queue](const std::wstring&) {
        if (!fresh_queue()["canPause"].boolean()) throw AssertionError("There is no queued transfer to pause. Run 'progress-demo' or start one first.");
        run("queue-pause");
        s_.settle();
        fresh_queue();
    };
    verbs_[L"resume"] = [this, run, fresh_queue](const std::wstring&) {
        if (!fresh_queue()["canResume"].boolean()) throw AssertionError("Nothing is paused.");
        run("queue-resume");
        s_.settle();
        fresh_queue();
    };
    verbs_[L"queue-move"] = [this, run, fresh_queue](const std::wstring& r) {
        std::vector<std::wstring> w;
        for (const auto& p : bb::core::split(r, L' ')) {
            if (!p.empty()) w.push_back(p);
        }
        if (w.size() != 2) throw FormatError("queue-move wants '<from> <to>', both 1-indexed.");
        const int slot = number(w[0], "queue-move"), to = number(w[1], "queue-move");
        const int step = to < slot ? -1 : 1;
        for (int i = 0; i < std::abs(to - slot); ++i) {
            const auto& q = fresh_queue();
            const auto& jobs = q["jobs"];
            const int at = slot + step * i;
            if (at < 1 || static_cast<size_t>(at) > jobs.size()) throw AssertionError("queue-move wants a slot between 1 and " + std::to_string(jobs.size()) + ".");
            const auto& job = jobs[static_cast<size_t>(at) - 1];
            if (!job[step < 0 ? "canUp" : "canDown"].boolean()) {
                throw AssertionError("'" + job["description"].str() + "' cannot move any further - the running job is always first.");
            }
            run("queue-move", "{\"id\":" + std::to_string(job["id"].i64()) + ",\"delta\":" + std::to_string(step) + "}");
        }
        s_.settle();
        fresh_queue();
    };
    verbs_[L"queue-cancel"] = [this, run, fresh_queue](const std::wstring& r) {
        const int slot = number(require(r, "queue-cancel"), "queue-cancel");
        const auto& jobs = fresh_queue()["jobs"];
        if (slot < 1 || static_cast<size_t>(slot) > jobs.size()) {
            throw AssertionError("queue-cancel wants a slot between 1 and " + std::to_string(jobs.size()) + "; the queue holds " + std::to_string(jobs.size()) + ".");
        }
        run("queue-cancel", std::to_string(jobs[static_cast<size_t>(slot) - 1]["id"].i64()));
        s_.settle();
        fresh_queue();
    };
    verbs_[L"assert-queue"] = [this, fresh_queue](const std::wstring& r) {
        const int expected = number(require(r, "assert-queue"), "assert-queue");
        const int actual = static_cast<int>(fresh_queue()["jobs"].size());
        if (actual != expected) throw AssertionError("expected " + std::to_string(expected) + " job(s) in the queue, found " + std::to_string(actual) + ".");
    };
    auto paused = [fresh_queue](bool expected) {
        if (fresh_queue()["paused"].boolean() != expected) {
            throw AssertionError(expected ? "expected the queue to be paused, and it is running." : "expected the queue to be running, and it is paused.");
        }
    };
    verbs_[L"assert-paused"] = [paused](const std::wstring&) { paused(true); };
    verbs_[L"assert-not-paused"] = [paused](const std::wstring&) { paused(false); };

    // ---- clashes ----
    verbs_[L"conflict"] = [this, run](const std::wstring& r) {
        const std::wstring arg = require(r, "conflict");
        if (arg.find(L'=') == std::wstring::npos) {
            if (lower(arg) == L"cancel") run("h:conflict", "{\"mode\":\"cancel\"}");
            else run("h:conflict", "{\"mode\":\"all\",\"answer\":" + ops::q(arg) + "}");
            return;
        }
        std::string answers = "{";
        bool first = true;
        for (const std::wstring& pair : csv(arg)) {
            const size_t at = pair.find(L'=');
            if (at == std::wstring::npos) throw FormatError("conflict wants '<name>=<answer>, <name>=<answer>'.");
            answers += std::string(first ? "" : ",") + ops::q(trim(pair.substr(0, at))) + ":" + ops::q(trim(pair.substr(at + 1)));
            first = false;
        }
        run("h:conflict", "{\"mode\":\"named\",\"answers\":" + answers + "}}");
    };
    verbs_[L"conflict-plan"] = [this, run](const std::wstring& r) {
        const auto [verb, tail] = split_first(r);
        const std::wstring v = lower(verb);
        if (v != L"move" && v != L"copy") throw FormatError("conflict-plan wants 'move|copy <names> to <folder>'.");
        const auto [names, dest] = split_on(tail, L"to", "conflict-plan");
        const std::vector<std::wstring> srcs = sources_for(names, "conflict-plan");
        const fs::path target = sandbox_.require_inside(dest, "conflict-plan");
        run("h:conflict-plan", "{\"verb\":\"" + to_utf8(v) + "\",\"sources\":" + ops::qa(srcs) + ",\"destination\":" + ops::q(target.wstring()) + "}");
    };
    verbs_[L"assert-conflicts"] = [this, query](const std::wstring& r) {
        const int expected = number(require(r, "assert-conflicts"), "assert-conflicts");
        const int actual = static_cast<int>(query("conflicts")["asked"].i64());
        if (actual != expected) {
            throw AssertionError("expected the conflict dialog to be asked " + std::to_string(expected) + " time(s), it was asked " + std::to_string(actual) + ".");
        }
    };
    auto offered = [query]() { return query("conflicts")["last"]["rows"]; };
    verbs_[L"assert-conflict-rows"] = [offered](const std::wstring& r) {
        const int expected = number(require(r, "assert-conflict-rows"), "assert-conflict-rows");
        const auto rows = offered();
        if (static_cast<int>(rows.size()) != expected) {
            std::string names;
            for (size_t i = 0; i < rows.size(); ++i) names += (i ? ", " : "") + rows[i]["name"].str();
            throw AssertionError("expected " + std::to_string(expected) + " conflict row(s), got " + std::to_string(rows.size()) + (rows.size() ? ": " + names : "."));
        }
    };
    verbs_[L"assert-conflict-row"] = [offered](const std::wstring& r) {
        const std::wstring label = slashes(require(r, "assert-conflict-row"));
        const auto rows = offered();
        std::string names;
        for (size_t i = 0; i < rows.size(); ++i) {
            names += (i ? ", " : "") + rows[i]["name"].str();
            if (iequals(core::wide(rows[i]["name"].str()), label)) return;
        }
        throw AssertionError("no conflict row named '" + to_utf8(label) + "'. Offered: " + names + ".");
    };
    verbs_[L"assert-conflict-default"] = [offered](const std::wstring& r) {
        const std::wstring arg = require(r, "assert-conflict-default");
        const size_t at = arg.rfind(L'=');
        if (at == std::wstring::npos) throw FormatError("assert-conflict-default wants '<label>=<answer>'.");
        const std::wstring key = slashes(trim(arg.substr(0, at)));
        std::string wanted = to_utf8(lower(trim(arg.substr(at + 1))));
        if (wanted == "keepboth") wanted = "keep-both";
        const auto rows = offered();
        for (size_t i = 0; i < rows.size(); ++i) {
            if (!iequals(core::wide(rows[i]["name"].str()), key)) continue;
            if (rows[i]["default"].str() != wanted) {
                throw AssertionError("'" + to_utf8(key) + "' defaulted to " + rows[i]["default"].str() + ", expected " + wanted + ".");
            }
            return;
        }
        throw AssertionError("no conflict row named '" + to_utf8(key) + "'.");
    };

    // ---- right-drag, shortcuts ----
    verbs_[L"right-drop-menu"] = [this](const std::wstring& r) {
        const auto [names, tail] = split_on(r, L"to", "right-drop-menu");
        std::wstring destination = tail, name;
        const std::wstring lowered = lower(tail);
        if (const size_t marker = lowered.rfind(L" as "); marker != std::wstring::npos) {
            destination = trim(tail.substr(0, marker));
            name = trim(tail.substr(marker + 4));
        }
        const std::vector<std::wstring> srcs = sources_for(names, "right-drop-menu");
        const fs::path target = sandbox_.resolve(destination);
        // The real pipeline's menu, built for those sources over that folder; not opened and nothing dropped.
        bb::views::FileDrag drag(s_.view());
        const bb::ui::MenuItems items = drag.verb_menu(srcs, target.wstring());
        shot_menu(items, name.empty() ? L"right-drop-menu" : name);
    };
    verbs_[L"shortcut"] = [this, run, started](const std::wstring& r) {
        const auto [names, dest] = split_on(r, L"to", "shortcut");
        const std::vector<std::wstring> srcs = sources_for(names, "shortcut");
        const fs::path target = sandbox_.require_inside(dest, "shortcut");
        std::error_code ec;
        fs::create_directories(target, ec);
        started(run("shortcut", "{\"sources\":" + ops::qa(srcs) + ",\"directory\":" + ops::q(target.wstring()) + "}"));
    };

    // ---- archives ----
    verbs_[L"archive-fixture"] = [this](const std::wstring& r) {
        const fs::path root = sandbox_.require_inside(r.empty() ? L"." : r, "archive-fixture");
        core::check(bb_fixture_write(core::s(L"archives"), core::s(root.wstring())), "archive-fixture");
        say("# archive fixture: " + to_utf8(root.wstring()));
    };
    verbs_[L"extract"] = [this, run, started](const std::wstring& r) {
        const auto [names, dest] = split_on(r, L"to", "extract");
        std::vector<std::wstring> entries;
        if (!names.empty()) {
            for (const std::wstring& n : csv(names)) entries.push_back(row_path(n));
        }
        const fs::path target = sandbox_.require_inside(dest, "extract");
        std::error_code ec;
        fs::create_directories(target, ec);
        started(run("extract", "{\"tab\":" + std::to_string(tab()) + ",\"names\":" + ops::qa(entries) + ",\"destination\":" + ops::q(target.wstring()) +
                                ",\"conflict\":\"keep-both\"}"));
    };
    verbs_[L"compress"] = [this, run, query, started](const std::wstring& r) {
        const auto [format, name] = split_first(require(r, "compress"));
        if (name.empty()) throw FormatError("compress needs a name for the archive.");
        std::vector<std::wstring> srcs;
        const auto selected = s_.core().selection_paths(tab());
        const std::wstring folder = s_.core().tab_text(tab(), BB_TEXT_PATH);
        for (const auto& p : selected) srcs.push_back(sandbox_.require_inside(p, "compress").wstring());
        if (srcs.empty()) srcs.push_back(sandbox_.require_inside(folder, "compress").wstring());
        const auto defaults = query("compress-defaults", "{\"sources\":" + ops::qa(srcs) + ",\"directory\":" + ops::q(folder) + "}");
        std::wstring suffix;
        std::string all;
        for (size_t i = 0; i < defaults["formats"].size(); ++i) {
            const auto& f = defaults["formats"][i];
            all += (i ? ", " : "") + f["label"].str();
            if (iequals(core::wide(f["label"].str()), format)) suffix = core::wide(f["suffix"].str());
        }
        if (suffix.empty()) throw FormatError("'" + to_utf8(format) + "' is not a format this app can write. Try: " + all + ".");
        const fs::path target = sandbox_.require_inside((fs::path(folder) / (name + suffix)).wstring(), "compress");
        started(run("compress", "{\"sources\":" + ops::qa(srcs) + ",\"target\":" + ops::q(target.wstring()) + ",\"format\":" + ops::q(format) + ",\"level\":\"normal\"}"));
    };
    verbs_[L"unlock"] = [this, run](const std::wstring& r) {
        const std::wstring password = require(r, "unlock");
        run("unlock", "{\"tab\":" + std::to_string(tab()) + ",\"password\":" + ops::q(password) + "}");
        s_.settle();
    };

    // ---- elevation ----
    verbs_[L"deny"] = [this](const std::wstring& r) {
        const fs::path file = sandbox_.require_inside(require(r, "deny"), "deny");
        const fs::path folder = file.parent_path();
        std::error_code ec;
        fs::create_directories(folder, ec);
        Denied d;
        d.folder = folder.wstring();
        PSECURITY_DESCRIPTOR sd = nullptr;
        PACL old = nullptr;
        if (GetNamedSecurityInfoW(d.folder.c_str(), SE_FILE_OBJECT, DACL_SECURITY_INFORMATION, nullptr, nullptr, &old, nullptr, &sd) != ERROR_SUCCESS) {
            throw std::runtime_error("could not read the security of " + to_utf8(d.folder));
        }
        d.descriptor = sd;
        d.dacl = old;
        wchar_t user[256];
        DWORD n = 256;
        GetUserNameW(user, &n);
        EXPLICIT_ACCESSW access{};
        BuildExplicitAccessWithNameW(&access, user, DELETE | FILE_DELETE_CHILD, DENY_ACCESS, SUB_CONTAINERS_AND_OBJECTS_INHERIT);
        PACL merged = nullptr;
        if (SetEntriesInAclW(1, &access, old, &merged) != ERROR_SUCCESS) throw std::runtime_error("could not build a denial");
        const DWORD set = SetNamedSecurityInfoW(const_cast<LPWSTR>(d.folder.c_str()), SE_FILE_OBJECT, DACL_SECURITY_INFORMATION, nullptr, nullptr, merged, nullptr);
        LocalFree(merged);
        if (set != ERROR_SUCCESS) throw std::runtime_error("could not deny deletes in " + to_utf8(d.folder));
        denied().push_back(std::move(d));
        // Created after the denial, so it inherits it: the only version of this that looks like a protected file.
        if (!fs::exists(file)) {
            std::ofstream out(file, std::ios::binary);
            out << "denied";
        }
    };
    verbs_[L"assert-elevation-offered"] = [query](const std::wstring&) {
        if (query("elevation")["offers"].i64() == 0) throw AssertionError("expected an administrator retry to be offered; none was.");
    };
    verbs_[L"assert-no-elevation-offered"] = [query](const std::wstring&) {
        const auto e = query("elevation");
        if (e["offers"].i64() > 0) {
            throw AssertionError("an administrator retry was offered for: " + e["last"]["operation"].str() + " (" + std::to_string(e["last"]["items"].size()) + " item(s))");
        }
    };

    // ---- the right-click menus, as the Context menu page would have saved them ----
    auto built_in_ids = [query]() {
        std::vector<std::wstring> ids;
        const auto page = query("context-menu");
        for (size_t i = 0; i < page["builtIns"].size(); ++i) ids.push_back(core::wide(page["builtIns"][i]["id"].str()));
        return ids;
    };
    auto menu_item_setting = [this, query, built_in_ids](const std::wstring& r, bool shown) {
        const std::wstring id = require(r, shown ? "show-menu-item" : "hide-menu-item");
        const auto ids = built_in_ids();
        bool known = false;
        std::wstring all;
        for (const auto& i : ids) {
            known = known || iequals(i, id);
            all += (all.empty() ? L"" : L", ") + i;
        }
        if (!known) throw FormatError("'" + to_utf8(id) + "' is not one of the app's menu entries. Try: " + to_utf8(all) + ".");
        const auto page = query("context-menu");
        std::vector<std::wstring> hidden;
        for (size_t i = 0; i < page["hiddenBuiltIns"].size(); ++i) {
            const std::wstring h = core::wide(page["hiddenBuiltIns"][i].str());
            if (!iequals(h, id)) hidden.push_back(h);
        }
        if (!shown) hidden.push_back(id);
        core::check(bb_settings_set(s_.core().app(), core::s(L"hiddenBuiltInMenuItems"), core::s(core::wide(ops::qa(hidden)))), "menu item");
    };
    verbs_[L"hide-menu-item"] = [menu_item_setting](const std::wstring& r) { menu_item_setting(r, false); };
    verbs_[L"show-menu-item"] = [menu_item_setting](const std::wstring& r) { menu_item_setting(r, true); };
    verbs_[L"menu-layout"] = [this, built_in_ids](const std::wstring& r) {
        const std::vector<std::wstring> tokens = [&] {
            std::vector<std::wstring> out;
            for (const auto& t : bb::core::split(require(r, "menu-layout"), L' ')) {
                if (!t.empty()) out.push_back(t);
            }
            return out;
        }();
        const auto ids = built_in_ids();
        for (const auto& t : tokens) {
            if (t.rfind(L"app:", 0) != 0) continue;
            bool known = false;
            for (const auto& i : ids) known = known || iequals(i, t.substr(4));
            if (!known) throw FormatError("'" + to_utf8(t.substr(4)) + "' is not one of the app's menu entries.");
        }
        const bool reset = tokens.size() == 1 && tokens[0] == L"default";
        core::check(bb_settings_set(s_.core().app(), core::s(L"contextMenuLayout"), core::s(reset ? std::wstring(L"null") : core::wide(ops::qa(tokens)))), "menu-layout");
    };

    // A native-only aid: what this machine's registry says other programs add to the menu. Reads the
    // registry only - no extension is loaded and nothing is invoked - which is how the product's source is
    // checked without a scripted run ever hosting foreign code.
    verbs_[L"shell-catalog-real"] = [this](const std::wstring&) {
        auto source = bb::views::make_registry_shell_source(s_.view());
        std::string names;
        const auto catalog = source->catalog();
        for (const auto& e : catalog) names += (names.empty() ? "" : ", ") + e.name;
        say("# the registry lists " + std::to_string(catalog.size()) + " extension(s): " + names);
    };

    // ---- the history ----
    auto step_to = [this](const std::wstring& r, int dir, const char* verb) {
        const int steps = number(trim(r), verb);
        if (steps < 1) throw FormatError("'" + to_utf8(r) + "' is not a number of steps.");
        undo_step(dir * steps);
    };
    verbs_[L"undo-to"] = [step_to](const std::wstring& r) { step_to(r, 1, "undo-to"); };
    verbs_[L"redo-to"] = [step_to](const std::wstring& r) { step_to(r, -1, "redo-to"); };
    verbs_[L"undo-budget"] = [this, run, query](const std::wstring& r) {
        std::string arg = "{";
        bool first = true;
        for (const std::wstring& part : bb::core::split(r, L' ')) {
            if (part.empty()) continue;
            const size_t at = part.find(L'=');
            if (at == std::wstring::npos) throw FormatError("'" + to_utf8(part) + "' is not key=value.");
            const std::wstring key = part.substr(0, at);
            const long long n = _wtoi64(part.c_str() + at + 1);
            std::string field;
            long long value = n;
            if (key == L"steps") field = "steps";
            else if (key == L"gb") { field = "bytes"; value = n << 30; }
            else if (key == L"bytes") field = "bytes";
            else throw FormatError("'" + to_utf8(key) + "' is not an undo limit. Try: steps, gb, bytes.");
            arg += std::string(first ? "" : ",") + "\"" + field + "\":" + std::to_string(value);
            first = false;
        }
        run("undo-budget", arg + "}");
        s_.settle();
    };
    verbs_[L"assert-history"] = [this, query](const std::wstring& r) {
        const auto parts = bb::core::split(trim(r), L' ');
        if (parts.empty() || parts[0].empty()) throw FormatError("Try: assert-history 3 cursor=2 released=0");
        s_.settle();
        const auto h = query("history");
        auto check = [](const char* what, const std::wstring& expected, int64_t actual) {
            if (_wtoi64(expected.c_str()) != actual) throw AssertionError(std::string("expected ") + what + " " + to_utf8(expected) + ", but it is " + std::to_string(actual) + ".");
        };
        check("the history to hold", parts[0], h["count"].i64());
        for (size_t i = 1; i < parts.size(); ++i) {
            const size_t at = parts[i].find(L'=');
            const std::wstring key = parts[i].substr(0, at), value = at == std::wstring::npos ? L"" : parts[i].substr(at + 1);
            if (key == L"cursor") check("the cursor at", value, h["cursor"].i64());
            else if (key == L"released") check("released entries", value, h["released"].i64());
            else throw FormatError("'" + to_utf8(parts[i]) + "' is not cursor=N or released=N.");
        }
    };
    verbs_[L"assert-history-state"] = [this, query](const std::wstring& r) {
        const auto [position, tail] = split_first(r);
        const int n = number(position, "assert-history-state");
        const std::string expected = to_utf8(lower(trim(tail)));
        s_.settle();
        const auto rows = query("history")["rows"];
        if (n < 1 || static_cast<size_t>(n) > rows.size()) throw AssertionError("the history holds fewer than " + std::to_string(n) + " entries.");
        const auto& row = rows[static_cast<size_t>(n) - 1];
        const std::string badge = row["badge"].str();
        std::string actual = row["isDone"].boolean() ? "done" : "undone";
        if (badge.find("failed") != std::string::npos) actual = "failed";
        else if (badge.find("Partly") != std::string::npos) actual = "partial";
        if (actual != expected) throw AssertionError("expected entry " + std::to_string(n) + " to be " + expected + ", but it is " + actual + ".");
    };
}

}  // namespace bbh
