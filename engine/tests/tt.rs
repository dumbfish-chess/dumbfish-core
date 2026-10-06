use dumbfish::search::MATE;
use dumbfish::tt::{Bound, Entry, TranspositionTable, score_from_tt, score_to_tt};
use dumbfish_core::{Move, MoveKind, Square};

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
    let mut tt = TranspositionTable::new(1);
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
