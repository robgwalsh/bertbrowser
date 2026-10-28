//! Regenerates `native/docs/performance.md` from every committed baseline. Spec: Cli/ReportCommand.cs.

use bb_core::benchmarking::{BenchResultsJson, PerformanceDoc};

use crate::args::{ArgList, Fail, unknown};
use crate::env;

/// The "how to run" block the document opens with, then what is different about the native numbers.
const COMMANDS: &str = r#"```powershell
cd native
.\build.ps1 -Config release
cargo build --release -p bb-bench
$bench = "target\release\bb-bench.exe"

& $bench run                                    # Tier A, default job (long; --filter *pathkey* to narrow)
& $bench run --job short                        # Tier A, quick pass
& $bench ui --repeat 5                          # Tier B, each tools/ui/bench-*.bbs five times through bb-harness
& $bench ui --scenario memory                   # Tier B, just the memory scenario (idle, 10k, 20k flat, soaks)
& $bench startup --launches 10                  # Tier C, the real BertBrowser.exe: time and memory (a window is shown offscreen)

& $bench compare --baseline bench\baselines\<key>.json --current bench-results\current.json
& $bench baseline --from bench-results\current.json --from bench-results\ui.json --from bench-results\startup.json
& $bench report                                 # this file
```

How to read the native numbers next to the .NET ones in the repository's `docs/performance.md`:

- **Allocated/op** is exact: a counting global allocator sums the bytes requested by one operation, so it
  does not vary between runs. **Gen0/kop** has no collector behind it here and reads *allocator calls per
  thousand operations*.
- **Managed heap** in the Tier B and C snapshots is the bytes the process heap holds (`HeapSummary` /
  the global allocator), taken after the layout cache is dropped -- the native counterpart of "after a
  full collection".
- The synthetic corpus has the same shape as the .NET one (100,000 rows under `Q:`, the same queries
  and sizes) but is cut from this crate's own random stream, so rows are the same kind of disk, not the
  same disk.
- Tier A has no counterpart yet for the archive index, MFT builder, change log, folder comparison and
  content scan suites; the digests run over the 64 MB file through the public API, since the sinks are
  crate-private."#;

pub fn execute(mut args: ArgList) -> Result<i32, Fail> {
    let mut check = false;
    let mut repo: Option<String> = None;

    while let Some(arg) = args.next_arg() {
        match arg.as_str() {
            "--check" => check = true,
            "--repo" => repo = Some(args.value(&arg)?),
            _ => return Err(unknown("report", &arg)),
        }
    }

    let repo = env::Repo::find(repo.as_deref())?;
    let dir = repo.baselines_dir();
    let mut files: Vec<_> = if dir.is_dir() {
        std::fs::read_dir(&dir)?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .collect()
    } else {
        Vec::new()
    };
    files.sort();
    let baselines = files
        .iter()
        .map(|f| BenchResultsJson::read(f))
        .collect::<Result<Vec<_>, _>>()?;

    let markdown = PerformanceDoc::render(&baselines, COMMANDS);
    let path = repo.performance_doc();

    if check {
        let existing = std::fs::read_to_string(&path).unwrap_or_default();
        if normalise(&existing) == normalise(&markdown) {
            println!("{} is up to date.", path.display());
            return Ok(0);
        }
        eprintln!(
            "{} is out of date; run `bb-bench report` and commit it.",
            path.display()
        );
        return Ok(1);
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, markdown)?;
    println!(
        "Wrote {} from {} baseline file(s).",
        path.display(),
        baselines.len()
    );
    Ok(0)
}

fn normalise(text: &str) -> String {
    text.replace("\r\n", "\n").trim_end().to_string()
}
