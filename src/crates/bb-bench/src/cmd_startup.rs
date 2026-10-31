//! Tier C: launches the built `BertBrowser.exe` several times against a scratch data directory and
//! reads back the marks the app writes under `BERTBROWSER_STARTUP_TRACE`. Spec: Cli/StartupCommand.cs
//! and Core/Benchmarking/StartupTraceData.cs.
//!
//! The one place anything in this repository starts the real executable, and sanctioned only
//! because the user asked for this tier knowing a window appears. The app parks it offscreen and
//! never activates it. It refuses while a real BertBrowser is running: the single-instance claim is
//! per user, so the launch would be a "second" copy and would measure a different path. The first
//! launch creates and migrates the scratch database and is discarded as cold.
//!
//! The environment contract is the C# app's, unchanged: `BERTBROWSER_DATA_DIR` (the scratch data
//! root), `BERTBROWSER_STARTUP_TRACE` (the file the trace is written to) and
//! `BERTBROWSER_EXIT_AFTER_STARTUP` (exit once the first listing is shown).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use bb_core::FileTime;
use bb_core::benchmarking::{
    BenchEntry, BenchMarkdown, BenchResultSet, BenchResultsJson, CURRENT_SCHEMA_VERSION,
    ProcessStats, RunInfo, StartupMarks, StartupMemory, StartupMemoryPoints, StartupTraceData,
    StartupTraceJson, TimeStats,
};
use bb_core::layout::{SessionLayout, SessionTab, SplitOrientation};
use bb_core::settings::{AppPaths, AppSettings};

use crate::args::{ArgList, Fail, unknown};
use crate::{corpus, env};

const DEFAULT_LAUNCHES: u32 = 10;
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(90);
/// A million index rows: `--session`'s database, sized like a person's rather than like an empty one.
const SESSION_DB_SCALE: u32 = 10;

