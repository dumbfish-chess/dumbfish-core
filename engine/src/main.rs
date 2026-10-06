use dumbfish::{bench, uci};

/// `dumbfish` speaks UCI on stdin/stdout; `dumbfish bench [depth]` prints the bench signature.
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("bench") => {
            let depth = args
                .get(1)
                .and_then(|d| d.parse().ok())
                .unwrap_or(bench::DEFAULT_DEPTH);
            bench::run(depth);
        }
        _ => uci::run(),
    }
}
