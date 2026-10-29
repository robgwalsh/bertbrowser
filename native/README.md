# native

The Rust + C++ BertBrowser. This tree is the app; `../csharp` holds the frozen C# version it replaced,
kept as a behavioural reference.

## Build

Prerequisites: Visual Studio 2022 with the Desktop development with C++ workload (MSVC, CMake, Ninja,
Windows SDK), Rust stable x86_64-pc-windows-msvc, `cargo install cbindgen`.

```powershell
cd native
cargo test --workspace          # Rust: every crate (PowerShell, not the Bash tool)
.\build.ps1                     # C++: enters the VS developer shell, configures, builds (debug)
.\build.ps1 -Config release
.\out\debug\harness\bb-harness.exe --script ..\tools\ui\smoke.bbs --out $env:TEMP\look
```

`build.ps1` runs `cargo build -p bb-ffi -p bb-indexer -p bb-elevator` and `cbindgen` as part of the CMake
build, so the C++ tree always links the current Rust code. The header `include/bbcore.h` is generated and
committed.

## Layout

- `crates/bb-paths`, `bb-core`, `bb-shell`, `bb-app`: the Rust libraries, in dependency order.
- `crates/bb-ffi`: the C ABI staticlib `bbcore` that cbindgen reads. `bb-ffi-harness` is the same ABI
  plus the test fixtures (`bbcore_harness.lib`), used by the harness only.
- `crates/bb-indexer`, `bb-elevator`: the two elevated helper exes.
- `crates/bb-bench`: the benchmark driver (`bench/` holds baselines, `docs/performance.md` the report).
- `app/`: the C++ UI as a static library `bbapp` plus `BertBrowser.exe` (WinMain only).
- `harness/`: `bb-harness.exe`, the offscreen UI harness that runs `tools/ui/*.bbs`.
- `tools/icons`: the icon pipeline (`icons.txt` -> `app/gfx/icons.gen.h`, `app/app/app.ico`).
- `tools/parity`: diffs the C# test suite against the Rust one.

See `../CLAUDE.md` for the rules that are easy to break.
