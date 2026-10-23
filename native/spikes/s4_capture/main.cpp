// Spike S4: the harness capture path. A window parked off screen that never activates, a Direct3D 11
// WARP device (no GPU needed, same code path as the real swap chain), a Direct2D device context drawing
// DirectWrite text into a CPU-readable bitmap, read back and written as PNG through WIC, then checked
// for the flat-colour failure the C# harness applies. The foreground window must be untouched.
#include <windows.h>

#include <d2d1_1.h>
#include <d3d11.h>
#include <dwrite.h>
#include <dxgi1_2.h>
#include <wincodec.h>
#include <wrl/client.h>

#include <cstdio>
#include <string>
#include <vector>

using Microsoft::WRL::ComPtr;

namespace {

int fail(const char* what, HRESULT hr = S_OK) {
    std::printf("S4 FAIL: %s (0x%08lx)\n", what, static_cast<unsigned long>(hr));
    return 1;
}

LRESULT CALLBACK wnd_proc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp) {
    return DefWindowProcW(hwnd, msg, wp, lp);
}

}  // namespace

int wmain(int argc, wchar_t** argv) {
    const std::wstring out = argc > 1 ? argv[1] : L"s4_capture.png";
    const HWND foreground_before = GetForegroundWindow();

    // 1. A parked window that cannot activate or appear in the taskbar.
    WNDCLASSW wc{};
    wc.lpfnWndProc = wnd_proc;
    wc.hInstance = GetModuleHandleW(nullptr);
    wc.lpszClassName = L"BbSpikeS4";
    RegisterClassW(&wc);
    const HWND hwnd = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, wc.lpszClassName, L"s4",
                                      WS_OVERLAPPEDWINDOW, -32000, -32000, 640, 400, nullptr, nullptr,
                                      wc.hInstance, nullptr);
    if (!hwnd) {
        return fail("CreateWindowExW");
    }
    ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    EnableWindow(hwnd, FALSE);

    // 2. WARP device, D2D device and context.
    ComPtr<ID3D11Device> d3d;
    HRESULT hr = D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_WARP, nullptr, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                                   nullptr, 0, D3D11_SDK_VERSION, &d3d, nullptr, nullptr);
    if (FAILED(hr)) {
        return fail("D3D11CreateDevice WARP", hr);
    }
    ComPtr<IDXGIDevice> dxgi;
    d3d.As(&dxgi);
    ComPtr<ID2D1Factory1> factory;
    hr = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, IID_PPV_ARGS(&factory));
    if (FAILED(hr)) {
        return fail("D2D1CreateFactory", hr);
    }
    ComPtr<ID2D1Device> device;
    hr = factory->CreateDevice(dxgi.Get(), &device);
    if (FAILED(hr)) {
        return fail("CreateDevice", hr);
    }
    ComPtr<ID2D1DeviceContext> ctx;
    hr = device->CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE, &ctx);
    if (FAILED(hr)) {
        return fail("CreateDeviceContext", hr);
    }

    // 3. A bitmap target (what the real window would draw into a swap chain) and a CPU-readable copy.
    const UINT width = 640, height = 400;
    const D2D1_PIXEL_FORMAT fmt = D2D1::PixelFormat(DXGI_FORMAT_B8G8R8A8_UNORM, D2D1_ALPHA_MODE_PREMULTIPLIED);
    ComPtr<ID2D1Bitmap1> target;
    hr = ctx->CreateBitmap(D2D1::SizeU(width, height), nullptr, 0,
                           D2D1::BitmapProperties1(D2D1_BITMAP_OPTIONS_TARGET, fmt, 96.0f, 96.0f), &target);
    if (FAILED(hr)) {
        return fail("CreateBitmap target", hr);
    }
    ComPtr<ID2D1Bitmap1> staging;
    hr = ctx->CreateBitmap(D2D1::SizeU(width, height), nullptr, 0,
                           D2D1::BitmapProperties1(D2D1_BITMAP_OPTIONS_CPU_READ | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                                                   fmt, 96.0f, 96.0f),
                           &staging);
    if (FAILED(hr)) {
        return fail("CreateBitmap staging", hr);
    }

    // 4. Draw: a themed-looking background, a header strip, and DirectWrite text.
    ComPtr<IDWriteFactory> dwrite;
    hr = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED, __uuidof(IDWriteFactory),
                             reinterpret_cast<IUnknown**>(dwrite.GetAddressOf()));
    if (FAILED(hr)) {
        return fail("DWriteCreateFactory", hr);
    }
    ComPtr<IDWriteTextFormat> format;
    dwrite->CreateTextFormat(L"Segoe UI", nullptr, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STYLE_NORMAL,
                             DWRITE_FONT_STRETCH_NORMAL, 14.0f, L"en-us", &format);
    ComPtr<ID2D1SolidColorBrush> bg, strip, text;
    ctx->CreateSolidColorBrush(D2D1::ColorF(0.118f, 0.118f, 0.118f), &bg);
    ctx->CreateSolidColorBrush(D2D1::ColorF(0.20f, 0.20f, 0.20f), &strip);
    ctx->CreateSolidColorBrush(D2D1::ColorF(0.85f, 0.85f, 0.85f), &text);

    ctx->SetTarget(target.Get());
    ctx->BeginDraw();
    ctx->Clear(D2D1::ColorF(0.118f, 0.118f, 0.118f));
    ctx->FillRectangle(D2D1::RectF(0, 0, static_cast<float>(width), 38), strip.Get());
    const std::wstring caption = L"BertBrowser native: offscreen capture through WARP";
    ctx->DrawTextW(caption.c_str(), static_cast<UINT32>(caption.size()), format.Get(),
                   D2D1::RectF(12, 10, static_cast<float>(width), 38), text.Get());
    for (int row = 0; row < 8; ++row) {
        const std::wstring line = L"row " + std::to_wstring(row) + L"    file" + std::to_wstring(row) + L".txt";
        ctx->DrawTextW(line.c_str(), static_cast<UINT32>(line.size()), format.Get(),
                       D2D1::RectF(12, 50.0f + row * 22.0f, static_cast<float>(width), 72.0f + row * 22.0f),
                       text.Get());
    }
    hr = ctx->EndDraw();
    if (FAILED(hr)) {
        return fail("EndDraw", hr);
    }

    // 5. Read back.
    hr = staging->CopyFromBitmap(nullptr, target.Get(), nullptr);
    if (FAILED(hr)) {
        return fail("CopyFromBitmap", hr);
    }
    D2D1_MAPPED_RECT mapped{};
    hr = staging->Map(D2D1_MAP_OPTIONS_READ, &mapped);
    if (FAILED(hr)) {
        return fail("Map", hr);
    }
    std::vector<BYTE> pixels(static_cast<size_t>(width) * height * 4);
    for (UINT y = 0; y < height; ++y) {
        memcpy(pixels.data() + static_cast<size_t>(y) * width * 4, mapped.bits + static_cast<size_t>(y) * mapped.pitch,
               static_cast<size_t>(width) * 4);
    }
    staging->Unmap();

    // 6. The flat-colour check: a capture that is one colour is a failure, as in the C# harness.
    size_t distinct = 0;
    const uint32_t first = *reinterpret_cast<const uint32_t*>(pixels.data());
    for (size_t i = 0; i < pixels.size(); i += 4) {
        if (*reinterpret_cast<const uint32_t*>(pixels.data() + i) != first) {
            ++distinct;
        }
    }
    if (distinct == 0) {
        return fail("capture is one flat colour");
    }

    // 7. PNG through WIC.
    hr = CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
    if (FAILED(hr)) {
        return fail("CoInitializeEx", hr);
    }
    ComPtr<IWICImagingFactory> wic;
    hr = CoCreateInstance(CLSID_WICImagingFactory, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&wic));
    if (FAILED(hr)) {
        return fail("WICImagingFactory", hr);
    }
    ComPtr<IWICStream> stream;
    wic->CreateStream(&stream);
    hr = stream->InitializeFromFilename(out.c_str(), GENERIC_WRITE);
    if (FAILED(hr)) {
        return fail("InitializeFromFilename", hr);
    }
    ComPtr<IWICBitmapEncoder> encoder;
    wic->CreateEncoder(GUID_ContainerFormatPng, nullptr, &encoder);
    encoder->Initialize(stream.Get(), WICBitmapEncoderNoCache);
    ComPtr<IWICBitmapFrameEncode> frame;
    encoder->CreateNewFrame(&frame, nullptr);
    frame->Initialize(nullptr);
    frame->SetSize(width, height);
    WICPixelFormatGUID pf = GUID_WICPixelFormat32bppPBGRA;
    frame->SetPixelFormat(&pf);
    hr = frame->WritePixels(height, width * 4, static_cast<UINT>(pixels.size()), pixels.data());
    if (FAILED(hr)) {
        return fail("WritePixels", hr);
    }
    frame->Commit();
    hr = encoder->Commit();
    if (FAILED(hr)) {
        return fail("encoder Commit", hr);
    }

    // 8. Nothing on screen changed hands.
    const bool foreground_same = GetForegroundWindow() == foreground_before;
    DestroyWindow(hwnd);
    if (!foreground_same) {
        return fail("the foreground window changed");
    }
    std::printf("S4 OK: %ux%u capture, %zu non-background pixels, foreground untouched -> %ls\n", width, height,
                distinct, out.c_str());
    return 0;
}
