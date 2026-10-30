#include "views/folder_tree.h"

#include <windows.h>
#include <shlwapi.h>
#include <shellapi.h>

#include <algorithm>
#include <chrono>
#include <condition_variable>
#include <memory>
#include <mutex>
#include <thread>

namespace bb::views {

namespace {

struct DriveProbe {
    bool ready = false;
    std::wstring label;
    int64_t used = -1;
};

// Whether a drive is ready, its label and its used space. Asked on a thread of its own with a short wait:
// an optical drive with nothing in it says so at once, but a network share that has gone to sleep can take
// seconds, and the tree is built on the window's thread. One that has not answered is taken as ready and
// listed by its letter alone.
DriveProbe probe_drive(const std::wstring& root) {
    struct Shared {
        std::mutex lock;
        std::condition_variable done;
        bool finished = false;
        DriveProbe result;
    };
    auto shared = std::make_shared<Shared>();
    std::thread([shared, root] {
        DriveProbe p;
        wchar_t label[MAX_PATH + 1] = {};
        if (GetVolumeInformationW(root.c_str(), label, MAX_PATH + 1, nullptr, nullptr, nullptr, nullptr, 0)) {
            p.ready = true;
            p.label = label;
            ULARGE_INTEGER total{}, free{};
            if (GetDiskFreeSpaceExW(root.c_str(), nullptr, &total, &free)) p.used = static_cast<int64_t>(total.QuadPart - free.QuadPart);
        }
        std::lock_guard<std::mutex> g(shared->lock);
        shared->result = std::move(p);
        shared->finished = true;
        shared->done.notify_all();
    }).detach();
    std::unique_lock<std::mutex> g(shared->lock);
    if (!shared->done.wait_for(g, std::chrono::milliseconds(400), [&] { return shared->finished; })) {
        DriveProbe slow;
        slow.ready = true;
        return slow;
    }
    return shared->result;
}

// Whether `dir` holds a sub-folder, and whether one that is not hidden: stops at the first visible one.
uint8_t probe_subfolders(const std::wstring& dir) {
    std::wstring base = dir;
    if (base.empty() || base.back() != L'\\') base += L'\\';
    WIN32_FIND_DATAW fd{};
    HANDLE h = FindFirstFileExW((base + L"*").c_str(), FindExInfoBasic, &fd, FindExSearchLimitToDirectories, nullptr, 0);
    if (h == INVALID_HANDLE_VALUE) return 1;
    uint8_t sub = 1;
    do {
        if (!(fd.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY)) continue;
        if (fd.cFileName[0] == L'.' && (fd.cFileName[1] == 0 || (fd.cFileName[1] == L'.' && fd.cFileName[2] == 0))) continue;
        if (fd.dwFileAttributes & FILE_ATTRIBUTE_HIDDEN) {
            sub = 2;
        } else {
            sub = 3;
            break;
        }
    } while (FindNextFileW(h, &fd));
    FindClose(h);
    return sub;
}

}  // namespace

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
        // Ready drives only, named as Explorer names them ("Windows (C:)"), with the space they use. A drive
        // that does not answer in time (a sleeping network share) is listed by its letter rather than holding
        // the window up. Spec: FolderTreeViewModel.LoadDrivesAsync, UsedBytes.
        const DWORD drives = GetLogicalDrives();
        for (int i = 0; i < 26; ++i) {
            if (!(drives & (1u << i))) continue;
            const std::wstring root = std::wstring(1, static_cast<wchar_t>(L'A' + i)) + L":\\";
            const DriveProbe probe = probe_drive(root);
            if (!probe.ready) continue;
            Node drive{probe.label.empty() ? root.substr(0, 2) : probe.label + L" (" + root.substr(0, 2) + L")", root, false, {}};
            if (probe.used >= 0 && format_bytes) drive.size = format_bytes(probe.used);
            const uint64_t id = add(std::move(drive));
            nodes_[0].kids.push_back(id);
        }
        nodes_[0].shown = nodes_[0].kids;
        return;
    }
    if (!sync_) {
        // Opened by hand: read on a worker; the chevron stays until the answer lands.
        n.loaded = false;
        if (n.loading) return;
        n.loading = true;
        Result job;
        job.node = node;
        job.listing = true;
        start(std::move(job), {n.path});
        return;
    }
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
        Node kid;
        kid.label = f.label;
        kid.path = f.path;
        kid.attributes = f.attributes;
        const uint64_t id = add(std::move(kid));
        nodes_[node].kids.push_back(id);
    }
    fill_sizes(nodes_[node].kids);
    filter(nodes_[node]);
    probe_kids(node);
}

