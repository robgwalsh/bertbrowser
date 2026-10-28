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
    return (count + c - 1) / c;
}

float TileLayout::content_height() const {
    const size_t r = rows();
    if (r == 0) return 0;
    return 2 * m.margin + static_cast<float>(r) * m.tile_h() + static_cast<float>(r - 1) * m.gap;
}

Rect TileLayout::tile_rect(size_t index) const {
    const size_t c = static_cast<size_t>(columns());
    const float x = m.margin + static_cast<float>(index % c) * (m.tile_w() + m.gap);
    const float y = m.margin + static_cast<float>(index / c) * (m.tile_h() + m.gap);
    return Rect::xywh(x, y, m.tile_w(), m.tile_h());
}

std::pair<size_t, size_t> TileLayout::visible(float scroll, float viewport_h) const {
    if (count == 0 || viewport_h <= 0) return {0, 0};
    const float pitch = m.tile_h() + m.gap;
    const size_t c = static_cast<size_t>(columns());
    const float top = scroll - m.margin, bottom = scroll + viewport_h - m.margin;
    if (bottom <= 0) return {0, 0};
    const long long r0 = std::max(0LL, static_cast<long long>(std::floor(top / pitch)));
    const long long r1 = std::min(static_cast<long long>(rows()) - 1, static_cast<long long>(std::floor((bottom - 0.001f) / pitch)));
    if (r1 < r0) return {0, 0};
    return {static_cast<size_t>(r0) * c, std::min(count, (static_cast<size_t>(r1) + 1) * c)};
}

size_t TileLayout::index_at(Point p) const {
    const float x = p.x - m.margin, y = p.y - m.margin;
    if (x < 0 || y < 0) return static_cast<size_t>(-1);
    const float px = m.tile_w() + m.gap, py = m.tile_h() + m.gap;
    const size_t col = static_cast<size_t>(std::floor(x / px)), row = static_cast<size_t>(std::floor(y / py));
    if (x - static_cast<float>(col) * px >= m.tile_w() || y - static_cast<float>(row) * py >= m.tile_h()) return static_cast<size_t>(-1);
    if (col >= static_cast<size_t>(columns())) return static_cast<size_t>(-1);
    const size_t i = row * static_cast<size_t>(columns()) + col;
    return i < count ? i : static_cast<size_t>(-1);
}

Rect TileLayout::image_rect(size_t index) const {
    const Rect t = tile_rect(index);
    return Rect::xywh(t.l + m.pad, t.t + m.pad, m.image, m.image);
}

