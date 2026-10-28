// Runs a .bbs script against a Session (tools/BertBrowser.Harness/ScriptRunner*.cs). The language is the
// C# harness's, verb for verb: a line is `verb rest-of-line`, `#` starts a comment at the front of a line,
// every command echoes `OK <line>` or `FAIL <line> - <why>`.
#pragma once
#include "sandbox.h"
#include "session.h"
#include "ui/popup_layer.h"

#include <functional>
#include <memory>
#include <map>
#include <unordered_map>

namespace bbh {

class Runner {
public:
    Runner(Session& session, const Options& options);
    // Runs every command; returns the process exit code.
    int run();

private:
    using Handler = std::function<void(const std::wstring&)>;

    void execute(const std::wstring& line);
    void install();
    void install_asserts();
    void install_bench();
    void install_ui();
    void install_panes();

    // ---- helpers ----
    static std::pair<std::wstring, std::wstring> split_first(const std::wstring& line);
    static std::pair<std::wstring, std::wstring> split_on(const std::wstring& line, const std::wstring& sep, const char* verb);
    static std::wstring require(const std::wstring& rest, const char* verb);
    static int number(const std::wstring& text, const char* verb);
    static double fraction(const std::wstring& text, const char* verb);
    static bool on_off(const std::wstring& text, const char* verb);
    static std::vector<std::wstring> csv(const std::wstring& text);

    uint64_t tab() { return s_.tab(); }
    std::vector<std::wstring> row_names();
    size_t row_index(const std::wstring& name);
    std::wstring row_path(const std::wstring& name);
    std::vector<std::wstring> sources_for(const std::wstring& names, const char* verb);
    void invoke_settled(const std::function<void()>& fn);
    // The side panes (runner_panes.cpp).
    void settle_preview();
    bb::views::PreviewPane* preview_pane();
    bb::views::MetadataPane* metadata_pane();
    void press_metadata(bb::views::MetadataPane* pane, int action, const std::string& what);
    void await_op(uint64_t op);
    void transfer(const std::wstring& rest, bool move);
    void erase(const std::wstring& rest, BbDeleteMode mode);
    void undo_step(int steps);
    void select_names(const std::wstring& names);
    void shot(const std::wstring& rest);
    std::string state_json();
    void search(const std::wstring& rest, bool global);
    void split_pane(const std::wstring& rest);
    // Photographs loose menu items (nothing is opened) to <name>.png in the output directory.
    void shot_menu(const bb::ui::MenuItems& items, const std::wstring& name);
    // Writes a bitmap that something rendered detached.
    void write_shot(bb::gfx::BitmapSurface& bitmap, const std::wstring& name);

    // ---- bench ----
    void record_time(const std::wstring& name, double ms);
    void append_line(const std::string& json);
    void time_verb(const std::wstring& rest);
    void mem_verb(const std::wstring& rest);
    void assert_mem(const std::wstring& rest);
    void bench_fixture(const std::wstring& rest);
    struct MemorySample {
        uint64_t managed = 0, working_set = 0, private_bytes = 0, peak_working_set = 0;
        int64_t items = 0, realized = 0, retained_thumbnails = 0;
    };
    MemorySample read_memory();

    // The state the window-feature verbs (runner_ui.cpp) keep between commands.
    struct UiState;
    std::shared_ptr<UiState> ui_;

    Session& s_;
    const Options& o_;
    Sandbox sandbox_;
    int shots_ = 0;
    std::unordered_map<std::wstring, Handler> verbs_;
    std::map<std::wstring, MemorySample> memory_;
};

}  // namespace bbh
