//! Standard perft set from https://www.chessprogramming.org/Perft_Results, and Chess960
//! positions checked against Stockfish 19 (`UCI_Chess960`, `go perft`).
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

/// Chess960 positions in Shredder-FEN: the first eight from the Chess960 perft list of the
/// Chess Programming Wiki, then a king that castles without moving and a castling rook that
/// shields the king's target square.
const CHESS960: [(&str, &[u64]); 10] = [
    (
        "bqnb1rkr/pp3ppp/3ppn2/2p5/5P2/P2P4/NPP1P1PP/BQ1BNRKR w HFhf - 2 9",
        &[21, 528, 12_189, 326_672, 8_146_062],
    ),
    (
        "2nnrbkr/p1qppppp/8/1ppb4/6PP/3PP3/PPP2P2/BQNNRBKR w HEhe - 1 9",
        &[21, 807, 18_002, 667_366, 16_253_601],
    ),
    (
        "b1q1rrkb/pppppppp/3nn3/8/P7/1PPP4/4PPPP/BQNNRKRB w GE - 1 9",
        &[20, 479, 10_471, 273_318, 6_417_013],
    ),
    (
        "qbbnnrkr/2pp2pp/p7/1p2pp2/8/P3PP2/1PPP1KPP/QBBNNR1R w hf - 0 9",
        &[22, 593, 13_440, 382_958, 9_183_776],
    ),
    (
        "1nbbnrkr/p1p1ppp1/3p4/1p3P1p/3Pq2P/8/PPP1P1P1/QNBBNRKR w HFhf - 0 9",
        &[28, 1_120, 31_058, 1_171_749, 34_030_312],
    ),
    (
        "qnbnr1kr/ppp1b1pp/4p3/3p1p2/8/2NPP3/PPP1BPPP/QNB1R1KR w HEhe - 1 9",
        &[29, 899, 26_578, 824_055, 24_851_983],
    ),
    (
        "q1bnrkr1/ppppp2p/2n2p2/4b1p1/2NP4/8/PPP1PPPP/QNB1RRKB w ge - 1 9",
        &[30, 860, 24_566, 732_757, 21_093_346],
    ),
    (
        "qbn1brkr/ppp1p1p1/2n4p/3p1p2/P7/6PP/QPPPPP2/1BNNBRKR w HFhf - 0 9",
        &[25, 635, 17_054, 465_806, 13_203_304],
    ),
    (
        "4k3/8/8/8/8/8/8/6KR w H - 0 1",
        &[12, 53, 948, 5_571, 102_638],
    ),
    (
        "4k3/8/8/8/8/8/8/qR1K4 w B - 0 1",
        &[7, 126, 1_558, 34_815, 468_742],
    ),
];

/// Checks every depth whose leaf count is at most `max_nodes`.
fn run_suite(max_nodes: u64) {
    for (fen, counts) in SUITE.into_iter().chain(CHESS960) {
        let mut board = Board::from_fen(fen).unwrap();
        let before = board.fen();
        for (depth, &expected) in (1..).zip(counts).filter(|&(_, &n)| n <= max_nodes) {
            assert_eq!(perft(&mut board, depth), expected, "{fen} at depth {depth}");
        }
        assert_eq!(board.fen(), before, "board not restored");
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
