//! Attack tables against slow reference implementations written independently of the engine.

use dumbfish_core::Square::*;
use dumbfish_core::attacks::{bishop, king, knight, pawn, queen, rook};
use dumbfish_core::{Bitboard, Color, Square};

const BISHOP_DIRS: [(i8, i8); 4] = [(1, 1), (1, -1), (-1, 1), (-1, -1)];
const ROOK_DIRS: [(i8, i8); 4] = [(0, 1), (0, -1), (1, 0), (-1, 0)];

fn offset(sq: Square, df: i8, dr: i8) -> Option<Square> {
    let (f, r) = (sq.file() as i8 + df, sq.rank() as i8 + dr);
    ((0..8).contains(&f) && (0..8).contains(&r)).then(|| Square::new(f as u8, r as u8))
}

fn leaper_slow(sq: Square, deltas: &[(i8, i8)]) -> Bitboard {
    deltas
        .iter()
        .filter_map(|&(df, dr)| offset(sq, df, dr))
        .fold(Bitboard::EMPTY, |acc, to| acc | to.bb())
}

fn slider_slow(sq: Square, occupied: Bitboard, dirs: &[(i8, i8)]) -> Bitboard {
    let mut attacks = Bitboard::EMPTY;
    for &(df, dr) in dirs {
        let mut cur = sq;
        while let Some(to) = offset(cur, df, dr) {
            attacks |= to.bb();
            if occupied.contains(to) {
                break;
            }
            cur = to;
        }
    }
    attacks
}

/// Squares whose occupancy can change the attacks: ray squares except the last one in each direction.
fn relevant_mask(sq: Square, dirs: &[(i8, i8)]) -> Bitboard {
    let mut mask = Bitboard::EMPTY;
    for &(df, dr) in dirs {
        let mut cur = sq;
        while let Some(to) = offset(cur, df, dr) {
            if offset(to, df, dr).is_some() {
                mask |= to.bb();
            }
            cur = to;
        }
    }
    mask
}

fn subsets(mask: Bitboard) -> impl Iterator<Item = Bitboard> {
    std::iter::successors(Some(0u64), move |&s| {
        let next = s.wrapping_sub(mask.0) & mask.0;
        (next != 0).then_some(next)
    })
    .map(Bitboard)
}

#[test]
fn leapers_match_reference() {
    let knight_deltas = [
        (1, 2),
        (2, 1),
        (2, -1),
        (1, -2),
        (-1, -2),
        (-2, -1),
        (-2, 1),
        (-1, 2),
    ];
    let king_deltas = [
        (0, 1),
        (1, 1),
        (1, 0),
        (1, -1),
        (0, -1),
        (-1, -1),
        (-1, 0),
        (-1, 1),
    ];
    for sq in Square::ALL {
        assert_eq!(knight(sq), leaper_slow(sq, &knight_deltas), "knight {sq:?}");
        assert_eq!(king(sq), leaper_slow(sq, &king_deltas), "king {sq:?}");
        assert_eq!(
            pawn(Color::White, sq),
            leaper_slow(sq, &[(-1, 1), (1, 1)]),
            "white pawn {sq:?}"
        );
        assert_eq!(
            pawn(Color::Black, sq),
            leaper_slow(sq, &[(-1, -1), (1, -1)]),
            "black pawn {sq:?}"
        );
    }
}

#[test]
fn sliders_match_reference_on_all_relevant_subsets() {
    let mut total = 0;
    for sq in Square::ALL {
        for occupied in subsets(relevant_mask(sq, &BISHOP_DIRS)) {
            assert_eq!(
                bishop(sq, occupied),
                slider_slow(sq, occupied, &BISHOP_DIRS),
                "bishop {sq:?} {occupied:?}"
            );
            total += 1;
        }
        for occupied in subsets(relevant_mask(sq, &ROOK_DIRS)) {
            assert_eq!(
                rook(sq, occupied),
                slider_slow(sq, occupied, &ROOK_DIRS),
                "rook {sq:?} {occupied:?}"
            );
            total += 1;
        }
    }
    assert_eq!(total, 5248 + 102_400);
}

#[test]
fn sliders_match_reference_on_random_boards() {
    // xorshift64*
    let mut state = 42u64;
    let mut next = || {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        state.wrapping_mul(0x2545_F491_4F6C_DD1D)
    };
    for sq in Square::ALL {
        for i in 0..2000 {
            // Vary density: ~50%, ~25%, ~12% of squares occupied.
            let mut occ = next();
            for _ in 0..i % 3 {
                occ &= next();
            }
            let occupied = Bitboard(occ);
            assert_eq!(
                bishop(sq, occupied),
                slider_slow(sq, occupied, &BISHOP_DIRS),
                "bishop {sq:?} {occ:#x}"
            );
            assert_eq!(
                rook(sq, occupied),
                slider_slow(sq, occupied, &ROOK_DIRS),
                "rook {sq:?} {occ:#x}"
            );
            assert_eq!(
                queen(sq, occupied),
                bishop(sq, occupied) | rook(sq, occupied)
            );
        }
    }
}

#[test]
fn known_values() {
    assert_eq!(rook(A1, Bitboard::EMPTY).count(), 14);
    assert_eq!(bishop(D4, Bitboard::EMPTY).count(), 13);
    assert_eq!(queen(D4, Bitboard::EMPTY).count(), 27);
    assert_eq!(rook(A1, B1.bb() | A2.bb()), B1.bb() | A2.bb());
    assert_eq!(knight(A1), B3.bb() | C2.bb());
}
