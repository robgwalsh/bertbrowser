//! Results and comparisons as GitHub-flavoured Markdown — the console, the CI job summary and
//! `docs/performance.md` all read this. Spec: Core/Benchmarking/BenchMarkdown.cs.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::{BenchEntry, BenchResultSet, CompareOptions, CompareReport, CompareStatus};

pub struct BenchMarkdown;

impl BenchMarkdown {
    /// A comparison, regressions first.
    pub fn render_compare(report: &CompareReport, options: &CompareOptions, title: &str) -> String {
        let mut sb = String::new();
        let _ = writeln!(sb, "## {title}\n");

        let mut counts: BTreeMap<CompareStatus, usize> = BTreeMap::new();
        for row in &report.rows {
            *counts.entry(row.status).or_default() += 1;
        }
        let counts: Vec<String> = counts
            .iter()
            .map(|(status, n)| format!("{n} {}", describe(*status)))
            .collect();
        let _ = writeln!(
            sb,
            "{} benchmarks: {}\n",
            report.rows.len(),
            counts.join(", ")
        );

        let _ = write!(
            sb,
            "Time gate {} at {}; allocation gate {}{}",
            if options.gate_time {
                "on"
            } else {
                "off (reported only)"
            },
            pct(options.time_threshold_pct),
            if options.gate_alloc { "on" } else { "off" },
            match options.alloc_tolerance_pct_override {
                Some(tol) => format!(" at {} for every benchmark.", pct(tol)),
                None => " at each benchmark's own tolerance.".to_string(),
            }
        );
        let _ = writeln!(
            sb,
            " Process-memory gate {}: Rust allocator and process heap at {} over {}, private bytes at {} over {}.\n",
            if options.gate_memory {
                "on"
            } else {
                "off (reported only)"
            },
            pct(options.memory_threshold_pct),
            Self::bytes(Some(options.memory_noise_floor_bytes)),
            pct(options.private_bytes_threshold_pct),
            Self::bytes(Some(options.private_bytes_noise_floor_bytes)),
        );

        // A snapshot has no time and no per-operation allocation, so in the table below it would be
        // a row of dashes; it gets its own, with the three numbers it does have.
        let mut ordered: Vec<_> = report.rows.iter().collect();
        ordered.sort_by(|a, b| a.status.cmp(&b.status).then_with(|| a.id.cmp(&b.id)));
        let is_snapshot =
            |r: &&super::CompareRow| r.baseline_process.is_some() || r.current_process.is_some();

        sb.push_str(
            "| Benchmark | Tier | Base | Current | Δ | Alloc base | Alloc now | Δ | Status |\n",
        );
        sb.push_str("|---|---|---:|---:|---:|---:|---:|---:|---|\n");

        for row in ordered.iter().filter(|r| !is_snapshot(r)) {
            let _ = write!(
                sb,
                "| `{}` | {} | {} | {} | {} | {} | {} | {} | {}",
                row.id,
                row.tier,
                Self::time(row.baseline_median, row.unit.as_deref()),
                Self::time(row.current_median, row.unit.as_deref()),
                delta(row.time_delta_pct),
                Self::bytes(row.baseline_alloc),
                Self::bytes(row.current_alloc),
                delta(row.alloc_delta_pct),
                describe(row.status)
            );
            if let Some(note) = &row.note {
                let _ = write!(sb, " — {}", note.replace('|', "/"));
            }
            sb.push_str(" |\n");
        }

        let snapshots: Vec<_> = ordered.iter().filter(|r| is_snapshot(r)).collect();
        if !snapshots.is_empty() {
            sb.push_str("\n### Process memory\n\n");
            sb.push_str("| Snapshot | Tier | Private base | Private now | Δ | Allocator base | Allocator now | Δ | Process heap base | Process heap now | Δ | Working set base | Working set now | Status |\n");
            sb.push_str("|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|\n");

            for row in snapshots {
                let was = row.baseline_process.as_ref();
                let now = row.current_process.as_ref();
                let _ = write!(
                    sb,
                    "| `{}` | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {}",
                    row.id,
                    row.tier,
                    Self::bytes(was.map(|p| p.private_bytes)),
                    Self::bytes(now.map(|p| p.private_bytes)),
                    delta(row.memory_delta_pct),
                    Self::bytes(was.map(|p| p.allocator_bytes)),
                    Self::bytes(now.map(|p| p.allocator_bytes)),
                    delta(delta_pct(
                        was.map(|p| p.allocator_bytes),
                        now.map(|p| p.allocator_bytes)
                    )),
                    Self::bytes(was.map(|p| p.process_heap_bytes)),
                    Self::bytes(now.map(|p| p.process_heap_bytes)),
                    delta(delta_pct(
                        was.map(|p| p.process_heap_bytes),
                        now.map(|p| p.process_heap_bytes)
                    )),
                    Self::bytes(was.map(|p| p.working_set_bytes)),
                    Self::bytes(now.map(|p| p.working_set_bytes)),
                    describe(row.status)
                );
                if let Some(note) = &row.note {
                    let _ = write!(sb, " — {}", note.replace('|', "/"));
                }
                sb.push_str(" |\n");
            }
        }

        sb
    }

