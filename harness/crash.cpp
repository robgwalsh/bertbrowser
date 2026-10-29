#include "crash.h"

#include <windows.h>

#include <dbghelp.h>

#include <cstdio>
#include <cstring>
#include <string>

namespace bbh {

namespace {

wchar_t g_dump_path[MAX_PATH * 2] = {};
LONG g_entered = 0;

void print_stack(const EXCEPTION_POINTERS* info) {
    HANDLE process = GetCurrentProcess();
    HANDLE thread = GetCurrentThread();
    SymSetOptions(SYMOPT_UNDNAME | SYMOPT_DEFERRED_LOADS | SYMOPT_LOAD_LINES);
    // The search path is the exe's own folder: the PDBs are built beside it.
    wchar_t exe[MAX_PATH] = {};
    GetModuleFileNameW(nullptr, exe, MAX_PATH);
    std::wstring dir(exe);
    dir = dir.substr(0, dir.find_last_of(L"\\/"));
    char dir_a[MAX_PATH * 2] = {};
    WideCharToMultiByte(CP_ACP, 0, dir.c_str(), -1, dir_a, sizeof dir_a - 1, nullptr, nullptr);
    SymInitialize(process, dir_a, TRUE);

    CONTEXT ctx = *info->ContextRecord;
    const DWORD64 pc0 = ctx.Rip;

    auto describe = [&](DWORD64 addr, char* out, size_t cap) {
        alignas(SYMBOL_INFO) char buf[sizeof(SYMBOL_INFO) + 512] = {};
        auto* sym = reinterpret_cast<SYMBOL_INFO*>(buf);
        sym->SizeOfStruct = sizeof(SYMBOL_INFO);
        sym->MaxNameLen = 511;
        DWORD64 disp = 0;
        char module[MAX_PATH] = "?";
        DWORD64 base = SymGetModuleBase64(process, addr);
        if (base) {
            IMAGEHLP_MODULE64 mi{};
            mi.SizeOfStruct = sizeof mi;
            if (SymGetModuleInfo64(process, addr, &mi)) std::snprintf(module, sizeof module, "%s", mi.ModuleName);
        }
        if (SymFromAddr(process, addr, &disp, sym)) {
            IMAGEHLP_LINE64 line{};
            line.SizeOfStruct = sizeof line;
            DWORD ld = 0;
            if (SymGetLineFromAddr64(process, addr, &ld, &line))
                std::snprintf(out, cap, "%s!%s+0x%llx (%s:%lu)", module, sym->Name, (unsigned long long)disp,
                              line.FileName, line.LineNumber);
            else
                std::snprintf(out, cap, "%s!%s+0x%llx", module, sym->Name, (unsigned long long)disp);
        } else {
            std::snprintf(out, cap, "%s+0x%llx", module, (unsigned long long)(addr - base));
        }
    };

    char first[1024];
    describe(pc0, first, sizeof first);
    char modoff[MAX_PATH + 32] = "?";
    if (DWORD64 base = SymGetModuleBase64(process, pc0)) {
        IMAGEHLP_MODULE64 mi{};
        mi.SizeOfStruct = sizeof mi;
        if (SymGetModuleInfo64(process, pc0, &mi))
            std::snprintf(modoff, sizeof modoff, "%s+0x%llx", mi.ModuleName, (unsigned long long)(pc0 - base));
    }
    std::printf("CRASH 0x%08lX at %s\n", info->ExceptionRecord->ExceptionCode, modoff);
    if (info->ExceptionRecord->ExceptionCode == EXCEPTION_ACCESS_VIOLATION &&
        info->ExceptionRecord->NumberParameters >= 2)
        std::printf("  access violation %s address 0x%llx\n",
                    info->ExceptionRecord->ExceptionInformation[0] == 0   ? "reading"
                    : info->ExceptionRecord->ExceptionInformation[0] == 1 ? "writing"
                                                                           : "executing",
                    (unsigned long long)info->ExceptionRecord->ExceptionInformation[1]);

    STACKFRAME64 frame{};
    frame.AddrPC = {ctx.Rip, 0, AddrModeFlat};
    frame.AddrFrame = {ctx.Rbp, 0, AddrModeFlat};
    frame.AddrStack = {ctx.Rsp, 0, AddrModeFlat};
    for (int i = 0; i < 160; ++i) {
        if (!StackWalk64(IMAGE_FILE_MACHINE_AMD64, process, thread, &frame, &ctx, nullptr, SymFunctionTableAccess64,
                         SymGetModuleBase64, nullptr))
            break;
        if (!frame.AddrPC.Offset) break;
        char text[1024];
        describe(frame.AddrPC.Offset, text, sizeof text);
        std::printf("  #%02d %s\n", i, text);
    }
}

LONG WINAPI filter(EXCEPTION_POINTERS* info) {
    if (InterlockedExchange(&g_entered, 1)) return EXCEPTION_CONTINUE_SEARCH;
    if (g_dump_path[0]) {
        HANDLE file = CreateFileW(g_dump_path, GENERIC_WRITE, 0, nullptr, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
        if (file != INVALID_HANDLE_VALUE) {
            MINIDUMP_EXCEPTION_INFORMATION mei{GetCurrentThreadId(), info, FALSE};
            MiniDumpWriteDump(GetCurrentProcess(), GetCurrentProcessId(), file,
                              static_cast<MINIDUMP_TYPE>(MiniDumpWithThreadInfo | MiniDumpWithIndirectlyReferencedMemory |
                                                         MiniDumpWithUnloadedModules | MiniDumpWithDataSegs),
                              &mei, nullptr, nullptr);
            CloseHandle(file);
            std::printf("CRASH dump: %ls\n", g_dump_path);
        }
    }
    print_stack(info);
    std::fflush(stdout);
    TerminateProcess(GetCurrentProcess(), info->ExceptionRecord->ExceptionCode);
    return EXCEPTION_EXECUTE_HANDLER;
}

}  // namespace

void install_crash_handler(const std::filesystem::path& dump_dir) {
    std::error_code ec;
    std::filesystem::create_directories(dump_dir, ec);
    const std::wstring p = (dump_dir / L"crash.dmp").wstring();
    wcsncpy_s(g_dump_path, p.c_str(), _TRUNCATE);
    SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX);
    SetUnhandledExceptionFilter(filter);
}

}  // namespace bbh
