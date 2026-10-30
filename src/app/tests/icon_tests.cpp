#include "fixture.h"
#include "gfx/icons.gen.h"

#include <filesystem>
#include <fstream>
#include <set>
#include <sstream>

using namespace bb;
using namespace bb::ui;

namespace {

// Every icon, large and at the size the UI draws it, with its name underneath.
class IconSheet : public Widget {
public:
    static constexpr float kCellW = 104.0f, kCellH = 86.0f, kMargin = 10.0f;
    static constexpr int kColumns = 10;

    IconSheet() : big_(icons::kCount), small_(icons::kCount), big_gen_(icons::kCount, 0), small_gen_(icons::kCount, 0) {}

    Rect cell(size_t i) const {
        return Rect::xywh(bounds.l + kMargin + static_cast<float>(i % kColumns) * kCellW,
                          bounds.t + kMargin + static_cast<float>(i / kColumns) * kCellH, kCellW, kCellH);
    }
    Rect big_rect(size_t i) const {
        const Rect c = cell(i);
        return Rect::xywh(c.l + 14, c.t + 4, 44, 44);
    }
    static float height_for(size_t n) { return 2 * kMargin + static_cast<float>((n + kColumns - 1) / kColumns) * kCellH; }

    void paint(PaintContext& pc) override {
        pc.fill(bounds, tok::WindowBackground);
        gfx::FontSpec small_font;
        small_font.size = 11.0f;
        for (size_t i = 0; i < icons::kCount; ++i) {
            const Rect c = cell(i);
            Icon::draw(pc, *icons::kAll[i].icon, big_rect(i), tok::TextPrimary, big_[i], big_gen_[i]);
            Icon::draw(pc, *icons::kAll[i].icon, Rect::xywh(c.l + 68, c.t + 18, 16, 16), tok::TextSecondary, small_[i], small_gen_[i]);
            const std::string n = icons::kAll[i].name;
            pc.text(std::wstring(n.begin(), n.end()), Rect{c.l, c.t + 52, c.r, c.t + 70}, small_font, tok::TextMuted,
                    gfx::TextAlign::Center);
        }
    }

private:
    std::vector<ComPtr<ID2D1PathGeometry>> big_, small_;
    std::vector<uint32_t> big_gen_, small_gen_;
};

}  // namespace

TEST(icon_data_is_well_formed) {
    CHECK(icons::kCount >= 80);
    std::set<std::string> names;
    for (const icons::NamedIcon& n : icons::kAll) {
        CHECK(names.insert(n.name).second);  // unique
        const IconData& d = *n.icon;
        CHECK(d.viewbox == 16 || d.viewbox == 20 || d.viewbox == 24 || d.viewbox == 28 || d.viewbox == 32 || d.viewbox == 48);
        CHECK(d.count > 6);
        // Walk the stream: every op is known and the lengths add up exactly; the first op moves.
        size_t i = 0, figures = 0, closes = 0;
        bool bad = false;
        while (i < d.count) {
            const int op = static_cast<int>(d.cmds[i]);
            size_t len = 0;
            switch (op) {
                case IconMove: len = 3; ++figures; break;
                case IconLine: len = 3; break;
                case IconCubic: len = 7; break;
                case IconClose: len = 1; ++closes; break;
                default: bad = true; break;
            }
            if (bad || i + len > d.count) {
                bad = true;
                break;
            }
            if (i == 0 && op != IconMove) bad = true;
            for (size_t k = 1; k < len; ++k) {
                if (d.cmds[i + k] < -2.0f || d.cmds[i + k] > d.viewbox + 2.0f) bad = true;
            }
            i += len;
        }
        if (bad) std::printf("  icon %s is malformed\n", n.name);
        CHECK(!bad);
        CHECK(figures > 0 && figures == closes);  // every figure is closed
        CHECK(icons::find(n.name) == n.icon);
    }
    CHECK(icons::find("NoSuchIcon") == nullptr);
    CHECK(icons::find("Back") == &icons::Back);
}

// The manifest is the source of truth for the names: the generated header must list exactly its entries, in order.
TEST(icon_names_match_the_manifest) {
    // __FILE__ is <repo>/src/app/tests/icon_tests.cpp.
    const std::filesystem::path file = __FILE__;
    if (!file.has_parent_path()) return;
    std::ifstream in(file.parent_path().parent_path().parent_path().parent_path() / "tools" / "icons" / "icons.txt");
    if (!in) {
        std::printf("  (icons.txt not found next to the sources; skipped)\n");
        return;
    }
    std::vector<std::string> names;
    for (std::string line; std::getline(in, line);) {
        const size_t s = line.find_first_not_of(" \t");
        if (s == std::string::npos || line[s] == '#') continue;
        std::string name = line.substr(s, line.find('|') - s);
        name.erase(name.find_last_not_of(" \t") + 1);
        names.push_back(name);
    }
    CHECK_EQ(names.size(), icons::kCount);
    for (size_t i = 0; i < names.size() && i < icons::kCount; ++i) CHECK(names[i] == icons::kAll[i].name);
}

TEST(icon_sheet_renders_every_icon) {
    ThemeScope theme;
    Rig rig(1060, static_cast<int>(IconSheet::height_for(icons::kCount)));
    auto* sheet = rig.set(std::make_unique<IconSheet>());
    rig.render();
    auto& bmp = rig.bitmap();
    std::vector<uint8_t> px;
    CHECK(bmp.read_pixels(px));
    const UINT stride = bmp.width_px() * 4;
    const uint32_t bg = Env::get().res->palette.argb(tok::WindowBackground) & 0xFFFFFF;
    int blank = 0;
    for (size_t i = 0; i < icons::kCount; ++i) {
        const Rect r = sheet->big_rect(i);
        size_t ink = 0;
        for (UINT y = static_cast<UINT>(r.t); y < static_cast<UINT>(r.b); ++y) {
            for (UINT x = static_cast<UINT>(r.l); x < static_cast<UINT>(r.r); ++x) {
                const uint8_t* p = &px[y * stride + x * 4];
                const uint32_t rgb = (static_cast<uint32_t>(p[2]) << 16) | (static_cast<uint32_t>(p[1]) << 8) | p[0];
                if (rgb != bg) ++ink;
            }
        }
        if (ink < 40) {
            ++blank;
            std::printf("  icon %s drew %zu pixels\n", icons::kAll[i].name, ink);
        }
        // An icon is a picture, not a smudge or a filled square: it covers some of its box, not all of it.
        CHECK(ink < static_cast<size_t>(r.width() * r.height() * 0.85f));
    }
    CHECK_EQ(blank, 0);
    CHECK(bmp.write_png(exe_dir() + L"icon_sheet.png"));
}
