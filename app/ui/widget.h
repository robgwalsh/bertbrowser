// The retained widget tree. A Widget has a name, an automation id, bounds (absolute, in window DIPs),
// children, flags and style token ids, and the usual virtuals: measure/arrange/paint/hit_test, mouse,
// key, char, focus. Invalidation unions into the root's dirty rect; the root paints that rect only.
// Widgets hold token ids, never brushes.
#pragma once
#include "gfx/palette.h"
#include "gfx/resources.h"
#include "gfx/surface.h"
#include "gfx/text_cache.h"
#include "ui/geom.h"

#include <cstdint>
#include <functional>
#include <memory>
#include <string>
#include <string_view>
#include <vector>

namespace bb::ui {

using gfx::ComPtr;
using gfx::kNoToken;
using gfx::Token;
namespace tok = gfx::tok;

class Widget;
class UiRoot;

struct Mods {
    bool shift = false, ctrl = false, alt = false;
};
enum class MouseButton { Left, Right, Middle };
struct MouseEvent {
    Point pt;
    MouseButton button = MouseButton::Left;
    Mods mods;
    int clicks = 1;  // 2 = double click
};
struct KeyEvent {
    unsigned vk = 0;
    Mods mods;
};
enum class Cursor { Arrow, IBeam, SizeWE, SizeNS, Hand };
enum class Dock { Fill, Left, Top, Right, Bottom };

struct Flags {
    bool visible = true;
    bool enabled = true;
    bool focusable = false;  // can take focus by click or code
    bool tab_stop = false;   // and by Tab
    bool clip_children = false;
    bool hit_testable = true;
};

struct Style {
    Token bg = kNoToken;
    Token fg = tok::TextPrimary;
    Token border = kNoToken;
    gfx::FontSpec font;
};

// What a widget paints with. All coordinates are DIPs; the device context's DPI does the scaling.
struct PaintContext {
    ID2D1DeviceContext* dc;
    gfx::Resources& res;
    UiRoot& root;
    Rect dirty;

    ID2D1SolidColorBrush* brush(Token t) const { return res.palette.brush(t); }
    void fill(const Rect& r, Token t) const;
    void stroke(const Rect& r, Token t, float width = 1.0f) const;  // inside the rect
    void line(Point a, Point b, Token t, float width = 1.0f) const;
    // One-off text (not cached): a label, a header. Cells that repeat go through res.layouts.
    void text(std::wstring_view s, const Rect& r, const gfx::FontSpec& font, Token fg,
              gfx::TextAlign align = gfx::TextAlign::Leading) const;
    void clip(const Rect& r) const { dc->PushAxisAlignedClip(r.d2d(), D2D1_ANTIALIAS_MODE_ALIASED); }
    void unclip() const { dc->PopAxisAlignedClip(); }
};

class Widget {
public:
    Widget() = default;
    virtual ~Widget();
    Widget(const Widget&) = delete;
    Widget& operator=(const Widget&) = delete;

    // ---- identity and data ----
    std::string name;
    std::string automation_id;
    std::wstring tooltip;  // shown by a TooltipHost once the pointer rests on the widget
    Rect bounds;  // absolute, window DIPs; set by arrange
    Flags flags;
    Style style;
    // Layout hints read by containers. Negative width/height mean "size to content".
    float width = -1, height = -1;
    float flex = 0;  // share of leftover space in a StackPanel
    Dock dock = Dock::Fill;
    // Non-client role for the window's WM_NCHITTEST (HTCAPTION, HTCLOSE, ...); 0 = none.
    int nc_hit = 0;

    // ---- tree ----
    Widget* parent() const { return parent_; }
    bool holds(const Widget* w) const;  // w is this or below it (never dereferences w)
    UiRoot* root() const { return root_; }
    const std::vector<std::unique_ptr<Widget>>& children() const { return children_; }
    template <class T>
    T* add(std::unique_ptr<T> child) {
        T* raw = child.get();
        add_child(std::move(child));
        return raw;
    }
    template <class T, class... A>
    T* emplace(A&&... args) {
        return add(std::make_unique<T>(std::forward<A>(args)...));
    }
    void add_child(std::unique_ptr<Widget> child);
    std::unique_ptr<Widget> remove_child(Widget* child);
    Widget* find(std::string_view n);
    Widget* find_automation(std::string_view id);
    bool is_ancestor_of(const Widget* w) const;

