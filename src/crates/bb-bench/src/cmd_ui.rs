//! Tier B: runs each `tools/ui/bench-*.bbs` through the offscreen harness several times, each in a
//! fresh process, and aggregates the `time` and `mem` samples it emits. Spec: Cli/UiCommand.cs.
//!
//! A fresh process per run is deliberate: every run is a cold app with an empty scratch data
//! directory, so the K samples of one scenario are K independent measurements rather than K laps of
//! one warming cache. A scenario's parameters travel in its script, on a `#!` line, so the result
//! carries its denominator without this tool knowing anything about the fixture.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use bb_core::FileTime;
use bb_core::benchmarking::{
    BenchEntry, BenchMarkdown, BenchResultSet, BenchResultsJson, CURRENT_SCHEMA_VERSION,
    ProcessStats, RunInfo, TimeStats,
};

use crate::args::{ArgList, Fail, unknown};
use crate::env;

const SCRIPT_PREFIX: &str = "bench-";

#[derive(Clone, Debug, PartialEq)]
pub enum Sample {
    Time { name: String, ms: f64 },
    Mem { name: String, process: ProcessStats },
}

impl Sample {
    fn name(&self) -> &str {
        match self {
            Sample::Time { name, .. } | Sample::Mem { name, .. } => name,
        }
    }
}

