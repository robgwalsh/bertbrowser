#include "views/pane_images.h"

#include <windows.h>

#include <dwrite_3.h>
#include <shobjidl.h>
#include <shlobj.h>
#include <wincodec.h>
#include <wrl/client.h>

#include <algorithm>

namespace bb::views {

using Microsoft::WRL::ComPtr;

namespace {

// COM on the calling thread, for as long as the thread lives: a worker calls this once and never undoes it.
void ensure_com() {
    static thread_local bool done = false;
    if (!done) {
        CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
        done = true;
    }
}

ComPtr<IWICImagingFactory> wic() {
    ensure_com();
    ComPtr<IWICImagingFactory> f;
    CoCreateInstance(CLSID_WICImagingFactory, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&f));
    return f;
}

}  // namespace

bool read_whole_file(const std::wstring& path, uint64_t limit, std::vector<uint8_t>& out) {
    out.clear();
    HANDLE h = CreateFileW(path.c_str(), GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, nullptr,
                           OPEN_EXISTING, FILE_FLAG_SEQUENTIAL_SCAN, nullptr);
    if (h == INVALID_HANDLE_VALUE) return false;
    LARGE_INTEGER size{};
    if (!GetFileSizeEx(h, &size) || static_cast<uint64_t>(size.QuadPart) > limit) {
        CloseHandle(h);
        return false;
    }
    out.resize(static_cast<size_t>(size.QuadPart));
    size_t at = 0;
    while (at < out.size()) {
        DWORD got = 0;
        const DWORD want = static_cast<DWORD>(std::min<size_t>(out.size() - at, 1u << 20));
        if (!ReadFile(h, out.data() + at, want, &got, nullptr) || got == 0) break;
        at += got;
    }
    CloseHandle(h);
    out.resize(at);
    return true;
}

ui::ThumbPtr decode_picture(const std::vector<uint8_t>& bytes, uint32_t max_width, uint32_t* width_out, uint32_t* height_out) {
    if (bytes.empty()) return nullptr;
    auto factory = wic();
    if (!factory) return nullptr;
    ComPtr<IWICStream> stream;
    if (FAILED(factory->CreateStream(&stream)) ||
        FAILED(stream->InitializeFromMemory(const_cast<BYTE*>(bytes.data()), static_cast<DWORD>(bytes.size())))) {
        return nullptr;
    }
    ComPtr<IWICBitmapDecoder> decoder;
    if (FAILED(factory->CreateDecoderFromStream(stream.Get(), nullptr, WICDecodeMetadataCacheOnDemand, &decoder))) return nullptr;
    ComPtr<IWICBitmapFrameDecode> frame;
    if (FAILED(decoder->GetFrame(0, &frame))) return nullptr;
    UINT w = 0, h = 0;
    frame->GetSize(&w, &h);
    if (!w || !h) return nullptr;
    if (width_out) *width_out = w;
    if (height_out) *height_out = h;

    ComPtr<IWICBitmapSource> source = frame;
    UINT tw = w, th = h;
    if (max_width && w > max_width) {
        tw = max_width;
        th = std::max<UINT>(1, static_cast<UINT>(static_cast<uint64_t>(h) * max_width / w));
        ComPtr<IWICBitmapScaler> scaler;
        if (FAILED(factory->CreateBitmapScaler(&scaler)) ||
            FAILED(scaler->Initialize(source.Get(), tw, th, WICBitmapInterpolationModeFant))) {
            return nullptr;
        }
        source = scaler;
    }
    ComPtr<IWICFormatConverter> converter;
    if (FAILED(factory->CreateFormatConverter(&converter)) ||
        FAILED(converter->Initialize(source.Get(), GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, nullptr, 0.0,
                                     WICBitmapPaletteTypeCustom))) {
        return nullptr;
    }
    auto image = std::make_shared<ui::ThumbImage>();
    image->width = tw;
    image->height = th;
    image->bgra.resize(static_cast<size_t>(tw) * th * 4);
    if (FAILED(converter->CopyPixels(nullptr, tw * 4, static_cast<UINT>(image->bgra.size()), image->bgra.data()))) return nullptr;
    return image;
}

ui::ThumbPtr decode_picture_file(const std::wstring& path, uint32_t max_width, uint32_t* width_out, uint32_t* height_out) {
    std::vector<uint8_t> bytes;
    if (!read_whole_file(path, 64ull << 20, bytes)) return nullptr;
    return decode_picture(bytes, max_width, width_out, height_out);
}

