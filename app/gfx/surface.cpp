#include "gfx/surface.h"

#include <algorithm>
#include <cstring>

namespace bb::gfx {

namespace {
D2D1_PIXEL_FORMAT bgra() { return D2D1::PixelFormat(DXGI_FORMAT_B8G8R8A8_UNORM, D2D1_ALPHA_MODE_PREMULTIPLIED); }
}  // namespace

void Surface::set_dpi(float dpi) {
    dpi_ = dpi;
    if (ctx_) {
        ctx_->SetDpi(dpi_, dpi_);
    }
}

HRESULT Surface::begin_draw() {
    ID2D1Bitmap1* t = target();
    if (!t || !ctx_) {
        return E_FAIL;
    }
    ctx_->SetTarget(t);
    ctx_->SetDpi(dpi_, dpi_);
    ctx_->BeginDraw();
    return S_OK;
}

HRESULT Surface::end_draw(const RECT* dirty_px) {
    const HRESULT hr = ctx_->EndDraw();
    if (FAILED(hr)) {
        return hr;
    }
    return present(dirty_px);
}

// ---- BitmapSurface ---------------------------------------------------------------------------------

std::unique_ptr<BitmapSurface> BitmapSurface::create(GfxDevice& dev, UINT w, UINT h, float dpi, HRESULT* hr_out) {
    std::unique_ptr<BitmapSurface> s(new BitmapSurface(dev, w, h, dpi));
    s->ctx_ = dev.create_context();
    HRESULT hr = s->ctx_ ? s->build() : E_FAIL;
    if (hr_out) {
        *hr_out = hr;
    }
    return SUCCEEDED(hr) ? std::move(s) : nullptr;
}

HRESULT BitmapSurface::build() {
    target_.Reset();
    staging_.Reset();
    ctx_->SetTarget(nullptr);
    HRESULT hr = ctx_->CreateBitmap(D2D1::SizeU(w_, h_), nullptr, 0,
                                    D2D1::BitmapProperties1(D2D1_BITMAP_OPTIONS_TARGET, bgra(), 96.0f, 96.0f), &target_);
    if (FAILED(hr)) {
        return hr;
    }
    hr = ctx_->CreateBitmap(D2D1::SizeU(w_, h_), nullptr, 0,
                            D2D1::BitmapProperties1(D2D1_BITMAP_OPTIONS_CPU_READ | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                                                    bgra(), 96.0f, 96.0f),
                            &staging_);
    return hr;
}

HRESULT BitmapSurface::resize(UINT w, UINT h) {
    w_ = std::max(w, 1u);
    h_ = std::max(h, 1u);
    return build();
}

bool BitmapSurface::read_pixels(std::vector<uint8_t>& out) {
    if (FAILED(staging_->CopyFromBitmap(nullptr, target_.Get(), nullptr))) {
        return false;
    }
    D2D1_MAPPED_RECT mapped{};
    if (FAILED(staging_->Map(D2D1_MAP_OPTIONS_READ, &mapped))) {
        return false;
    }
    out.resize(static_cast<size_t>(w_) * h_ * 4);
    for (UINT y = 0; y < h_; ++y) {
        std::memcpy(out.data() + static_cast<size_t>(y) * w_ * 4, mapped.bits + static_cast<size_t>(y) * mapped.pitch,
                    static_cast<size_t>(w_) * 4);
    }
    staging_->Unmap();
    return true;
}

size_t BitmapSurface::count_non_background() {
    std::vector<uint8_t> px;
    if (!read_pixels(px) || px.empty()) {
        return 0;
    }
    uint32_t first = 0;
    std::memcpy(&first, px.data(), 4);
    size_t n = 0;
    for (size_t i = 0; i < px.size(); i += 4) {
        uint32_t v = 0;
        std::memcpy(&v, px.data() + i, 4);
        if (v != first) {
            ++n;
        }
    }
    return n;
}

uint32_t BitmapSurface::pixel(UINT x, UINT y) {
    std::vector<uint8_t> px;
    if (!read_pixels(px) || x >= w_ || y >= h_) {
        return 0;
    }
    uint32_t v = 0;
    std::memcpy(&v, px.data() + (static_cast<size_t>(y) * w_ + x) * 4, 4);
    return v;
}

bool BitmapSurface::write_png(const std::wstring& path, const RECT* region) {
    std::vector<uint8_t> px;
    if (!read_pixels(px)) {
        return false;
    }
    UINT out_w = w_, out_h = h_;
    if (region) {
        // Crop (clamped to the surface) into a tightly packed copy.
        const LONG l = std::clamp<LONG>(region->left, 0, static_cast<LONG>(w_));
        const LONG t = std::clamp<LONG>(region->top, 0, static_cast<LONG>(h_));
        const LONG r = std::clamp<LONG>(region->right, l, static_cast<LONG>(w_));
        const LONG b = std::clamp<LONG>(region->bottom, t, static_cast<LONG>(h_));
        out_w = static_cast<UINT>(r - l);
        out_h = static_cast<UINT>(b - t);
        if (out_w == 0 || out_h == 0) {
            return false;
        }
        std::vector<uint8_t> cropped(static_cast<size_t>(out_w) * out_h * 4);
        for (UINT y = 0; y < out_h; ++y) {
            std::memcpy(&cropped[static_cast<size_t>(y) * out_w * 4], &px[(static_cast<size_t>(t) + y) * w_ * 4 + static_cast<size_t>(l) * 4],
                        static_cast<size_t>(out_w) * 4);
        }
        px = std::move(cropped);
    }
    IWICImagingFactory* wic = dev_.wic();
    if (!wic) {
        return false;
    }
    ComPtr<IWICStream> stream;
    wic->CreateStream(&stream);
    if (!stream || FAILED(stream->InitializeFromFilename(path.c_str(), GENERIC_WRITE))) {
        return false;
    }
    ComPtr<IWICBitmapEncoder> encoder;
    wic->CreateEncoder(GUID_ContainerFormatPng, nullptr, &encoder);
    if (!encoder || FAILED(encoder->Initialize(stream.Get(), WICBitmapEncoderNoCache))) {
        return false;
    }
    ComPtr<IWICBitmapFrameEncode> frame;
    encoder->CreateNewFrame(&frame, nullptr);
    if (!frame || FAILED(frame->Initialize(nullptr))) {
        return false;
    }
    frame->SetSize(out_w, out_h);
    WICPixelFormatGUID pf = GUID_WICPixelFormat32bppPBGRA;
    frame->SetPixelFormat(&pf);
    if (FAILED(frame->WritePixels(out_h, out_w * 4, static_cast<UINT>(px.size()), px.data()))) {
        return false;
    }
    frame->Commit();
    return SUCCEEDED(encoder->Commit());
}

// ---- SwapChainSurface ------------------------------------------------------------------------------

std::unique_ptr<SwapChainSurface> SwapChainSurface::create(GfxDevice& dev, HWND hwnd, UINT w, UINT h, float dpi,
                                                           HRESULT* hr_out) {
    std::unique_ptr<SwapChainSurface> s(new SwapChainSurface(dev, std::max(w, 1u), std::max(h, 1u), dpi));
    s->ctx_ = dev.create_context();
    s->d3d_ = dev.d3d();
    HRESULT hr = s->ctx_ ? S_OK : E_FAIL;
    if (SUCCEEDED(hr)) {
        ComPtr<IDXGIAdapter> adapter;
        hr = dev.dxgi()->GetAdapter(&adapter);
        ComPtr<IDXGIFactory2> factory;
        if (SUCCEEDED(hr)) {
            hr = adapter->GetParent(IID_PPV_ARGS(&factory));
        }
        if (SUCCEEDED(hr)) {
            DXGI_SWAP_CHAIN_DESC1 desc{};
            desc.Width = s->w_;
            desc.Height = s->h_;
            desc.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
            desc.SampleDesc.Count = 1;
            desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
            desc.BufferCount = 2;
            desc.Scaling = DXGI_SCALING_NONE;
            desc.SwapEffect = DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL;
            desc.AlphaMode = DXGI_ALPHA_MODE_IGNORE;
            hr = factory->CreateSwapChainForHwnd(dev.d3d(), hwnd, &desc, nullptr, nullptr, &s->chain_);
            if (SUCCEEDED(hr)) {
                factory->MakeWindowAssociation(hwnd, DXGI_MWA_NO_ALT_ENTER | DXGI_MWA_NO_WINDOW_CHANGES);
            }
        }
        if (SUCCEEDED(hr)) {
            hr = s->build_target();
        }
    }
    if (hr_out) {
        *hr_out = hr;
    }
    return SUCCEEDED(hr) ? std::move(s) : nullptr;
}

SwapChainSurface::~SwapChainSurface() {
    if (ctx_) {
        ctx_->SetTarget(nullptr);
    }
    target_.Reset();
    chain_.Reset();
    ctx_.Reset();
    if (d3d_) {
        ComPtr<ID3D11DeviceContext> immediate;
        d3d_->GetImmediateContext(&immediate);
        if (immediate) {
            immediate->ClearState();
            immediate->Flush();
        }
    }
}

HRESULT SwapChainSurface::build_target() {
    target_.Reset();
    ctx_->SetTarget(nullptr);
    ComPtr<IDXGISurface> back;
    HRESULT hr = chain_->GetBuffer(0, IID_PPV_ARGS(&back));
    if (FAILED(hr)) {
        return hr;
    }
    return ctx_->CreateBitmapFromDxgiSurface(
        back.Get(),
        D2D1::BitmapProperties1(D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW, bgra(), 96.0f, 96.0f),
        &target_);
}

HRESULT SwapChainSurface::resize(UINT w, UINT h) {
    w_ = std::max(w, 1u);
    h_ = std::max(h, 1u);
    target_.Reset();
    ctx_->SetTarget(nullptr);
    HRESULT hr = chain_->ResizeBuffers(0, w_, h_, DXGI_FORMAT_UNKNOWN, 0);
    if (FAILED(hr)) {
        return hr;
    }
    first_present_ = true;  // new buffers: nothing to retain, so the next present is whole
    return build_target();
}

HRESULT SwapChainSurface::present(const RECT* dirty_px) {
    DXGI_PRESENT_PARAMETERS params{};
    RECT rc{};
    if (dirty_px && !first_present_) {
        rc.left = std::clamp<LONG>(dirty_px->left, 0, static_cast<LONG>(w_));
        rc.top = std::clamp<LONG>(dirty_px->top, 0, static_cast<LONG>(h_));
        rc.right = std::clamp<LONG>(dirty_px->right, rc.left, static_cast<LONG>(w_));
        rc.bottom = std::clamp<LONG>(dirty_px->bottom, rc.top, static_cast<LONG>(h_));
        params.DirtyRectsCount = 1;
        params.pDirtyRects = &rc;
    }
    const HRESULT hr = chain_->Present1(0, 0, &params);
    first_present_ = false;
    ++presents_;
    return hr;
}

}  // namespace bb::gfx
