//! Elevated MFT index helper. Outlives the app; dials the app's well-known per-user pipe; one per user
//! via the presence mutex. Spec: src/BertBrowser.Indexer/*.cs, Core/Services/Mft/*.
fn main() {
    println!("BertBrowser.Indexer {}", env!("CARGO_PKG_VERSION"));
}
