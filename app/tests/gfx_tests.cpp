#include "fixture.h"

#include <filesystem>

using namespace bb;
using Microsoft::WRL::ComPtr;

namespace {
void fill(gfx::Surface& s, D2D1_COLOR_F c, D2D1_RECT_F r) {
    s.begin_draw();
    ComPtr<ID2D1SolidColorBrush> b;
    s.context()->CreateSolidColorBrush(c, &b);
    s.context()->Clear(D2D1::ColorF(0, 0, 0, 1));
    s.context()->FillRectangle(r, b.Get());
    CHECK(SUCCEEDED(s.end_draw(nullptr)));
}
}  // namespace

TEST(device_is_warp_and_ignores_video_flag) {
    auto& dev = *Env::get().dev;
    CHECK(dev.is_warp());
    CHECK(dev.d3d() && dev.d2d() && dev.dwrite() && dev.dxgi());
    // WARP rejects D3D11_CREATE_DEVICE_VIDEO_SUPPORT; asking for it must not break creation.
    HRESULT hr = E_FAIL;
    auto d = gfx::GfxDevice::create(gfx::DeviceOptions{.warp = true, .video_support = true}, &hr);
    CHECK(d != nullptr);
    CHECK(d && d->is_warp());
    CHECK(d && d->create_context() != nullptr);
}

TEST(bitmap_surface_flat_check_and_pixels) {
    auto surf = gfx::BitmapSurface::create(*Env::get().dev, 64, 48);
    CHECK(surf != nullptr);
    surf->begin_draw();
    surf->context()->Clear(D2D1::ColorF(0, 0, 1, 1));
    CHECK(SUCCEEDED(surf->end_draw(nullptr)));
    CHECK(surf->is_flat());  // the failure the harness guards against: a capture of one colour
    fill(*surf, D2D1::ColorF(1, 0, 0, 1), D2D1::RectF(8, 8, 24, 24));
    CHECK(!surf->is_flat());
    CHECK_EQ(surf->count_non_background(), 16 * 16);
    CHECK_EQ(surf->pixel(10, 10), 0xFFFF0000u);
    CHECK_EQ(surf->pixel(40, 40), 0xFF000000u);
}

TEST(bitmap_surface_scales_dips_by_dpi) {
    auto surf = gfx::BitmapSurface::create(*Env::get().dev, 192, 192, 192.0f);
    CHECK(surf != nullptr);
    CHECK_NEAR(surf->width_dip(), 96.0, 0.001);
    fill(*surf, D2D1::ColorF(0, 1, 0, 1), D2D1::RectF(0, 0, 48, 48));  // 48 DIPs = 96 px at 192 dpi
    CHECK_EQ(surf->pixel(90, 90), 0xFF00FF00u);
    CHECK_EQ(surf->pixel(100, 100), 0xFF000000u);
}

TEST(bitmap_surface_writes_png) {
    auto surf = gfx::BitmapSurface::create(*Env::get().dev, 32, 32);
    fill(*surf, D2D1::ColorF(1, 1, 0, 1), D2D1::RectF(4, 4, 20, 20));
    const std::wstring path = exe_dir() + L"gfx_png_test.png";
    CHECK(surf->write_png(path));
    std::error_code ec;
    CHECK(std::filesystem::file_size(path, ec) > 50);
    std::filesystem::remove(path, ec);
}

TEST(text_cache_formats_are_shared) {
    auto& text = Env::get().res->text;
    gfx::FontSpec a;
    gfx::FontSpec b = a;
    b.size = 20;
    gfx::FontSpec c = a;
    c.family = gfx::Family::Mono;
    IDWriteTextFormat* fa = text.format(a);
    CHECK(fa != nullptr);
    CHECK(text.format(a) == fa);
    CHECK(text.format(b) != fa);
    CHECK(text.format(c) != fa);
    const size_t made = text.formats_made();
    text.format(a);
    text.format(b);
    CHECK_EQ(text.formats_made(), made);  // cached: no new format
    CHECK(text.measure(L"Hello", a).width > 10.0f);
}