void FolderTreeProvider::probe_kids(uint64_t node) const {
    Result job;
    job.node = node;
    std::vector<std::wstring> paths;
    for (const uint64_t k : nodes_[node].kids) {
        if (nodes_[k].sub != 0 || nodes_[k].loaded) continue;
        job.probes.push_back({k, uint8_t{0}});
        paths.push_back(nodes_[k].path);
    }
    if (!paths.empty()) start(std::move(job), std::move(paths));
}

// Reads on a thread of its own and leaves the answer in the inbox, which outlives this provider if it has to.
void FolderTreeProvider::start(Result job, std::vector<std::wstring> paths) const {
    std::shared_ptr<Inbox> inbox = inbox_;
    {
        std::lock_guard<std::mutex> g(inbox->lock);
        ++inbox->pending;
    }
    std::function<void()> told = on_worker_done;
    std::thread([inbox, told, job = std::move(job), paths = std::move(paths)]() mutable {
        if (job.listing) {
            std::wstring base = paths.front();
            if (base.empty() || base.back() != L'\\') base += L'\\';
            WIN32_FIND_DATAW fd{};
            HANDLE h = FindFirstFileExW((base + L"*").c_str(), FindExInfoBasic, &fd, FindExSearchLimitToDirectories, nullptr,
                                        FIND_FIRST_EX_LARGE_FETCH);
            if (h != INVALID_HANDLE_VALUE) {
                do {
                    if (!(fd.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY)) continue;
                    if (fd.cFileName[0] == L'.' && (fd.cFileName[1] == 0 || (fd.cFileName[1] == L'.' && fd.cFileName[2] == 0))) continue;
                    job.found.push_back({fd.cFileName, base + fd.cFileName, fd.dwFileAttributes});
                } while (FindNextFileW(h, &fd));
                FindClose(h);
            }
            std::sort(job.found.begin(), job.found.end(),
                      [](const Found& a, const Found& b) { return StrCmpLogicalW(a.label.c_str(), b.label.c_str()) < 0; });
            for (Found& f : job.found) f.sub = probe_subfolders(f.path);
        } else {
            for (size_t i = 0; i < job.probes.size() && i < paths.size(); ++i) job.probes[i].second = probe_subfolders(paths[i]);
        }
        {
            std::lock_guard<std::mutex> g(inbox->lock);
            inbox->done.push_back(std::move(job));
            --inbox->pending;
        }
        if (told) told();
    }).detach();
}

bool FolderTreeProvider::set_devices(const std::vector<std::pair<std::wstring, std::wstring>>& devices) {
    load(0);
    std::vector<uint64_t>& kids = nodes_[0].kids;
    std::vector<std::pair<std::wstring, std::wstring>> had;
    for (const uint64_t k : kids) {
        if (nodes_[k].device) had.push_back({nodes_[k].label, nodes_[k].path});
    }
    if (had == devices) return false;
    for (auto k = kids.begin(); k != kids.end();) {
        if (nodes_[*k].device) {
            nodes_.erase(*k);
            k = kids.erase(k);
        } else {
            ++k;
        }
    }
    for (const auto& [name, path] : devices) {
        Node d;
        d.label = name;
        d.path = path;
        d.device = true;
        d.loaded = true;
        kids.push_back(add(std::move(d)));
    }
    nodes_[0].shown = kids;
    return true;
}

bool FolderTreeProvider::is_device(uint64_t node) const {
    const auto it = nodes_.find(node);
    return it != nodes_.end() && it->second.device;
}

size_t FolderTreeProvider::pending() const {
    std::lock_guard<std::mutex> g(inbox_->lock);
    return inbox_->pending + inbox_->done.size();
}

