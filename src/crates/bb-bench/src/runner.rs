//! The Tier A engine: a hand-rolled micro-benchmark runner. Spec: the BenchmarkDotNet job in
//! Infrastructure/BenchConfig.cs and the adapter in Infrastructure/SummaryAdapter.cs.
//!
//! Each benchmark is set up lazily (only if the filter selects it), warmed, sized so one timed
//! iteration lasts long enough to measure, then timed for K iterations; the samples are ns/op. What an
//! operation allocates is measured in a separate pass under the counting allocator, so it is exact
//! and the timings carry none of its cost.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Instant;

use bb_core::benchmarking::{AllocStats, BenchEntry, TimeStats};

use crate::alloc;

/// One measurable operation.
pub struct Op {
    pub run: Box<dyn FnMut()>,
    /// Untimed work before every call (emptying a table). Disables batching.
    pub before_each: Option<Box<dyn FnMut()>>,
}

impl Op {
    pub fn new(run: impl FnMut() + 'static) -> Op {
        Op {
            run: Box::new(run),
            before_each: None,
        }
    }

    pub fn with_before(mut self, before: impl FnMut() + 'static) -> Op {
        self.before_each = Some(Box::new(before));
        self
    }
}

type Maker = Box<dyn FnOnce() -> Result<Op, String>>;

struct Pending {
    id: String,
    parameters: BTreeMap<String, String>,
    tolerance_pct: f64,
    make: Maker,
}

/// How hard a run looks. `default` is what a committed baseline uses; `short` is the CI pass.
#[derive(Clone, Copy, Debug)]
pub struct Job {
    pub name: &'static str,
    pub iterations: usize,
    pub warmup_ms: f64,
    pub target_iteration_ms: f64,
    /// A benchmark whose iterations are slow takes fewer, but never fewer than five.
    pub budget_ms: f64,
}

impl Job {
    pub const DEFAULT: Job = Job {
        name: "default",
        iterations: 15,
        warmup_ms: 150.0,
        target_iteration_ms: 40.0,
        budget_ms: 20_000.0,
    };
    pub const SHORT: Job = Job {
        name: "short",
        iterations: 5,
        warmup_ms: 40.0,
        target_iteration_ms: 15.0,
        budget_ms: 6_000.0,
    };

    pub fn parse(name: &str) -> Option<Job> {
        match name {
            "default" => Some(Job::DEFAULT),
            "short" => Some(Job::SHORT),
            _ => None,
        }
    }
}

/// The benchmarks a run may execute, and which of them the filters select.
pub struct Suite {
    filters: Vec<String>,
    pending: Vec<Pending>,
}

impl Suite {
    pub fn new(filters: &[String]) -> Suite {
        Suite {
            filters: filters.iter().map(|f| f.to_lowercase()).collect(),
            pending: Vec::new(),
        }
    }

    pub fn selects(&self, id: &str) -> bool {
        if self.filters.is_empty() {
            return true;
        }
        let id = id.to_lowercase();
        self.filters.iter().any(|f| glob(f, &id))
    }

    /// Registers a benchmark. `make` runs only if the filter selects `id`, so a corpus-backed
    /// benchmark costs nothing in a run that leaves it out.
    pub fn add(
        &mut self,
        id: String,
        parameters: &[(&str, &str)],
        tolerance_pct: f64,
        make: impl FnOnce() -> Result<Op, String> + 'static,
    ) {
        if !self.selects(&id) {
            return;
        }
        self.pending.push(Pending {
            id,
            parameters: parameters
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            tolerance_pct,
            make: Box::new(make),
        });
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Measures everything selected, in id order, reporting each as it finishes.
    pub fn run(mut self, job: Job, scale: u32, progress: &dyn Fn(&str)) -> Vec<BenchEntry> {
        self.pending.sort_by(|a, b| a.id.cmp(&b.id));
        let total = self.pending.len();
        let mut entries = Vec::with_capacity(total);
        for (i, p) in self.pending.into_iter().enumerate() {
            let mut parameters = p.parameters.clone();
            parameters.insert("scale".to_string(), scale.to_string());
            let outcome = catch_unwind(AssertUnwindSafe(|| match (p.make)() {
                Ok(op) => measure(op, &job, p.tolerance_pct),
                Err(e) => Err(e),
            }));
            let entry = match outcome {
                Ok(Ok(m)) => {
                    progress(&format!(
                        "# [{}/{}] {}: {} {}",
                        i + 1,
                        total,
                        p.id,
                        format_ns(m.time.median),
                        format_bytes(m.memory.allocated_bytes)
                    ));
                    BenchEntry {
                        id: p.id,
                        tier: "A".to_string(),
                        parameters,
                        n: m.n as u32,
                        time: Some(m.time),
                        memory: Some(m.memory),
                        process: None,
                        error: None,
                    }
                }
                Ok(Err(e)) => failed(p.id, parameters, e, progress),
                Err(panic) => {
                    let why = panic
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                        .unwrap_or_else(|| "panicked".to_string());
                    failed(p.id, parameters, why, progress)
                }
            };
            entries.push(entry);
        }
        entries
    }
}

fn failed(
    id: String,
    parameters: BTreeMap<String, String>,
    why: String,
    progress: &dyn Fn(&str),
) -> BenchEntry {
    progress(&format!("# {id}: FAILED {why}"));
    BenchEntry {
        id,
        tier: "A".to_string(),
        parameters,
        n: 0,
        time: None,
        memory: None,
        process: None,
        error: Some(why),
    }
}

struct Measured {
    time: TimeStats,
    memory: AllocStats,
    n: usize,
}

fn time_once(op: &mut Op) -> f64 {
    if let Some(before) = op.before_each.as_mut() {
        before();
    }
    let start = Instant::now();
    (op.run)();
    start.elapsed().as_secs_f64() * 1e9
}

fn measure(mut op: Op, job: &Job, tolerance_pct: f64) -> Result<Measured, String> {
    // Cold call first: lazies, page cache, connection pools.
    let cold = time_once(&mut op);

    let warm_start = Instant::now();
    let mut warm_runs = 1;
    let mut estimate = cold;
    while warm_runs < 3 || warm_start.elapsed().as_secs_f64() * 1e3 < job.warmup_ms {
        estimate = time_once(&mut op);
        warm_runs += 1;
        if estimate * warm_runs as f64 > job.warmup_ms * 1e6 * 4.0 && warm_runs >= 3 {
            break;
        }
    }

    // Size one iteration to last about `target_iteration_ms`.
    let batch = if op.before_each.is_some() {
        1
    } else {
        ((job.target_iteration_ms * 1e6 / estimate.max(1.0)).ceil() as usize).clamp(1, 2_000_000)
    };

    let iteration_ns = estimate.max(1.0) * batch as f64;
    let mut iterations = job.iterations;
    if iteration_ns * iterations as f64 > job.budget_ms * 1e6 {
        iterations = ((job.budget_ms * 1e6 / iteration_ns) as usize)
            .max(5)
            .min(job.iterations);
    }

    let mut samples = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        if batch == 1 {
            samples.push(time_once(&mut op));
        } else {
            let start = Instant::now();
            for _ in 0..batch {
                (op.run)();
            }
            samples.push(start.elapsed().as_secs_f64() * 1e9 / batch as f64);
        }
    }

