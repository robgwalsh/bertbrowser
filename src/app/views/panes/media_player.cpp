#include "views/panes/media_player.h"

#include <windows.h>

#include <d3d11.h>
#include <mfapi.h>
#include <mferror.h>
#include <mfidl.h>
#include <wrl/client.h>
#include <wrl/implements.h>

#include <mutex>

namespace bb::views {

using Microsoft::WRL::ComPtr;

class MediaPlayer::Notify : public Microsoft::WRL::RuntimeClass<Microsoft::WRL::RuntimeClassFlags<Microsoft::WRL::ClassicCom>, IMFMediaEngineNotify> {
public:
    std::atomic<bool> loaded{false}, errored{false};

    STDMETHODIMP EventNotify(DWORD event, DWORD_PTR, DWORD) noexcept override {
        switch (event) {
            case MF_MEDIA_ENGINE_EVENT_LOADEDMETADATA: loaded = true; break;
            case MF_MEDIA_ENGINE_EVENT_ERROR: errored = true; break;
            default: break;
        }
        return S_OK;
    }
};

struct MediaPlayer::Impl {
    ComPtr<IMFDXGIDeviceManager> manager;
    ComPtr<Notify> notify;
    ComPtr<IMFMediaEngine> engine;
    ComPtr<ID3D11DeviceContext> context;
    ComPtr<ID3D11Texture2D> target, staging;
    uint32_t tex_w = 0, tex_h = 0;
};

MediaPlayer::MediaPlayer(gfx::GfxDevice& dev) : impl_(std::make_unique<Impl>()), dev_(dev) {
    static std::once_flag started;
    std::call_once(started, [] { MFStartup(MF_VERSION); });
}

MediaPlayer::~MediaPlayer() { close(); }

bool MediaPlayer::open(const std::wstring& path) {
    close();
    ready_ = failed_ = false;
    video_w_ = video_h_ = 0;
    ID3D11Device* d3d = dev_.d3d();
    {
        ComPtr<ID3D10Multithread> mt;
        if (SUCCEEDED(d3d->QueryInterface(IID_PPV_ARGS(&mt)))) mt->SetMultithreadProtected(TRUE);
    }
    UINT token = 0;
    if (FAILED(MFCreateDXGIDeviceManager(&token, &impl_->manager)) || FAILED(impl_->manager->ResetDevice(d3d, token))) {
        failed_ = true;
        return false;
    }
    d3d->GetImmediateContext(&impl_->context);
    impl_->notify = Microsoft::WRL::Make<Notify>();
    ComPtr<IMFAttributes> attrs;
    MFCreateAttributes(&attrs, 3);
    attrs->SetUnknown(MF_MEDIA_ENGINE_DXGI_MANAGER, impl_->manager.Get());
    attrs->SetUnknown(MF_MEDIA_ENGINE_CALLBACK, impl_->notify.Get());
    attrs->SetUINT32(MF_MEDIA_ENGINE_VIDEO_OUTPUT_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM);
    ComPtr<IMFMediaEngineClassFactory> factory;
    if (FAILED(CoCreateInstance(CLSID_MFMediaEngineClassFactory, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&factory))) ||
        FAILED(factory->CreateInstance(0, attrs.Get(), &impl_->engine))) {
        failed_ = true;
        return false;
    }
    BSTR url = SysAllocString(path.c_str());
    const HRESULT hr = impl_->engine->SetSource(url);
    SysFreeString(url);
    if (FAILED(hr)) {
        failed_ = true;
        return false;
    }
    return true;
}

void MediaPlayer::close() {
    if (impl_->engine) {
        impl_->engine->Shutdown();
        impl_->engine.Reset();
    }
    impl_->target.Reset();
    impl_->staging.Reset();
    impl_->tex_w = impl_->tex_h = 0;
    impl_->notify.Reset();
    ready_ = false;
}

bool MediaPlayer::ended() const { return impl_->engine && impl_->engine->IsEnded(); }

void MediaPlayer::play() {
    if (impl_->engine) impl_->engine->Play();
}

