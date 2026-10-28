// DirectWrite caches: one IDWriteTextFormat per (family, size, weight), and an LRU of IDWriteTextLayouts
// for the places that draw the same string over and over (file list cells, keyed by row id, column and
// width). Fonts are Segoe UI and Consolas.
#pragma once
#include "gfx/device.h"

#include <list>
#include <map>
#include <string>
#include <string_view>
#include <tuple>
#include <unordered_map>

namespace bb::gfx {

enum class Family : uint8_t { Ui, Mono };

struct FontSpec {
    Family family = Family::Ui;
    float size = 14.0f;  // DIPs
    int weight = 400;
    bool operator==(const FontSpec&) const = default;
};

enum class TextAlign : uint8_t { Leading, Center, Trailing };

struct LayoutOptions {
    float max_width = 1.0e6f;
    float max_height = 1.0e6f;
    bool wrap = false;
    bool ellipsis = false;  // trim with an ellipsis at the max width (only meaningful without wrap)
    bool vcenter = false;   // centre vertically in max_height
    TextAlign align = TextAlign::Leading;
};

class TextCache {
public:
    explicit TextCache(IDWriteFactory* dwrite) : dwrite_(dwrite) {}

    IDWriteTextFormat* format(const FontSpec& font);
    ComPtr<IDWriteTextLayout> make_layout(std::wstring_view text, const FontSpec& font, const LayoutOptions& opt);
    // Width and height of `text` on one line.
    D2D1_SIZE_F measure(std::wstring_view text, const FontSpec& font);
    size_t formats_made() const { return made_; }
    size_t format_count() const { return formats_.size(); }
    IDWriteFactory* dwrite() const { return dwrite_; }

private:
    IDWriteFactory* dwrite_;
    size_t made_ = 0;
    std::map<std::tuple<int, int, int>, ComPtr<IDWriteTextFormat>> formats_;  // family, size*100, weight
};

struct LayoutKey {
    uint64_t id = 0;
    uint32_t col = 0;
    float width = 0;
    bool operator==(const LayoutKey&) const = default;
};

struct LayoutKeyHash {
    size_t operator()(const LayoutKey& k) const {
        size_t h = std::hash<uint64_t>()(k.id);
        h = h * 1099511628211ull ^ std::hash<uint32_t>()(k.col);
        h = h * 1099511628211ull ^ std::hash<float>()(k.width);
        return h;
    }
};

class LayoutLru {
public:
    LayoutLru(TextCache& text, size_t capacity) : text_(text), capacity_(capacity) {}

    // The layout for `key`; rebuilt if the text, font or options changed since it was cached.
    IDWriteTextLayout* get(const LayoutKey& key, std::wstring_view text, const FontSpec& font, const LayoutOptions& opt);

    size_t size() const { return map_.size(); }
    size_t hits() const { return hits_; }
    size_t misses() const { return misses_; }
    void clear();

private:
    struct Entry {
        LayoutKey key;
        std::wstring text;
        FontSpec font;
        TextAlign align;
        ComPtr<IDWriteTextLayout> layout;
    };
    TextCache& text_;
    size_t capacity_;
    size_t hits_ = 0, misses_ = 0;
    std::list<Entry> lru_;  // front = most recent
    std::unordered_map<LayoutKey, std::list<Entry>::iterator, LayoutKeyHash> map_;
};

}  // namespace bb::gfx
