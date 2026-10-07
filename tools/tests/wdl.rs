use std::fs;
use std::process::Command;

#[test]
fn fits_a_model_with_draws_in_the_middle() {
    let mut dataset = String::new();
    for (fen, results) in [
        (
            "4k3/8/8/8/8/8/8/3QK3 w - -",
            ["1-0", "1-0", "1-0", "1/2-1/2"],
        ),
        (
            "3qk3/8/8/8/8/8/8/4K3 w - -",
            ["0-1", "0-1", "0-1", "1/2-1/2"],
        ),
        (
            "4k3/pppppppp/8/8/8/8/PPPPPPPP/4K3 w - -",
            ["1/2-1/2", "1/2-1/2", "1-0", "0-1"],
        ),
    ] {
        for result in results {
            dataset += &format!("{fen} c9 \"{result}\";\n");
        }
    }
    let path = format!("{}/wdl.epd", env!("CARGO_TARGET_TMPDIR"));
    fs::write(&path, dataset).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_dumbfish-tools"))
        .args(["wdl", &path])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let line = stdout.lines().find(|l| !l.starts_with('#')).unwrap();
    let values: Vec<f64> = line
        .split_whitespace()
        .map(|v| v.parse().unwrap())
        .collect();
    let [a, b] = values[..] else {
        panic!("{stdout}")
    };
    assert!(a > 0.0 && b > 0.0, "{stdout}");
}

#[test]
fn rejects_lines_without_results() {
    let path = format!("{}/wdl-bad.epd", env!("CARGO_TARGET_TMPDIR"));
    fs::write(&path, "4k3/8/8/8/8/8/8/3QK3 w - - c9;\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_dumbfish-tools"))
        .args(["wdl", &path])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("line 1: no result"));
}
