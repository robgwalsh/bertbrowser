// Dragging files out of, and into, the window - over OLE, so Explorer, editors and mail clients are all peers.
// A drag from the list carries the paths in a private format (so a drop back here knows it is ours) and
// as an ordinary file list (so everything else can take it). A drop is decided by the core (`drop-decide`,
// `drop-allowed`, `dragout-decide`): ours reports nothing, a foreign one reports the verb it really did, and
// the right button ends by asking which verb was meant - from a menu that is posted, never opened inside
// Drop, whose modal loop still owns the mouse. Spec: Views/DropPipeline, DragSession, FileDragDropController,
// Interop/DropEffectFormats.
#pragma once
#include "ui/list_view.h"
#include "ui/menu.h"

#include <windows.h>

#include <memory>
#include <optional>
#include <string>
#include <vector>

namespace bb::core {
class Core;
}

namespace bb::views {

class MainView;

// The files on the clipboard as Explorer puts them there: CF_HDROP for the paths and "Preferred DropEffect"
// to tell a cut from a copy, so cut, copy and paste work both ways with Explorer. Spec: Services/FileClipboard.
struct ClipboardFiles {
    std::vector<std::wstring> paths;
    bool cut = false;
};
namespace file_clipboard {
bool put(const std::vector<std::wstring>& paths, bool cut);  // false: another program holds the clipboard
std::optional<ClipboardFiles> get();
bool has_files();
void clear();
}  // namespace file_clipboard

// Moves, copies or links (`verb`: move, copy, shortcut; paste-move, paste-copy for a paste) sources into a folder - what a drop and a paste both
// are. Into an archive it is an archive edit. Returns the operation, or 0 when none started (the core has
// said why on the status line).
uint64_t transfer_into(core::Core& core, const std::vector<std::wstring>& sources, const std::wstring& destination,
                       const std::string& verb);

class FileDrag {
public:
    explicit FileDrag(MainView& main);
    ~FileDrag();
    FileDrag(const FileDrag&) = delete;
    FileDrag& operator=(const FileDrag&) = delete;

    // Registers the window as a drop target.
    void enable();
    // The list asked for a drag of its selection with `button`. Runs OLE's modal loop, then acts on what
    // the target said: the originals are removed only on the decision of `dragout-decide`.
    void start(uint64_t tab, ui::MouseButton button);

    // Carries out a verb the user chose ("move", "copy", "shortcut") for these sources over this folder.
    void perform(const std::vector<std::wstring>& sources, const std::wstring& destination, const std::string& verb);
    // The menu a right-drag ends with, as items.
    ui::MenuItems verb_menu(const std::vector<std::wstring>& sources, const std::wstring& destination);

    // The state of the drag in flight: whether our own drop took it, and which button started it.
    struct Session {
        bool right_button = false;
        bool handled_in_app = false;
    };
    Session* session() { return session_; }

private:
    class Target;
    friend class Target;
    MainView& main_;
    Target* target_ = nullptr;
    Session* session_ = nullptr;
    bool registered_ = false;
};

}  // namespace bb::views
