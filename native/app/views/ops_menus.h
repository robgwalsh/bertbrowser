// The composed right-click menus and the Undo / Redo lists (ops_menus.cpp).
#pragma once
#include "ui/menu.h"
#include "views/ops.h"
#include "views/shell_menu.h"

#include <memory>

namespace bb::views {
class MainView;
}

namespace bb::views::ops {

enum class MenuKind { Files, Background, Tree };

struct ComposedMenu {
    ui::MenuItems items;
    std::shared_ptr<void> keep;  // the shell session behind the items; dropped once the menu has closed
};

ComposedMenu compose_context_menu(MainView& main, MenuKind kind);

// `undo`, `undo-range` (the third action marked, as a pointer hovering it would), `redo`.
bool build_step_menu(MainView& main, const std::wstring& kind, ui::MenuItems* out);

}  // namespace bb::views::ops
