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

#[test]
fn soft_deadline_stops_new_iterations() {
    let limits = Limits {
        depth: Some(10),
        soft_deadline: Some(Instant::now()),
        ..Limits::default()
    };
    let mut board = Board::from_fen(KIWIPETE).unwrap();
    let tt = TranspositionTable::new(1);
    let mut iterations = 0;
    let (best, _) = search::search(&mut board, &limits, &AtomicBool::new(false), &tt, |_| {
        iterations += 1
    });
    assert!(best.is_some());
    assert_eq!(
        iterations, 1,
        "only the first iteration starts past the soft deadline"
    );
}

#[test]
fn principal_variation_is_a_legal_line_from_the_best_move() {
    for fen in [KIWIPETE, MATE_IN_TWO, dumbfish_core::board::STARTPOS] {
        let limits = Limits {
            depth: Some(7),
            ..Limits::default()
        };
        let mut board = Board::from_fen(fen).unwrap();
        let tt = TranspositionTable::new(4);
        let mut lines = Vec::new();
        search::search(&mut board, &limits, &AtomicBool::new(false), &tt, |it| {
            lines.push((it.best, it.pv.clone()))
        });
        assert!(!lines.is_empty());
        for (best, pv) in lines {
            assert_eq!(pv.first(), Some(&best), "{fen}");
            let mut line = Board::from_fen(fen).unwrap();
            for mv in &pv {
                assert!(line.legal_moves().contains(mv), "{fen}: {mv} in {pv:?}");
                line.make_move(*mv);
            }
        }
    }
}

/// Every reported line: depth, rank, score, best move and principal variation.
fn lines(fen: &str, limits: &Limits) -> Vec<(u32, usize, i32, Move, Vec<Move>)> {
    let mut board = Board::from_fen(fen).unwrap();
    let tt = TranspositionTable::new(4);
    let mut lines = Vec::new();
    search::search(&mut board, limits, &AtomicBool::new(false), &tt, |it| {
        lines.push((it.depth, it.multipv, it.score, it.best, it.pv.clone()))
    });
    lines
}

#[test]
fn multipv_reports_distinct_lines_best_first() {
    let limits = Limits {
        depth: Some(5),
        multipv: 3,
        ..Limits::default()
    };
    let all = lines(KIWIPETE, &limits);
    for depth in 1..=5 {
        let at_depth: Vec<_> = all.iter().filter(|l| l.0 == depth).collect();
        assert_eq!(
            at_depth.iter().map(|l| l.1).collect::<Vec<_>>(),
            [1, 2, 3],
            "depth {depth}"
        );
        assert!(
            at_depth.windows(2).all(|w| w[0].2 >= w[1].2),
            "scores fall at depth {depth}"
        );
        let mut firsts: Vec<_> = at_depth.iter().map(|l| l.3).collect();
        firsts.dedup();
        assert_eq!(firsts.len(), 3, "distinct moves at depth {depth}");
        for (_, _, _, best, pv) in at_depth {
            assert_eq!(pv.first(), Some(best));
        }
    }
}

#[test]
fn multipv_is_capped_by_the_legal_moves() {
    let limits = Limits {
        depth: Some(3),
        multipv: 5,
        ..Limits::default()
    };
    let fen = "4k3/8/8/8/8/8/4r3/4K3 w - - 0 1";
    let legal = Board::from_fen(fen).unwrap().legal_moves().len();
    assert_eq!(legal, 3, "Kxe2, Kd1, Kf1");
    let all = lines(fen, &limits);
    for depth in 1..=3 {
        let ranks: Vec<_> = all.iter().filter(|l| l.0 == depth).map(|l| l.1).collect();
        assert_eq!(ranks, [1, 2, 3], "depth {depth}");
    }
}

#[test]
fn iterations_report_seldepth_and_hashfull() {
    let limits = Limits {
        depth: Some(6),
        ..Limits::default()
    };
    let mut board = Board::from_fen(KIWIPETE).unwrap();
    let tt = TranspositionTable::new(1);
    assert_eq!(tt.hashfull(), 0);
    let mut reports = Vec::new();
    search::search(&mut board, &limits, &AtomicBool::new(false), &tt, |it| {
        reports.push((it.depth, it.seldepth, it.hashfull))
    });
    for (depth, seldepth, hashfull) in reports {
        assert!(seldepth >= depth, "seldepth {seldepth} below depth {depth}");
        assert!(hashfull <= 1000);
    }
    assert!(tt.hashfull() > 0, "the search stored entries");
}
