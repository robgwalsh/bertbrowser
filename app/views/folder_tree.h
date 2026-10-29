// The folder tree's provider: drives at the top, a folder's sub-folders on demand. The core has no tree
// call (a listing is the only thing it hands out), so this reads directory names itself - names only,
// never sizes, and nothing is enumerated until a node is expanded or revealed.
#pragma once
#include "ui/tree_view.h"
#include "views/shell_icons.h"

#include <functional>
#include <memory>
#include <mutex>
#include <string>
#include <unordered_map>
#include <vector>

namespace bb::views {

class FolderTreeProvider : public ui::TreeProvider {
public:
    explicit FolderTreeProvider(ShellIcons* icons = nullptr);

    size_t child_count(uint64_t node) const override;
    uint64_t child_id(uint64_t node, size_t index) const override;
    std::wstring label(uint64_t node) const override;
    bool has_children(uint64_t node) const override;
    ui::ThumbPtr icon(uint64_t node, uint32_t px) const override;
    bool dimmed(uint64_t node) const override;
    std::wstring detail(uint64_t node) const override;

    // The size beside each folder: the core's cached size text per path, in order ("" when unknown). Asked
    // once per batch of sub-folders as they load, and again by `refresh_sizes`. Unset: no sizes.
    std::function<std::vector<std::wstring>(const std::vector<std::wstring>& paths)> size_source;
    // Bytes in the app's words, for a drive's used space.
    std::function<std::wstring(int64_t bytes)> format_bytes;
    // Every loaded node's size read again (a volume finished indexing, which is when the numbers first exist).
    void refresh_sizes();
    // Folds nothing itself; the ids of every loaded node, for Collapse all.
    std::vector<uint64_t> loaded_ids() const;

    // Folders opened by hand are read on a worker, and every listing's sub-folders are probed there for sub-folders
    // of their own (so a chevron only shows where there is something to open). `on_worker_done` runs on that worker
    // and must only post; the UI thread then calls `drain` and, when it says so, refreshes the tree.
    // Spec: DirectoryNodeViewModel.PopulateAsync.
    std::function<void()> on_worker_done;
    bool drain();
    size_t pending() const;

    // Phones, cameras and players under "This PC" (name, shell path), listed after the drives. They are leaves:
    // their contents are no path the list can read. True when the rows changed. Spec: FolderTreeViewModel
    // (the portable devices), PortableDeviceNodeViewModel.
    bool set_devices(const std::vector<std::pair<std::wstring, std::wstring>>& devices);
    bool is_device(uint64_t node) const;
    // A folder's full path; a device's name. Spec: MainWindow.xaml (the tree row's ToolTip, FullPath).
    std::wstring tip(uint64_t node) const override { return is_device(node) ? label(node) : path_of(node); }

    std::wstring path_of(uint64_t node) const;
    // The nodes from the drive down to `path`, loading each level. Empty when the path is not under a drive.
    std::vector<uint64_t> chain_to(const std::wstring& path) const;
    // Hidden folders are left out unless asked for, and dimmed when they are shown. Every node found is kept either way, so
    // a change re-filters in place: ids, expansion and selection survive it. True when it changed.
    bool set_show_hidden(bool on);
    // Reads the sub-folders of every loaded node `keep` says yes to again, after something may have made,
    // renamed, moved or deleted one. A folder still there keeps its id (so expansion and selection survive);
    // one that has gone is dropped. True when anything changed.
    template <class Keep>
    bool reload_where(Keep keep) {
        bool changed = false;
        std::vector<uint64_t> ids;
        for (const auto& kv : nodes_) {
            if (kv.first != 0 && kv.second.loaded && keep(kv.first)) ids.push_back(kv.first);
        }
        for (const uint64_t id : ids) changed |= reload(id);
        return changed;
    }

private:
    struct Node {
        std::wstring label, path;
        bool loaded = false;
        bool loading = false;  // a worker is reading it
        bool device = false;   // a portable device: no children, opens in Explorer
        // What a probe found under it before it was opened: 0 not probed, 1 nothing, 2 hidden folders only,
        // 3 a folder that shows.
        uint8_t sub = 0;
        std::wstring size;  // what the row shows at its right edge
        std::vector<uint64_t> kids;   // every sub-folder found
        uint32_t attributes = 0;  // from the listing that found it: a special folder is read-only or system
        std::vector<uint64_t> shown;  // the ones `show_hidden_` lets through
    };
    struct Found {
        std::wstring label, path;
        uint32_t attributes;
        uint8_t sub = 0;
    };
    struct Result {
        uint64_t node = 0;
        bool listing = false;               // the node's own sub-folders (else probes only)
        std::vector<Found> found;           // listing: what it holds, sorted
        std::vector<std::pair<uint64_t, uint8_t>> probes;  // probes only: each kid's `sub`
    };
    struct Inbox {
        std::mutex lock;
        std::vector<Result> done;
        size_t pending = 0;
    };
    void load(uint64_t node) const;
    void start(Result job, std::vector<std::wstring> probe_paths) const;  // on a worker
    void probe_kids(uint64_t node) const;
    bool reload(uint64_t node);
    uint64_t add(Node n) const;
    void filter(Node& n) const;
    void fill_sizes(const std::vector<uint64_t>& ids) const;

    ShellIcons* icons_;
    bool show_hidden_ = false;
    mutable bool sync_ = false;  // load in place (a reveal, a reload) rather than on a worker
    std::shared_ptr<Inbox> inbox_ = std::make_shared<Inbox>();
    mutable std::unordered_map<uint64_t, Node> nodes_;
    mutable uint64_t next_ = 1;
};

}  // namespace bb::views