    // ---- state ----
    bool shown() const;  // visible, and so are all its ancestors
    void set_visible(bool v);
    void set_enabled(bool v);
    bool focused() const;
    bool hovered() const;
    Size desired() const { return desired_; }

    // ---- invalidation ----
    void invalidate();
    void invalidate(const Rect& r);
    void request_layout();

    // ---- layout ----
    // Default: the fixed size if given, else the largest child. `avail` may be kInf.
    virtual Size measure(Size avail);
    // What containers call: measure, and remember the answer in desired().
    Size measure_tree(Size avail) {
        desired_ = measure(avail);
        return desired_;
    }
    // Default: each child fills this widget's bounds.
    virtual void arrange(Rect r);

    // ---- painting ----
    virtual void paint(PaintContext& pc);  // own content; the default fills style.bg
    virtual void paint_children(PaintContext& pc);
    void paint_tree(PaintContext& pc);

    // ---- input ----
    virtual Widget* hit_test(Point p);
    virtual bool on_mouse_down(const MouseEvent&) { return false; }
    virtual bool on_mouse_up(const MouseEvent&) { return false; }
    virtual bool on_mouse_move(const MouseEvent&) { return false; }
    virtual bool on_wheel(Point, int /*delta*/, Mods) { return false; }
    virtual void on_mouse_enter() {}
    virtual void on_mouse_leave() {}
    virtual bool on_key_down(const KeyEvent&) { return false; }
    virtual bool on_key_up(const KeyEvent&) { return false; }
    virtual bool on_char(wchar_t) { return false; }
    virtual void on_focus(bool) {}
    virtual void on_timer(int) {}
    // The root took the modal away from this widget (window deactivated, capture cancelled by the system).
    virtual void on_modal_cancel() {}
    virtual Cursor cursor_at(Point) const { return Cursor::Arrow; }
    // IME: where the caret is (window DIPs) and the composition string as it changes. Edit implements.
    virtual bool ime_caret_rect(Rect*) { return false; }
    virtual void on_ime_composition(const std::wstring&, size_t /*cursor*/) {}
    virtual void on_ime_commit(const std::wstring&) {}
    virtual void on_ime_end() {}

protected:
    // Used by containers: honour fixed width/height over a measured size.
    Size fixed_or(Size content) const {
        return {width >= 0 ? width : content.w, height >= 0 ? height : content.h};
    }
    gfx::Resources& res() const;

private:
    friend class UiRoot;
    void attach(UiRoot* r);
    Widget* parent_ = nullptr;
    UiRoot* root_ = nullptr;
    std::vector<std::unique_ptr<Widget>> children_;
    Size desired_;
};

struct Clipboard {
    std::function<std::wstring()> read;
    std::function<void(const std::wstring&)> write;
    // The system clipboard. A harness installs a fake instead and never touches the user's.
    static Clipboard system(HWND owner);
};

class UiRoot {
public:
    explicit UiRoot(gfx::Resources& res);
    ~UiRoot();
    UiRoot(const UiRoot&) = delete;
    UiRoot& operator=(const UiRoot&) = delete;

    gfx::Resources& resources() const { return res_; }

    template <class T>
    T* set_content(std::unique_ptr<T> w) {
        T* raw = w.get();
        install(std::move(w));
        return raw;
    }
    Widget* content() const { return content_.get(); }
    // Destroys the widget tree now rather than with the root: a tree whose widgets call into the core
    // (a preview pane closing its GIF) has to go before the core does.
    void clear_content() { content_.reset(); }
    Widget* find(std::string_view n) { return content_ ? content_->find(n) : nullptr; }
    Widget* find_automation(std::string_view id) { return content_ ? content_->find_automation(id) : nullptr; }

    // ---- layout ----
    void layout(Size size);
    Size size() const { return size_; }
    void request_layout() { layout_pending_ = true; notify_invalidate(); }
    bool layout_pending() const { return layout_pending_; }

    // ---- painting ----
    void invalidate(const Rect& r);
    void invalidate_all();
    Rect dirty() const { return dirty_; }
    // Lay out if asked, paint the dirty rect, present it. S_FALSE when nothing was dirty.
    HRESULT paint(gfx::Surface& surface);
    std::function<void()> on_invalidate;  // the window turns this into a WM_PAINT
    uint64_t paint_count() const { return paint_count_; }

