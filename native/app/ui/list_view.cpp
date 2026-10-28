#include "ui/list_view.h"

#include <cmath>

namespace bb::ui {

namespace {
constexpr float kCellPad = 8.0f;
constexpr float kGrip = 4.0f;       // half-width of the resize hot zone at a column's right edge
constexpr float kDragStart = 4.0f;  // movement before a header press becomes a reorder
}  // namespace

ListView::ListView() : v_(true), h_(false) {
    flags.focusable = true;
    flags.tab_stop = true;
    flags.clip_children = true;
}

void ListView::set_provider(RowProvider* p) {
    provider_ = p;
    cursor_ = anchor_ = npos;
    v_.set_offset(0);
    h_.set_offset(0);
    refresh();
}

void ListView::set_columns(std::vector<Column> cols) {
    columns_ = std::move(cols);
    refresh();
}

void ListView::set_sort(int column_id, bool ascending) {
    sort_column_ = column_id;
    sort_ascending_ = ascending;
    invalidate();
}

void ListView::refresh() {
    layout_bars();
    const size_t n = count();
    if (cursor_ != npos && cursor_ >= n) cursor_ = npos;
    if (anchor_ != npos && anchor_ >= n) anchor_ = npos;
    invalidate();
}

Size ListView::measure(Size) { return fixed_or({0, 0}); }

void ListView::arrange(Rect r) {
    bounds = r;
    layout_bars();
}

float ListView::content_width() const {
    float w = 0;
    for (const Column& c : columns_) w += c.width;
    return w;
}

void ListView::layout_bars() {
    const float th = ScrollBar::kThickness;
    const float hh = show_header ? header_height : 0.0f;
    const float ch = static_cast<float>(count()) * row_height;
    const float cw = content_width();
    const float bw = bounds.width(), bh = std::max(0.0f, bounds.height() - hh);
    bool vn = ch > bh;
    float vw = bw - (vn ? th : 0.0f);
    bool hn = cw > vw;
    float vh = bh - (hn ? th : 0.0f);
    if (!vn && ch > vh) {
        vn = true;
        vw = bw - th;
        hn = cw > vw;
    }
    vw = std::max(0.0f, vw);
    vh = std::max(0.0f, vh);
    v_.extent = ch;
    v_.viewport = vh;
    h_.extent = cw;
    h_.viewport = vw;
    body_ = Rect::xywh(bounds.l, bounds.t + hh, vw, vh);
    v_.area = vn ? Rect{bounds.r - th, body_.t, bounds.r, body_.t + vh} : Rect{};
    h_.area = hn ? Rect{bounds.l, body_.b, bounds.l + vw, body_.b + th} : Rect{};
    v_.set_offset(v_.offset());
    h_.set_offset(h_.offset());
}

std::pair<size_t, size_t> ListView::visible_range() const {
    const size_t n = count();
    if (row_height <= 0 || n == 0) return {0, 0};
    const size_t first = std::min(n, static_cast<size_t>(std::floor(v_.offset() / row_height)));
    const size_t last = std::min(n, static_cast<size_t>(std::ceil((v_.offset() + v_.viewport) / row_height)));
    return {first, std::max(first, last)};
}

Rect ListView::header_rect() const {
    return show_header ? Rect::xywh(bounds.l, bounds.t, bounds.width(), header_height) : Rect{};
}

Rect ListView::row_rect(size_t row) const {
    const float y = body_.t + static_cast<float>(row) * row_height - v_.offset();
    return {body_.l, y, std::max(body_.r, body_.l - h_.offset() + content_width()), y + row_height};
}

Rect ListView::column_header_rect(size_t index) const {
    float x = bounds.l - h_.offset();
    for (size_t i = 0; i < index && i < columns_.size(); ++i) x += columns_[i].width;
    const float w = index < columns_.size() ? columns_[index].width : 0.0f;
    return {x, bounds.t, x + w, bounds.t + header_height};
}

size_t ListView::row_at(Point p) const {
    if (!body_.contains(p)) return npos;
    if (p.x >= body_.l - h_.offset() + content_width()) return npos;  // right of the columns: empty space
    const float y = p.y - body_.t + v_.offset();
    if (y < 0) return npos;
    const size_t r = static_cast<size_t>(y / row_height);
    return r < count() ? r : npos;
}

int ListView::resize_zone(Point p) const {
    if (!show_header || !header_rect().contains(p)) return -1;
    for (size_t i = 0; i < columns_.size(); ++i) {
        const float right = column_header_rect(i).r;
        if (p.x >= right - kGrip && p.x <= right + kGrip) return static_cast<int>(i);
    }
    return -1;
}

int ListView::header_index_at(float x) const {
    for (size_t i = 0; i < columns_.size(); ++i) {
        const Rect r = column_header_rect(i);
        if (x >= r.l && x < r.r) return static_cast<int>(i);
    }
    return -1;
}

size_t ListView::clamp_row(long long r) const {
    const long long n = static_cast<long long>(count());
    if (n == 0) return npos;
    return static_cast<size_t>(std::clamp(r, 0ll, n - 1));
}

Cursor ListView::cursor_at(Point p) const {
    return (resize_col_ >= 0 || resize_zone(p) >= 0) ? Cursor::SizeWE : Cursor::Arrow;
}

// ---- painting --------------------------------------------------------------------------------------

void ListView::paint(PaintContext& pc) {
    pc.fill(bounds, style.bg != kNoToken ? style.bg : tok::ListBackground);
    painted_rows_ = 0;
    if (provider_ && !columns_.empty()) {
        pc.clip(body_.intersect(pc.dirty));
        const auto [first, last] = visible_range();
        const float x0 = body_.l - h_.offset();
        for (size_t row = first; row < last; ++row) {
            const Rect rr = row_rect(row);
            if (!rr.intersects(pc.dirty)) continue;
            ++painted_rows_;
            const bool sel = provider_->selected(row);
            const uint64_t id = provider_->row_id(row);
            if (sel) {
                pc.fill(rr, focused() ? tok::ListSelectedBackground : tok::ListSelectedInactiveBackground);
            } else if (const Token tint = provider_->row_tint(row); tint != kNoToken) {
                pc.fill(rr, tint);
            }
            if (row == cursor_ && focused()) {
                pc.stroke(rr, tok::BorderFocus);
            }
            ID2D1SolidColorBrush* brush = pc.brush(sel ? tok::ListSelectedForeground : style.fg);
            float x = x0;
            bool first_column = true;
            for (const Column& col : columns_) {
                if (x + col.width > body_.l && x < body_.r) {
                    const std::wstring text = provider_->cell_text(row, col.id);
                    float icon_w = 0;
                    if (first_column) {
                        if (const IconData* icon = provider_->row_icon(row)) {
                            IconCache& cache = icon_cache_[icon];
                            Icon::draw(pc, *icon, Rect::xywh(x + kCellPad, rr.t + (row_height - 16.0f) / 2, 16.0f, 16.0f),
                                       sel ? tok::ListSelectedForeground : tok::TextSecondary, cache.geometry, cache.generation);
                            icon_w = 22.0f;
                        }
                    }
                    if (!text.empty()) {
                        const float w = std::max(1.0f, col.width - 2 * kCellPad - icon_w);
                        gfx::LayoutOptions opt;
                        opt.max_width = w;
                        opt.max_height = row_height;
                        opt.ellipsis = true;
                        opt.vcenter = true;
                        opt.align = col.right_align ? gfx::TextAlign::Trailing : gfx::TextAlign::Leading;
                        IDWriteTextLayout* layout = pc.res.layouts.get(
                            gfx::LayoutKey{id, static_cast<uint32_t>(col.id), w}, text, style.font, opt);
                        if (layout && brush) {
                            pc.dc->DrawTextLayout(D2D1::Point2F(x + kCellPad + icon_w, rr.t), layout, brush,
                                                  D2D1_DRAW_TEXT_OPTIONS_CLIP);
                        }
                    }
                }
                first_column = false;
                x += col.width;
            }
        }
        if (marquee_) {
            pc.fill(marquee_rect_, tok::MarqueeFill);
            pc.stroke(marquee_rect_, tok::AccentBackground);
        }
        pc.unclip();
    }
    paint_header(pc);
    v_.paint(pc);
    h_.paint(pc);
}

void ListView::paint_header(PaintContext& pc) {
    if (!show_header) return;
    const Rect hr = header_rect();
    pc.fill(hr, tok::ListHeaderBackground);
    pc.clip(hr.intersect(pc.dirty));
    for (size_t i = 0; i < columns_.size(); ++i) {
        const Column& col = columns_[i];
        const Rect rc = column_header_rect(i);
        if (rc.r < hr.l || rc.l > hr.r) continue;
        if (reordering_ && static_cast<int>(i) == press_col_) {
            pc.fill(rc, tok::ListHeaderHoverBackground);
        }
        const bool sorted = col.id == sort_column_;
        pc.text(col.title, Rect{rc.l + kCellPad, rc.t, rc.r - (sorted ? 22.0f : kCellPad), rc.b}, style.font,
                tok::ListHeaderForeground, col.right_align ? gfx::TextAlign::Trailing : gfx::TextAlign::Leading);
        if (sorted) {
            const float cx = rc.r - 12.0f, cy = (rc.t + rc.b) / 2;
            ComPtr<ID2D1PathGeometry> geo;
            if (SUCCEEDED(pc.res.dev.d2d_factory()->CreatePathGeometry(&geo))) {
                ComPtr<ID2D1GeometrySink> sink;
                geo->Open(&sink);
                const float base = sort_ascending_ ? cy + 2.0f : cy - 2.0f;  // ascending points up
                const float apex = sort_ascending_ ? cy - 2.0f : cy + 2.0f;
                sink->BeginFigure(D2D1::Point2F(cx - 4, base), D2D1_FIGURE_BEGIN_FILLED);
                sink->AddLine(D2D1::Point2F(cx + 4, base));
                sink->AddLine(D2D1::Point2F(cx, apex));
                sink->EndFigure(D2D1_FIGURE_END_CLOSED);
                sink->Close();
                if (ID2D1SolidColorBrush* b = pc.brush(tok::ListHeaderForeground)) pc.dc->FillGeometry(geo.Get(), b);
            }
        }
        pc.line({rc.r - 0.5f, rc.t + 4}, {rc.r - 0.5f, rc.b - 4}, tok::ListHeaderBorder);
    }
    pc.line({hr.l, hr.b - 0.5f}, {hr.r, hr.b - 0.5f}, tok::ListHeaderBorder);
    if (reordering_ && drop_index_ >= 0) {
        const size_t di = static_cast<size_t>(drop_index_);
        const float x = di < columns_.size() ? column_header_rect(di).l : column_header_rect(columns_.size() - 1).r;
        pc.fill(Rect{x - 1, hr.t, x + 1, hr.b}, tok::AccentBackground);
    }
    pc.unclip();
}

// ---- selection and cursor --------------------------------------------------------------------------

void ListView::scroll_to_row(size_t row) {
    const float top = static_cast<float>(row) * row_height;
    float y = v_.offset();
    if (top < y) {
        y = top;
    } else if (top + row_height > y + v_.viewport) {
        y = top + row_height - v_.viewport;
    }
    set_scroll_y(y);
}

void ListView::set_scroll_y(float y) {
    const float before = v_.offset();
    v_.set_offset(y);
    if (before != v_.offset()) invalidate();
}

void ListView::select_row(size_t row) {
    if (!provider_ || row >= count()) return;
    provider_->select(SelectOp::Replace, row, row);
    cursor_ = anchor_ = row;
    scroll_to_row(row);
    invalidate();
}

void ListView::move_cursor(size_t row, const Mods& m) {
    if (!provider_ || row == npos) return;
    if (m.shift) {
        if (anchor_ == npos) anchor_ = cursor_ == npos ? row : cursor_;
        provider_->select(m.ctrl ? SelectOp::Add : SelectOp::Replace, std::min(anchor_, row), std::max(anchor_, row));
    } else if (m.ctrl) {
        // Ctrl+arrow moves the cursor and leaves the selection alone.
    } else {
        provider_->select(SelectOp::Replace, row, row);
        anchor_ = row;
    }
    cursor_ = row;
    scroll_to_row(row);
    invalidate();
}

// ---- mouse -----------------------------------------------------------------------------------------

bool ListView::on_mouse_down(const MouseEvent& e) {
    if (e.button == MouseButton::Middle) return false;
    if (e.button == MouseButton::Left && (v_.mouse_down(e.pt) || h_.mouse_down(e.pt))) {
        bar_drag_ = true;
        root()->capture(this);
        invalidate();
        return true;
    }
    if (show_header && header_rect().contains(e.pt)) {
        if (e.button == MouseButton::Right && on_header_menu) on_header_menu(e.pt);
        if (e.button != MouseButton::Left) return true;
        const int zone = resize_zone(e.pt);
        if (zone >= 0) {
            resize_col_ = zone;
            resize_x_ = e.pt.x;
            resize_w_ = columns_[static_cast<size_t>(zone)].width;
        } else {
            press_col_ = header_index_at(e.pt.x);
            press_x_ = e.pt.x;
            reordering_ = false;
        }
        root()->capture(this);
        return true;
    }
    if (!provider_ || !body_.contains(e.pt)) return false;
    const size_t row = row_at(e.pt);
    if (row != npos) {
        if (e.button == MouseButton::Right && provider_->selected(row)) {
            cursor_ = row;  // a right click on the selection keeps it
        } else if (e.button == MouseButton::Right) {
            provider_->select(SelectOp::Replace, row, row);
            anchor_ = cursor_ = row;
        } else if (e.mods.shift && anchor_ != npos) {
            provider_->select(e.mods.ctrl ? SelectOp::Add : SelectOp::Replace, std::min(anchor_, row),
                              std::max(anchor_, row));
            cursor_ = row;
        } else if (e.mods.ctrl) {
            provider_->select(SelectOp::Toggle, row, row);
            anchor_ = cursor_ = row;
        } else {
            provider_->select(SelectOp::Replace, row, row);
            anchor_ = cursor_ = row;
        }
        if (e.clicks >= 2 && e.button == MouseButton::Left && on_activate) on_activate(row);
        if (e.button == MouseButton::Right && on_context) on_context(e.pt, row);
        invalidate();
        return true;
    }
    if (e.button == MouseButton::Right) {
        if (on_context) on_context(e.pt, npos);
        return true;
    }
    if (e.button != MouseButton::Left) return true;
    // Empty space: a marquee.
    marquee_ = true;
    marquee_add_ = e.mods.ctrl;
    marquee_origin_ = {e.pt.x - (body_.l - h_.offset()), e.pt.y - (body_.t - v_.offset())};
    marquee_first_ = marquee_last_ = npos;
    marquee_rect_ = Rect{e.pt.x, e.pt.y, e.pt.x, e.pt.y};
    if (!marquee_add_) provider_->select(SelectOp::Clear, 0, 0);
    root()->capture(this);
    invalidate();
    return true;
}

void ListView::update_marquee(Point p) {
    const float ox = body_.l - h_.offset(), oy = body_.t - v_.offset();
    const float cx = p.x - ox, cy = p.y - oy;
    const Rect m{std::min(marquee_origin_.x, cx), std::min(marquee_origin_.y, cy), std::max(marquee_origin_.x, cx),
                 std::max(marquee_origin_.y, cy)};
    marquee_rect_ = m.offset(ox, oy);
    const size_t n = count();
    size_t a = npos, b = npos;
    if (n > 0 && m.b >= 0 && m.t < static_cast<float>(n) * row_height && m.r >= 0 && m.l <= content_width()) {
        a = static_cast<size_t>(std::max(0.0f, m.t) / row_height);
        b = std::min(n - 1, static_cast<size_t>(std::max(0.0f, m.b) / row_height));
    }
    if (a != marquee_first_ || b != marquee_last_) {
        marquee_first_ = a;
        marquee_last_ = b;
        if (a == npos) {
            if (!marquee_add_) provider_->select(SelectOp::Clear, 0, 0);
        } else {
            provider_->select(marquee_add_ ? SelectOp::Add : SelectOp::Replace, a, b);  // one bulk call
            anchor_ = cursor_ = a;
        }
    }
    invalidate();
}

bool ListView::on_mouse_move(const MouseEvent& e) {
    if (bar_drag_) {
        v_.mouse_move(e.pt);
        h_.mouse_move(e.pt);
        invalidate();
        return true;
    }
    if (resize_col_ >= 0) {
        Column& c = columns_[static_cast<size_t>(resize_col_)];
        c.width = std::max(c.min_width, resize_w_ + (e.pt.x - resize_x_));
        layout_bars();
        invalidate();
        return true;
    }
    if (press_col_ >= 0) {
        if (!reordering_ && std::abs(e.pt.x - press_x_) > kDragStart) reordering_ = true;
        if (reordering_) {
            drop_index_ = static_cast<int>(columns_.size());
            for (size_t i = 0; i < columns_.size(); ++i) {
                const Rect r = column_header_rect(i);
                if (e.pt.x < (r.l + r.r) / 2) {
                    drop_index_ = static_cast<int>(i);
                    break;
                }
            }
            invalidate(header_rect());
        }
        return true;
    }
    if (marquee_) {
        if (e.pt.y < body_.t) {
            set_scroll_y(v_.offset() - row_height);
        } else if (e.pt.y > body_.b) {
            set_scroll_y(v_.offset() + row_height);
        }
        update_marquee(e.pt);
        return true;
    }
    return false;
}

bool ListView::on_mouse_up(const MouseEvent& e) {
    bool handled = false;
    if (bar_drag_) {
        v_.mouse_up();
        h_.mouse_up();
        bar_drag_ = false;
        handled = true;
    }
    if (resize_col_ >= 0) {
        resize_col_ = -1;
        if (on_columns_changed) on_columns_changed();
        handled = true;
    }
    if (press_col_ >= 0) {
        const size_t from = static_cast<size_t>(press_col_);
        if (reordering_ && drop_index_ >= 0) {
            size_t to = static_cast<size_t>(drop_index_);
            if (to > from) --to;
            if (to != from) {
                Column moved = std::move(columns_[from]);
                columns_.erase(columns_.begin() + static_cast<std::ptrdiff_t>(from));
                columns_.insert(columns_.begin() + static_cast<std::ptrdiff_t>(to), std::move(moved));
                if (on_columns_changed) on_columns_changed();
            }
        } else if (!reordering_ && columns_[from].sortable && header_index_at(e.pt.x) == press_col_) {
            const int id = columns_[from].id;
            const bool asc = id == sort_column_ ? !sort_ascending_ : true;
            set_sort(id, asc);
            if (on_sort) on_sort(id, asc);
        }
        press_col_ = -1;
        reordering_ = false;
        drop_index_ = -1;
        handled = true;
    }
    if (marquee_) {
        marquee_ = false;
        handled = true;
    }
    if (handled) invalidate();
    return handled;
}

bool ListView::on_wheel(Point, int delta, Mods m) {
    const float step = -static_cast<float>(delta) / 120.0f * row_height * 3.0f;
    if (m.shift && h_.needed()) {
        h_.set_offset(h_.offset() + step);
        invalidate();
        return true;
    }
    if (!v_.needed()) return false;
    set_scroll_y(v_.offset() + step);
    return true;
}

// ---- keyboard --------------------------------------------------------------------------------------

bool ListView::on_key_down(const KeyEvent& e) {
    if (!provider_) return false;
    const size_t n = count();
    const size_t page = std::max<size_t>(1, static_cast<size_t>(v_.viewport / row_height));
    const long long cur = cursor_ == npos ? -1 : static_cast<long long>(cursor_);
    switch (e.vk) {
        case VK_DOWN:
            move_cursor(clamp_row(cur + 1), e.mods);
            return true;
        case VK_UP:
            move_cursor(clamp_row(cur < 0 ? 0 : cur - 1), e.mods);
            return true;
        case VK_NEXT:
            move_cursor(clamp_row(cur < 0 ? 0 : cur + static_cast<long long>(page)), e.mods);
            return true;
        case VK_PRIOR:
            move_cursor(clamp_row(cur < 0 ? 0 : cur - static_cast<long long>(page)), e.mods);
            return true;
        case VK_HOME:
            move_cursor(clamp_row(0), e.mods);
            return true;
        case VK_END:
            move_cursor(clamp_row(static_cast<long long>(n) - 1), e.mods);
            return true;
        case VK_LEFT:
        case VK_RIGHT:
            if (!h_.needed()) return false;
            h_.set_offset(h_.offset() + (e.vk == VK_LEFT ? -40.0f : 40.0f));
            invalidate();
            return true;
        case VK_RETURN:
            if (cursor_ == npos || !on_activate) return false;
            on_activate(cursor_);
            return true;
        case VK_SPACE:
            if (e.mods.ctrl && cursor_ != npos) {
                provider_->select(SelectOp::Toggle, cursor_, cursor_);
                anchor_ = cursor_;
                invalidate();
                return true;
            }
            return false;
        case 'A':
            if (e.mods.ctrl && n > 0) {
                provider_->select(SelectOp::Replace, 0, n - 1);
                invalidate();
                return true;
            }
            return false;
        default:
            return false;
    }
}

bool ListView::on_char(wchar_t c) {
    if (c < 0x20 || c == 0x7f || !on_typeahead) return false;
    const uint64_t now = root()->now_ms();
    if (now - typed_at_ > kTypeAheadMs) typed_.clear();
    if (c == L' ' && typed_.empty()) return false;
    typed_.push_back(c);
    typed_at_ = now;
    on_typeahead(typed_);
    return true;
}

}  // namespace bb::ui
