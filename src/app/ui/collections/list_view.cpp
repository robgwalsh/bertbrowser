#include "ui/collections/list_view.h"

#include <cmath>

namespace bb::ui {

namespace {
constexpr float kCellPad = 8.0f;
constexpr float kGrip = 4.0f;       // half-width of the resize hot zone at a column's right edge
constexpr float kDragStart = 4.0f;  // movement before a header press becomes a reorder
constexpr int kMarqueeTimer = 0x4D51;
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

size_t ListView::find_row(uint64_t id, size_t hint) const {
    const size_t n = count();
    if (!provider_ || n == 0) return npos;
    // Near where it was first: a merge moves a row by a few places, not across the list.
    const size_t lo = hint > 64 ? hint - 64 : 0, hi = std::min(n, hint + 65);
    for (size_t i = lo; i < hi; ++i) {
        if (provider_->row_id(i) == id) return i;
    }
    for (size_t i = 0; i < n; ++i) {
        if (provider_->row_id(i) == id) return i;
    }
    return npos;
}

void ListView::note_ids() {
    const size_t n = count();
    cursor_known_ = provider_ && cursor_ != npos && cursor_ < n;
    if (cursor_known_) cursor_id_ = provider_->row_id(cursor_);
    anchor_known_ = provider_ && anchor_ != npos && anchor_ < n;
    if (anchor_known_) anchor_id_ = provider_->row_id(anchor_);
}

void ListView::refresh() {
    if (cursor_ != npos) cursor_ = cursor_known_ ? find_row(cursor_id_, cursor_) : npos;
    if (anchor_ != npos) anchor_ = anchor_known_ ? find_row(anchor_id_, anchor_) : npos;
    note_ids();
    layout_bars();
    invalidate();
}

Size ListView::measure(Size) { return fixed_or({0, 0}); }

void ListView::arrange(Rect r) {
    bounds = r;
    layout_bars();
    if (reveal_pending_) {
        reveal_pending_ = false;
        if (cursor_ != npos) scroll_to_row(cursor_);
    }
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
    note_ids();
    pc.fill(bounds, style.bg != kNoToken ? style.bg : tok::ListBackground);
    painted_rows_ = 0;
    if (provider_ && !columns_.empty()) {
        pc.clip(body_.intersect(pc.dirty));
        const auto [first, last] = visible_range();
        const float x0 = body_.l - h_.offset();
        const uint32_t icon_px = device_px(pc, 16.0f);
        for (size_t row = first; row < last; ++row) {
            const Rect rr = row_rect(row);
            if (!rr.intersects(pc.dirty)) continue;
            ++painted_rows_;
            const bool sel = provider_->selected(row);
            const uint64_t id = provider_->row_id(row);
            // A comparison's tint is the floor; hover and selection both stand above it. Spec: Styles.xaml
            // (FileListItem: the tint first, then IsMouseOver and IsSelected).
            if (const Token tint = provider_->row_tint(row); tint != kNoToken) pc.fill(rr, tint);
            if (sel) {
                pc.fill(rr, !focused() ? tok::ListSelectedInactiveBackground
                            : row == hot_row_ ? tok::ListSelectedHoverBackground
                                              : tok::ListSelectedBackground);
            } else if (row == hot_row_) {
                pc.fill(rr, tok::ListHoverBackground);
            }
            if (row == drop_row) {
                pc.fill(rr, tok::ListDropBackground);
                pc.stroke(rr, tok::ListDropBorder, 2.0f);
            }
            if (row == cursor_ && focused()) {
                pc.stroke(rr, tok::BorderFocus);
            }
            const Token row_fg = sel ? (focused() ? tok::ListSelectedForeground : tok::ListSelectedInactiveForeground) : style.fg;
            ID2D1SolidColorBrush* brush = pc.brush(row_fg);
            float x = x0;
            bool first_column = true;
            for (const Column& col : columns_) {
                if (x + col.width > body_.l && x < body_.r) {
                    const std::wstring text = provider_->cell_text(row, col.id);
                    float icon_w = 0;
                    if (first_column) {
                        const Rect slot = Rect::xywh(x + kCellPad, rr.t + (row_height - 16.0f) / 2, 16.0f, 16.0f);
                        bool faded = false;
                        if (const ThumbPtr image = provider_->row_image(row, icon_px, &faded)) {
                            images_.draw(pc, image, slot, faded ? 0.5f : 1.0f);
                            icon_w = 22.0f;
                        } else if (const IconData* icon = provider_->row_icon(row)) {
                            IconCache& cache = icon_cache_[icon];
                            Icon::draw(pc, *icon, slot,
                                       sel ? tok::ListSelectedForeground : tok::TextSecondary, cache.geometry, cache.generation);
                            icon_w = 22.0f;
                        }
                    }
                    RowProvider::CellMarks marks;
                    const bool marked = !text.empty() && provider_->cell_marks(row, col.id, &marks);
                    if (marked && !marks.lead.empty()) {
                        // The line number, quieter, then the line. Spec: Styles.xaml (Column.Cell.Match).
                        const float lw = std::ceil(pc.res.text.measure(marks.lead + L" ", style.font).width);
                        pc.text(marks.lead, Rect{x + kCellPad + icon_w, rr.t, x + col.width - kCellPad, rr.b}, style.font,
                                sel ? row_fg : tok::TextSecondary);
                        icon_w += lw;
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
                        const D2D1_POINT_2F at = D2D1::Point2F(x + kCellPad + icon_w, rr.t);
                        if (layout && marked && marks.length > 0) {
                            // The needle on the selection colour, in semibold.
                            const DWRITE_TEXT_RANGE range{marks.start, marks.length};
                            layout->SetFontWeight(DWRITE_FONT_WEIGHT_SEMI_BOLD, range);
                            UINT32 n = 0;
                            layout->HitTestTextRange(marks.start, marks.length, 0, 0, nullptr, 0, &n);
                            std::vector<DWRITE_HIT_TEST_METRICS> hits(n);
                            if (n && SUCCEEDED(layout->HitTestTextRange(marks.start, marks.length, 0, 0, hits.data(), n, &n))) {
                                for (const auto& m : hits) {
                                    const Rect hr = Rect::xywh(at.x + m.left, at.y + m.top, m.width, m.height).intersect(
                                        Rect{x + kCellPad, rr.t, x + col.width - kCellPad, rr.b});
                                    if (!hr.empty()) pc.fill(hr, tok::InputSelectionBackground);
                                }
                            }
                        }
                        ID2D1SolidColorBrush* cell_brush = !sel && col.fg != kNoToken ? pc.brush(col.fg) : brush;
                        if (layout && cell_brush) pc.dc->DrawTextLayout(at, layout, cell_brush, D2D1_DRAW_TEXT_OPTIONS_CLIP);
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
        // Lit under the pointer and while pressed, the pressed one's title in the primary colour. Spec:
        // Lists.xaml (GridViewColumnHeader: 11 semibold, IsMouseOver, IsPressed).
        const bool pressed = static_cast<int>(i) == press_col_;
        if (pressed || (static_cast<int>(i) == hot_header_ && resize_col_ < 0 && press_col_ < 0)) {
            pc.fill(rc, tok::ListHeaderHoverBackground);
        }
        const bool sorted = col.id == sort_column_;
        gfx::FontSpec head = style.font;
        head.size = 11.0f;
        head.weight = 600;
        pc.text(col.title, Rect{rc.l + kCellPad, rc.t, rc.r - (sorted ? 22.0f : kCellPad), rc.b}, head,
                pressed && !reordering_ ? tok::TextPrimary : tok::ListHeaderForeground,
                col.right_align ? gfx::TextAlign::Trailing : gfx::TextAlign::Leading);
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

void ListView::reveal_cursor(size_t row) {
    if (row >= count()) return;
    anchor_ = cursor_ = row;
    scroll_to_row(row);
    invalidate();
}

void ListView::reveal_cursor_when_arranged(size_t row) {
    if (row >= count()) return;
    reveal_cursor(row);
    reveal_pending_ = true;
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
    const NoteIds noted{this};
    if (e.button == MouseButton::Middle) {
        const size_t row = (show_header && header_rect().contains(e.pt)) ? npos : row_at(e.pt);
        if (row == npos || !on_middle) return false;
        on_middle(row);
        return true;
    }
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
        if (zone >= 0 && e.clicks >= 2) {
            auto_fit(static_cast<size_t>(zone));
            return true;
        }
        if (zone >= 0) {
            resize_col_ = zone;
            resize_x_ = e.pt.x;
            resize_w_ = columns_[static_cast<size_t>(zone)].width;
        } else {
            press_col_ = header_index_at(e.pt.x);
            press_x_ = e.pt.x;
            reordering_ = false;
            invalidate(header_rect());
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
        } else if (on_drag_start && e.button == MouseButton::Left && e.clicks < 2 && provider_->selected(row)) {
            // Pressing on the selection must not narrow it yet: it may be about to be dragged whole.
            deferred_row_ = row;
            anchor_ = cursor_ = row;
        } else {
            provider_->select(SelectOp::Replace, row, row);
            anchor_ = cursor_ = row;
        }
        if (e.clicks >= 2 && e.button == MouseButton::Left && on_activate) on_activate(row);
        const bool may_drag = on_drag_start && e.clicks < 2 && (e.button == MouseButton::Left || e.button == MouseButton::Right);
        if (may_drag) {
            drag_armed_ = true;
            drag_origin_ = e.pt;
            drag_button_ = e.button;
            root()->capture(this);
        }
        if (e.button == MouseButton::Right && on_context) {
            if (may_drag) {
                // The menu opens on release, once it is known the press was not the start of a drag.
                context_pending_ = true;
                context_point_ = e.pt;
                context_row_ = row;
            } else {
                on_context(e.pt, row);
            }
        }
        invalidate();
        return true;
    }
    if (e.button == MouseButton::Right) {
        if (on_context) on_context(e.pt, npos);
        return true;
    }
    if (e.button != MouseButton::Left) return true;
    // Empty space: a marquee. Ctrl or Shift keeps what was selected and adds the band to it.
    // Spec: MarqueeSelector (additive, SyncSelection, the 50 ms auto-scroll).
    marquee_ = true;
    marquee_add_ = e.mods.ctrl || e.mods.shift;
    marquee_origin_ = {e.pt.x - (body_.l - h_.offset()), e.pt.y - (body_.t - v_.offset())};
    marquee_first_ = marquee_last_ = npos;
    marquee_rect_ = Rect{e.pt.x, e.pt.y, e.pt.x, e.pt.y};
    marquee_before_.clear();
    if (marquee_add_) {
        for (size_t i = 0, n = count(); i < n; ++i) {
            if (provider_->selected(i)) marquee_before_.push_back(i);
        }
    } else {
        provider_->select(SelectOp::Clear, 0, 0);
    }
    root()->capture(this);
    root()->start_timer(this, kMarqueeTimer, 50);
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
        if (marquee_add_) {
            // What was selected before, and the band: a row swept and then unswept goes back to how it was.
            std::vector<size_t> rows = marquee_before_;
            if (a != npos) {
                for (size_t i = a; i <= b; ++i) rows.push_back(i);
                anchor_ = cursor_ = a;
            }
            std::sort(rows.begin(), rows.end());
            rows.erase(std::unique(rows.begin(), rows.end()), rows.end());
            provider_->select_rows(rows);  // one bulk call
        } else if (a == npos) {
            provider_->select(SelectOp::Clear, 0, 0);
        } else {
            provider_->select(SelectOp::Replace, a, b);  // one bulk call
            anchor_ = cursor_ = a;
        }
    }
    invalidate();
}

bool ListView::on_mouse_move(const MouseEvent& e) {
    if (drag_armed_) {
        if (std::abs(e.pt.x - drag_origin_.x) > kDragStart * 2 || std::abs(e.pt.y - drag_origin_.y) > kDragStart * 2) {
            drag_armed_ = false;
            context_pending_ = false;
            deferred_row_ = npos;
            const MouseButton button = drag_button_;
            root()->release_capture();
            // The drag runs OLE's own loop and returns when the button is released.
            if (on_drag_start) on_drag_start(button);
        }
        return true;
    }
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
        update_marquee(e.pt);
        return true;
    }
    // The header under the pointer is lit, and so is the row, as a list row is.
    const int header = show_header && header_rect().contains(e.pt) && resize_zone(e.pt) < 0 ? header_index_at(e.pt.x) : -1;
    if (header != hot_header_) {
        hot_header_ = header;
        invalidate(header_rect());
    }
    const size_t hot = provider_ && body_.contains(e.pt) ? row_at(e.pt) : npos;
    const int hot_col = hot != npos ? header_index_at(e.pt.x) : -1;
    if (hot != hot_row_) {
        if (hot_row_ != npos && hot_row_ < count()) invalidate(row_rect(hot_row_));
        hot_row_ = hot;
        if (hot != npos) invalidate(row_rect(hot));
    }
    if (hot != tip_row_ || hot_col != tip_col_) {
        tip_row_ = hot;
        tip_col_ = hot_col;
        std::wstring tip;
        if (hot != npos && hot_col >= 0 && static_cast<size_t>(hot_col) < columns_.size()) tip = provider_->cell_tip(hot, columns_[static_cast<size_t>(hot_col)].id);
        if (tip.empty() && hot != npos) tip = provider_->row_tip(hot);
        tooltip = std::move(tip);
    }
    return false;
}

// A double click on a column's gripper sizes it to what it shows: its title and the rows on screen, as a
// WPF grid column does. Spec: DirectoryTabView.Header_DragCompleted (reads the width the auto-fit left).
void ListView::auto_fit(size_t index) {
    if (index >= columns_.size()) return;
    Column& col = columns_[index];
    float want = res().text.measure(col.title, style.font).width + 2 * kCellPad + 14.0f;  // room for a sort arrow
    if (provider_) {
        const auto [first, last] = visible_range();
        const bool first_column = index == 0;
        for (size_t row = first; row < last && row < count(); ++row) {
            const std::wstring text = provider_->cell_text(row, col.id);
            float w = text.empty() ? 0.0f : res().text.measure(text, style.font).width;
            if (first_column) {
                bool faded = false;
                if (provider_->row_image(row, 16, &faded) || provider_->row_icon(row)) w += 22.0f;
            }
            want = std::max(want, w + 2 * kCellPad + 2.0f);
        }
    }
    col.width = std::max(col.min_width, std::ceil(want));
    layout_bars();
    invalidate();
    if (on_columns_changed) on_columns_changed();
}

void ListView::on_mouse_leave() {
    if (hot_header_ >= 0) {
        hot_header_ = -1;
        invalidate(header_rect());
    }
    if (hot_row_ == npos) return;
    if (hot_row_ < count()) invalidate(row_rect(hot_row_));
    hot_row_ = npos;
}

// While the band is held near or past an edge the list scrolls on its own, faster the further out the
// pointer is, and the band follows. Spec: MarqueeSelector.AutoScroll.
void ListView::on_timer(int id) {
    if (id != kMarqueeTimer) return;
    if (!marquee_ || !root()) {
        if (root()) root()->stop_timer(this, kMarqueeTimer);
        return;
    }
    const Point p = root()->pointer();
    constexpr float kMargin = 20.0f;
    const float over = p.y < body_.t + kMargin ? p.y - (body_.t + kMargin) : p.y > body_.b - kMargin ? p.y - (body_.b - kMargin) : 0.0f;
    if (over == 0.0f) return;
    const float lines = std::clamp(std::floor(std::fabs(over) / 12.0f), 1.0f, 6.0f);
    set_scroll_y(v_.offset() + (over < 0 ? -lines : lines) * row_height);
    update_marquee(p);
}

bool ListView::on_mouse_up(const MouseEvent& e) {
    bool handled = false;
    if (drag_armed_) {
        // A press that turned out to be a plain click: the selection change held back applies now, and so
        // does the menu a right press asked for.
        drag_armed_ = false;
        root()->release_capture();
        if (deferred_row_ != npos && provider_) {
            provider_->select(SelectOp::Replace, deferred_row_, deferred_row_);
            deferred_row_ = npos;
        }
        if (context_pending_ && on_context) {
            context_pending_ = false;
            on_context(context_point_, context_row_);
        }
        invalidate();
        return true;
    }
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
        marquee_before_.clear();
        root()->stop_timer(this, kMarqueeTimer);
        handled = true;
    }
    if (handled) invalidate();
    return handled;
}

bool ListView::on_wheel(Point, int delta, Mods m) {
    const float step = m.shift ? -static_cast<float>(delta) / 120.0f * row_height * 3.0f : wheel_rows(delta) * row_height;
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
    const NoteIds noted{this};
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
            // Plain Space on a row the cursor reached with Ctrl+arrow selects it, as an Extended ListBox does. On
            // a selected row it is left to type-ahead, where it is part of a name.
            if (!e.mods.ctrl && !e.mods.shift && !e.mods.alt && cursor_ != npos && cursor_ < n && !provider_->selected(cursor_)) {
                provider_->select(SelectOp::Replace, cursor_, cursor_);
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

// The Apps key / Shift+F10: the menu of the row the cursor is on (scrolled into view first), or the
// empty-space menu when the cursor is not on the selection. Spec: ListView.ContextMenu opens at the focused item.
bool ListView::on_context_key() {
    if (!provider_ || !on_context) return false;
    // The menu is the selection's: at the cursor when it is on it, else at the first selected row (a Ctrl+arrow
    // can leave the cursor off the selection). Nothing selected is the folder's own menu.
    size_t at = cursor_ != npos && cursor_ < count() && provider_->selected(cursor_) ? cursor_ : npos;
    for (size_t i = 0; at == npos && i < count(); ++i) {
        if (provider_->selected(i)) at = i;
    }
    if (at == npos) {
        on_context(Point{body_.l + 24.0f, body_.t + row_height * 0.5f}, npos);
        return true;
    }
    scroll_to_row(at);
    const Rect r = row_rect(at);
    on_context(Point{r.l + 24.0f, (r.t + r.b) * 0.5f}, at);
    return true;
}

bool ListView::on_char(wchar_t c) {
    const NoteIds noted{this};
    if (c < 0x20 || c == 0x7f || !on_typeahead) return false;
    // A character typed with Ctrl or Alt held (AltGr counts as both) is not part of a name.
    // Spec: DirectoryTabView.FileList_TextInput.
    if (root()->mods().ctrl || root()->mods().alt) return false;
    const uint64_t now = root()->now_ms();
    if (now - typed_at_ > kTypeAheadMs) typed_.clear();
    if (c == L' ' && typed_.empty()) return false;
    typed_.push_back(c);
    typed_at_ = now;
    on_typeahead(typed_);
    return true;
}

}  // namespace bb::ui
