//! Elevated helper for one retried file operation. Serves one request over a nonced pipe, then exits.
//! Spec: src/BertBrowser.Elevator/*.cs, Core/Services/Elevation/*.
fn main() {
    println!("BertBrowser.Elevator {}", env!("CARGO_PKG_VERSION"));
}
