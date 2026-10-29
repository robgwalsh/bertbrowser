# csharp

The frozen C#/WPF reference implementation of BertBrowser. It is **not shipped, not built by CI, and not
maintained**: the product is the native app under `native/`. It stays in the repository so behaviour can be
compared while parity is still being checked (`native/tools/parity/tests.ps1` maps each C# test to its Rust
counterpart; the offscreen scripts in `../tools/ui/*.bbs` run against either harness).

The layout is the one the code was written in, so nothing inside it moved relative to anything else:

- `src/` the app (`BertBrowser.App`), `BertBrowser.Core`, the two elevated helpers
- `tests/BertBrowser.Core.Tests` xUnit
- `tools/BertBrowser.Harness` (offscreen WPF harness), `tools/BertBrowser.Bench`, `tools/icon` (its icon pipeline)
- `bench/baselines` the recorded C# benchmark baselines

```powershell
dotnet build csharp/bertbrowser.sln
dotnet test csharp/tests/BertBrowser.Core.Tests --filter "FullyQualifiedName~PathKeyTests"
csharp/tools/BertBrowser.Harness/bin/Debug/net10.0-windows/BertBrowser.Harness.exe --script tools/ui/smoke.bbs
```

Never run `BertBrowser.exe` from here to look at it: use the harness. The `.bbs` scripts
are shared with the native app and live in the repository root (`../tools/ui`); its `docs/performance.md` is here.