    /// A results file on its own, one table per tier.
    pub fn render_results(set: &BenchResultSet) -> String {
        let c = set.generated_utc.civil();
        let mut sb = format!(
            "## Results — {} — {:04}-{:02}-{:02} {:02}:{:02} UTC\n\n",
            set.machine.key, c.year, c.month, c.day, c.hour, c.minute
        );
        Self::append_tier_tables(&mut sb, &set.benchmarks);
        sb
    }

    /// The per-tier tables, shared with `PerformanceDoc`.
    pub(crate) fn append_tier_tables(sb: &mut String, entries: &[BenchEntry]) {
        let mut tiers: BTreeMap<&str, Vec<&BenchEntry>> = BTreeMap::new();
        for e in entries {
            tiers.entry(e.tier.as_str()).or_default().push(e);
        }

        for (tier, members) in tiers {
            let _ = writeln!(sb, "### Tier {tier} — {}\n", Self::tier_name(tier));

            match tier {
                "A" => {
                    sb.push_str("| Benchmark | Median | p95 | Allocated/op | Allocs/kop | n |\n");
                    sb.push_str("|---|---:|---:|---:|---:|---:|\n");
                    for e in by_id(members.iter().copied()) {
                        let _ = write!(sb, "| `{}` | ", e.id);
                        if let Some(error) = &e.error {
                            let _ = writeln!(sb, "error: {} | | | | |", error.replace('|', "/"));
                            continue;
                        }
                        let _ = writeln!(
                            sb,
                            "{} | {} | {} | {} | {} |",
                            Self::time(e.time.as_ref().map(|t| t.median), unit(e)),
                            Self::time(e.time.as_ref().map(|t| t.p95), unit(e)),
                            Self::bytes(e.memory.as_ref().map(|m| m.allocated_bytes)),
                            match &e.memory {
                                None => "—".to_string(),
                                Some(m) => net_custom(m.gen0_per_k_ops, 0, 2),
                            },
                            e.n
                        );
                    }
                }

                "B" => {
                    sb.push_str("| Scenario | Median | p95 | n |\n");
                    sb.push_str("|---|---:|---:|---:|\n");
                    for e in by_id(members.iter().copied().filter(|e| e.process.is_none())) {
                        let _ = write!(sb, "| `{}` | ", e.id);
                        if let Some(error) = &e.error {
                            let _ = writeln!(sb, "error: {} | | |", error.replace('|', "/"));
                            continue;
                        }
                        let _ = writeln!(
                            sb,
                            "{} | {} | {} |",
                            Self::time(e.time.as_ref().map(|t| t.median), unit(e)),
                            Self::time(e.time.as_ref().map(|t| t.p95), unit(e)),
                            e.n
                        );
                    }
                    append_memory_table(sb, &members, true);
                }

                _ => {
                    sb.push_str("| Mark | Median | p95 | Min | n |\n");
                    sb.push_str("|---|---:|---:|---:|---:|\n");
                    let mut marks: Vec<&BenchEntry> = members
                        .iter()
                        .copied()
                        .filter(|e| e.process.is_none())
                        .collect();
                    // In the order they happen: by median, the unmeasured last.
                    marks.sort_by(|a, b| {
                        let median =
                            |e: &BenchEntry| e.time.as_ref().map_or(f64::MAX, |t| t.median);
                        median(a)
                            .total_cmp(&median(b))
                            .then_with(|| a.id.cmp(&b.id))
                    });
                    for e in marks {
                        let _ = write!(sb, "| `{}` | ", e.id);
                        if let Some(error) = &e.error {
                            let _ = writeln!(sb, "error: {} | | | |", error.replace('|', "/"));
                            continue;
                        }
                        let _ = writeln!(
                            sb,
                            "{} | {} | {} | {} |",
                            Self::time(e.time.as_ref().map(|t| t.median), unit(e)),
                            Self::time(e.time.as_ref().map(|t| t.p95), unit(e)),
                            Self::time(e.time.as_ref().map(|t| t.min), unit(e)),
                            e.n
                        );
                    }
                    append_memory_table(sb, &members, false);
                }
            }

            sb.push('\n');
        }
    }