TEST(layout_lru_hits_misses_and_evicts) {
    gfx::TextCache text(Env::get().dev->dwrite());
    gfx::LayoutLru lru(text, 3);
    gfx::FontSpec font;
    gfx::LayoutOptions opt;
    opt.max_width = 100;
    opt.ellipsis = true;
    IDWriteTextLayout* l1 = lru.get({1, 0, 100}, L"alpha", font, opt);
    CHECK(l1 != nullptr);
    CHECK(lru.get({1, 0, 100}, L"alpha", font, opt) == l1);
    CHECK_EQ(lru.hits(), 1);
    CHECK_EQ(lru.misses(), 1);
    lru.get({1, 0, 100}, L"alpha changed", font, opt);  // same key, new text: rebuilt
    CHECK_EQ(lru.misses(), 2);
    for (uint64_t i = 2; i < 8; ++i) lru.get({i, 0, 100}, L"x", font, opt);
    CHECK_EQ(lru.size(), 3);  // capped
    const size_t misses = lru.misses();
    lru.get({1, 0, 100}, L"alpha changed", font, opt);  // evicted long ago
    CHECK_EQ(lru.misses(), misses + 1);
}

TEST(palette_takes_a_count_and_rebuilds_on_loss) {
    auto dev = gfx::GfxDevice::create(gfx::DeviceOptions{.warp = true});
    CHECK(dev != nullptr);
    gfx::Palette pal(*dev);
    std::vector<uint32_t> argb(126);
    for (size_t i = 0; i < argb.size(); ++i) argb[i] = 0xFF000000u | static_cast<uint32_t>(i * 2);
    pal.set_colors(argb);  // size and order come from the Rust theme later
    CHECK_EQ(pal.size(), 126);
    CHECK_EQ(pal.argb(125), 0xFF0000FAu);
    CHECK(pal.brush(0) != nullptr);
    CHECK(pal.brush(125) != nullptr);
    CHECK(pal.brush(126) == nullptr);
    CHECK(pal.brush(gfx::kNoToken) == nullptr);
    CHECK_NEAR(pal.color(125).b, 250.0 / 255.0, 0.001);
    ID2D1SolidColorBrush* before = pal.brush(5);
    CHECK(pal.brush(5) == before);  // cached
    const uint32_t made = pal.brushes_made();
    CHECK(SUCCEEDED(dev->recreate()));  // device loss: the next lookup rebuilds
    CHECK(pal.brush(5) != nullptr);
    CHECK(pal.brushes_made() > made);
    // A theme change replaces the colours and the brush follows.
    argb[5] = 0xFF123456u;
    pal.set_colors(argb);
    CHECK_NEAR(pal.brush(5)->GetColor().r, 0x12 / 255.0, 0.001);
}

TEST(swap_chain_presents_dirty_rects) {
    Rig rig(320, 200);
    auto sc = gfx::SwapChainSurface::create(*Env::get().dev, rig.win.hwnd(), 320, 200);
    CHECK(sc != nullptr);
    if (!sc) return;
    for (int i = 0; i < 3; ++i) {
        CHECK(SUCCEEDED(sc->begin_draw()));
        sc->context()->Clear(D2D1::ColorF(0.1f, 0.1f, 0.1f));
        const RECT dirty{10, 10, 100, 60};  // flip model: only this is presented after the first frame
        CHECK(SUCCEEDED(sc->end_draw(i == 0 ? nullptr : &dirty)));
    }
    CHECK_EQ(sc->presents(), 3);
    CHECK(SUCCEEDED(sc->resize(400, 300)));
    CHECK(SUCCEEDED(sc->begin_draw()));
    sc->context()->Clear(D2D1::ColorF(0, 0, 0));
    CHECK(SUCCEEDED(sc->end_draw(nullptr)));
}

// ---- moving from WARP to a hardware device ----------------------------------------------------------

