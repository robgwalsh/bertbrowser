#include "views/folder_tree.h"

#include <windows.h>
#include <shlwapi.h>

#include <algorithm>

namespace bb::views {

FolderTreeProvider::FolderTreeProvider(ShellIcons* icons) : icons_(icons) {
    nodes_[0] = Node{};
    next_ = 1;
}

bool FolderTreeProvider::set_show_hidden(bool on) {
    if (on == show_hidden_) return false;
    show_hidden_ = on;
    for (auto& kv : nodes_) {
        if (kv.second.loaded) filter(kv.second);
    }
    return true;
}

void FolderTreeProvider::filter(Node& n) const {
    n.shown.clear();
    for (const uint64_t k : n.kids) {
        if (show_hidden_ || !(nodes_[k].attributes & FILE_ATTRIBUTE_HIDDEN)) n.shown.push_back(k);
    }
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
        nodes_[0].shown = nodes_[0].kids;
        return;
    }
    struct Found {
        std::wstring label, path;
        uint32_t attributes;
    };
    std::vector<Found> found;
    WIN32_FIND_DATAW fd{};
    std::wstring base = nodes_[node].path;
    if (base.empty() || base.back() != L'\\') base += L'\\';
    HANDLE h = FindFirstFileExW((base + L"*").c_str(), FindExInfoBasic, &fd, FindExSearchLimitToDirectories, nullptr,
                                FIND_FIRST_EX_LARGE_FETCH);
    if (h != INVALID_HANDLE_VALUE) {
        do {
            if (!(fd.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY)) continue;
            if (fd.cFileName[0] == L'.' && (fd.cFileName[1] == 0 || (fd.cFileName[1] == L'.' && fd.cFileName[2] == 0))) continue;
            found.push_back({fd.cFileName, base + fd.cFileName, fd.dwFileAttributes});
        } while (FindNextFileW(h, &fd));
        FindClose(h);
    }
    std::sort(found.begin(), found.end(),
              [](const Found& a, const Found& b) { return StrCmpLogicalW(a.label.c_str(), b.label.c_str()) < 0; });
    for (auto& f : found) {
        const uint64_t id = add(Node{f.label, f.path, false, {}, f.attributes});
        nodes_[node].kids.push_back(id);
    }
    filter(nodes_[node]);
}

size_t FolderTreeProvider::child_count(uint64_t node) const {
    load(node);
    return nodes_[node].shown.size();
}

uint64_t FolderTreeProvider::child_id(uint64_t node, size_t index) const {
    load(node);
    const auto& shown = nodes_[node].shown;
    return index < shown.size() ? shown[index] : 0;
}

std::wstring FolderTreeProvider::label(uint64_t node) const {
    const auto it = nodes_.find(node);
    return it == nodes_.end() ? std::wstring() : it->second.label;
}

bool FolderTreeProvider::has_children(uint64_t node) const {
    const auto it = nodes_.find(node);
    if (it == nodes_.end()) return false;
    // Unloaded means "maybe": a chevron now, the truth when it is opened.
    return it->second.loaded ? !it->second.shown.empty() : true;
}

ui::ThumbPtr FolderTreeProvider::icon(uint64_t node, uint32_t px) const {
    const auto it = nodes_.find(node);
    if (!icons_ || it == nodes_.end()) return nullptr;
    const Node& n = it->second;
    // A drive's path is its root ("C:\"); everything under it is a folder.
    if (n.path.size() == 3) return icons_->drive(n.path, px);
    return icons_->file(n.path, true, n.attributes, px);
}

bool FolderTreeProvider::dimmed(uint64_t node) const {
    const auto it = nodes_.find(node);
    return it != nodes_.end() && (it->second.attributes & FILE_ATTRIBUTE_HIDDEN) != 0;
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
        for (const uint64_t k : nodes_[cur].shown) {
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
