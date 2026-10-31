// What other programs add to the right-click menu, as the views see it: per extension, never asked of the
// shell whole (CDefFolderMenu would hand back Cut/Copy/Delete/Rename/Properties alongside the extensions
// with no way to tell whose item is whose, and the Context menu page needs exactly that attribution).
// The product's source hosts each COM handler in a private menu (shell_menu_real.cpp); the harness swaps in
// a canned one, so a scripted run loads no foreign code and launches nothing.
// Spec: Services/ShellMenuSource, Interop/ShellContextMenu, Views/ShellMenu, Core/Services/ShellMenu/*.
#pragma once
#include <windows.h>

#include "ui/controls/controls.h"

#include <memory>
#include <string>
#include <vector>

namespace bb::views {

struct ShellTarget {
    std::wstring path;
    bool is_directory = false;
};

enum class ShellContext { Items, Background };

// One line of an extension's contribution, ready for a MenuItem: the Win32 text with its `&` access marker
// and the tab-separated shortcut taken off. `command` is the handle the session turns back into an invocation.
struct ShellEntry {
    std::wstring header, gesture;
    bool separator = false;
    bool enabled = true;
    // The extension's own picture for the line (MENUITEMINFO::hbmpItem), 32-bit premultiplied; null when it gave none.
    ui::ThumbPtr picture;
    std::vector<ShellEntry> children;
    int command = -1;
};

struct ShellGroup {
    std::string extension_id;  // `clsid:{...}` or `verb:name`, as the Context menu page stores it
    std::vector<ShellEntry> entries;
};

// A row of the Context menu page's checklist.
struct ShellExtensionInfo {
    std::string id, name;
    bool handler = false;
};

// Owns the COM objects, PIDLs and menus behind one opening of the menu. Released after the menu closes,
// deferred, because the click that calls Invoke has to have run first - and only this session, since a
// right-click may have opened the next.
class ShellMenuSession {
public:
    virtual ~ShellMenuSession() = default;
    std::vector<ShellGroup> groups;
    // Runs one entry. Returns a sentence for the status line (empty when there is nothing to say).
    virtual std::wstring invoke(int command, HWND owner) = 0;
};

class ShellMenuSource {
public:
    virtual ~ShellMenuSource() = default;
    // Every extension this machine has, for the checklist. May be slow: it reads the registry.
    virtual std::vector<ShellExtensionInfo> catalog() = 0;
    // The extensions with something to offer for these targets. Everything is returned: whether one is
    // shown is the composer's question (the page's ticks), so a newly installed extension appears
    // without a visit to Settings.
    virtual std::unique_ptr<ShellMenuSession> open(const std::vector<ShellTarget>& targets, ShellContext context,
                                                   const std::wstring& folder) = 0;
};

// The product's: the registry and COM handlers hosted in private menus.
std::unique_ptr<ShellMenuSource> make_registry_shell_source(class MainView& main);

}  // namespace bb::views
