#include "views/forms.h"

#include <windows.h>

#include <algorithm>
#include <cmath>
#include <utility>

namespace bb::views::forms {

void Picture::paint(PaintContext& pc) {
    if (!source) return;
    const ThumbPtr image = source(device_px(pc, size));
    if (!image) return;
    const float s = std::min(size, std::min(bounds.width(), bounds.height()));
    images_.draw(pc, image, Rect::xywh(bounds.l + (bounds.width() - s) / 2, bounds.t + (bounds.height() - s) / 2, s, s));
}

TextBlock* text(Widget& parent, std::wstring t, Token fg, float size, int weight) {
    auto* tb = parent.emplace<TextBlock>(std::move(t));
    tb->style.fg = fg;
    tb->style.font.size = size;
    tb->style.font.weight = weight;
    return tb;
}

TextBlock* heading(Widget& parent, std::wstring t) { return text(parent, std::move(t), tok::TextPrimary, 20.0f, 600); }

TextBlock* note(Widget& parent, std::wstring t) {
    auto* tb = text(parent, std::move(t), tok::TextMuted, 13.0f);
    tb->wrap = true;
    return tb;
}

StackPanel* row(Widget& parent, float g) { return parent.emplace<StackPanel>(Orientation::Horizontal, g); }
StackPanel* column(Widget& parent, float g) { return parent.emplace<StackPanel>(Orientation::Vertical, g); }

Widget* gap(Widget& parent, float h) {
    auto* w = parent.emplace<Widget>();
    w->height = h;
    w->width = 1;
    return w;
}

TextBlock* row_label(StackPanel& r, std::wstring t, float width) {
    auto* tb = text(r, std::move(t));
    tb->width = width;
    return tb;
}

CheckBox* check(Widget& parent, std::wstring label, bool value, std::function<void(bool)> on_toggle) {
    auto* c = parent.emplace<CheckBox>(std::move(label));
    c->set_checked(value);
    c->on_toggle = std::move(on_toggle);
    return c;
}

Button* button(Widget& parent, std::wstring label, std::function<void()> on_click, float width) {
    auto* b = parent.emplace<Button>(std::move(label));
    b->on_click = std::move(on_click);
    if (width >= 0) b->width = width;
    return b;
}

Edit* field(Widget& parent, std::wstring value, float width, std::function<void(const std::wstring&)> on_changed, std::wstring placeholder) {
    auto* e = parent.emplace<Edit>();
    e->set_text(std::move(value));
    e->width = width;
    e->placeholder = std::move(placeholder);
    e->on_changed = std::move(on_changed);
    return e;
}

ComboBox* combo(Widget& parent, std::vector<std::wstring> items, int selected, float width, std::function<void(int)> on_change) {
    auto* c = parent.emplace<ComboBox>();
    c->set_items(std::move(items));
    c->set_selected(selected);
    c->width = width;
    c->on_change = std::move(on_change);
    return c;
}

RadioGroup* radios(Widget& parent, std::vector<std::wstring> items, int selected, std::function<void(int)> on_change) {
    auto* r = parent.emplace<RadioGroup>();
    r->set_items(std::move(items));
    r->set_selected(selected);
    r->on_change = std::move(on_change);
    return r;
}

// ---- ItemList -------------------------------------------------------------------------------------------

void ItemList::set_entries(std::vector<ListEntry> e, int select_row) {
    entries = std::move(e);
    selected = select_row;
    icon_geo_.assign(entries.size(), {});
    icon_gen_.assign(entries.size(), 0);
    layout_bar();
    invalidate();
}

void ItemList::update_entry(int i, ListEntry e) {
    if (i < 0 || i >= static_cast<int>(entries.size())) return;
    entries[static_cast<size_t>(i)] = std::move(e);
    icon_geo_[static_cast<size_t>(i)] = nullptr;
    icon_gen_[static_cast<size_t>(i)] = 0;
    invalidate();
}

void ItemList::select(int i, bool notify) {
    if (i < -1 || i >= static_cast<int>(entries.size())) return;
    selected = i;
    scroll_into_view(i);
    invalidate();
    if (notify && on_select) on_select(i);
}

void ItemList::layout_bar() {
    v_.extent = static_cast<float>(entries.size()) * row_height;
    v_.viewport = bounds.height();
    v_.area = v_.needed() ? Rect{bounds.r - ScrollBar::kThickness, bounds.t, bounds.r, bounds.b} : Rect{};
    v_.set_offset(v_.offset());
}

int ItemList::visible_rows() const { return static_cast<int>(bounds.height() / row_height); }

void ItemList::scroll_into_view(int i) {
    if (i < 0) return;
    const float top = static_cast<float>(i) * row_height;
    if (top < v_.offset()) v_.set_offset(top);
    else if (top + row_height > v_.offset() + bounds.height()) v_.set_offset(top + row_height - bounds.height());
}

Rect ItemList::row_rect(int i) const {
    const float w = bounds.width() - (v_.needed() ? ScrollBar::kThickness : 0.0f);
    return Rect::xywh(bounds.l, bounds.t + static_cast<float>(i) * row_height - v_.offset(), w, row_height);
}

int ItemList::row_at(Point p) const {
    if (!bounds.contains(p) || (v_.needed() && v_.area.contains(p))) return -1;
    const int i = static_cast<int>(std::floor((p.y - bounds.t + v_.offset()) / row_height));
    return i >= 0 && i < static_cast<int>(entries.size()) ? i : -1;
}

Size ItemList::measure(Size) { return fixed_or({200.0f, 120.0f}); }

void ItemList::arrange(Rect r) {
    bounds = r;
    layout_bar();
}

int ItemList::step(int from, int dir) const {
    for (int i = from + dir; i >= 0 && i < static_cast<int>(entries.size()); i += dir) {
        if (!entries[static_cast<size_t>(i)].heading && !entries[static_cast<size_t>(i)].separator) return i;
    }
    return from;
}

void ItemList::paint(PaintContext& pc) {
    Widget::paint(pc);
    pc.clip(bounds.intersect(pc.dirty));
    const int first = std::max(0, static_cast<int>(std::floor(v_.offset() / row_height)));
    const int last = std::min(static_cast<int>(entries.size()), first + static_cast<int>(std::ceil(bounds.height() / row_height)) + 2);
    gfx::FontSpec font;
    font.size = 13.5f;
    for (int i = first; i < last; ++i) {
        const ListEntry& e = entries[static_cast<size_t>(i)];
        const Rect rc = row_rect(i);
        if (e.separator) {
            pc.line({rc.l + 8, rc.t + row_height / 2}, {rc.r - 8, rc.t + row_height / 2}, tok::BorderSubtle);
            continue;
        }
        const bool sel = i == selected && !e.heading;
        if (sel) pc.fill(rc, focused() ? tok::ListSelectedBackground : tok::ListSelectedInactiveBackground);
        else if (hover && i == hot_ && !e.heading && !e.separator && !e.muted) pc.fill(rc, tok::ListHoverBackground);
        Token fg = e.heading ? tok::TextMuted : !e.enabled || e.muted ? tok::TextDisabled : sel ? tok::ListSelectedForeground : tok::TextPrimary;
        float x = rc.l + 8;
        if (e.picture) {
            if (const ThumbPtr image = e.picture(device_px(pc, 16.0f)))
                images_.draw(pc, image, Rect::xywh(x, rc.t + (rc.height() - 16) / 2, 16, 16), e.faded ? 0.5f : 1.0f);
            x += 22;
        } else if (e.icon) {
            Icon::draw(pc, *e.icon, Rect::xywh(x, rc.t + (rc.height() - 16) / 2, 16, 16), fg, icon_geo_[static_cast<size_t>(i)], icon_gen_[static_cast<size_t>(i)]);
            x += 24;
        }
        if (e.swatch_on) {
            ComPtr<ID2D1SolidColorBrush> brush;
            const D2D1_COLOR_F color = D2D1::ColorF(e.swatch & 0xFFFFFF, static_cast<float>((e.swatch >> 24) & 0xFF) / 255.0f);
            if (SUCCEEDED(pc.dc->CreateSolidColorBrush(color, &brush))) {
                const Rect sw = Rect::xywh(x, rc.t + (rc.height() - 14) / 2, 22, 14);
                pc.dc->FillRectangle(sw.d2d(), brush.Get());
                pc.stroke(sw, tok::BorderDefault);
            }
            x += 30;
        }
        gfx::FontSpec f = font;
        if (e.heading) f.weight = 600;
        float right = rc.r - 8;
        if (!e.detail.empty()) {
            const float dw = std::ceil(pc.res.text.measure(e.detail, font).width);
            pc.text(e.detail, Rect{right - dw, rc.t, right, rc.b}, font, e.heading ? tok::TextMuted : tok::TextMuted, gfx::TextAlign::Trailing);
            right -= dw + 12;
        }
        pc.text(e.text, Rect{x, rc.t, std::max(x, right), rc.b}, f, fg);
    }
    if (drop_line_ >= 0) {
        const float y = std::clamp(bounds.t + static_cast<float>(drop_line_) * row_height - v_.offset(), bounds.t + 1, bounds.b - 1);
        pc.fill(Rect{bounds.l + 4, y - 1, bounds.r - 4 - (v_.needed() ? ScrollBar::kThickness : 0.0f), y + 1}, tok::AccentBackground);
    }
    pc.unclip();
    v_.paint(pc);
    if (style.border != kNoToken) pc.stroke(bounds, focused() ? tok::BorderFocus : style.border);
}

void ItemList::set_drop_line(int gap) {
    if (gap == drop_line_) return;
    drop_line_ = gap;
    invalidate();
}

int ItemList::gap_at(Point p) const {
    const float y = p.y - bounds.t + v_.offset();
    return std::clamp(static_cast<int>(std::lround(y / row_height)), 0, static_cast<int>(entries.size()));
}

void ItemList::end_drag() {
    if (drop_target_) drop_target_->set_drop_line(-1);
    set_drop_line(-1);
    drop_target_ = nullptr;
    dragging_ = false;
    drag_from_ = -1;
}

bool ItemList::on_mouse_down(const MouseEvent& e) {
    if (e.button == MouseButton::Right && on_context) {
        const int i = row_at(e.pt);
        if (i >= 0 && !entries[static_cast<size_t>(i)].heading && !entries[static_cast<size_t>(i)].separator) on_context(i, e.pt);
        return true;
    }
    if (e.button == MouseButton::Middle && on_middle) {
        const int i = row_at(e.pt);
        if (i >= 0 && !entries[static_cast<size_t>(i)].heading && !entries[static_cast<size_t>(i)].separator && !entries[static_cast<size_t>(i)].muted) on_middle(i);
        return true;
    }
    if (e.button != MouseButton::Left) return false;
    if (v_.mouse_down(e.pt)) {
        root()->capture(this);
        invalidate();
        return true;
    }
    const int i = row_at(e.pt);
    // A separator can be dragged; a heading cannot.
    const bool draggable = i >= 0 && !entries[static_cast<size_t>(i)].heading && (on_reorder || (drag_into && drag_into->on_drop_in)) &&
                           (!can_drag || can_drag(i)) && e.clicks == 1;
    if (draggable) {
        drag_from_ = i;
        drag_origin_ = e.pt;
        root()->capture(this);
    }
    if (i >= 0 && !entries[static_cast<size_t>(i)].heading && !entries[static_cast<size_t>(i)].separator) {
        if (!keep_selection) {
            // A link: it runs, and nothing stays chosen. The handler may rebuild the list, so nothing after it.
            if (!entries[static_cast<size_t>(i)].muted && on_select) on_select(i);
            return true;
        }
        select(i, true);
        if (activate_on_click) pressed_ = e.clicks == 1 ? i : -1;
        else if (e.clicks >= 2 && on_activate) on_activate(i);
    }
    return true;
}

void ItemList::on_mouse_leave() {
    if (hot_ < 0) return;
    hot_ = -1;
    invalidate();
}

bool ItemList::on_mouse_move(const MouseEvent& e) {
    if (v_.mouse_move(e.pt)) {
        invalidate();
        return true;
    }
    if (drag_from_ < 0) {
        const int hot = bounds.contains(e.pt) ? row_at(e.pt) : -1;
        if (hot != hot_) {
            hot_ = hot;
            tooltip = hot >= 0 && static_cast<size_t>(hot) < entries.size() ? entries[static_cast<size_t>(hot)].tip : std::wstring();
            if (hover) invalidate();
        }
    }
    if (drag_from_ < 0) return false;
    if (!dragging_ && std::abs(e.pt.y - drag_origin_.y) + std::abs(e.pt.x - drag_origin_.x) < 5.0f) return true;
    dragging_ = true;
    pressed_ = -1;  // a drag is not a click
    ItemList* target = on_reorder && bounds.contains(e.pt) ? this
                       : drag_into && drag_into->on_drop_in && drag_into->bounds.contains(e.pt) ? drag_into
                                                                                               : nullptr;
    if (drop_target_ && drop_target_ != target) drop_target_->set_drop_line(-1);
    drop_target_ = target;
    if (target) {
        // Near an edge, the list under the pointer scrolls to show where the row could go.
        if (e.pt.y < target->bounds.t + target->row_height / 2) target->v_.set_offset(target->v_.offset() - target->row_height / 2);
        else if (e.pt.y > target->bounds.b - target->row_height / 2) target->v_.set_offset(target->v_.offset() + target->row_height / 2);
        target->set_drop_line(target->gap_at(e.pt));
        target->invalidate();
    }
    return true;
}

bool ItemList::on_mouse_up(const MouseEvent& e) {
    if (drag_from_ >= 0) {
        root()->release_capture();
        const int from = drag_from_;
        const bool dragged = dragging_;
        ItemList* target = drop_target_;
        const int gap = target ? target->drop_line_ : -1;
        end_drag();
        if (dragged) {
            pressed_ = -1;
            // Either handler may rebuild the page this list is on, so nothing of `this` is touched after.
            if (target == this && gap >= 0) {
                const int to = gap > from ? gap - 1 : gap;
                if (to != from && on_reorder) on_reorder(from, to);
            } else if (target && gap >= 0 && target->on_drop_in) {
                target->on_drop_in(from, gap);
            }
            return true;
        }
    }
    const bool was = v_.dragging();
    v_.mouse_up();
    const int pressed = std::exchange(pressed_, -1);
    if (!was && activate_on_click && pressed >= 0 && pressed == row_at(e.pt) && on_activate) {
        on_activate(pressed);
        return true;
    }
    return was;
}

bool ItemList::on_wheel(Point, int delta, Mods) {
    if (!v_.needed()) return false;
    v_.set_offset(v_.offset() - static_cast<float>(delta) / 120.0f * row_height * 3.0f);
    invalidate();
    return true;
}

bool ItemList::on_key_down(const KeyEvent& e) {
    if (on_key && !e.mods.ctrl && !e.mods.alt && on_key(e.vk)) return true;
    switch (e.vk) {
        case VK_DOWN: select(step(selected, 1), true); return true;
        case VK_UP: select(selected < 0 ? step(-1, 1) : step(selected, -1), true); return true;
        case VK_HOME: select(step(-1, 1), true); return true;
        case VK_END: select(step(static_cast<int>(entries.size()), -1), true); return true;
        case VK_RETURN:
            if (selected >= 0 && on_activate) on_activate(selected);
            return true;
        default: return false;
    }
}

}  // namespace bb::views::forms
