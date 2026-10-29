#include "ui/edit.h"

#include <cmath>

namespace bb::ui {

Edit::Edit(bool multiline_) : multiline(multiline_) {
    flags.focusable = true;
    flags.tab_stop = true;
    style.bg = tok::InputBackground;
}

std::wstring Edit::selected_text() const {
    const auto [lo, hi] = selection();
    return text_.substr(lo, hi - lo);
}

std::wstring Edit::display_text() const {
    if (!composing_) {
        return password ? std::wstring(text_.size(), L'•') : text_;
    }
    return text_.substr(0, caret_) + composition_ + text_.substr(caret_);
}

void Edit::set_text(std::wstring t) {
    text_ = std::move(t);
    caret_ = anchor_ = text_.size();
    undo_.clear();
    redo_.clear();
    last_kind_ = Kind::None;
    dirty_ = true;
    scroll_x_ = scroll_y_ = 0;
    keep_caret_visible();
    invalidate();
}

void Edit::select(size_t anchor, size_t caret) {
    anchor_ = std::min(anchor, text_.size());
    caret_ = std::min(caret, text_.size());
    last_kind_ = Kind::None;
    keep_caret_visible();
    touch();
}

Size Edit::measure(Size) {
    const float line = res().text.measure(L"Mg", style.font).height;
    return fixed_or(multiline ? Size{160.0f, std::max(80.0f, line + 2 * kPadY)} : Size{160.0f, std::ceil(line) + 2 * kPadY});
}

void Edit::arrange(Rect r) {
    const bool resized = r.width() != bounds.width() || r.height() != bounds.height();
    bounds = r;
    if (resized) {
        dirty_ = true;
        keep_caret_visible();
    }
}

void Edit::ensure_layout() {
    if (layout_ && !dirty_ && layout_font_ == style.font && (!multiline || layout_width_ == bounds.width())) {
        return;
    }
    gfx::LayoutOptions opt;
    opt.wrap = multiline;
    opt.max_width = multiline ? inner_width() : 1.0e6f;
    opt.max_height = 1.0e6f;
    layout_ = res().text.make_layout(display_text(), style.font, opt);
    layout_font_ = style.font;
    layout_width_ = bounds.width();
    dirty_ = false;
}

Point Edit::origin() {
    ensure_layout();
    if (multiline) {
        return {bounds.l + kPadX - scroll_x_, bounds.t + kPadY - scroll_y_};
    }
    DWRITE_TEXT_METRICS tm{};
    layout_->GetMetrics(&tm);
    return {bounds.l + kPadX - scroll_x_, bounds.t + std::max(0.0f, (bounds.height() - tm.height) / 2)};
}

Rect Edit::caret_rect() {
    const Point o = origin();
    float x = 0, y = 0;
    DWRITE_HIT_TEST_METRICS m{};
    layout_->HitTestTextPosition(static_cast<UINT32>(display_caret()), FALSE, &x, &y, &m);
    return {o.x + x, o.y + y, o.x + x + 1.0f, o.y + y + m.height};
}

bool Edit::ime_caret_rect(Rect* r) {
    if (!root()) return false;
    *r = caret_rect();
    return true;
}

std::vector<DWRITE_HIT_TEST_METRICS> Edit::range_metrics(size_t start, size_t length) {
    ensure_layout();
    UINT32 count = 0;
    layout_->HitTestTextRange(static_cast<UINT32>(start), static_cast<UINT32>(length), 0, 0, nullptr, 0, &count);
    std::vector<DWRITE_HIT_TEST_METRICS> out(count);
    if (count) {
        layout_->HitTestTextRange(static_cast<UINT32>(start), static_cast<UINT32>(length), 0, 0, out.data(), count,
                                  &count);
    }
    return out;
}

void Edit::paint(PaintContext& pc) {
    ensure_layout();
    pc.fill(bounds, style.bg);
    pc.stroke(bounds, focused() ? tok::BorderFocus : (style.border != kNoToken ? style.border : tok::InputBorder));
    const Rect inner = bounds.deflate(kPadX, 1, kPadX, 1);
    pc.clip(inner.intersect(pc.dirty));
    const Point o = origin();
    if (display_text().empty() && !placeholder.empty()) {
        pc.text(placeholder, Rect{o.x, bounds.t, inner.r, bounds.b}, style.font, tok::TextPlaceholder);
    }
    const auto [lo, hi] = selection();
    if (hi > lo && !composing_) {
        for (const auto& m : range_metrics(lo, hi - lo)) {
            pc.fill({o.x + m.left, o.y + m.top, o.x + m.left + m.width, o.y + m.top + m.height},
                    focused() ? tok::InputSelectionBackground : tok::ListSelectedInactiveBackground);
        }
    }
    if (composing_) {
        const DWRITE_TEXT_RANGE range{static_cast<UINT32>(caret_), static_cast<UINT32>(composition_.size())};
        layout_->SetUnderline(TRUE, range);
        layout_->SetDrawingEffect(pc.brush(tok::BorderActive), range);
    }
    pc.dc->DrawTextLayout(D2D1::Point2F(o.x, o.y), layout_.Get(), pc.brush(flags.enabled ? style.fg : tok::TextMuted));
    if (focused() && caret_on_) {
        pc.fill(caret_rect(), tok::InputCaret);
    }
    pc.unclip();
}

// ---- movement helpers ------------------------------------------------------------------------------

std::vector<size_t> Edit::boundaries() {
    ensure_layout();
    UINT32 count = 0;
    layout_->GetClusterMetrics(nullptr, 0, &count);
    std::vector<DWRITE_CLUSTER_METRICS> clusters(count);
    if (count) {
        layout_->GetClusterMetrics(clusters.data(), count, &count);
    }
    std::vector<size_t> out{0};
    size_t pos = 0;
    for (const auto& c : clusters) {
        pos += c.length;
        out.push_back(pos);
    }
    return out;
}

size_t Edit::prev_cluster(size_t pos) {
    size_t best = 0;
    for (size_t b : boundaries()) {
        if (b < pos) best = b;
    }
    return best;
}

size_t Edit::next_cluster(size_t pos) {
    for (size_t b : boundaries()) {
        if (b > pos) return b;
    }
    return text_.size();
}

size_t Edit::word_left(size_t pos) const {
    while (pos > 0 && text_[pos - 1] == L' ') --pos;
    while (pos > 0 && text_[pos - 1] != L' ') --pos;
    return pos;
}

size_t Edit::word_right(size_t pos) const {
    while (pos < text_.size() && text_[pos] != L' ') ++pos;
    while (pos < text_.size() && text_[pos] == L' ') ++pos;
    return pos;
}

size_t Edit::line_edge(size_t pos, bool end) const {
    if (end) {
        const size_t n = text_.find(L'\n', pos);
        return n == std::wstring::npos ? text_.size() : n;
    }
    if (pos == 0) return 0;
    const size_t n = text_.rfind(L'\n', pos - 1);
    return n == std::wstring::npos ? 0 : n + 1;
}

size_t Edit::position_at(Point p) {
    if (composing_) return caret_;
    ensure_layout();
    const Point o = origin();
    BOOL trailing = FALSE, inside = FALSE;
    DWRITE_HIT_TEST_METRICS m{};
    layout_->HitTestPoint(p.x - o.x, p.y - o.y, &trailing, &inside, &m);
    return std::min(text_.size(), static_cast<size_t>(m.textPosition) + (trailing ? m.length : 0u));
}

size_t Edit::vertical_move(size_t pos, int dir) {
    ensure_layout();
    float x = 0, y = 0;
    DWRITE_HIT_TEST_METRICS m{};
    layout_->HitTestTextPosition(static_cast<UINT32>(pos), FALSE, &x, &y, &m);
    DWRITE_TEXT_METRICS tm{};
    layout_->GetMetrics(&tm);
    if (dir < 0 && y < m.height * 0.5f) return 0;
    const float ty = dir < 0 ? y - m.height * 0.5f : y + m.height * 1.5f;
    if (dir > 0 && ty > tm.height) return text_.size();
    BOOL trailing = FALSE, inside = FALSE;
    DWRITE_HIT_TEST_METRICS h{};
    layout_->HitTestPoint(x, ty, &trailing, &inside, &h);
    return std::min(text_.size(), static_cast<size_t>(h.textPosition) + (trailing ? h.length : 0u));
}

void Edit::move(size_t to, bool extend) {
    caret_ = std::min(to, text_.size());
    if (!extend) anchor_ = caret_;
    last_kind_ = Kind::None;
    keep_caret_visible();
    touch();
}

void Edit::keep_caret_visible() {
    if (!root() || bounds.empty()) {
        return;
    }
    ensure_layout();
    DWRITE_TEXT_METRICS tm{};
    layout_->GetMetrics(&tm);
    float x = 0, y = 0;
    DWRITE_HIT_TEST_METRICS m{};
    layout_->HitTestTextPosition(static_cast<UINT32>(display_caret()), FALSE, &x, &y, &m);
    if (multiline) {
        scroll_x_ = 0;
        const float inner_h = std::max(1.0f, bounds.height() - 2 * kPadY);
        if (tm.height <= inner_h) {
            scroll_y_ = 0;
            return;
        }
        if (y < scroll_y_) {
            scroll_y_ = y;
        } else if (y + m.height > scroll_y_ + inner_h) {
            scroll_y_ = y + m.height - inner_h;
        }
        scroll_y_ = std::clamp(scroll_y_, 0.0f, tm.height - inner_h);
        return;
    }
    scroll_y_ = 0;
    const float inner = inner_width();
    if (tm.widthIncludingTrailingWhitespace <= inner) {
        scroll_x_ = 0;
        return;
    }
    if (x - scroll_x_ > inner - 1.0f) {
        scroll_x_ = x - (inner - 1.0f);
    } else if (x - scroll_x_ < 0) {
        scroll_x_ = x;
    }
    scroll_x_ = std::min(scroll_x_, tm.widthIncludingTrailingWhitespace - inner + 1.0f);
    scroll_x_ = std::max(scroll_x_, 0.0f);
}

void Edit::touch() {
    caret_on_ = true;
    invalidate();
    if (root() && focused() && root()->caret_blink) {
        root()->start_timer(this, 1, 530);
    }
}

void Edit::on_focus(bool f) {
    if (f) {
        touch();
    } else if (root()) {
        root()->stop_timer(this, 1);
    }
    invalidate();
}

void Edit::on_timer(int) {
    caret_on_ = !caret_on_;
    invalidate();
}

// ---- editing ---------------------------------------------------------------------------------------

void Edit::backspace() {
    if (caret_ == anchor_) {
        if (caret_ == 0) return;
        anchor_ = prev_cluster(caret_);
    }
    insert(L"", Kind::Deleting);
}

void Edit::del() {
    if (caret_ == anchor_) {
        if (caret_ == text_.size()) return;
        anchor_ = next_cluster(caret_);
    }
    insert(L"", Kind::Deleting);
}

void Edit::insert(const std::wstring& s, Kind kind) {
    if (read_only) return;
    if (kind != last_kind_ || kind == Kind::Other) {
        undo_.push_back({text_, anchor_, caret_});
        redo_.clear();
    }
    last_kind_ = kind;
    const auto [lo, hi] = selection();
    text_.replace(lo, hi - lo, s);
    caret_ = anchor_ = lo + s.size();
    dirty_ = true;
    keep_caret_visible();
    touch();
    if (on_changed) on_changed(text_);
}

void Edit::step(std::vector<Snapshot>& from, std::vector<Snapshot>& to) {
    if (from.empty()) return;
    to.push_back({text_, anchor_, caret_});
    const Snapshot s = from.back();
    from.pop_back();
    text_ = s.text;
    anchor_ = s.anchor;
    caret_ = s.caret;
    last_kind_ = Kind::None;
    dirty_ = true;
    keep_caret_visible();
    touch();
    if (on_changed) on_changed(text_);
}

void Edit::paste() {
    if (!root() || !root()->clipboard.read) return;
    const std::wstring s = root()->clipboard.read();
    std::wstring flat;
    for (size_t i = 0; i < s.size(); ++i) {
        if (s[i] == L'\r') {
            if (i + 1 < s.size() && s[i + 1] == L'\n') ++i;
            flat.push_back(multiline ? L'\n' : L' ');
        } else if (s[i] == L'\n') {
            flat.push_back(multiline ? L'\n' : L' ');
        } else {
            flat.push_back(s[i]);
        }
    }
    if (!flat.empty()) insert(flat, Kind::Other);
}

bool Edit::on_char(wchar_t c) {
    if (composing_ || read_only) {
        return false;
    }
    if (c == L'\b') {
        backspace();
        return true;
    }
    if (c < 0x20 || c == 0x7f) {
        return false;  // Ctrl+letter, Tab and Enter arrive here too; on_key_down dealt with them
    }
    if (IS_HIGH_SURROGATE(c)) {
        pending_high_ = c;
        return true;
    }
    std::wstring s;
    if (IS_LOW_SURROGATE(c) && pending_high_) {
        s.push_back(pending_high_);
    }
    pending_high_ = 0;
    s.push_back(c);
    insert(s, Kind::Typing);
    return true;
}

bool Edit::on_key_down(const KeyEvent& e) {
    if (composing_) {
        return false;
    }
    const bool shift = e.mods.shift, ctrl = e.mods.ctrl;
    const auto [lo, hi] = selection();
    switch (e.vk) {
        case VK_LEFT:
            if (!shift && lo != hi && !ctrl) move(lo, false);
            else move(ctrl ? word_left(caret_) : prev_cluster(caret_), shift);
            return true;
        case VK_RIGHT:
            if (!shift && lo != hi && !ctrl) move(hi, false);
            else move(ctrl ? word_right(caret_) : next_cluster(caret_), shift);
            return true;
        case VK_UP:
            if (!multiline) return false;
            move(vertical_move(caret_, -1), shift);
            return true;
        case VK_DOWN:
            if (!multiline) return false;
            move(vertical_move(caret_, 1), shift);
            return true;
        case VK_HOME:
            move(ctrl || !multiline ? 0 : line_edge(caret_, false), shift);
            return true;
        case VK_END:
            move(ctrl || !multiline ? text_.size() : line_edge(caret_, true), shift);
            return true;
        case VK_DELETE:
            del();
            return true;
        case VK_RETURN:
            if (multiline) {
                insert(L"\n", Kind::Other);
                return true;
            }
            if (on_commit) {
                on_commit();
                return true;
            }
            return false;
        case VK_ESCAPE:
            if (on_cancel) {
                on_cancel();
                return true;
            }
            return false;
        default:
            break;
    }
    if (!ctrl) {
        return false;
    }
    switch (e.vk) {
        case 'A':
            select(0, text_.size());
            return true;
        case 'C':
            if (root()->clipboard.write) root()->clipboard.write(selected_text());
            return true;
        case 'X':
            if (root()->clipboard.write) root()->clipboard.write(selected_text());
            if (caret_ != anchor_) insert(L"", Kind::Other);
            return true;
        case 'V':
            paste();
            return true;
        case 'Z':
            undo();
            return true;
        case 'Y':
            redo();
            return true;
        default:
            return false;
    }
}

// ---- mouse -----------------------------------------------------------------------------------------

bool Edit::on_mouse_down(const MouseEvent& e) {
    if (e.button != MouseButton::Left) return false;
    const size_t pos = position_at(e.pt);
    if (e.clicks >= 2) {
        size_t lo = pos, hi = pos;
        while (lo > 0 && text_[lo - 1] != L' ') --lo;
        while (hi < text_.size() && text_[hi] != L' ') ++hi;
        select(lo, hi);
    } else {
        move(pos, e.mods.shift);
    }
    selecting_ = true;
    root()->capture(this);
    return true;
}

bool Edit::on_mouse_move(const MouseEvent& e) {
    if (!selecting_) return false;
    move(position_at(e.pt), true);
    return true;
}

bool Edit::on_mouse_up(const MouseEvent&) {
    const bool was = selecting_;
    selecting_ = false;
    return was;
}

// ---- IME -------------------------------------------------------------------------------------------

void Edit::set_composition(const std::wstring& composition, size_t cursor) {
    if (!composing_) {
        if (caret_ != anchor_) insert(L"", Kind::Other);  // the IME replaces the selection
        composing_ = true;
    }
    composition_ = composition;
    composition_cursor_ = std::min(cursor, composition.size());
    dirty_ = true;
    keep_caret_visible();
    invalidate();
}

void Edit::commit_composition(const std::wstring& result) {
    composing_ = false;
    composition_.clear();
    composition_cursor_ = 0;
    dirty_ = true;
    if (!result.empty()) insert(result, Kind::Other);
    invalidate();
}

void Edit::end_composition() {
    composing_ = false;
    composition_.clear();
    composition_cursor_ = 0;
    dirty_ = true;
    keep_caret_visible();
    invalidate();
}

}  // namespace bb::ui
