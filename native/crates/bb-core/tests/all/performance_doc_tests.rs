// Port of tests/BertBrowser.Core.Tests/Benchmarking/PerformanceDocTests.cs.
use bb_core::benchmarking::PerformanceDoc;

use crate::bench_compare_tests::{entry, set_on};

#[test]
fn render_starts_with_generated_header() {
    let md = PerformanceDoc::render(&[], "```\nrun\n```");
    assert!(md.starts_with(PerformanceDoc::HEADER), "{md}");
    assert!(md.contains("# Performance"), "{md}");
    assert!(md.contains("## How to run"), "{md}");
    assert!(md.contains("_No baselines recorded yet._"), "{md}");
    assert!(
        md.contains("## What the numbers do and do not mean"),
        "{md}"
    );
}

#[test]
fn render_orders_machines_and_ids_deterministically() {
    let zeta = set_on(
        vec![entry("core.z", 1.0, Some(0)), entry("core.a", 1.0, Some(0))],
        false,
        "zeta-machine",
    );
    let alpha = set_on(vec![entry("core.m", 1.0, Some(0))], false, "alpha-machine");

    let one = PerformanceDoc::render(&[zeta.clone(), alpha.clone()], "cmds");
    let two = PerformanceDoc::render(&[alpha, zeta], "cmds");

    assert_eq!(one, two);
    assert!(
        one.find("## alpha-machine").expect("alpha") < one.find("## zeta-machine").expect("zeta")
    );
    assert!(one.find("`core.a`").expect("a") < one.find("`core.z`").expect("z"));
}

#[test]
fn render_legend_carries_the_machine() {
    let set = set_on(vec![entry("core.a", 1.0, Some(0))], false, "the-key");
    let md = PerformanceDoc::render(&[set], "cmds");

    assert!(md.contains("## the-key"), "{md}");
    assert!(md.contains("| CPU | Test CPU, 8 logical cores |"), "{md}");
    assert!(md.contains("| RAM | 16 GB |"), "{md}");
    assert!(
        md.contains("| Corpus | synthetic, scale 1 (100,000 rows) |"),
        "{md}"
    );
    assert!(md.contains("`012345678`"), "{md}");
}

#[test]
fn render_real_baseline_is_labelled() {
    let set = set_on(vec![entry("core.a", 1.0, Some(0))], true, "the-key");
    let md = PerformanceDoc::render(&[set], "cmds");
    assert!(md.contains("## the-key (real data)"), "{md}");
    assert!(
        md.contains("| Corpus | the machine's own index and folders |"),
        "{md}"
    );
}