ui::ThumbPtr shell_preview(const std::wstring& path, uint32_t px) {
    ensure_com();
    ComPtr<IShellItemImageFactory> factory;
    if (FAILED(SHCreateItemFromParsingName(path.c_str(), nullptr, IID_PPV_ARGS(&factory))) || !factory) return nullptr;
    HBITMAP hbm = nullptr;
    // Thumbnails only: without it the shell answers a file with no preview by handing back its icon.
    const HRESULT hr =
        factory->GetImage(SIZE{static_cast<LONG>(px), static_cast<LONG>(px)}, SIIGBF_BIGGERSIZEOK | SIIGBF_THUMBNAILONLY, &hbm);
    if (FAILED(hr) || !hbm) return nullptr;
    BITMAP bm{};
    GetObjectW(hbm, sizeof(bm), &bm);
    ui::ThumbPtr result;
    if (bm.bmWidth > 0 && bm.bmHeight > 0) {
        auto img = std::make_shared<ui::ThumbImage>();
        img->width = static_cast<uint32_t>(bm.bmWidth);
        img->height = static_cast<uint32_t>(bm.bmHeight);
        img->bgra.assign(static_cast<size_t>(img->width) * img->height * 4, 0);
        BITMAPINFO bi{};
        bi.bmiHeader.biSize = sizeof(bi.bmiHeader);
        bi.bmiHeader.biWidth = bm.bmWidth;
        bi.bmiHeader.biHeight = -bm.bmHeight;
        bi.bmiHeader.biPlanes = 1;
        bi.bmiHeader.biBitCount = 32;
        bi.bmiHeader.biCompression = BI_RGB;
        HDC dc = GetDC(nullptr);
        GetDIBits(dc, hbm, 0, static_cast<UINT>(bm.bmHeight), img->bgra.data(), &bi, DIB_RGB_COLORS);
        ReleaseDC(nullptr, dc);
        bool any_alpha = false;
        for (size_t i = 3; i < img->bgra.size() && !any_alpha; i += 4) any_alpha = img->bgra[i] != 0;
        if (!any_alpha) {
            for (size_t i = 3; i < img->bgra.size(); i += 4) img->bgra[i] = 255;
        }
        result = img;
    }
    DeleteObject(hbm);
    return result;
}

FontInfo read_font_info(const std::wstring& path) {
    FontInfo info;
    ensure_com();
    ComPtr<IDWriteFactory3> dw;
    if (FAILED(DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED, __uuidof(IDWriteFactory3), reinterpret_cast<IUnknown**>(dw.GetAddressOf())))) return info;
    ComPtr<IDWriteFontFile> file;
    if (FAILED(dw->CreateFontFileReference(path.c_str(), nullptr, &file))) return info;
    BOOL supported = FALSE;
    DWRITE_FONT_FILE_TYPE type{};
    DWRITE_FONT_FACE_TYPE face_type{};
    UINT32 faces = 0;
    if (FAILED(file->Analyze(&supported, &type, &face_type, &faces)) || !supported || faces == 0) return info;
    ComPtr<IDWriteFontFace> base;
    IDWriteFontFile* files[] = {file.Get()};
    if (FAILED(dw->CreateFontFace(face_type, 1, files, 0, DWRITE_FONT_SIMULATIONS_NONE, &base))) return info;
    ComPtr<IDWriteFontFace3> face;
    if (FAILED(base.As(&face))) return info;
    auto first = [](IDWriteLocalizedStrings* s) {
        std::wstring out;
        if (!s) return out;
        UINT32 index = 0;
        BOOL exists = FALSE;
        s->FindLocaleName(L"en-us", &index, &exists);
        if (!exists) index = 0;
        UINT32 len = 0;
        if (SUCCEEDED(s->GetStringLength(index, &len))) {
            out.resize(len + 1);
            if (SUCCEEDED(s->GetString(index, out.data(), len + 1))) out.resize(len);
            else out.clear();
        }
        return out;
    };
    ComPtr<IDWriteLocalizedStrings> family, faces_names, version;
    face->GetFamilyNames(&family);
    face->GetFaceNames(&faces_names);
    face->GetInformationalStrings(DWRITE_INFORMATIONAL_STRING_VERSION_STRINGS, &version, nullptr);
    info.family = first(family.Get());
    info.face = first(faces_names.Get());
    info.version = first(version.Get());
    info.glyphs = face->GetGlyphCount();
    info.ok = !info.family.empty();
    return info;
}

}  // namespace bb::views
