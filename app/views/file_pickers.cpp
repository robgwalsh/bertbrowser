#include "views/file_pickers.h"

#include <shobjidl.h>

namespace bb::views {

namespace {

std::optional<std::wstring> run(HWND owner, const PickPath& p, bool save, bool folder) {
    IFileDialog* dialog = nullptr;
    const HRESULT made = save ? CoCreateInstance(CLSID_FileSaveDialog, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(reinterpret_cast<IFileSaveDialog**>(&dialog)))
                              : CoCreateInstance(CLSID_FileOpenDialog, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(reinterpret_cast<IFileOpenDialog**>(&dialog)));
    if (FAILED(made) || !dialog) return std::nullopt;
    DWORD options = 0;
    dialog->GetOptions(&options);
    options |= FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST;
    if (folder) options |= FOS_PICKFOLDERS;
    if (save) options |= FOS_OVERWRITEPROMPT;
    else if (!folder) options |= FOS_FILEMUSTEXIST;
    dialog->SetOptions(options);
    if (!p.title.empty()) dialog->SetTitle(p.title.c_str());
    if (!p.file_name.empty()) dialog->SetFileName(p.file_name.c_str());
    if (!p.default_ext.empty()) dialog->SetDefaultExtension(p.default_ext.c_str());
    std::vector<COMDLG_FILTERSPEC> specs;
    for (const auto& [label, pattern] : p.filters) specs.push_back({label.c_str(), pattern.c_str()});
    if (!specs.empty()) {
        dialog->SetFileTypes(static_cast<UINT>(specs.size()), specs.data());
        dialog->SetFileTypeIndex(p.filter_index + 1);  // 1-based
    }
    if (!p.folder.empty()) {
        IShellItem* start = nullptr;
        if (SUCCEEDED(SHCreateItemFromParsingName(p.folder.c_str(), nullptr, IID_PPV_ARGS(&start)))) {
            dialog->SetFolder(start);
            start->Release();
        }
    }
    std::optional<std::wstring> chosen;
    if (SUCCEEDED(dialog->Show(owner))) {
        IShellItem* item = nullptr;
        if (SUCCEEDED(dialog->GetResult(&item))) {
            PWSTR path = nullptr;
            if (SUCCEEDED(item->GetDisplayName(SIGDN_FILESYSPATH, &path))) {
                chosen = path;
                CoTaskMemFree(path);
            }
            item->Release();
        }
    }
    dialog->Release();
    return chosen;
}

}  // namespace

std::optional<std::wstring> pick_open_file(HWND owner, const PickPath& p) { return run(owner, p, false, false); }
std::optional<std::wstring> pick_save_file(HWND owner, const PickPath& p) { return run(owner, p, true, false); }
std::optional<std::wstring> pick_folder(HWND owner, const PickPath& p) { return run(owner, p, false, true); }

}  // namespace bb::views
