#include "views/shell/thumbs.h"

#include <objbase.h>
#include <shobjidl.h>
#include <shlobj.h>

#include <algorithm>
#include <cmath>
#include <functional>

namespace bb::views {

namespace {

uint32_t hash_name(const std::wstring& s) {
    uint32_t h = 2166136261u;
    for (wchar_t c : s) h = (h ^ static_cast<uint32_t>(c)) * 16777619u;
    return h;
}

// h in [0,360), s and v in [0,1] -> BGR bytes.
void hsv_to_bgr(float h, float s, float v, uint8_t* b, uint8_t* g, uint8_t* r) {
    const float c = v * s, x = c * (1 - std::fabs(std::fmod(h / 60.0f, 2.0f) - 1)), m = v - c;
    float rr = 0, gg = 0, bb = 0;
    if (h < 60) { rr = c; gg = x; }
    else if (h < 120) { rr = x; gg = c; }
    else if (h < 180) { gg = c; bb = x; }
    else if (h < 240) { gg = x; bb = c; }
    else if (h < 300) { rr = x; bb = c; }
    else { rr = c; bb = x; }
    *r = static_cast<uint8_t>((rr + m) * 255);
    *g = static_cast<uint8_t>((gg + m) * 255);
    *b = static_cast<uint8_t>((bb + m) * 255);
}

}  // namespace

ThumbnailService::ThumbnailService(HWND ui_window, ThumbSource source, std::function<std::wstring(size_t)> row_path)
    : window_(ui_window), source_(source), row_path_(std::move(row_path)) {
    const size_t n = source == ThumbSource::Shell ? kSlots : 1;
    for (size_t i = 0; i < n; ++i) threads_.emplace_back([this] { work(); });
}

ThumbnailService::~ThumbnailService() {
    {
        std::lock_guard<std::mutex> lock(mu_);
        stop_ = true;
        jobs_.clear();
    }
    cv_.notify_all();
    for (auto& t : threads_) {
        if (t.joinable()) t.join();
    }
}

void ThumbnailService::request(uint64_t row_id, size_t row, uint32_t px, ui::ThumbnailSink& sink) {
    Job job{row_id, row_path_ ? row_path_(row) : std::wstring(), px, &sink};
    {
        std::lock_guard<std::mutex> lock(mu_);
        cancelled_.erase(std::remove(cancelled_.begin(), cancelled_.end(), row_id), cancelled_.end());
        jobs_.push_back(std::move(job));
    }
    cv_.notify_one();
}

void ThumbnailService::cancel(uint64_t row_id) {
    std::lock_guard<std::mutex> lock(mu_);
    for (auto it = jobs_.begin(); it != jobs_.end(); ++it) {
        if (it->row_id == row_id) {
            jobs_.erase(it);
            return;
        }
    }
    // Already running or finished: the answer is dropped on its way in.
    cancelled_.push_back(row_id);
}

size_t ThumbnailService::pending() const {
    std::lock_guard<std::mutex> lock(mu_);
    return jobs_.size() + running_.load() + done_.size();
}

void ThumbnailService::drain() {
    std::deque<Done> batch;
    {
        std::lock_guard<std::mutex> lock(mu_);
        batch.swap(done_);
        notified_ = false;
    }
    for (Done& d : batch) {
        bool dropped = false;
        {
            std::lock_guard<std::mutex> lock(mu_);
            const auto it = std::find(cancelled_.begin(), cancelled_.end(), d.row_id);
            if (it != cancelled_.end()) {
                cancelled_.erase(it);
                dropped = true;
            }
        }
        if (!dropped) d.sink->deliver(d.row_id, std::move(d.image));
    }
}

void ThumbnailService::work() {
    const bool shell = source_ == ThumbSource::Shell;
    if (shell) CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
    for (;;) {
        Job job;
        {
            std::unique_lock<std::mutex> lock(mu_);
            cv_.wait(lock, [this] { return stop_ || !jobs_.empty(); });
            if (stop_) break;
            job = std::move(jobs_.front());
            jobs_.pop_front();
            ++running_;
        }
        ui::ThumbPtr image = shell ? shell_image(job.path, job.px) : generate(job.path, job.px);
        bool post = false;
        {
            std::lock_guard<std::mutex> lock(mu_);
            done_.push_back({job.row_id, job.sink, std::move(image)});
            --running_;
            post = !notified_;
            notified_ = true;
        }
        if (post && window_) PostMessageW(window_, WM_BB_THUMBS, 0, 0);
    }
    if (shell) CoUninitialize();
}

ui::ThumbPtr ThumbnailService::shell_image(const std::wstring& path, uint32_t px) {
    if (path.empty()) return nullptr;
    IShellItemImageFactory* factory = nullptr;
    if (FAILED(SHCreateItemFromParsingName(path.c_str(), nullptr, IID_PPV_ARGS(&factory))) || !factory) return nullptr;
    HBITMAP hbm = nullptr;
    const HRESULT hr = factory->GetImage(SIZE{static_cast<LONG>(px), static_cast<LONG>(px)}, SIIGBF_BIGGERSIZEOK, &hbm);
    factory->Release();
    if (FAILED(hr) || !hbm) return nullptr;
    BITMAP bm{};
    GetObjectW(hbm, sizeof(bm), &bm);
    auto img = std::make_shared<ui::ThumbImage>();
    if (bm.bmWidth > 0 && bm.bmHeight > 0) {
        img->width = static_cast<uint32_t>(bm.bmWidth);
        img->height = static_cast<uint32_t>(bm.bmHeight);
        img->bgra.assign(static_cast<size_t>(img->width) * img->height * 4, 0);
        BITMAPINFO bi{};
        bi.bmiHeader.biSize = sizeof(bi.bmiHeader);
        bi.bmiHeader.biWidth = bm.bmWidth;
        bi.bmiHeader.biHeight = -bm.bmHeight;  // top-down
        bi.bmiHeader.biPlanes = 1;
        bi.bmiHeader.biBitCount = 32;
        bi.bmiHeader.biCompression = BI_RGB;
        HDC dc = GetDC(nullptr);
        GetDIBits(dc, hbm, 0, static_cast<UINT>(bm.bmHeight), img->bgra.data(), &bi, DIB_RGB_COLORS);
        ReleaseDC(nullptr, dc);
        // A bitmap with no alpha anywhere is opaque.
        bool any_alpha = false;
        for (size_t i = 3; i < img->bgra.size(); i += 4) {
            if (img->bgra[i]) {
                any_alpha = true;
                break;
            }
        }
        if (!any_alpha) {
            for (size_t i = 3; i < img->bgra.size(); i += 4) img->bgra[i] = 255;
        }
    }
    DeleteObject(hbm);
    return img->width ? img : nullptr;
}

ui::ThumbPtr ThumbnailService::generate(const std::wstring& name, uint32_t px) {
    auto img = std::make_shared<ui::ThumbImage>();
    img->width = img->height = std::max<uint32_t>(8, px);
    const uint32_t n = img->width;
    img->bgra.resize(static_cast<size_t>(n) * n * 4);
    const uint32_t h = hash_name(name.substr(name.find_last_of(L"\\/") == std::wstring::npos ? 0 : name.find_last_of(L"\\/") + 1));
    const float hue = static_cast<float>(h % 360);
    const float cx = static_cast<float>(h >> 9 & 63) / 63.0f * 0.4f + 0.3f, cy = static_cast<float>(h >> 15 & 63) / 63.0f * 0.4f + 0.3f;
    for (uint32_t y = 0; y < n; ++y) {
        for (uint32_t x = 0; x < n; ++x) {
            const float fx = static_cast<float>(x) / static_cast<float>(n), fy = static_cast<float>(y) / static_cast<float>(n);
            const float d = std::sqrt((fx - cx) * (fx - cx) + (fy - cy) * (fy - cy));
            uint8_t b, g, r;
            const bool disc = d < 0.22f;
            hsv_to_bgr(std::fmod(hue + (disc ? 40.0f : 0.0f), 360.0f), disc ? 0.45f : 0.7f, disc ? 0.95f : 0.45f + 0.4f * (1 - fy), &b, &g, &r);
            uint8_t* p = &img->bgra[(static_cast<size_t>(y) * n + x) * 4];
            p[0] = b;
            p[1] = g;
            p[2] = r;
            p[3] = 255;
        }
    }
    return img;
}

}  // namespace bb::views
