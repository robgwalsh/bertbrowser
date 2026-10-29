#include "ui/tree_view.h"

#include <cmath>

namespace bb::ui {

TreeView::TreeView() : v_(true) {
    flags.focusable = true;
    flags.tab_stop = true;
    flags.clip_children = true;
}

void TreeView::set_provider(TreeProvider* p) {
    provider_ = p;
    expanded_.clear();
    selected_ = 0;
    v_.set_offset(0);
    refresh();
}

void TreeView::refresh() {
    rebuild();
    layout_bar();
    invalidate();
}

void TreeView::rebuild() {
    rows_.clear();
    if (!provider_) return;
    // Depth-first over expanded nodes only; the stack holds (node, depth, parent row, next child index).
    struct Frame {
        uint64_t node;
        int depth;
        size_t parent;
        size_t next;
        size_t count;
    };
    std::vector<Frame> stack;
    stack.push_back({0, 0, npos, 0, provider_->child_count(0)});
    while (!stack.empty()) {
        Frame& f = stack.back();
        if (f.next >= f.count) {
            stack.pop_back();
            continue;
        }
        const uint64_t id = provider_->child_id(f.node, f.next++);
        const int depth = f.depth;
        const size_t parent = f.parent;
        const bool has = provider_->has_children(id);
        rows_.push_back({id, depth, parent, has});
        if (has && expanded_.count(id)) {
            stack.push_back({id, depth + 1, rows_.size() - 1, 0, provider_->child_count(id)});  // invalidates f
        }
    }
}

void TreeView::layout_bar() {
    const float th = ScrollBar::kThickness;
    const float ch = static_cast<float>(rows_.size()) * row_height;
    const bool needed = ch > bounds.height();
    v_.extent = ch;
    v_.viewport = bounds.height();
    v_.area = needed ? Rect{bounds.r - th, bounds.t, bounds.r, bounds.b} : Rect{};
    v_.set_offset(v_.offset());
}

Size TreeView::measure(Size) { return fixed_or({0, 0}); }

void TreeView::arrange(Rect r) {
    bounds = r;
    layout_bar();
    // A reveal asked for before there was a viewport to place it in (the first folder at startup).
    if (pending_place_ && v_.viewport > 0) {
        const auto [id, fraction] = *pending_place_;
        pending_place_.reset();
        place(id, fraction);
    }
}

std::pair<size_t, size_t> TreeView::visible_range() const {
    const size_t n = rows_.size();
    const size_t first = std::min(n, static_cast<size_t>(std::floor(v_.offset() / row_height)));
    const size_t last = std::min(n, static_cast<size_t>(std::ceil((v_.offset() + v_.viewport) / row_height)));
    return {first, std::max(first, last)};
}

size_t TreeView::row_of(uint64_t id) const {
    for (size_t i = 0; i < rows_.size(); ++i) {
        if (rows_[i].id == id) return i;
    }
    return npos;
}

Rect TreeView::row_rect(size_t row) const {
    const float y = bounds.t + static_cast<float>(row) * row_height - v_.offset();
    return {bounds.l, y, v_.needed() ? bounds.r - ScrollBar::kThickness : bounds.r, y + row_height};
}

Rect TreeView::chevron_rect(size_t row) const {
    const Rect rr = row_rect(row);
    const float x = rr.l + 4.0f + static_cast<float>(rows_[row].depth) * indent;
    return {x, rr.t, x + 16.0f, rr.b};
}

size_t TreeView::row_at(Point p) const {
    if (!bounds.contains(p) || (v_.needed() && v_.area.contains(p))) return npos;
    const float y = p.y - bounds.t + v_.offset();
    if (y < 0) return npos;
    const size_t r = static_cast<size_t>(y / row_height);
    return r < rows_.size() ? r : npos;
}

void TreeView::set_scroll_y(float y) {
    const float before = v_.offset();
    v_.set_offset(y);
    if (before != v_.offset()) invalidate();
}

void TreeView::paint(PaintContext& pc) {
    pc.fill(bounds, style.bg != kNoToken ? style.bg : tok::ListBackground);
    painted_rows_ = 0;
    pc.clip(bounds.intersect(pc.dirty));
    const auto [first, last] = visible_range();
    const uint32_t icon_px = device_px(pc, 16.0f);
    for (size_t i = first; i < last; ++i) {
        const Rect rr = row_rect(i);
        if (!rr.intersects(pc.dirty)) continue;
        ++painted_rows_;
        const Entry& e = rows_[i];
        const bool sel = e.id == selected_;
        if (sel) pc.fill(rr, focused() ? tok::ListSelectedBackground : tok::ListSelectedInactiveBackground);
        if (drop_id != 0 && e.id == drop_id) {
            pc.fill(rr, tok::ListDropBackground);
            pc.stroke(rr, tok::ListDropBorder, 2.0f);
        }
        const Rect cv = chevron_rect(i);
        if (e.has_children) {
            const float cx = cv.l + 8.0f, cy = (cv.t + cv.b) / 2;
            const Token t = sel ? tok::ListSelectedForeground : tok::TreeChevronForeground;
            if (expanded_.count(e.id)) {
                pc.line({cx - 4, cy - 2}, {cx, cy + 2}, t, 1.3f);
                pc.line({cx, cy + 2}, {cx + 4, cy - 2}, t, 1.3f);
            } else {
                pc.line({cx - 2, cy - 4}, {cx + 2, cy}, t, 1.3f);
                pc.line({cx + 2, cy}, {cx - 2, cy + 4}, t, 1.3f);
            }
        }
        float text_l = cv.r + 2.0f;
        if (const ThumbPtr image = provider_->icon(e.id, icon_px)) {
            images_.draw(pc, image, Rect::xywh(text_l, rr.t + (row_height - 16.0f) / 2, 16.0f, 16.0f),
                         provider_->dimmed(e.id) ? 0.5f : 1.0f);
            text_l += 22.0f;
        }
        // A size at the right edge, muted; the name gives way to it rather than running under it.
        float text_r = rr.r - 4.0f;
        const std::wstring detail = provider_->detail(e.id);
        if (!detail.empty()) {
            gfx::FontSpec detail_font = style.font;
            detail_font.size = std::max(9.0f, style.font.size - 1.5f);
            const float w = std::ceil(pc.res.text.measure(detail, detail_font).width);
            const Rect dr{std::max(text_l + 40.0f, rr.r - 8.0f - w), rr.t, rr.r - 8.0f, rr.b};
            pc.text(detail, dr, detail_font, sel ? tok::ListSelectedForeground : tok::TextMuted, gfx::TextAlign::Trailing);
            text_r = dr.l - 6.0f;
        }
        pc.text(provider_->label(e.id), Rect{text_l, rr.t, text_r, rr.b}, style.font,
                sel ? tok::ListSelectedForeground : style.fg);
        if (sel && focused()) pc.stroke(rr, tok::BorderFocus);
    }
    pc.unclip();
    v_.paint(pc);
}

// ---- state changes ---------------------------------------------------------------------------------

void TreeView::expand(uint64_t id) {
    if (!provider_ || expanded_.count(id)) return;
    expanded_.insert(id);
    if (on_toggle) on_toggle(id, true);
    refresh();
}

void TreeView::collapse(uint64_t id) {
    if (!expanded_.erase(id)) return;
    if (on_toggle) on_toggle(id, false);
    refresh();
}

void TreeView::toggle(uint64_t id) {
    if (is_expanded(id)) {
        collapse(id);
    } else {
        expand(id);
    }
}

void TreeView::select_row(size_t row) {
    if (row >= rows_.size()) return;
    selected_ = rows_[row].id;
    const float top = static_cast<float>(row) * row_height;
    float y = v_.offset();
    if (top < y) {
        y = top;
    } else if (top + row_height > y + v_.viewport) {
        y = top + row_height - v_.viewport;
    }
    v_.set_offset(y);
    invalidate();
    if (on_select) on_select(selected_);
}

void TreeView::place(uint64_t id, float fraction) {
    const size_t r = row_of(id);
    if (r == npos) return;
    if (v_.viewport <= 0) {
        pending_place_ = std::make_pair(id, fraction);
        return;
    }
    v_.set_offset(static_cast<float>(r) * row_height - v_.viewport * fraction);
    invalidate();
}

void TreeView::select(uint64_t id) {
    const size_t r = row_of(id);
    if (r == npos) {
        selected_ = id;
        invalidate();
        return;
    }
    select_row(r);
}

// ---- input -----------------------------------------------------------------------------------------

bool TreeView::on_mouse_down(const MouseEvent& e) {
    if (e.button == MouseButton::Left && v_.mouse_down(e.pt)) {
        bar_drag_ = true;
        root()->capture(this);
        invalidate();
        return true;
    }
    const size_t row = row_at(e.pt);
    if (row == npos) return false;
    const Entry en = rows_[row];  // a copy: toggling rebuilds rows_
    if (e.button == MouseButton::Right) {
        if (on_context) on_context(en.id, e.pt);
        return true;
    }
    if (e.button == MouseButton::Middle) {
        if (on_middle) on_middle(en.id);
        return true;
    }
    if (e.button == MouseButton::Left && en.has_children && chevron_rect(row).contains(e.pt)) {
        toggle(en.id);
        return true;
    }
    select_row(row);
    if (e.button == MouseButton::Left && e.clicks >= 2) {
        if (en.has_children) toggle(en.id);
        if (on_activate) on_activate(en.id);
    }
    return true;
}

bool TreeView::on_mouse_move(const MouseEvent& e) {
    if (!bar_drag_) {
        const size_t row = row_at(e.pt);
        const uint64_t id = row != npos ? id_at_row(row) : 0;
        if (id != tip_id_) {
            tip_id_ = id;
            tooltip = id && provider_ ? provider_->tip(id) : std::wstring();
        }
        return false;
    }
    v_.mouse_move(e.pt);
    invalidate();
    return true;
}

bool TreeView::on_mouse_up(const MouseEvent&) {
    if (!bar_drag_) return false;
    bar_drag_ = false;
    v_.mouse_up();
    invalidate();
    return true;
}

bool TreeView::on_wheel(Point, int delta, Mods) {
    if (!v_.needed()) return false;
    set_scroll_y(v_.offset() + wheel_rows(delta) * row_height);
    return true;
}

bool TreeView::on_key_down(const KeyEvent& e) {
    if (rows_.empty()) return false;
    size_t cur = row_of(selected_);
    const size_t n = rows_.size();
    const size_t page = std::max<size_t>(1, static_cast<size_t>(v_.viewport / row_height));
    switch (e.vk) {
        case VK_DOWN:
            select_row(cur == npos ? 0 : std::min(n - 1, cur + 1));
            return true;
        case VK_UP:
            select_row(cur == npos ? 0 : (cur > 0 ? cur - 1 : 0));
            return true;
        case VK_NEXT:
            select_row(cur == npos ? 0 : std::min(n - 1, cur + page));
            return true;
        case VK_PRIOR:
            select_row(cur == npos ? 0 : (cur > page ? cur - page : 0));
            return true;
        case VK_HOME:
            select_row(0);
            return true;
        case VK_END:
            select_row(n - 1);
            return true;
        case VK_RIGHT:
            if (cur == npos) return false;
            if (rows_[cur].has_children && !is_expanded(rows_[cur].id)) {
                expand(rows_[cur].id);
            } else if (cur + 1 < n && rows_[cur + 1].parent == cur) {
                select_row(cur + 1);
            }
            return true;
        case VK_LEFT:
            if (cur == npos) return false;
            if (is_expanded(rows_[cur].id)) {
                collapse(rows_[cur].id);
            } else if (rows_[cur].parent != npos) {
                select_row(rows_[cur].parent);
            }
            return true;
        case VK_RETURN:
            if (cur == npos) return false;
            if (rows_[cur].has_children) toggle(rows_[cur].id);
            if (on_activate) on_activate(selected_);
            return true;
        default:
            return false;
    }
}

}  // namespace bb::ui
