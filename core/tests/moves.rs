use dumbfish_core::Square::*;
use dumbfish_core::{Move, MoveKind, PieceKind};

#[test]
fn pack_unpack() {
    let kinds = [
        MoveKind::Normal,
        MoveKind::DoublePush,
        MoveKind::Castle,
        MoveKind::EnPassant,
        MoveKind::Promotion(PieceKind::Knight),
        MoveKind::Promotion(PieceKind::Bishop),
        MoveKind::Promotion(PieceKind::Rook),
        MoveKind::Promotion(PieceKind::Queen),
    ];
    for kind in kinds {
        for (from, to) in [(A1, H8), (H8, A1), (E2, E4), (G7, G8)] {
            let m = Move::new(from, to, kind);
            assert_eq!((m.from(), m.to(), m.kind()), (from, to, kind));
        }
    }
    assert_eq!(
        Move::new(E7, E8, MoveKind::Promotion(PieceKind::Queen)).to_string(),
        "e7e8q"
    );
    assert_eq!(Move::new(E1, G1, MoveKind::Castle).to_string(), "e1g1");
}

#[test]
fn bits_roundtrip() {
    for kind in [
        MoveKind::Normal,
        MoveKind::Castle,
        MoveKind::Promotion(PieceKind::Queen),
    ] {
        let m = Move::new(G7, H8, kind);
        assert_eq!(Move::from_bits(m.bits()), Some(m));
    }
    assert_eq!(Move::from_bits(8 << 12), None, "flag 8 is no move kind");
    assert_eq!(Move::from_bits(u16::MAX), None);
}
