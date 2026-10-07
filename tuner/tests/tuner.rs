use std::fs;
use std::process::Command;

use dumbfish::eval::{FEATURE_NAMES, evaluate_with, parse_weights};
use dumbfish_core::Board;

fn tune(name: &str, dataset: &str, weights: &str, args: &[&str]) -> (bool, String, String) {
    let dir = format!("{}/{name}", env!("CARGO_TARGET_TMPDIR"));
    fs::create_dir_all(&dir).unwrap();
    let (dataset_path, weights_path) = (format!("{dir}/data.epd"), format!("{dir}/weights.txt"));
    fs::write(&dataset_path, dataset).unwrap();
    fs::write(&weights_path, weights).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_dumbfish-tuner"))
        .args([&dataset_path, &weights_path])
        .args(args)
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
    FEATURE_NAMES.iter().position(|n| n == name).unwrap()
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
        &["500"],
    );
    assert!(ok, "{log}");

    let tuned = parse_weights(&weights).unwrap();
    let score = |fen: &str| evaluate_with(&Board::from_fen(fen).unwrap(), &tuned);
    assert!(
        score(knight_up) < score(pawn_up),
        "a lone knight wins less often than a pawn"
    );
    let untouched =
        parse_weights(include_str!("../../engine/weights.txt")).unwrap()[index("queen")];
    assert_eq!(tuned[index("queen")], untouched, "no queens in the data");
}

#[test]
fn rejects_bad_input() {
    let weights = include_str!("../../engine/weights.txt");
    let (ok, after, log) = tune("bad", "4k3/8/8/8/8/8/P7/4K3 w - - c9;\n", weights, &["10"]);
    assert!(!ok);
    assert!(log.contains("line 1: no result"), "{log}");
    assert_eq!(after, weights, "weights untouched on error");
}

#[test]
fn rare_features_keep_their_weights() {
    let mut dataset = String::new();
    for i in 0..10_000 {
        let result = if i % 4 == 0 { "1/2-1/2" } else { "1-0" };
        dataset += &format!("4k3/8/8/8/8/8/P7/4K3 w - - c9 \"{result}\";\n");
    }
    dataset += "4k3/8/8/8/8/8/8/R3K3 w - - c9 \"0-1\";\n";
    let initial = include_str!("../../engine/weights.txt");
    let (ok, weights, log) = tune("rare", &dataset, initial, &["50"]);
    assert!(ok, "{log}");
    assert!(
        log.contains("kept: frozen groups or too little data"),
        "{log}"
    );
    let rook = index("rook");
    assert_eq!(
        parse_weights(&weights).unwrap()[rook],
        parse_weights(initial).unwrap()[rook],
        "one lost game with an extra rook is not evidence"
    );
}

#[test]
fn frozen_groups_keep_their_weights() {
    let dataset =
        "4k3/8/8/8/8/8/P7/4K3 w - - c9 \"1-0\";\n4k3/8/8/8/8/8/8/N3K3 w - - c9 \"1/2-1/2\";\n";
    let initial = include_str!("../../engine/weights.txt");
    let all = "--freeze=material,positional,pawns,mobility,king_attack,king_defense";
    let (ok, weights, log) = tune("frozen", dataset, initial, &["100", all]);
    assert!(ok, "{log}");
    assert_eq!(weights, initial, "every group frozen");

    let (ok, after, log) = tune("unknown", dataset, initial, &["--freeze=tempo"]);
    assert!(!ok);
    assert!(log.contains("unknown group \"tempo\""), "{log}");
    assert_eq!(after, initial);
}
