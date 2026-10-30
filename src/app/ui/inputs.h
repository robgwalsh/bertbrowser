// Small input widgets: progress bar, slider, radio group, combo box (drop-down list).
#pragma once
#include "ui/layout.h"
#include "ui/popup_layer.h"

namespace bb::ui {

// A bar filled to `value` (0..1), or a segment sweeping back and forth when indeterminate.
class ProgressBar : public Widget {
public:
    static constexpr int kTimer = 1;
    static constexpr unsigned kPeriodMs = 1600;

    double value() const { return value_; }
    void set_value(double v);  // clamped to 0..1
    bool indeterminate() const { return indeterminate_; }
    void set_indeterminate(bool on);
    // Where the indeterminate segment is for a point in time: [0, 1) of the way across.
    static double sweep_phase(uint64_t now_ms) { return static_cast<double>(now_ms % kPeriodMs) / kPeriodMs; }
    // The filled part of the track.
    Rect filled() const;

    Size measure(Size) override { return fixed_or({160.0f, 8.0f}); }
    void paint(PaintContext& pc) override;
    void on_timer(int) override { invalidate(); }

private:
    double value_ = 0;
    bool indeterminate_ = false;
    bool timer_on_ = false;
};

// A track with a thumb. Pressing anywhere on the track - not only on the thumb - moves the value there and
// carries on dragging, which a Win32 trackbar does not do. Vertical sliders have the minimum at the bottom.
class Slider : public Widget {
public:
    explicit Slider(Orientation o = Orientation::Horizontal);
    static constexpr float kThumb = 14.0f;

    Orientation orientation;
    double min = 0, max = 100, step = 1, large_step = 10;
    double value() const { return value_; }
    void set_value(double v);  // clamped and snapped; does not call on_change
    bool dragging() const { return dragging_; }
    std::function<void(double value, bool dragging)> on_change;  // dragging false = released or a key

    Rect track() const;  // the usable run of the track (thumb centres move within it)
    Rect thumb() const;
    double value_at(Point p) const;

    Size measure(Size) override;
    void paint(PaintContext& pc) override;
    bool on_mouse_down(const MouseEvent& e) override;
    bool on_mouse_move(const MouseEvent& e) override;
    bool on_mouse_up(const MouseEvent& e) override;
    bool on_key_down(const KeyEvent& e) override;
    void on_mouse_enter() override { invalidate(); }
    void on_mouse_leave() override { invalidate(); }
    void on_focus(bool) override { invalidate(); }

private:
    double snap(double v) const;
    void apply(double v, bool dragging);
    double value_ = 0;
    bool dragging_ = false;
};

// One choice of several. Arrow keys move the choice (and the focus ring) like a Win32 radio group.
class RadioGroup : public Widget {
public:
    static constexpr float kRow = 26.0f, kDot = 16.0f, kGap = 8.0f;

    RadioGroup() {
        flags.focusable = true;
        flags.tab_stop = true;
    }
    Orientation orientation = Orientation::Vertical;
    void set_items(std::vector<std::wstring> items);
    const std::vector<std::wstring>& items() const { return items_; }
    int selected() const { return selected_; }
    void set_selected(int i);  // no callback
    void set_item_enabled(size_t i, bool enabled);
    bool item_enabled(size_t i) const { return i < items_.size() && enabled_[i]; }
    std::function<void(int)> on_change;

    Rect item_rect(int i) const;
    int item_at(Point p) const;

    Size measure(Size) override;
    void paint(PaintContext& pc) override;
    bool on_mouse_down(const MouseEvent& e) override;
    bool on_key_down(const KeyEvent& e) override;
    void on_focus(bool) override { invalidate(); }

private:
    void choose(int i);
    std::vector<std::wstring> items_;
    std::vector<bool> enabled_;
    std::vector<float> widths_;
    int selected_ = -1;
    int cursor_ = 0;
};

// A box showing the chosen item; opens its list through the window's PopupLayer (so it needs one).
class ComboBox : public Widget {
public:
    ComboBox() {
        flags.focusable = true;
        flags.tab_stop = true;
    }
    void set_items(std::vector<std::wstring> items);
    const std::vector<std::wstring>& items() const { return items_; }
    int selected() const { return selected_; }
    void set_selected(int i);  // no callback
    std::wstring placeholder;
    std::function<void(int)> on_change;

    void open_list();
    bool list_open() const { return open_; }
    // The type-ahead rule: the next item after `from` whose text starts with `ch` (case-insensitive), or -1.
    static int next_starting_with(const std::vector<std::wstring>& items, int from, wchar_t ch);

    Size measure(Size) override;
    void paint(PaintContext& pc) override;
    bool on_mouse_down(const MouseEvent& e) override;
    bool on_key_down(const KeyEvent& e) override;
    bool on_char(wchar_t c) override;
    void on_mouse_enter() override { invalidate(); }
    void on_mouse_leave() override { invalidate(); }
    void on_focus(bool) override { invalidate(); }

private:
    void choose(int i);
    std::vector<std::wstring> items_;
    MenuItems list_;  // alive while the list is open: the menu points into it
    int selected_ = -1;
    bool open_ = false;
};

}  // namespace bb::ui