Rect TileLayout::caption_rect(size_t index) const {
    const Rect t = tile_rect(index);
    return Rect::xywh(t.l + m.pad, t.t + m.pad + m.image + 4, m.image, m.caption_h);
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

void TileView::refresh() {
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
    return TileLayout{metrics, w, count()};
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
    const uint32_t px = static_cast<uint32_t>(std::lround(metrics.image * dpi_scale));
    for (size_t r = first; r < last; ++r) {
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
    pc.fill(bounds, style.bg != kNoToken ? style.bg : tok::ListBackground);
    painted_ = 0;
    if (!provider_) return;
    pc.clip(bounds);
    const TileLayout lay = layout();
    const auto [first, last] = lay.visible(v_.offset(), bounds.height());
    const Point ptr = root() ? root()->pointer() : Point{};
    const bool active = focused();
    const gfx::LayoutOptions cap_opt = [&] {
        gfx::LayoutOptions o;
        o.wrap = true;
        o.max_width = metrics.image;
        o.max_height = metrics.caption_h;
        o.align = gfx::TextAlign::Center;
        return o;
    }();
    for (size_t row = first; row < last; ++row) {
        const Rect tr = lay.tile_rect(row).offset(bounds.l, bounds.t - v_.offset());
        if (!tr.intersects(pc.dirty)) continue;
        ++painted_;
        const bool sel = provider_->selected(row);
        const uint64_t id = provider_->row_id(row);
        if (sel) {
            pc.fill(tr, active ? tok::ListSelectedBackground : tok::ListSelectedInactiveBackground);
        } else if (hovered() && tr.contains(ptr)) {
            pc.fill(tr, tok::ListHoverBackground);
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
                pc.dc->DrawBitmap(e.bitmap.Get(), dst.d2d(), 1.0f, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR);
            }
        } else {
            pc.stroke(slot, tok::BorderSubtle);  // the placeholder: an empty frame until the image arrives
        }
        const Rect cap = lay.caption_rect(row).offset(bounds.l, bounds.t - v_.offset());
        const std::wstring text = provider_->cell_text(row, caption_column);
        if (auto* layout = pc.res.layouts.get({id, 0x7000u + static_cast<uint32_t>(caption_column), cap.width()}, text,
                                              style.font, cap_opt)) {
            const Token fg = sel ? (active ? tok::ListSelectedForeground : tok::ListSelectedInactiveForeground) : tok::TextPrimary;
            pc.dc->DrawTextLayout(D2D1::Point2F(cap.l, cap.t), layout, pc.brush(fg), D2D1_DRAW_TEXT_OPTIONS_CLIP);
        }
        if (active && row == cursor_) pc.stroke(tr, tok::BorderFocus);
    }
    pc.unclip();
    v_.paint(pc);
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
        if (!e.mods.ctrl && !e.mods.shift) provider_->select(SelectOp::Clear, 0, 0);
        invalidate();
        return true;
    }
    if (e.button == MouseButton::Right && provider_->selected(row)) {
        cursor_ = row;  // a right press on the selection keeps it
        invalidate();
        return true;
    }
    if (e.mods.shift && anchor_ != npos) {
        provider_->select(SelectOp::Replace, std::min(anchor_, row), std::max(anchor_, row));
    } else if (e.mods.ctrl) {
        provider_->select(SelectOp::Toggle, row, row);
        anchor_ = row;
    } else {
        provider_->select(SelectOp::Replace, row, row);
        anchor_ = row;
    }
    cursor_ = row;
    if (e.clicks >= 2 && e.button == MouseButton::Left && on_activate) on_activate(row);
    invalidate();
    return true;
}

bool TileView::on_mouse_move(const MouseEvent& e) {
    if (bar_drag_) {
        v_.mouse_move(e.pt);
        update_requests();
        invalidate();
        return true;
    }
    invalidate();  // hover highlight follows the pointer
    return false;
}

bool TileView::on_mouse_up(const MouseEvent&) {
    if (!bar_drag_) return false;
    bar_drag_ = false;
    v_.mouse_up();
    return true;
}

bool TileView::on_wheel(Point, int delta, Mods) {
    if (!v_.needed()) return false;
    set_scroll_y(v_.offset() - static_cast<float>(delta) / 120.0f * (metrics.tile_h() + metrics.gap) * 0.6f);
    return true;
}

bool TileView::on_key_down(const KeyEvent& e) {
    if (!provider_ || count() == 0) return false;
    const size_t cols = static_cast<size_t>(layout().columns());
    const size_t cur = cursor_ == npos ? 0 : cursor_;
    const size_t page = std::max<size_t>(1, static_cast<size_t>(v_.viewport / (metrics.tile_h() + metrics.gap))) * cols;
    switch (e.vk) {
        case VK_LEFT: move_cursor(cursor_ == npos ? 0 : (cur > 0 ? cur - 1 : 0), e.mods); return true;
        case VK_RIGHT: move_cursor(cursor_ == npos ? 0 : cur + 1, e.mods); return true;
        case VK_UP: move_cursor(cursor_ == npos ? 0 : (cur >= cols ? cur - cols : cur), e.mods); return true;
        case VK_DOWN: move_cursor(cursor_ == npos ? 0 : (cur + cols < count() ? cur + cols : cur), e.mods); return true;
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
