#include "views/shell_icons.h"

#include <commctrl.h>
#include <commoncontrols.h>
#include <objbase.h>
#include <shellapi.h>
#include <shlobj.h>
#include <wincodec.h>
#include <wrl/client.h>

#include <algorithm>
#include <cwctype>

namespace bb::views {

using Microsoft::WRL::ComPtr;

namespace {

std::wstring lower_ext(const std::wstring& path) {
    const size_t slash = path.find_last_of(L"\\/");
    const size_t dot = path.rfind(L'.');
    if (dot == std::wstring::npos || (slash != std::wstring::npos && dot < slash)) return {};
    std::wstring e = path.substr(dot);
    for (wchar_t& c : e) c = static_cast<wchar_t>(std::towlower(c));
    return e;
}

bool is_shortcut(const std::wstring& ext) { return ext == L".lnk" || ext == L".url"; }

// The pictures the shell keeps, one list per size: 16, 32, 48 and 256 at 96 DPI, scaled by the system DPI.
struct ImageLists {
    struct List {
        ComPtr<IImageList> list;
        int size = 0;
    };
    List small_, large_, extra_, jumbo_;
    bool loaded = false;

    void load() {
        if (loaded) return;
        loaded = true;
        const std::pair<int, List*> want[] = {
            {SHIL_SMALL, &small_}, {SHIL_LARGE, &large_}, {SHIL_EXTRALARGE, &extra_}, {SHIL_JUMBO, &jumbo_}};
        for (const auto& [shil, slot] : want) {
            if (FAILED(SHGetImageList(shil, IID_PPV_ARGS(&slot->list))) || !slot->list) continue;
            int cx = 0, cy = 0;
            slot->list->GetIconSize(&cx, &cy);
            slot->size = cx;
        }
    }
};

ImageLists& lists() {
    static ImageLists l;
    l.load();
    return l;
}

IWICImagingFactory* wic() {
    static ComPtr<IWICImagingFactory> f;
    if (!f) CoCreateInstance(CLSID_WICImagingFactory, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&f));
    return f.Get();
}

// The list's picture for a slot, `px` square, premultiplied.
ui::ThumbPtr render(IImageList* list, int index, int overlay, uint32_t px) {
    if (!list || px == 0) return nullptr;
    HICON icon = nullptr;
    if (FAILED(list->GetIcon(index, ILD_TRANSPARENT | INDEXTOOVERLAYMASK(overlay), &icon)) || !icon) return nullptr;
    ui::ThumbPtr img = icon_picture(icon, px);
    DestroyIcon(icon);
    return img;
}

}  // namespace

ui::ThumbPtr icon_picture(HICON icon, uint32_t px) {
    IWICImagingFactory* f = wic();
    if (!icon || !f || px == 0) return nullptr;
    ComPtr<IWICBitmap> bitmap;
    const HRESULT hr = f->CreateBitmapFromHICON(icon, &bitmap);
    if (FAILED(hr)) return nullptr;
    ComPtr<IWICFormatConverter> pbgra;
    if (FAILED(f->CreateFormatConverter(&pbgra)) ||
        FAILED(pbgra->Initialize(bitmap.Get(), GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, nullptr, 0,
                                 WICBitmapPaletteTypeCustom))) {
        return nullptr;
    }
    UINT w = 0, h = 0;
    pbgra->GetSize(&w, &h);
    ComPtr<IWICBitmapSource> source = pbgra;
    if (w != px || h != px) {
        ComPtr<IWICBitmapScaler> scaler;
        if (FAILED(f->CreateBitmapScaler(&scaler)) ||
            FAILED(scaler->Initialize(pbgra.Get(), px, px, WICBitmapInterpolationModeHighQualityCubic))) {
            return nullptr;
        }
        source = scaler;
    }
    auto img = std::make_shared<ui::ThumbImage>();
    img->width = img->height = px;
    img->bgra.resize(static_cast<size_t>(px) * px * 4);
    if (FAILED(source->CopyPixels(nullptr, px * 4, static_cast<UINT>(img->bgra.size()), img->bgra.data()))) return nullptr;
    return img;
}

namespace {

// How much of the square the picture covers, as the larger side of its opaque bounds. A type that ships no
// 256-pixel picture comes out of the jumbo list as its 48 in a corner of an empty 256.
uint32_t drawn_extent(const ui::ThumbImage& img) {
    uint32_t r = 0, b = 0;
    for (uint32_t y = 0; y < img.height; ++y) {
        for (uint32_t x = 0; x < img.width; ++x) {
            if (img.bgra[(static_cast<size_t>(y) * img.width + x) * 4 + 3] > 8) {
                r = std::max(r, x + 1);
                b = std::max(b, y + 1);
            }
        }
    }
    return std::max(r, b);
}

}  // namespace

