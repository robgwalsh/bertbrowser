// A font specimen, drawn with the face in the file itself: the alphabet at a reading size, then a pangram at
// four sizes. The file is added to a private font collection for the specimen only; nothing is installed.
#pragma once
#include "ui/core/scroll.h"
#include "ui/core/widget.h"

#include <string>

namespace bb::views {

class FontView : public ui::Widget {
public:
    FontView() {
        style.bg = ui::tok::SurfaceBackground;
        flags.clip_children = true;
    }
    // An empty path clears it. False when the system will not load the face.
    bool set_font(const std::wstring& path);
    bool loaded() const { return !path_.empty() && failed_ == false; }

    ui::Size measure(ui::Size) override { return fixed_or({0, 0}); }
    void arrange(ui::Rect r) override;
    void paint(ui::PaintContext& pc) override;
    bool on_mouse_down(const ui::MouseEvent& e) override;
    bool on_mouse_move(const ui::MouseEvent& e) override;
    bool on_mouse_up(const ui::MouseEvent& e) override;
    bool on_wheel(ui::Point p, int delta, ui::Mods m) override;

private:
    struct Block {
        std::wstring text;
        float size;
    };
    void build();  // collection + a layout per block, for the current width
    float layout_height() const;

    std::wstring path_;
    bool failed_ = false;
    bool built_ = false;
    float built_width_ = -1;
    std::wstring family_;
    Microsoft::WRL::ComPtr<IDWriteFontCollection> collection_;
    std::vector<Microsoft::WRL::ComPtr<IDWriteTextLayout>> layouts_;
    std::vector<float> tops_;
    float total_ = 0;
    ui::ScrollBar v_{true};
    bool bar_drag_ = false;
};

}  // namespace bb::views
