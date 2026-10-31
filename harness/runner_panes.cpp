// The side panes' verbs: the preview (modes, the animated GIF's transport, what it holds), the metadata pane
// (typing into its boxes, pressing its buttons, reading a file's tags back a second way), the fixtures they
// edit, rename by rule, and the comparison's verdicts. Every write goes through the pane's own boxes and
// buttons, and so through the queue and the undo history, never straight to an executor: a script that edited
// files by a shorter path would prove the shorter path.
#include "runner.h"

#include <windows.h>

#include <algorithm>
#include <chrono>
#include <cwctype>
#include <fstream>

#include "views/bridge/json.h"
#include "views/main/main_view.h"

namespace bbh {

namespace core = bb::core;
namespace views = bb::views;
namespace fs_ = std::filesystem;

namespace {

using Clock = std::chrono::steady_clock;

std::wstring lower(std::wstring s) {
    for (auto& c : s) c = static_cast<wchar_t>(std::towlower(c));
    return s;
}

std::wstring trim(const std::wstring& s) {
    const size_t b = s.find_first_not_of(L" \t\r\n");
    if (b == std::wstring::npos) return {};
    return s.substr(b, s.find_last_not_of(L" \t\r\n") - b + 1);
}

std::wstring unquote(const std::wstring& s) {
    if (s.size() > 1 && s.front() == L'"' && s.back() == L'"') return s.substr(1, s.size() - 2);
    return s;
}

std::string json_quote(const std::string& text) {
    std::string out = "\"";
    for (const char c : text) {
        switch (c) {
            case '\\': out += "\\\\"; break;
            case '"': out += "\\\""; break;
            case '\n': out += "\\n"; break;
            case '\r': out += "\\r"; break;
            case '\t': out += "\\t"; break;
            default: out += c; break;
        }
    }
    return out + "\"";
}

// ---- the GIF fixture, laid out byte by byte -----------------------------------------------------------------
//
// Not through an encoder, which cannot place a frame anywhere but the origin or ask for a disposal, and a
// sub-rectangle that clears itself is exactly what compositing has to get right. A GIF89a with a 128-colour
// palette, so every LZW code is exactly 8 bits (one byte) and a clear code every hundred pixels keeps the
// dictionary from ever growing past that width: uncompressed in all but name, and readable by any decoder.
// Index 0 is transparent.

struct GifFrame {
    int left, top, width, height;
    std::vector<uint8_t> indices;
    int delay_cs, disposal;
};

void put16(std::string& out, int v) {
    out += static_cast<char>(v & 0xFF);
    out += static_cast<char>((v >> 8) & 0xFF);
}

void write_gif(const fs_::path& path, int width, int height, const std::vector<GifFrame>& frames) {
    std::string out = "GIF89a";
    put16(out, width);
    put16(out, height);
    out += static_cast<char>(0xF6);  // global colour table, 128 entries
    out += '\0';
    out += '\0';
    std::string palette(128 * 3, '\0');
    auto colour = [&](int i, int r, int g, int b) {
        palette[static_cast<size_t>(i) * 3] = static_cast<char>(r);
        palette[static_cast<size_t>(i) * 3 + 1] = static_cast<char>(g);
        palette[static_cast<size_t>(i) * 3 + 2] = static_cast<char>(b);
    };
    colour(1, 0xE0, 0x30, 0x30);
    colour(2, 0x30, 0xC0, 0x50);
    colour(3, 0x30, 0x60, 0xE0);
    out += palette;
    if (frames.size() > 1) {
        out += std::string("\x21\xFF\x0B", 3);
        out += "NETSCAPE2.0";
        out += std::string("\x03\x01\x00\x00\x00", 5);
    }
    for (const GifFrame& f : frames) {
        out += std::string("\x21\xF9\x04", 3);
        out += static_cast<char>((f.disposal << 2) | 1);
        put16(out, f.delay_cs);
        out += '\0';  // transparent index 0
        out += '\0';
        out += '\x2C';
        put16(out, f.left);
        put16(out, f.top);
        put16(out, f.width);
        put16(out, f.height);
        out += '\0';
        out += '\x07';  // minimum code size: codes start 8 bits wide
        std::string codes;
        for (size_t i = 0; i < f.indices.size(); ++i) {
            if (i % 100 == 0) codes += static_cast<char>(128);  // clear
            codes += static_cast<char>(f.indices[i]);
        }
        codes += static_cast<char>(129);  // end of information
        for (size_t i = 0; i < codes.size(); i += 255) {
            const size_t n = std::min<size_t>(255, codes.size() - i);
            out += static_cast<char>(n);
            out.append(codes, i, n);
        }
        out += '\0';
    }
    out += '\x3B';
    std::ofstream file(path, std::ios::binary | std::ios::trunc);
    file.write(out.data(), static_cast<std::streamsize>(out.size()));
}

std::string local_stamp(const FILETIME& utc) {
    // The zone's rules for that date, not today's: FileTimeToLocalFileTime uses the current offset, which in
    // winter reads a summer date an hour out.
    SYSTEMTIME utc_st{}, st{};
    FileTimeToSystemTime(&utc, &utc_st);
    SystemTimeToTzSpecificLocalTime(nullptr, &utc_st, &st);
    char buf[32];
    snprintf(buf, sizeof(buf), "%04d-%02d-%02d %02d:%02d:%02d", st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond);
    return buf;
}

// "field=value; field=value". A piece with no '=' belongs to the value before it: a list field's own values
// are separated by semicolons too, and "keywords=sea; boats" should mean what it says.
std::vector<std::pair<std::wstring, std::wstring>> typed_pairs(const std::wstring& text) {
    std::vector<std::pair<std::wstring, std::wstring>> pairs;
    for (const std::wstring& raw : core::split(text, L';')) {
        const std::wstring part = trim(raw);
        if (part.empty()) continue;
        const size_t eq = part.find(L'=');
        if (eq != std::wstring::npos && eq > 0) {
            pairs.emplace_back(trim(part.substr(0, eq)), trim(part.substr(eq + 1)));
        } else if (!pairs.empty()) {
            pairs.back().second += L"; " + part;
        } else {
            throw FormatError("'" + to_utf8(part) + "' is not 'field=value'.");
        }
    }
    return pairs;
}

// key=value settings, quoted when they hold a space: an empty value is a real setting.
std::vector<std::pair<std::wstring, std::wstring>> settings(const std::wstring& text) {
    std::vector<std::pair<std::wstring, std::wstring>> out;
    size_t i = 0;
    while (i < text.size()) {
        while (i < text.size() && text[i] == L' ') ++i;
        if (i >= text.size()) break;
        const size_t eq = text.find(L'=', i);
        if (eq == std::wstring::npos) throw FormatError("rename-rule needs key=value settings.");
        const std::wstring key = lower(trim(text.substr(i, eq - i)));
        i = eq + 1;
        std::wstring value;
        if (i < text.size() && text[i] == L'"') {
            const size_t close = text.find(L'"', i + 1);
            if (close == std::wstring::npos) throw FormatError("rename-rule: an unclosed quote.");
            value = text.substr(i + 1, close - i - 1);
            i = close + 1;
        } else {
            const size_t end = text.find(L' ', i);
            value = text.substr(i, end == std::wstring::npos ? std::wstring::npos : end - i);
            i = end == std::wstring::npos ? text.size() : end;
        }
        out.emplace_back(key, value);
    }
    return out;
}

}  // namespace

void Runner::settle_preview() {
    const auto start = Clock::now();
    while (s_.view().preview_pending() &&
           std::chrono::duration_cast<std::chrono::milliseconds>(Clock::now() - start).count() < o_.busy_timeout_ms) {
        s_.pump();
        Sleep(1);
    }
    if (s_.view().preview_pending()) throw TimeoutError("The preview was still loading after " + std::to_string(o_.busy_timeout_ms) + " ms.");
    s_.settle();
}

bb::views::PreviewPane* Runner::preview_pane() {
    views::TabView* tv = s_.view().active_view();
    return tv ? tv->preview() : nullptr;
}

bb::views::MetadataPane* Runner::metadata_pane() {
    core::check(bb_tab_set_side_pane(s_.core().app(), tab(), 2), "side-pane");
    s_.settle();
    views::TabView* tv = s_.view().active_view();
    if (!tv) throw AssertionError("The metadata pane did not open.");
    views::MetadataPane* pane = tv->ensure_metadata();
    pane->set_active(true);
    pane->follow_selection(false);
    return pane;
}

void Runner::install_panes() {
    // ---- the preview ----
    verbs_[L"preview-mode"] = [this](const std::wstring& r) {
        const std::wstring mode = lower(require(r, "preview-mode"));
        const wchar_t* id = mode == L"auto" ? L"preview.mode-auto"
                            : mode == L"raw" || mode == L"text" ? L"preview.mode-raw"
                            : mode == L"hex"                   ? L"preview.mode-hex"
                                                               : nullptr;
        if (!id) throw FormatError("preview-mode wants auto, raw or hex, got '" + to_utf8(mode) + "'.");
        core::check(bb_command_run(s_.core().app(), core::s(std::wstring(id))), "preview-mode");
        // The pane re-reads off-thread, so an assertion made straight after this would pass whatever the mode did.
        s_.settle();
        settle_preview();
    };
    verbs_[L"settle-preview"] = [this](const std::wstring&) { settle_preview(); };
    verbs_[L"assert-preview"] = [this](const std::wstring& r) {
        const std::string expected = to_utf8(lower(require(r, "assert-preview")));
        views::PreviewPane* pane = preview_pane();
        const std::string actual = pane ? pane->state_name() : "none";
        if (actual != expected) throw AssertionError("expected the preview to be '" + expected + "', it is '" + actual + "'.");
    };
    verbs_[L"preview-fit-width"] = [this](const std::wstring&) {
        bb::ui::Widget* w = s_.view().find_named("FitWidthButton");
        auto* button = dynamic_cast<bb::ui::Button*>(w);
        if (!button) throw std::runtime_error("FitWidthButton is not in the visual tree.");
        button->click();
        s_.settle();
    };

    // ---- the animated GIF ----
    verbs_[L"gif-fixture"] = [this](const std::wstring& r) {
        const fs_::path root = sandbox_.require_inside(r.empty() ? L"Gif" : r, "gif-fixture");
        fs_::create_directories(root);
        constexpr uint8_t transparent = 0, red = 1, green = 2, blue = 3;
        std::vector<uint8_t> background(64 * 64);
        for (int y = 0; y < 64; ++y) {
            for (int x = 0; x < 64; ++x) background[static_cast<size_t>(y * 64 + x)] = x < 12 ? red : blue;
        }
        std::vector<uint8_t> square(16 * 16, green), corner(16 * 16);
        for (size_t i = 0; i < corner.size(); ++i) corner[i] = (i / 16 + i % 16) % 2 == 0 ? red : transparent;
        // Frame 0 is blue with a red bar (200 ms); frame 1 a green square that clears itself (200 ms); frame 2 a
        // half-transparent red corner declaring 0 ms, which plays at 100 ms as in every browser.
        write_gif(root / "spinner.gif", 64, 64,
                  {{0, 0, 64, 64, background, 20, 1}, {24, 24, 16, 16, square, 20, 2}, {44, 4, 16, 16, corner, 0, 1}});
        write_gif(root / "still.gif", 64, 64, {{0, 0, 64, 64, background, 0, 0}});
        // Big enough that decoding every frame takes a visible while: what the pane must not wait for before
        // showing the first one. A green bar sweeping across blue, whole frames.
        constexpr int bw = 320, bh = 240, bn = 150;
        std::vector<GifFrame> big;
        for (int f = 0; f < bn; ++f) {
            std::vector<uint8_t> pixels(static_cast<size_t>(bw * bh));
            const int bar = f * (bw - 20) / (bn - 1);
            for (int y = 0; y < bh; ++y) {
                for (int x = 0; x < bw; ++x) pixels[static_cast<size_t>(y * bw + x)] = (x >= bar && x < bar + 20) ? green : blue;
            }
            big.push_back({0, 0, bw, bh, std::move(pixels), 4, 1});
        }
        write_gif(root / "big.gif", bw, bh, big);
        Sandbox::stamp(root);
        say("# gif fixture: " + to_utf8(root.wstring()));
    };
    verbs_[L"gif"] = [this](const std::wstring& r) {
        const auto parts = core::split(require(r, "gif"), L' ');
        views::PreviewPane* pane = preview_pane();
        if (!pane) throw std::runtime_error("PreviewPane is not in the visual tree.");
        const std::wstring what = lower(parts[0]);
        if (what == L"play" || what == L"pause") {
            if (pane->animation_running() != (what == L"play")) pane->gif_play_pause()->click();
        } else if (what == L"seek" && parts.size() == 2) {
            pane->animation_seek(_wtof(parts[1].c_str()));
        } else {
            throw FormatError("gif: expected 'play', 'pause' or 'seek <ms>', got '" + to_utf8(r) + "'.");
        }
    };
    verbs_[L"assert-gif"] = [this](const std::wstring& r) {
        const auto parts = core::split(require(r, "assert-gif"), L' ');
        views::PreviewPane* pane = preview_pane();
        const bool animated = pane && pane->animated();
        const std::string state = !animated ? "none" : pane->animation_running() ? "playing" : "paused";
        const std::wstring what = lower(parts[0]);
        if (what == L"none" || what == L"playing" || what == L"paused") {
            if (state != to_utf8(what)) throw AssertionError("expected the animation to be '" + to_utf8(what) + "', it is '" + state + "'.");
        } else if (what == L"frame" && parts.size() == 2) {
            if (!animated) throw AssertionError("expected an animation, there is none.");
            const int expected = _wtoi(parts[1].c_str());
            if (pane->animation_frame() != expected) {
                throw AssertionError("expected frame " + std::to_string(expected) + ", it is frame " + std::to_string(pane->animation_frame()) + ".");
            }
        } else {
            throw FormatError("assert-gif: expected 'none', 'playing', 'paused' or 'frame <n>', got '" + to_utf8(r) + "'.");
        }
    };

    // ---- the side pane ----
    verbs_[L"side-pane"] = [this](const std::wstring& r) {
        const std::wstring what = lower(require(r, "side-pane"));
        if (what == L"cycle") {
            core::check(bb_command_run(s_.core().app(), core::s(L"view.side-pane")), "side-pane");
        } else if (what == L"none" || what == L"preview" || what == L"metadata") {
            core::check(bb_tab_set_side_pane(s_.core().app(), tab(), what == L"none" ? 0 : what == L"preview" ? 1 : 2), "side-pane");
        } else {
            throw FormatError("side-pane wants none, preview, metadata or cycle.");
        }
        s_.settle(400);
    };
    verbs_[L"assert-side-pane"] = [this](const std::wstring& r) {
        const uint32_t pane = s_.core().tab_state(tab()).side_pane;
        const std::wstring actual = pane == 1 ? L"preview" : pane == 2 ? L"metadata" : L"none";
        if (actual != lower(require(r, "assert-side-pane"))) {
            throw AssertionError("the side pane is " + to_utf8(actual) + ", expected " + to_utf8(r) + ".");
        }
    };

    // ---- fixtures ----
    auto fixture = [this](const char* verb, const char* kind, const wchar_t* fallback) {
        return [this, verb, kind, fallback](const std::wstring& r) {
            const fs_::path root = sandbox_.require_inside(r.empty() ? fallback : r, verb);
            core::check(bb_fixture_write(core::s(core::wide(kind)), core::s(root.wstring())), verb);
            say(std::string("# ") + verb + ": " + to_utf8(root.wstring()));
        };
    };
    verbs_[L"metadata-fixture"] = fixture("metadata-fixture", "metadata", L".");
    verbs_[L"media-fixture"] = fixture("media-fixture", "media", L".");
    verbs_[L"formats-fixture"] = fixture("formats-fixture", "formats", L".");
    verbs_[L"windows-fixture"] = fixture("windows-fixture", "windows", L".");

    // ---- the metadata pane ----
    verbs_[L"metadata-type"] = [this](const std::wstring& r) {
        views::MetadataPane* pane = metadata_pane();
        for (const auto& [id, value] : typed_pairs(r)) {
            if (pane->field_text(id) == L"(absent)") {
                std::string have;
                for (size_t i = 0; i < pane->model()["fields"].size(); ++i) have += (i ? ", " : "") + pane->model()["fields"][i]["id"].str();
                throw AssertionError("the pane has no '" + to_utf8(id) + "' box for this selection. It has: " + have);
            }
            pane->type_into(id, value);
        }
        s_.settle(200);
    };
    verbs_[L"metadata-set"] = [this](const std::wstring& r) {
        if (r.empty()) throw FormatError("metadata-set wants 'field=value; field=value'.");
        views::MetadataPane* pane = metadata_pane();
        for (const auto& [id, value] : typed_pairs(r)) {
            if (pane->field_text(id) == L"(absent)") {
                std::string have;
                for (size_t i = 0; i < pane->model()["fields"].size(); ++i) have += (i ? ", " : "") + pane->model()["fields"][i]["id"].str();
                throw AssertionError("the pane has no '" + to_utf8(id) + "' box for this selection. It has: " + have);
            }
            pane->type_into(id, value);
        }
        press_metadata(pane, 0, "apply '" + to_utf8(r) + "'");
    };
    verbs_[L"metadata-remove"] = [this](const std::wstring& r) {
        const std::wstring what = lower(trim(r));
        const int action = what == L"location" ? 1 : what == L"all" ? 2 : what == L"picture" ? 3 : 0;
        if (!action) throw FormatError("metadata-remove wants 'location', 'picture' or 'all'.");
        press_metadata(metadata_pane(), action, "remove " + to_utf8(what));
    };
    verbs_[L"metadata-picture"] = [this](const std::wstring& r) {
        const std::wstring path = row_path(require(r, "metadata-picture"));
        std::vector<uint8_t> bytes;
        if (!views::read_whole_file(path, 32ull << 20, bytes)) throw AssertionError("could not read " + to_utf8(path));
        views::MetadataPane* pane = metadata_pane();
        uint64_t op = 0;
        try {
            op = pane->set_cover(bytes);
        } catch (const core::CoreError& e) {
            throw AssertionError(e.what());
        }
        await_op(op);
        s_.settle();
        pane->refresh();
    };
    verbs_[L"assert-metadata-field"] = [this](const std::wstring& r) {
        const auto [id, expected] = split_first(r);
        views::MetadataPane* pane = metadata_pane();
        s_.settle();
        pane->refresh();
        const std::wstring actual = pane->field_text(id);
        if (actual != unquote(expected)) {
            throw AssertionError("the pane's '" + to_utf8(id) + "' box shows " + to_utf8(actual) + ", expected " + to_utf8(expected) + ".");
        }
    };
    verbs_[L"assert-metadata-file"] = [this](const std::wstring& r) {
        const auto [name, tail] = split_first(r);
        const auto [state, words] = split_first(tail);
        views::MetadataPane* pane = metadata_pane();
        s_.settle();
        pane->refresh();
        const bb::json::Value* file = pane->file(name);
        if (!file) {
            std::string have;
            for (size_t i = 0; i < pane->model()["files"].size(); ++i) have += (i ? ", " : "") + pane->model()["files"][i]["name"].str();
            throw AssertionError("'" + to_utf8(name) + "' is not in the pane. It lists: " + have);
        }
        const std::wstring actual = core::wide((*file)["state"].str()), detail = core::wide((*file)["detail"].str());
        if (lower(actual) != lower(state)) {
            throw AssertionError("'" + to_utf8(name) + "' is " + to_utf8(actual) + " in the pane (" + to_utf8(detail) + "), expected " + to_utf8(state) + ".");
        }
        if (!words.empty() && !core::icontains(detail, words)) {
            throw AssertionError("'" + to_utf8(name) + "' says '" + to_utf8(detail) + "', expected it to mention '" + to_utf8(words) + "'.");
        }
    };
    verbs_[L"assert-metadata-detail"] = [this](const std::wstring& r) {
        const auto [label, value] = split_first(r);
        views::MetadataPane* pane = metadata_pane();
        s_.settle();
        pane->refresh();
        std::string shown;
        for (size_t i = 0; i < pane->model()["details"].size(); ++i) {
            const bb::json::Value& d = pane->model()["details"][i];
            const std::wstring l = core::wide(d["label"].str()), v = core::wide(d["value"].str());
            shown += (i ? " | " : "") + to_utf8(l) + ": " + to_utf8(v);
            if (lower(l).rfind(lower(label), 0) == 0 && core::icontains(v, value)) return;
        }
        throw AssertionError("no detail '" + to_utf8(label) + "' containing '" + to_utf8(value) + "'. The pane shows: " + shown);
    };
    verbs_[L"assert-tag"] = [this](const std::wstring& r) {
        const auto [name, tail] = split_first(r);
        const auto [id, expected] = split_first(tail);
        if (expected.empty()) throw FormatError("assert-tag wants a row, a field and the value, or (none).");
        const std::wstring path = row_path(name);
        BbOwnedStr o{};
        const BbStatus st = bb_metadata_peek(s_.core().app(), core::s(path), core::s(id), &o);
        if (st != BB_OK) throw AssertionError(to_utf8(core::last_error()));
        const std::wstring actual = core::take(o);
        const std::wstring wanted = expected == L"(none)" ? L"" : unquote(expected);
        if (actual != wanted) {
            throw AssertionError("the " + to_utf8(id) + " of '" + to_utf8(name) + "' is '" + to_utf8(actual) + "', expected '" + to_utf8(wanted) + "'.");
        }
    };
    verbs_[L"assert-file-date"] = [this](const std::wstring& r) {
        const auto [name, tail] = split_first(r);
        const auto [which, expected] = split_first(tail);
        const std::wstring path = row_path(name);
        WIN32_FILE_ATTRIBUTE_DATA data{};
        if (!GetFileAttributesExW(path.c_str(), GetFileExInfoStandard, &data)) throw AssertionError("could not read the dates of " + to_utf8(path));
        const std::wstring w = lower(which);
        if (w != L"modified" && w != L"created") throw FormatError("assert-file-date wants 'modified' or 'created'.");
        const std::string actual = local_stamp(w == L"modified" ? data.ftLastWriteTime : data.ftCreationTime);
        if (actual != to_utf8(unquote(expected))) {
            throw AssertionError("'" + to_utf8(name) + "' was " + to_utf8(which) + " " + actual + ", expected " + to_utf8(expected) + ".");
        }
    };

    // ---- names from tags, and the history ----
    verbs_[L"rename-rule"] = [this](const std::wstring& r) {
        if (trim(r).empty()) throw FormatError("rename-rule needs key=value settings.");
        std::string json = "{";
        bool first = true;
        auto add = [&](const char* key, const std::string& value) {
            json += std::string(first ? "" : ",") + "\"" + key + "\":" + value;
            first = false;
        };
        auto flag = [](const std::wstring& v) { return lower(v) == L"on" || lower(v) == L"true" || lower(v) == L"yes" ? "true" : "false"; };
        for (const auto& [key, value] : settings(r)) {
            if (key == L"template") add("template", json_quote(to_utf8(value)));
            else if (key == L"find") add("find", json_quote(to_utf8(value)));
            else if (key == L"replace") add("replace", json_quote(to_utf8(value)));
            else if (key == L"regex") add("useRegex", flag(value));
            else if (key == L"matchcase") add("matchCase", flag(value));
            else if (key == L"scope") {
                const std::wstring v = lower(value);
                const char* name = v == L"extension" || v == L"ext" ? "Extension" : v == L"whole" || v == L"wholename" || v == L"full" ? "WholeName" : "Stem";
                add("scope", json_quote(name));
            } else if (key == L"case") {
                const std::wstring v = lower(value);
                const char* name = v == L"upper" ? "Upper" : v == L"lower" ? "Lower" : v == L"title" ? "Title" : v == L"sentence" ? "Sentence" : "AsIs";
                add("case", json_quote(name));
            }
            else if (key == L"start") add("counterStart", std::to_string(_wtoi(value.c_str())));
            else if (key == L"step") add("counterStep", std::to_string(_wtoi(value.c_str())));
            else {
                throw FormatError("'" + to_utf8(key) + "' is not a rename-rule setting. Try template, find, replace, regex, matchcase, scope, case, start, step.");
            }
        }
        json += "}";
        const std::vector<std::wstring> srcs = sources_for(L"", "rename-rule");
        std::vector<BbStr> views_;
        for (const auto& p : srcs) views_.push_back(core::s(p));
        const std::wstring rule = core::wide(json);
        uint64_t op = 0;
        core::check(bb_op_rename_rule(s_.core().app(), views_.data(), static_cast<uint32_t>(views_.size()), core::s(rule), &op), "rename-rule");
        await_op(op);
    };
    verbs_[L"clear-undo-history"] = [this](const std::wstring&) {
        core::check(bb_undo_clear(s_.core().app()), "clear-undo-history");
        s_.settle();
    };

    // ---- content search ----
    verbs_[L"assert-match"] = [this](const std::wstring& r) {
        const auto [name, expected] = split_first(r);
        if (name.empty() || expected.empty()) throw FormatError("assert-match needs a row name and some text.");
        constexpr uint32_t kMatchCell = 2;  // CELL_COLUMNS: Name, Folder, Match, ...
        auto listing = s_.core().listing(tab());
        const size_t row = row_index(name);
        const std::wstring actual = listing ? listing->cell(row, kMatchCell) : std::wstring();
        if (actual.empty()) {
            throw AssertionError("expected row '" + to_utf8(name) + "' to carry a content match containing '" + to_utf8(expected) +
                                 "', but it has none (or the row is not there).");
        }
        if (!core::icontains(actual, expected)) {
            throw AssertionError("expected '" + to_utf8(name) + "' to have matched on a line containing '" + to_utf8(expected) + "', got '" + to_utf8(actual) + "'.");
        }
    };

    // ---- comparing two panes ----
    verbs_[L"settle-content"] = [this](const std::wstring&) {
        std::vector<std::wstring> files;
        for (const std::wstring& p : s_.core().selection_paths(tab())) {
            std::error_code ec;
            if (!fs_::is_directory(p, ec)) files.push_back(p);
        }
        if (files.empty()) throw AssertionError("Settling by content needs files selected.");
        std::vector<BbStr> views_;
        for (const auto& p : files) views_.push_back(core::s(p));
        uint32_t identical = 0;
        const BbStatus st = bb_compare_settle(s_.core().app(), views_.data(), static_cast<uint32_t>(views_.size()), &identical);
        if (st != BB_OK) throw AssertionError(to_utf8(core::last_error()));
        say("SETTLED " + std::to_string(identical));
        s_.settle();
    };
    verbs_[L"assert-compare-row"] = [this](const std::wstring& r) {
        const auto [name, expected] = split_first(r);
        if (expected.empty()) throw FormatError("assert-compare-row wants a row name and a status.");
        constexpr uint32_t kStatusCell = 3;  // CELL_COLUMNS: Name, Folder, Match, CompareStatus, ...
        auto listing = s_.core().listing(tab());
        const size_t row = row_index(name);
        const std::wstring actual = listing ? listing->cell(row, kStatusCell) : std::wstring();
        if (lower(actual) != lower(trim(expected))) {
            throw AssertionError("'" + to_utf8(name) + "' reads '" + to_utf8(actual) + "', not '" + to_utf8(trim(expected)) + "'.");
        }
    };
}

void Runner::press_metadata(bb::views::MetadataPane* pane, int action, const std::string& what) {
    uint64_t op = 0;
    try {
        op = pane->press(action);
    } catch (const core::CoreError& e) {
        throw AssertionError("the metadata pane would not " + what + ": " + e.what());
    }
    await_op(op);
    s_.settle();
    pane->refresh();
}

}  // namespace bbh
