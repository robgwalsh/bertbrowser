// Spike S2: video in the preview pane without WPF's MediaElement. IMFMediaEngine in frame-server
// mode (no window of its own), frames transferred into a Direct3D 11 texture that Direct2D can wrap,
// on the same WARP device the harness uses, with seek and volume. The risk is that WARP, or the
// engine without a visible window, refuses frame transfer; the proof is pixels of the right colour.
//
// Self-contained: it encodes a two-second H.264 clip with the sink writer (one second red, one
// second blue) into %TEMP%, plays it, and checks the frame colour before and after a seek.
#include <windows.h>

#include <d3d11.h>
#include <mfapi.h>
#include <mferror.h>
#include <mfidl.h>
#include <mfmediaengine.h>
#include <mfreadwrite.h>
#include <wrl/client.h>
#include <wrl/implements.h>

#include <condition_variable>
#include <cstdio>
#include <mutex>
#include <set>
#include <string>
#include <vector>

using Microsoft::WRL::ComPtr;

namespace {

int failures = 0;

void check(bool ok, const char* what, HRESULT hr = S_OK) {
    if (!ok) {
        ++failures;
        std::printf("S2 FAIL: %s (0x%08lx)\n", what, static_cast<unsigned long>(hr));
    }
}

constexpr UINT32 W = 320, H = 240;
constexpr UINT32 FPS = 30;
constexpr LONGLONG FRAME_100NS = 10'000'000 / FPS;

// ---- encode a clip -----------------------------------------------------------------------------

bool write_clip(const std::wstring& path) {
    ComPtr<IMFSinkWriter> writer;
    HRESULT hr = MFCreateSinkWriterFromURL(path.c_str(), nullptr, nullptr, &writer);
    if (FAILED(hr)) {
        check(false, "MFCreateSinkWriterFromURL", hr);
        return false;
    }
    ComPtr<IMFMediaType> out;
    MFCreateMediaType(&out);
    out->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Video);
    out->SetGUID(MF_MT_SUBTYPE, MFVideoFormat_H264);
    out->SetUINT32(MF_MT_AVG_BITRATE, 1'000'000);
    out->SetUINT32(MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive);
    MFSetAttributeSize(out.Get(), MF_MT_FRAME_SIZE, W, H);
    MFSetAttributeRatio(out.Get(), MF_MT_FRAME_RATE, FPS, 1);
    MFSetAttributeRatio(out.Get(), MF_MT_PIXEL_ASPECT_RATIO, 1, 1);
    DWORD stream = 0;
    hr = writer->AddStream(out.Get(), &stream);
    if (FAILED(hr)) {
        check(false, "AddStream H264", hr);
        return false;
    }
    ComPtr<IMFMediaType> in;
    MFCreateMediaType(&in);
    in->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Video);
    in->SetGUID(MF_MT_SUBTYPE, MFVideoFormat_RGB32);
    in->SetUINT32(MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive);
    MFSetAttributeSize(in.Get(), MF_MT_FRAME_SIZE, W, H);
    MFSetAttributeRatio(in.Get(), MF_MT_FRAME_RATE, FPS, 1);
    MFSetAttributeRatio(in.Get(), MF_MT_PIXEL_ASPECT_RATIO, 1, 1);
    hr = writer->SetInputMediaType(stream, in.Get(), nullptr);
    if (FAILED(hr)) {
        check(false, "SetInputMediaType RGB32", hr);
        return false;
    }
    hr = writer->BeginWriting();
    if (FAILED(hr)) {
        check(false, "BeginWriting", hr);
        return false;
    }
    const DWORD bytes = W * H * 4;
    for (UINT32 i = 0; i < FPS * 2; ++i) {
        ComPtr<IMFMediaBuffer> buffer;
        MFCreateMemoryBuffer(bytes, &buffer);
        BYTE* data = nullptr;
        buffer->Lock(&data, nullptr, nullptr);
        for (DWORD p = 0; p < bytes; p += 4) {
            const bool red = i < FPS;
            data[p] = red ? 0 : 255;  // B
            data[p + 1] = 0;          // G
            data[p + 2] = red ? 255 : 0;  // R
            data[p + 3] = 255;
        }
        buffer->Unlock();
        buffer->SetCurrentLength(bytes);
        ComPtr<IMFSample> sample;
        MFCreateSample(&sample);
        sample->AddBuffer(buffer.Get());
        sample->SetSampleTime(static_cast<LONGLONG>(i) * FRAME_100NS);
        sample->SetSampleDuration(FRAME_100NS);
        hr = writer->WriteSample(stream, sample.Get());
        if (FAILED(hr)) {
            check(false, "WriteSample", hr);
            return false;
        }
    }
    hr = writer->Finalize();
    check(SUCCEEDED(hr), "Finalize", hr);
    return SUCCEEDED(hr);
}

