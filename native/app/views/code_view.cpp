#include "views/code_view.h"

#include <windows.h>

#include <algorithm>

namespace bb::views {

using namespace ui;

namespace {

Token class_token(uint8_t cls) {
    switch (cls) {
        case 1: return tok::SyntaxKeyword;
        case 2: return tok::SyntaxString;
        case 3: return tok::SyntaxComment;
        case 4: return tok::SyntaxNumber;
        case 5: return tok::SyntaxPunctuation;
        default: return kNoToken;
    }
}

}  // namespace

CodeView::CodeView() {
    flags.focusable = true;
    flags.clip_children = true;
    style.bg = tok::SurfaceBackground;
}

void CodeView::set_content(std::vector<std::wstring> lines, std::vector<Span> spans, bool numbers) {
    lines_ = std::move(lines);
    spans_ = std::move(spans);
    numbers_ = numbers;
    span_begin_.assign(lines_.size() + 1, spans_.size());
    for (size_t i = spans_.size(); i-- > 0;) span_begin_[spans_[i].line] = i;
    for (size_t l = lines_.size(); l-- > 0;) span_begin_[l] = std::min(span_begin_[l], span_begin_[l + 1]);
    anchor_ = caret_ = {};
    wrapped_for_ = -1;
    v_.set_offset(0);
    h_.set_offset(0);
    relayout();
    request_layout();
    invalidate();
}

void CodeView::set_wrap(bool wrap) {
    if (wrap_ == wrap) return;
    wrap_ = wrap;
    wrapped_for_ = -1;
    h_.set_offset(0);
    relayout();
    request_layout();
    invalidate();
}

void CodeView::scroll_to_top() {
    v_.set_offset(0);
    h_.set_offset(0);
    invalidate();
}

std::wstring CodeView::all_text() const {
    std::wstring out;
    for (size_t i = 0; i < lines_.size(); ++i) {
        if (i) out += L'\n';
        out += lines_[i];
    }
    return out;
}

std::wstring CodeView::selected_text() const {
    if (!has_selection() || lines_.empty()) return {};
    Pos a = anchor_, b = caret_;
    if (b < a) std::swap(a, b);
    std::wstring out;
    for (size_t l = a.line; l <= b.line && l < lines_.size(); ++l) {
        const std::wstring& s = lines_[l];
        const size_t from = l == a.line ? std::min(a.col, s.size()) : 0;
        const size_t to = l == b.line ? std::min(b.col, s.size()) : s.size();
        if (l != a.line) out += L'\n';
        out += s.substr(from, to > from ? to - from : 0);
    }
    return out;
}

void CodeView::select_all() {
    if (lines_.empty()) return;
    anchor_ = {0, 0};
    caret_ = {lines_.size() - 1, lines_.back().size()};
    invalidate();
}

void CodeView::ensure_metrics() {
    if (metrics_ready_ || !root()) return;
    const auto one = res().text.measure(L"0", font_);
    const auto tall = res().text.measure(L"Ag", font_);
    char_w_ = std::max(1.0f, one.width);
    line_h_ = std::max(8.0f, std::ceil(tall.height) + 1.0f);
    metrics_ready_ = true;
}

float CodeView::gutter_width() const {
    if (!gutter_shown()) return 0;
    size_t digits = 1;
    for (size_t n = lines_.size(); n >= 10; n /= 10) ++digits;
    return static_cast<float>(digits) * char_w_ + 16.0f;
}

float CodeView::desired_width() {
    ensure_metrics();
    size_t widest = 0;
    for (const auto& l : lines_) widest = std::max(widest, l.size());
    return gutter_width() + static_cast<float>(widest) * char_w_ + 2 * kPadX + ScrollBar::kThickness;
}

size_t CodeView::line_at(float y) const {
    if (lines_.empty()) return 0;
    if (!wrap_) return std::min(lines_.size() - 1, static_cast<size_t>(std::max(0.0f, std::floor(y / line_h_))));
    const auto it = std::upper_bound(tops_.begin(), tops_.end(), y);
    const size_t at = it == tops_.begin() ? 0 : static_cast<size_t>(it - tops_.begin()) - 1;
    return std::min(lines_.size() - 1, at);
}

void CodeView::relayout() {
    ensure_metrics();
    const float th = ScrollBar::kThickness;
    const size_t n = lines_.size();
    size_t widest = 0;
    for (const auto& l : lines_) widest = std::max(widest, l.size());
    widest_ = static_cast<float>(widest) * char_w_ + 2 * kPadX;

    const float bw = bounds.width(), bh = bounds.height();
    float gw = gutter_width();
    float content_h = static_cast<float>(n) * line_h_ + 2 * kPadY;
    bool vn = false, hn = false;

    auto wrapped_height = [&](float avail) {
        if (wrapped_for_ == avail && tops_.size() == n + 1) return tops_.back() + 2 * kPadY;
        tops_.assign(n + 1, 0.0f);
        float y = 0;
        for (size_t i = 0; i < n; ++i) {
            tops_[i] = y;
            gfx::LayoutOptions opt;
            opt.wrap = true;
            opt.max_width = std::max(1.0f, avail - 2 * kPadX);
            auto layout = res().text.make_layout(lines_[i].empty() ? L" " : lines_[i], font_, opt);
            DWRITE_TEXT_METRICS m{};
            float h = line_h_;
            if (layout && SUCCEEDED(layout->GetMetrics(&m))) h = std::max(line_h_, m.height);
            y += h;
        }
        tops_[n] = y;
        wrapped_for_ = avail;
        return y + 2 * kPadY;
    };

    if (wrap_ && root()) {
        gw = 0;
        content_h = wrapped_height(bw - th);
        vn = content_h > bh;
        if (!vn) content_h = wrapped_height(bw);
    } else {
        vn = content_h > bh;
        hn = widest_ > bw - gw - (vn ? th : 0.0f);
        if (!vn && hn && content_h > bh - th) vn = true;
    }
    const float vw = bw - gw - (vn ? th : 0.0f);
    const float vh = bh - (hn ? th : 0.0f);
    text_ = Rect{bounds.l + gw, bounds.t, bounds.l + gw + std::max(0.0f, vw), bounds.t + std::max(0.0f, vh)};
    gutter_ = Rect{bounds.l, bounds.t, bounds.l + gw, text_.b};
    v_.extent = content_h;
    v_.viewport = text_.height();
    h_.extent = wrap_ ? 0 : widest_;
    h_.viewport = text_.width();
    v_.area = vn ? Rect{bounds.r - th, bounds.t, bounds.r, text_.b} : Rect{};
    h_.area = hn ? Rect{text_.l, text_.b, text_.r, bounds.b} : Rect{};
    v_.set_offset(v_.offset());
    h_.set_offset(h_.offset());
}

void CodeView::arrange(Rect r) {
    bounds = r;
    wrapped_for_ = -1;
    relayout();
}

ui::ComPtr<IDWriteTextLayout> CodeView::layout_for(size_t line) {
    gfx::LayoutOptions opt;
    opt.wrap = wrap_;
    opt.max_width = wrap_ ? std::max(1.0f, text_.width() - 2 * kPadX) : 1.0e6f;
    return res().text.make_layout(lines_[line].empty() ? L" " : lines_[line], font_, opt);
}

void CodeView::paint(PaintContext& pc) {
    ensure_metrics();
    Widget::paint(pc);
    painted_ = 0;
    if (lines_.empty()) return;

    const float top = v_.offset();
    const size_t first = line_at(std::max(0.0f, top - kPadY));
    const float view_h = text_.height();

    // The gutter: numbers right-aligned against a rule, drawn here rather than laid out beside the text.
    if (gutter_shown() && !gutter_.empty()) {
        pc.clip(gutter_);
        for (size_t i = first; i < lines_.size(); ++i) {
            const float y = text_.t + kPadY + line_top(i) - top;
            if (y > text_.t + view_h) break;
            const std::wstring number = std::to_wstring(i + 1);
            pc.text(number, Rect::xywh(gutter_.l, y, gutter_.width() - 8.0f, line_h_), font_, tok::TextMuted, gfx::TextAlign::Trailing);
        }
        pc.unclip();
        pc.fill(Rect{gutter_.r - 1.0f, gutter_.t, gutter_.r, gutter_.b}, tok::BorderSubtle);
    }

    pc.clip(text_);
    Pos lo = anchor_, hi = caret_;
    if (hi < lo) std::swap(lo, hi);
    for (size_t i = first; i < lines_.size(); ++i) {
        const float y = text_.t + kPadY + line_top(i) - top;
        if (y > text_.t + view_h) break;
        auto layout = layout_for(i);
        if (!layout) continue;
        const float x = text_.l + kPadX - h_.offset();

        if (has_selection() && i >= lo.line && i <= hi.line) {
            const size_t len = lines_[i].size();
            const size_t from = i == lo.line ? std::min(lo.col, len) : 0;
            const size_t to = i == hi.line ? std::min(hi.col, len) : len;
            if (to > from) {
                UINT32 count = 0;
                layout->HitTestTextRange(static_cast<UINT32>(from), static_cast<UINT32>(to - from), 0, 0, nullptr, 0, &count);
                std::vector<DWRITE_HIT_TEST_METRICS> boxes(count);
                if (count && SUCCEEDED(layout->HitTestTextRange(static_cast<UINT32>(from), static_cast<UINT32>(to - from), 0, 0,
                                                                  boxes.data(), count, &count))) {
                    for (UINT32 b = 0; b < count; ++b) {
                        pc.fill(Rect::xywh(x + boxes[b].left, y + boxes[b].top, boxes[b].width, boxes[b].height), tok::InputSelectionBackground);
                    }
                }
            } else if (i != hi.line && i != lo.line) {
                pc.fill(Rect::xywh(x, y, char_w_ * 0.5f, line_h_), tok::InputSelectionBackground);
            }
        }

        for (size_t s = span_begin_[i]; s < span_begin_[i + 1] && s < spans_.size() && spans_[s].line == i; ++s) {
            const Token t = class_token(spans_[s].cls);
            if (t == kNoToken) continue;
            layout->SetDrawingEffect(pc.brush(t), DWRITE_TEXT_RANGE{spans_[s].start, spans_[s].length});
        }
        pc.dc->DrawTextLayout(D2D1::Point2F(x, y), layout.Get(), pc.brush(tok::TextPrimary), D2D1_DRAW_TEXT_OPTIONS_NONE);
        ++painted_;
    }
    pc.unclip();
    v_.paint(pc);
    h_.paint(pc);
}

CodeView::Pos CodeView::hit(Point p) {
    Pos out;
    if (lines_.empty()) return out;
    const float cy = p.y - text_.t - kPadY + v_.offset();
    out.line = line_at(cy);
    auto layout = layout_for(out.line);
    if (!layout) return out;
    BOOL trailing = FALSE, inside = FALSE;
    DWRITE_HIT_TEST_METRICS m{};
    const float lx = p.x - text_.l - kPadX + h_.offset();
    const float ly = cy - line_top(out.line);
    if (SUCCEEDED(layout->HitTestPoint(lx, ly, &trailing, &inside, &m))) {
        out.col = std::min(lines_[out.line].size(), static_cast<size_t>(m.textPosition) + (trailing ? m.length : 0));
    }
    return out;
}

void CodeView::keep_in_view(Pos p) {
    const float top = line_top(p.line), bottom = top + line_height(p.line) + 2 * kPadY;
    if (top < v_.offset()) v_.set_offset(top);
    else if (bottom > v_.offset() + v_.viewport) v_.set_offset(bottom - v_.viewport);
}

bool CodeView::on_mouse_down(const MouseEvent& e) {
    if (e.button != MouseButton::Left) return false;
    if (v_.mouse_down(e.pt) || h_.mouse_down(e.pt)) {
        bar_drag_ = true;
        root()->capture(this);
        invalidate();
        return true;
    }
    root()->set_focus(this);
    if (!text_.contains(e.pt)) return true;
    caret_ = hit(e.pt);
    if (e.clicks >= 2 && caret_.line < lines_.size()) {
        // A double click takes the line, the way a code view usually does for a triple.
        anchor_ = {caret_.line, 0};
        caret_ = {caret_.line, lines_[caret_.line].size()};
    } else if (!e.mods.shift) {
        anchor_ = caret_;
    }
    selecting_ = true;
    root()->capture(this);
    invalidate();
    return true;
}

bool CodeView::on_mouse_move(const MouseEvent& e) {
    if (bar_drag_) {
        v_.mouse_move(e.pt);
        h_.mouse_move(e.pt);
        invalidate();
        return true;
    }
    if (!selecting_) return false;
    if (e.pt.y < text_.t) v_.set_offset(v_.offset() - line_h_);
    else if (e.pt.y > text_.b) v_.set_offset(v_.offset() + line_h_);
    caret_ = hit(e.pt);
    invalidate();
    return true;
}

bool CodeView::on_mouse_up(const MouseEvent&) {
    if (!selecting_ && !bar_drag_) return false;
    selecting_ = false;
    bar_drag_ = false;
    v_.mouse_up();
    h_.mouse_up();
    root()->release_capture();
    invalidate();
    return true;
}

bool CodeView::on_wheel(Point, int delta, Mods m) {
    const float step = -static_cast<float>(delta) / 120.0f * line_h_ * 3.0f;
    if (m.shift && h_.needed()) {
        h_.set_offset(h_.offset() + step);
    } else if (v_.needed()) {
        v_.set_offset(v_.offset() + step);
    } else {
        return false;
    }
    invalidate();
    return true;
}

void CodeView::copy_selection() {
    const std::wstring text = selected_text();
    if (!text.empty() && root() && root()->clipboard.write) root()->clipboard.write(text);
}

bool CodeView::on_key_down(const KeyEvent& e) {
    if (e.mods.ctrl && (e.vk == 'C' || e.vk == VK_INSERT)) {
        copy_selection();
        return true;
    }
    if (e.mods.ctrl && e.vk == 'A') {
        select_all();
        return true;
    }
    const float page = std::max(line_h_, v_.viewport - line_h_);
    switch (e.vk) {
        case VK_DOWN: v_.set_offset(v_.offset() + line_h_); break;
        case VK_UP: v_.set_offset(v_.offset() - line_h_); break;
        case VK_NEXT: v_.set_offset(v_.offset() + page); break;
        case VK_PRIOR: v_.set_offset(v_.offset() - page); break;
        case VK_HOME: e.mods.ctrl ? v_.set_offset(0) : h_.set_offset(0); break;
        case VK_END: if (e.mods.ctrl) v_.set_offset(v_.max_offset()); break;
        case VK_RIGHT: h_.set_offset(h_.offset() + char_w_ * 4); break;
        case VK_LEFT: h_.set_offset(h_.offset() - char_w_ * 4); break;
        default: return false;
    }
    invalidate();
    return true;
}

Cursor CodeView::cursor_at(Point p) const { return text_.contains(p) ? Cursor::IBeam : Cursor::Arrow; }

}  // namespace bb::views
