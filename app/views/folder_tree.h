// The folder tree's provider: drives at the top, a folder's sub-folders on demand. The core has no tree
// call (a listing is the only thing it hands out), so this reads directory names itself - names only,
// never sizes, and nothing is enumerated until a node is expanded or revealed.
#pragma once
#include "ui/tree_view.h"
#include "views/shell_icons.h"

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
        std::vector<uint64_t> kids;   // every sub-folder found
        uint32_t attributes = 0;  // from the listing that found it: a special folder is read-only or system
        std::vector<uint64_t> shown;  // the ones `show_hidden_` lets through
    };
    void load(uint64_t node) const;
    bool reload(uint64_t node);
    uint64_t add(Node n) const;
    void filter(Node& n) const;

    ShellIcons* icons_;
    bool show_hidden_ = false;
    mutable std::unordered_map<uint64_t, Node> nodes_;
    mutable uint64_t next_ = 1;
};

}  // namespace bb::views