// ---- play it -----------------------------------------------------------------------------------

class Notify : public Microsoft::WRL::RuntimeClass<Microsoft::WRL::RuntimeClassFlags<Microsoft::WRL::ClassicCom>,
                                                    IMFMediaEngineNotify> {
public:
    STDMETHODIMP EventNotify(DWORD event, DWORD_PTR, DWORD param2) noexcept override {
        std::lock_guard<std::mutex> lock(m_);
        seen_.insert(event);
        order_.push_back(event);
        if (event == MF_MEDIA_ENGINE_EVENT_ERROR) error_ = static_cast<HRESULT>(param2);
        cv_.notify_all();
        return S_OK;
    }

    bool wait_for(DWORD event, DWORD ms) {
        std::unique_lock<std::mutex> lock(m_);
        return cv_.wait_for(lock, std::chrono::milliseconds(ms), [&] { return seen_.count(event) > 0; });
    }

    void forget(DWORD event) {
        std::lock_guard<std::mutex> lock(m_);
        seen_.erase(event);
    }

    HRESULT error() {
        std::lock_guard<std::mutex> lock(m_);
        return error_;
    }

    std::string sequence() {
        std::lock_guard<std::mutex> lock(m_);
        std::string out;
        for (DWORD e : order_) out += std::to_string(e) + " ";
        return out;
    }

private:
    std::mutex m_;
    std::condition_variable cv_;
    std::set<DWORD> seen_;
    std::vector<DWORD> order_;
    HRESULT error_ = S_OK;
};

struct Pixel {
    BYTE b, g, r, a;
};

// Transfers the current frame into `target` and reads the centre pixel back through a staging copy.
bool read_centre(IMFMediaEngine* engine, ID3D11Device* d3d, ID3D11DeviceContext* ctx, ID3D11Texture2D* target,
                 ID3D11Texture2D* staging, Pixel& out) {
    const RECT dst{0, 0, static_cast<LONG>(W), static_cast<LONG>(H)};
    const MFARGB border{0, 0, 0, 255};
    const HRESULT hr = engine->TransferVideoFrame(target, nullptr, &dst, &border);
    if (FAILED(hr)) {
        check(false, "TransferVideoFrame", hr);
        return false;
    }
    (void)d3d;
    ctx->CopyResource(staging, target);
    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (FAILED(ctx->Map(staging, 0, D3D11_MAP_READ, 0, &mapped))) {
        check(false, "Map staging");
        return false;
    }
    const auto* row = static_cast<const BYTE*>(mapped.pData) + static_cast<size_t>(H / 2) * mapped.RowPitch;
    memcpy(&out, row + static_cast<size_t>(W / 2) * 4, 4);
    ctx->Unmap(staging, 0);
    return true;
}

bool wait_tick(IMFMediaEngine* engine, DWORD ms) {
    const ULONGLONG start = GetTickCount64();
    while (GetTickCount64() - start < ms) {
        LONGLONG pts = 0;
        if (engine->OnVideoStreamTick(&pts) == S_OK) return true;
        Sleep(10);
    }
    return false;
}

