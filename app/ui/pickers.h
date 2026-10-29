// Pickers: a month-grid calendar and a colour picker (HSV square + hue strip).
#pragma once
#include "ui/controls.h"

namespace bb::ui {

// ---- calendar ---------------------------------------------------------------------------------------

struct Date {
    int year = 1970, month = 1, day = 1;  // month 1..12
    bool operator==(const Date&) const = default;
    auto operator<=>(const Date&) const = default;
};

namespace calendar {
int days_in_month(int year, int month);
bool is_leap(int year);
int days_from_civil(Date d);  // days since 1970-01-01
Date civil_from_days(int days);
int weekday(Date d);  // 0 = Sunday .. 6 = Saturday
Date add_days(Date d, int n);
Date add_months(Date d, int n);  // the day is clamped to the target month's length
// The date in cell (row, col) of the 6x7 grid showing `year`/`month`, weeks starting on `first_dow`.
Date cell(int year, int month, int row, int col, int first_dow);
}  // namespace calendar

class Calendar : public Widget {
public:
    static constexpr float kCell = 34.0f, kRow = 30.0f, kHeader = 36.0f, kDays = 26.0f, kPad = 8.0f;

    Calendar();

    int first_day_of_week = 1;  // 0 = Sunday; the user's locale at construction
    Date today;                 // highlighted; the system date at construction, a test sets its own

    Date selected() const { return selected_; }
    void set_selected(Date d);  // also shows its month; no callback
    int shown_year() const { return year_; }
    int shown_month() const { return month_; }
    void show_month(int year, int month);

    std::function<void(Date)> on_select;
    std::function<void(int year, int month)> on_month_changed;

    Rect prev_rect() const;
    Rect next_rect() const;
    Rect cell_rect(int row, int col) const;
    Date cell_date(int row, int col) const { return calendar::cell(year_, month_, row, col, first_day_of_week); }
    bool cell_at(Point p, int* row, int* col) const;

    Size measure(Size) override;
    void paint(PaintContext& pc) override;
    bool on_mouse_down(const MouseEvent& e) override;
    bool on_key_down(const KeyEvent& e) override;
    void on_focus(bool) override { invalidate(); }

private:
    void choose(Date d);
    int year_ = 1970, month_ = 1;
    Date selected_;
};

// ---- colour picker ----------------------------------------------------------------------------------

struct Hsv {
    float h = 0, s = 0, v = 0;  // h in [0, 360), s and v in [0, 1]
};
Hsv rgb_to_hsv(uint32_t rgb);  // 0xRRGGBB
uint32_t hsv_to_rgb(Hsv c);

class ColorPicker : public Widget {
public:
    static constexpr float kPad = 8.0f, kSquareW = 220.0f, kSquareH = 160.0f, kStripW = 20.0f, kGap = 10.0f;

    ColorPicker() {
        flags.focusable = true;
        flags.tab_stop = true;
        set_rgb(0xFF0000);
    }

    uint32_t rgb() const { return hsv_to_rgb(hsv_); }
    Hsv hsv() const { return hsv_; }
    void set_rgb(uint32_t rgb);  // no callback. Greys keep the hue already shown.
    void set_hsv(Hsv c);
    std::function<void(uint32_t rgb, bool dragging)> on_change;

    Rect square() const;
    Rect strip() const;
    Rect swatch() const;
    std::wstring hex_text() const;  // "#RRGGBB"

    Size measure(Size) override;
    void paint(PaintContext& pc) override;
    bool on_mouse_down(const MouseEvent& e) override;
    bool on_mouse_move(const MouseEvent& e) override;
    bool on_mouse_up(const MouseEvent& e) override;
    bool on_key_down(const KeyEvent& e) override;
    void on_focus(bool) override { invalidate(); }

private:
    enum class Drag { None, Square, Strip };
    void drag_to(Point p, bool dragging);
    Hsv hsv_;
    Drag drag_ = Drag::None;
};

}  // namespace bb::ui
