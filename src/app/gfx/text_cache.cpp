#include "gfx/text_cache.h"

#include <cmath>

namespace bb::gfx {

IDWriteTextFormat* TextCache::format(const FontSpec& font) {
    const auto key = std::make_tuple(static_cast<int>(font.family), static_cast<int>(std::lround(font.size * 100.0f)),
                                     font.weight);
    auto it = formats_.find(key);
    if (it != formats_.end()) {
        return it->second.Get();
    }
    ComPtr<IDWriteTextFormat> f;
    const wchar_t* name = font.family == Family::Mono ? L"Consolas" : L"Segoe UI";
    const HRESULT hr = dwrite_->CreateTextFormat(name, nullptr, static_cast<DWRITE_FONT_WEIGHT>(font.weight),
                                                 DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, font.size,
                                                 L"en-us", &f);
    if (FAILED(hr)) {
        return nullptr;
    }
    ++made_;
    return formats_.emplace(key, std::move(f)).first->second.Get();
}

ComPtr<IDWriteTextLayout> TextCache::make_layout(std::wstring_view text, const FontSpec& font,
                                                 const LayoutOptions& opt) {
    ComPtr<IDWriteTextLayout> layout;
    IDWriteTextFormat* fmt = format(font);
    if (!fmt) {
        return layout;
    }
    dwrite_->CreateTextLayout(text.data(), static_cast<UINT32>(text.size()), fmt, opt.max_width, opt.max_height,
                              &layout);
    if (!layout) {
        return layout;
    }
    layout->SetWordWrapping(opt.wrap ? DWRITE_WORD_WRAPPING_WRAP : DWRITE_WORD_WRAPPING_NO_WRAP);
    layout->SetTextAlignment(opt.align == TextAlign::Trailing  ? DWRITE_TEXT_ALIGNMENT_TRAILING
                             : opt.align == TextAlign::Center ? DWRITE_TEXT_ALIGNMENT_CENTER
                                                              : DWRITE_TEXT_ALIGNMENT_LEADING);
    if (opt.vcenter) {
        layout->SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
    }
    // Wrapped text trims too: the last line that fits the height ends in the ellipsis (a two-line caption).
    if (opt.ellipsis) {
        ComPtr<IDWriteInlineObject> sign;
        if (SUCCEEDED(dwrite_->CreateEllipsisTrimmingSign(layout.Get(), &sign))) {
            const DWRITE_TRIMMING trimming{DWRITE_TRIMMING_GRANULARITY_CHARACTER, 0, 0};
            layout->SetTrimming(&trimming, sign.Get());
        }
    }
    return layout;
}

D2D1_SIZE_F TextCache::measure(std::wstring_view text, const FontSpec& font) {
    LayoutOptions opt;
    ComPtr<IDWriteTextLayout> l = make_layout(text, font, opt);
    DWRITE_TEXT_METRICS m{};
    if (l) {
        l->GetMetrics(&m);
    }
    return D2D1::SizeF(m.widthIncludingTrailingWhitespace, m.height);
}

IDWriteTextLayout* LayoutLru::get(const LayoutKey& key, std::wstring_view text, const FontSpec& font,
                                  const LayoutOptions& opt) {
    auto it = map_.find(key);
    if (it != map_.end()) {
        Entry& e = *it->second;
        if (e.font == font && e.align == opt.align && e.text == text) {
            ++hits_;
            lru_.splice(lru_.begin(), lru_, it->second);
            return e.layout.Get();
        }
        lru_.erase(it->second);
        map_.erase(it);
    }
    ++misses_;
    Entry e;
    e.key = key;
    e.text = std::wstring(text);
    e.font = font;
    e.align = opt.align;
    e.layout = text_.make_layout(text, font, opt);
    lru_.push_front(std::move(e));
    map_[key] = lru_.begin();
    while (map_.size() > capacity_) {
        map_.erase(lru_.back().key);
        lru_.pop_back();
    }
    return lru_.front().layout.Get();
}

void LayoutLru::clear() {
    map_.clear();
    lru_.clear();
}

}  // namespace bb::gfx
