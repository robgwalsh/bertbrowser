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

// The pictures the shell keeps, one list per size: 16, 32, 48 and 256 at 96 DPI, scaled by the system DPI. Each is
// asked of the shell the first time something needs it - a list of 16-pixel rows never touches the jumbo one, and
// getting all four up front cost tens of milliseconds in the first listing and in startup.
struct ImageLists {
    struct List {
        ComPtr<IImageList> list;
        int size = 0;
        bool loaded = false;
    };
    static constexpr int kSizes[4] = {SHIL_SMALL, SHIL_LARGE, SHIL_EXTRALARGE, SHIL_JUMBO};
    List lists_[4];

    // Smallest (0) to jumbo (3). A list the shell would not give has a null `list`.
    const List& at(int i) {
        List& l = lists_[i];
        if (!l.loaded) {
            l.loaded = true;
            if (SUCCEEDED(SHGetImageList(kSizes[i], IID_PPV_ARGS(&l.list))) && l.list) {
                int cx = 0, cy = 0;
                l.list->GetIconSize(&cx, &cy);
                l.size = cx;
            }
        }
        return l;
    }
};

// Per thread: an IImageList belongs to the apartment that asked for it, and the workers render too.
ImageLists& lists() {
    static thread_local ImageLists l;
    return l;
}

ComPtr<IWICImagingFactory>& wic_slot() {
    static thread_local ComPtr<IWICImagingFactory> f;
    return f;
}

IWICImagingFactory* wic() {
    ComPtr<IWICImagingFactory>& f = wic_slot();
    if (!f) CoCreateInstance(CLSID_WICImagingFactory, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&f));
    return f.Get();
}

