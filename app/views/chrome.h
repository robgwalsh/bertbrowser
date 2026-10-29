// The window's own furniture: title bar with caption buttons, status bar, tab strip, breadcrumb. Plain
// widgets over the theme tokens; they know nothing about the core.
#pragma once
#include "ui/controls.h"
#include "ui/edit.h"
#include "ui/layout.h"
#include "views/core.h"

#include <functional>
#include <string>
#include <vector>

namespace bb::views {

class CaptionButton : public ui::Widget {
public:
    enum class Kind { Minimize, Maximize, Close };
    explicit CaptionButton(Kind k);
    Kind kind;
    void paint(ui::PaintContext& pc) override;
    void on_mouse_enter() override { invalidate(); }
    void on_mouse_leave() override { invalidate(); }
};

// Draggable caption (the window's WM_NCHITTEST finds it by nc_hit) with the three caption buttons.
class TitleBar : public ui::Widget {
public:
    TitleBar();
    std::wstring title = L"BertBrowser";
    bool show_title = true;       // the main window hides it: the menu bar and toolbar take the room
    ui::Widget* left = nullptr;   // placed after the left padding (a menu bar); sized to its content
    ui::Widget* middle = nullptr; // takes what is left between it and the caption buttons (a toolbar)
    void set_title(std::wstring t);
    void arrange(ui::Rect r) override;
    void paint(ui::PaintContext& pc) override;
    static constexpr float kHeight = 32.0f;
};

class StatusBar : public ui::Widget {
public:
    StatusBar() { height = kHeight; }
    void set_text(std::wstring left, std::wstring right);
    const std::wstring& left() const { return left_; }
    const std::wstring& right() const { return right_; }
    // A click on the right-hand text (the index line offers "retry").
    std::function<void()> on_right_click;
    void paint(ui::PaintContext& pc) override;
    bool on_mouse_down(const ui::MouseEvent& e) override;
    static constexpr float kHeight = 24.0f;

private:
    float right_split() const;
    std::wstring left_, right_;
};

struct TabEntry {
    uint64_t id = 0;
    std::wstring title;
    bool active = false;
};

class TabStrip : public ui::Widget {
public:
    TabStrip() { height = kHeight; }
    static constexpr float kHeight = 30.0f;

    std::function<void(uint64_t)> on_activate;
    std::function<void(uint64_t)> on_close;
    void set_tabs(std::vector<TabEntry> tabs, bool pane_active);
    const std::vector<TabEntry>& tabs() const { return tabs_; }
    ui::Rect tab_rect(size_t i) const;
    // Poses the insertion line a tab drag shows at a gap (0 is before the first tab); -1 takes it away.
    void set_drag_gap(int gap) {
        drag_gap_ = gap;
        invalidate();
    }
    int drag_gap() const { return drag_gap_; }

    void paint(ui::PaintContext& pc) override;
    bool on_mouse_down(const ui::MouseEvent& e) override;
    bool on_mouse_move(const ui::MouseEvent& e) override;
    void on_mouse_leave() override;

private:
    void paint_tabs(ui::PaintContext& pc);
    size_t index_at(ui::Point p) const;
    float tab_width(size_t i) const;
    std::vector<TabEntry> tabs_;
    int drag_gap_ = -1;
    bool pane_active_ = false;
    size_t hot_ = static_cast<size_t>(-1);
};

class BreadcrumbBar : public ui::Widget {
public:
    BreadcrumbBar() { flags.focusable = false; }
    std::function<void(const std::wstring&)> on_navigate;
    std::function<void()> on_edit;  // a click on the empty part: the path box takes over
    void set_crumbs(std::vector<core::Crumb> crumbs);
    const std::vector<core::Crumb>& crumbs() const { return crumbs_; }

    void arrange(ui::Rect r) override;
    void paint(ui::PaintContext& pc) override;
    bool on_mouse_down(const ui::MouseEvent& e) override;
    bool on_mouse_move(const ui::MouseEvent& e) override;
    void on_mouse_leave() override;
    ui::Cursor cursor_at(ui::Point p) const override;

private:
    struct Seg {
        size_t crumb;
        ui::Rect rect;
    };
    void lay_out();
    size_t seg_at(ui::Point p) const;
    std::vector<core::Crumb> crumbs_;
    std::vector<Seg> segs_;
    bool laid_out_ = false;
    size_t hot_ = static_cast<size_t>(-1);
};

// An Edit that says when it loses the focus, so the path box can hand back to the breadcrumb.
class PathEdit : public ui::Edit {
public:
    std::function<void()> on_blur;
    void on_focus(bool f) override {
        ui::Edit::on_focus(f);
        if (!f && on_blur) on_blur();
    }
};

// Text laid over a list (the empty-folder message).
class Overlay : public ui::TextBlock {
public:
    Overlay() {
        align = gfx::TextAlign::Center;
        flags.hit_testable = false;
        style.fg = ui::tok::TextMuted;
    }
    void set_message(std::wstring t) {
        set_visible(!t.empty());
        set_text(std::move(t));
    }
};

}  // namespace bb::views
