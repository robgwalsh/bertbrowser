// Where a widget tree is drawn. A Surface owns an ID2D1DeviceContext and a target bitmap in device
// pixels; the context's DPI is set so everything drawn on it is in DIPs.
//   SwapChainSurface: a flip-model swap chain on an HWND, presented with dirty rects.
//   BitmapSurface:    an ID2D1Bitmap1 target plus a CPU-readable copy, for the harness and the tests.
#pragma once
#include "gfx/device.h"

#include <string>
#include <vector>

namespace bb::gfx {

class Surface {
public:
    virtual ~Surface() = default;

    ID2D1DeviceContext* context() const { return ctx_.Get(); }
    UINT width_px() const { return w_; }
    UINT height_px() const { return h_; }
    float dpi() const { return dpi_; }
    float scale() const { return dpi_ / 96.0f; }
    float width_dip() const { return static_cast<float>(w_) / scale(); }
    float height_dip() const { return static_cast<float>(h_) / scale(); }

    void set_dpi(float dpi);
    // BeginDraw on the current target. Returns a failure if the target could not be set up.
    HRESULT begin_draw();
    // EndDraw, then present what changed. `dirty_px` null means everything.
    HRESULT end_draw(const RECT* dirty_px);
    virtual HRESULT resize(UINT w, UINT h) = 0;

protected:
    Surface(GfxDevice& dev, UINT w, UINT h, float dpi) : dev_(dev), w_(w), h_(h), dpi_(dpi) {}
    virtual ID2D1Bitmap1* target() = 0;
    virtual HRESULT present(const RECT* dirty_px) = 0;

    GfxDevice& dev_;
    ComPtr<ID2D1DeviceContext> ctx_;
    UINT w_, h_;
    float dpi_;
};

class BitmapSurface : public Surface {
public:
    static std::unique_ptr<BitmapSurface> create(GfxDevice& dev, UINT w, UINT h, float dpi = 96.0f,
                                                 HRESULT* hr = nullptr);

    HRESULT resize(UINT w, UINT h) override;

    // Copies the target to the CPU: BGRA premultiplied, w*h*4 bytes, rows tightly packed.
    bool read_pixels(std::vector<uint8_t>& out);
    // The flat-colour check: how many pixels differ from the first one. A capture of zero is a failure.
    size_t count_non_background();
    bool is_flat() { return count_non_background() == 0; }
    // 0xAARRGGBB of one pixel (reads the whole surface back; for tests).
    uint32_t pixel(UINT x, UINT y);
    bool write_png(const std::wstring& path);

private:
    using Surface::Surface;
    HRESULT build();
    ID2D1Bitmap1* target() override { return target_.Get(); }
    HRESULT present(const RECT*) override { return S_OK; }

    ComPtr<ID2D1Bitmap1> target_;
    ComPtr<ID2D1Bitmap1> staging_;
};

class SwapChainSurface : public Surface {
public:
    static std::unique_ptr<SwapChainSurface> create(GfxDevice& dev, HWND hwnd, UINT w, UINT h, float dpi = 96.0f,
                                                    HRESULT* hr = nullptr);

    HRESULT resize(UINT w, UINT h) override;
    UINT presents() const { return presents_; }

private:
    using Surface::Surface;
    HRESULT build_target();
    ID2D1Bitmap1* target() override { return target_.Get(); }
    HRESULT present(const RECT* dirty_px) override;

    ComPtr<IDXGISwapChain1> chain_;
    ComPtr<ID2D1Bitmap1> target_;
    bool first_present_ = true;
    UINT presents_ = 0;
};

}  // namespace bb::gfx
