// The app's dialogs, as widgets. Each is a card (title, body, buttons) built from what the core says; the
// window shows one over a scrim (DialogLayer), and the harness photographs one on its own, outside any
// window, so a capture never depends on what is behind it.
#pragma once
#include "gfx/surface.h"
#include "ui/layout.h"
#include "views/forms.h"
#include "views/model.h"

#include <functional>
#include <memory>
#include <string>
#include <vector>

namespace bb::views {

class MainView;

// "4.200 GB", "112.0 MB", "93.8 KB", "240 B".
std::wstring format_bytes(int64_t bytes);
// "2024-03-14 09:26" from FILETIME ticks; empty when unknown.
std::wstring format_file_time(int64_t ticks);

struct DialogContext {
    MainView& main;
    std::function<void()> close;  // dismisses the dialog (a no-op for a photograph)
};

using DialogBuilder = std::function<std::unique_ptr<ui::Widget>(DialogContext&)>;

// Whether a kind of dialog exists.
bool dialog_exists(const std::wstring& kind);
// The kinds, for the error that names them.
std::vector<std::wstring> dialog_kinds();
// Builds a dialog's card, or null for a kind that does not exist. The card has its natural height at its
// own width.
std::unique_ptr<ui::Widget> build_dialog(const std::wstring& kind, DialogContext ctx);

// Lays a dialog's card out at its natural size inside a scratch root (nothing of the window shows) and
// paints it to a bitmap.
std::unique_ptr<gfx::BitmapSurface> render_dialog_detached(gfx::Resources& res, std::unique_ptr<ui::Widget> card);

// The layer that shows one dialog over the window: a scrim, the card centred, Esc closes it.
class DialogLayer : public ui::Widget {
public:
    DialogLayer();
    void show(std::unique_ptr<ui::Widget> card);
    void close();
    bool is_open() const { return card_ != nullptr; }
    std::function<void()> on_closed;

    ui::Size measure(ui::Size avail) override { return avail; }
    void arrange(ui::Rect r) override;
    bool on_mouse_down(const ui::MouseEvent&) override { return true; }
    bool on_wheel(ui::Point, int, ui::Mods) override { return true; }

private:
    ui::Widget* card_ = nullptr;
};

}  // namespace bb::views
