// The text edit: single or multi-line, over IDWriteTextLayout. Caret and backspace go by DirectWrite
// cluster (an emoji is one step), selection, undo grouped by kind of change (typing, deleting, other),
// clipboard through UiRoot::clipboard (a seam: the harness installs a fake), IME composition drawn inline
// and underlined, the caret kept in view. Generalised from spike S1.
#pragma once
#include "ui/widget.h"

namespace bb::ui {

class Edit : public Widget {
public:
    explicit Edit(bool multiline_ = false);

    std::wstring placeholder;
    bool multiline;
    bool read_only = false;
    bool password = false;  // shows a dot per character (the archive password box)
    bool plain = false;     // no box: a value shown as text that can still be selected and copied
    std::function<void(const std::wstring&)> on_changed;  // after any change to the text
    std::function<void()> on_commit;                      // Enter, single-line
    std::function<void()> on_cancel;                      // Escape
    float inset_right = 0;  // room kept clear of text at the right edge, for buttons inside the box

    const std::wstring& text() const { return text_; }
    void set_text(std::wstring t);  // replaces the text, drops the undo history, caret to the end
    size_t caret() const { return caret_; }
    size_t anchor() const { return anchor_; }
    std::wstring selected_text() const;
    void select(size_t anchor, size_t caret);
    bool composing() const { return composing_; }
    float scroll_x() const { return scroll_x_; }
    float scroll_y() const { return scroll_y_; }
    bool can_undo() const { return !undo_.empty(); }
    // Where the caret is, in window DIPs.
    Rect caret_rect();

    Size measure(Size avail) override;
    void arrange(Rect r) override;
    void paint(PaintContext& pc) override;
    bool on_mouse_down(const MouseEvent& e) override;
    bool on_mouse_move(const MouseEvent& e) override;
    bool on_mouse_up(const MouseEvent& e) override;
    bool on_key_down(const KeyEvent& e) override;
    bool on_char(wchar_t c) override;
    void on_focus(bool f) override;
    void on_timer(int id) override;
    Cursor cursor_at(Point) const override { return Cursor::IBeam; }
    bool ime_caret_rect(Rect* r) override;
    void on_ime_composition(const std::wstring& s, size_t cursor) override { set_composition(s, cursor); }
    void on_ime_commit(const std::wstring& s) override { commit_composition(s); }
    void on_ime_end() override { end_composition(); }

    void set_composition(const std::wstring& composition, size_t cursor);
    void commit_composition(const std::wstring& result);
    void end_composition();
    void undo() { step(undo_, redo_); }
    void redo() { step(redo_, undo_); }

private:
    enum class Kind { None, Typing, Deleting, Other };
    struct Snapshot {
        std::wstring text;
        size_t anchor, caret;
    };
    static constexpr float kPadX = 6.0f;
    static constexpr float kPadY = 4.0f;

    std::pair<size_t, size_t> selection() const { return {std::min(anchor_, caret_), std::max(anchor_, caret_)}; }
    std::wstring display_text() const;
    size_t display_caret() const { return composing_ ? caret_ + composition_cursor_ : caret_; }
    float inner_width() const { return std::max(1.0f, bounds.width() - 2 * kPadX - inset_right); }
    void ensure_layout();
    Point origin();
    std::vector<DWRITE_HIT_TEST_METRICS> range_metrics(size_t start, size_t length);
    std::vector<size_t> boundaries();
    size_t prev_cluster(size_t pos);
    size_t next_cluster(size_t pos);
    size_t word_left(size_t pos) const;
    size_t word_right(size_t pos) const;
    size_t position_at(Point p);
    size_t vertical_move(size_t pos, int dir);
    size_t line_edge(size_t pos, bool end) const;
    void move(size_t to, bool extend);
    void backspace();
    void del();
    void insert(const std::wstring& s, Kind kind);
    void paste();
    void step(std::vector<Snapshot>& from, std::vector<Snapshot>& to);
    void keep_caret_visible();
    void touch();  // a key or click: show the caret and restart the blink

    std::wstring text_;
    size_t anchor_ = 0, caret_ = 0;
    wchar_t pending_high_ = 0;
    bool composing_ = false;
    std::wstring composition_;
    size_t composition_cursor_ = 0;
    Kind last_kind_ = Kind::None;
    std::vector<Snapshot> undo_, redo_;
    ComPtr<IDWriteTextLayout> layout_;
    bool dirty_ = true;
    float layout_width_ = -1;
    gfx::FontSpec layout_font_;
    float scroll_x_ = 0, scroll_y_ = 0;
    bool caret_on_ = true;
    bool selecting_ = false;
};

}  // namespace bb::ui
