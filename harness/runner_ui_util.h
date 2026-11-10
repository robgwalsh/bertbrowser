// Helpers shared by the window-feature verbs (runner_ui*.cpp).
#pragma once
#include "options.h"
#include "runner.h"

#include <cstdint>
#include <filesystem>
#include <string>
#include <utility>
#include <vector>

namespace bbh::ui_util {

std::wstring lower(std::wstring s);
std::wstring trim(const std::wstring& s);
std::vector<std::wstring> words(const std::wstring& s);
std::vector<std::wstring> bar_split(const std::wstring& s);
std::string join(const std::vector<std::wstring>& v, const char* sep);
// "Ctrl+Shift+F5" -> modifiers (1 Ctrl, 2 Alt, 4 Shift) and the key's name.
bool parse_chord(const std::wstring& text, uint32_t* mods, std::wstring* key);
BbFocusKind focus_word(const std::wstring& w);
void collect_headers(const bb::ui::MenuItems& items, std::vector<std::wstring>* headers,
                     std::vector<std::pair<std::wstring, std::wstring>>* gestures);
// 8-bit RGBA, filter 0, zlib "stored" blocks.
void write_png_rgba(const fs::path& path, uint32_t w, uint32_t h, const std::vector<uint8_t>& rgba);
void write_zip(const fs::path& path, const std::vector<std::pair<std::string, std::string>>& entries);
void write_text(const fs::path& path, const std::string& text, bool bom = false);

}  // namespace bbh::ui_util