    // Allocation pass, one counted call at a time so untimed setup stays out of it.
    let alloc_ops = ((5e6 / estimate.max(1.0)) as usize).clamp(1, 1000);
    let (mut bytes, mut calls) = (0u64, 0u64);
    for _ in 0..alloc_ops {
        if let Some(before) = op.before_each.as_mut() {
            before();
        }
        let ((), counts) = alloc::count(|| (op.run)());
        bytes += counts.bytes;
        calls += counts.calls;
    }

    let time = TimeStats::from_samples(&samples, "ns").map_err(|e| e.to_string())?;
    Ok(Measured {
        time,
        memory: AllocStats {
            allocated_bytes: (bytes as f64 / alloc_ops as f64).round() as i64,
            gen0_per_k_ops: (calls as f64 / alloc_ops as f64 * 1000.0 * 100.0).round() / 100.0,
            gen1_per_k_ops: 0.0,
            gen2_per_k_ops: 0.0,
            tolerance_pct,
        },
        n: samples.len(),
    })
}

/// `*` and `?` over already-lowercased text.
pub fn glob(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// `core.class.method(name=value,...)`, the C# id shape.
pub fn bench_id(class: &str, method: &str, parameters: &[(&str, &str)]) -> String {
    let mut id = format!("core.{class}.{method}");
    if !parameters.is_empty() {
        id.push('(');
        id.push_str(
            &parameters
                .iter()
                .map(|(k, v)| format!("{}={v}", k.to_lowercase()))
                .collect::<Vec<_>>()
                .join(","),
        );
        id.push(')');
    }
    id
}

fn format_ns(ns: f64) -> String {
    if ns >= 1e9 {
        format!("{:.2} s", ns / 1e9)
    } else if ns >= 1e6 {
        format!("{:.2} ms", ns / 1e6)
    } else if ns >= 1e3 {
        format!("{:.2} us", ns / 1e3)
    } else {
        format!("{ns:.1} ns")
    }
}

fn format_bytes(b: i64) -> String {
    if b >= 1 << 20 {
        format!("{:.2} MB", b as f64 / (1 << 20) as f64)
    } else if b >= 1 << 10 {
        format!("{:.2} KB", b as f64 / 1024.0)
    } else {
        format!("{b} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs_match_ids_case_blind() {
        assert!(glob("*pathkey*", "core.pathkey.canonicalizeshort"));
        assert!(glob("core.?ath*", "core.pathkey.x"));
        assert!(!glob("*fsindex*", "core.pathkey.x"));
        assert!(glob("*", ""));
    }

    #[test]
    fn ids_have_the_csharp_shape() {
        assert_eq!(bench_id("pathkey", "x", &[]), "core.pathkey.x");
        assert_eq!(
            bench_id("fsindex", "search", &[("Query", "ext:txt")]),
            "core.fsindex.search(query=ext:txt)"
        );
    }

    #[test]
    fn a_suite_measures_and_counts() {
        let mut suite = Suite::new(&["*alloc*".to_string()]);
        suite.add("core.test.alloc".to_string(), &[], 0.0, || {
            Ok(Op::new(|| {
                std::hint::black_box(vec![0u8; 1000]);
            }))
        });
        suite.add("core.test.other".to_string(), &[], 0.0, || {
            Err("never made".into())
        });
        let tiny = Job {
            iterations: 5,
            warmup_ms: 1.0,
            target_iteration_ms: 1.0,
            ..Job::SHORT
        };
        let entries = suite.run(tiny, 1, &|_| {});
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(e.n, 5);
        assert!(e.memory.as_ref().unwrap().allocated_bytes >= 1000);
        assert!(e.time.as_ref().unwrap().median > 0.0);
    }
}
