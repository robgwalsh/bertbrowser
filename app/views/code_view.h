// Read-only text for the preview pane: source with syntax colours, a hex dump, or plain text. Selectable and
// copyable, scrolled both ways, with the line-number gutter drawn beside it (not a column of text laid out
// next to it, which laid out every line of a large file to show thirty).
//
// Only the lines in view are given a text layout, so a five-thousand-line file costs what a screenful does.
// Wrapping turns the gutter off: a wrapped line is not one row, so a number beside it would number the wrong
// thing.
#pragma once
#include "ui/scroll.h"
#include "ui/widget.h"

#include <functional>
#include <string>
#include <vector>

namespace bb::views {

class CodeView : public ui::Widget {
public:
    // A coloured run: `start`/`length` are UTF-16 units within the line. `cls` is the core's SyntaxClass
    // number: 0 text, 1 keyword, 2 string, 3 comment, 4 number, 5 punctuation.
    struct Span {
        uint32_t line, start, length;
        uint8_t cls;
    };

    CodeView();

    // `numbers` asks for the gutter; wrapping suppresses it.
    void set_content(std::vector<std::wstring> lines, std::vector<Span> spans, bool numbers);
    void clear() { set_content({}, {}, false); }
    void set_wrap(bool wrap);
    bool wrap() const { return wrap_; }
    bool gutter_shown() const { return numbers_ && !wrap_ && !lines_.empty(); }

    const std::vector<std::wstring>& lines() const { return lines_; }
    std::wstring all_text() const;
    std::wstring selected_text() const;
    void select_all();
    bool has_selection() const { return anchor_ != caret_; }
    void copy_selection();
    // A right click, at the pointer: whoever owns the view offers Copy and Select all.
    std::function<void(ui::Point)> on_context;

    // How wide the pane would need to be to show the widest line with no horizontal bar.
    float desired_width();
    float scroll_y() const { return v_.offset(); }
    float scroll_x() const { return h_.offset(); }
    void scroll_to_top();
    // Lines with a layout made by the last paint (the gutter and the text both draw only these).
    size_t painted_lines() const { return painted_; }

    ui::Size measure(ui::Size) override { return fixed_or({0, 0}); }
    void arrange(ui::Rect r) override;
    void paint(ui::PaintContext& pc) override;
    bool on_mouse_down(const ui::MouseEvent& e) override;
    bool on_mouse_move(const ui::MouseEvent& e) override;
    bool on_mouse_up(const ui::MouseEvent& e) override;
    bool on_wheel(ui::Point p, int delta, ui::Mods m) override;
    bool on_key_down(const ui::KeyEvent& e) override;
    ui::Cursor cursor_at(ui::Point p) const override;

    static constexpr float kPadX = 8.0f, kPadY = 4.0f;

private:
    struct Pos {
        size_t line = 0, col = 0;
        bool operator==(const Pos&) const = default;
        bool operator<(const Pos& o) const { return line != o.line ? line < o.line : col < o.col; }
    };

    void ensure_metrics();
    void relayout();
    float gutter_width() const;
    float line_top(size_t i) const { return wrap_ ? tops_[i] : static_cast<float>(i) * line_h_; }
    float line_height(size_t i) const { return wrap_ ? tops_[i + 1] - tops_[i] : line_h_; }
    size_t line_at(float content_y) const;
    ui::Rect text_rect() const { return text_; }
    Pos hit(ui::Point p);
    ui::ComPtr<IDWriteTextLayout> layout_for(size_t line);
    void keep_in_view(Pos p);

    std::vector<std::wstring> lines_;
    std::vector<Span> spans_;
    std::vector<size_t> span_begin_;  // per line, into spans_ (size lines + 1)
    bool numbers_ = false, wrap_ = false;

    gfx::FontSpec font_{gfx::Family::Mono, 13.0f, 400};
    float line_h_ = 18.0f, char_w_ = 8.0f;
    bool metrics_ready_ = false;
    std::vector<float> tops_;  // wrapped: top of each line, plus the end
    float wrapped_for_ = -1;

    ui::ScrollBar v_{true}, h_{false};
    ui::Rect text_, gutter_;
    float widest_ = 0;
    size_t painted_ = 0;

    Pos anchor_, caret_;
    bool selecting_ = false, bar_drag_ = false;
};

}  // namespace bb::views
