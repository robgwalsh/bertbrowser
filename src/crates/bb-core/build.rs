// Bakes the compiler version in for `benchmarking::MachineKey`. A native build has no runtime to
// name; the compiler that built it is what moves a baseline's numbers the way a runtime patch did,
// so it is what the machine key carries. Reruns only when the compiler changes, not on every edit.
fn main() {
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let version = std::process::Command::new(rustc)
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.split_whitespace().nth(1).map(str::to_string))
        .unwrap_or_else(|| "0.0.0".to_string());
    println!("cargo:rustc-env=BB_RUSTC_VERSION={version}");
    println!("cargo:rerun-if-env-changed=RUSTC");
    println!("cargo:rerun-if-changed=build.rs");
}
