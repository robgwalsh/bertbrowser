// bb-core and the crates split out of it have no lib test target (`test = false` in their Cargo.toml):
// building each library a second time as its own test binary cost every `cargo test` a whole extra
// compile. The price is that a `#[cfg(test)]` module under such a crate's src/ would never be compiled
// or run, and nothing would say so. This says so, for every crate in the workspace that opts out.
// Their tests live in <crate>/tests/all/<module>/.
use std::fs;
use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The crates whose `[lib]` says `test = false`.
fn crates_without_lib_tests() -> Vec<PathBuf> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    fs::read_dir(crates)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|dir| {
            let manifest = fs::read_to_string(dir.join("Cargo.toml")).unwrap_or_default();
            let lib = manifest.split("\n[").find(|s| s.starts_with("lib]"));
            lib.is_some_and(|s| s.lines().any(|l| l.trim() == "test = false"))
        })
        .collect()
}

#[test]
fn no_test_module_hides_in_the_library_where_it_would_never_run() {
    let crates = crates_without_lib_tests();
    assert!(
        crates.iter().any(|c| c.ends_with("bb-core")),
        "bb-core not among {crates:?}"
    );
    let mut offenders = Vec::new();
    let mut scanned = 0;
    for dir in &crates {
        let src = dir.join("src");
        let mut files = Vec::new();
        rust_files(&src, &mut files);
        scanned += files.len();
        offenders.extend(
            files
                .iter()
                .filter(|p| {
                    fs::read_to_string(p)
                        .unwrap()
                        .lines()
                        .any(|l| l.trim_start().starts_with("#[cfg(test)]"))
                })
                .map(|p| {
                    p.strip_prefix(dir.parent().unwrap())
                        .unwrap()
                        .display()
                        .to_string()
                }),
        );
    }
    assert!(scanned > 100, "found only {scanned} files");
    assert!(
        offenders.is_empty(),
        "#[cfg(test)] in a lib with test = false never runs; move these to <crate>/tests/all: {offenders:?}"
    );
}
