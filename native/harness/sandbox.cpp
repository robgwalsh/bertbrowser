#include "sandbox.h"

#include <windows.h>

#include <algorithm>
#include <fstream>

namespace bbh {

namespace {

bool starts_with_nocase(const std::wstring& s, const std::wstring& prefix) {
    return s.size() >= prefix.size() && _wcsnicmp(s.c_str(), prefix.c_str(), prefix.size()) == 0;
}

void set_times(const fs::path& p, const FILETIME& ft) {
    HANDLE h = CreateFileW(p.c_str(), FILE_WRITE_ATTRIBUTES, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, nullptr,
                           OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS, nullptr);
    if (h == INVALID_HANDLE_VALUE) return;
    SetFileTime(h, nullptr, nullptr, &ft);
    CloseHandle(h);
}

}  // namespace

fs::path Sandbox::resolve(const std::wstring& path) const {
    const fs::path p(path);
    if (p.has_root_name() && p.has_root_directory()) return fs::absolute(p).lexically_normal();
    return fs::absolute(root_ / p).lexically_normal();
}

fs::path Sandbox::require_inside(const std::wstring& path, const std::string& verb) const {
    const fs::path full = resolve(path);
    if (allow_outside_) return full;
    std::wstring root = root_.wstring();
    while (!root.empty() && (root.back() == L'\\' || root.back() == L'/')) root.pop_back();
    const std::wstring f = full.wstring();
    if (!starts_with_nocase(f, root + L"\\") && _wcsicmp(f.c_str(), root.c_str()) != 0) {
        throw std::runtime_error(verb + " would touch '" + to_utf8(f) + "', which is outside the run's sandbox (" +
                                 to_utf8(root_.wstring()) +
                                 "). Build fixtures with 'tree'/'mkdir'/'write', or pass --allow-outside if you mean it.");
    }
    return full;
}

void Sandbox::write(const fs::path& path, size_t bytes) {
    if (path.has_parent_path()) fs::create_directories(path.parent_path());
    if (fs::exists(path)) {
        const DWORD a = GetFileAttributesW(path.c_str());
        if (a != INVALID_FILE_ATTRIBUTES) SetFileAttributesW(path.c_str(), a & ~(FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_READONLY));
    }
    const std::string seed = to_utf8(path.filename().wstring()) + "\n";
    std::string text;
    text.reserve(bytes + seed.size());
    while (text.size() < bytes) text += seed;
    text.resize(std::max(bytes, seed.size()));
    std::ofstream out(path, std::ios::binary | std::ios::trunc);
    if (!out) throw std::runtime_error("cannot write " + to_utf8(path.wstring()));
    out.write(text.data(), static_cast<std::streamsize>(text.size()));
}

void Sandbox::stamp(const fs::path& root) {
    SYSTEMTIME st{};
    st.wYear = 2024;
    st.wMonth = 3;
    st.wDay = 14;
    st.wHour = 9;
    st.wMinute = 26;
    st.wSecond = 53;
    FILETIME local{}, utc{};
    SystemTimeToFileTime(&st, &local);  // read as local wall-clock time
    LocalFileTimeToFileTime(&local, &utc);
    std::error_code ec;
    for (fs::recursive_directory_iterator it(root, fs::directory_options::skip_permission_denied, ec), end; !ec && it != end;
         it.increment(ec)) {
        set_times(it->path(), utc);
    }
    set_times(root, utc);
}

fs::path Sandbox::populate(const std::wstring& relative) const {
    const fs::path root = resolve(relative);
    fs::create_directories(root);
    write(root / L"notes.txt", 240);
    write(root / L"report.md", 1100);
    write(root / L"budget.xlsx", 18400);
    write(root / L"photo.jpg", 96000);
    write(root / L"archive.zip", 512000);

    const fs::path documents = root / L"Documents";
    fs::create_directories(documents);
    write(documents / L"letter.txt", 800);
    write(documents / L"report.md", 2400);  // a duplicate name, for search results
    write(documents / L"draft.md", 640);

    const fs::path nested = documents / L"Archive";
    fs::create_directories(nested);
    write(nested / L"report.md", 300);  // three levels down, so search must recurse
    write(nested / L"old.txt", 120);

    const fs::path pictures = root / L"Pictures";
    fs::create_directories(pictures);
    write(pictures / L"holiday.jpg", 240000);
    write(pictures / L"diagram.png", 44000);

    fs::create_directories(root / L"Empty");

    const fs::path hidden_file = root / L".hidden-notes.txt";
    write(hidden_file, 64);
    SetFileAttributesW(hidden_file.c_str(), GetFileAttributesW(hidden_file.c_str()) | FILE_ATTRIBUTE_HIDDEN);

    const fs::path hidden_dir = root / L"HiddenFolder";
    fs::create_directories(hidden_dir);
    write(hidden_dir / L"secret.txt", 32);
    SetFileAttributesW(hidden_dir.c_str(), GetFileAttributesW(hidden_dir.c_str()) | FILE_ATTRIBUTE_HIDDEN);

    stamp(root);
    return root;
}

}  // namespace bbh
