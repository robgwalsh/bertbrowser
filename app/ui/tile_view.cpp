#include "ui/tile_view.h"

#include <algorithm>
#include <cmath>
#include <unordered_set>

namespace bb::ui {

// ---- TileLayout -------------------------------------------------------------------------------------

int TileLayout::columns() const {
    const float avail = width - 2 * m.margin;
    return std::max(1, static_cast<int>(std::floor((avail + m.gap) / (m.tile_w() + m.gap))));
}

size_t TileLayout::rows() const {
    const size_t c = static_cast<size_t>(columns());
    return (tiles() + c - 1) / c;
}

float TileLayout::content_height() const {
    if (count == 0) return 0;
    const size_t r = rows();
    const float grid = r == 0 ? 0.0f : static_cast<float>(r) * m.tile_h() + static_cast<float>(r - 1) * m.gap;
    return grid_top() + grid + m.margin - (r == 0 && leading > 0 ? m.gap : 0.0f);
}

Rect TileLayout::tile_rect(size_t index) const {
    if (index < leading) return Rect::xywh(m.margin, m.margin + static_cast<float>(index) * row_h, std::max(0.0f, width - 2 * m.margin), row_h);
    const size_t i = index - leading;
    const size_t c = static_cast<size_t>(columns());
    const float x = m.margin + static_cast<float>(i % c) * (m.tile_w() + m.gap);
    const float y = grid_top() + static_cast<float>(i / c) * (m.tile_h() + m.gap);
    return Rect::xywh(x, y, m.tile_w(), m.tile_h());
}

std::pair<size_t, size_t> TileLayout::visible(float scroll, float viewport_h) const {
    if (count == 0 || viewport_h <= 0) return {0, 0};
    const float bottom_edge = scroll + viewport_h;
    size_t first = static_cast<size_t>(-1), last = 0;
    // The rows above the grid.
    if (leading > 0 && bottom_edge > m.margin) {
        const long long r0 = std::max(0LL, static_cast<long long>(std::floor((scroll - m.margin) / row_h)));
        const long long r1 = std::min(static_cast<long long>(leading) - 1, static_cast<long long>(std::floor((bottom_edge - m.margin - 0.001f) / row_h)));
        if (r1 >= r0) {
            first = static_cast<size_t>(r0);
            last = static_cast<size_t>(r1) + 1;
        }
    }
    // The grid.
    const float g = grid_top();
    const float top = scroll - g, bottom = bottom_edge - g;
    if (tiles() > 0 && bottom > 0) {
        const float pitch = m.tile_h() + m.gap;
        const size_t c = static_cast<size_t>(columns());
        const long long r0 = std::max(0LL, static_cast<long long>(std::floor(top / pitch)));
        const long long r1 = std::min(static_cast<long long>(rows()) - 1, static_cast<long long>(std::floor((bottom - 0.001f) / pitch)));
        if (r1 >= r0) {
            if (first == static_cast<size_t>(-1)) first = leading + static_cast<size_t>(r0) * c;
            last = std::min(count, leading + (static_cast<size_t>(r1) + 1) * c);
        }
    }
    if (first == static_cast<size_t>(-1)) return {0, 0};
    return {first, last};
}

size_t TileLayout::index_at(Point p) const {
    if (leading > 0 && p.y >= m.margin && p.y < m.margin + static_cast<float>(leading) * row_h) {
        if (p.x < m.margin || p.x >= width - m.margin) return static_cast<size_t>(-1);
        return static_cast<size_t>(std::floor((p.y - m.margin) / row_h));
    }
    const float x = p.x - m.margin, y = p.y - grid_top();
    if (x < 0 || y < 0) return static_cast<size_t>(-1);
    const float px = m.tile_w() + m.gap, py = m.tile_h() + m.gap;
    const size_t col = static_cast<size_t>(std::floor(x / px)), row = static_cast<size_t>(std::floor(y / py));
    if (x - static_cast<float>(col) * px >= m.tile_w() || y - static_cast<float>(row) * py >= m.tile_h()) return static_cast<size_t>(-1);
    if (col >= static_cast<size_t>(columns())) return static_cast<size_t>(-1);
    const size_t i = leading + row * static_cast<size_t>(columns()) + col;
    return i < count ? i : static_cast<size_t>(-1);
}

Rect TileLayout::image_rect(size_t index) const {
    const Rect t = tile_rect(index);
    return Rect::xywh(t.l + m.pad, t.t + m.pad, m.image, m.slot_h());
}

Rect TileLayout::caption_rect(size_t index) const {
    const Rect t = tile_rect(index);
    return Rect::xywh(t.l + m.pad, t.t + m.pad + m.slot_h() + 4, m.image, m.caption_h);
}

// ---- TileView ---------------------------------------------------------------------------------------

TileView::TileView() : v_(true) {
    flags.focusable = true;
    flags.tab_stop = true;
    flags.clip_children = true;
}

void TileView::set_provider(RowProvider* p) {
    provider_ = p;
    cursor_ = anchor_ = npos;
    v_.set_offset(0);
    refresh();
}

void TileView::set_thumbnails(ThumbnailProvider* t) {
    if (thumbs_ && thumbs_ != t) {
        for (auto& [id, e] : cache_) {
            if (e.state == Entry::State::Requested) thumbs_->cancel(id);
        }
    }
    cache_.clear();
    in_flight_ = 0;
    thumbs_ = t;
    update_requests();
    invalidate();
}

size_t TileView::find_row(uint64_t id, size_t hint) const {
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

void TileView::note_ids() {
    const size_t n = count();
    cursor_known_ = provider_ && cursor_ != npos && cursor_ < n;
    if (cursor_known_) cursor_id_ = provider_->row_id(cursor_);
    anchor_known_ = provider_ && anchor_ != npos && anchor_ < n;
    if (anchor_known_) anchor_id_ = provider_->row_id(anchor_);
}

void TileView::refresh() {
    if (cursor_ != npos) cursor_ = cursor_known_ ? find_row(cursor_id_, cursor_) : npos;
    if (anchor_ != npos) anchor_ = anchor_known_ ? find_row(anchor_id_, anchor_) : npos;
    note_ids();
    // The rows above the grid: the sort puts everything that is not a picture first.
    leading_ = 0;
    const size_t n = count();
    while (provider_ && leading_ < n && !provider_->row_is_tile(leading_)) ++leading_;
    layout_bar();
    update_requests();
    invalidate();
}

Size TileView::measure(Size) { return fixed_or({0, 0}); }

void TileView::arrange(Rect r) {
    bounds = r;
    layout_bar();
    update_requests();
}

void TileView::layout_bar() {
    body_ = bounds;
    TileLayout t{metrics, bounds.width(), count()};
    float ch = t.content_height();
    bool bar = ch > bounds.height();
    if (bar) {
        t.width -= ScrollBar::kThickness;
        ch = t.content_height();
    }
    v_.extent = ch;
    v_.viewport = bounds.height();
    v_.area = bar ? Rect{bounds.r - ScrollBar::kThickness, bounds.t, bounds.r, bounds.b} : Rect{};
    v_.set_offset(v_.offset());
}

TileLayout TileView::layout() const {
    const float w = v_.needed() ? bounds.width() - ScrollBar::kThickness : bounds.width();
    TileLayout lay{metrics, w, count()};
    lay.leading = std::min(leading_, lay.count);
    return lay;
}

Rect TileView::tile_rect(size_t row) const { return layout().tile_rect(row).offset(bounds.l, bounds.t - v_.offset()); }

size_t TileView::row_at(Point p) const {
    if (!bounds.contains(p) || (v_.needed() && v_.area.contains(p))) return npos;
    return layout().index_at({p.x - bounds.l, p.y - bounds.t + v_.offset()});
}

std::pair<size_t, size_t> TileView::realized() const { return layout().visible(v_.offset(), bounds.height()); }

size_t TileView::ready_count() const {
    size_t n = 0;
    for (const auto& [id, e] : cache_) n += e.state == Entry::State::Ready;
    return n;
}

bool TileView::has_image(uint64_t row_id) const {
    const auto it = cache_.find(row_id);
    return it != cache_.end() && it->second.state == Entry::State::Ready;
}

// ---- thumbnails -------------------------------------------------------------------------------------

void TileView::update_requests() {
    if (!thumbs_ || !provider_) {
        cache_.clear();
        in_flight_ = 0;
        return;
    }
    if (updating_) return;  // a provider answering inside request() must not re-enter us
    updating_ = true;
    const TileLayout lay = layout();
    const auto [first, last] = lay.visible(v_.offset(), bounds.height());
    // What stays: the realized rows, plus whole rows of tiles beyond them if asked.
    const size_t cols = static_cast<size_t>(lay.columns());
    const size_t extra = static_cast<size_t>(std::max(0, retain_extra_rows)) * cols;
    const size_t kf = first > extra ? first - extra : 0;
    const size_t kl = std::min(count(), last + extra);
    std::unordered_set<uint64_t> keep;
    for (size_t r = kf; r < kl; ++r) keep.insert(provider_->row_id(r));
    for (auto it = cache_.begin(); it != cache_.end();) {
        if (keep.count(it->first)) {
            ++it;
            continue;
        }
        if (it->second.state == Entry::State::Requested) {
            thumbs_->cancel(it->first);
            if (in_flight_ > 0) --in_flight_;
        }
        it = cache_.erase(it);
    }
    const uint32_t px = static_cast<uint32_t>(std::lround(std::max(metrics.image, metrics.slot_h()) * dpi_scale));
    for (size_t r = std::max(first, lay.leading); r < last; ++r) {
        const uint64_t id = provider_->row_id(r);
        const auto it = cache_.find(id);
        if (it != cache_.end()) {
            it->second.row = r;
            continue;
        }
        if (in_flight_ >= max_in_flight) break;
        Entry e;
        e.row = r;
        cache_.emplace(id, std::move(e));
        ++in_flight_;
        thumbs_->request(id, r, px, *this);
    }
    updating_ = false;
}

void TileView::deliver(uint64_t row_id, ThumbPtr image) {
    const auto it = cache_.find(row_id);
    if (it == cache_.end() || it->second.state != Entry::State::Requested) return;  // scrolled away, or a repeat
    Entry& e = it->second;
    e.state = image ? Entry::State::Ready : Entry::State::None;
    e.image = std::move(image);
    e.bitmap.Reset();
    if (in_flight_ > 0) --in_flight_;
    invalidate(tile_rect(e.row));
    update_requests();  // a slot is free: ask for the next missing tile
}

// ---- painting ---------------------------------------------------------------------------------------

void TileView::paint(PaintContext& pc) {
    note_ids();
    pc.fill(bounds, style.bg != kNoToken ? style.bg : tok::ListBackground);
    painted_ = 0;
    if (!provider_) return;
    pc.clip(bounds);
    const TileLayout lay = layout();
    const auto [first, last] = lay.visible(v_.offset(), bounds.height());
    const Point ptr = root() ? root()->pointer() : Point{};
    const bool active = focused();
    const uint32_t placeholder_px = device_px(pc, kPlaceholderIcon);
    const gfx::LayoutOptions cap_opt = [&] {
        gfx::LayoutOptions o;
        o.wrap = true;
        o.max_width = metrics.image;
        o.max_height = metrics.caption_h;
        o.align = gfx::TextAlign::Center;
        o.ellipsis = true;  // two lines, then the ellipsis
        return o;
    }();
    for (size_t row = first; row < last; ++row) {
        const Rect tr = lay.tile_rect(row).offset(bounds.l, bounds.t - v_.offset());
        if (!tr.intersects(pc.dirty)) continue;
        ++painted_;
        const bool sel = provider_->selected(row);
        const uint64_t id = provider_->row_id(row);
        const bool hot = hovered() && tr.contains(ptr);
        // A comparison's tint first, so hover and selection both win over it. Spec: Styles.xaml (ThumbItemStyle).
        if (const Token tint = provider_->row_tint(row); tint != kNoToken) pc.fill(tr, tint);
        if (sel) {
            pc.fill(tr, !active ? tok::ListSelectedInactiveBackground : hot ? tok::ListSelectedHoverBackground : tok::ListSelectedBackground);
        } else if (hot) {
            pc.fill(tr, tok::ListHoverBackground);
        }
        const Token fg = sel ? (active ? tok::ListSelectedForeground : tok::ListSelectedInactiveForeground) : tok::TextPrimary;
        if (lay.is_row(row)) {
            paint_row(pc, row, tr, fg, sel);
            if (active && row == cursor_) pc.stroke(tr, tok::BorderFocus);
            if (row == drop_row) {
                pc.fill(tr, tok::ListDropBackground);
                pc.stroke(tr, tok::ListDropBorder, 2.0f);
            }
            continue;
        }
        const Rect slot = lay.image_rect(row).offset(bounds.l, bounds.t - v_.offset());
        pc.fill(slot, tok::ThumbnailTileBackground);
        const auto it = cache_.find(id);
        if (it != cache_.end() && it->second.state == Entry::State::Ready && it->second.image) {
            Entry& e = it->second;
            if (!e.bitmap || e.generation != pc.res.dev.generation()) {
                const D2D1_BITMAP_PROPERTIES props = D2D1::BitmapProperties(
                    D2D1::PixelFormat(DXGI_FORMAT_B8G8R8A8_UNORM, D2D1_ALPHA_MODE_PREMULTIPLIED));
                e.bitmap.Reset();
                pc.dc->CreateBitmap(D2D1::SizeU(e.image->width, e.image->height), e.image->bgra.data(), e.image->width * 4,
                                    props, &e.bitmap);
                e.generation = pc.res.dev.generation();
            }
            if (e.bitmap) {
                const float w = static_cast<float>(e.image->width) / dpi_scale, h = static_cast<float>(e.image->height) / dpi_scale;
                const float s = std::min({slot.width() / w, slot.height() / h, 1.0f});
                const float dw = w * s, dh = h * s;
                const Rect dst = Rect::xywh(slot.l + (slot.width() - dw) / 2, slot.t + (slot.height() - dh) / 2, dw, dh);
                // A hidden file's picture is dimmed as its icon would be. Spec: Styles.xaml (ThumbTileTemplate, IconOpacity).
                pc.dc->DrawBitmap(e.bitmap.Get(), dst.d2d(), provider_->row_faded(row) ? 0.55f : 1.0f, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR);
            }
        } else {
            // The placeholder: the file's icon in an empty frame until the picture arrives.
            pc.stroke(slot, tok::BorderSubtle);
            bool faded = false;
            if (const ThumbPtr icon = provider_->row_image(row, placeholder_px, &faded)) {
                const float d = kPlaceholderIcon;
                images_.draw(pc, icon, Rect::xywh(slot.l + (slot.width() - d) / 2, slot.t + (slot.height() - d) / 2, d, d),
                             faded ? 0.5f : 1.0f);
            }
        }
        const Rect cap = lay.caption_rect(row).offset(bounds.l, bounds.t - v_.offset());
        const std::wstring text = provider_->cell_text(row, caption_column);
        if (auto* layout = pc.res.layouts.get({id, 0x7000u + static_cast<uint32_t>(caption_column), cap.width()}, text,
                                              style.font, cap_opt)) {
            pc.dc->DrawTextLayout(D2D1::Point2F(cap.l, cap.t), layout, pc.brush(fg), D2D1_DRAW_TEXT_OPTIONS_CLIP);
        }
        if (active && row == cursor_) pc.stroke(tr, tok::BorderFocus);
        if (row == drop_row) {
            pc.fill(tr, tok::ListDropBackground);
            pc.stroke(tr, tok::ListDropBorder, 2.0f);
        }
    }
    if (marquee_) {
        pc.fill(marquee_rect_, tok::MarqueeFill);
        pc.stroke(marquee_rect_, tok::AccentBackground);
    }
    pc.unclip();
    v_.paint(pc);
}

// A folder (or a file that is not a picture) above the grid: its icon and name, and the size and modified date at
// the right in the quieter colour. Spec: Styles.xaml (ThumbRowTemplate).
void TileView::paint_row(PaintContext& pc, size_t row, const Rect& r, Token fg, bool sel) {
    float x = r.l + 6;
    bool faded = false;
    if (const ThumbPtr icon = provider_->row_image(row, device_px(pc, 16.0f), &faded)) {
        images_.draw(pc, icon, Rect::xywh(x, r.t + (r.height() - 16) / 2, 16, 16), faded ? 0.5f : 1.0f);
    }
    x += 22;
    const Token quiet = sel ? fg : tok::TextSecondary;
    const float modified_l = r.r - 8 - 150, size_r = modified_l - 10;
    pc.text(provider_->tile_row_text(row, 1), Rect{modified_l, r.t, r.r - 8, r.b}, style.font, quiet);
    pc.text(provider_->tile_row_text(row, 0), Rect{size_r - 90, r.t, size_r, r.b}, style.font, quiet, gfx::TextAlign::Trailing);
    pc.text(provider_->cell_text(row, caption_column), Rect{x, r.t, std::max(x, size_r - 100), r.b}, style.font, fg);
}

// ---- input ------------------------------------------------------------------------------------------

void TileView::set_scroll_y(float y) {
    const float before = v_.offset();
    v_.set_offset(y);
    if (before == v_.offset()) return;
    update_requests();
    invalidate();
}

void TileView::scroll_to_row(size_t row) {
    if (row >= count()) return;
    const Rect t = layout().tile_rect(row);
    float y = v_.offset();
    if (t.t - layout().m.margin < y) {
        y = t.t - layout().m.margin;
    } else if (t.b + layout().m.margin > y + v_.viewport) {
        y = t.b + layout().m.margin - v_.viewport;
    }
    set_scroll_y(y);
}

void TileView::reveal_cursor(size_t row) {
    if (row >= count()) return;
    anchor_ = cursor_ = row;
    scroll_to_row(row);
    invalidate();
}

void TileView::select_row(size_t row) {
    if (!provider_ || row >= count()) return;
    cursor_ = anchor_ = row;
    provider_->select(SelectOp::Replace, row, row);
    scroll_to_row(row);
    invalidate();
}

void TileView::move_cursor(size_t row, const Mods& m) {
    if (!provider_ || count() == 0) return;
    row = std::min(row, count() - 1);
    if (m.shift) {
        if (anchor_ == npos) anchor_ = cursor_ == npos ? row : cursor_;
        provider_->select(SelectOp::Replace, std::min(anchor_, row), std::max(anchor_, row));
    } else if (!m.ctrl) {
        provider_->select(SelectOp::Replace, row, row);
        anchor_ = row;
    }
    cursor_ = row;
    scroll_to_row(row);
    invalidate();
}

bool TileView::on_mouse_down(const MouseEvent& e) {
    const NoteIds noted{this};
    if (e.button == MouseButton::Middle) {
        const size_t row = row_at(e.pt);
        if (row == npos || !on_middle) return false;
        on_middle(row);
        return true;
    }
    if (e.button != MouseButton::Left && e.button != MouseButton::Right) return false;
    if (e.button == MouseButton::Left && v_.mouse_down(e.pt)) {
        bar_drag_ = true;
        root()->capture(this);
        update_requests();
        invalidate();
        return true;
    }
    if (!provider_) return false;
    const size_t row = row_at(e.pt);
    if (row == npos) {
        if (e.button == MouseButton::Right) {
            if (on_context) on_context(e.pt, npos);
            return true;
        }
        // Empty space: a marquee; Ctrl or Shift keeps what was selected and adds the band to it.
        marquee_ = true;
        marquee_add_ = e.mods.ctrl || e.mods.shift;
        marquee_origin_ = {e.pt.x - bounds.l, e.pt.y - bounds.t + v_.offset()};
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
        invalidate();
        return true;
    }
    if (e.button == MouseButton::Right && provider_->selected(row)) {
        cursor_ = row;  // a right press on the selection keeps it
    } else if (e.button == MouseButton::Right) {
        provider_->select(SelectOp::Replace, row, row);
        anchor_ = cursor_ = row;
    } else if (e.mods.shift && anchor_ != npos) {
        provider_->select(e.mods.ctrl ? SelectOp::Add : SelectOp::Replace, std::min(anchor_, row), std::max(anchor_, row));
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
    const bool may_drag = on_drag_start && e.clicks < 2;
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

void TileView::update_marquee(Point p) {
    const float cx = p.x - bounds.l, cy = p.y - bounds.t + v_.offset();
    const Rect m{std::min(marquee_origin_.x, cx), std::min(marquee_origin_.y, cy), std::max(marquee_origin_.x, cx),
                 std::max(marquee_origin_.y, cy)};
    marquee_rect_ = m.offset(bounds.l, bounds.t - v_.offset());
    // Tiles are laid out by arithmetic, so the swept ones are found without measuring any: every tile of
    // the rows the band spans, kept if it intersects. Selected in one call per contiguous run.
    const TileLayout lay = layout();
    const auto [first, last] = lay.visible(m.t, std::max(1.0f, m.height()));
    std::vector<size_t> rows = marquee_add_ ? marquee_before_ : std::vector<size_t>{};
    for (size_t i = first; i < last; ++i) {
        if (lay.tile_rect(i).intersects(m)) rows.push_back(i);
    }
    if (first < last && !rows.empty()) anchor_ = cursor_ = rows.back();
    std::sort(rows.begin(), rows.end());
    rows.erase(std::unique(rows.begin(), rows.end()), rows.end());
    provider_->select_rows(rows);  // one bulk call
    invalidate();
}

bool TileView::on_mouse_move(const MouseEvent& e) {
    if (drag_armed_) {
        if (std::abs(e.pt.x - drag_origin_.x) > 8.0f || std::abs(e.pt.y - drag_origin_.y) > 8.0f) {
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
        update_requests();
        invalidate();
        return true;
    }
    if (marquee_) {
        const float step = metrics.tile_h() + metrics.gap;
        if (e.pt.y < bounds.t) {
            set_scroll_y(v_.offset() - step / 4);
        } else if (e.pt.y > bounds.b) {
            set_scroll_y(v_.offset() + step / 4);
        }
        update_marquee(e.pt);
        return true;
    }
    invalidate();  // hover highlight follows the pointer
    return false;
}

bool TileView::on_mouse_up(const MouseEvent&) {
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
    if (marquee_) {
        marquee_ = false;
        root()->release_capture();
        invalidate();
        return true;
    }
    if (!bar_drag_) return false;
    bar_drag_ = false;
    v_.mouse_up();
    return true;
}

void TileView::on_modal_cancel() {
    drag_armed_ = context_pending_ = marquee_ = bar_drag_ = false;
    deferred_row_ = npos;
    invalidate();
}

bool TileView::on_char(wchar_t c) {
    const NoteIds noted{this};
    if (c < 0x20 || c == 0x7f || !on_typeahead) return false;
    const uint64_t now = root()->now_ms();
    if (now - typed_at_ > kTypeAheadMs) typed_.clear();
    if (c == L' ' && typed_.empty()) return false;
    typed_.push_back(c);
    typed_at_ = now;
    on_typeahead(typed_);
    return true;
}

bool TileView::on_wheel(Point, int delta, Mods) {
    if (!v_.needed()) return false;
    // The same speed as the list: Windows' lines per notch times the setting, a line being a fifth of a tile.
    set_scroll_y(v_.offset() + wheel_rows(delta) * (metrics.tile_h() + metrics.gap) * 0.2f);
    return true;
}

bool TileView::on_key_down(const KeyEvent& e) {
    const NoteIds noted{this};
    if (!provider_ || count() == 0) return false;
    const size_t cols = static_cast<size_t>(layout().columns());
    const size_t cur = cursor_ == npos ? 0 : cursor_;
    const size_t page = std::max<size_t>(1, static_cast<size_t>(v_.viewport / (metrics.tile_h() + metrics.gap))) * cols;
    switch (e.vk) {
        case VK_LEFT: move_cursor(cursor_ == npos ? 0 : (cur > 0 ? cur - 1 : 0), e.mods); return true;
        case VK_RIGHT: move_cursor(cursor_ == npos ? 0 : cur + 1, e.mods); return true;
        // In the rows above the grid a step is one row; in the grid it is a row of tiles, and up from its first row
        // is the last row above it.
        case VK_UP: {
            const size_t lead = std::min(leading_, count());
            const size_t to = cur < lead ? (cur > 0 ? cur - 1 : 0) : cur - lead >= cols ? cur - cols : (lead > 0 ? lead - 1 : cur);
            move_cursor(cursor_ == npos ? 0 : to, e.mods);
            return true;
        }
        case VK_DOWN: {
            const size_t lead = std::min(leading_, count());
            const size_t to = cur < lead ? cur + 1 : (cur + cols < count() ? cur + cols : cur);
            move_cursor(cursor_ == npos ? 0 : std::min(to, count() - 1), e.mods);
            return true;
        }
        case VK_PRIOR: move_cursor(cur > page ? cur - page : 0, e.mods); return true;
        case VK_NEXT: move_cursor(cur + page, e.mods); return true;
        case VK_HOME: move_cursor(0, e.mods); return true;
        case VK_END: move_cursor(count() - 1, e.mods); return true;
        case VK_RETURN:
            if (cursor_ != npos && on_activate) on_activate(cursor_);
            return true;
        case VK_SPACE:
            if (e.mods.ctrl && cursor_ != npos) {
                provider_->select(SelectOp::Toggle, cursor_, cursor_);
                invalidate();
                return true;
            }
            return false;
        case VK_ESCAPE: provider_->select(SelectOp::Clear, 0, 0); invalidate(); return true;
        case 'A':
            if (e.mods.ctrl) {
                provider_->select(SelectOp::Replace, 0, count() - 1);
                invalidate();
                return true;
            }
            return false;
        default: return false;
    }
}

}  // namespace bb::ui
