// Shared test fixtures: one WARP device for the process, and a Rig = a parked window with a widget tree.
#pragma once
#include "gfx/resources.h"
#include "platform/application.h"
#include "platform/window.h"
#include "test.h"
#include "ui/controls.h"
#include "ui/edit.h"
#include "ui/layout.h"
#include "ui/list_view.h"
#include "ui/scroll.h"
#include "ui/tree_view.h"

#include <cstdlib>
#include <memory>
#include <string>

struct Env {
    std::unique_ptr<bb::gfx::GfxDevice> dev;
    std::unique_ptr<bb::gfx::Resources> res;

    static Env& get() {
        static Env e;
        return e;
    }

private:
    Env() {
        bb::platform::init_process();
        HRESULT hr = S_OK;
        dev = bb::gfx::GfxDevice::create(bb::gfx::DeviceOptions{.warp = true}, &hr);
        if (!dev) {
            std::printf("FATAL: no WARP device (0x%08lx)\n", static_cast<unsigned long>(hr));
            std::exit(2);
        }
        res = std::make_unique<bb::gfx::Resources>(*dev);
    }
};

inline std::wstring exe_dir() {
    wchar_t buf[MAX_PATH];
    const DWORD n = GetModuleFileNameW(nullptr, buf, MAX_PATH);
    std::wstring p(buf, n);
    return p.substr(0, p.find_last_of(L'\\') + 1);
}

struct Rig {
    bb::platform::Window win;
    bb::ui::UiRoot& root;
    std::wstring clip;
    uint64_t now = 100000;

    explicit Rig(int width = 800, int height = 600) : win(*Env::get().res, options(width, height)), root(win.root()) {
        CHECK(win.ok());
        win.show();
        root.caret_blink = false;
        root.clipboard = {[this] { return clip; }, [this](const std::wstring& s) { clip = s; }};
        root.set_clock([this] { return now; });
    }
    static bb::platform::WindowOptions options(int w, int h) {
        bb::platform::WindowOptions o;
        o.parked = true;
        o.width_dip = w;
        o.height_dip = h;
        o.dpi_override = 96;
        return o;
    }

    template <class T>
    T* set(std::unique_ptr<T> content) {
        T* p = root.set_content(std::move(content));
        relayout();
        return p;
    }
    void relayout() { root.layout({win.surface().width_dip(), win.surface().height_dip()}); }
    void render() { CHECK(SUCCEEDED(win.render())); }
    bb::gfx::BitmapSurface& bitmap() { return *win.bitmap(); }

    static bb::ui::Point center(const bb::ui::Widget* w) {
        return {(w->bounds.l + w->bounds.r) / 2, (w->bounds.t + w->bounds.b) / 2};
    }
    void down(bb::ui::Point p, bb::ui::Mods m = {}, int clicks = 1) {
        root.mouse_move(p, m);
        root.mouse_down(p, bb::ui::MouseButton::Left, m, clicks);
    }
    void move(bb::ui::Point p, bb::ui::Mods m = {}) { root.mouse_move(p, m); }
    void up(bb::ui::Point p, bb::ui::Mods m = {}) { root.mouse_up(p, bb::ui::MouseButton::Left, m); }
    void click(bb::ui::Point p, bb::ui::Mods m = {}) {
        down(p, m);
        up(p, m);
    }
    void key(unsigned vk, bool shift = false, bool ctrl = false) {
        if (shift) root.key_down(VK_SHIFT);
        if (ctrl) root.key_down(VK_CONTROL);
        root.key_down(vk);
        root.key_up(vk);
        if (ctrl) root.key_up(VK_CONTROL);
        if (shift) root.key_up(VK_SHIFT);
    }
    void type(const std::wstring& s) {
        for (wchar_t c : s) root.char_input(c);
    }
};
