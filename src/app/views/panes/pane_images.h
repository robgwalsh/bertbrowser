// Pixels for the side panes, from the places only Windows can read them: the imaging component (WIC) for
// pictures, the shell for the preview of a document, and DirectWrite for the face inside a font file. All of
// it blocks, so the panes call it from a worker thread (COM is initialised on the thread that calls).
//
// **Nothing here holds a file open.** A file is read into memory with everything shared and closed before a
// pixel is decoded, which is the whole difference from Explorer's pane.
#pragma once
#include "ui/collections/tile_view.h"

#include <cstdint>
#include <string>
#include <vector>

namespace bb::views {

// The file's bytes, read whole (up to `limit`) with the handle closed again. False if it cannot be read.
bool read_whole_file(const std::wstring& path, uint64_t limit, std::vector<uint8_t>& out);

// A picture decoded to premultiplied BGRA, no wider than `max_width` (a smaller one is never scaled up).
// Null when no installed decoder knows the format; `width_out`/`height_out` are the file's own size.
ui::ThumbPtr decode_picture(const std::vector<uint8_t>& bytes, uint32_t max_width, uint32_t* width_out = nullptr,
                            uint32_t* height_out = nullptr);
ui::ThumbPtr decode_picture_file(const std::wstring& path, uint32_t max_width, uint32_t* width_out = nullptr,
                                 uint32_t* height_out = nullptr);

// What the shell can show of a file (a page of a document, a poster frame): null when it has nothing.
// Icons do not count.
ui::ThumbPtr shell_preview(const std::wstring& path, uint32_t px);

// The face inside a font file, for a specimen. `family`, `face` and the glyph count describe it; a face the
// system will not load comes back with `ok` false.
struct FontInfo {
    bool ok = false;
    std::wstring family, face, version;
    uint32_t glyphs = 0;
};
FontInfo read_font_info(const std::wstring& path);

}  // namespace bb::views
