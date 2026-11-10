// bb-core's library has no test target (`test = false` in its Cargo.toml): building the 100k-line crate
// a second time as its own test binary cost every `cargo test` a whole extra compile. The price is
// that a `#[cfg(test)]` module under src/ would never be compiled or run, and nothing would say so.
// This says so. Tests of bb-core live here, in tests/all/<module>/.
use std::fs;
use std::path::Path;

fn rust_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_test_module_hides_in_the_library_where_it_would_never_run() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    assert!(files.len() > 100, "found only {} files", files.len());
    let offenders: Vec<String> = files
        .iter()
        .filter(|p| {
            fs::read_to_string(p)
                .unwrap()
                .lines()
                .any(|l| l.trim_start().starts_with("#[cfg(test)]"))
        })
        .map(|p| p.strip_prefix(&src).unwrap().display().to_string())
        .collect();
    assert!(
        offenders.is_empty(),
        "#[cfg(test)] in bb-core/src never runs (the lib has test = false); move these to tests/all: {offenders:?}"
    );
}