bool FolderTreeProvider::drain() {
    std::vector<Result> done;
    {
        std::lock_guard<std::mutex> g(inbox_->lock);
        done.swap(inbox_->done);
    }
    bool changed = false;
    for (Result& r : done) {
        const auto it = nodes_.find(r.node);
        if (it == nodes_.end()) continue;
        if (!r.listing) {
            for (const auto& [id, sub] : r.probes) {
                if (const auto k = nodes_.find(id); k != nodes_.end() && k->second.sub != sub) {
                    k->second.sub = sub;
                    changed = true;
                }
            }
            continue;
        }
        it->second.loading = false;
        if (it->second.loaded) continue;  // read in place meanwhile (a reveal got there first)
        it->second.loaded = true;
        for (Found& f : r.found) {
            Node kid;
            kid.label = std::move(f.label);
            kid.path = std::move(f.path);
            kid.attributes = f.attributes;
            kid.sub = f.sub;
            const uint64_t id = add(std::move(kid));
            nodes_[r.node].kids.push_back(id);
        }
        fill_sizes(nodes_[r.node].kids);
        filter(nodes_[r.node]);
        changed = true;
    }
    return changed;
}

void FolderTreeProvider::fill_sizes(const std::vector<uint64_t>& ids) const {
    if (!size_source || ids.empty()) return;
    std::vector<std::wstring> paths;
    paths.reserve(ids.size());
    for (const uint64_t id : ids) paths.push_back(nodes_[id].path);
    const std::vector<std::wstring> sizes = size_source(paths);
    for (size_t i = 0; i < ids.size() && i < sizes.size(); ++i) nodes_[ids[i]].size = sizes[i];
}

void FolderTreeProvider::refresh_sizes() {
    std::vector<uint64_t> folders;
    for (auto& [id, n] : nodes_) {
        if (id == 0) continue;
        if (n.path.size() == 3) {
            // A drive: its used space, read again.
            const DriveProbe probe = probe_drive(n.path);
            if (probe.used >= 0 && format_bytes) n.size = format_bytes(probe.used);
        } else {
            folders.push_back(id);
        }
    }
    fill_sizes(folders);
}

std::vector<uint64_t> FolderTreeProvider::loaded_ids() const {
    std::vector<uint64_t> out;
    for (const auto& [id, n] : nodes_) {
        if (id != 0 && n.loaded) out.push_back(id);
    }
    return out;
}

std::wstring FolderTreeProvider::detail(uint64_t node) const {
    const auto it = nodes_.find(node);
    return it == nodes_.end() ? std::wstring() : it->second.size;
}

bool FolderTreeProvider::reload(uint64_t node) {
    auto it = nodes_.find(node);
    if (it == nodes_.end() || !it->second.loaded) return false;
    // What the node had, by path, so a folder still there keeps its id.
    std::unordered_map<std::wstring, uint64_t> had;
    for (const uint64_t kid : it->second.kids) {
        std::wstring key = nodes_[kid].path;
        CharUpperBuffW(key.data(), static_cast<DWORD>(key.size()));
        had[key] = kid;
    }
    const std::vector<uint64_t> before = it->second.kids;
    it->second.loaded = false;
    it->second.kids.clear();
    const bool was = sync_;
    sync_ = true;
    load(node);
    sync_ = was;
    // load() made fresh nodes for everything; put the old ids back where the folder is the same one.
    Node& n = nodes_[node];
    for (uint64_t& kid : n.kids) {
        std::wstring key = nodes_[kid].path;
        CharUpperBuffW(key.data(), static_cast<DWORD>(key.size()));
        const auto old = had.find(key);
        if (old == had.end()) continue;
        nodes_[old->second].label = nodes_[kid].label;
        nodes_[old->second].attributes = nodes_[kid].attributes;
        nodes_.erase(kid);
        kid = old->second;
    }
    filter(n);
    return n.kids != before;
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
    // Unloaded and unprobed means "maybe": a chevron until the probe or the opening says otherwise.
    const Node& n = it->second;
    if (n.loaded) return !n.shown.empty();
    return n.sub == 0 || n.sub == 3 || (show_hidden_ && n.sub == 2);
}

ui::ThumbPtr FolderTreeProvider::icon(uint64_t node, uint32_t px) const {
    const auto it = nodes_.find(node);
    if (!icons_ || it == nodes_.end()) return nullptr;
    const Node& n = it->second;
    // A drive's path is its root ("C:\"); everything under it is a folder. A device has no file to ask about.
    if (n.device) return icons_->stock(SIID_DESKTOPPC, px);
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
    // Each level is read in place: the reveal needs the rows now. Their sub-folders are probed on a worker.
    struct InPlace {
        bool& flag;
        bool was;
        explicit InPlace(bool& f) : flag(f), was(f) { flag = true; }
        ~InPlace() { flag = was; }
    } in_place(sync_);
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