namespace {

// A hardware device from the loader, or null on a machine with no adapter (a CI runner): the tests that need
// one then have nothing to check and say nothing.
Microsoft::WRL::ComPtr<ID3D11Device> hardware_or_null() {
    gfx::HardwareDeviceLoader loader(false);
    for (int i = 0; i < 1000 && !loader.finished(); ++i) Sleep(10);
    return loader.take();
}

}  // namespace

TEST(a_warp_device_adopts_a_hardware_one_and_draws_on_it) {
    HRESULT hr = E_FAIL;
    auto dev = gfx::GfxDevice::create(gfx::DeviceOptions{.warp = true}, &hr);
    CHECK(dev != nullptr);
    auto hw = hardware_or_null();
    if (!dev || !hw) return;
    const uint32_t before = dev->generation();
    CHECK(SUCCEEDED(dev->adopt_hardware(hw, false)));
    CHECK(!dev->is_warp());
    CHECK_EQ(dev->generation(), before + 1);
    auto surf = gfx::BitmapSurface::create(*dev, 64, 48);
    CHECK(surf != nullptr);
    fill(*surf, D2D1::ColorF(1, 0, 0, 1), D2D1::RectF(8, 8, 24, 24));
    CHECK_EQ(surf->pixel(10, 10), 0xFFFF0000u);
    CHECK_EQ(surf->generation(), dev->generation());
}

TEST(a_window_gets_a_new_swap_chain_on_the_hardware_device) {
    HRESULT hr = E_FAIL;
    auto dev = gfx::GfxDevice::create(gfx::DeviceOptions{.warp = true}, &hr);
    CHECK(dev != nullptr);
    auto hw = hardware_or_null();
    if (!dev || !hw) return;
    // A window nothing ever shows: the swap chain only needs the handle.
    HWND hwnd = CreateWindowExW(0, L"STATIC", L"", WS_POPUP, 0, 0, 120, 80, nullptr, nullptr, GetModuleHandleW(nullptr), nullptr);
    CHECK(hwnd != nullptr);
    auto first = gfx::SwapChainSurface::create(*dev, hwnd, 120, 80, 96.0f);
    CHECK(first != nullptr);
    CHECK(first && SUCCEEDED(first->begin_draw()));
    if (first) first->context()->Clear(D2D1::ColorF(0, 0, 1, 1));
    CHECK(first && SUCCEEDED(first->end_draw(nullptr)));
    CHECK(SUCCEEDED(dev->adopt_hardware(hw, false)));
    CHECK(first && first->generation() != dev->generation());  // what tells a window to rebuild
    first.reset();  // the window lets go of the old chain, as it does before making the new one
    auto second = gfx::SwapChainSurface::create(*dev, hwnd, 120, 80, 96.0f);
    CHECK(second != nullptr);
    CHECK(second && SUCCEEDED(second->begin_draw()));
    if (second) second->context()->Clear(D2D1::ColorF(0, 1, 0, 1));
    CHECK(second && SUCCEEDED(second->end_draw(nullptr)));
    CHECK(second && second->generation() == dev->generation());
    second.reset();
    DestroyWindow(hwnd);
}

TEST(a_loader_posts_once_when_it_is_done_whether_or_not_it_found_a_device) {
    gfx::HardwareDeviceLoader loader(false);
    HWND hwnd = CreateWindowExW(0, L"STATIC", L"", WS_POPUP, 0, 0, 10, 10, nullptr, nullptr, GetModuleHandleW(nullptr), nullptr);
    CHECK(hwnd != nullptr);
    constexpr UINT kMsg = WM_APP + 77;
    loader.notify(hwnd, kMsg);
    for (int i = 0; i < 1000 && !loader.finished(); ++i) Sleep(10);
    CHECK(loader.finished());
    MSG msg{};
    int posted = 0;
    for (int i = 0; i < 20; ++i) {
        while (PeekMessageW(&msg, hwnd, kMsg, kMsg, PM_REMOVE)) ++posted;
        Sleep(5);
    }
    CHECK_EQ(posted, 1);  // a machine with no adapter still has to say the drawing has settled
    auto device = loader.take();
    (void)device;
    CHECK(loader.take() == nullptr);  // taken once
    DestroyWindow(hwnd);
}
