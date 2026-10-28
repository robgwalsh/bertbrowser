// The tool windows' verbs: checksums (what the window computed, a checksum file written by the app's own
// writer, a verify run), the file compare's verdict, the duplicate finder's assertions and its remove, the
// change timeline's seed, and the folder comparison (its banner's words, the differences-only filter, what it
// refuses and sync). Each goes through the same core calls the windows' buttons do, so a script proves the
// button's path rather than a shorter one.
#include "runner.h"

#include <windows.h>

#include <algorithm>
#include <cwctype>

#include "views/json.h"
#include "views/main_view.h"
#include "views/tool_state.h"

namespace bbh {

namespace core = bb::core;
namespace views = bb::views;
namespace json = bb::json;
namespace fs_ = std::filesystem;

namespace {

std::wstring lower(std::wstring s) {
    for (auto& c : s) c = static_cast<wchar_t>(std::towlower(c));
    return s;
}

std::vector<std::wstring> words(const std::wstring& text) {
    std::vector<std::wstring> out;
    size_t i = 0;
    while (i < text.size()) {
        while (i < text.size() && text[i] == L' ') ++i;
        const size_t start = i;
        while (i < text.size() && text[i] != L' ') ++i;
        if (i > start) out.push_back(text.substr(start, i - start));
    }
    return out;
}

}  // namespace

void Runner::install_tools() {
    auto call = [this](const char* verb, const std::string& args = {}) {
        try {
            return views::tool_call(s_.view(), verb, args);
        } catch (const core::CoreError& e) {
            throw AssertionError(e.what());
        }
    };

    // ---- checksums ----
    // checksum-algorithms <name>,<name>... -- the window starts with these ticked, as it would from Settings.
    verbs_[L"checksum-algorithms"] = [this](const std::wstring& r) {
        const std::wstring list = require(r, "checksum-algorithms");
        core::check(bb_settings_set(s_.core().app(), core::s(std::wstring(L"checksumAlgorithms")), core::s(L"\"" + list + L"\"")), "checksum-algorithms");
    };
    // checksum-file <path> <algorithm> <name>,<name>... -- a real checksum file over real files, written through
    // the same ChecksumFile::render the window's Save goes through.
    verbs_[L"checksum-file"] = [this, call](const std::wstring& r) {
        const std::vector<std::wstring> parts = words(r);
        if (parts.size() != 3) throw FormatError("checksum-file wants <path> <algorithm> <name>,<name>...");
        std::vector<std::wstring> files;
        for (const std::wstring& n : csv(parts[2])) files.push_back(sandbox_.resolve(n).wstring());
        call("checksum.write-file",
             "{\"path\":" + views::json_quote(sandbox_.resolve(parts[0]).wstring()) + ",\"algorithm\":" + views::json_quote(parts[1]) +
                 ",\"files\":" + views::json_array(files) + "}");
    };
    // assert-digest <name> <algorithm> <digest> -- what the window actually computed. The fixture is deterministic
    // (a sandbox file holds its own name repeated to length), so a change to a digest means the hashing changed;
    // a picture of a digest proves only that something was rendered.
    verbs_[L"assert-digest"] = [this](const std::wstring& r) {
        const std::vector<std::wstring> parts = words(require(r, "assert-digest"));
        if (parts.size() != 3) throw FormatError("assert-digest wants <name> <algorithm> <digest>.");
        json::Value m;
        try {
            m = views::tool_call(s_.view(), "checksum.model");
        } catch (const core::CoreError&) {
            throw AssertionError("There is no checksum window. Run 'dialog checksum' first.");
        }
        const std::string name = to_utf8(parts[0]);
        std::string have;
        for (size_t i = 0; i < m["rows"].size(); ++i) {
            const json::Value& row = m["rows"][i];
            if (lower(core::wide(row["name"].str())) != lower(parts[0])) continue;
            const json::Value& digest = row["computed"][to_utf8(parts[1])];
            if (digest.is_null()) throw AssertionError("No " + to_utf8(parts[1]) + " digest was computed for '" + name + "'.");
            const std::wstring actual = core::wide(digest.str());
            if (lower(actual) != lower(parts[2])) {
                throw AssertionError("'" + name + "' " + to_utf8(parts[1]) + " is " + to_utf8(lower(actual)) + ", not " + to_utf8(lower(parts[2])) + ".");
            }
            return;
        }
        for (size_t i = 0; i < m["rows"].size(); ++i) have += (i ? ", " : "") + m["rows"][i]["name"].str();
        throw AssertionError("The checksum window holds no row named '" + name + "'. It holds: " + have);
    };
    // assert-verify <name> <state> -- how one row of a verify run came out.
    verbs_[L"assert-verify"] = [this](const std::wstring& r) {
        const std::vector<std::wstring> parts = words(require(r, "assert-verify"));
        if (parts.size() != 2) throw FormatError("assert-verify wants <name> <state>.");
        json::Value m;
        try {
            m = views::tool_call(s_.view(), "checksum.model");
        } catch (const core::CoreError&) {
            throw AssertionError("There is no checksum window. Run 'dialog checksum-verify' first.");
        }
        std::string have;
        for (size_t i = 0; i < m["rows"].size(); ++i) {
            const json::Value& row = m["rows"][i];
            have += (i ? ", " : "") + row["name"].str();
            if (lower(core::wide(row["name"].str())) != lower(parts[0])) continue;
            const std::string state = row["verifyState"].is_null() ? "nothing" : row["verifyState"].str();
            if (lower(core::wide(state)) != lower(parts[1])) {
                throw AssertionError("'" + to_utf8(parts[0]) + "' verified as " + state + ", not " + to_utf8(parts[1]) + ".");
            }
            return;
        }
        throw AssertionError("The verify report holds no row named '" + to_utf8(parts[0]) + "'. It holds: " + have);
    };

    // ---- comparing two files ----
    // assert-compare-files <substring> -- the verdict strip's words.
    verbs_[L"assert-compare-files"] = [this](const std::wstring& r) {
        const std::wstring expected = require(r, "assert-compare-files");
        json::Value m;
        try {
            m = views::tool_call(s_.view(), "filecompare.model");
        } catch (const core::CoreError&) {
            throw AssertionError("Nothing has been compared. Run 'dialog compare-files' first.");
        }
        const std::wstring verdict = core::wide(m["verdict"].str());
        std::wstring want = expected;
        if (want.size() > 1 && want.front() == L'"' && want.back() == L'"') want = want.substr(1, want.size() - 2);
        if (!core::icontains(verdict, want)) {
            throw AssertionError("The verdict is '" + to_utf8(verdict) + "', which does not hold '" + to_utf8(want) + "'.");
        }
    };

    // ---- the duplicate finder ----
    auto groups_of = [this]() -> const json::Value& {
        auto& d = s_.view().duplicates();
        if (!d.has) throw AssertionError("Run 'duplicates' first.");
        return d.result["groups"];
    };
    verbs_[L"assert-duplicate-groups"] = [this, groups_of](const std::wstring& r) {
        const int expected = number(r, "assert-duplicate-groups");
        const size_t actual = groups_of().size();
        if (actual != static_cast<size_t>(expected)) {
            throw AssertionError("Expected " + std::to_string(expected) + " duplicate groups, found " + std::to_string(actual) + ".");
        }
    };
    verbs_[L"assert-duplicate-selected"] = [this, groups_of](const std::wstring& r) {
        const int expected = number(r, "assert-duplicate-selected");
        groups_of();
        const size_t actual = s_.view().duplicates().ticked.size();
        if (actual != static_cast<size_t>(expected)) {
            throw AssertionError("Expected " + std::to_string(expected) + " copies ticked, found " + std::to_string(actual) + ".");
        }
    };
    auto has_row = [this, groups_of](const std::wstring& name) {
        const json::Value& groups = groups_of();
        for (size_t g = 0; g < groups.size(); ++g) {
            for (size_t i = 0; i < groups[g]["files"].size(); ++i) {
                if (lower(core::wide(groups[g]["files"][i]["name"].str())) == lower(name)) return true;
            }
        }
        return false;
    };
    verbs_[L"assert-duplicate-row"] = [this, has_row](const std::wstring& r) {
        const std::wstring name = require(r, "assert-duplicate-row");
        if (!has_row(name)) throw AssertionError("No duplicate named '" + to_utf8(name) + "'.");
    };
    verbs_[L"assert-no-duplicate-row"] = [this, has_row](const std::wstring& r) {
        const std::wstring name = require(r, "assert-no-duplicate-row");
        if (has_row(name)) throw AssertionError("'" + to_utf8(name) + "' was reported as a duplicate, and should not have been.");
    };
    // duplicates-remove -- deletes the ticked copies through the ordinary reversible delete.
    verbs_[L"duplicates-remove"] = [this, groups_of](const std::wstring&) {
        groups_of();
        uint64_t op = 0;
        bool started = false;
        try {
            started = views::duplicates_remove(s_.view(), &op);
        } catch (const core::CoreError& e) {
            throw AssertionError(e.what());
        }
        if (!started) throw AssertionError("Nothing is ticked. Run 'duplicates-keep' first.");
        await_op(op);
    };

    // ---- the change timeline ----
    // changes-seed -- a dozen recorded changes under the sandbox, written through the repository the index helper
    // writes through, and recording turned on so the window will show them.
    verbs_[L"changes-seed"] = [this, call](const std::wstring&) {
        const json::Value r = call("changes.seed", "{\"root\":" + views::json_quote(sandbox_.root().wstring()) + "}");
        say("CHANGES " + std::to_string(static_cast<int64_t>(r["rows"].num())));
    };

    // ---- comparing two folders ----
    verbs_[L"compare-filter"] = [this, call](const std::wstring& r) {
        const bool on = on_off(r, "compare-filter");
        call("compare.filter", on ? "{\"on\":true}" : "{\"on\":false}");
        s_.settle();
    };
    // assert-compare <text> -- the banner's summary, where every count a comparison found is written out.
    verbs_[L"assert-compare"] = [this, call](const std::wstring& r) {
        const std::wstring expected = require(r, "assert-compare");
        const json::Value m = call("compare.model");
        if (!m["active"].boolean()) throw AssertionError("There is no comparison running. Run 'compare' before 'assert-compare'.");
        const std::wstring summary = core::wide(m["summary"].str());
        if (!core::icontains(summary, expected)) {
            throw AssertionError("The comparison says '" + to_utf8(summary) + "', not '" + to_utf8(expected) + "'.");
        }
    };
    // compare-refused [text] -- the pair was turned down, and the reason was said out loud.
    verbs_[L"compare-refused"] = [this, call](const std::wstring& r) {
        const BbStatus st = bb_compare_toggle(s_.core().app());
        const std::wstring said = st == BB_OK ? std::wstring() : core::last_error();
        s_.settle();
        const json::Value m = call("compare.model");
        if (st == BB_OK || m["active"].boolean()) {
            if (m["active"].boolean()) call("compare.end");
            throw AssertionError("The comparison was accepted, and should not have been.");
        }
        if (said.empty()) throw AssertionError("Comparing was refused, but the user was told nothing.");
        if (!r.empty() && !core::icontains(said, r)) throw AssertionError("It said '" + to_utf8(said) + "', not '" + to_utf8(r) + "'.");
    };
    // sync [with-deletes] -- runs the sync the comparison on show would do, through the same planner, runner and
    // undo history the dialog's Sync button goes through. `with-deletes` ticks the destructive half, which the
    // dialog leaves off.
    verbs_[L"sync"] = [this, call](const std::wstring& r) {
        const std::wstring arg = lower(r);
        if (!arg.empty() && arg != L"with-deletes") throw FormatError("sync takes 'with-deletes' or nothing, not '" + to_utf8(r) + "'.");
        const json::Value m = call("compare.sync", arg.empty() ? "{\"withDeletes\":false}" : "{\"withDeletes\":true}");
        await_op(static_cast<uint64_t>(m["op"].num()));
        s_.settle();
    };
}

}  // namespace bbh
