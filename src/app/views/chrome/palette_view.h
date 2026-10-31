// The command palette: an overlay in the window's tree (so a capture sees it) with a box, a line of
// hint and the rows the core says match. The core owns the query, the ranking, the selection and what a
// pick does; this draws them and forwards the keys that belong to it.
#pragma once
#include "views/dialogs/forms.h"
#include "views/bridge/model.h"

namespace bb::views {

class MainView;

class PaletteView : public ui::Widget {
public:
    PaletteView(MainView& main, core::Core& core);

    void sync();
    bool is_open() const { return open_; }
    ui::Edit* box() const { return box_; }
    // A key the palette takes while it is up (arrows, Enter, Tab, Esc, Alt+P / Alt+K). true = handled.
    bool preview_key(const ui::KeyEvent& e);

    ui::Size measure(ui::Size avail) override { return avail; }
    void arrange(ui::Rect r) override;
    bool on_mouse_down(const ui::MouseEvent& e) override;
    bool on_wheel(ui::Point, int, ui::Mods) override { return true; }

private:
    void pick();
    void send(const wchar_t* verb, const std::wstring& arg = L"");

    MainView& main_;
    core::Core& core_;
    bool open_ = false;
    bool suppress_ = false;
    ui::Panel* card_ = nullptr;
    ui::Edit* box_ = nullptr;
    ui::TextBlock* hint_ = nullptr;
    forms::ItemList* list_ = nullptr;
    std::string signature_;
};

}  // namespace bb::views