    pub(crate) fn tier_name(tier: &str) -> &str {
        match tier {
            "A" => "Core micro-benchmarks",
            "B" => "UI scenarios (offscreen harness)",
            "C" => "Startup of the real executable",
            other => other,
        }
    }

    /// A duration in its unit, scaled to read well: `12.3 µs`, `812 ms`, `1.21 s`.
    pub fn time(value: Option<f64>, unit: Option<&str>) -> String {
        let (Some(v), Some(unit)) = (value, unit) else {
            return "—".to_string();
        };

        let ns = match unit {
            "ns" => v,
            "ms" => v * 1_000_000.0,
            "s" => v * 1_000_000_000.0,
            _ => return format!("{} {unit}", num(v)),
        };

        if ns < 1_000.0 {
            format!("{} ns", num(ns))
        } else if ns < 1_000_000.0 {
            format!("{} µs", num(ns / 1_000.0))
        } else if ns < 1_000_000_000.0 {
            format!("{} ms", num(ns / 1_000_000.0))
        } else {
            format!("{} s", num(ns / 1_000_000_000.0))
        }
    }

    pub fn bytes(value: Option<i64>) -> String {
        let Some(b) = value else {
            return "—".to_string();
        };
        if b < 1024 {
            format!("{b} B")
        } else if b < 1024 * 1024 {
            format!("{} KB", num(b as f64 / 1024.0))
        } else if b < 1024 * 1024 * 1024 {
            format!("{} MB", num(b as f64 / (1024.0 * 1024.0)))
        } else {
            format!("{} GB", num(b as f64 / (1024.0 * 1024.0 * 1024.0)))
        }
    }
}

/// The tier's process snapshots, if it has any, as a table of their own under its timings. `rows`:
/// whether the snapshots know how many rows the list held (Tier B does; the real executable in
/// Tier C has nobody to ask).
fn append_memory_table(sb: &mut String, tier: &[&BenchEntry], rows: bool) {
    let snapshots: Vec<&BenchEntry> = by_id(tier.iter().copied().filter(|e| e.process.is_some()));
    if snapshots.is_empty() {
        return;
    }

    sb.push('\n');
    let _ = writeln!(
        sb,
        "| Memory snapshot | Rust allocator | Process heap | Private bytes | Working set | Peak working set |{}",
        if rows {
            " Items | Realized | n |"
        } else {
            " n |"
        }
    );
    let _ = writeln!(
        sb,
        "|---|---:|---:|---:|---:|---:|{}",
        if rows { "---:|---:|---:|" } else { "---:|" }
    );

    for e in snapshots {
        let p = e.process.as_ref().expect("filtered on process");
        let _ = write!(
            sb,
            "| `{}` | {} | {} | {} | {} | {} | ",
            e.id,
            BenchMarkdown::bytes(Some(p.allocator_bytes)),
            BenchMarkdown::bytes(Some(p.process_heap_bytes)),
            BenchMarkdown::bytes(Some(p.private_bytes)),
            BenchMarkdown::bytes(Some(p.working_set_bytes)),
            BenchMarkdown::bytes(p.peak_working_set_bytes)
        );
        if rows {
            let _ = write!(sb, "{} | {} | ", p.items, p.realized_rows);
        }
        let _ = writeln!(sb, "{} |", e.n);
    }
}

