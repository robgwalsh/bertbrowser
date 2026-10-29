//! Records a baseline: one or more results files merged into
//! `bench/baselines/<machine-key>[.real].json`. Spec: Cli/BaselineCommand.cs.
//!
//! Merging is what lets a Tier A run, a Tier B run and a Tier C run -- three processes, three files
//! -- land in one baseline per machine. Later files override earlier ones where ids coincide, so
//! re-recording one tier is `baseline --from <the existing baseline> --from <the new file>`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use bb_core::FileTime;
use bb_core::benchmarking::{BenchEntry, BenchResultSet, BenchResultsJson, CURRENT_SCHEMA_VERSION};

use crate::args::{ArgList, Fail, unknown};
use crate::env;

pub fn execute(mut args: ArgList) -> Result<i32, Fail> {
    let mut from = Vec::new();
    let mut machine_key: Option<String> = None;
    let mut repo: Option<String> = None;

    while let Some(arg) = args.next_arg() {
        match arg.as_str() {
            "--from" => from.push(args.value(&arg)?),
            "--machine-key" => machine_key = Some(args.value(&arg)?),
            "--repo" => repo = Some(args.value(&arg)?),
            _ => return Err(unknown("baseline", &arg)),
        }
    }
    if from.is_empty() {
        return Err(Fail::Usage(
            "baseline needs at least one --from <file>.".to_string(),
        ));
    }

    let repo = env::Repo::find(repo.as_deref())?;
    let sets = from
        .iter()
        .map(|f| BenchResultsJson::read(Path::new(f)))
        .collect::<Result<Vec<_>, _>>()?;

    let real = sets[0].run.real;
    if sets.iter().any(|s| s.run.real != real) {
        return Err(Fail::Error(
            "Real and synthetic results cannot share a baseline; record them separately."
                .to_string(),
        ));
    }

    let merged = merge(&sets, machine_key.as_deref());
    let path = repo.baseline_path(&merged.machine.key, real);
    let existed = path.exists();
    BenchResultsJson::write(&merged, &path)?;

    let sha = &merged.git.sha;
    println!(
        "{} {}: {} benchmarks, tiers {}, at {}{}.",
        if existed { "Replaced" } else { "Recorded" },
        path.display(),
        merged.benchmarks.len(),
        merged.run.tiers.join("+"),
        &sha[..sha.len().min(9)],
        if merged.git.dirty {
            " (tree had uncommitted changes)"
        } else {
            ""
        }
    );
    println!("Run `bb-bench report` to regenerate docs/performance.md, and commit both.");
    Ok(0)
}

pub fn merge(sets: &[BenchResultSet], machine_key: Option<&str>) -> BenchResultSet {
    let mut entries: BTreeMap<String, BenchEntry> = BTreeMap::new();
    let mut tiers: BTreeSet<String> = BTreeSet::new();
    for set in sets {
        for entry in &set.benchmarks {
            entries.insert(entry.id.clone(), entry.clone());
        }
        tiers.extend(set.run.tiers.iter().cloned());
    }

    let first = &sets[0];
    let last = &sets[sets.len() - 1];
    let mut machine = first.machine.clone();
    if let Some(key) = machine_key {
        machine.key = key.to_string();
    }
    let mut run = first.run.clone();
    run.tiers = tiers.into_iter().collect();

    BenchResultSet {
        schema_version: CURRENT_SCHEMA_VERSION,
        generated_utc: FileTime::now(),
        tool: env::tool_version(),
        git: last.git.clone(),
        machine,
        run,
        // BTreeMap order is ordinal on the id, which is the order the document is sorted by.
        benchmarks: entries.into_values().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bb_core::benchmarking::{GitInfo, MachineInfo, RunInfo};

    fn set(tier: &str, ids: &[&str]) -> BenchResultSet {
        BenchResultSet {
            schema_version: CURRENT_SCHEMA_VERSION,
            generated_utc: FileTime(1),
            tool: "t".into(),
            git: GitInfo {
                sha: "abc".into(),
                branch: "b".into(),
                dirty: false,
            },
            machine: MachineInfo {
                key: "k".into(),
                cpu: "c".into(),
                logical_cores: 1,
                ram_gb: 1,
                os_build: 1,
                runtime: "r".into(),
                power_plan: None,
                app_running: false,
                indexer_running: false,
            },
            run: RunInfo {
                job: "default".into(),
                scale: 1,
                real: false,
                tiers: vec![tier.into()],
                render_mode: None,
                repeat: None,
                toolchain: None,
            },
            benchmarks: ids
                .iter()
                .map(|id| BenchEntry {
                    id: id.to_string(),
                    tier: tier.into(),
                    parameters: BTreeMap::new(),
                    n: 1,
                    time: None,
                    memory: None,
                    process: None,
                    error: None,
                })
                .collect(),
        }
    }

    #[test]
    fn tiers_merge_and_later_files_win() {
        let mut later = set("B", &["ui.a", "core.b"]);
        later.benchmarks[1].n = 9;
        let merged = merge(&[set("A", &["core.b", "core.a"]), later], Some("mine"));
        assert_eq!(merged.machine.key, "mine");
        assert_eq!(merged.run.tiers, ["A", "B"]);
        let ids: Vec<&str> = merged.benchmarks.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(ids, ["core.a", "core.b", "ui.a"]);
        assert_eq!(merged.benchmarks[1].n, 9);
    }
}
