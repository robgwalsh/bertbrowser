// What the tool windows keep between a dialog being built and the next: which folder the disk-usage window
// is drilled into, which sync rows are unticked. Everything else lives in the core (`bb_tools`).
#pragma once
#include "views/json.h"

#include <set>
#include <string>
#include <vector>

namespace bb::views {

class MainView;

struct ToolsUi {
    // The folder the disk-usage window shows; empty means the active tab's.
    std::wstring disk_root;
    // The sync preview: the box under the list, and the rows the person has unticked.
    bool sync_with_deletes = false;
    std::set<std::string> sync_unticked;
    bool sync_touched = false;
    // The Checksums window's last Save, said under the list.
    std::wstring checksum_note;
    // The file compare shows as text even though the pair does not look like text.
    bool compare_force_text = false;
    // The theme editor: the colour being edited, what the list is filtered to, and what it last said.
    std::string theme_token;
    std::wstring theme_search;
    bool theme_all = false;
    std::wstring theme_status;
};

ToolsUi& tools_ui(MainView& main);

// A JSON string literal (UTF-8) for a wide string, and a JSON array of them.
std::string json_quote(const std::wstring& text);
std::string json_array(const std::vector<std::wstring>& items);

// Runs one tool verb (see bb-app/src/tools.rs) and parses the answer. Throws CoreError when it fails.
json::Value tool_call(MainView& main, const char* verb, const std::string& args = {});
// As tool_call, but a failure is a null value and `error` (when given) says why.
json::Value try_tool_call(MainView& main, const char* verb, const std::string& args = {}, std::wstring* error = nullptr);

// The commands the core leaves to the host that open a tool window (checksum, compare files, disk usage,
// duplicates, the change timeline) or act on the comparison (rescan, differences only, sync). True when
// the command was one of these and was carried out.
bool tool_host_command(MainView& main, const std::wstring& id);

// The copies ticked in the duplicate finder, as paths.
std::vector<std::wstring> duplicates_ticked_paths(MainView& main);
// Removes them through the ordinary reversible delete (the Recycle Bin, the shared undo history) and
// leaves the finder showing what is still duplicated - never every copy of a group. False, with nothing
// done, when none may go; otherwise the operation, which the caller may wait on. A scripted run's way in:
// it stands in for someone who answered the confirmation.
bool duplicates_remove(MainView& main, uint64_t* op);
// The window's Remove ticked: the delete confirmation over the same copies, then back to the finder.
void duplicates_ask_remove(MainView& main);
// An operation finished: the copies a removal was deleting that are now gone leave their groups.
void duplicates_settle_prune(MainView& main);

}  // namespace bb::views
