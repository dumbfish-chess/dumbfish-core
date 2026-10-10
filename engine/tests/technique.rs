//! Mating a bare king without tablebases, against the longest defence the Syzygy tables in
//! `SYZYGY_PATH` (by default `target/syzygy`) give:
//! `cargo test --release -p dumbfish --test technique -- --include-ignored`

use std::sync::atomic::AtomicBool;

use dumbfish::search::{self, Limits};
use dumbfish::syzygy::Tablebases;
use dumbfish::tt::TranspositionTable;
use dumbfish_core::Board;

/// Nodes the attacker searches per move.
const NODES: u64 = 200_000;

/// Plies until the side to move in `fen` mates when it searches `NODES` per move without
/// tables and the defender plays the first of the moves the tables rank best; `None` if the
/// 50-move rule comes first.
fn plies_to_mate(tablebases: &Tablebases, fen: &str) -> Option<usize> {
    let mut board = Board::from_fen(fen).unwrap();
    let tt = TranspositionTable::new(16);
    let limits = Limits {
        nodes: Some(NODES),
        ..Limits::default()
    };
    let (stop, mut tables) = (AtomicBool::new(false), Vec::new());
    for ply in 0..100 {
        let moves = board.legal_moves();
        if moves.is_empty() {
            return board.in_check().then_some(ply);
        }
        let mv = if ply % 2 == 0 {
            let report = (|_: &search::Iteration| {}, |_, _, _| {});
            search::search_reporting(
                &mut board,
                &limits,
                &stop,
                &tt,
                &mut tables,
                report.0,
                report.1,
            )
            .0
            .unwrap()
        } else {
            tablebases.rank_root(&mut board, &moves).expect(fen).moves[0]
        };
        board.make_move(mv);
    }
    None
}

#[test]
#[ignore = "needs Syzygy tables"]
fn mates_a_bare_king_within_fifty_moves() {
    let dir = std::env::var("SYZYGY_PATH")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../target/syzygy").into());
    let tablebases = Tablebases::open(&dir);
    assert!(!tablebases.is_empty(), "no tables in {dir}");
    let results = [
        "8/8/8/4k3/8/8/8/KQ6 w - - 0 1",
        "8/8/8/4k3/8/8/8/KR6 w - - 0 1",
        "8/8/8/4k3/8/8/8/KBB5 w - - 0 1",
        "8/8/8/4k3/8/8/8/KBN5 w - - 0 1",
        "8/8/8/4k3/8/8/8/KNB5 w - - 0 1",
        "7k/8/5K2/8/8/8/8/5BN1 w - - 0 1",
        "k7/8/2K5/8/8/8/8/2B3N1 w - - 0 1",
        "8/8/3k4/8/8/3K4/8/b5n1 b - - 0 1",
    ]
    .map(|fen| (fen, plies_to_mate(&tablebases, fen)));
    println!("{results:?}");
    let failed: Vec<_> = results.iter().filter(|r| r.1.is_none()).collect();
    assert!(failed.is_empty(), "no mate within 50 moves: {results:?}");
}
