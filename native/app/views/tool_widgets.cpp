#include "views/tool_widgets.h"

#include <windows.h>

#include <algorithm>
#include <cmath>

namespace bb::views::tools {

namespace {

gfx::FontSpec ui_font(float size = 13.5f, int weight = 400) {
    gfx::FontSpec f;
    f.size = size;
    f.weight = weight;
    return f;
}

gfx::FontSpec mono_font(float size = 12.5f) {
    gfx::FontSpec f;
    f.family = gfx::Family::Mono;
    f.size = size;
    return f;
}

void fill_argb(PaintContext& pc, const Rect& r, uint32_t argb) {
    ComPtr<ID2D1SolidColorBrush> brush;
    const D2D1_COLOR_F color = D2D1::ColorF(argb & 0xFFFFFF, static_cast<float>((argb >> 24) & 0xFF) / 255.0f);
    if (SUCCEEDED(pc.dc->CreateSolidColorBrush(color, &brush))) pc.dc->FillRectangle(r.d2d(), brush.Get());
}

}  // namespace

// ---- TableList ------------------------------------------------------------------------------------------

void TableList::set_rows(std::vector<TableRow> r) {
    rows = std::move(r);
    if (selected >= static_cast<int>(rows.size())) selected = -1;
    layout_bar();
    invalidate();
}

Size TableList::measure(Size) { return fixed_or({300.0f, 140.0f}); }

void TableList::arrange(Rect r) {
    bounds = r;
    layout_bar();
}

void TableList::layout_bar() {
    float total = 0;
    for (const TableRow& row : rows) total += row_step(row);
    v_.extent = total;
    v_.viewport = std::max(0.0f, bounds.height() - header_height());
    v_.area = v_.needed() ? Rect{bounds.r - ScrollBar::kThickness, body_top(), bounds.r, bounds.b} : Rect{};
    v_.set_offset(v_.offset());
}

std::vector<float> TableList::column_x(float total) const {
    float fixed = 0;
    int flexes = 0;
    for (const TableColumn& c : columns) {
        if (c.flex) ++flexes;
        else fixed += c.width;
    }
    const float share = flexes > 0 ? std::max(40.0f, (total - fixed) / static_cast<float>(flexes)) : 0.0f;
    std::vector<float> xs{0.0f};
    for (const TableColumn& c : columns) xs.push_back(xs.back() + (c.flex ? share : c.width));
    return xs;
}

int TableList::row_at(Point p) const {
    if (!bounds.contains(p) || p.y < body_top() || (v_.needed() && v_.area.contains(p))) return -1;
    float y = body_top() - v_.offset();
    for (size_t i = 0; i < rows.size(); ++i) {
        const float step = row_step(rows[i]);
        if (p.y >= y && p.y < y + step) return static_cast<int>(i);
        y += step;
    }
    return -1;
}

void TableList::paint(PaintContext& pc) {
    Widget::paint(pc);
    pc.clip(bounds.intersect(pc.dirty));
    const float inner_w = bounds.width() - (v_.needed() ? ScrollBar::kThickness : 0.0f);
    const std::vector<float> xs = column_x(inner_w);
    if (show_header) {
        const Rect h = Rect::xywh(bounds.l, bounds.t, bounds.width(), header_height());
        pc.fill(h, tok::ListHeaderBackground);
        for (size_t c = 0; c < columns.size(); ++c) {
            const Rect cell{bounds.l + xs[c] + 8, h.t, bounds.l + xs[c + 1] - 4, h.b};
            pc.text(columns[c].title, cell, ui_font(12.5f, 600), tok::ListHeaderForeground,
                    columns[c].right ? gfx::TextAlign::Trailing : gfx::TextAlign::Leading);
        }
        pc.line({h.l, h.b - 0.5f}, {h.r, h.b - 0.5f}, tok::ListHeaderBorder);
    }
    pc.clip(Rect{bounds.l, body_top(), bounds.r, bounds.b}.intersect(pc.dirty));
    if (rows.empty() && !empty_text.empty()) {
        pc.text(empty_text, Rect{bounds.l + 10, body_top() + 6, bounds.r - 10, body_top() + 6 + row_height}, ui_font(), tok::TextMuted);
    }
    float y = body_top() - v_.offset();
    for (size_t i = 0; i < rows.size(); ++i) {
        const TableRow& row = rows[i];
        const float step = row_step(row);
        if (y + step < body_top()) {
            y += step;
            continue;
        }
        if (y > bounds.b) break;
        const Rect rc = Rect::xywh(bounds.l, y, inner_w, row_height);
        if (row.tint != gfx::kNoToken) pc.fill(Rect::xywh(bounds.l, y, inner_w, step), row.tint);
        const bool sel = static_cast<int>(i) == selected && !row.heading;
        if (sel) pc.fill(Rect::xywh(bounds.l, y, inner_w, step), focused() ? tok::ListSelectedBackground : tok::ListSelectedInactiveBackground);
        for (size_t c = 0; c < columns.size() && c < row.cells.size(); ++c) {
            Token fg = row.heading ? tok::TextMuted : row.muted ? tok::TextDisabled : sel ? tok::ListSelectedForeground : tok::TextPrimary;
            if (c == 0 && row.fg != gfx::kNoToken && !row.muted && !sel) fg = row.fg;
            if (c > 0 && !sel && !row.muted && !row.heading) fg = columns[c].right || columns[c].mono ? tok::TextSecondary : fg;
            gfx::FontSpec f = columns[c].mono ? mono_font() : ui_font();
            if (row.heading) f.weight = 600;
            const Rect cell{bounds.l + xs[c] + 8, rc.t, std::max(bounds.l + xs[c] + 8, bounds.l + xs[c + 1] - 4), rc.b};
            pc.text(row.cells[c], cell, f, fg, columns[c].right ? gfx::TextAlign::Trailing : gfx::TextAlign::Leading);
        }
        if (row.bar_slot || row.bar >= 0) {
            const Rect track = Rect::xywh(bounds.l + 8, y + row_height - 1, std::max(0.0f, inner_w - 16), 3);
            if (row.bar >= 0) {
                pc.fill(track, tok::ProgressTrack);
                pc.fill(Rect::xywh(track.l, track.t, track.width() * std::clamp(row.bar, 0.0f, 1.0f), track.height()), tok::ProgressIndicator);
            }
        }
        y += step;
    }
    pc.unclip();
    v_.paint(pc);
    pc.unclip();
    if (style.border != gfx::kNoToken) pc.stroke(bounds, focused() ? tok::BorderFocus : style.border);
}

bool TableList::on_mouse_down(const MouseEvent& e) {
    if (e.button != MouseButton::Left) return false;
    if (v_.mouse_down(e.pt)) {
        root()->capture(this);
        invalidate();
        return true;
    }
    const int i = row_at(e.pt);
    if (i >= 0 && !rows[static_cast<size_t>(i)].heading) {
        selected = i;
        invalidate();
        if (on_select) on_select(i);
        if (e.clicks >= 2 && on_activate) on_activate(i);
    }
    return true;
}

bool TableList::on_mouse_move(const MouseEvent& e) {
    if (v_.mouse_move(e.pt)) {
        invalidate();
        return true;
    }
    return false;
}

bool TableList::on_mouse_up(const MouseEvent&) {
    const bool was = v_.dragging();
    v_.mouse_up();
    return was;
}

bool TableList::on_wheel(Point, int delta, Mods) {
    if (!v_.needed()) return false;
    v_.set_offset(v_.offset() - static_cast<float>(delta) / 120.0f * row_height * 3.0f);
    invalidate();
    return true;
}

bool TableList::on_key_down(const KeyEvent& e) {
    auto move = [&](int dir) {
        for (int i = selected + dir; i >= 0 && i < static_cast<int>(rows.size()); i += dir) {
            if (!rows[static_cast<size_t>(i)].heading) {
                selected = i;
                invalidate();
                if (on_select) on_select(i);
                return;
            }
        }
    };
    switch (e.vk) {
        case VK_DOWN: move(1); return true;
        case VK_UP: move(-1); return true;
        case VK_RETURN:
        case VK_SPACE:
            if (selected >= 0 && on_activate) on_activate(selected);
            return true;
        default: return false;
    }
}

// ---- DiffView -------------------------------------------------------------------------------------------

void DiffView::set_lines(std::vector<DiffLine> lines) {
    lines_ = std::move(lines);
    layout_bar();
    invalidate();
}

void DiffView::arrange(Rect r) {
    bounds = r;
    layout_bar();
}

void DiffView::layout_bar() {
    v_.extent = static_cast<float>(lines_.size()) * kRow + 4.0f;
    v_.viewport = bounds.height();
    v_.area = v_.needed() ? Rect{bounds.r - ScrollBar::kThickness, bounds.t, bounds.r, bounds.b} : Rect{};
    v_.set_offset(v_.offset());
}

void DiffView::scroll_to_row(int row) {
    v_.set_offset(std::max(0.0f, static_cast<float>(row) * kRow - kRow * 2));
    invalidate();
}

void DiffView::paint(PaintContext& pc) {
    Widget::paint(pc);
    pc.clip(bounds.intersect(pc.dirty));
    const float inner_w = bounds.width() - (v_.needed() ? ScrollBar::kThickness : 0.0f);
    const float half = std::floor(inner_w / 2);
    constexpr float kGutter = 46.0f;
    const gfx::FontSpec font = mono_font();
    const int first = std::max(0, static_cast<int>(std::floor(v_.offset() / kRow)));
    const int last = std::min(static_cast<int>(lines_.size()), first + static_cast<int>(std::ceil(bounds.height() / kRow)) + 2);
    for (int i = first; i < last; ++i) {
        const DiffLine& l = lines_[static_cast<size_t>(i)];
        const float y = bounds.t - v_.offset() + static_cast<float>(i) * kRow;
        const Rect left = Rect::xywh(bounds.l, y, half, kRow);
        const Rect right = Rect::xywh(bounds.l + half, y, inner_w - half, kRow);
        // Deleted lines are the left side's, inserted the right's; a replaced line is both.
        if (l.op == 1 || l.op == 3) pc.fill(left, l.op == 1 ? tok::CompareOlder : tok::CompareDiffers);
        if (l.op == 2 || l.op == 3) pc.fill(right, l.op == 2 ? tok::CompareNewer : tok::CompareDiffers);
        pc.text(l.ln, Rect{left.l + 2, y, left.l + kGutter - 6, y + kRow}, font, tok::TextMuted, gfx::TextAlign::Trailing);
        pc.text(l.lt, Rect{left.l + kGutter, y, left.r - 4, y + kRow}, font, tok::TextPrimary);
        pc.text(l.rn, Rect{right.l + 2, y, right.l + kGutter - 6, y + kRow}, font, tok::TextMuted, gfx::TextAlign::Trailing);
        pc.text(l.rt, Rect{right.l + kGutter, y, right.r - 4, y + kRow}, font, tok::TextPrimary);
    }
    pc.line({bounds.l + half, bounds.t}, {bounds.l + half, bounds.b}, tok::BorderSubtle);
    pc.unclip();
    v_.paint(pc);
    if (style.border != gfx::kNoToken) pc.stroke(bounds, style.border);
}

bool DiffView::on_mouse_down(const MouseEvent& e) {
    if (v_.mouse_down(e.pt)) {
        root()->capture(this);
        invalidate();
        return true;
    }
    return true;
}

bool DiffView::on_mouse_move(const MouseEvent& e) {
    if (v_.mouse_move(e.pt)) {
        invalidate();
        return true;
    }
    return false;
}

bool DiffView::on_mouse_up(const MouseEvent&) {
    const bool was = v_.dragging();
    v_.mouse_up();
    return was;
}

bool DiffView::on_wheel(Point, int delta, Mods) {
    if (!v_.needed()) return false;
    v_.set_offset(v_.offset() - static_cast<float>(delta) / 120.0f * kRow * 3.0f);
    invalidate();
    return true;
}

// ---- TreemapCanvas --------------------------------------------------------------------------------------

void TreemapCanvas::set_tiles(std::vector<TreemapTile> tiles) {
    tiles_ = std::move(tiles);
    hovered_ = -1;
    relayout();
    invalidate();
}

void TreemapCanvas::arrange(Rect r) {
    const bool resized = r.width() != bounds.width() || r.height() != bounds.height();
    bounds = r;
    if (resized) relayout();
}

void TreemapCanvas::relayout() {
    cells_.clear();
    if (!layouter || tiles_.empty() || bounds.empty()) return;
    std::vector<double> weights;
    weights.reserve(tiles_.size());
    for (const TreemapTile& t : tiles_) weights.push_back(t.weight);
    cells_ = layouter(weights, bounds.width(), bounds.height());
}

int TreemapCanvas::tile_at(Point p) const {
    for (const TreemapCell& c : cells_) {
        const Rect r = Rect::xywh(bounds.l + c.x, bounds.t + c.y, c.w, c.h);
        if (r.contains(p)) return static_cast<int>(c.index);
    }
    return -1;
}

void TreemapCanvas::paint(PaintContext& pc) {
    Widget::paint(pc);
    if (cells_.empty() || fills.empty()) return;
    pc.clip(bounds.intersect(pc.dirty));
    constexpr float kMinLabelWidth = 44.0f, kMinLabelHeight = 16.0f;
    for (const TreemapCell& c : cells_) {
        const Rect r = Rect::xywh(bounds.l + c.x, bounds.t + c.y, c.w, c.h);
        fill_argb(pc, r, fills[c.index % fills.size()]);
        pc.stroke(r, tok::BorderSubtle);
        // The hovered tile gets an outline rather than a different fill: changing the colour would
        // misreport its size band.
        if (static_cast<int>(c.index) == hovered_) pc.stroke(r.inflate(-1), tok::TextOnAccent, 2.0f);
        if (r.width() < kMinLabelWidth || r.height() < kMinLabelHeight) continue;
        pc.text(tiles_[c.index].name, Rect{r.l + 4, r.t + 2, r.r - 4, r.t + 2 + 16}, ui_font(12.0f), tok::TextOnAccent);
    }
    pc.unclip();
}

bool TreemapCanvas::on_mouse_down(const MouseEvent& e) {
    if (e.button != MouseButton::Left) return false;
    const int i = tile_at(e.pt);
    if (i >= 0 && on_tile) on_tile(static_cast<size_t>(i));
    return true;
}

bool TreemapCanvas::on_mouse_move(const MouseEvent& e) {
    const int i = tile_at(e.pt);
    if (i == hovered_) return false;
    hovered_ = i;
    tooltip = i >= 0 ? tiles_[static_cast<size_t>(i)].name + L" — " + tiles_[static_cast<size_t>(i)].size_text : std::wstring();
    invalidate();
    return true;
}

void TreemapCanvas::on_mouse_leave() {
    if (hovered_ < 0) return;
    hovered_ = -1;
    tooltip.clear();
    invalidate();
}

}  // namespace bb::views::tools