void MediaPlayer::pause() {
    if (impl_->engine) impl_->engine->Pause();
}

bool MediaPlayer::playing() const { return impl_->engine && !impl_->engine->IsPaused() && !impl_->engine->IsEnded(); }

double MediaPlayer::position() const { return impl_->engine ? impl_->engine->GetCurrentTime() : 0.0; }

double MediaPlayer::duration() const {
    if (!impl_->engine) return 0.0;
    const double d = impl_->engine->GetDuration();
    return std::isfinite(d) ? d : 0.0;
}

void MediaPlayer::seek(double seconds) {
    if (impl_->engine) impl_->engine->SetCurrentTime(seconds);
}

void MediaPlayer::set_volume(double v) {
    if (impl_->engine) impl_->engine->SetVolume(std::clamp(v, 0.0, 1.0));
}

double MediaPlayer::volume() const { return impl_->engine ? impl_->engine->GetVolume() : 1.0; }

void MediaPlayer::set_loop(bool loop) {
    if (impl_->engine) impl_->engine->SetLoop(loop ? TRUE : FALSE);
}

ui::ThumbPtr MediaPlayer::take_frame() {
    if (!impl_->engine || !impl_->notify) return nullptr;
    if (impl_->notify->errored) {
        failed_ = true;
        return nullptr;
    }
    if (!ready_ && impl_->notify->loaded) {
        ready_ = true;
        DWORD w = 0, h = 0;
        impl_->engine->GetNativeVideoSize(&w, &h);
        video_w_ = w;
        video_h_ = h;
    }
    if (!ready_ || !video_w_ || !video_h_) return nullptr;
    LONGLONG pts = 0;
    if (impl_->engine->OnVideoStreamTick(&pts) != S_OK) return nullptr;

    ID3D11Device* d3d = dev_.d3d();
    if (!impl_->target || impl_->tex_w != video_w_ || impl_->tex_h != video_h_) {
        D3D11_TEXTURE2D_DESC desc{};
        desc.Width = video_w_;
        desc.Height = video_h_;
        desc.MipLevels = 1;
        desc.ArraySize = 1;
        desc.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
        desc.SampleDesc.Count = 1;
        desc.Usage = D3D11_USAGE_DEFAULT;
        desc.BindFlags = D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE;
        impl_->target.Reset();
        impl_->staging.Reset();
        if (FAILED(d3d->CreateTexture2D(&desc, nullptr, &impl_->target))) return nullptr;
        desc.Usage = D3D11_USAGE_STAGING;
        desc.BindFlags = 0;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
        if (FAILED(d3d->CreateTexture2D(&desc, nullptr, &impl_->staging))) return nullptr;
        impl_->tex_w = video_w_;
        impl_->tex_h = video_h_;
    }
    const RECT dst{0, 0, static_cast<LONG>(video_w_), static_cast<LONG>(video_h_)};
    const MFARGB border{0, 0, 0, 255};
    if (FAILED(impl_->engine->TransferVideoFrame(impl_->target.Get(), nullptr, &dst, &border))) return nullptr;
    impl_->context->CopyResource(impl_->staging.Get(), impl_->target.Get());
    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (FAILED(impl_->context->Map(impl_->staging.Get(), 0, D3D11_MAP_READ, 0, &mapped))) return nullptr;
    auto image = std::make_shared<ui::ThumbImage>();
    image->width = video_w_;
    image->height = video_h_;
    image->bgra.resize(static_cast<size_t>(video_w_) * video_h_ * 4);
    for (uint32_t y = 0; y < video_h_; ++y) {
        const uint8_t* src = static_cast<const uint8_t*>(mapped.pData) + static_cast<size_t>(y) * mapped.RowPitch;
        uint8_t* out = &image->bgra[static_cast<size_t>(y) * video_w_ * 4];
        memcpy(out, src, static_cast<size_t>(video_w_) * 4);
        for (uint32_t x = 0; x < video_w_; ++x) out[x * 4 + 3] = 255;
    }
    impl_->context->Unmap(impl_->staging.Get(), 0);
    return image;
}

}  // namespace bb::views