// One full playback run on one device configuration. Returns the number of failed checks, so the
// caller can decide which configurations are required and which are merely reported.
int play_on(const std::wstring& clip, D3D_DRIVER_TYPE driver, UINT flags, const char* label) {
    const int before = failures;
    std::printf("S2 [%s]\n", label);
    ComPtr<ID3D11Device> d3d;
    ComPtr<ID3D11DeviceContext> ctx;
    HRESULT hr = D3D11CreateDevice(nullptr, driver, nullptr, flags, nullptr, 0, D3D11_SDK_VERSION, &d3d, nullptr,
                                   &ctx);
    if (FAILED(hr)) {
        check(false, "D3D11CreateDevice", hr);
        return failures - before;
    }
    {
        ComPtr<ID3D10Multithread> mt;
        if (SUCCEEDED(d3d.As(&mt))) mt->SetMultithreadProtected(TRUE);
    }
    UINT token = 0;
    ComPtr<IMFDXGIDeviceManager> manager;
    check(SUCCEEDED(MFCreateDXGIDeviceManager(&token, &manager)), "MFCreateDXGIDeviceManager");
    hr = manager->ResetDevice(d3d.Get(), token);
    check(SUCCEEDED(hr), "ResetDevice", hr);

    auto notify = Microsoft::WRL::Make<Notify>();
    ComPtr<IMFAttributes> attrs;
    MFCreateAttributes(&attrs, 3);
    attrs->SetUnknown(MF_MEDIA_ENGINE_DXGI_MANAGER, manager.Get());
    attrs->SetUnknown(MF_MEDIA_ENGINE_CALLBACK, notify.Get());
    attrs->SetUINT32(MF_MEDIA_ENGINE_VIDEO_OUTPUT_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM);
    ComPtr<IMFMediaEngineClassFactory> factory;
    hr = CoCreateInstance(CLSID_MFMediaEngineClassFactory, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&factory));
    check(SUCCEEDED(hr), "MFMediaEngineClassFactory", hr);
    ComPtr<IMFMediaEngine> engine;
    hr = factory->CreateInstance(0, attrs.Get(), &engine);  // no window: frame-server mode
    check(SUCCEEDED(hr), "IMFMediaEngine CreateInstance", hr);
    if (!engine) return failures - before;

    BSTR url = SysAllocString(clip.c_str());
    hr = engine->SetSource(url);
    SysFreeString(url);
    check(SUCCEEDED(hr), "SetSource", hr);
    check(notify->wait_for(MF_MEDIA_ENGINE_EVENT_CANPLAY, 10000), "CANPLAY within 10 s");
    check(notify->error() == S_OK, "no engine error", notify->error());

    DWORD cx = 0, cy = 0;
    engine->GetNativeVideoSize(&cx, &cy);
    check(cx == W && cy == H, "native video size");
    const double duration = engine->GetDuration();
    check(duration > 1.9 && duration < 2.2, "duration about two seconds");

    engine->SetMuted(TRUE);
    check(SUCCEEDED(engine->SetVolume(0.5)), "SetVolume");
    check(engine->GetVolume() == 0.5, "GetVolume round-trips");

    D3D11_TEXTURE2D_DESC desc{};
    desc.Width = W;
    desc.Height = H;
    desc.MipLevels = 1;
    desc.ArraySize = 1;
    desc.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
    desc.SampleDesc.Count = 1;
    desc.Usage = D3D11_USAGE_DEFAULT;
    desc.BindFlags = D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE;
    ComPtr<ID3D11Texture2D> target;
    check(SUCCEEDED(d3d->CreateTexture2D(&desc, nullptr, &target)), "target texture");
    desc.Usage = D3D11_USAGE_STAGING;
    desc.BindFlags = 0;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    ComPtr<ID3D11Texture2D> staging;
    check(SUCCEEDED(d3d->CreateTexture2D(&desc, nullptr, &staging)), "staging texture");

    // 1. Play from the start: the first frame is red.
    check(SUCCEEDED(engine->Play()), "Play");
    check(wait_tick(engine.Get(), 5000), "a video frame arrived after Play");
    Pixel first{};
    if (read_centre(engine.Get(), d3d.Get(), ctx.Get(), target.Get(), staging.Get(), first)) {
        std::printf("  first frame centre: r=%u g=%u b=%u at t=%.2f s\n", first.r, first.g, first.b,
                    engine->GetCurrentTime());
        check(first.r > 200 && first.b < 60, "first frame is red");
    }

    // 2. Seek into the second half: the frame is blue.
    engine->Pause();
    notify->forget(MF_MEDIA_ENGINE_EVENT_SEEKED);
    check(SUCCEEDED(engine->SetCurrentTime(1.5)), "SetCurrentTime");
    // The engine reports the seek through the frame itself and the current time; SEEKED is
    // logged below but not required, since a paused engine was seen to complete the seek without it.
    const bool seeked = notify->wait_for(MF_MEDIA_ENGINE_EVENT_SEEKED, 1500);
    check(wait_tick(engine.Get(), 5000), "a frame arrived after the seek");
    Pixel after{};
    if (read_centre(engine.Get(), d3d.Get(), ctx.Get(), target.Get(), staging.Get(), after)) {
        std::printf("  after seek centre: r=%u g=%u b=%u at t=%.2f s\n", after.r, after.g, after.b,
                    engine->GetCurrentTime());
        check(after.b > 200 && after.r < 60, "frame after seek is blue");
    }
    check(engine->GetCurrentTime() > 1.4, "current time follows the seek");
    std::printf("  SEEKED within 1.5 s: %s; events seen: %s(SEEKED=%d, TIMEUPDATE=%d)\n", seeked ? "yes" : "no, it arrives later",
                notify->sequence().c_str(), MF_MEDIA_ENGINE_EVENT_SEEKED, MF_MEDIA_ENGINE_EVENT_TIMEUPDATE);

    engine->Shutdown();
    return failures - before;
}

}  // namespace

