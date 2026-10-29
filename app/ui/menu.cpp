#include "ui/menu.h"

#include <algorithm>
#include <cmath>
#include <cwctype>

namespace bb::ui {

// ---- pure rules -------------------------------------------------------------------------------------

Rect place_popup(Size size, Rect anchor, PopupSide side, Rect work) {
    constexpr float kOverlap = 2.0f;
    constexpr float kTopPad = 5.0f;  // a submenu's first item lines up with the item that opened it
    float x, y;
    if (side == PopupSide::Below) {
        x = anchor.l;
        if (x + size.w > work.r) x = work.r - size.w;
        y = anchor.b;
        if (y + size.h > work.b) {
            if (anchor.t - size.h >= work.t) {
                y = anchor.t - size.h;  // flip above
            } else {
                y = work.b - size.h;
            }
        }
    } else {
        x = anchor.r - kOverlap;
        if (x + size.w > work.r) x = anchor.l - size.w + kOverlap;  // flip to the left
        y = anchor.t - kTopPad;
        if (y + size.h > work.b) y = work.b - size.h;
    }
    x = std::max(x, work.l);
    y = std::max(y, work.t);
    return Rect::xywh(x, y, size.w, size.h);
}

MenuLabel parse_menu_label(const std::wstring& text) {
    MenuLabel out;
    for (size_t i = 0; i < text.size(); ++i) {
        if (text[i] == L'&') {
            if (i + 1 < text.size() && text[i + 1] == L'&') {
                out.display += L'&';
                ++i;
            } else if (i + 1 < text.size() && out.key_index < 0) {
                out.key_index = static_cast<int>(out.display.size());
                out.key = static_cast<wchar_t>(std::towlower(text[i + 1]));
            }
            continue;  // a marker with nothing after it, or a second marker, is dropped
        }
        out.display += text[i];
    }
    return out;
}

int menu_next_selectable(const MenuItems& items, int from, int dir) {
    const int n = static_cast<int>(items.size());
    if (n == 0) return -1;
    int i = from;
    for (int step = 0; step < n; ++step) {
        i = from < 0 && step == 0 ? (dir > 0 ? 0 : n - 1) : ((i + dir) % n + n) % n;
        if (items[static_cast<size_t>(i)].selectable()) return i;
    }
    return -1;
}

int menu_first_selectable(const MenuItems& items) { return menu_next_selectable(items, -1, 1); }
int menu_last_selectable(const MenuItems& items) { return menu_next_selectable(items, -1, -1); }

std::vector<int> menu_access_matches(const MenuItems& items, wchar_t ch) {
    std::vector<int> out;
    const wchar_t want = static_cast<wchar_t>(std::towlower(ch));
    for (size_t i = 0; i < items.size(); ++i) {
        if (!items[i].selectable()) continue;
        if (parse_menu_label(items[i].text).key == want) out.push_back(static_cast<int>(i));
    }
    return out;
}

// ---- MenuPanel --------------------------------------------------------------------------------------

MenuPanel::MenuPanel(gfx::Resources& res, const MenuItems& items, float min_width)
    : res_(res), items_(&items), min_width_(min_width) {
    flags.hit_testable = false;
    icon_geo_.resize(items.size());
    icon_gen_.assign(items.size(), 0);
    compute();
}

void MenuPanel::compute() {
    const MenuMetrics& m = metrics;
    float content_w = 0, y = 0;
    tops_.clear();
    for (const MenuItem& it : *items_) {
        tops_.push_back(y);
        if (it.is_separator) {
            y += m.separator_h;
            continue;
        }
        y += m.item_h;
        float w = m.icon_col + 4 + std::ceil(res_.text.measure(parse_menu_label(it.text).display, style.font).width);
        if (!it.gesture.empty()) w += m.gesture_gap + std::ceil(res_.text.measure(it.gesture, style.font).width);
        w += m.arrow_w;
        content_w = std::max(content_w, w);
    }
    tops_.push_back(y);
    const float chrome = 2.0f + 2.0f * m.pad;
    natural_ = {std::max({content_w + chrome, m.min_width, min_width_}), y + chrome};
}

Rect MenuPanel::item_rect(int i) const {
    const float edge = 1.0f + metrics.pad;
    const float top = tops_[static_cast<size_t>(i)] + edge;
    return Rect{edge, top, natural_.w - edge, tops_[static_cast<size_t>(i) + 1] + edge};
}

int MenuPanel::item_at(Point p) const {
    for (size_t i = 0; i + 1 < tops_.size(); ++i) {
        if (item_rect(static_cast<int>(i)).contains(p)) return static_cast<int>(i);
    }
    return -1;
}

void MenuPanel::set_selected(int i) {
    if (i == selected) return;
    selected = i;
    if (i >= 0 && static_cast<size_t>(i) < items_->size()) {
        // The items belong to whoever opened the menu; only the panel's view of them is const.
        auto& items = const_cast<MenuItems&>(*items_);
        if (items[static_cast<size_t>(i)].on_hover) items[static_cast<size_t>(i)].on_hover(items, i);
    }
    invalidate();
}

void MenuPanel::paint(PaintContext& pc) {
    const MenuMetrics& m = metrics;
    pc.fill(bounds, tok::MenuBackground);
    pc.stroke(bounds, tok::MenuBorder);
    const float ox = bounds.l, oy = bounds.t;
    for (size_t k = 0; k < items_->size(); ++k) {
        const int i = static_cast<int>(k);
        const MenuItem& it = (*items_)[k];
        const Rect r = item_rect(i).offset(ox, oy);
        if (!r.intersects(pc.dirty)) continue;
        if (it.is_separator) {
            const float y = std::floor((r.t + r.b) / 2) + 0.5f;
            pc.line({r.l + 4, y}, {r.r - 4, y}, tok::MenuSeparator);
            continue;
        }
        const bool sel = (i == selected || it.marked) && it.enabled;
        if (sel) pc.fill(r, tok::MenuHoverBackground);
        const Token fg = !it.enabled ? tok::MenuDisabledForeground : (sel ? tok::MenuHoverForeground : tok::MenuForeground);
        const Token icon_fg = !it.enabled ? tok::MenuDisabledForeground : (sel ? tok::MenuHoverForeground : tok::MenuIconForeground);
        const Rect col = Rect::xywh(r.l, r.t, m.icon_col, r.height());
        const float cx = (col.l + col.r) / 2, cy = (col.t + col.b) / 2;
        if (it.icon) {
            const Rect ir = Rect::xywh(cx - 8, cy - 8, 16, 16);
            if (it.checked) pc.stroke(ir.inflate(3), icon_fg);
            Icon::draw(pc, *it.icon, ir, icon_fg, icon_geo_[k], icon_gen_[k]);
        } else if (it.picture) {
            images_.draw(pc, it.picture, Rect::xywh(cx - 8, cy - 8, 16, 16), it.enabled ? 1.0f : 0.4f);
        } else if (it.checked) {
            if (it.radio) {
                const D2D1_ELLIPSE e = D2D1::Ellipse(D2D1::Point2F(cx, cy), 3.0f, 3.0f);
                if (auto* b = pc.brush(icon_fg)) pc.dc->FillEllipse(e, b);
            } else {
                pc.line({cx - 5, cy + 0.5f}, {cx - 2, cy + 3.5f}, icon_fg, 1.6f);
                pc.line({cx - 2, cy + 3.5f}, {cx + 5, cy - 3.5f}, icon_fg, 1.6f);
            }
        }
        const MenuLabel label = parse_menu_label(it.text);
        const float tx = r.l + m.icon_col + 4;
        const float gw = it.gesture.empty() ? 0.0f : std::ceil(res_.text.measure(it.gesture, style.font).width);
        pc.text(label.display, Rect{tx, r.t, r.r - m.arrow_w - (gw > 0 ? gw + m.gesture_gap : 0.0f), r.b}, style.font, fg);
        if (label.key_index >= 0 && show_access) {
            const D2D1_SIZE_F full = res_.text.measure(label.display, style.font);
            const float x0 = tx + res_.text.measure(label.display.substr(0, static_cast<size_t>(label.key_index)), style.font).width;
            const float x1 = tx + res_.text.measure(label.display.substr(0, static_cast<size_t>(label.key_index) + 1), style.font).width;
            const float ty = r.t + (r.height() - full.height) / 2 + full.height - 2.0f;
            pc.line({x0, std::floor(ty) + 0.5f}, {x1, std::floor(ty) + 0.5f}, fg);
        }
        if (gw > 0) {
            pc.text(it.gesture, Rect{r.r - m.arrow_w - gw - 2, r.t, r.r - m.arrow_w, r.b}, style.font,
                    it.enabled ? (sel ? tok::MenuHoverForeground : tok::MenuGestureForeground) : tok::MenuDisabledForeground,
                    gfx::TextAlign::Trailing);
        }
        if (it.is_submenu()) {
            const float ax = r.r - 13, ay = (r.t + r.b) / 2;
            pc.line({ax, ay - 4}, {ax + 4, ay}, fg, 1.4f);
            pc.line({ax + 4, ay}, {ax, ay + 4}, fg, 1.4f);
        }
    }
}

// ---- MenuController ---------------------------------------------------------------------------------

void MenuController::teardown() {
    while (!levels_.empty()) {
        Level l = levels_.back();
        levels_.pop_back();
        pres_.hide(l.panel);
    }
    pending_level_ = pending_item_ = pressed_level_ = pressed_item_ = -1;
}

void MenuController::push_level(const MenuItems& items, Rect anchor, PopupSide side, float min_width, int opener) {
    auto panel = std::make_unique<MenuPanel>(res_, items, min_width);
    panel->show_access = show_access;
    const Rect r = place_popup(panel->natural_size(), anchor, side, pres_.work_area());
    MenuPanel* raw = panel.get();
    pres_.show(std::move(panel), r);
    levels_.push_back({&items, raw, r, opener});
}

void MenuController::open(const MenuItems& items, Rect anchor, PopupSide side, float min_width, bool select_first,
                          bool button_down) {
    if (is_open()) {  // replaced, not closed: on_closed is for a menu that went away
        pres_.stop_timer();
        teardown();
    }
    push_level(items, anchor, side, min_width, -1);
    button_held_ = button_down;
    if (select_first) select(0, menu_first_selectable(items));
}

void MenuController::close(MenuClose why) {
    if (closing_ || !is_open()) return;
    closing_ = true;
    pres_.stop_timer();
    teardown();
    closing_ = false;
    if (on_closed) on_closed(why);
}

void MenuController::close_levels_from(size_t level) {
    if (level == 0) {
        close(MenuClose::Dismissed);
        return;
    }
    while (levels_.size() > level) {
        Level l = levels_.back();
        levels_.pop_back();
        pres_.hide(l.panel);
    }
    pending_level_ = pending_item_ = -1;
}

void MenuController::select(size_t level, int index) {
    if (level >= levels_.size()) return;
    levels_[level].panel->set_selected(index);
    if (levels_.size() > level + 1 && levels_[level + 1].opener != index) close_levels_from(level + 1);
}

bool MenuController::open_submenu(size_t level, bool first) {
    if (level >= levels_.size()) return false;
    const int sel = levels_[level].panel->selected;
    if (sel < 0) return false;
    const MenuItem& it = (*levels_[level].items)[static_cast<size_t>(sel)];
    if (!it.is_submenu() || !it.enabled) return false;
    if (levels_.size() > level + 1) {
        if (levels_[level + 1].opener == sel) {
            if (first) select(level + 1, menu_first_selectable(it.sub));
            return true;
        }
        close_levels_from(level + 1);
    }
    const Level& lv = levels_[level];
    // The submenu abuts the whole panel, not the item inside its padding.
    const Rect item = lv.panel->item_rect(sel).offset(lv.rect.l, lv.rect.t);
    push_level(it.sub, Rect{lv.rect.l, item.t, lv.rect.r, item.b}, PopupSide::Beside, 0, sel);
    if (first) select(levels_.size() - 1, menu_first_selectable(it.sub));
    return true;
}

int MenuController::level_at(Point p) const {
    for (int i = static_cast<int>(levels_.size()) - 1; i >= 0; --i) {
        if (levels_[static_cast<size_t>(i)].rect.contains(p)) return i;
    }
    return -1;
}

const MenuItem* MenuController::item_at(Point p) const {
    const int L = level_at(p);
    if (L < 0) return nullptr;
    const Level& lv = levels_[static_cast<size_t>(L)];
    const int idx = lv.panel->item_at({p.x - lv.rect.l, p.y - lv.rect.t});
    if (idx < 0 || static_cast<size_t>(idx) >= lv.items->size()) return nullptr;
    const MenuItem& it = (*lv.items)[static_cast<size_t>(idx)];
    return it.is_separator ? nullptr : &it;
}

bool MenuController::pointer_moved(Point p) {
    const int L = level_at(p);
    if (L < 0) return false;
    const Level& lv = levels_[static_cast<size_t>(L)];
    const int idx = lv.panel->item_at({p.x - lv.rect.l, p.y - lv.rect.t});
    if (idx < 0) return true;
    const MenuItem& it = (*lv.items)[static_cast<size_t>(idx)];
    if (it.is_separator) return true;
    if (!it.enabled) {
        select(static_cast<size_t>(L), -1);
        return true;
    }
    if (lv.panel->selected == idx) return true;
    select(static_cast<size_t>(L), idx);
    if (it.is_submenu()) {
        pending_level_ = L;
        pending_item_ = idx;
        pres_.start_timer(kSubmenuDelayMs);
    } else {
        pending_level_ = pending_item_ = -1;
        pres_.stop_timer();
    }
    return true;
}

void MenuController::timer() {
    pres_.stop_timer();
    const int L = pending_level_, I = pending_item_;
    pending_level_ = pending_item_ = -1;
    if (L >= 0 && static_cast<size_t>(L) < levels_.size() && levels_[static_cast<size_t>(L)].panel->selected == I) {
        open_submenu(static_cast<size_t>(L), false);
    }
}

MenuController::Press MenuController::press(Point p) {
    const int L = level_at(p);
    if (L < 0) return Press::Outside;
    const Level& lv = levels_[static_cast<size_t>(L)];
    const int idx = lv.panel->item_at({p.x - lv.rect.l, p.y - lv.rect.t});
    button_held_ = false;
    pressed_level_ = pressed_item_ = -1;
    if (idx < 0) return Press::Inside;
    const MenuItem& it = (*lv.items)[static_cast<size_t>(idx)];
    if (!it.selectable()) return Press::Inside;
    select(static_cast<size_t>(L), idx);
    pressed_level_ = L;
    pressed_item_ = idx;
    if (it.is_submenu()) open_submenu(static_cast<size_t>(L), false);
    return Press::Inside;
}

MenuController::Press MenuController::release(Point p) {
    const bool held = button_held_;
    const int pl = pressed_level_, pi = pressed_item_;
    button_held_ = false;
    pressed_level_ = pressed_item_ = -1;
    const int L = level_at(p);
    if (L < 0) return Press::Outside;
    const Level& lv = levels_[static_cast<size_t>(L)];
    const int idx = lv.panel->item_at({p.x - lv.rect.l, p.y - lv.rect.t});
    if (idx < 0) return Press::Inside;
    const MenuItem& it = (*lv.items)[static_cast<size_t>(idx)];
    if (it.selectable() && !it.is_submenu() && ((pl == L && pi == idx) || held)) activate(static_cast<size_t>(L), idx);
    return Press::Inside;
}

void MenuController::activate(size_t level, int index) {
    const MenuItem item = (*levels_[level].items)[static_cast<size_t>(index)];  // the menu is about to go away
    close(MenuClose::Invoked);
    if (item.on_invoke) item.on_invoke();
    if (on_invoke) on_invoke(item);
}

bool MenuController::key(unsigned vk) {
    if (!is_open()) return false;
    const size_t D = levels_.size() - 1;
    Level& lv = levels_[D];
    const int sel = lv.panel->selected;
    switch (vk) {
        case VK_DOWN: select(D, menu_next_selectable(*lv.items, sel, 1)); return true;
        case VK_UP: select(D, menu_next_selectable(*lv.items, sel, -1)); return true;
        case VK_HOME: select(D, menu_first_selectable(*lv.items)); return true;
        case VK_END: select(D, menu_last_selectable(*lv.items)); return true;
        case VK_RIGHT:
            if (sel >= 0 && open_submenu(D, true)) return true;
            if (D == 0 && on_edge) on_edge(true);
            return true;
        case VK_LEFT:
            if (D > 0) {
                close_levels_from(D);
            } else if (on_edge) {
                on_edge(false);
            }
            return true;
        case VK_RETURN:
        case VK_SPACE:
            if (sel >= 0) {
                if ((*lv.items)[static_cast<size_t>(sel)].is_submenu()) {
                    open_submenu(D, true);
                } else {
                    activate(D, sel);
                }
            }
            return true;
        case VK_ESCAPE:
            if (D > 0) {
                close_levels_from(D);
            } else {
                close(MenuClose::Escape);
            }
            return true;
        case VK_F10:
            close(MenuClose::Dismissed);
            return true;
        default: return true;  // swallowed
    }
}

bool MenuController::character(wchar_t c) {
    if (!is_open()) return false;
    const size_t D = levels_.size() - 1;
    Level& lv = levels_[D];
    const std::vector<int> hits = menu_access_matches(*lv.items, c);
    if (hits.empty()) return false;
    if (hits.size() == 1) {
        select(D, hits[0]);
        if ((*lv.items)[static_cast<size_t>(hits[0])].is_submenu()) {
            open_submenu(D, true);
        } else {
            activate(D, hits[0]);
        }
        return true;
    }
    const int cur = lv.panel->selected;
    int next = hits[0];
    for (int h : hits) {
        if (h > cur) {
            next = h;
            break;
        }
    }
    select(D, next);
    return true;
}

}  // namespace bb::ui
