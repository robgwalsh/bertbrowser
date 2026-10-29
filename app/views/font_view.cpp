#include "views/font_view.h"

#include "views/pane_images.h"

#include <windows.h>

#include <dwrite_3.h>

#include <algorithm>

namespace bb::views {

using namespace ui;
using Microsoft::WRL::ComPtr;

bool FontView::set_font(const std::wstring& path) {
    path_ = path;
    built_ = false;
    failed_ = false;
    collection_.Reset();
    layouts_.clear();
    v_.set_offset(0);
    if (path.empty()) {
        invalidate();
        return true;
    }
    const FontInfo info = read_font_info(path);
    family_ = info.family;
    failed_ = !info.ok;
    invalidate();
    return info.ok;
}

void FontView::arrange(Rect r) {
    bounds = r;
    built_ = false;
}

void FontView::build() {
    if (built_ && built_width_ == bounds.width()) return;
    built_ = true;
    built_width_ = bounds.width();
    layouts_.clear();
    tops_.clear();
    total_ = 0;
    if (path_.empty() || failed_ || !root()) return;

    ComPtr<IDWriteFactory5> dw;
    if (FAILED(res().dev.dwrite()->QueryInterface(IID_PPV_ARGS(&dw)))) {
        failed_ = true;
        return;
    }
    if (!collection_) {
        ComPtr<IDWriteFontSetBuilder1> builder;
        ComPtr<IDWriteFontFile> file;
        ComPtr<IDWriteFontSet> set;
        ComPtr<IDWriteFontCollection1> collection;
        if (FAILED(dw->CreateFontSetBuilder(&builder)) || FAILED(dw->CreateFontFileReference(path_.c_str(), nullptr, &file)) ||
            FAILED(builder->AddFontFile(file.Get())) || FAILED(builder->CreateFontSet(&set)) ||
            FAILED(dw->CreateFontCollectionFromFontSet(set.Get(), &collection))) {
            failed_ = true;
            return;
        }
        collection_ = collection;
    }
    static const Block kBlocks[] = {
        {L"ABCDEFGHIJKLM\nNOPQRSTUVWXYZ\nabcdefghijklmnopqrstuvwxyz\n0123456789  &@$£€ .,;:!?", 22.0f},
        {L"Sphinx of black quartz, judge my vow.", 12.0f},
        {L"Sphinx of black quartz, judge my vow.", 18.0f},
        {L"Sphinx of black quartz, judge my vow.", 24.0f},
        {L"Sphinx of black quartz", 36.0f},
    };
    const float avail = std::max(40.0f, bounds.width() - ScrollBar::kThickness - 20.0f);
    float y = 10.0f;
    for (const Block& b : kBlocks) {
        ComPtr<IDWriteTextFormat> format;
        if (FAILED(res().dev.dwrite()->CreateTextFormat(family_.c_str(), collection_.Get(), DWRITE_FONT_WEIGHT_NORMAL,
                                                         DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, b.size, L"en-us", &format))) {
            continue;
        }
        ComPtr<IDWriteTextLayout> layout;
        if (FAILED(res().dev.dwrite()->CreateTextLayout(b.text.c_str(), static_cast<UINT32>(b.text.size()), format.Get(), avail, 1.0e6f, &layout))) continue;
        DWRITE_TEXT_METRICS m{};
        layout->GetMetrics(&m);
        layouts_.push_back(layout);
        tops_.push_back(y);
        y += m.height + 14.0f;
    }
    total_ = y;
    v_.extent = total_;
}

void FontView::paint(PaintContext& pc) {
    Widget::paint(pc);
    build();
    v_.viewport = bounds.height();
    v_.extent = total_;
    v_.area = v_.needed() ? Rect{bounds.r - ScrollBar::kThickness, bounds.t, bounds.r, bounds.b} : Rect{};
    v_.set_offset(v_.offset());
    if (failed_) {
        pc.text(L"This font could not be read.", bounds, gfx::FontSpec{}, tok::TextMuted, gfx::TextAlign::Center);
        return;
    }
    pc.clip(bounds);
    for (size_t i = 0; i < layouts_.size(); ++i) {
        pc.dc->DrawTextLayout(D2D1::Point2F(bounds.l + 10.0f, bounds.t + tops_[i] - v_.offset()), layouts_[i].Get(),
                              pc.brush(tok::TextPrimary), D2D1_DRAW_TEXT_OPTIONS_NONE);
        if (i == 0 && layouts_.size() > 1) {
            const float rule = bounds.t + tops_[1] - 7.0f - v_.offset();
            pc.fill(Rect{bounds.l + 10.0f, rule, bounds.r - 10.0f, rule + 1.0f}, tok::BorderSubtle);
        }
    }
    pc.unclip();
    v_.paint(pc);
}

bool FontView::on_mouse_down(const MouseEvent& e) {
    if (e.button == MouseButton::Left && v_.mouse_down(e.pt)) {
        bar_drag_ = true;
        root()->capture(this);
        invalidate();
        return true;
    }
    return false;
}

bool FontView::on_mouse_move(const MouseEvent& e) {
    if (!bar_drag_) return false;
    v_.mouse_move(e.pt);
    invalidate();
    return true;
}

bool FontView::on_mouse_up(const MouseEvent&) {
    if (!bar_drag_) return false;
    bar_drag_ = false;
    v_.mouse_up();
    root()->release_capture();
    return true;
}

bool FontView::on_wheel(Point, int delta, Mods) {
    if (!v_.needed()) return false;
    v_.set_offset(v_.offset() - static_cast<float>(delta) / 120.0f * 48.0f);
    invalidate();
    return true;
}

}  // namespace bb::views
