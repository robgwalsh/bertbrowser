#include "ui/controls/pickers.h"

#include <algorithm>
#include <cmath>

namespace bb::ui {

// ---- calendar maths ---------------------------------------------------------------------------------

namespace calendar {

bool is_leap(int y) { return (y % 4 == 0 && y % 100 != 0) || y % 400 == 0; }

int days_in_month(int y, int m) {
    static const int kDays[] = {31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31};
    return m == 2 && is_leap(y) ? 29 : kDays[std::clamp(m, 1, 12) - 1];
}

int days_from_civil(Date d) {
    int y = d.year - (d.month <= 2 ? 1 : 0);
    const int era = (y >= 0 ? y : y - 399) / 400;
    const int yoe = y - era * 400;
    const int doy = (153 * (d.month + (d.month > 2 ? -3 : 9)) + 2) / 5 + d.day - 1;
    const int doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    return era * 146097 + doe - 719468;
}

Date civil_from_days(int z) {
    z += 719468;
    const int era = (z >= 0 ? z : z - 146096) / 146097;
    const int doe = z - era * 146097;
    const int yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    const int y = yoe + era * 400;
    const int doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    const int mp = (5 * doy + 2) / 153;
    const int d = doy - (153 * mp + 2) / 5 + 1;
    const int m = mp < 10 ? mp + 3 : mp - 9;
    return Date{y + (m <= 2 ? 1 : 0), m, d};
}

int weekday(Date d) { return ((days_from_civil(d) + 4) % 7 + 7) % 7; }  // 1970-01-01 was a Thursday

Date add_days(Date d, int n) { return civil_from_days(days_from_civil(d) + n); }

Date add_months(Date d, int n) {
    const int idx = d.year * 12 + (d.month - 1) + n;
    const int y = idx >= 0 ? idx / 12 : -((-idx + 11) / 12);
    const int m = idx - y * 12 + 1;
    return Date{y, m, std::min(d.day, days_in_month(y, m))};
}

Date cell(int year, int month, int row, int col, int first_dow) {
    const Date first{year, month, 1};
    const int lead = (weekday(first) - first_dow + 7) % 7;
    return civil_from_days(days_from_civil(first) - lead + row * 7 + col);
}

}  // namespace calendar

// ---- Calendar ---------------------------------------------------------------------------------------

namespace {

std::wstring locale_string(LCTYPE type) {
    wchar_t buf[80] = {};
    if (GetLocaleInfoEx(LOCALE_NAME_USER_DEFAULT, type, buf, 80) <= 0) return {};
    return buf;
}

}  // namespace

Calendar::Calendar() {
    flags.focusable = true;
    flags.tab_stop = true;
    SYSTEMTIME st{};
    GetLocalTime(&st);
    today = Date{st.wYear, st.wMonth, st.wDay};
    selected_ = today;
    year_ = today.year;
    month_ = today.month;
    DWORD fdow = 0;  // 0 = Monday .. 6 = Sunday
    if (GetLocaleInfoEx(LOCALE_NAME_USER_DEFAULT, LOCALE_IFIRSTDAYOFWEEK | LOCALE_RETURN_NUMBER,
                        reinterpret_cast<LPWSTR>(&fdow), sizeof(fdow) / sizeof(wchar_t)) > 0) {
        first_day_of_week = static_cast<int>((fdow + 1) % 7);
    }
}

Size Calendar::measure(Size) {
    return fixed_or({2 * kPad + 7 * kCell, 2 * kPad + kHeader + kDays + 6 * kRow});
}

Rect Calendar::prev_rect() const { return Rect::xywh(bounds.l + kPad, bounds.t + kPad, kHeader, kHeader); }
Rect Calendar::next_rect() const { return Rect::xywh(bounds.l + kPad + 7 * kCell - kHeader, bounds.t + kPad, kHeader, kHeader); }

Rect Calendar::cell_rect(int row, int col) const {
    return Rect::xywh(bounds.l + kPad + static_cast<float>(col) * kCell,
                      bounds.t + kPad + kHeader + kDays + static_cast<float>(row) * kRow, kCell, kRow);
}

bool Calendar::cell_at(Point p, int* row, int* col) const {
    const float x = p.x - (bounds.l + kPad), y = p.y - (bounds.t + kPad + kHeader + kDays);
    if (x < 0 || y < 0 || x >= 7 * kCell || y >= 6 * kRow) return false;
    *col = static_cast<int>(x / kCell);
    *row = static_cast<int>(y / kRow);
    return true;
}

void Calendar::show_month(int year, int month) {
    if (year == year_ && month == month_) return;
    year_ = year;
    month_ = month;
    invalidate();
    if (on_month_changed) on_month_changed(year_, month_);
}

void Calendar::set_selected(Date d) {
    selected_ = d;
    year_ = d.year;
    month_ = d.month;
    invalidate();
}

void Calendar::choose(Date d) {
    selected_ = d;
    if (d.year != year_ || d.month != month_) show_month(d.year, d.month);
    invalidate();
    if (on_select) on_select(d);
}

void Calendar::paint(PaintContext& pc) {
    pc.fill(bounds, style.bg != kNoToken ? style.bg : tok::SurfaceBackground);
    const std::wstring title = locale_string(LOCALE_SMONTHNAME1 + static_cast<LCTYPE>(month_ - 1)) + L" " + std::to_wstring(year_);
    pc.text(title, Rect{bounds.l + kPad + kHeader, bounds.t + kPad, bounds.l + kPad + 7 * kCell - kHeader, bounds.t + kPad + kHeader},
            style.font, tok::TextPrimary, gfx::TextAlign::Center);
    {
        const Rect p = prev_rect(), n = next_rect();
        const float py = (p.t + p.b) / 2, ny = py;
        const float px = (p.l + p.r) / 2, nx = (n.l + n.r) / 2;
        pc.line({px + 2, py - 5}, {px - 3, py}, tok::TextSecondary, 1.5f);
        pc.line({px - 3, py}, {px + 2, py + 5}, tok::TextSecondary, 1.5f);
        pc.line({nx - 2, ny - 5}, {nx + 3, ny}, tok::TextSecondary, 1.5f);
        pc.line({nx + 3, ny}, {nx - 2, ny + 5}, tok::TextSecondary, 1.5f);
    }
    for (int c = 0; c < 7; ++c) {
        const int dow = (first_day_of_week + c) % 7;
        std::wstring dname = locale_string(LOCALE_SABBREVDAYNAME1 + static_cast<LCTYPE>((dow + 6) % 7));
        if (dname.size() > 2) dname.resize(2);
        const Rect r = Rect::xywh(bounds.l + kPad + static_cast<float>(c) * kCell, bounds.t + kPad + kHeader, kCell, kDays);
        pc.text(dname, r, style.font, tok::TextMuted, gfx::TextAlign::Center);
    }
    for (int row = 0; row < 6; ++row) {
        for (int col = 0; col < 7; ++col) {
            const Date d = cell_date(row, col);
            const Rect r = cell_rect(row, col).deflate(2, 2, 2, 2);
            const bool inside = d.month == month_;
            const bool sel = d == selected_;
            if (sel) {
                pc.fill(r, tok::AccentBackground);
            } else if (d == today) {
                pc.stroke(r, tok::BorderActive);
            }
            if (sel && focused()) pc.stroke(r.inflate(1), tok::BorderFocus);
            pc.text(std::to_wstring(d.day), r, style.font,
                    sel ? tok::AccentForeground : (inside ? tok::TextPrimary : tok::TextMuted), gfx::TextAlign::Center);
        }
    }
}

bool Calendar::on_mouse_down(const MouseEvent& e) {
    if (e.button != MouseButton::Left) return false;
    if (prev_rect().contains(e.pt)) {
        const Date m = calendar::add_months(Date{year_, month_, 1}, -1);
        show_month(m.year, m.month);
        return true;
    }
    if (next_rect().contains(e.pt)) {
        const Date m = calendar::add_months(Date{year_, month_, 1}, 1);
        show_month(m.year, m.month);
        return true;
    }
    int row = 0, col = 0;
    if (cell_at(e.pt, &row, &col)) {
        choose(cell_date(row, col));
        return true;
    }
    return false;
}

bool Calendar::on_key_down(const KeyEvent& e) {
    Date d = selected_;
    switch (e.vk) {
        case VK_LEFT: d = calendar::add_days(d, -1); break;
        case VK_RIGHT: d = calendar::add_days(d, 1); break;
        case VK_UP: d = calendar::add_days(d, -7); break;
        case VK_DOWN: d = calendar::add_days(d, 7); break;
        case VK_PRIOR: d = calendar::add_months(d, e.mods.ctrl ? -12 : -1); break;
        case VK_NEXT: d = calendar::add_months(d, e.mods.ctrl ? 12 : 1); break;
        case VK_HOME: d.day = 1; break;
        case VK_END: d.day = calendar::days_in_month(d.year, d.month); break;
        case VK_RETURN: break;
        default: return false;
    }
    choose(d);
    return true;
}

// ---- colour maths -----------------------------------------------------------------------------------

Hsv rgb_to_hsv(uint32_t rgb) {
    const float r = static_cast<float>((rgb >> 16) & 0xFF) / 255.0f;
    const float g = static_cast<float>((rgb >> 8) & 0xFF) / 255.0f;
    const float b = static_cast<float>(rgb & 0xFF) / 255.0f;
    const float mx = std::max({r, g, b}), mn = std::min({r, g, b}), d = mx - mn;
    Hsv out;
    out.v = mx;
    out.s = mx <= 0 ? 0.0f : d / mx;
    if (d > 0) {
        float h;
        if (mx == r) {
            h = std::fmod((g - b) / d, 6.0f);
        } else if (mx == g) {
            h = (b - r) / d + 2.0f;
        } else {
            h = (r - g) / d + 4.0f;
        }
        h *= 60.0f;
        out.h = h < 0 ? h + 360.0f : h;
    }
    return out;
}

uint32_t hsv_to_rgb(Hsv c) {
    const float h = std::fmod(std::fmod(c.h, 360.0f) + 360.0f, 360.0f);
    const float s = std::clamp(c.s, 0.0f, 1.0f), v = std::clamp(c.v, 0.0f, 1.0f);
    const float chroma = v * s;
    const float hh = h / 60.0f;
    const float x = chroma * (1.0f - std::fabs(std::fmod(hh, 2.0f) - 1.0f));
    float r = 0, g = 0, b = 0;
    switch (static_cast<int>(hh)) {
        case 0: r = chroma; g = x; break;
        case 1: r = x; g = chroma; break;
        case 2: g = chroma; b = x; break;
        case 3: g = x; b = chroma; break;
        case 4: r = x; b = chroma; break;
        default: r = chroma; b = x; break;
    }
    const float m = v - chroma;
    auto to8 = [&](float f) { return static_cast<uint32_t>(std::lround(std::clamp((f + m) * 255.0f, 0.0f, 255.0f))); };
    return (to8(r) << 16) | (to8(g) << 8) | to8(b);
}

// ---- ColorPicker ------------------------------------------------------------------------------------

void ColorPicker::set_rgb(uint32_t rgb) {
    Hsv c = rgb_to_hsv(rgb);
    if (c.s == 0 || c.v == 0) c.h = hsv_.h;  // a grey has no hue of its own: keep the one on show
    if (c.v == 0) c.s = hsv_.s;
    set_hsv(c);
}

void ColorPicker::set_argb(uint32_t argb) {
    alpha_ = static_cast<uint8_t>(argb >> 24);
    set_rgb(argb & 0xFFFFFF);
}

std::optional<uint32_t> ColorPicker::parse_hex(std::wstring t) {
    if (!t.empty() && t[0] == L'#') t.erase(0, 1);
    if (t.size() != 6 && t.size() != 8) return std::nullopt;
    for (wchar_t ch : t) {
        if (!iswxdigit(ch)) return std::nullopt;
    }
    const uint32_t v = static_cast<uint32_t>(std::wcstoul(t.c_str(), nullptr, 16));
    return t.size() == 6 ? 0xFF000000u | v : v;
}

void ColorPicker::set_hsv(Hsv c) {
    c.s = std::clamp(c.s, 0.0f, 1.0f);
    c.v = std::clamp(c.v, 0.0f, 1.0f);
    c.h = std::clamp(c.h, 0.0f, 359.999f);
    hsv_ = c;
    invalidate();
}

Size ColorPicker::measure(Size) {
    return fixed_or({2 * kPad + kSquareW + kGap + kStripW, 4 * kPad + kSquareH + kAlphaH + 28.0f});
}

Rect ColorPicker::square() const { return Rect::xywh(bounds.l + kPad, bounds.t + kPad, kSquareW, kSquareH); }
Rect ColorPicker::strip() const { return Rect::xywh(bounds.l + kPad + kSquareW + kGap, bounds.t + kPad, kStripW, kSquareH); }
Rect ColorPicker::alpha_strip() const {
    return Rect::xywh(bounds.l + kPad, bounds.t + 2 * kPad + kSquareH, kSquareW + kGap + kStripW, kAlphaH);
}
Rect ColorPicker::swatch() const { return Rect::xywh(bounds.l + kPad, bounds.t + 3 * kPad + kSquareH + kAlphaH, 44, 28); }

std::wstring ColorPicker::hex_text() const {
    wchar_t buf[16];
    if (alpha_ == 255) swprintf_s(buf, L"#%06X", rgb());
    else swprintf_s(buf, L"#%08X", argb());
    return buf;
}

// So a partly transparent colour reads as transparent. Spec: ColorPicker.xaml (Checkerboard).
void ColorPicker::checker(PaintContext& pc, const Rect& r) const {
    constexpr float kCell = 6.0f;
    pc.fill(r, tok::PreviewCheckerLight);
    for (float y = r.t; y < r.b; y += kCell) {
        for (float x = r.l; x < r.r; x += kCell) {
            const int parity = static_cast<int>((x - r.l) / kCell) + static_cast<int>((y - r.t) / kCell);
            if (parity & 1) pc.fill(Rect{x, y, std::min(r.r, x + kCell), std::min(r.b, y + kCell)}, tok::PreviewCheckerDark);
        }
    }
}

void ColorPicker::paint(PaintContext& pc) {
    Widget::paint(pc);
    ID2D1DeviceContext* dc = pc.dc;
    const Rect sq = square(), st = strip();
    auto gradient = [&](const std::vector<D2D1_GRADIENT_STOP>& stops, D2D1_POINT_2F a, D2D1_POINT_2F b) {
        ComPtr<ID2D1GradientStopCollection> coll;
        ComPtr<ID2D1LinearGradientBrush> brush;
        if (SUCCEEDED(dc->CreateGradientStopCollection(stops.data(), static_cast<UINT32>(stops.size()), D2D1_GAMMA_2_2,
                                                       D2D1_EXTEND_MODE_CLAMP, &coll))) {
            dc->CreateLinearGradientBrush(D2D1::LinearGradientBrushProperties(a, b), coll.Get(), &brush);
        }
        return brush;
    };
    // The square: the pure hue, a white-to-clear wash left to right, a clear-to-black wash top to bottom.
    ComPtr<ID2D1SolidColorBrush> hue;
    const uint32_t pure = hsv_to_rgb({hsv_.h, 1, 1});
    dc->CreateSolidColorBrush(D2D1::ColorF(pure), &hue);
    if (hue) dc->FillRectangle(sq.d2d(), hue.Get());
    if (auto w = gradient({{0.0f, D2D1::ColorF(1, 1, 1, 1)}, {1.0f, D2D1::ColorF(1, 1, 1, 0)}}, D2D1::Point2F(sq.l, 0), D2D1::Point2F(sq.r, 0))) {
        dc->FillRectangle(sq.d2d(), w.Get());
    }
    if (auto k = gradient({{0.0f, D2D1::ColorF(0, 0, 0, 0)}, {1.0f, D2D1::ColorF(0, 0, 0, 1)}}, D2D1::Point2F(0, sq.t), D2D1::Point2F(0, sq.b))) {
        dc->FillRectangle(sq.d2d(), k.Get());
    }
    pc.stroke(sq, tok::BorderDefault);
    // The hue strip: red through the spectrum and back.
    std::vector<D2D1_GRADIENT_STOP> spectrum;
    for (int i = 0; i <= 6; ++i) {
        spectrum.push_back({static_cast<float>(i) / 6.0f, D2D1::ColorF(hsv_to_rgb({static_cast<float>(i) * 60.0f, 1, 1}))});
    }
    if (auto sp = gradient(spectrum, D2D1::Point2F(0, st.t), D2D1::Point2F(0, st.b))) dc->FillRectangle(st.d2d(), sp.Get());
    pc.stroke(st, tok::BorderDefault);

    ComPtr<ID2D1SolidColorBrush> white, black;
    dc->CreateSolidColorBrush(D2D1::ColorF(1, 1, 1), &white);
    dc->CreateSolidColorBrush(D2D1::ColorF(0, 0, 0), &black);
    if (white && black) {
        const float mx = sq.l + hsv_.s * sq.width(), my = sq.t + (1.0f - hsv_.v) * sq.height();
        dc->DrawEllipse(D2D1::Ellipse(D2D1::Point2F(mx, my), 6.5f, 6.5f), black.Get(), 3.0f);
        dc->DrawEllipse(D2D1::Ellipse(D2D1::Point2F(mx, my), 6.5f, 6.5f), white.Get(), 1.5f);
        const float hy = st.t + hsv_.h / 360.0f * st.height();
        dc->DrawRectangle(D2D1::RectF(st.l - 2, hy - 3, st.r + 2, hy + 3), black.Get(), 3.0f);
        dc->DrawRectangle(D2D1::RectF(st.l - 2, hy - 3, st.r + 2, hy + 3), white.Get(), 1.5f);
    }
    // The alpha strip: this colour from clear to opaque, over the checkerboard.
    const Rect al = alpha_strip();
    checker(pc, al);
    {
        const D2D1::ColorF solid(rgb());
        if (auto a = gradient({{0.0f, D2D1::ColorF(solid.r, solid.g, solid.b, 0.0f)}, {1.0f, D2D1::ColorF(solid.r, solid.g, solid.b, 1.0f)}},
                              D2D1::Point2F(al.l, 0), D2D1::Point2F(al.r, 0))) {
            dc->FillRectangle(al.d2d(), a.Get());
        }
    }
    pc.stroke(al, tok::BorderDefault);
    if (white && black) {
        const float ax = al.l + static_cast<float>(alpha_) / 255.0f * al.width();
        dc->DrawRectangle(D2D1::RectF(ax - 2, al.t - 3, ax + 2, al.b + 3), black.Get(), 3.0f);
        dc->DrawRectangle(D2D1::RectF(ax - 2, al.t - 3, ax + 2, al.b + 3), white.Get(), 1.5f);
    }
    if (focused()) pc.stroke(bounds.deflate(1, 1, 1, 1), tok::BorderFocus);

    const Rect sw = swatch();
    checker(pc, sw);
    ComPtr<ID2D1SolidColorBrush> cur;
    dc->CreateSolidColorBrush(D2D1::ColorF(rgb(), static_cast<float>(alpha_) / 255.0f), &cur);
    if (cur) dc->FillRectangle(sw.d2d(), cur.Get());
    pc.stroke(sw, tok::BorderDefault);
    pc.text(hex_text(), Rect{sw.r + 10, sw.t, sw.r + 110, sw.b}, style.font, tok::TextPrimary);
}

void ColorPicker::drag_to(Point p, bool dragging) {
    if (drag_ == Drag::Square) {
        const Rect sq = square();
        hsv_.s = std::clamp((p.x - sq.l) / sq.width(), 0.0f, 1.0f);
        hsv_.v = 1.0f - std::clamp((p.y - sq.t) / sq.height(), 0.0f, 1.0f);
    } else if (drag_ == Drag::Strip) {
        const Rect st = strip();
        hsv_.h = std::min(359.999f, std::clamp((p.y - st.t) / st.height(), 0.0f, 1.0f) * 360.0f);
    } else if (drag_ == Drag::Alpha) {
        const Rect al = alpha_strip();
        alpha_ = static_cast<uint8_t>(std::lround(std::clamp((p.x - al.l) / al.width(), 0.0f, 1.0f) * 255.0f));
    } else {
        return;
    }
    invalidate();
    if (on_change) on_change(rgb(), dragging);
}

bool ColorPicker::on_mouse_down(const MouseEvent& e) {
    if (e.button != MouseButton::Left) return false;
    if (square().contains(e.pt)) {
        drag_ = Drag::Square;
    } else if (strip().inflate(2).contains(e.pt)) {
        drag_ = Drag::Strip;
    } else if (alpha_strip().inflate(2).contains(e.pt)) {
        drag_ = Drag::Alpha;
    } else {
        return false;
    }
    root()->capture(this);
    drag_to(e.pt, true);
    return true;
}

bool ColorPicker::on_mouse_move(const MouseEvent& e) {
    if (drag_ == Drag::None) return false;
    drag_to(e.pt, true);
    return true;
}

bool ColorPicker::on_mouse_up(const MouseEvent& e) {
    if (drag_ == Drag::None) return false;
    drag_to(e.pt, false);
    drag_ = Drag::None;
    return true;
}

bool ColorPicker::on_key_down(const KeyEvent& e) {
    const float step = e.mods.shift ? 0.05f : 0.01f;
    Hsv c = hsv_;
    switch (e.vk) {
        case VK_LEFT: if (e.mods.ctrl) c.h -= 3; else c.s -= step; break;
        case VK_RIGHT: if (e.mods.ctrl) c.h += 3; else c.s += step; break;
        case VK_UP: c.v += step; break;
        case VK_DOWN: c.v -= step; break;
        default: return false;
    }
    if (c.h < 0) c.h += 360;
    set_hsv(c);
    if (on_change) on_change(rgb(), false);
    return true;
}

}  // namespace bb::ui
