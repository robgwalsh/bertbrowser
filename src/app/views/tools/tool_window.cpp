#include "views/tools/tool_window.h"

#include "gfx/tokens.h"
#include "ui/core/layout.h"
#include "views/chrome/chrome.h"
#include "views/main/main_view.h"
#include "views/tools/tool_widgets.h"

#include <windows.h>

#include <algorithm>
#include <cmath>

namespace bb::views {

using namespace bb::ui;
namespace tok = gfx::tok;
namespace tw = tools;

namespace {

// The card's own title, read off the header the dialog layer would have shown.
std::wstring title_of(Widget* card) {
    Widget* head = card ? card->find("CardHeader") : nullptr;
    if (!head) return L"BertBrowser";
    for (const auto& c : head->children()) {
        if (auto* t = dynamic_cast<TextBlock*>(c.get())) return t->text();
    }
    return L"BertBrowser";
}

// How much of the screen a window may ask for before it scrolls (tall cards: the history, the queue).
constexpr float kMaxHeight = 760.0f;

}  // namespace

ToolWindow::ToolWindow(MainView& main, std::wstring kind, std::unique_ptr<Widget> card) : main_(main), kind_(std::move(kind)) {
    const platform::WindowOptions& owner = main.window().options();
    platform::WindowOptions o;
    o.title = title_of(card.get());
    o.width_dip = 480;
    o.height_dip = 320;
    o.parked = owner.parked;
    o.offscreen = owner.offscreen;
    o.dpi_override = owner.dpi_override;
    o.custom_chrome = true;
    o.owner = main.window().hwnd();
    window_ = std::make_unique<platform::Window>(main.window().root().resources(), o);
    window_->on_close = [this] {
        // Not from inside the window's own message: the main view owns it and destroys it after.
        MainView* m = &main_;
        const std::wstring k = kind_;
        m->defer([m, k] { m->close_tool(k); });
    };
    // Esc closes it, as it closes a dialog; every other key is the window's own (the folders' shortcuts
    // belong to the main window, which is one click away).
    window_->root().on_preview_key = [this](const KeyEvent& e) {
        if (e.vk != VK_ESCAPE || e.mods.ctrl || e.mods.alt || e.mods.shift) return main_.tool_window_key(kind_, e);
        if (window_->on_close) window_->on_close();
        return true;
    };
    install(std::move(card));
}

ToolWindow::~ToolWindow() {
    if (!window_) return;
    window_->on_close = nullptr;
    window_->root().on_preview_key = nullptr;
    // The tree goes first: its widgets may call into the core as they die (a cancelled survey).
    window_->root().clear_content();
}

void ToolWindow::install(std::unique_ptr<Widget> card) {
    auto frame = std::make_unique<DockPanel>();
    frame->style.bg = tok::OverlayBackground;
    title_ = frame->emplace<TitleBar>();
    title_->dock = Dock::Top;
    const std::wstring title = title_of(card.get());
    title_->set_title(title);
    SetWindowTextW(window_->hwnd(), title.c_str());  // the taskbar's and Alt+Tab's name for it
    // Owned by the main window, which it minimizes with: no minimize button of its own.
    if (!title_->children().empty()) title_->remove_child(title_->children().front().get());
    if (Widget* head = card->find("CardHeader")) head->set_visible(false);
    card_width_ = card->width >= 0 ? card->width : 480.0f;
    card->width = -1;  // the card fills the window, whatever size it is dragged to
    card->dock = Dock::Fill;
    card_ = frame->add(std::move(card));
    window_->root().set_content(std::move(frame));
    window_->set_frame_colors(true, RGB(0x3c, 0x3c, 0x3c), RGB(0x1e, 0x1e, 0x1e), RGB(0xe0, 0xe0, 0xe0));
    main_.style_tool_window(*this);
    fit();
}

void ToolWindow::fit() {
    // The natural size of the card at its own width, plus the caption. Set the first time; afterwards only
    // grown, so a window someone made bigger stays the size they made it, and one whose model grew (a scan
    // that found something) gets the room for it.
    const Size d = card_->measure_tree({card_width_, kInf});
    const float w = card_width_;
    const float h = std::min(kMaxHeight, TitleBar::kHeight + d.h);
    if (!sized_) {
        window_->set_client_size_dip(w, h);
        window_->center_over(main_.window().hwnd());
        sized_ = true;
    } else {
        const Size now = window_->root().size();
        if (h > now.h + 0.5f || w > now.w + 0.5f) window_->set_client_size_dip(std::max(w, now.w), std::max(h, now.h));
    }
    window_->root().request_layout();
    window_->root().invalidate_all();
}

void ToolWindow::replace(std::unique_ptr<Widget> card) {
    // Keep what had the keyboard, by name, and where every list was scrolled to, so a rebuild after a tick
    // or a refresh does not throw either away.
    std::string focused;
    if (Widget* f = window_->root().focused()) focused = f->name;
    const tw::ListPlaces places = card_ ? tw::save_places(*card_) : tw::ListPlaces{};
    window_->root().set_focus(nullptr);
    tw::restore_places(*card, places);
    install(std::move(card));
    if (!focused.empty()) {
        if (Widget* again = window_->root().find(focused)) window_->root().set_focus(again);
    }
}

void ToolWindow::show() { window_->show(); }

void ToolWindow::focus() {
    if (window_->options().parked) return;
    if (IsIconic(window_->hwnd())) ShowWindow(window_->hwnd(), SW_RESTORE);
    SetForegroundWindow(window_->hwnd());
}

void ToolWindow::apply_theme(bool dark, COLORREF border, COLORREF caption, COLORREF text) {
    window_->set_frame_colors(dark, border, caption, text);
    window_->root().invalidate_all();
}

Widget* ToolWindow::find(std::string_view name) { return window_->root().find(name); }

}  // namespace bb::views