pub fn execute(mut args: ArgList) -> Result<i32, Fail> {
    let mut scenarios = Vec::new();
    let mut repeat = 5;
    let mut harness: Option<PathBuf> = None;
    let mut out: Option<String> = None;
    let mut repo: Option<String> = None;
    let mut machine_key: Option<String> = None;
    let mut timeout = 600;
    let mut busy_timeout = 120_000;

    while let Some(arg) = args.next_arg() {
        match arg.as_str() {
            "--scenario" => scenarios.push(args.value(&arg)?),
            "--repeat" => repeat = args.int(&arg)?.max(1),
            "--harness" => harness = Some(PathBuf::from(args.value(&arg)?)),
            "--out" => out = Some(args.value(&arg)?),
            "--repo" => repo = Some(args.value(&arg)?),
            "--machine-key" => machine_key = Some(args.value(&arg)?),
            "--timeout" => timeout = args.int(&arg)?,
            "--busy-timeout" => busy_timeout = args.int(&arg)?,
            _ => return Err(unknown("ui", &arg)),
        }
    }

    let repo = env::Repo::find(repo.as_deref())?;
    let harness =
        harness.unwrap_or_else(|| repo.release_out().join("harness").join("bb-harness.exe"));
    if !harness.exists() {
        eprintln!(
            "No harness at {}. Build with `.\\build.ps1 -Config release` from the repository root, or pass --harness.",
            harness.display()
        );
        return Ok(2);
    }

    let script_dir = repo.ui_scripts();
    let scripts: Vec<PathBuf> = if scenarios.is_empty() {
        let mut found: Vec<PathBuf> = std::fs::read_dir(&script_dir)
            .map_err(|e| Fail::Error(format!("{}: {e}", script_dir.display())))?
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                let name = p
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                name.starts_with(SCRIPT_PREFIX) && name.ends_with(".bbs")
            })
            .collect();
        found.sort();
        found
    } else {
        scenarios
            .iter()
            .map(|s| script_dir.join(format!("{SCRIPT_PREFIX}{}.bbs", s.trim_start_matches('-'))))
            .collect()
    };
    if let Some(missing) = scripts.iter().find(|s| !s.exists()) {
        return Err(Fail::Usage(format!(
            "No scenario script at {}.",
            missing.display()
        )));
    }
    if scripts.is_empty() {
        eprintln!(
            "No {SCRIPT_PREFIX}*.bbs scripts under {}.",
            script_dir.display()
        );
        return Ok(2);
    }

    let work = std::env::temp_dir()
        .join("bertbrowser-bench-native")
        .join("ui")
        .join(format!(
            "{}-{}",
            env::local_date_stamp(),
            std::process::id()
        ));
    std::fs::create_dir_all(&work)?;

    let clock = Instant::now();
    let mut entries = Vec::new();
    let mut any_failed = false;

    for script in &scripts {
        let scenario = script
            .file_stem()
            .map(|s| s.to_string_lossy()[SCRIPT_PREFIX.len()..].to_string())
            .unwrap_or_default();
        let mut parameters = read_parameters(script);
        parameters.insert("renderMode".to_string(), "Offscreen".to_string());
        parameters.insert("repeat".to_string(), repeat.to_string());

        println!("# {scenario}: {repeat} run(s)");
        let mut runs: Vec<Vec<Sample>> = Vec::new();
        let mut failure: Option<String> = None;

        for k in 1..=repeat {
            let run_dir = work.join(&scenario).join(k.to_string());
            let bench_out = run_dir.join("samples.jsonl");
            let log = run_dir.join("harness.log");
            std::fs::create_dir_all(&run_dir)?;
            let exit = run_harness(
                &harness,
                script,
                &run_dir,
                &bench_out,
                &log,
                timeout,
                busy_timeout,
            );
            if exit != 0 || !bench_out.exists() {
                failure = Some(format!(
                    "harness exit {exit} on run {k}; see {}",
                    log.display()
                ));
                break;
            }
            let samples = read_samples(&bench_out)?;
            println!(
                "#   run {k}: {}",
                samples
                    .iter()
                    .map(|s| match s {
                        Sample::Time { name, ms } => format!("{name} {ms:.0} ms"),
                        Sample::Mem { name, process } => format!(
                            "{name} {:.1}/{:.1}/{:.0} MB",
                            process.allocator_bytes as f64 / (1024.0 * 1024.0),
                            process.process_heap_bytes as f64 / (1024.0 * 1024.0),
                            process.private_bytes as f64 / (1024.0 * 1024.0)
                        ),
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            runs.push(samples);
        }

        if let Some(why) = failure {
            any_failed = true;
            eprintln!("# {scenario}: {why}");
            entries.push(BenchEntry {
                id: format!("ui.{scenario}"),
                tier: "B".to_string(),
                parameters,
                n: 0,
                time: None,
                memory: None,
                process: None,
                error: Some(why),
            });
            continue;
        }
        entries.extend(aggregate(&scenario, &parameters, &runs)?);
    }

    let set = BenchResultSet {
        schema_version: CURRENT_SCHEMA_VERSION,
        generated_utc: FileTime::now(),
        tool: env::tool_version(),
        git: env::git(&repo.root),
        machine: env::machine(machine_key.as_deref()),
        run: RunInfo {
            job: "ui".to_string(),
            scale: 1,
            real: false,
            tiers: vec!["B".to_string()],
            render_mode: Some("Offscreen".to_string()),
            repeat: Some(repeat),
            toolchain: None,
        },
        benchmarks: entries,
    };
    let out_path = out
        .map(PathBuf::from)
        .unwrap_or_else(|| repo.results_dir().join("ui.json"));
    BenchResultsJson::write(&set, &out_path)?;

    println!();
    println!("{}", BenchMarkdown::render_results(&set));
    println!(
        "# {} entries from {} scenario(s) in {:.1} min -> {}",
        set.benchmarks.len(),
        scripts.len(),
        clock.elapsed().as_secs_f64() / 60.0,
        out_path.display()
    );

    if any_failed {
        eprintln!(
            "# some scenarios failed; their logs are under {}",
            work.display()
        );
        return Ok(2);
    }
    let _ = std::fs::remove_dir_all(&work);
    Ok(0)
}

fn run_harness(
    harness: &Path,
    script: &Path,
    out_dir: &Path,
    bench_out: &Path,
    log: &Path,
    timeout: u32,
    busy_timeout: u32,
) -> i32 {
    let spawned = Command::new(harness)
        .arg("--script")
        .arg(script)
        .arg("--out")
        .arg(out_dir)
        .arg("--bench-out")
        .arg(bench_out)
        .arg("--timeout")
        .arg(timeout.to_string())
        .arg("--busy-timeout")
        .arg(busy_timeout.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let Ok(mut child) = spawned else {
        let _ = std::fs::write(log, format!("could not start {}\n", harness.display()));
        return -1;
    };

    // Drain both pipes on their own threads so a chatty harness cannot fill one and stall.
    let drain = |pipe: Option<Box<dyn std::io::Read + Send>>| {
        std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(mut p) = pipe {
                let mut bytes = Vec::new();
                let _ = p.read_to_end(&mut bytes);
                text = String::from_utf8_lossy(&bytes).to_string();
            }
            text
        })
    };
    let stdout = drain(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
    );
    let stderr = drain(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
    );

    let deadline = Instant::now() + Duration::from_secs(u64::from(timeout) + 30);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => break None,
        }
    };

    let text = stdout.join().unwrap_or_default() + &stderr.join().unwrap_or_default();
    match status {
        Some(status) => {
            let _ = std::fs::write(log, text);
            status.code().unwrap_or(-1)
        }
        None => {
            let _ = std::fs::write(
                log,
                format!("killed: the harness outlived its own watchdog\n{text}"),
            );
            -1
        }
    }
}