ShellIcons::ShellIcons(HWND ui_window) : window_(ui_window) {
    for (int i = 0; i < 2; ++i) threads_.emplace_back([this] { work(); });
}

ShellIcons::~ShellIcons() {
    stop_ = true;
    cv_.notify_all();
    for (auto& t : threads_) t.join();
}

bool ShellIcons::is_per_file(const std::wstring& path, bool directory, uint32_t attributes) {
    // A folder can only have a desktop.ini picture when it is marked read-only or system.
    if (directory) return (attributes & (FILE_ATTRIBUTE_READONLY | FILE_ATTRIBUTE_SYSTEM)) != 0;
    const std::wstring e = lower_ext(path);
    return e == L".exe" || e == L".ico" || e == L".lnk" || e == L".url" || e == L".cur" || e == L".ani" ||
           e == L".scr" || e == L".cpl" || e == L".msc";
}

ShellIcons::Slot ShellIcons::type_slot(const std::wstring& path, bool directory) {
    const std::wstring ext = directory ? L"<dir>" : lower_ext(path);
    const std::wstring key = ext.empty() ? L"<none>" : ext;
    if (const auto it = by_type_.find(key); it != by_type_.end()) return it->second;
    SHFILEINFOW info{};
    UINT flags = SHGFI_SYSICONINDEX | SHGFI_USEFILEATTRIBUTES;
    if (is_shortcut(ext)) flags |= SHGFI_ICON | SHGFI_SMALLICON | SHGFI_OVERLAYINDEX;  // the arrow
    const wchar_t* query = directory ? L"folder" : ext.empty() ? L"file" : ext.c_str();
    const DWORD attrs = directory ? FILE_ATTRIBUTE_DIRECTORY : FILE_ATTRIBUTE_NORMAL;
    Slot s;
    if (SHGetFileInfoW(query, attrs, &info, sizeof(info), flags)) {
        s.index = info.iIcon & 0x00FFFFFF;
        s.overlay = (flags & SHGFI_OVERLAYINDEX) ? (info.iIcon >> 24) & 0xFF : 0;
    }
    if (info.hIcon) DestroyIcon(info.hIcon);
    by_type_[key] = s;
    return s;
}

ShellIcons::Slot ShellIcons::own_slot(const std::wstring& path, Slot fallback) {
    if (const auto it = by_path_.find(path); it != by_path_.end()) {
        lru_.splice(lru_.begin(), lru_, it->second);
        // The shell could not say (index -1): the type's picture, without asking again.
        return it->second->second.index >= 0 ? it->second->second : fallback;
    }
    if (asked_.insert(path).second) {
        std::lock_guard<std::mutex> lock(mu_);
        jobs_.push_front(path);
        // Scrolling past thousands of programs must not queue all of them: the oldest asks are forgotten (and
        // asked again if they come back on screen).
        while (jobs_.size() > 256) {
            asked_.erase(jobs_.back());
            jobs_.pop_back();
        }
        cv_.notify_one();
    }
    return fallback;
}

ui::ThumbPtr ShellIcons::image(Slot s, uint32_t px) {
    if (s.index < 0 || px == 0) return nullptr;
    const uint64_t key = (static_cast<uint64_t>(s.index) << 24) | (static_cast<uint64_t>(s.overlay & 0xFF) << 16) | (px & 0xFFFF);
    if (const auto it = images_.find(key); it != images_.end()) return it->second;
    ImageLists& l = lists();
    // The smallest list at least as big, so a picture is only ever scaled down - except a jumbo one that turns
    // out to be a small picture in a big frame, where the 48 scaled up is the better answer.
    const ImageLists::List* pick = nullptr;
    for (const ImageLists::List* c : {&l.small_, &l.large_, &l.extra_, &l.jumbo_}) {
        if (c->list && c->size >= static_cast<int>(px)) {
            pick = c;
            break;
        }
    }
    if (!pick) pick = l.jumbo_.list ? &l.jumbo_ : l.extra_.list ? &l.extra_ : l.large_.list ? &l.large_ : &l.small_;
    ui::ThumbPtr img = render(pick->list.Get(), s.index, s.overlay, px);
    if (img && pick == &l.jumbo_ && l.extra_.list) {
        const ui::ThumbPtr full = render(l.jumbo_.list.Get(), s.index, s.overlay, static_cast<uint32_t>(l.jumbo_.size));
        if (full && drawn_extent(*full) <= static_cast<uint32_t>(l.extra_.size)) {
            img = render(l.extra_.list.Get(), s.index, s.overlay, px);
        }
    }
    images_[key] = img;
    return img;
}

