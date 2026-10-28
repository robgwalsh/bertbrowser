//! Benchmark driver: run | ui | startup | compare | baseline | report | corpus.
//! Spec: tools/BertBrowser.Bench/**, Core/Benchmarking/*.
//!
//! One executable, several verbs: `run` (Tier A, a hand-rolled runner over bb-core with a counting
//! allocator), `ui` (Tier B, the offscreen harness), `startup` (Tier C, the real exe) and the
//! bookkeeping around them. Every tier writes the one schema in `bb_core::benchmarking`, and this is
//! the only program that writes it.

mod alloc;
mod args;
mod cmd_baseline;
mod cmd_compare;
mod cmd_corpus;
mod cmd_report;
mod cmd_run;
mod cmd_startup;
mod cmd_ui;
mod corpus;
mod env;
mod fake_disk;
mod rng;
mod runner;
mod suite;
mod synthetic;

use args::{ArgList, Fail};

#[global_allocator]
static ALLOCATOR: alloc::CountingAlloc = alloc::CountingAlloc;

pub const TOOL_NAME: &str = "bb-bench";

const USAGE: &str = "\
bb-bench - measures the native BertBrowser and keeps the evidence.

  run       Tier A: bb-core micro-benchmarks (hand-rolled runner, counting allocator)
              --job short|default      default: default; CI uses short
              --filter <glob>          over the benchmark id, case-blind, repeatable
                                       e.g. --filter *pathkey* --filter *fsindex*search*
              --scale <n>              synthetic corpus: n x 100,000 rows (default 1)
              --real                   read-only run against native/bench/real.local.json
              --out <file>             default native/bench-results/current[.real].json
              --machine-key <name>     file baselines under this key instead of the probed one
              --ci                     = --job short
              --allow-debug            a debug build is otherwise refused
  ui        Tier B: whole-UI scenarios through the offscreen harness (bb-harness)
              --scenario <name>        a tools/ui/bench-<name>.bbs, repeatable (default: all)
              --repeat <k>             runs per scenario, each a fresh process (default 5)
              --harness <exe>          default native/out/release/harness/bb-harness.exe
              --out <file>             default native/bench-results/ui.json
              --timeout <sec> --busy-timeout <ms>   forwarded (defaults 600 / 120000)
              --machine-key <name>
  startup   Tier C: the real BertBrowser.exe, launched against a scratch data directory
              --launches <n>           default 10; the first is cold and discarded
              --app <exe>              default native/out/release/app/BertBrowser.exe
              --allow-running          measure even though a real BertBrowser is open
              --out <file>             default native/bench-results/startup.json
              --machine-key <name>
  compare   --baseline <file> --current <file>
              [--time-threshold <pct>] [--alloc-threshold <pct>] [--memory-threshold <pct>]
              [--no-time] [--no-alloc] [--no-memory]
              [--strict] [--summary <md-file>] [--title <text>]
              --ci                     gates allocations only; writes $GITHUB_STEP_SUMMARY;
                                       with no baseline, writes a candidate and passes
  baseline  --from <file> (repeatable)  [--machine-key <name>]
            records or replaces native/bench/baselines/<key>[.real].json, merging tiers
  report    [--check]                   regenerates native/docs/performance.md from the baselines
  corpus    [--scale <n>] [--rebuild] [--stats]   builds or inspects the synthetic corpus

  --repo <dir>  on any verb: the repository root or its native folder (default: found from the exe)

Exit codes: 0 ok | 1 regression | 2 benchmark errors or environment | 64 usage";

fn run(args: Vec<String>) -> Result<i32, Fail> {
    let Some((verb, rest)) = args.split_first() else {
        println!("{USAGE}");
        return Ok(64);
    };
    if matches!(verb.as_str(), "--help" | "-h" | "/?" | "help") {
        println!("{USAGE}");
        return Ok(0);
    }
    let rest = ArgList::new(rest.to_vec());
    match verb.as_str() {
        "run" => cmd_run::execute(rest),
        "ui" => cmd_ui::execute(rest),
        "startup" => cmd_startup::execute(rest),
        "compare" => cmd_compare::execute(rest),
        "baseline" => cmd_baseline::execute(rest),
        "report" => cmd_report::execute(rest),
        "corpus" => cmd_corpus::execute(rest),
        other => Err(Fail::Usage(format!("Unknown command '{other}'."))),
    }
}

fn main() {
    let code = match run(std::env::args().skip(1).collect()) {
        Ok(code) => code,
        Err(Fail::Usage(message)) => {
            eprintln!("{message}\n\n{USAGE}");
            64
        }
        Err(Fail::Error(message)) => {
            eprintln!("error: {message}");
            2
        }
    };
    std::process::exit(code);
}
