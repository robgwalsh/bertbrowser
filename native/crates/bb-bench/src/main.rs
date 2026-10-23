//! Benchmark driver: run | ui | startup | compare | baseline | report | corpus.
//! Spec: tools/BertBrowser.Bench/**, Core/Benchmarking/*.
fn main() {
    let sub = std::env::args().nth(1).unwrap_or_default();
    match sub.as_str() {
        "run" | "ui" | "startup" | "compare" | "baseline" | "report" | "corpus" => {
            eprintln!("bb-bench {sub}: not implemented yet");
            std::process::exit(2);
        }
        _ => {
            eprintln!("usage: bb-bench <run|ui|startup|compare|baseline|report|corpus> [options]");
            std::process::exit(64);
        }
    }
}
