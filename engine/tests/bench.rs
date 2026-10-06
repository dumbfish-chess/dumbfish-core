use std::process::Command;

/// Runs `dumbfish bench [args]` and returns the node count from the last line.
fn bench(args: &[&str]) -> u64 {
    let output = Command::new(env!("CARGO_BIN_EXE_dumbfish"))
        .arg("bench")
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let last = stdout.lines().last().unwrap();
    let fields: Vec<_> = last.split_whitespace().collect();
    assert!(matches!(fields[..], [_, "nodes", _, "nps"]), "{last:?}");
    fields[0].parse().unwrap()
}

#[test]
fn bench_is_deterministic() {
    // The default depth takes seconds in a debug build; determinism does not depend on depth.
    let nodes = bench(&["4"]);
    assert!(nodes > 0);
    assert_eq!(bench(&["4"]), nodes);
}

#[test]
fn bench_depth_is_configurable() {
    assert!(bench(&["2"]) < bench(&["3"]));
}