fn by_id<'a>(entries: impl Iterator<Item = &'a BenchEntry>) -> Vec<&'a BenchEntry> {
    let mut v: Vec<&BenchEntry> = entries.collect();
    v.sort_by(|a, b| a.id.cmp(&b.id));
    v
}

fn unit(e: &BenchEntry) -> Option<&str> {
    e.time.as_ref().map(|t| t.unit.as_str())
}

fn describe(status: CompareStatus) -> &'static str {
    match status {
        CompareStatus::AllocUp => "allocates more",
        CompareStatus::MemoryUp => "holds more memory",
        CompareStatus::Slower => "slower",
        CompareStatus::Errored => "errored",
        CompareStatus::Removed => "removed",
        CompareStatus::New => "new",
        CompareStatus::AllocDown => "allocates less",
        CompareStatus::MemoryDown => "holds less memory",
        CompareStatus::Faster => "faster",
        CompareStatus::Same => "same",
    }
}

fn delta(pct: Option<f64>) -> String {
    match pct {
        Some(p) => format!("{}{}%", if p >= 0.0 { "+" } else { "" }, net_fixed(p, 1)),
        None => "—".to_string(),
    }
}

fn delta_pct(was: Option<i64>, now: Option<i64>) -> Option<f64> {
    match (was, now) {
        (Some(was), Some(now)) if was > 0 => Some((now - was) as f64 / was as f64 * 100.0),
        _ => None,
    }
}

fn pct(pct: f64) -> String {
    format!("{}%", net_custom(pct, 0, 1))
}

/// Three significant figures, no exponent.
fn num(v: f64) -> String {
    let a = v.abs();
    let decimals = if a >= 100.0 {
        0
    } else if a >= 10.0 {
        1
    } else {
        2
    };
    net_fixed(v, decimals)
}

/// `v.ToString("0.00")` and its relatives. A .NET custom format works from fifteen significant
/// digits and then rounds those half away from zero — not what `{:.2}` does at an exact tie
/// (0.125 is "0.13" there and "0.12" here) — so the digits are produced the same way, and a
/// re-rendered table matches the committed one to the last digit.
fn net_fixed(v: f64, decimals: usize) -> String {
    if !v.is_finite() {
        return v.to_string();
    }
    let sci = format!("{:.14e}", v.abs());
    let (mantissa, exponent) = sci.split_once('e').expect("exponent");
    let exponent: i32 = exponent.parse().expect("exponent");
    let mut digits: Vec<u8> = mantissa
        .bytes()
        .filter(u8::is_ascii_digit)
        .map(|b| b - b'0')
        .collect();
    // How many of the digits sit before the decimal point (zero or negative: none).
    let mut scale = exponent + 1;

    let keep = scale + decimals as i32;
    if keep < 0 {
        digits.clear();
        scale = 0;
    } else if (keep as usize) < digits.len() {
        let keep = keep as usize;
        let round_up = digits[keep] >= 5;
        digits.truncate(keep);
        if round_up {
            let mut i = keep;
            loop {
                if i == 0 {
                    digits.insert(0, 1);
                    scale += 1;
                    break;
                }
                i -= 1;
                if digits[i] == 9 {
                    digits[i] = 0;
                } else {
                    digits[i] += 1;
                    break;
                }
            }
        }
    }

    let digit = |i: i32| -> char {
        if i < 0 {
            '0'
        } else {
            (b'0' + digits.get(i as usize).copied().unwrap_or(0)) as char
        }
    };
    let mut out = String::new();
    if v.is_sign_negative() {
        out.push('-');
    }
    if scale <= 0 {
        out.push('0');
    } else {
        out.extend((0..scale).map(digit));
    }
    if decimals > 0 {
        out.push('.');
        out.extend((0..decimals as i32).map(|i| digit(scale + i)));
    }
    out
}

/// `"0.##"`-style: at most `max` decimals, trailing zeros dropped down to `min`.
fn net_custom(v: f64, min: usize, max: usize) -> String {
    let mut s = net_fixed(v, max);
    if max > min {
        let Some(dot) = s.find('.') else {
            return s;
        };
        let keep_to = dot + 1 + min;
        while s.len() > keep_to && s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    s
}
