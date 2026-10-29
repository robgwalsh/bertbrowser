// The window's own furniture: title bar with caption buttons, status bar, tab strip, breadcrumb. Plain
// widgets over the theme tokens; they know nothing about the core.
#pragma once
#include "ui/controls.h"
#include "ui/edit.h"
#include "ui/inputs.h"
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
    ui::Widget* after = nullptr;  // sized to its content, just after `middle`'s (the title bar's switchers)
    // Centred on the whole bar rather than on what is left, at its own width, and pushed right (then
    // narrowed) when what is to its left needs the room (the whole-PC search).
    ui::Widget* center = nullptr;
    void set_title(std::wstring t);
    void arrange(ui::Rect r) override;
    void paint(ui::PaintContext& pc) override;
    static constexpr float kHeight = 32.0f;
};

// The tab's status, then what Undo would undo (a link to the history), then the selection; at the right
// the index line and the thumbnail-size slider. Spec: MainWindow.xaml (StatusBar).
class StatusBar : public ui::Widget {
public:
    StatusBar();
    void set_text(std::wstring left, std::wstring right);
    // What Undo would undo ("Ctrl+Z: undo move 3 items"), and "3 items selected (1.2 MB)"; empty for none.
    void set_extras(std::wstring undo, std::wstring selection);
    void set_scale(double scale);  // the slider, unless it is being dragged
    const std::wstring& left() const { return left_; }
    const std::wstring& right() const { return right_; }
    const std::wstring& undo_text() const { return undo_; }
    const std::wstring& selection_text() const { return selection_; }
    ui::Slider* slider() const { return slider_; }
    // A click on the right-hand text (the index line offers "retry").
    std::function<void()> on_right_click;
    std::function<void()> on_undo_click;
    std::function<void(double scale)> on_scale;  // 0 is the details list
    void arrange(ui::Rect r) override;
    void paint(ui::PaintContext& pc) override;
    bool on_mouse_down(const ui::MouseEvent& e) override;
    ui::Cursor cursor_at(ui::Point p) const override;
    static constexpr float kHeight = 24.0f;

private:
    static constexpr float kSlider = 130.0f, kSliderIcon = 19.0f;
    float right_split() const;
    float right_edge() const;  // where the index line ends: the slider is beyond it
    ui::Rect undo_rect() const;
    std::wstring left_, right_, undo_, selection_;
    ui::Slider* slider_ = nullptr;
    gfx::ComPtr<ID2D1PathGeometry> icon_geo_;
    uint32_t icon_gen_ = 0;
};

struct TabEntry {
    uint64_t id = 0;
    std::wstring title;
    bool active = false;
    std::wstring path;  // the tab's tooltip
};

// A pane's tabs, then "+", split right and split below just after the last one. Once the tabs overflow
// the buttons stay put at the right edge and the tabs scroll under the wheel. A tab dragged along the
// strip is reordered. Spec: FilePaneView.xaml (tab strip), FilePaneView.TabHeader_PreviewMouseDown.
class TabStrip : public ui::Widget {
public:
    TabStrip();
    static constexpr float kHeight = 30.0f;

    std::function<void(uint64_t)> on_activate;
    std::function<void(uint64_t)> on_close;
    std::function<void(uint64_t, ui::Point)> on_context;  // a tab right-clicked
    std::function<void(uint64_t, uint32_t)> on_move;      // a tab dragged to a new index
    std::function<void()> on_new_tab;
    std::function<void(bool right)> on_split;
    ui::Button* new_tab_button() const { return new_tab_; }
    ui::Button* split_right_button() const { return split_right_; }
    ui::Button* split_below_button() const { return split_below_; }
    void set_tabs(std::vector<TabEntry> tabs, bool pane_active);
    const std::vector<TabEntry>& tabs() const { return tabs_; }
    ui::Rect tab_rect(size_t i) const;
    // Poses the insertion line a tab drag shows at a gap (0 is before the first tab); -1 takes it away.
    void set_drag_gap(int gap) {
        drag_gap_ = gap;
        invalidate();
    }
    int drag_gap() const { return drag_gap_; }

    void arrange(ui::Rect r) override;
    void paint(ui::PaintContext& pc) override;
    bool on_mouse_down(const ui::MouseEvent& e) override;
    bool on_mouse_up(const ui::MouseEvent& e) override;
    bool on_mouse_move(const ui::MouseEvent& e) override;
    bool on_wheel(ui::Point p, int delta, ui::Mods mods) override;
    void on_mouse_leave() override;
    void on_modal_cancel() override;

private:
    static constexpr float kButtons = 3 * 26.0f + 9.0f;  // three buttons and the rule between + and the splits
    void paint_tabs(ui::PaintContext& pc);
    size_t index_at(ui::Point p) const;
    float tab_width(size_t i) const;
    float tabs_width() const;  // every tab, unscrolled
    float tabs_right() const;  // where the tabs' visible run ends and the buttons begin
    void clamp_scroll();
    void reveal_active();
    int gap_at(float x) const;
    void end_drag();
    std::vector<TabEntry> tabs_;
    int drag_gap_ = -1;
    bool pane_active_ = false;
    size_t hot_ = static_cast<size_t>(-1);
    float scroll_ = 0;
    size_t pressed_ = static_cast<size_t>(-1);
    ui::Point press_at_;
    bool dragging_ = false;
    ui::Button* new_tab_ = nullptr;
    ui::Button* split_right_ = nullptr;
    ui::Button* split_below_ = nullptr;
};

class BreadcrumbBar : public ui::Widget {
public:
    BreadcrumbBar() { flags.focusable = false; }
    std::function<void(const std::wstring&)> on_navigate;
    std::function<void(const std::wstring&)> on_middle;  // a crumb middle-clicked
    std::function<void()> on_edit;  // a click on the empty part: the path box takes over
    void set_crumbs(std::vector<core::Crumb> crumbs);
    const std::vector<core::Crumb>& crumbs() const { return crumbs_; }

    void arrange(ui::Rect r) override;
    void paint(ui::PaintContext& pc) override;
    bool on_mouse_down(const ui::MouseEvent& e) override;
    bool on_mouse_move(const ui::MouseEvent& e) override;
    void on_mouse_leave() override;
    // The wheel walks back through crumbs that did not fit, and forward again.
    bool on_wheel(ui::Point p, int delta, ui::Mods m) override;
    ui::Cursor cursor_at(ui::Point p) const override;

private:
    struct Seg {
        size_t crumb;
        ui::Rect rect;
    };
    size_t back_ = 0;            // crumbs scrolled off the right end (0: the folder you are in shows)
    bool more_left_ = false, more_right_ = false;  // crumbs hidden that way, marked with an ellipsis
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
