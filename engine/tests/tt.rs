use dumbfish::search::MATE;
use dumbfish::tt::{Bound, Entry, TranspositionTable, score_from_tt, score_to_tt};
use dumbfish_core::{Move, MoveKind, PieceKind, Square};

fn entry(key: u64, score: i32) -> Entry {
    Entry {
        key,
        best: Some(Move::new(Square::E2, Square::E4, MoveKind::DoublePush)),
        score,
        depth: 3,
        bound: Bound::Exact,
    }
}

#[test]
fn store_probe_clear() {
    let mut tt = TranspositionTable::new(1);
    assert!(tt.probe(42).is_none());
    tt.store(entry(42, 17));
    assert_eq!(tt.probe(42).map(|e| e.score), Some(17));
    assert!(tt.probe(43).is_none(), "different key");
    tt.clear();
    assert!(tt.probe(42).is_none());
}

#[test]
fn same_slot_is_replaced() {
    let tt = TranspositionTable::new(1);
    tt.store(entry(5, 1));
    tt.store(entry(5 + (1 << 40), 2));
    assert!(
        tt.probe(5).is_none(),
        "keys differing only in high bits share a slot"
    );
    assert_eq!(tt.probe(5 + (1 << 40)).map(|e| e.score), Some(2));
}

#[test]
fn mate_scores_are_stored_relative_to_the_node() {
    let mate_at_ply_7 = MATE - 7;
    let stored = score_to_tt(mate_at_ply_7, 3);
    assert_eq!(score_from_tt(stored, 3), mate_at_ply_7);
    assert_eq!(
        score_from_tt(stored, 5),
        MATE - 9,
        "same mate seen two plies deeper"
    );
    let mated_at_ply_6 = 6 - MATE;
    assert_eq!(score_from_tt(score_to_tt(mated_at_ply_6, 2), 4), 8 - MATE);
    for score in [-500, 0, 123] {
        assert_eq!(
            score_from_tt(score_to_tt(score, 10), 4),
            score,
            "plain scores do not move"
        );
    }
}

#[test]
fn entries_roundtrip() {
    let tt = TranspositionTable::new(1);
    let moves = [
        None,
        Some(Move::new(
            Square::A7,
            Square::B8,
            MoveKind::Promotion(PieceKind::Knight),
        )),
        Some(Move::new(Square::E1, Square::G1, MoveKind::Castle)),
    ];
    let mut key = 1u64;
    for best in moves {
        for score in [
            0,
            -1,
            17,
            -31_000 + 5,
            31_000 - 3,
            i16::MAX as i32,
            i16::MIN as i32,
        ] {
            for (depth, bound) in [(0, Bound::Exact), (1, Bound::Lower), (255, Bound::Upper)] {
                key = key.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1);
                let e = Entry {
                    key,
                    best,
                    score,
                    depth,
                    bound,
                };
                tt.store(e);
                assert_eq!(tt.probe(key), Some(e));
            }
        }
    }
    assert!(
        tt.probe(0).is_none(),
        "an empty slot is no entry, even for key 0"
    );
}

/// Every key always gets the same entry, so whatever a probe returns can be checked.
fn entry_for(key: u64) -> Entry {
    Entry {
        key,
        best: Move::from_bits((key >> 20) as u16 & 0x7FFF),
        score: (key >> 40) as i16 as i32,
        depth: (key >> 8) as u8,
        bound: [Bound::Exact, Bound::Lower, Bound::Upper][(key % 3) as usize],
    }
}

/// Smoke test of concurrent use: 16 keys differing only above bit 20 share one slot, so writes
/// collide and probes hit all the time. On x86 both halves of a slot share a cache line and torn
/// entries are too rare to provoke, so this does not prove the key check is needed.
#[test]
fn concurrent_use_keeps_entries_consistent() {
    let tt = TranspositionTable::new(1);
    std::thread::scope(|scope| {
        for thread in 0..8u64 {
            let tt = &tt;
            scope.spawn(move || {
                let mut state = thread + 1;
                for _ in 0..1_000_000 {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    let key = (state % 16 + 1) << 20;
                    if state % 2 == 0 {
                        tt.store(entry_for(key));
                    } else if let Some(e) = tt.probe(key) {
                        assert_eq!(e, entry_for(key), "torn entry for {key:#x}");
                    }
                }
            });
        }
    });
}
