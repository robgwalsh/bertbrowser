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
    println!("cargo:rerun-if-changed=build.rs");
}
