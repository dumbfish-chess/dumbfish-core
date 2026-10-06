use std::fs;
use std::process::Command;

use dumbfish::eval::{FEATURES, parse_weights};

fn tune(name: &str, dataset: &str, weights: &str, epochs: &str) -> (bool, String, String) {
    let dir = format!("{}/{name}", env!("CARGO_TARGET_TMPDIR"));
    fs::create_dir_all(&dir).unwrap();
    let (dataset_path, weights_path) = (format!("{dir}/data.epd"), format!("{dir}/weights.txt"));
    fs::write(&dataset_path, dataset).unwrap();
    fs::write(&weights_path, weights).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_dumbfish-tuner"))
        .args([&dataset_path, &weights_path, &epochs.to_string()])
        .output()
        .unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    (
        output.status.success(),
        fs::read_to_string(&weights_path).unwrap(),
        stderr,
    )
}

fn index(name: &str) -> usize {
    FEATURES.iter().position(|f| f.name == name).unwrap()
}

#[test]
fn learns_relative_piece_values() {
    let pawn_up = "4k3/8/8/8/8/8/P7/4K3 w - -";
    let knight_up = "4k3/8/8/8/8/8/8/N3K3 w - -";
    let mut dataset = String::new();
    for (fen, result, count) in [
        (pawn_up, "1-0", 3),
        (pawn_up, "1/2-1/2", 1),
        (knight_up, "1-0", 1),
        (knight_up, "1/2-1/2", 3),
    ] {
        for _ in 0..count {
            dataset += &format!("{fen} c9 \"{result}\";\n");
        }
    }
    let (ok, weights, log) = tune(
        "relative",
        &dataset,
        include_str!("../../engine/weights.txt"),
        "500",
    );
    assert!(ok, "{log}");

    let tuned = parse_weights(&weights).unwrap();
    let (pawn, knight) = (tuned[index("pawn")], tuned[index("knight")]);
    assert!(
        knight[2] < pawn[2],
        "a lone knight wins less often than a pawn: {weights}"
    );
    let untouched =
        parse_weights(include_str!("../../engine/weights.txt")).unwrap()[index("queen")];
    assert_eq!(tuned[index("queen")], untouched, "no queens in the data");
}

#[test]
fn rejects_bad_input() {
    let weights = include_str!("../../engine/weights.txt");
    let (ok, after, log) = tune("bad", "4k3/8/8/8/8/8/P7/4K3 w - - c9;\n", weights, "10");
    assert!(!ok);
    assert!(log.contains("line 1: no result"), "{log}");
    assert_eq!(after, weights, "weights untouched on error");
}