pub fn execute(mut args: ArgList) -> Result<i32, Fail> {
    let mut launches = DEFAULT_LAUNCHES;
    let mut app: Option<PathBuf> = None;
    let mut out: Option<String> = None;
    let mut repo: Option<String> = None;
    let mut machine_key: Option<String> = None;
    let mut allow_running = false;
    let mut affinity: Option<usize> = None;
    let mut session = false;

    while let Some(arg) = args.next_arg() {
        match arg.as_str() {
            "--launches" => launches = args.int(&arg)?.max(2),
            "--app" => app = Some(PathBuf::from(args.value(&arg)?)),
            "--out" => out = Some(args.value(&arg)?),
            "--repo" => repo = Some(args.value(&arg)?),
            "--machine-key" => machine_key = Some(args.value(&arg)?),
            "--allow-running" => allow_running = true,
            "--affinity" => affinity = Some(parse_affinity(&args.value(&arg)?)?),
            "--session" => session = true,
            _ => return Err(unknown("startup", &arg)),
        }
    }

    let repo = env::Repo::find(repo.as_deref())?;
    let app = app.unwrap_or_else(|| repo.release_out().join("app").join("BertBrowser.exe"));
    if !app.exists() {
        eprintln!(
            "No app at {}. Build with `.\\build.ps1 -Config release` from the repository root, or pass --app.",
            app.display()
        );
        return Ok(2);
    }
    if env::is_running("BertBrowser.exe") && !allow_running {
        eprintln!(
            "A BertBrowser is running. The single-instance claim is per user, so a measured launch would start as a second copy and take a different path. Close it, or pass --allow-running to measure that path on purpose."
        );
        return Ok(2);
    }
    if env::is_running("bb-indexer.exe") || env::is_running("BertBrowser.Indexer.exe") {
        println!(
            "# note: the index helper is running. A traced launch does not attach to it, but it is tailing the journal."
        );
    }

    // A child inherits this, which leaves nothing to race between starting it and restricting it.
    if let Some(mask) = affinity {
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, SetProcessAffinityMask};
        // SAFETY: a plain call on this process's own pseudo-handle.
        if unsafe { SetProcessAffinityMask(GetCurrentProcess(), mask) } == 0 {
            return Err(Fail::Usage(format!(
                "--affinity {mask:#x} is not a mask of this machine's processors."
            )));
        }
    }

    let corpus_root = corpus::default_root();
    let say = |line: &str| println!("{line}");
    let tree = corpus::ensure_tree(&corpus_root, 1, &say, false)?;
    let session_db = if session {
        Some(corpus::ensure_db(
            &corpus_root,
            SESSION_DB_SCALE,
            &say,
            false,
        )?)
    } else {
        None
    };

    let work = corpus_root.join("startup");
    let state = work.join("state");
    let traces = work.join("traces");
    if work.exists() {
        std::fs::remove_dir_all(&work)?;
    }
    std::fs::create_dir_all(&state)?;
    std::fs::create_dir_all(&traces)?;

    println!(
        "# {launches} launches of {}, first discarded; data dir {}",
        app.display(),
        state.display()
    );

    let mut kept: Vec<(StartupTraceData, f64)> = Vec::new();
    let mut failure: Option<String> = None;

    for k in 0..launches {
        let trace_path = traces.join(format!("trace-{k:02}.json"));
        // The discarded launch makes the data directory one this build owns (it would move the seeded
        // files aside as an older version's otherwise), and only then is there a session to come back to.
        let named = (session_db.is_none() || k == 0).then_some(tree.startup.as_path());
        let result = launch(&app, named, &state, &trace_path);
        let (trace, lifetime_ms) = match result {
            Ok(r) => r,
            Err(e) => {
                failure = Some(format!("launch {k}: {e}"));
                break;
            }
        };

        let first = trace.marks_ms.get(StartupMarks::FIRST_LISTING).map_or_else(
            || "no first listing".to_string(),
            |ms| format!("{ms:.0} ms"),
        );
        let held = trace
            .memory
            .as_ref()
            .and_then(|m| m.get(StartupMemoryPoints::FIRST_LISTING))
            .map_or_else(String::new, |m| {
                format!(
                    ", {:.0} MB working set",
                    m.working_set_bytes as f64 / (1024.0 * 1024.0)
                )
            });
        println!(
            "#   launch {k}{}: first listing at {first}{held}, exit after {lifetime_ms:.0} ms, instance {}{}",
            if k == 0 { " (cold, discarded)" } else { "" },
            trace.instance,
            trace
                .error
                .as_ref()
                .map_or_else(String::new, |e| format!(", error: {e}"))
        );
        if let Some(e) = &trace.error {
            failure = Some(format!("launch {k}: {e}"));
            break;
        }
        if k > 0 {
            kept.push((trace, lifetime_ms));
        }
        if k == 0
            && let Some(db) = &session_db
        {
            seed_session(&state, db, &tree)?;
        }
    }

    // Closing saves what the window held. Had a launch fallen back to one pane, the numbers would still look
    // fine and would be measuring the wrong thing.
    if session_db.is_some() && failure.is_none() && !session_came_back(&state) {
        failure = Some("the launches did not restore the seeded session".to_string());
    }

    let mut parameters: BTreeMap<String, String> = [
        ("launches", launches.to_string()),
        ("discarded", "1".to_string()),
        (
            "startFolder",
            if session { "session" } else { "Startup(500)" }.to_string(),
        ),
        ("parked", "true".to_string()),
        (
            "app",
            app.strip_prefix(&repo.root)
                .unwrap_or(&app)
                .to_string_lossy()
                .to_string(),
        ),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();

    let entries = match &failure {
        Some(why) => {
            eprintln!("# {why}; traces under {}", traces.display());
            vec![BenchEntry {
                id: "startup.firstListing".to_string(),
                tier: "C".to_string(),
                parameters,
                n: 0,
                time: None,
                memory: None,
                process: None,
                error: Some(why.clone()),
            }]
        }
        None => {
            parameters.insert("instance".to_string(), dominant_instance(&kept));
            if let Some(mask) = affinity {
                parameters.insert("affinity".to_string(), format!("{mask:#x}"));
            }
            parameters.extend(page_fault_parameters(&kept));
            let mut entries = entries_of(&kept, &parameters)?;
            if session {
                // Its own ids: a launch into a session is never compared with one into a named folder.
                for entry in &mut entries {
                    entry.id = entry.id.replacen("startup.", "startup.session.", 1);
                }
            }
            entries
        }
    };

    let set = BenchResultSet {
        schema_version: CURRENT_SCHEMA_VERSION,
        generated_utc: FileTime::now(),
        tool: env::tool_version(),
        git: env::git(&repo.root),
        machine: env::machine(machine_key.as_deref()),
        run: RunInfo {
            job: if session {
                "startup-session"
            } else {
                "startup"
            }
            .to_string(),
            scale: 1,
            real: false,
            tiers: vec!["C".to_string()],
            render_mode: None,
            repeat: Some(launches),
            toolchain: None,
        },
        benchmarks: entries,
    };
    let out_path = out.map(PathBuf::from).unwrap_or_else(|| {
        repo.results_dir().join(if session {
            "startup-session.json"
        } else {
            "startup.json"
        })
    });
    BenchResultsJson::write(&set, &out_path)?;

    println!();
    println!("{}", BenchMarkdown::render_results(&set));
    println!("# -> {}", out_path.display());

    if failure.is_none() {
        let _ = std::fs::remove_dir_all(&work);
    }
    Ok(if failure.is_none() { 0 } else { 2 })
}

