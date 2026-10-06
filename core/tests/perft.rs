//! Standard perft set from https://www.chessprogramming.org/Perft_Results.
//!
//! `cargo test` runs the shallow depths; the full set needs a release build:
//! `cargo test --release -p dumbfish-core --test perft -- --include-ignored`

use dumbfish_core::Board;
use dumbfish_core::perft::{divide, perft};

/// FEN and leaf counts for depth 1, 2, ...
const SUITE: [(&str, &[u64]); 7] = [
    (
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        &[20, 400, 8_902, 197_281, 4_865_609, 119_060_324],
    ),
    (
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        &[48, 2_039, 97_862, 4_085_603, 193_690_690],
    ),
    (
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        &[14, 191, 2_812, 43_238, 674_624, 11_030_083, 178_633_661],
    ),
    (
        "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
        &[6, 264, 9_467, 422_333, 15_833_292, 706_045_033],
    ),
    (
        "r2q1rk1/pP1p2pp/Q4n2/bbp1p3/Np6/1B3NBn/pPPP1PPP/R3K2R b KQ - 0 1",
        &[6, 264, 9_467, 422_333, 15_833_292, 706_045_033],
    ),
    (
        "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
        &[44, 1_486, 62_379, 2_103_487, 89_941_194],
    ),
    (
        "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
        &[46, 2_079, 89_890, 3_894_594, 164_075_551],
    ),
];

/// Checks every depth whose leaf count is at most `max_nodes`.
fn run_suite(max_nodes: u64) {
    for (fen, counts) in SUITE {
        let mut board = Board::from_fen(fen).unwrap();
        for (depth, &expected) in (1..).zip(counts).filter(|&(_, &n)| n <= max_nodes) {
            assert_eq!(perft(&mut board, depth), expected, "{fen} at depth {depth}");
        }
        assert_eq!(board.fen(), fen, "board not restored");
    }
}

#[test]
fn standard_suite_shallow() {
    run_suite(5_000_000);
}

#[test]
#[ignore = "takes seconds in release, minutes in debug"]
fn standard_suite_full() {
    run_suite(u64::MAX);
}

#[test]
fn divide_splits_perft() {
    let mut board = Board::startpos();
    let split = divide(&mut board, 3);
    assert_eq!(split.len(), 20);
    assert_eq!(split.iter().map(|&(_, n)| n).sum::<u64>(), 8_902);
    let e2e4 = split
        .iter()
        .find(|(mv, _)| mv.to_string() == "e2e4")
        .unwrap();
    assert_eq!(e2e4.1, 600);
    assert_eq!(perft(&mut board, 0), 1);
    assert!(divide(&mut board, 0).is_empty());
}
