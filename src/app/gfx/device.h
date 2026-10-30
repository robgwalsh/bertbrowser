// One Direct3D 11 device for the whole process, with the Direct2D device, DirectWrite and WIC factories
// that hang off it. Hardware by default; WARP when asked (always in the harness), and as the fallback
// when no adapter will do. Surfaces create their own ID2D1DeviceContext from it.
#pragma once
#include <windows.h>

#include <d2d1_1.h>
#include <d3d11.h>
#include <d3d11_4.h>
#include <dwrite.h>
#include <dxgi1_2.h>
#include <wincodec.h>
#include <wrl/client.h>

#include <cstdint>
#include <memory>
#include <mutex>
#include <thread>

namespace bb::gfx {

using Microsoft::WRL::ComPtr;

struct DeviceOptions {
    bool warp = false;           // software rasteriser: no GPU needed, same code path
    bool video_support = false;  // D3D11_CREATE_DEVICE_VIDEO_SUPPORT; ignored on WARP, which rejects it
};

class GfxDevice {
public:
    // Returns null and fills `hr` on failure.
    static std::unique_ptr<GfxDevice> create(const DeviceOptions& options, HRESULT* hr = nullptr);

    ID3D11Device* d3d() const { return d3d_.Get(); }
    IDXGIDevice* dxgi() const { return dxgi_.Get(); }
    ID2D1Factory1* d2d_factory() const { return factory_.Get(); }
    ID2D1Device* d2d() const { return device_.Get(); }
    IDWriteFactory* dwrite() const { return dwrite_.Get(); }
    IWICImagingFactory* wic();  // created on first use; needs COM initialised on the calling thread

    bool is_warp() const { return warp_; }
    const DeviceOptions& options() const { return options_; }

    ComPtr<ID2D1DeviceContext> create_context();

    // Device loss: anything holding device-dependent resources compares `generation()` with the one it
    // built under and rebuilds when it moved. `recreate()` makes a fresh device and bumps it.
    uint32_t generation() const { return generation_; }
    HRESULT recreate();
    // Moves a WARP device onto a hardware one made elsewhere (HardwareDeviceLoader): the same bump of
    // `generation()` as a loss, so everything built under the old device rebuilds. Unchanged on failure.
    HRESULT adopt_hardware(ComPtr<ID3D11Device> d3d, bool video_support);
    static bool is_loss(HRESULT hr);

private:
    GfxDevice() = default;
    HRESULT build();

    DeviceOptions options_;
    bool warp_ = false;
    uint32_t generation_ = 1;
    ComPtr<ID3D11Device> d3d_;
    ComPtr<IDXGIDevice> dxgi_;
    ComPtr<ID2D1Factory1> factory_;
    ComPtr<ID2D1Device> device_;
    ComPtr<IDWriteFactory> dwrite_;
    ComPtr<IWICImagingFactory> wic_;
};

// A hardware Direct3D device made on a thread of its own. Creating one loads the GPU's user-mode driver and
// takes the better part of 200 ms on a discrete card, which the window does not need before its first frame:
// the app starts on WARP and moves over when this finishes (`Window::use_hardware_when_ready`).
class HardwareDeviceLoader {
public:
    explicit HardwareDeviceLoader(bool video_support);
    ~HardwareDeviceLoader();  // waits for the thread: a driver is not left half-loaded at exit
    HardwareDeviceLoader(const HardwareDeviceLoader&) = delete;
    HardwareDeviceLoader& operator=(const HardwareDeviceLoader&) = delete;

    bool video_support() const { return video_; }
    // Posts `msg` to `hwnd` once the attempt is over - at once if it already is - whether or not it found a
    // device, so the window can say the drawing has settled either way.
    void notify(HWND hwnd, UINT msg);
    // The device, once. Null when it is not ready, was taken, or the machine has no hardware adapter.
    ComPtr<ID3D11Device> take();
    // Whether the attempt is over, whichever way it went.
    bool finished();

private:
    void run();

    bool video_;
    std::mutex mu_;
    std::thread thread_;
    ComPtr<ID3D11Device> device_;
    bool done_ = false;
    bool posted_ = false;
    HWND hwnd_ = nullptr;
    UINT msg_ = 0;
};

}  // namespace bb::gfx