/// What a person comes back to: two panes, three tabs (one of them ten thousand files, showing in neither pane's
/// front), and a database with an index in it. Every launch after this one restores it.
fn seed_session(data_dir: &Path, db: &Path, tree: &corpus::TreePaths) -> Result<(), Fail> {
    let tab = |path: &Path| SessionTab {
        path: path.to_string_lossy().to_string(),
        sort_by: None,
        sort_descending: false,
        is_flat: false,
        columns: None,
    };
    let pane = |tabs: Vec<SessionTab>, active: bool| SessionLayout {
        orientation: None,
        children: None,
        tabs: Some(tabs),
        is_active_pane: active,
        ..SessionLayout::default()
    };
    let paths = AppPaths::new(data_dir);
    let mut settings = AppSettings::load(&paths.settings_path());
    settings.session = Some(SessionLayout {
        orientation: Some(SplitOrientation::Vertical),
        children: Some(vec![
            pane(vec![tab(&tree.startup), tab(&tree.flat)], true),
            pane(vec![tab(&tree.deep)], false),
        ]),
        tabs: None,
        ..SessionLayout::default()
    });
    settings
        .save(&paths.settings_path())
        .map_err(|e| Fail::Error(format!("the session could not be seeded: {e}")))?;
    std::fs::copy(db, paths.db_path())?;
    Ok(())
}

fn session_came_back(data_dir: &Path) -> bool {
    let saved = AppSettings::load(&AppPaths::new(data_dir).settings_path()).session;
    saved.is_some_and(|s| s.is_split())
}

/// A processor mask, in hex with or without its `0x`.
fn parse_affinity(text: &str) -> Result<usize, Fail> {
    let digits = text.strip_prefix("0x").unwrap_or(text);
    match usize::from_str_radix(digits, 16) {
        Ok(mask) if mask != 0 => Ok(mask),
        _ => Err(Fail::Usage(format!(
            "--affinity wants a non-zero hex mask, not {text}."
        ))),
    }
}

/// The median fault count at the moments worth comparing. Parameters rather than entries: `compare`
/// would judge an entry as a time, and how many pages a launch touches is evidence, not a gate.
fn page_fault_parameters(kept: &[(StartupTraceData, f64)]) -> BTreeMap<String, String> {
    [
        StartupMarks::MAIN,
        StartupMarks::WINDOW_SHOWN,
        StartupMarks::LISTING_PAINTED,
    ]
    .into_iter()
    .filter_map(|mark| {
        let readings: Vec<i64> = kept
            .iter()
            .filter_map(|(t, _)| t.page_faults.as_ref().and_then(|m| m.get(mark)).copied())
            .collect();
        (!readings.is_empty()).then(|| (format!("pageFaults.{mark}"), median(readings).to_string()))
    })
    .collect()
}