/// `#! key=value key=value` on any line of the script.
pub fn read_parameters(script: &Path) -> BTreeMap<String, String> {
    let mut parameters = BTreeMap::new();
    let Ok(text) = std::fs::read_to_string(script) else {
        return parameters;
    };
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("#!") else {
            continue;
        };
        for pair in rest.split_whitespace() {
            if let Some((k, v)) = pair.split_once('=')
                && !k.is_empty()
            {
                parameters.insert(k.to_string(), v.to_string());
            }
        }
    }
    parameters
}

/// One line of the harness's `--bench-out` file.
pub fn parse_sample(line: &str) -> Result<Sample, String> {
    let v: serde_json::Value = serde_json::from_str(line).map_err(|e| format!("{e}: {line}"))?;
    let text = |key: &str| v.get(key).and_then(|x| x.as_str()).map(str::to_string);
    let int = |key: &str| v.get(key).and_then(serde_json::Value::as_i64);
    let kind = text("kind").ok_or_else(|| format!("no kind: {line}"))?;
    let name = text("name").ok_or_else(|| format!("no name: {line}"))?;
    if kind == "time" {
        let ms = v
            .get("ms")
            .and_then(serde_json::Value::as_f64)
            .ok_or_else(|| format!("no ms: {line}"))?;
        return Ok(Sample::Time { name, ms });
    }
    let need = |key: &str| int(key).ok_or_else(|| format!("no {key}: {line}"));
    Ok(Sample::Mem {
        name,
        process: ProcessStats {
            working_set_bytes: need("workingSetBytes")?,
            private_bytes: need("privateBytes")?,
            allocator_bytes: need("allocatorBytes")?,
            process_heap_bytes: need("processHeapBytes")?,
            realized_rows: need("realized")? as i32,
            items: need("items")? as u32,
            retained_thumbnails: need("retainedThumbnails")? as u32,
            peak_working_set_bytes: int("peakWorkingSetBytes"),
        },
    })
}

fn read_samples(path: &Path) -> Result<Vec<Sample>, Fail> {
    let text = std::fs::read_to_string(path)?;
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| parse_sample(l).map_err(Fail::Error))
        .collect()
}

fn median(values: impl IntoIterator<Item = i64>) -> i64 {
    let mut sorted: Vec<i64> = values.into_iter().collect();
    sorted.sort_unstable();
    match sorted.len() {
        0 => 0,
        n if n % 2 == 1 => sorted[n / 2],
        n => (sorted[n / 2 - 1] + sorted[n / 2]) / 2,
    }
}

