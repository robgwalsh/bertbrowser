#include "gfx/device.h"

namespace bb::gfx {

std::unique_ptr<GfxDevice> GfxDevice::create(const DeviceOptions& options, HRESULT* hr_out) {
    std::unique_ptr<GfxDevice> d(new GfxDevice());
    d->options_ = options;
    const HRESULT hr = d->build();
    if (hr_out) {
        *hr_out = hr;
    }
    return SUCCEEDED(hr) ? std::move(d) : nullptr;
}

HRESULT GfxDevice::build() {
    d3d_.Reset();
    dxgi_.Reset();
    device_.Reset();
    UINT flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT;
    HRESULT hr = E_FAIL;
    warp_ = options_.warp;
    if (!warp_) {
        UINT hw_flags = flags;
        if (options_.video_support) {
            hw_flags |= D3D11_CREATE_DEVICE_VIDEO_SUPPORT;
        }
        hr = D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, hw_flags, nullptr, 0, D3D11_SDK_VERSION,
                               &d3d_, nullptr, nullptr);
        if (FAILED(hr)) {
            warp_ = true;
        }
    }
    if (warp_) {
        // Never VIDEO_SUPPORT here: WARP refuses the flag outright.
        hr = D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_WARP, nullptr, flags, nullptr, 0, D3D11_SDK_VERSION, &d3d_,
                               nullptr, nullptr);
    }
    if (FAILED(hr)) {
        return hr;
    }
    ComPtr<ID3D11Multithread> mt;
    if (SUCCEEDED(d3d_.As(&mt))) {
        mt->SetMultithreadProtected(TRUE);  // media decoders share the device from their own threads
    }
    hr = d3d_.As(&dxgi_);
    if (FAILED(hr)) {
        return hr;
    }
    if (!factory_) {
        hr = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, IID_PPV_ARGS(&factory_));
        if (FAILED(hr)) {
            return hr;
        }
    }
    hr = factory_->CreateDevice(dxgi_.Get(), &device_);
    if (FAILED(hr)) {
        return hr;
    }
    if (!dwrite_) {
        hr = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED, __uuidof(IDWriteFactory),
                                 reinterpret_cast<IUnknown**>(dwrite_.GetAddressOf()));
    }
    return hr;
}

IWICImagingFactory* GfxDevice::wic() {
    if (!wic_) {
        CoCreateInstance(CLSID_WICImagingFactory, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&wic_));
    }
    return wic_.Get();
}

ComPtr<ID2D1DeviceContext> GfxDevice::create_context() {
    ComPtr<ID2D1DeviceContext> ctx;
    device_->CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE, &ctx);
    return ctx;
}

HRESULT GfxDevice::recreate() {
    const HRESULT hr = build();
    ++generation_;
    return hr;
}

bool GfxDevice::is_loss(HRESULT hr) {
    return hr == D2DERR_RECREATE_TARGET || hr == DXGI_ERROR_DEVICE_REMOVED || hr == DXGI_ERROR_DEVICE_RESET ||
           hr == DXGI_ERROR_DRIVER_INTERNAL_ERROR;
}

}  // namespace bb::gfx
