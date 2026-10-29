// The tool windows - disk usage, duplicates, what changed, checksums, the file compare, the undo history,
// the transfer queue and the theme editor - as windows of their own beside the main one, as the original's
// were: modeless, so the folders behind stay live and a window can be left open while the work goes on.
// Each is the same card the dialog layer would show, under a caption of its own (custom chrome; the card's
// header gives the title and is hidden). It is owned by the main window, so it stays above it and minimizes
// with it, and a parked main window (the harness) makes parked tool windows.
#pragma once
#include "platform/window.h"
#include "ui/widget.h"

#include <memory>
#include <string>
#include <string_view>

namespace bb::views {

class MainView;
class TitleBar;

class ToolWindow {
public:
    ToolWindow(MainView& main, std::wstring kind, std::unique_ptr<ui::Widget> card);
    ToolWindow(const ToolWindow&) = delete;
    ToolWindow& operator=(const ToolWindow&) = delete;
    ~ToolWindow();

    // A rebuilt card (the model moved): swapped in where the window is, at its new natural height.
    void replace(std::unique_ptr<ui::Widget> card);
    void show();
    void focus();
    void apply_theme(bool dark, COLORREF border, COLORREF caption, COLORREF text);
    ui::Widget* find(std::string_view name);
    const std::wstring& kind() const { return kind_; }
    platform::Window& window() { return *window_; }

private:
    void install(std::unique_ptr<ui::Widget> card);
    void fit();

    MainView& main_;
    std::wstring kind_;
    std::unique_ptr<platform::Window> window_;
    TitleBar* title_ = nullptr;
    ui::Widget* card_ = nullptr;
    float card_width_ = 480.0f;
    bool sized_ = false;
};

}  // namespace bb::views
