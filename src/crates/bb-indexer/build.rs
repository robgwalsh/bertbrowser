use embed_manifest::manifest::ExecutionLevel;
use embed_manifest::{embed_manifest, new_manifest};

// One of the two elevated helpers. The app itself stays asInvoker; only these exes carry
// requireAdministrator, exactly as the C# manifests did.
fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        embed_manifest(
            new_manifest("bb-indexer")
                .requested_execution_level(ExecutionLevel::RequireAdministrator),
        )
        .expect("embed manifest");
    }
    // Static imports resolve from System32 only. Without it the loader looks in the exe's own
    // folder first, so a DLL planted beside this administrator-token process would run elevated.
    println!("cargo:rustc-link-arg-bins=/DEPENDENTLOADFLAG:0x800");
    println!("cargo:rerun-if-changed=build.rs");
}