int wmain() {
    setvbuf(stdout, nullptr, _IONBF, 0);
    const HWND foreground_before = GetForegroundWindow();
    check(SUCCEEDED(CoInitializeEx(nullptr, COINIT_MULTITHREADED)), "CoInitializeEx");
    check(SUCCEEDED(MFStartup(MF_VERSION)), "MFStartup");

    wchar_t temp[MAX_PATH]{};
    GetTempPathW(MAX_PATH, temp);
    const std::wstring clip = std::wstring(temp) + L"bb-s2-" + std::to_wstring(GetCurrentProcessId()) + L".mp4";
    if (!write_clip(clip)) {
        std::printf("S2 FAILED: could not encode the test clip\n");
        return 1;
    }

    // The app's path: the real GPU with video support. Required.
    const int hardware = play_on(clip, D3D_DRIVER_TYPE_HARDWARE,
                                 D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
                                 "hardware device, video support");
    // The harness's path: WARP. WARP has no video device, so the flag is refused; the question is
    // whether the engine will still decode in software and transfer frames into a WARP texture.
    const int warp_plain = play_on(clip, D3D_DRIVER_TYPE_WARP, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                                   "WARP device, no video flag");
    const int warp_video = play_on(clip, D3D_DRIVER_TYPE_WARP,
                                   D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
                                   "WARP device, video flag (expected to be refused)");

    MFShutdown();
    DeleteFileW(clip.c_str());
    const bool foreground_same = GetForegroundWindow() == foreground_before;

    std::printf("S2 summary: hardware %s, WARP without video flag %s, WARP with video flag %s, foreground %s\n",
                hardware == 0 ? "OK" : "FAILED", warp_plain == 0 ? "OK" : "FAILED",
                warp_video == 0 ? "OK" : "refused", foreground_same ? "untouched" : "CHANGED");
    if (hardware != 0 || !foreground_same) {
        std::printf("S2 FAILED\n");
        return 1;
    }
    if (warp_plain != 0) {
        std::printf("S2 OK with a caveat: the harness cannot play video on WARP; the preview needs a software frame path there\n");
        return 0;
    }
    std::printf("S2 OK: IMFMediaEngine frame-server playback, seek and volume work on hardware and on WARP\n");
    return 0;
}
