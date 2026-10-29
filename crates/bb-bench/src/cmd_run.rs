//! Tier A: runs the micro-benchmark suite and writes one results file. Spec: Cli/RunCommand.cs.

use std::path::PathBuf;
use std::time::Instant;

use bb_core::FileTime;
use bb_core::benchmarking::{
    BenchMarkdown, BenchResultSet, BenchResultsJson, CURRENT_SCHEMA_VERSION, RunInfo,
};

use crate::args::{ArgList, Fail, unknown};
use crate::runner::{Job, Suite};
use crate::suite::{self, Ctx};
use crate::{corpus, env};

pub fn execute(mut args: ArgList) -> Result<i32, Fail> {
    let mut job = "default".to_string();
    let mut filters = Vec::new();
    let mut scale = 1;
    let mut real = false;
    let mut refresh_real = false;
    let mut out: Option<String> = None;
    let mut machine_key: Option<String> = None;
    let mut repo: Option<String> = None;
    let mut ci = false;
    let mut allow_debug = false;

    while let Some(arg) = args.next_arg() {
        match arg.as_str() {
            "--job" => job = args.value(&arg)?,
            "--filter" => filters.push(args.value(&arg)?),
            "--scale" => scale = args.int(&arg)?.max(1),
            "--real" => real = true,
            "--refresh-real" => {
                real = true;
                refresh_real = true;
            }
            "--out" => out = Some(args.value(&arg)?),
            "--machine-key" => machine_key = Some(args.value(&arg)?),
            "--repo" => repo = Some(args.value(&arg)?),
            "--ci" => ci = true,
            "--allow-debug" => allow_debug = true,
            _ => return Err(unknown("run", &arg)),
        }
    }
    if ci {
        job = "short".to_string();
    }
    let job_config = Job::parse(&job)
        .ok_or_else(|| Fail::Usage(format!("--job wants default or short, got '{job}'.")))?;
    let repo = env::Repo::find(repo.as_deref())?;

    if cfg!(debug_assertions) && !allow_debug {
        eprintln!(
            "This is a Debug build. Benchmark numbers from unoptimised code are not worth recording; build with --release, or pass --allow-debug to run anyway."
        );
        return Ok(2);
    }

    let corpus_root = corpus::default_root();
    let say = |line: &str| println!("{line}");
    let mut real_config = None;
    if real {
        let config_path = repo.real_config();
        if !config_path.exists() {
            eprintln!(
                "--real needs {}; see bench/README.md for its shape.",
                config_path.display()
            );
            return Ok(2);
        }
        // The backup is taken here, once: the benchmarks only ever read the copy.
        let mut config = corpus::load_real(&config_path)?;
        let copy = corpus::ensure_backup(&config, &corpus_root, &say, refresh_real)?;
        config.db_copy_path = Some(copy.to_string_lossy().to_string());
        corpus::write_resolved(&config, &corpus_root)?;
        real_config = Some(config);
    }

    // The synthetic corpus is only needed if something selected reads it, but building it is the
    // once-only cost the next run saves, so a missing one is built whatever the filter.
    if real_config.is_none() {
        corpus::ensure_all(&corpus_root, scale, &say, false)?;
    }

    let ctx = Ctx::new(scale, corpus_root, real_config);
    let mut suite = Suite::new(&filters);
    suite::register(&mut suite, &ctx);
    if suite.is_empty() {
        eprintln!(
            "No benchmarks matched. Filters are globs over the benchmark id, for example --filter *pathkey*."
        );
        return Ok(2);
    }
    println!("# {} benchmark(s), job {}", suite.len(), job_config.name);

    let clock = Instant::now();
    let entries = suite.run(job_config, scale, &|line| println!("{line}"));

    let set = BenchResultSet {
        schema_version: CURRENT_SCHEMA_VERSION,
        generated_utc: FileTime::now(),
        tool: env::tool_version(),
        git: env::git(&repo.root),
        machine: env::machine(machine_key.as_deref()),
        run: RunInfo {
            job: job_config.name.to_string(),
            scale,
            real,
            tiers: vec!["A".to_string()],
            render_mode: None,
            repeat: None,
            toolchain: Some("counting-allocator".to_string()),
        },
        benchmarks: entries,
    };

    let out_path = out.map(PathBuf::from).unwrap_or_else(|| {
        repo.results_dir().join(if real {
            "current.real.json"
        } else {
            "current.json"
        })
    });
    BenchResultsJson::write(&set, &out_path)?;

    println!();
    println!("{}", BenchMarkdown::render_results(&set));
    println!(
        "# {} benchmarks in {:.1} min -> {}",
        set.benchmarks.len(),
        clock.elapsed().as_secs_f64() / 60.0,
        out_path.display()
    );

    let failed = set.benchmarks.iter().filter(|b| b.error.is_some()).count();
    if failed > 0 {
        eprintln!("# {failed} benchmark(s) failed; see the entries marked error.");
        return Ok(2);
    }
    Ok(0)
}
