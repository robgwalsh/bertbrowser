//! Judges a results file against a baseline and says so in Markdown. Spec: Cli/CompareCommand.cs.

use std::io::Write as _;
use std::path::Path;

use bb_core::benchmarking::{
    BenchCompare, BenchMarkdown, BenchResultSet, BenchResultsJson, CompareOptions, CompareReport,
};

use crate::args::{ArgList, Fail, unknown};

pub fn execute(mut args: ArgList) -> Result<i32, Fail> {
    let mut baseline_path: Option<String> = None;
    let mut current_path: Option<String> = None;
    let mut summary_path: Option<String> = None;
    let mut title: Option<String> = None;
    let mut options = CompareOptions::default();
    let mut ci = false;

    while let Some(arg) = args.next_arg() {
        match arg.as_str() {
            "--baseline" => baseline_path = Some(args.value(&arg)?),
            "--current" => current_path = Some(args.value(&arg)?),
            "--time-threshold" => options.time_threshold_pct = args.double(&arg)?,
            "--alloc-threshold" => options.alloc_tolerance_pct_override = Some(args.double(&arg)?),
            "--memory-threshold" => options.memory_threshold_pct = args.double(&arg)?,
            "--no-time" => options.gate_time = false,
            "--no-alloc" => options.gate_alloc = false,
            "--no-memory" => options.gate_memory = false,
            "--strict" => options.strict = true,
            "--summary" => summary_path = Some(args.value(&arg)?),
            "--title" => title = Some(args.value(&arg)?),
            "--ci" => ci = true,
            "--repo" => {
                args.value(&arg)?;
            }
            _ => return Err(unknown("compare", &arg)),
        }
    }

    let (Some(baseline_path), Some(current_path)) = (baseline_path, current_path) else {
        return Err(Fail::Usage(
            "compare needs --baseline <file> and --current <file>.".to_string(),
        ));
    };

    // A shared runner's process memory is as much the runner as the code, like its clock.
    if ci {
        options.gate_time = false;
        options.gate_memory = false;
    }

    let current = BenchResultsJson::read(Path::new(&current_path))?;
    let sha = &current.git.sha;
    let title = title.unwrap_or_else(|| {
        format!(
            "Benchmarks - {} - {}",
            current.machine.key,
            &sha[..sha.len().min(9)]
        )
    });

    if !Path::new(&baseline_path).exists() {
        if !ci {
            eprintln!(
                "No baseline at {baseline_path}. Record one with: bb-bench baseline --from {current_path}"
            );
            return Ok(2);
        }
        // CI bootstraps itself: the first run has nothing to compare against, so it hands back a
        // candidate for a person to commit and does not fail.
        let candidate = Path::new(&current_path)
            .canonicalize()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_default()
            .join("candidate-baseline.json");
        BenchResultsJson::write(&current, &candidate)?;
        let notice = format!(
            "No baseline at `{baseline_path}`. This run's results were written to `{}`; download the artifact and commit that file as the baseline to turn the allocation gate on.",
            candidate.display()
        );
        println!("{notice}");
        write_summary(
            summary_path.as_deref(),
            ci,
            &format!(
                "## {title}\n\n{notice}\n\n{}",
                BenchMarkdown::render_results(&current)
            ),
        );
        return Ok(if current.benchmarks.iter().any(|b| b.error.is_some()) {
            2
        } else {
            0
        });
    }

    let baseline = BenchResultsJson::read(Path::new(&baseline_path))?;
    let report = BenchCompare::compare(&baseline, &current, &options)?;
    let mut markdown = BenchMarkdown::render_compare(&report, &options, &title);
    if let Some(note) = mismatch_note(&baseline, &current) {
        markdown = format!("{note}\n\n{markdown}");
    }

    println!("{markdown}");
    write_summary(summary_path.as_deref(), ci, &markdown);

    if report.has_errors {
        eprintln!("# benchmarks errored - see the rows marked errored.");
        return Ok(2);
    }
    if regressed(&report, &options) {
        eprintln!("# regression: {}", reasons(&report, &options).join(", "));
        return Ok(1);
    }
    println!("# no regressions.");
    Ok(0)
}

fn regressed(report: &CompareReport, options: &CompareOptions) -> bool {
    report.time_regressed
        || report.alloc_regressed
        || report.memory_regressed
        || (options.strict && report.has_removed)
}

fn reasons(report: &CompareReport, options: &CompareOptions) -> Vec<String> {
    let mut out = Vec::new();
    if report.alloc_regressed {
        out.push("more allocation than the baseline allows".to_string());
    }
    if report.memory_regressed {
        out.push(format!(
            "a process snapshot holds more than the baseline (heap over {:.1}%, or private bytes over {:.1}%)",
            options.memory_threshold_pct, options.private_bytes_threshold_pct
        ));
    }
    if report.time_regressed {
        out.push(format!(
            "slower than the baseline by more than {:.1}%",
            options.time_threshold_pct
        ));
    }
    if options.strict && report.has_removed {
        out.push("benchmarks missing from this run (--strict)".to_string());
    }
    out
}

/// Allocation and time differences between jobs or toolchains are expected and are not regressions.
/// A mismatch is not refused -- someone may want the picture anyway -- but it is said above the table.
fn mismatch_note(baseline: &BenchResultSet, current: &BenchResultSet) -> Option<String> {
    let mut mismatch = Vec::new();
    if baseline.run.job != current.run.job {
        mismatch.push(format!(
            "job {} against {}",
            baseline.run.job, current.run.job
        ));
    }
    if let (Some(a), Some(b)) = (&baseline.run.toolchain, &current.run.toolchain)
        && a != b
    {
        mismatch.push(format!("toolchain {a} against {b}"));
    }
    if mismatch.is_empty() {
        return None;
    }
    Some(format!(
        "> **Not like for like:** {}. Allocation and time differences between jobs or toolchains are expected and are not regressions. Record a baseline with the same settings to compare properly.",
        mismatch.join("; ")
    ))
}

fn write_summary(summary_path: Option<&str>, ci: bool, markdown: &str) {
    if let Some(path) = summary_path {
        let _ = std::fs::write(path, markdown);
    }
    if ci
        && let Some(step) = std::env::var_os("GITHUB_STEP_SUMMARY").filter(|v| !v.is_empty())
        && let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(step)
    {
        let _ = writeln!(file, "{markdown}");
    }
}
