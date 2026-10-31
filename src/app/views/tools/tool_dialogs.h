// The tool windows as dialogs: checksums (hash and verify), comparing two files, disk usage, the change
// timeline, the duplicate finder and the folder comparison's sync preview. dialogs.cpp asks here first.
// Kinds ending in -show or -at rebuild a window from the state the core already holds (a toggle just
// changed it); the plain kinds start a fresh one from the selection or the folder.
#pragma once
#include "views/dialogs/dialogs.h"

namespace bb::views {

bool tool_dialog_exists(const std::wstring& kind);
std::vector<std::wstring> tool_dialog_kinds();
// Null when `kind` is not one of these.
std::unique_ptr<ui::Widget> tool_build_dialog(const std::wstring& kind, DialogContext ctx);

// The banner over the panes while two are being compared (named CompareBanner); hidden until there is a
// comparison. `compare_banner_sync` re-reads the core's model into it.
std::unique_ptr<ui::Widget> make_compare_banner(MainView& main);
void compare_banner_sync(ui::Widget* banner);

}  // namespace bb::views