/// One entry per sample name: timings become `TimeStats`, memory readbacks become the median of
/// each field.
pub fn aggregate(
    scenario: &str,
    parameters: &BTreeMap<String, String>,
    runs: &[Vec<Sample>],
) -> Result<Vec<BenchEntry>, Fail> {
    let mut names: Vec<&str> = Vec::new();
    for run in runs {
        for sample in run {
            if !names.contains(&sample.name()) {
                names.push(sample.name());
            }
        }
    }

    let mut entries = Vec::new();
    for name in names {
        // Every run records its own startup; only the startup scenario is about it.
        if name == "startup.hosted" && scenario != "startup" {
            continue;
        }
        let id = format!("ui.{scenario}.{name}");
        let times: Vec<f64> = runs
            .iter()
            .flatten()
            .filter_map(|s| match s {
                Sample::Time { name: n, ms } if n == name => Some(*ms),
                _ => None,
            })
            .collect();
        let mems: Vec<&ProcessStats> = runs
            .iter()
            .flatten()
            .filter_map(|s| match s {
                Sample::Mem { name: n, process } if n == name => Some(process),
                _ => None,
            })
            .collect();

        if !times.is_empty() {
            entries.push(BenchEntry {
                id,
                tier: "B".to_string(),
                parameters: parameters.clone(),
                n: times.len() as u32,
                time: Some(TimeStats::from_samples(&times, "ms")?),
                memory: None,
                process: None,
                error: None,
            });
        } else if !mems.is_empty() {
            let process = ProcessStats {
                working_set_bytes: median(mems.iter().map(|m| m.working_set_bytes)),
                private_bytes: median(mems.iter().map(|m| m.private_bytes)),
                allocator_bytes: median(mems.iter().map(|m| m.allocator_bytes)),
                process_heap_bytes: median(mems.iter().map(|m| m.process_heap_bytes)),
                realized_rows: median(mems.iter().map(|m| i64::from(m.realized_rows))) as i32,
                items: median(mems.iter().map(|m| i64::from(m.items))) as u32,
                retained_thumbnails: median(mems.iter().map(|m| i64::from(m.retained_thumbnails)))
                    as u32,
                peak_working_set_bytes: if mems.iter().all(|m| m.peak_working_set_bytes.is_some()) {
                    Some(median(mems.iter().filter_map(|m| m.peak_working_set_bytes)))
                } else {
                    None
                },
            };
            entries.push(BenchEntry {
                id,
                tier: "B".to_string(),
                parameters: parameters.clone(),
                n: mems.len() as u32,
                time: None,
                memory: None,
                process: Some(process),
                error: None,
            });
        }
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_reads_the_harness_lines() {
        let time = parse_sample(r#"{"kind":"time","name":"list","ms":12.5}"#).unwrap();
        assert_eq!(
            time,
            Sample::Time {
                name: "list".into(),
                ms: 12.5
            }
        );
        let mem = parse_sample(
            r#"{"kind":"mem","name":"idle","allocatorBytes":100,"processHeapBytes":150,"workingSetBytes":300,"privateBytes":200,"peakWorkingSetBytes":400,"items":0,"realized":-1,"retainedThumbnails":0}"#,
        )
        .unwrap();
        match mem {
            Sample::Mem { process, .. } => {
                assert_eq!(process.allocator_bytes, 100);
                assert_eq!(process.process_heap_bytes, 150);
                assert_eq!(process.realized_rows, -1);
                assert_eq!(process.peak_working_set_bytes, Some(400));
            }
            Sample::Time { .. } => panic!("a memory line"),
        }
        assert!(parse_sample("garbage").is_err());
    }

    #[test]
    fn it_aggregates_medians_per_name_and_drops_foreign_startup() {
        let t = |name: &str, ms| Sample::Time {
            name: name.into(),
            ms,
        };
        let runs = vec![
            vec![t("list", 10.0), t("startup.hosted", 900.0)],
            vec![t("list", 30.0), t("startup.hosted", 950.0)],
            vec![t("list", 20.0), t("startup.hosted", 920.0)],
        ];
        let params = BTreeMap::new();
        let entries = aggregate("listing-10k", &params, &runs).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "ui.listing-10k.list");
        assert_eq!(entries[0].n, 3);
        assert_eq!(entries[0].time.as_ref().unwrap().median, 20.0);
        assert_eq!(aggregate("startup", &params, &runs).unwrap().len(), 2);
    }

    #[test]
    fn script_parameters_come_from_hash_bang_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bench-x.bbs");
        std::fs::write(&path, "#! files=10000 dirs=50\n# a comment\nmem a\n").unwrap();
        let p = read_parameters(&path);
        assert_eq!(p.get("files").map(String::as_str), Some("10000"));
        assert_eq!(p.get("dirs").map(String::as_str), Some("50"));
    }
}
