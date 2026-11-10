// Helpers shared by the window-feature verbs (runner_ui*.cpp).
#include "runner_ui_util.h"

#include <windows.h>

#include <algorithm>
#include <cwctype>
#include <fstream>

#include "views/bridge/json.h"

namespace bbh::ui_util {

namespace core = bb::core;
namespace ui = bb::ui;

std::wstring lower(std::wstring s) {
    for (auto& c : s) c = static_cast<wchar_t>(std::towlower(c));
    return s;
}

std::wstring trim(const std::wstring& s) {
    const size_t b = s.find_first_not_of(L" \t\r\n");
    if (b == std::wstring::npos) return {};
    return s.substr(b, s.find_last_not_of(L" \t\r\n") - b + 1);
}

std::vector<std::wstring> words(const std::wstring& s) {
    std::vector<std::wstring> out;
    std::wstring cur;
    for (wchar_t c : s) {
        if (c == L' ') {
            if (!cur.empty()) out.push_back(cur);
            cur.clear();
        } else {
            cur += c;
        }
    }
    if (!cur.empty()) out.push_back(cur);
    return out;
}

std::vector<std::wstring> bar_split(const std::wstring& s) {
    std::vector<std::wstring> out;
    for (const std::wstring& p : core::split(s, L'|')) {
        const std::wstring t = trim(p);
        if (!t.empty()) out.push_back(t);
    }
    return out;
}

std::string join(const std::vector<std::wstring>& v, const char* sep) {
    std::string out;
    for (size_t i = 0; i < v.size(); ++i) out += (i ? sep : "") + to_utf8(v[i]);
    return out;
}

// "Ctrl+Shift+F5" -> modifiers (1 Ctrl, 2 Alt, 4 Shift) and the key's name.
bool parse_chord(const std::wstring& text, uint32_t* mods, std::wstring* key) {
    std::vector<std::wstring> parts = core::split(trim(text), L'+');
    *mods = 0;
    if (parts.empty()) return false;
    std::wstring k = parts.back();
    parts.pop_back();
    if (k.empty() && !parts.empty()) {  // "Num+": the key is the plus itself
        k = parts.back() + L"+";
        parts.pop_back();
    }
    for (const std::wstring& p : parts) {
        const std::wstring m = lower(p);
        if (m == L"ctrl") *mods |= 1;
        else if (m == L"alt") *mods |= 2;
        else if (m == L"shift") *mods |= 4;
        else return false;
    }
    if (k.size() == 1) k[0] = static_cast<wchar_t>(std::towupper(k[0]));
    *key = k;
    return !k.empty();
}

BbFocusKind focus_word(const std::wstring& w) {
    const std::wstring f = lower(trim(w));
    if (f == L"list") return BB_FOCUS_FILE_LIST;
    if (f == L"text") return BB_FOCUS_TEXT_EDITABLE;
    if (f == L"readonly") return BB_FOCUS_TEXT_READ_ONLY;
    if (f == L"other" || f == L"tree") return BB_FOCUS_OTHER;
    throw FormatError("Focus is one of list, text, readonly or other, not '" + to_utf8(w) + "'.");
}

void collect_headers(const ui::MenuItems& items, std::vector<std::wstring>* headers,
                     std::vector<std::pair<std::wstring, std::wstring>>* gestures) {
    for (const ui::MenuItem& m : items) {
        if (m.is_separator) continue;
        const std::wstring text = ui::parse_menu_label(m.text).display;
        headers->push_back(text);
        gestures->push_back({text, m.gesture});
        collect_headers(m.sub, headers, gestures);
    }
}


// ---- fixtures: a PNG and a zip written by hand, so two runs make byte-identical files ----

namespace {

uint32_t crc32_of(const uint8_t* data, size_t n, uint32_t crc = 0) {
    static uint32_t table[256];
    static bool made = false;
    if (!made) {
        for (uint32_t i = 0; i < 256; ++i) {
            uint32_t c = i;
            for (int k = 0; k < 8; ++k) c = c & 1 ? 0xEDB88320u ^ (c >> 1) : c >> 1;
            table[i] = c;
        }
        made = true;
    }
    crc = ~crc;
    for (size_t i = 0; i < n; ++i) crc = table[(crc ^ data[i]) & 0xFF] ^ (crc >> 8);
    return ~crc;
}

void put_be32(std::string& s, uint32_t v) {
    for (int i = 3; i >= 0; --i) s += static_cast<char>(v >> (8 * i) & 0xFF);
}
void put_le16(std::string& s, uint32_t v) {
    s += static_cast<char>(v & 0xFF);
    s += static_cast<char>(v >> 8 & 0xFF);
}
void put_le32(std::string& s, uint32_t v) {
    for (int i = 0; i < 4; ++i) s += static_cast<char>(v >> (8 * i) & 0xFF);
}

void png_chunk(std::string& out, const char* type, const std::string& data) {
    put_be32(out, static_cast<uint32_t>(data.size()));
    std::string body = std::string(type, 4) + data;
    out += body;
    put_be32(out, crc32_of(reinterpret_cast<const uint8_t*>(body.data()), body.size()));
}

}  // namespace

// 8-bit RGBA, filter 0, zlib "stored" blocks.
void write_png_rgba(const fs::path& path, uint32_t w, uint32_t h, const std::vector<uint8_t>& rgba) {
    std::string raw;
    for (uint32_t y = 0; y < h; ++y) {
        raw += '\0';
        raw.append(reinterpret_cast<const char*>(&rgba[static_cast<size_t>(y) * w * 4]), static_cast<size_t>(w) * 4);
    }
    std::string z = "\x78\x01";
    for (size_t pos = 0; pos < raw.size(); pos += 65535) {
        const size_t n = std::min<size_t>(65535, raw.size() - pos);
        z += pos + n >= raw.size() ? '\x01' : '\x00';
        put_le16(z, static_cast<uint32_t>(n));
        put_le16(z, static_cast<uint32_t>(~n & 0xFFFF));
        z.append(raw, pos, n);
    }
    uint32_t a = 1, b = 0;
    for (const unsigned char c : raw) {
        a = (a + c) % 65521;
        b = (b + a) % 65521;
    }
    put_be32(z, b << 16 | a);
    std::string out = "\x89PNG\r\n\x1a\n";
    std::string ihdr;
    put_be32(ihdr, w);
    put_be32(ihdr, h);
    ihdr += std::string("\x08\x06\x00\x00\x00", 5);
    png_chunk(out, "IHDR", ihdr);
    png_chunk(out, "IDAT", z);
    png_chunk(out, "IEND", "");
    std::ofstream(path, std::ios::binary | std::ios::trunc).write(out.data(), static_cast<std::streamsize>(out.size()));
}

void write_zip(const fs::path& path, const std::vector<std::pair<std::string, std::string>>& entries) {
    std::string out, central;
    for (const auto& [name, data] : entries) {
        const uint32_t crc = crc32_of(reinterpret_cast<const uint8_t*>(data.data()), data.size());
        const uint32_t offset = static_cast<uint32_t>(out.size());
        put_le32(out, 0x04034b50);
        put_le16(out, 20);
        put_le16(out, 0);
        put_le16(out, 0);
        put_le16(out, 0x6000);  // a fixed time and date
        put_le16(out, 0x5821);
        put_le32(out, crc);
        put_le32(out, static_cast<uint32_t>(data.size()));
        put_le32(out, static_cast<uint32_t>(data.size()));
        put_le16(out, static_cast<uint32_t>(name.size()));
        put_le16(out, 0);
        out += name;
        out += data;
        put_le32(central, 0x02014b50);
        put_le16(central, 20);
        put_le16(central, 20);
        put_le16(central, 0);
        put_le16(central, 0);
        put_le16(central, 0x6000);
        put_le16(central, 0x5821);
        put_le32(central, crc);
        put_le32(central, static_cast<uint32_t>(data.size()));
        put_le32(central, static_cast<uint32_t>(data.size()));
        put_le16(central, static_cast<uint32_t>(name.size()));
        put_le16(central, 0);
        put_le16(central, 0);
        put_le16(central, 0);
        put_le16(central, 0);
        put_le32(central, 0);
        put_le32(central, offset);
        central += name;
    }
    const uint32_t cd_offset = static_cast<uint32_t>(out.size());
    out += central;
    put_le32(out, 0x06054b50);
    put_le16(out, 0);
    put_le16(out, 0);
    put_le16(out, static_cast<uint32_t>(entries.size()));
    put_le16(out, static_cast<uint32_t>(entries.size()));
    put_le32(out, static_cast<uint32_t>(central.size()));
    put_le32(out, cd_offset);
    put_le16(out, 0);
    std::ofstream(path, std::ios::binary | std::ios::trunc).write(out.data(), static_cast<std::streamsize>(out.size()));
}

void write_text(const fs::path& path, const std::string& text, bool bom) {
    fs::create_directories(path.parent_path());
    std::ofstream f(path, std::ios::binary | std::ios::trunc);
    if (bom) f << "\xEF\xBB\xBF";
    f << text;
}

}  // namespace bbh::ui_util