fn dominant_instance(kept: &[(StartupTraceData, f64)]) -> String {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for (trace, _) in kept {
        *counts.entry(trace.instance.as_str()).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .map_or_else(String::new, |(k, _)| k.to_string())
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

/// One entry per mark (the median launch), the process lifetime, and one memory entry per point the
/// app read itself at.
pub fn entries_of(
    kept: &[(StartupTraceData, f64)],
    parameters: &BTreeMap<String, String>,
) -> Result<Vec<BenchEntry>, Fail> {
    let mut entries = Vec::new();
    let mut marks: Vec<&str> = Vec::new();
    for (trace, _) in kept {
        for mark in trace.marks_ms.keys() {
            if !marks.contains(&mark.as_str()) {
                marks.push(mark);
            }
        }
    }
    for mark in marks {
        let samples: Vec<f64> = kept
            .iter()
            .filter_map(|(t, _)| t.marks_ms.get(mark).copied())
            .collect();
        entries.push(BenchEntry {
            id: format!("startup.{mark}"),
            tier: "C".to_string(),
            parameters: parameters.clone(),
            n: samples.len() as u32,
            time: Some(TimeStats::from_samples(&samples, "ms")?),
            memory: None,
            process: None,
            error: None,
        });
    }

    let lifetimes: Vec<f64> = kept.iter().map(|(_, ms)| *ms).collect();
    entries.push(BenchEntry {
        id: "startup.processLifetime".to_string(),
        tier: "C".to_string(),
        parameters: parameters.clone(),
        n: lifetimes.len() as u32,
        time: Some(TimeStats::from_samples(&lifetimes, "ms")?),
        memory: None,
        process: None,
        error: None,
    });

    let mut points: Vec<&str> = Vec::new();
    for (trace, _) in kept {
        for point in trace.memory.iter().flat_map(|m| m.keys()) {
            if !points.contains(&point.as_str()) {
                points.push(point);
            }
        }
    }
    for point in points {
        let readings: Vec<&StartupMemory> = kept
            .iter()
            .filter_map(|(t, _)| t.memory.as_ref().and_then(|m| m.get(point)))
            .collect();
        entries.push(BenchEntry {
            id: format!("startup.memory.{point}"),
            tier: "C".to_string(),
            parameters: parameters.clone(),
            n: readings.len() as u32,
            time: None,
            memory: None,
            process: Some(ProcessStats {
                working_set_bytes: median(readings.iter().map(|m| m.working_set_bytes)),
                private_bytes: median(readings.iter().map(|m| m.private_bytes)),
                allocator_bytes: median(readings.iter().map(|m| m.allocator_bytes)),
                process_heap_bytes: median(readings.iter().map(|m| m.process_heap_bytes)),
                realized_rows: 0,
                items: 0,
                retained_thumbnails: 0,
                peak_working_set_bytes: Some(median(
                    readings.iter().map(|m| m.peak_working_set_bytes),
                )),
            }),
            error: None,
        });
    }
    Ok(entries)
}

fn launch(
    app: &Path,
    start_folder: Option<&Path>,
    data_dir: &Path,
    trace_path: &Path,
) -> Result<(StartupTraceData, f64), String> {
    let clock = Instant::now();
    let mut child = Command::new(app)
        .args(start_folder)
        .current_dir(app.parent().unwrap_or(Path::new(".")))
        .env("BERTBROWSER_DATA_DIR", data_dir)
        .env("BERTBROWSER_STARTUP_TRACE", trace_path)
        .env("BERTBROWSER_EXIT_AFTER_STARTUP", "1")
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| format!("the process did not start: {e}"))?;

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if clock.elapsed() >= LAUNCH_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "still running after {} s; killed",
                    LAUNCH_TIMEOUT.as_secs()
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => return Err(e.to_string()),
        }
    };
    let lifetime = clock.elapsed().as_secs_f64() * 1000.0;
    if !trace_path.exists() {
        return Err(format!(
            "exited {} without writing a trace",
            status.code().unwrap_or(-1)
        ));
    }
    StartupTraceJson::read(trace_path)
        .map(|t| (t, lifetime))
        .map_err(|e| format!("unreadable trace: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trace(first: f64, working_set: i64) -> StartupTraceData {
        StartupTraceData {
            pid: 1,
            process_start_utc: FileTime(1),
            marks_ms: [
                ("main".to_string(), 5.0),
                ("firstListing".to_string(), first),
            ]
            .into_iter()
            .collect(),
            instance: "first".to_string(),
            indexer: "skipped".to_string(),
            side_effects_skipped: true,
            error: None,
            page_faults: Some([("main".to_string(), first as i64)].into_iter().collect()),
            memory: Some(
                [(
                    "firstListing".to_string(),
                    StartupMemory {
                        working_set_bytes: working_set,
                        private_bytes: 2,
                        allocator_bytes: 3,
                        process_heap_bytes: 5,
                        peak_working_set_bytes: 4,
                    },
                )]
                .into_iter()
                .collect(),
            ),
        }
    }

    #[test]
    fn it_summarises_launches_into_entries() {
        let kept = vec![
            (trace(100.0, 10), 200.0),
            (trace(300.0, 30), 220.0),
            (trace(200.0, 20), 210.0),
        ];
        let entries = entries_of(&kept, &BTreeMap::new()).unwrap();
        let ids: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "startup.firstListing",
                "startup.main",
                "startup.processLifetime",
                "startup.memory.firstListing"
            ]
        );
        let first = entries
            .iter()
            .find(|e| e.id == "startup.firstListing")
            .unwrap();
        assert_eq!(first.time.as_ref().unwrap().median, 200.0);
        let mem = entries.last().unwrap().process.as_ref().unwrap();
        assert_eq!(mem.working_set_bytes, 20);
        assert_eq!(dominant_instance(&kept), "first");
        assert_eq!(page_fault_parameters(&kept)["pageFaults.main"], "200");
    }

    #[test]
    fn an_affinity_is_a_non_zero_hex_mask() {
        assert_eq!(0xF, parse_affinity("F").unwrap());
        assert_eq!(0xF, parse_affinity("0xF").unwrap());
        assert!(parse_affinity("0").is_err());
        assert!(parse_affinity("all").is_err());
    }
}
