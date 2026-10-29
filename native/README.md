# native

The Rust + C++ rewrite of BertBrowser, built side by side with the C# app until it reaches full parity.

## Build

Prerequisites: Visual Studio 2022 with the Desktop development with C++ workload (MSVC, CMake, Ninja,
Windows SDK), Rust stable x86_64-pc-windows-msvc, `cargo install cbindgen`.

```powershell
cd native
cargo test --workspace          # Rust: every crate
.\build.ps1                     # C++: enters the VS developer shell, configures, builds (debug)
.\build.ps1 -Config release
```

`build.ps1` runs `cargo build -p bb-ffi` and `cbindgen` as part of the CMake build, so the C++ tree
always links the current Rust code. The header `include/bbcore.h` is generated and committed.

## Layout

- `crates/bb-paths`, `bb-core`, `bb-shell`, `bb-app`: the Rust libraries, in dependency order.
- `crates/bb-ffi`: the C ABI staticlib `bbcore` that cbindgen reads.
- `crates/bb-indexer`, `bb-elevator`: the two elevated helper exes.
- `crates/bb-bench`: the benchmark driver.
- `app/`: the C++ UI as a static library `bbapp` plus `BertBrowser.exe` (WinMain only).
- `harness/`: `bb-harness.exe`, the offscreen UI harness that runs `tools/ui/*.bbs`.
