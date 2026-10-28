// The leaf widgets: text, icon, button, checkbox.
#pragma once
#include "ui/widget.h"

namespace bb::ui {

class TextBlock : public Widget {
public:
    explicit TextBlock(std::wstring t = {}) : text_(std::move(t)) {}
    const std::wstring& text() const { return text_; }
    void set_text(std::wstring t);
    gfx::TextAlign align = gfx::TextAlign::Leading;
    bool wrap = false;

    Size measure(Size avail) override;
    void paint(PaintContext& pc) override;

private:
    std::wstring text_;
};

// Icon geometry as data: a stream of floats, so tools/icons can emit a constexpr array.
//   0 x y            move to (starts a figure)
//   1 x y            line to
//   2 x1 y1 x2 y2 x y  cubic Bezier to
//   3                close the figure
// Coordinates are in a square `viewbox` (Fluent's 20 or 24); the widget scales to its size.
struct IconData {
    float viewbox;
    const float* cmds;
    size_t count;
};
enum IconOp : int { IconMove = 0, IconLine = 1, IconCubic = 2, IconClose = 3 };

class Icon : public Widget {
public:
    explicit Icon(const IconData* d = nullptr, float size_dip = 16.0f) : data(d), size(size_dip) {}
    const IconData* data;
    float size;

    Size measure(Size avail) override;
    void paint(PaintContext& pc) override;
    // Fills `bounds` with the icon in `fg`. Used by Button too.
    static void draw(PaintContext& pc, const IconData& d, const Rect& into, Token fg, ComPtr<ID2D1PathGeometry>& cache,
                     uint32_t& cache_generation);

private:
    ComPtr<ID2D1PathGeometry> geometry_;
    uint32_t generation_ = 0;
};

class Button : public Widget {
public:
    explicit Button(std::wstring label = {});
    std::wstring text;
    const IconData* icon = nullptr;
    bool primary = false;  // the accent-filled one
    std::function<void()> on_click;
    bool pressed() const { return pressed_; }
    void click();  // as if clicked

    Size measure(Size avail) override;
    void paint(PaintContext& pc) override;
    bool on_mouse_down(const MouseEvent& e) override;
    bool on_mouse_up(const MouseEvent& e) override;
    void on_mouse_enter() override { invalidate(); }
    void on_mouse_leave() override { invalidate(); }
    bool on_key_down(const KeyEvent& e) override;
    void on_focus(bool) override { invalidate(); }
    Cursor cursor_at(Point) const override { return Cursor::Arrow; }

private:
    bool pressed_ = false;
    ComPtr<ID2D1PathGeometry> icon_geo_;
    uint32_t icon_gen_ = 0;
};

class CheckBox : public Widget {
public:
    explicit CheckBox(std::wstring label = {});
    std::wstring text;
    bool checked() const { return checked_; }
    void set_checked(bool v);
    std::function<void(bool)> on_toggle;

    Size measure(Size avail) override;
    void paint(PaintContext& pc) override;
    bool on_mouse_down(const MouseEvent& e) override;
    bool on_mouse_up(const MouseEvent& e) override;
    void on_mouse_enter() override { invalidate(); }
    void on_mouse_leave() override { invalidate(); }
    bool on_key_down(const KeyEvent& e) override;
    void on_focus(bool) override { invalidate(); }

    static constexpr float kBox = 16.0f;
    static constexpr float kGap = 8.0f;

private:
    void toggle();
    bool checked_ = false;
    bool pressed_ = false;
};

}  // namespace bb::ui
