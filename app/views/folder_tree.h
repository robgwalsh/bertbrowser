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

    std::wstring path_of(uint64_t node) const;
    // The nodes from the drive down to `path`, loading each level. Empty when the path is not under a drive.
    std::vector<uint64_t> chain_to(const std::wstring& path) const;
    // Hidden folders are left out unless asked for.
    bool show_hidden = false;

private:
    struct Node {
        std::wstring label, path;
        bool loaded = false;
        std::vector<uint64_t> kids;
        uint32_t attributes = 0;  // from the listing that found it: a special folder is read-only or system
    };
    void load(uint64_t node) const;
    uint64_t add(Node n) const;

    ShellIcons* icons_;
    mutable std::unordered_map<uint64_t, Node> nodes_;
    mutable uint64_t next_ = 1;
};

}  // namespace bb::views