    // ---- input (window DIPs) ----
    bool mouse_down(Point p, MouseButton b = MouseButton::Left, Mods m = {}, int clicks = 1);
    bool mouse_up(Point p, MouseButton b = MouseButton::Left, Mods m = {});
    bool mouse_move(Point p, Mods m = {});
    bool mouse_leave();
    bool wheel(Point p, int delta, Mods m = {});
    // Keys: modifiers are tracked from the messages seen, so a posted key sequence behaves like a pressed one.
    bool key_down(unsigned vk);
    bool key_up(unsigned vk);
    // Sees a key before the focused widget does (the app's one shortcut dispatcher); true = handled.
    std::function<bool(const KeyEvent&)> on_preview_key;
    bool char_input(wchar_t c);
    Mods mods() const { return mods_; }
    void set_mods(Mods m) { mods_ = m; }
    Cursor cursor() const;

    Widget* focused() const { return focus_; }
    void set_focus(Widget* w);
    bool focus_next(bool reverse);
    // Where Tab cycles: the subtree this answers (an open dialog card, a page over the content), or the
    // whole content when it answers null.
    std::function<Widget*()> tab_scope;
    bool contains(const Widget* w) const;
    Widget* hovered() const { return hover_; }
    Widget* captured() const { return capture_; }
    void capture(Widget* w) { capture_ = w; }
    void release_capture() { capture_ = nullptr; }
    // A modal (an open menu) takes every mouse, wheel and key event until it is cleared, whatever is
    // under the pointer: hover stops following the pointer, the preview-key dispatcher stands down, and
    // a release does not end it the way it ends a capture. cancel_modal() is what the window calls on
    // deactivation; it tells the widget through on_modal_cancel().
    Widget* modal() const { return modal_; }
    void set_modal(Widget* w);
    void cancel_modal();
    std::function<void(bool)> on_modal_changed;  // the window takes / gives back the OS mouse capture
    // The widget popups are shown in (a PopupLayer); widgets that open popups look it up here.
    Widget* popup_layer() const { return popup_layer_; }
    void set_popup_layer(Widget* w) { popup_layer_ = w; }
    Point pointer() const { return last_pt_; }  // the last pointer position seen
    // Fired when the widget under the pointer changes, and with null on a press (a tooltip listens).
    std::function<void(Widget*)> on_hover_changed;
    // Alt went down (true) or up (false), by itself; the menu bar's Alt-tap mode listens.
    std::function<void(bool)> on_alt_key;
    Widget* hit_test(Point p) const { return content_ ? content_->hit_test(p) : nullptr; }

    // IME, forwarded to the focused widget.
    bool ime_caret_rect(Rect* r) { return focus_ && focus_->ime_caret_rect(r); }
    void ime_composition(const std::wstring& s, size_t cursor) { if (focus_) focus_->on_ime_composition(s, cursor); }
    void ime_commit(const std::wstring& s) { if (focus_) focus_->on_ime_commit(s); }
    void ime_end() { if (focus_) focus_->on_ime_end(); }

    // ---- time ----
    uint64_t now_ms() const { return clock_ ? clock_() : default_clock(); }
    void set_clock(std::function<uint64_t()> c) { clock_ = std::move(c); }
    void start_timer(Widget* w, int id, unsigned interval_ms);
    void stop_timer(Widget* w, int id);
    bool has_timers() const { return !timers_.empty(); }
    void tick();  // fire the timers that are due; the window calls it from its WM_TIMER
    std::function<void()> on_timers_changed;

    Clipboard clipboard;
    bool caret_blink = true;  // the harness turns this off so a photograph never catches a blink

private:
    friend class Widget;
    struct Timer {
        Widget* w;
        int id;
        unsigned interval;
        uint64_t due;
    };
    static uint64_t default_clock();
    void install(std::unique_ptr<Widget> w);
    void forget(Widget* w);
    void notify_invalidate();
    template <class F>
    bool bubble(Widget* from, F&& f);
    static void collect_tab_stops(Widget* w, std::vector<Widget*>& out);

    gfx::Resources& res_;
    Size size_;
    Rect dirty_;
    bool layout_pending_ = false;
    uint64_t paint_count_ = 0;
    Mods mods_;
    Point last_pt_;
    Widget* focus_ = nullptr;
    Widget* hover_ = nullptr;
    Widget* capture_ = nullptr;
    Widget* modal_ = nullptr;
    Widget* popup_layer_ = nullptr;
    std::vector<Timer> timers_;
    std::function<uint64_t()> clock_;
    std::unique_ptr<Widget> content_;  // last: destroyed first, while the rest is still alive
};

}  // namespace bb::ui
