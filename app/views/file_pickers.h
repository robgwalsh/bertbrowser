// Windows' own file and folder pickers (IFileOpenDialog / IFileSaveDialog), for the few places that ask
// for a path somewhere else: Save a checksum file, Verify against one, Browse for where an archive goes.
// They are modal over `owner` and block until answered, so a scripted run never calls them.
#pragma once
#include <windows.h>

#include <optional>
#include <string>
#include <utility>
#include <vector>

namespace bb::views {

struct PickPath {
    std::wstring title;
    std::wstring folder;        // where it starts; empty is wherever Windows last was
    std::wstring file_name;     // a save's suggested name
    std::wstring default_ext;   // a save's extension when none is typed, without the dot
    std::vector<std::pair<std::wstring, std::wstring>> filters;  // label, pattern ("*.sha256;*.md5")
    unsigned filter_index = 0;  // which filter starts selected (0-based)
};

std::optional<std::wstring> pick_open_file(HWND owner, const PickPath& p);
std::optional<std::wstring> pick_save_file(HWND owner, const PickPath& p);
std::optional<std::wstring> pick_folder(HWND owner, const PickPath& p);

}  // namespace bb::views
