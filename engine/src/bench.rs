use std::sync::atomic::AtomicBool;
use std::time::Instant;

use dumbfish_core::Board;

use crate::search::{self, Limits};
use crate::tt::{self, TranspositionTable};

pub const DEFAULT_DEPTH: u32 = 5;

/// One FEN per line; changing it or `DEFAULT_DEPTH` changes the bench signature.
const POSITIONS: &str = include_str!("../bench.epd");

/// Searches every position to `depth` and prints the total node count, which must be
/// identical on every machine; `nps` is informational.
pub fn run(depth: u32) {
    let stop = AtomicBool::new(false);
    let limits = Limits {
        depth: Some(depth),
        ..Limits::default()
    };
    let start = Instant::now();
    let mut tt = TranspositionTable::new(tt::DEFAULT_MEGABYTES);
    let mut nodes = 0;
    for fen in POSITIONS.lines() {
        let mut board = Board::from_fen(fen).expect("valid bench position");
        tt.clear();
        nodes += search::search(&mut board, &limits, &stop, &tt, |_| {}).1;
    }
    let nps = (nodes as f64 / start.elapsed().as_secs_f64().max(1e-3)) as u64;
    println!("{nodes} nodes {nps} nps");
}
