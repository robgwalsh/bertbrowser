#include "views/folder_tree.h"

#include <windows.h>
#include <shlwapi.h>

#include <algorithm>

namespace bb::views {

FolderTreeProvider::FolderTreeProvider() {
    nodes_[0] = Node{};
    next_ = 1;
}

uint64_t FolderTreeProvider::add(Node n) const {
    const uint64_t id = next_++;
    nodes_[id] = std::move(n);
    return id;
}

void FolderTreeProvider::load(uint64_t node) const {
    Node& n = nodes_[node];
    if (n.loaded) return;
    n.loaded = true;
    if (node == 0) {
        const DWORD drives = GetLogicalDrives();
        for (int i = 0; i < 26; ++i) {
            if (!(drives & (1u << i))) continue;
            const std::wstring root = std::wstring(1, static_cast<wchar_t>(L'A' + i)) + L":\\";
            const uint64_t id = add(Node{root.substr(0, 2), root, false, {}});
            nodes_[0].kids.push_back(id);
        }
        return;
    }
    std::vector<std::pair<std::wstring, std::wstring>> found;  // label, path
    WIN32_FIND_DATAW fd{};
    std::wstring base = nodes_[node].path;
    if (base.empty() || base.back() != L'\\') base += L'\\';
    HANDLE h = FindFirstFileExW((base + L"*").c_str(), FindExInfoBasic, &fd, FindExSearchLimitToDirectories, nullptr,
                                FIND_FIRST_EX_LARGE_FETCH);
    if (h != INVALID_HANDLE_VALUE) {
        do {
            if (!(fd.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY)) continue;
            if (fd.cFileName[0] == L'.' && (fd.cFileName[1] == 0 || (fd.cFileName[1] == L'.' && fd.cFileName[2] == 0))) continue;
            if (!show_hidden && (fd.dwFileAttributes & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM))) continue;
            found.emplace_back(fd.cFileName, base + fd.cFileName);
        } while (FindNextFileW(h, &fd));
        FindClose(h);
    }
    std::sort(found.begin(), found.end(),
              [](const auto& a, const auto& b) { return StrCmpLogicalW(a.first.c_str(), b.first.c_str()) < 0; });
    for (auto& f : found) {
        const uint64_t id = add(Node{f.first, f.second, false, {}});
        nodes_[node].kids.push_back(id);
    }
}

size_t FolderTreeProvider::child_count(uint64_t node) const {
    load(node);
    return nodes_[node].kids.size();
}

uint64_t FolderTreeProvider::child_id(uint64_t node, size_t index) const {
    load(node);
    const auto& kids = nodes_[node].kids;
    return index < kids.size() ? kids[index] : 0;
}

std::wstring FolderTreeProvider::label(uint64_t node) const {
    const auto it = nodes_.find(node);
    return it == nodes_.end() ? std::wstring() : it->second.label;
}

bool FolderTreeProvider::has_children(uint64_t node) const {
    const auto it = nodes_.find(node);
    if (it == nodes_.end()) return false;
    // Unloaded means "maybe": a chevron now, the truth when it is opened.
    return it->second.loaded ? !it->second.kids.empty() : true;
}

std::wstring FolderTreeProvider::path_of(uint64_t node) const {
    const auto it = nodes_.find(node);
    return it == nodes_.end() ? std::wstring() : it->second.path;
}

std::vector<uint64_t> FolderTreeProvider::chain_to(const std::wstring& path) const {
    std::vector<uint64_t> chain;
    if (path.size() < 2 || path[1] != L':') return chain;
    load(0);
    uint64_t cur = 0;
    for (const uint64_t d : nodes_[0].kids) {
        if (_wcsnicmp(nodes_[d].path.c_str(), path.c_str(), 2) == 0) {
            cur = d;
            break;
        }
    }
    if (!cur) return chain;
    chain.push_back(cur);
    size_t pos = path.find(L'\\');
    while (pos != std::wstring::npos && pos + 1 < path.size()) {
        const size_t next = path.find(L'\\', pos + 1);
        const std::wstring part = path.substr(pos + 1, next == std::wstring::npos ? std::wstring::npos : next - pos - 1);
        pos = next;
        if (part.empty()) continue;
        load(cur);
        uint64_t found = 0;
        for (const uint64_t k : nodes_[cur].kids) {
            if (_wcsicmp(nodes_[k].label.c_str(), part.c_str()) == 0) {
                found = k;
                break;
            }
        }
        if (!found) break;  // hidden or gone: the chain stops at the nearest folder that is showing
        chain.push_back(found);
        cur = found;
    }
    return chain;
}

}  // namespace bb::views
