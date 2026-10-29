// File, folder and drive icons from the shell's system image list - Windows' own pictures, in colour, for
// every type something on the machine has registered. Spec: App/Interop/ShellIcons.cs.
//
// Most icons are a type's, not a file's: those are asked by extension with no disk access
// (SHGFI_USEFILEATTRIBUTES) on the UI thread and remembered for the session, so a folder of ten thousand
// .txt files costs one lookup. A few kinds carry their own picture - programs, shortcuts, icon files, and a
// folder with a desktop.ini (Downloads, Documents...) - and those are read from the file on a worker, because
// a shortcut to a dead network share can stall; until the answer is posted back the type's icon stands in.
// Drives get the stock drive pictures at once (no disk touched) and the fixed ones their own afterwards.
#pragma once
#include "ui/controls.h"

#include <windows.h>

#include <atomic>
#include <condition_variable>
#include <deque>
#include <functional>
#include <list>
#include <mutex>
#include <string>
#include <thread>
#include <unordered_map>
#include <unordered_set>
#include <vector>

namespace bb::views {

constexpr UINT WM_BB_ICONS = WM_APP + 5;

// An icon handle as a picture, `px` square, premultiplied (the app's own icon in About). Null if it cannot be drawn.
ui::ThumbPtr icon_picture(HICON icon, uint32_t px);

class ShellIcons {
public:
    explicit ShellIcons(HWND ui_window);
    ~ShellIcons();
    ShellIcons(const ShellIcons&) = delete;
    ShellIcons& operator=(const ShellIcons&) = delete;

    // UI thread only. `px` is the square size in device pixels; null when the shell has nothing. `real` false
    // means the path is not one the shell may open (inside an archive): the type's icon, never the file's own.
    ui::ThumbPtr file(const std::wstring& path, bool directory, uint32_t attributes, uint32_t px, bool real = true);
    ui::ThumbPtr drive(const std::wstring& root, uint32_t px);
    // One of Windows' stock pictures (SIID_*), for a row with no file behind it (a portable device).
    ui::ThumbPtr stock(int siid, uint32_t px);

    // On WM_BB_ICONS: takes in what the workers finished and calls `on_ready` when anything changed.
    void drain();
    size_t pending() const;
    std::function<void()> on_ready;

    // Whether a file's icon is its own (read from it) rather than its type's.
    static bool is_per_file(const std::wstring& path, bool directory, uint32_t attributes);

private:
    struct Slot {  // an entry of the system image list, with the overlay drawn over it (0 = none)
        int index = -1;
        int overlay = 0;
    };
    Slot type_slot(const std::wstring& path, bool directory);
    // The file's own slot when known; otherwise asks a worker and answers `fallback` meanwhile.
    Slot own_slot(const std::wstring& path, Slot fallback);
    ui::ThumbPtr image(Slot s, uint32_t px);
    void work();

    HWND window_;
    std::unordered_map<std::wstring, Slot> by_type_;  // ".txt", "<dir>", "<none>"
    struct Drive {
        Slot stock;
        bool fixed = false;
    };
    std::unordered_map<int, Slot> stock_;
    std::unordered_map<std::wstring, Drive> drives_;  // by root, asked of Windows once
    std::unordered_map<uint64_t, ui::ThumbPtr> images_;  // (slot, px)
    // Per-file answers, least recently used last, capped: every program ever browsed must not stay pinned.
    static constexpr size_t kPerFileCap = 512;
    std::list<std::pair<std::wstring, Slot>> lru_;
    std::unordered_map<std::wstring, std::list<std::pair<std::wstring, Slot>>::iterator> by_path_;
    std::unordered_set<std::wstring> asked_;  // queued or running, so a repaint does not ask twice

    mutable std::mutex mu_;
    std::condition_variable cv_;
    std::deque<std::wstring> jobs_;  // newest at the front: what is on screen now matters most
    std::vector<std::pair<std::wstring, Slot>> done_;
    std::vector<std::thread> threads_;
    std::atomic<bool> stop_{false};
    size_t running_ = 0;
    bool notified_ = false;
};

}  // namespace bb::views
