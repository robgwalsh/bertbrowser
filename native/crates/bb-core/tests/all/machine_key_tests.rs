// Port of tests/BertBrowser.Core.Tests/Benchmarking/MachineKeyTests.cs.
use bb_core::benchmarking::{MachineFacts, MachineKey};

fn facts() -> MachineFacts {
    MachineFacts {
        cpu: "AMD Ryzen 9 7950X 16-Core Processor".to_string(),
        logical_cores: 32,
        ram_gb: 64,
        os_build: 26200,
        runtime_major_minor: "10.0".to_string(),
    }
}

#[test]
fn compute_is_deterministic() {
    assert_eq!(
        MachineKey::compute(&facts()),
        MachineKey::compute(&facts().clone())
    );
}

#[test]
fn compute_reads_as_what_it_is() {
    let key = MachineKey::compute(&facts());
    assert!(
        key.starts_with("amd-ryzen-9-7950x-16-core-32c-64gb-w26200-net10.0-"),
        "{key}"
    );
    // -[0-9a-f]{8}$
    let tail = &key[key.len() - 9..];
    assert!(tail.starts_with('-'), "{key}");
    assert!(
        tail[1..]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "{key}"
    );
}

#[test]
fn compute_different_core_count_different_key() {
    let other = MachineFacts {
        logical_cores: 16,
        ..facts()
    };
    assert_ne!(MachineKey::compute(&facts()), MachineKey::compute(&other));
}

#[test]
fn compute_different_os_build_different_key() {
    let other = MachineFacts {
        os_build: 22631,
        ..facts()
    };
    assert_ne!(MachineKey::compute(&facts()), MachineKey::compute(&other));
}

#[test]
fn compute_same_slug_different_cpu_still_differs_in_hash() {
    // Two names that slug identically after the clock suffix is dropped still get their own key.
    let a = MachineKey::compute(&MachineFacts {
        cpu: "Intel Core i7 @ 2.60GHz".to_string(),
        ..facts()
    });
    let b = MachineKey::compute(&MachineFacts {
        cpu: "Intel Core i7 @ 3.10GHz".to_string(),
        ..facts()
    });
    let stem = |k: &str| k[..k.rfind('-').expect("dash")].to_string();
    assert_eq!(stem(&a), stem(&b));
    assert_ne!(a, b);
}

#[test]
fn slug_strips_vendor_noise() {
    for (cpu, expected) in [
        ("Intel(R) Core(TM) i9-13900K", "intel-core-i9-13900k"),
        (
            "AMD Ryzen 7 5800X 8-Core Processor",
            "amd-ryzen-7-5800x-8-core",
        ),
        (
            "Intel(R) Xeon(R) CPU E5-2690 v4 @ 2.60GHz",
            "intel-xeon-e5-2690-v4",
        ),
        (
            "AMD Ryzen 7 PRO 6850U with Radeon Graphics",
            "amd-ryzen-7-pro-6850u",
        ),
        ("   ", "cpu"),
    ] {
        assert_eq!(expected, MachineKey::slug(cpu), "{cpu:?}");
    }
}

#[test]
fn slug_is_bounded() {
    let slug =
        MachineKey::slug("Some Very Long Processor Marketing Name With Many Many Words In It");
    assert!(slug.len() <= 32, "{slug}");
    assert!(!slug.ends_with('-'), "{slug}");
}

#[test]
fn probe_returns_plausible_facts() {
    let facts = MachineKey::probe();
    assert!(facts.logical_cores > 0);
    assert!(facts.ram_gb > 0);
    assert!(facts.os_build > 0);
    // ^\d+\.\d+$
    let (major, minor) = facts
        .runtime_major_minor
        .split_once('.')
        .unwrap_or_else(|| panic!("{}", facts.runtime_major_minor));
    assert!(
        !major.is_empty() && major.bytes().all(|b| b.is_ascii_digit()),
        "{}",
        facts.runtime_major_minor
    );
    assert!(
        !minor.is_empty() && minor.bytes().all(|b| b.is_ascii_digit()),
        "{}",
        facts.runtime_major_minor
    );
    assert!(!facts.cpu.trim().is_empty());
}
