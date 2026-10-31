#include "ui/popups/popup_layer.h"

#include "ui/core/layout.h"

#include <algorithm>
#include <cmath>

namespace bb::ui {

// ---- TooltipBubble ----------------------------------------------------------------------------------

TooltipBubble::TooltipBubble(gfx::Resources& res, std::wstring text) : res_(res), text_(std::move(text)) {
    flags.hit_testable = false;
    gfx::LayoutOptions opt;
    opt.wrap = true;
    opt.max_width = 300.0f;
    DWRITE_TEXT_METRICS m{};
    if (auto layout = res_.text.make_layout(text_, style.font, opt)) layout->GetMetrics(&m);
    natural_ = {std::ceil(m.widthIncludingTrailingWhitespace) + 16.0f, std::ceil(m.height) + 10.0f};
}

void TooltipBubble::paint(PaintContext& pc) {
    pc.fill(bounds, tok::OverlayBackground);
    pc.stroke(bounds, tok::OverlayBorder);
    gfx::LayoutOptions opt;
    opt.wrap = true;
    opt.max_width = bounds.width() - 16.0f;
    opt.max_height = bounds.height() - 10.0f;
    if (auto layout = pc.res.text.make_layout(text_, style.font, opt)) {
        pc.dc->DrawTextLayout(D2D1::Point2F(bounds.l + 8, bounds.t + 5), layout.Get(), pc.brush(tok::TextPrimary),
                              D2D1_DRAW_TEXT_OPTIONS_CLIP);
    }
}

// ---- PopupLayer -------------------------------------------------------------------------------------

PopupLayer::PopupLayer(gfx::Resources& res) : res_(res), menus_(res, *this) {
    flags.hit_testable = false;
    menus_.on_closed = [this](MenuClose why) {
        if (root()) root()->set_modal(nullptr);
        MenuOpen o = std::move(opt_);
        opt_ = {};
        pending_invoke_ = why == MenuClose::Invoked ? std::move(o.on_invoke) : nullptr;
        if (o.on_closed) o.on_closed(why);
    };
    menus_.on_invoke = [this](const MenuItem& item) {
        auto cb = std::move(pending_invoke_);
        pending_invoke_ = nullptr;
        if (cb) cb(item);
    };
    menus_.on_edge = [this](bool right) {
        const auto cb = opt_.on_edge;  // a copy: the callback may open another menu and replace opt_
        if (cb) cb(right);
    };
}

PopupLayer::~PopupLayer() {
    menus_.on_closed = nullptr;
    menus_.close();
}

PopupLayer* PopupLayer::install(Widget& host) {
    UiRoot* root = host.root();
    if (!root) return nullptr;
    auto* layer = host.emplace<PopupLayer>(root->resources());
    root->set_popup_layer(layer);
    root->on_hover_changed = [layer](Widget* w) { layer->hover_changed(w); };
    return layer;
}

PopupLayer* PopupLayer::of(const Widget& w) {
    UiRoot* r = w.root();
    return r ? static_cast<PopupLayer*>(r->popup_layer()) : nullptr;
}

void PopupLayer::open_menu(const MenuItems& items, MenuOpen opt) {
    if (menus_.is_open()) menus_.close(MenuClose::Dismissed);
    hide_tip();
    opt_ = std::move(opt);
    menus_.open(items, opt_.anchor, opt_.side, opt_.min_width, opt_.select_first, opt_.button_down);
    if (root()) root()->set_modal(this);
}

Rect PopupLayer::work_area() {
    if (external_) return external_->work_area();
    return root() ? Rect{0, 0, root()->size().w, root()->size().h} : bounds;
}

Widget* PopupLayer::show(std::unique_ptr<Widget> content, Rect rect) {
    if (external_) return external_->show(std::move(content), rect);
    Widget* w = content.get();
    add_child(std::move(content));
    shown_.push_back({w, rect});
    w->measure_tree({rect.width(), rect.height()});
    w->arrange(rect);
    invalidate(rect.inflate(1));
    return w;
}

void PopupLayer::hide(Widget* content) {
    if (external_) {
        external_->hide(content);
        return;
    }
    const auto it = std::find_if(shown_.begin(), shown_.end(), [&](const Shown& s) { return s.w == content; });
    if (it == shown_.end()) return;
    const Rect r = it->rect;
    shown_.erase(it);
    remove_child(content);
    invalidate(r.inflate(1));
}

void PopupLayer::start_timer(unsigned ms) {
    if (root()) root()->start_timer(this, kMenuTimer, ms);
}

void PopupLayer::stop_timer() {
    if (root()) root()->stop_timer(this, kMenuTimer);
}

void PopupLayer::arrange(Rect) {
    bounds = root() ? Rect{0, 0, root()->size().w, root()->size().h} : Rect{};
    for (const Shown& s : shown_) s.w->arrange(s.rect);
}

bool PopupLayer::on_mouse_down(const MouseEvent& e) {
    if (menus_.press(e.pt) == MenuController::Press::Outside) {
        const auto cb = opt_.on_outside_press;
        const bool handled = cb && cb(e.pt);
        if (!handled) menus_.close(MenuClose::Dismissed);
    }
    return true;
}

bool PopupLayer::on_mouse_up(const MouseEvent& e) {
    menus_.release(e.pt);
    return true;
}

bool PopupLayer::on_mouse_move(const MouseEvent& e) {
    if (!menus_.pointer_moved(e.pt)) {
        const auto cb = opt_.on_outside_move;
        if (cb) cb(e.pt);
    }
    // Resting on an item with something to say (why a greyed row cannot run) shows it, as a control's tooltip
    // would. Spec: MainWindow.CommandBars (ToolTip = row.Reason, ShowOnDisabled).
    const MenuItem* item = menus_.item_at(e.pt);
    if (item != tip_item_) {
        tip_item_ = item;
        hide_tip();
        root()->stop_timer(this, kTipTimer);
        menu_tip_ = item ? item->tooltip : std::wstring();
        if (!menu_tip_.empty()) root()->start_timer(this, kTipTimer, kTipDelayMs);
    }
    return true;
}

bool PopupLayer::on_key_down(const KeyEvent& e) {
    menus_.key(e.vk);
    return true;
}

bool PopupLayer::on_char(wchar_t c) {
    menus_.character(c);
    return true;
}

void PopupLayer::on_timer(int id) {
    if (id == kMenuTimer) {
        menus_.timer();
    } else if (id == kTipTimer) {
        root()->stop_timer(this, kTipTimer);
        if (menus_.is_open()) {
            if (!menu_tip_.empty() && tip_item_ && menus_.item_at(root()->pointer()) == tip_item_) show_tip(menu_tip_, root()->pointer());
            return;
        }
        if (tip_target_ && root()->hovered() && tip_target_->is_ancestor_of(root()->hovered())) {
            show_tip(tip_target_->tooltip, root()->pointer());
        }
    }
}

void PopupLayer::on_modal_cancel() { menus_.close(MenuClose::Dismissed); }

void PopupLayer::hover_changed(Widget* w) {
    hide_tip();
    if (root()) root()->stop_timer(this, kTipTimer);
    tip_target_ = nullptr;
    if (!w || menus_.is_open()) return;
    for (Widget* a = w; a; a = a->parent()) {
        if (!a->tooltip.empty()) {
            tip_target_ = a;
            root()->start_timer(this, kTipTimer, kTipDelayMs);
            return;
        }
    }
}

void PopupLayer::show_tip(const std::wstring& text, Point p) {
    hide_tip();
    if (text.empty()) return;
    auto bubble = std::make_unique<TooltipBubble>(res_, text);
    const Size sz = bubble->natural_size();
    tip_text_ = text;
    tip_rect_ = place_popup(sz, Rect{p.x, p.y, p.x + 1, p.y + 20}, PopupSide::Below, work_area());
    tip_ = show(std::move(bubble), tip_rect_);
}

void PopupLayer::hide_tip() {
    if (!tip_) return;
    Widget* t = tip_;
    tip_ = nullptr;
    hide(t);
}

// ---- detached rendering -----------------------------------------------------------------------------

std::unique_ptr<gfx::BitmapSurface> render_menu_detached(gfx::Resources& res, const MenuItems& items,
                                                         const std::vector<int>& path, float dpi) {
    UiRoot root(res);
    auto* content = root.set_content(std::make_unique<Panel>());
    PopupLayer* layer = PopupLayer::install(*content);
    root.layout({1600, 1200});
    constexpr float kMargin = 12.0f;
    MenuOpen o;
    o.anchor = Rect{kMargin, kMargin - 1, kMargin, kMargin - 1};
    layer->open_menu(items, o);
    for (size_t k = 0; k < path.size(); ++k) {
        layer->menus().select(k, path[k]);
        if (k + 1 < path.size()) layer->menus().open_submenu(k, false);
    }
    Rect u;
    for (const auto& lv : layer->menus().levels()) u = u.united(lv.rect);
    const float s = dpi / 96.0f;
    HRESULT hr = S_OK;
    auto surf = gfx::BitmapSurface::create(res.dev, static_cast<UINT>(std::ceil((u.r + kMargin) * s)),
                                           static_cast<UINT>(std::ceil((u.b + kMargin) * s)), dpi, &hr);
    if (!surf) return nullptr;
    root.layout({surf->width_dip(), surf->height_dip()});
    if (FAILED(root.paint(*surf))) return nullptr;
    layer->close_menu();
    return surf;
}

}  // namespace bb::ui
