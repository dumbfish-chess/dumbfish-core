use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use dumbfish::search::{self, Limits, MATE};
use dumbfish::skill::Skill;
use dumbfish::tt::TranspositionTable;
use dumbfish_core::{Board, Move};

/// Best move, nodes and the score of the last finished iteration.
fn search(fen: &str, limits: &Limits, stop: &AtomicBool) -> (Option<Move>, u64, Option<i32>) {
    let mut board = Board::from_fen(fen).unwrap();
    let tt = TranspositionTable::new(4);
    let mut score = None;
    let (best, nodes) = search::search(&mut board, limits, stop, &tt, |it| score = Some(it.score));
    (best, nodes, score)
}

const MATE_IN_TWO: &str = "7k/8/5K2/8/8/8/8/R7 w - - 0 1";
const KIWIPETE: &str = "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1";

#[test]
fn threads_agree_on_a_mate() {
    let limits = Limits {
        depth: Some(6),
        threads: 4,
        ..Limits::default()
    };
    let (best, _, score) = search(MATE_IN_TWO, &limits, &AtomicBool::new(false));
    assert!(best.is_some());
    assert_eq!(
        score,
        Some(MATE - 3),
        "mate in two moves is three plies away"
    );
}

#[test]
fn one_thread_is_the_default() {
    let one = Limits {
        depth: Some(5),
        threads: 1,
        ..Limits::default()
    };
    let default = Limits {
        depth: Some(5),
        ..Limits::default()
    };
    let stop = AtomicBool::new(false);
    assert_eq!(
        search(KIWIPETE, &one, &stop),
        search(KIWIPETE, &default, &stop)
    );
}

#[test]
fn weak_skill_searches_on_one_thread() {
    let limits = Limits {
        threads: 4,
        skill: Some(Skill { level: 0, seed: 3 }),
        ..Limits::default()
    };
    let (best, nodes, _) = search(KIWIPETE, &limits, &AtomicBool::new(false));
    assert!(best.is_some());
    assert!(nodes <= 257, "the node budget covers all threads: {nodes}");
}

#[test]
fn stop_ends_every_thread() {
    let limits = Limits {
        threads: 4,
        ..Limits::default()
    };
    let stop = AtomicBool::new(false);
    let start = Instant::now();
    thread::scope(|scope| {
        scope.spawn(|| {
            thread::sleep(Duration::from_millis(100));
            stop.store(true, Ordering::Relaxed);
        });
        let (best, nodes, _) = search(KIWIPETE, &limits, &stop);
        assert!(best.is_some() && nodes > 0);
    });
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "{:?}",
        start.elapsed()
    );
}
