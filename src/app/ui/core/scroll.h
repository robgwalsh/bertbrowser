// Scrolling and splitting: the scroll bar the virtualized widgets share, the scroll viewer for
// ordinary content, and the splitter between two panes.
#pragma once
#include "ui/core/layout.h"
#include "ui/core/widget.h"

namespace bb::ui {

// Rows a turn of the wheel by `delta` moves the file list and the folder tree, downwards positive: Windows' own
// lines-per-notch times the user's multiplier (Settings, "Vertical scroll speed"). Shift+wheel's sideways scroll
// is left alone. Spec: ScrollSpeed.HandlePreviewMouseWheel.
float wheel_rows(int delta);
void set_wheel_multiplier(double multiplier);

// One axis of a scroll bar, drawn and dragged by whichever widget owns it. Not a Widget: the list and the
// tree need the thumb maths without a child to hit-test.
class ScrollBar {
public:
    explicit ScrollBar(bool is_vertical) : vertical(is_vertical) {}
    static constexpr float kThickness = 12.0f;

    bool vertical;
    float extent = 0, viewport = 0;
    Rect area;  // the strip it occupies; set by the owner

    float offset() const { return offset_; }
    void set_offset(float o) { offset_ = std::clamp(o, 0.0f, max_offset()); }
    // Before the first arrange, which clamps it to the extent then known (a rebuilt tree keeping its place).
    void preset(float o) { offset_ = std::max(0.0f, o); }
    float max_offset() const { return std::max(0.0f, extent - viewport); }
    bool needed() const { return extent > viewport + 0.5f; }
    Rect thumb() const;
    bool dragging() const { return dragging_; }

    void paint(PaintContext& pc) const;
    // Return true when the point is theirs; check offset() for what changed.
    bool mouse_down(Point p);
    bool mouse_move(Point p);
    void mouse_up() { dragging_ = false; }

private:
    float offset_ = 0;
    bool dragging_ = false;
    float grab_ = 0;
};

class ScrollViewer : public Widget {
public:
    bool vertical = true;
    bool horizontal = false;
    float line = 16.0f;  // one wheel notch scrolls three of these

    float offset_x() const { return h_.offset(); }
    float offset_y() const { return v_.offset(); }
    Size extent() const { return {h_.extent, v_.extent}; }
    Rect viewport() const { return viewport_; }
    void scroll_to(float x, float y);
    void scroll_by(float dx, float dy) { scroll_to(offset_x() + dx, offset_y() + dy); }
    void preset_offset(float x, float y) {
        h_.preset(x);
        v_.preset(y);
    }

    ScrollViewer() : v_(true), h_(false) {}
    Size measure(Size avail) override;
    void arrange(Rect r) override;
    void paint_children(PaintContext& pc) override;
    Widget* hit_test(Point p) override;
    bool on_mouse_down(const MouseEvent& e) override;
    bool on_mouse_move(const MouseEvent& e) override;
    bool on_mouse_up(const MouseEvent& e) override;
    bool on_wheel(Point p, int delta, Mods m) override;

private:
    void place_content();
    ScrollBar v_, h_;
    Rect viewport_;
    Size content_;
};

// Two children and a draggable bar between them. Horizontal puts them side by side (a vertical bar).
class Splitter : public Widget {
public:
    explicit Splitter(Orientation o = Orientation::Horizontal) : orientation(o) {}
    Orientation orientation;
    float thickness = 5.0f;
    float min_first = 40.0f, min_second = 40.0f;
    std::function<void(float)> on_moved;  // the first pane's new size
    std::function<void()> on_released;    // a drag of the bar ended

    float position() const { return position_; }
    void set_position(float p);
    Rect bar() const;

    void arrange(Rect r) override;
    void paint_children(PaintContext& pc) override;
    Widget* hit_test(Point p) override;
    bool on_mouse_down(const MouseEvent& e) override;
    bool on_mouse_move(const MouseEvent& e) override;
    bool on_mouse_up(const MouseEvent& e) override;
    void on_mouse_enter() override { invalidate(); }
    void on_mouse_leave() override { invalidate(); }
    Cursor cursor_at(Point p) const override;

private:
    float clamp(float p) const;
    float position_ = 200.0f;
    bool dragging_ = false;
    float grab_ = 0;
};

}  // namespace bb::ui