ui::ThumbPtr ShellIcons::file(const std::wstring& path, bool directory, uint32_t attributes, uint32_t px, bool real) {
    Slot s = type_slot(path, directory);
    if (real && is_per_file(path, directory, attributes)) s = own_slot(path, s);
    return image(s, px);
}

ui::ThumbPtr ShellIcons::drive(const std::wstring& root, uint32_t px) {
    // The stock picture for the kind of drive needs nothing from the drive itself; a fixed drive's own picture
    // (the Windows logo, a BitLocker lock) is read from it on a worker, which is safe for a local disk.
    auto it = drives_.find(root);
    if (it == drives_.end()) {
        SHSTOCKICONID id = SIID_DRIVEFIXED;
        const UINT kind = GetDriveTypeW(root.c_str());
        switch (kind) {
            case DRIVE_REMOVABLE: id = SIID_DRIVEREMOVE; break;
            case DRIVE_REMOTE: id = SIID_DRIVENET; break;
            case DRIVE_CDROM: id = SIID_DRIVECD; break;
            case DRIVE_RAMDISK: id = SIID_DRIVERAM; break;
            default: break;
        }
        Drive d;
        d.fixed = kind == DRIVE_FIXED;
        SHSTOCKICONINFO info{};
        info.cbSize = sizeof(info);
        if (SUCCEEDED(SHGetStockIconInfo(id, SHGSI_SYSICONINDEX, &info))) d.stock.index = info.iSysImageIndex;
        it = drives_.emplace(root, d).first;
    }
    const Slot s = it->second.fixed ? own_slot(root, it->second.stock) : it->second.stock;
    return image(s, px);
}

ui::ThumbPtr ShellIcons::stock(int siid, uint32_t px) {
    auto it = stock_.find(siid);
    if (it == stock_.end()) {
        Slot s;
        SHSTOCKICONINFO info{};
        info.cbSize = sizeof(info);
        if (SUCCEEDED(SHGetStockIconInfo(static_cast<SHSTOCKICONID>(siid), SHGSI_SYSICONINDEX, &info))) s.index = info.iSysImageIndex;
        it = stock_.emplace(siid, s).first;
    }
    return image(it->second, px);
}

void ShellIcons::work() {
    CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
    for (;;) {
        std::wstring path;
        {
            std::unique_lock<std::mutex> lock(mu_);
            cv_.wait(lock, [&] { return stop_ || !jobs_.empty(); });
            if (stop_) break;
            path = std::move(jobs_.front());
            jobs_.pop_front();
            ++running_;
        }
        SHFILEINFOW info{};
        Slot s;
        if (SHGetFileInfoW(path.c_str(), 0, &info, sizeof(info),
                           SHGFI_SYSICONINDEX | SHGFI_ICON | SHGFI_SMALLICON | SHGFI_OVERLAYINDEX)) {
            s.index = info.iIcon & 0x00FFFFFF;
            s.overlay = (info.iIcon >> 24) & 0xFF;
        }
        if (info.hIcon) DestroyIcon(info.hIcon);
        bool post = false;
        {
            std::lock_guard<std::mutex> lock(mu_);
            done_.emplace_back(std::move(path), s);
            --running_;
            post = !notified_;
            notified_ = true;
        }
        if (post && window_) PostMessageW(window_, WM_BB_ICONS, 0, 0);
    }
    CoUninitialize();
}

void ShellIcons::drain() {
    std::vector<std::pair<std::wstring, Slot>> done;
    {
        std::lock_guard<std::mutex> lock(mu_);
        done.swap(done_);
        notified_ = false;
    }
    if (done.empty()) return;
    for (auto& [path, slot] : done) {
        asked_.erase(path);
        if (const auto it = by_path_.find(path); it != by_path_.end()) {
            it->second->second = slot;
            continue;
        }
        lru_.emplace_front(path, slot);
        by_path_[path] = lru_.begin();
        if (lru_.size() > kPerFileCap) {
            by_path_.erase(lru_.back().first);
            lru_.pop_back();
        }
    }
    if (on_ready) on_ready();
}

size_t ShellIcons::pending() const {
    std::lock_guard<std::mutex> lock(mu_);
    return jobs_.size() + running_ + done_.size();
}

}  // namespace bb::views
