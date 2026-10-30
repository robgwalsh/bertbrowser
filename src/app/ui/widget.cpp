#include "ui/widget.h"

#include <algorithm>
#include <cmath>
#include <cstring>

namespace bb::ui {

// ---- PaintContext ----------------------------------------------------------------------------------

void PaintContext::fill(const Rect& r, Token t) const {
    if (ID2D1SolidColorBrush* b = brush(t)) {
        dc->FillRectangle(r.d2d(), b);
    }
}

void PaintContext::stroke(const Rect& r, Token t, float w) const {
    if (ID2D1SolidColorBrush* b = brush(t)) {
        dc->DrawRectangle(D2D1::RectF(r.l + w * 0.5f, r.t + w * 0.5f, r.r - w * 0.5f, r.b - w * 0.5f), b, w);
    }
}

void PaintContext::line(Point a, Point b, Token t, float w) const {
    if (ID2D1SolidColorBrush* br = brush(t)) {
        dc->DrawLine(D2D1::Point2F(a.x, a.y), D2D1::Point2F(b.x, b.y), br, w);
    }
}

void PaintContext::text(std::wstring_view s, const Rect& r, const gfx::FontSpec& font, Token fg,
                        gfx::TextAlign align) const {
    if (s.empty() || r.empty()) {
        return;
    }
    gfx::LayoutOptions opt;
    opt.max_width = r.width();
    opt.max_height = r.height();
    opt.ellipsis = true;
    opt.align = align;
    opt.vcenter = true;
    auto layout = res.text.make_layout(s, font, opt);
    if (layout) {
        dc->DrawTextLayout(D2D1::Point2F(r.l, r.t), layout.Get(), brush(fg), D2D1_DRAW_TEXT_OPTIONS_CLIP);
    }
}

// ---- Widget ----------------------------------------------------------------------------------------

Widget::~Widget() {
    if (root_) {
        root_->forget(this);
    }
}

gfx::Resources& Widget::res() const { return root_->resources(); }

void Widget::attach(UiRoot* r) {
    root_ = r;
    for (auto& c : children_) {
        c->attach(r);
    }
}

void Widget::add_child(std::unique_ptr<Widget> child) {
    child->parent_ = this;
    child->attach(root_);
    children_.push_back(std::move(child));
    request_layout();
    invalidate();
}

std::unique_ptr<Widget> Widget::remove_child(Widget* child) {
    auto it = std::find_if(children_.begin(), children_.end(), [&](auto& c) { return c.get() == child; });
    if (it == children_.end()) {
        return nullptr;
    }
    std::unique_ptr<Widget> out = std::move(*it);
    children_.erase(it);
    if (root_) {
        std::vector<Widget*> stack{out.get()};
        while (!stack.empty()) {
            Widget* w = stack.back();
            stack.pop_back();
            root_->forget(w);
            for (auto& c : w->children_) {
                stack.push_back(c.get());
            }
        }
    }
    out->parent_ = nullptr;
    out->attach(nullptr);
    request_layout();
    invalidate();
    return out;
}

Widget* Widget::find(std::string_view n) {
    if (name == n) {
        return this;
    }
    for (auto& c : children_) {
        if (Widget* w = c->find(n)) {
            return w;
        }
    }
    return nullptr;
}

Widget* Widget::find_automation(std::string_view id) {
    if (automation_id == id) {
        return this;
    }
    for (auto& c : children_) {
        if (Widget* w = c->find_automation(id)) {
            return w;
        }
    }
    return nullptr;
}

bool Widget::is_ancestor_of(const Widget* w) const {
    for (; w; w = w->parent_) {
        if (w == this) {
            return true;
        }
    }
    return false;
}

bool Widget::shown() const {
    for (const Widget* w = this; w; w = w->parent_) {
        if (!w->flags.visible) {
            return false;
        }
    }
    return true;
}

void Widget::set_visible(bool v) {
    if (flags.visible == v) return;
    invalidate();
    flags.visible = v;
    request_layout();
    invalidate();
}

void Widget::set_enabled(bool v) {
    if (flags.enabled == v) return;
    flags.enabled = v;
    invalidate();
}

bool Widget::focused() const { return root_ && root_->focused() == this; }
bool Widget::hovered() const { return root_ && root_->hovered() == this; }

void Widget::invalidate() { invalidate(bounds); }
void Widget::invalidate(const Rect& r) {
    if (root_) root_->invalidate(r);
}
void Widget::request_layout() {
    if (root_) root_->request_layout();
}

Size Widget::measure(Size avail) {
    Size m;
    for (auto& c : children_) {
        if (!c->flags.visible) continue;
        const Size s = c->measure_tree(avail);
        m.w = std::max(m.w, s.w);
        m.h = std::max(m.h, s.h);
    }
    return fixed_or(m);
}

void Widget::arrange(Rect r) {
    bounds = r;
    for (auto& c : children_) {
        if (c->flags.visible) c->arrange(r);
    }
}

void Widget::paint(PaintContext& pc) {
    if (style.bg != kNoToken) {
        pc.fill(bounds, style.bg);
    }
}

void Widget::paint_children(PaintContext& pc) {
    for (auto& c : children_) {
        c->paint_tree(pc);
    }
}

void Widget::paint_tree(PaintContext& pc) {
    if (!flags.visible || !bounds.intersects(pc.dirty)) {
        return;
    }
    paint(pc);
    if (children_.empty()) {
        return;
    }
    if (flags.clip_children) {
        pc.clip(bounds.intersect(pc.dirty));
        paint_children(pc);
        pc.unclip();
    } else {
        paint_children(pc);
    }
}

Widget* Widget::hit_test(Point p) {
    if (!flags.visible || !flags.hit_testable || !bounds.contains(p)) {
        return nullptr;
    }
    for (auto it = children_.rbegin(); it != children_.rend(); ++it) {
        if (Widget* h = (*it)->hit_test(p)) {
            return h;
        }
    }
    return this;
}

// ---- Clipboard -------------------------------------------------------------------------------------

Clipboard Clipboard::system(HWND owner) {
    Clipboard c;
    c.read = [owner] {
        std::wstring text;
        if (!OpenClipboard(owner)) {
            return text;
        }
        if (const HANDLE h = GetClipboardData(CF_UNICODETEXT)) {
            if (const auto* p = static_cast<const wchar_t*>(GlobalLock(h))) {
                text = p;
                GlobalUnlock(h);
            }
        }
        CloseClipboard();
        return text;
    };
    c.write = [owner](const std::wstring& text) {
        if (!OpenClipboard(owner)) {
            return;
        }
        EmptyClipboard();
        const size_t bytes = (text.size() + 1) * sizeof(wchar_t);
        if (const HGLOBAL h = GlobalAlloc(GMEM_MOVEABLE, bytes)) {
            if (void* p = GlobalLock(h)) {
                std::memcpy(p, text.c_str(), bytes);
                GlobalUnlock(h);
                SetClipboardData(CF_UNICODETEXT, h);
            }
        }
        CloseClipboard();
    };
    return c;
}

// ---- UiRoot ----------------------------------------------------------------------------------------

UiRoot::UiRoot(gfx::Resources& res) : res_(res) {
    clipboard = Clipboard::system(nullptr);
}

UiRoot::~UiRoot() = default;

uint64_t UiRoot::default_clock() { return GetTickCount64(); }

void UiRoot::install(std::unique_ptr<Widget> w) {
    content_ = std::move(w);
    content_->attach(this);
    request_layout();
}

void UiRoot::notify_invalidate() {
    if (on_invalidate) on_invalidate();
}

void UiRoot::invalidate(const Rect& r) {
    dirty_ = dirty_.united(r);
    notify_invalidate();
}

void UiRoot::invalidate_all() { invalidate(Rect{0, 0, size_.w, size_.h}); }

void UiRoot::layout(Size size) {
    size_ = size;
    layout_pending_ = false;
    if (content_) {
        content_->measure_tree(size);
        content_->arrange(Rect{0, 0, size.w, size.h});
    }
    invalidate_all();
}

HRESULT UiRoot::paint(gfx::Surface& surface) {
    if (layout_pending_ || (size_.w <= 0 && size_.h <= 0)) {
        layout(layout_pending_ && size_.w > 0 ? size_ : Size{surface.width_dip(), surface.height_dip()});
    }
    const Rect all{0, 0, surface.width_dip(), surface.height_dip()};
    const Rect d = dirty_.intersect(all);
    if (d.empty() || !content_) {
        dirty_ = {};
        return S_FALSE;
    }
    HRESULT hr = surface.begin_draw();
    if (FAILED(hr)) {
        return hr;
    }
    ID2D1DeviceContext* dc = surface.context();
    PaintContext pc{dc, res_, *this, d};
    dc->PushAxisAlignedClip(d.d2d(), D2D1_ANTIALIAS_MODE_ALIASED);
    pc.fill(all, tok::WindowBackground);
    content_->paint_tree(pc);
    dc->PopAxisAlignedClip();
    const float s = surface.scale();
    const RECT px{static_cast<LONG>(std::floor(d.l * s)), static_cast<LONG>(std::floor(d.t * s)),
                  static_cast<LONG>(std::ceil(d.r * s)), static_cast<LONG>(std::ceil(d.b * s))};
    hr = surface.end_draw(&px);
    dirty_ = {};
    ++paint_count_;
    return hr;
}

template <class F>
bool UiRoot::bubble(Widget* from, F&& f) {
    for (Widget* w = from; w; w = w->parent_) {
        if (f(w)) {
            return true;
        }
    }
    return false;
}

static Mods merge(Mods a, Mods b) { return {a.shift || b.shift, a.ctrl || b.ctrl, a.alt || b.alt}; }

bool UiRoot::mouse_down(Point p, MouseButton b, Mods m, int clicks) {
    last_pt_ = p;
    if (!modal_ && on_hover_changed) on_hover_changed(nullptr);
    Widget* target = modal_ ? modal_ : (capture_ ? capture_ : hit_test(p));
    if (!target) {
        return false;
    }
    for (Widget* w = modal_ ? nullptr : target; w; w = w->parent_) {
        if (w->flags.focusable && w->flags.enabled) {
            if (w->flags.click_focus) set_focus(w);
            break;
        }
    }
    if (!modal_) {
        for (Widget* w = target; w; w = w->parent_) w->on_inner_activity();
    }
    const MouseEvent ev{p, b, merge(m, mods_), clicks};
    return bubble(target, [&](Widget* w) { return w->flags.enabled && w->on_mouse_down(ev); });
}

bool UiRoot::mouse_up(Point p, MouseButton b, Mods m) {
    last_pt_ = p;
    Widget* target = modal_ ? modal_ : (capture_ ? capture_ : hit_test(p));
    bool handled = false;
    if (target) {
        const MouseEvent ev{p, b, merge(m, mods_), 1};
        handled = bubble(target, [&](Widget* w) { return w->flags.enabled && w->on_mouse_up(ev); });
    }
    capture_ = nullptr;
    return handled;
}

bool UiRoot::mouse_move(Point p, Mods m) {
    last_pt_ = p;
    if (modal_) {
        const MouseEvent ev{p, MouseButton::Left, merge(m, mods_), 1};
        return modal_->on_mouse_move(ev);
    }
    Widget* h = hit_test(p);
    if (h != hover_) {
        Widget* old = hover_;
        hover_ = h;
        if (old) old->on_mouse_leave();
        if (h) h->on_mouse_enter();
        if (on_hover_changed) on_hover_changed(h);
    }
    Widget* target = capture_ ? capture_ : h;
    if (!target) {
        return false;
    }
    const MouseEvent ev{p, MouseButton::Left, merge(m, mods_), 1};
    return bubble(target, [&](Widget* w) { return w->flags.enabled && w->on_mouse_move(ev); });
}

bool UiRoot::mouse_leave() {
    if (hover_) {
        Widget* old = hover_;
        hover_ = nullptr;
        old->on_mouse_leave();
        if (on_hover_changed) on_hover_changed(nullptr);
    }
    return true;
}

bool UiRoot::wheel(Point p, int delta, Mods m) {
    last_pt_ = p;
    Widget* target = modal_ ? modal_ : hit_test(p);
    const Mods eff = merge(m, mods_);
    return target && bubble(target, [&](Widget* w) { return w->flags.enabled && w->on_wheel(p, delta, eff); });
}

bool UiRoot::key_down(unsigned vk) {
    if (vk == VK_SHIFT) { mods_.shift = true; return true; }
    if (vk == VK_CONTROL) { mods_.ctrl = true; return true; }
    if (vk == VK_MENU) {
        const bool first = !mods_.alt;
        mods_.alt = true;
        if (first && on_alt_key) on_alt_key(true);
        return true;
    }
    const KeyEvent ev{vk, mods_};
    if (vk == VK_ESCAPE && capture_ && !modal_ && capture_->cancel_capture()) {
        capture_ = nullptr;
        return true;
    }
    if (!modal_ && on_preview_key && on_preview_key(ev)) {
        return true;
    }
    if (modal_) {
        modal_->on_key_down(ev);  // an open menu swallows every key
        return true;
    }
    Widget* target = focus_ ? focus_ : content_.get();
    if (target && bubble(target, [&](Widget* w) { return w->flags.enabled && w->on_key_down(ev); })) {
        return true;
    }
    if (vk == VK_TAB && !mods_.ctrl && !mods_.alt) {
        const bool moved = focus_next(mods_.shift);
        if (moved) {
            for (Widget* w = focus_; w; w = w->parent_) w->on_inner_activity();
        }
        return moved;
    }
    return false;
}

bool UiRoot::key_up(unsigned vk) {
    if (vk == VK_SHIFT) { mods_.shift = false; return true; }
    if (vk == VK_CONTROL) { mods_.ctrl = false; return true; }
    if (vk == VK_MENU) {
        const bool was = mods_.alt;
        mods_.alt = false;
        if (was && on_alt_key) on_alt_key(false);
        return true;
    }
    const KeyEvent ev{vk, mods_};
    Widget* target = modal_ ? modal_ : (focus_ ? focus_ : content_.get());
    return target && bubble(target, [&](Widget* w) { return w->flags.enabled && w->on_key_up(ev); });
}

bool UiRoot::char_input(wchar_t c) {
    Widget* target = modal_ ? modal_ : (focus_ ? focus_ : content_.get());
    return target && bubble(target, [&](Widget* w) { return w->flags.enabled && w->on_char(c); });
}

bool UiRoot::context_key() {
    if (modal_) return true;  // an open menu has the keyboard; no second one opens over it
    Widget* target = focus_ ? focus_ : content_.get();
    return target && bubble(target, [&](Widget* w) { return w->flags.enabled && w->on_context_key(); });
}

Cursor UiRoot::cursor() const {
    Widget* w = capture_ ? capture_ : hover_;
    return w ? w->cursor_at(last_pt_) : Cursor::Arrow;
}

void UiRoot::set_modal(Widget* w) {
    const bool was = modal_ != nullptr;
    modal_ = w;
    capture_ = nullptr;
    if (w && hover_) {
        Widget* old = hover_;
        hover_ = nullptr;
        old->on_mouse_leave();
    }
    if (was != (w != nullptr) && on_modal_changed) on_modal_changed(w != nullptr);
}

void UiRoot::cancel_modal() {
    if (Widget* m = modal_) {
        modal_ = nullptr;
        if (on_modal_changed) on_modal_changed(false);
        m->on_modal_cancel();
    }
}

void UiRoot::set_focus(Widget* w) {
    if (w == focus_) {
        return;
    }
    Widget* old = focus_;
    focus_ = w;
    if (old) {
        old->on_focus(false);
        old->invalidate();
    }
    if (w) {
        w->on_focus(true);
        w->invalidate();
    }
}

void UiRoot::collect_tab_stops(Widget* w, std::vector<Widget*>& out) {
    if (!w->flags.visible || !w->flags.enabled) {
        return;
    }
    if (w->flags.tab_stop && w->flags.focusable) {
        out.push_back(w);
    }
    for (auto& c : w->children()) {
        collect_tab_stops(c.get(), out);
    }
}

bool Widget::holds(const Widget* w) const {
    if (!w) return false;
    if (this == w) return true;
    for (const auto& c : children_) {
        if (c->holds(w)) return true;
    }
    return false;
}

bool UiRoot::contains(const Widget* w) const { return content_ && content_->holds(w); }

bool UiRoot::focus_next(bool reverse) {
    std::vector<Widget*> stops;
    Widget* scope = tab_scope ? tab_scope() : nullptr;
    if (scope) collect_tab_stops(scope, stops);
    else if (content_) collect_tab_stops(content_.get(), stops);
    if (stops.empty()) {
        return false;
    }
    const auto it = std::find(stops.begin(), stops.end(), focus_);
    size_t next;
    if (it == stops.end()) {
        next = reverse ? stops.size() - 1 : 0;
    } else {
        const size_t i = static_cast<size_t>(it - stops.begin());
        next = reverse ? (i + stops.size() - 1) % stops.size() : (i + 1) % stops.size();
    }
    set_focus(stops[next]);
    return true;
}

void UiRoot::start_timer(Widget* w, int id, unsigned interval_ms) {
    for (auto& t : timers_) {
        if (t.w == w && t.id == id) {
            t.interval = interval_ms;
            t.due = now_ms() + interval_ms;
            return;
        }
    }
    timers_.push_back({w, id, interval_ms, now_ms() + interval_ms});
    if (on_timers_changed) on_timers_changed();
}

void UiRoot::stop_timer(Widget* w, int id) {
    const auto n = std::erase_if(timers_, [&](const Timer& t) { return t.w == w && t.id == id; });
    if (n && on_timers_changed) on_timers_changed();
}

void UiRoot::tick() {
    const uint64_t now = now_ms();
    std::vector<Timer> due;
    for (auto& t : timers_) {
        if (t.due <= now) {
            due.push_back(t);
            t.due = now + t.interval;
        }
    }
    for (const Timer& t : due) {
        const bool present = std::any_of(timers_.begin(), timers_.end(),
                                         [&](const Timer& x) { return x.w == t.w && x.id == t.id; });
        if (present) {
            t.w->on_timer(t.id);
        }
    }
}

void UiRoot::forget(Widget* w) {
    if (focus_ == w) focus_ = nullptr;
    if (hover_ == w) hover_ = nullptr;
    if (capture_ == w) capture_ = nullptr;
    if (modal_ == w) {
        modal_ = nullptr;
        if (on_modal_changed) on_modal_changed(false);
    }
    if (popup_layer_ == w) popup_layer_ = nullptr;
    const auto n = std::erase_if(timers_, [&](const Timer& t) { return t.w == w; });
    if (n && on_timers_changed) on_timers_changed();
}

}  // namespace bb::ui
