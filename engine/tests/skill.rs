use std::collections::HashSet;
use std::sync::atomic::AtomicBool;

use dumbfish::search::{self, Limits};
use dumbfish::skill::{MAX_LEVEL, Skill};
use dumbfish::tt::TranspositionTable;
use dumbfish_core::{Board, Move, MoveKind, Square};

fn skill(level: u8, seed: u64) -> Skill {
    Skill { level, seed }
}

fn search(fen: &str, limits: &Limits) -> (Option<Move>, u64) {
    let mut board = Board::from_fen(fen).unwrap();
    let mut tt = TranspositionTable::new(1);
    search::search(&mut board, limits, &AtomicBool::new(false), &mut tt, |_| {})
}

#[test]
fn lower_levels_are_weaker() {
    for level in 1..MAX_LEVEL {
        let (lower, higher) = (skill(level - 1, 0), skill(level, 0));
        assert!(lower.nodes() <= higher.nodes(), "level {level}");
        assert!(lower.noise() > higher.noise(), "level {level}");
    }
    assert_eq!(skill(MAX_LEVEL, 0).nodes(), None);
    assert_eq!(skill(MAX_LEVEL, 0).noise(), 0);
    assert_eq!(skill(0, 0).nodes(), Some(256));
}

#[test]
fn noise_picks_among_close_moves() {
    let mv = |to| Move::new(Square::E2, to, MoveKind::Normal);
    let scored = [
        (mv(Square::E3), 0),
        (mv(Square::E4), -10),
        (mv(Square::E5), -1000),
    ];
    for seed in 0..100 {
        assert_eq!(skill(MAX_LEVEL, seed).choose(&scored), Some(mv(Square::E3)));
    }
    let chosen: HashSet<_> = (0..100)
        .filter_map(|seed| skill(0, seed).choose(&scored))
        .collect();
    assert_eq!(chosen, HashSet::from([mv(Square::E3), mv(Square::E4)]));
    assert_eq!(skill(0, 7).choose(&scored), skill(0, 7).choose(&scored));
    assert_eq!(skill(0, 7).choose(&[]), None);
}

#[test]
fn node_budget_is_respected() {
    let limits = Limits {
        skill: Some(skill(0, 1)),
        ..Limits::default()
    };
    let (best, nodes) = search(dumbfish_core::board::STARTPOS, &limits);
    assert!(best.is_some());
    assert!(nodes <= 257, "{nodes}");
}

#[test]
fn full_level_is_full_strength() {
    let fen = "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1";
    let plain = Limits {
        depth: Some(4),
        ..Limits::default()
    };
    let full = Limits {
        depth: Some(4),
        skill: Some(skill(MAX_LEVEL, 99)),
        ..Limits::default()
    };
    assert_eq!(search(fen, &plain), search(fen, &full));
}

#[test]
fn weak_levels_vary_their_moves() {
    let moves: HashSet<_> = (0..20)
        .map(|seed| {
            let limits = Limits {
                skill: Some(skill(0, seed)),
                ..Limits::default()
            };
            search(dumbfish_core::board::STARTPOS, &limits).0
        })
        .collect();
    assert!(moves.len() >= 3, "{moves:?}");
}

#[test]
fn weak_levels_still_take_a_hanging_queen() {
    for seed in 0..20 {
        let limits = Limits {
            skill: Some(skill(0, seed)),
            ..Limits::default()
        };
        let (best, _) = search("4k3/8/8/3q4/4P3/8/8/4K3 w - - 0 1", &limits);
        assert_eq!(
            best.map(|m| m.to_string()).as_deref(),
            Some("e4d5"),
            "seed {seed}"
        );
    }
}