// A worker lets go of its COM objects before it uninitialises COM.
void release_thread_state() {
    wic_slot().Reset();
    for (auto& l : lists().lists_) l = {};
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

uint64_t image_key(int index, int overlay, uint32_t px) {
    return (static_cast<uint64_t>(index) << 24) | (static_cast<uint64_t>(overlay & 0xFF) << 16) | (px & 0xFFFF);
}

// The picture of one system image list entry, `px` square, from this thread's lists. The smallest list at least
// as big, so a picture is only ever scaled down - except a jumbo one that turns out to be a small picture in a big
// frame, where the 48 scaled up is the better answer.
ui::ThumbPtr render_for(int index, int overlay, uint32_t px) {
    ImageLists& l = lists();
    const ImageLists::List* pick = nullptr;
    for (int i = 0; i < 4 && !pick; ++i) {
        const ImageLists::List& c = l.at(i);
        if (c.list && c.size >= static_cast<int>(px)) pick = &c;
    }
    if (!pick) {
        for (int i = 3; i >= 0 && !pick; --i) {
            if (l.at(i).list) pick = &l.at(i);
        }
    }
    if (!pick) pick = &l.at(0);
    ui::ThumbPtr img = render(pick->list.Get(), index, overlay, px);
    if (img && pick == &l.lists_[3]) {
        const ImageLists::List& extra = l.at(2);
        if (extra.list) {
            const ui::ThumbPtr full = render(pick->list.Get(), index, overlay, static_cast<uint32_t>(pick->size));
            if (full && drawn_extent(*full) <= static_cast<uint32_t>(extra.size)) {
                img = render(extra.list.Get(), index, overlay, px);
            }
        }
    }
    return img;
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

// A type lookup is a registry walk (about 2 ms) and its picture a render (about 1 ms): a folder of a dozen kinds
// of file would hold the UI thread for most of a frame. So the first sight of a type asks a worker, the row draws
// its glyph meanwhile, and the answer comes back with the picture already made.
void ShellIcons::ask_type(const std::wstring& path, bool directory, uint32_t px) {
    const std::wstring ext = directory ? L"<dir>" : lower_ext(path);
    const std::wstring key = ext.empty() ? L"<none>" : ext;
    if (!types_asked_.insert(key).second) return;
    TypeJob job;
    job.key = key;
    job.query = directory ? L"folder" : ext.empty() ? L"file" : ext;
    job.directory = directory;
    job.shortcut = is_shortcut(ext);
    job.px = px;
    std::lock_guard<std::mutex> lock(mu_);
    type_jobs_.push_back(std::move(job));
    cv_.notify_one();
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
    const uint64_t key = image_key(s.index, s.overlay, px);
    if (const auto it = images_.find(key); it != images_.end()) return it->second;
    ui::ThumbPtr img = render_for(s.index, s.overlay, px);
    images_[key] = img;
    return img;
}

ui::ThumbPtr ShellIcons::file(const std::wstring& path, bool directory, uint32_t attributes, uint32_t px, bool real) {
    const std::wstring ext = directory ? L"<dir>" : lower_ext(path);
    const auto known = by_type_.find(ext.empty() ? std::wstring(L"<none>") : ext);
    if (known == by_type_.end()) {
        ask_type(path, directory, px);
        return nullptr;  // the caller draws its own glyph until the worker answers
    }
    Slot s = known->second;
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

namespace {

// The shell sets its icon cache, the system image list and its overlays up the first time anything asks, and
// that cost lands on whoever asks first - the UI thread, in the middle of the first listing. One worker pays it
// at startup instead (the answers are process-wide; this thread keeps none of them).
void warm_shell_icons() {
    SHFILEINFOW info{};
    for (const wchar_t* query : {L"file", L"folder"}) {
        const bool dir = query[1] == L'o';
        SHGetFileInfoW(query, dir ? FILE_ATTRIBUTE_DIRECTORY : FILE_ATTRIBUTE_NORMAL, &info, sizeof(info),
                       SHGFI_SYSICONINDEX | SHGFI_USEFILEATTRIBUTES);
    }
    if (SHGetFileInfoW(L".lnk", FILE_ATTRIBUTE_NORMAL, &info, sizeof(info),
                       SHGFI_SYSICONINDEX | SHGFI_USEFILEATTRIBUTES | SHGFI_ICON | SHGFI_SMALLICON | SHGFI_OVERLAYINDEX) &&
        info.hIcon) {
        DestroyIcon(info.hIcon);
    }
    ComPtr<IImageList> small_list;
    SHGetImageList(SHIL_SMALL, IID_PPV_ARGS(&small_list));
    ComPtr<IWICImagingFactory> factory;
    CoCreateInstance(CLSID_WICImagingFactory, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&factory));
}

}  // namespace

void ShellIcons::work() {
    CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
    if (!warmed_.exchange(true)) warm_shell_icons();
    // Tells the UI thread there is something to drain. Every drain repaints the window, so a burst of answers is
    // held back until the queues are empty - or, in a long run, for at most kHoldBack, so pictures still arrive
    // while it goes on.
    const auto announce = [this] {
        bool post = false;
        {
            std::lock_guard<std::mutex> lock(mu_);
            --running_;
            const auto now = std::chrono::steady_clock::now();
            const bool idle = jobs_.empty() && type_jobs_.empty() && running_ == 0;
            if (!notified_ && (idle || now - last_post_ >= kHoldBack)) {
                post = true;
                notified_ = true;
                last_post_ = now;
            }
        }
        if (post && window_) PostMessageW(window_, WM_BB_ICONS, 0, 0);
    };
    for (;;) {
        TypeJob type;
        std::wstring path;
        bool is_type = false;
        {
            std::unique_lock<std::mutex> lock(mu_);
            cv_.wait(lock, [&] { return stop_ || !type_jobs_.empty() || !jobs_.empty(); });
            if (stop_) break;
            // A type is what a row on screen is waiting for; a file's own picture can follow it.
            if (!type_jobs_.empty()) {
                type = std::move(type_jobs_.front());
                type_jobs_.pop_front();
                is_type = true;
            } else {
                path = std::move(jobs_.front());
                jobs_.pop_front();
            }
            ++running_;
        }
        SHFILEINFOW info{};
        Slot s;
        if (is_type) {
            UINT flags = SHGFI_SYSICONINDEX | SHGFI_USEFILEATTRIBUTES;
            if (type.shortcut) flags |= SHGFI_ICON | SHGFI_SMALLICON | SHGFI_OVERLAYINDEX;  // the arrow
            if (SHGetFileInfoW(type.query.c_str(), type.directory ? FILE_ATTRIBUTE_DIRECTORY : FILE_ATTRIBUTE_NORMAL, &info,
                               sizeof(info), flags)) {
                s.index = info.iIcon & 0x00FFFFFF;
                s.overlay = (flags & SHGFI_OVERLAYINDEX) ? (info.iIcon >> 24) & 0xFF : 0;
            }
            if (info.hIcon) DestroyIcon(info.hIcon);
            TypeDone done;
            done.key = std::move(type.key);
            done.slot = s;
            done.px = type.px;
            if (s.index >= 0 && type.px) done.image = render_for(s.index, s.overlay, type.px);
            {
                std::lock_guard<std::mutex> lock(mu_);
                type_done_.push_back(std::move(done));
            }
            announce();
            continue;
        }
        if (SHGetFileInfoW(path.c_str(), 0, &info, sizeof(info),
                           SHGFI_SYSICONINDEX | SHGFI_ICON | SHGFI_SMALLICON | SHGFI_OVERLAYINDEX)) {
            s.index = info.iIcon & 0x00FFFFFF;
            s.overlay = (info.iIcon >> 24) & 0xFF;
        }
        if (info.hIcon) DestroyIcon(info.hIcon);
        {
            std::lock_guard<std::mutex> lock(mu_);
            done_.emplace_back(std::move(path), s);
        }
        announce();
    }
    release_thread_state();
    CoUninitialize();
}

void ShellIcons::drain() {
    std::vector<std::pair<std::wstring, Slot>> done;
    std::vector<TypeDone> types;
    {
        std::lock_guard<std::mutex> lock(mu_);
        done.swap(done_);
        types.swap(type_done_);
        notified_ = false;
    }
    if (done.empty() && types.empty()) return;
    for (TypeDone& t : types) {
        by_type_[t.key] = t.slot;
        types_asked_.erase(t.key);
        if (t.slot.index >= 0 && t.px) images_.emplace(image_key(t.slot.index, t.slot.overlay, t.px), std::move(t.image));
    }
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
    return jobs_.size() + type_jobs_.size() + running_ + done_.size() + type_done_.size();
}

}  // namespace bb::views
