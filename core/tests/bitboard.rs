use dumbfish_core::Square::*;
use dumbfish_core::{Bitboard, Square};

fn bb(squares: &[Square]) -> Bitboard {
    squares
        .iter()
        .fold(Bitboard::EMPTY, |acc, &sq| acc | sq.bb())
}

#[test]
fn files_and_ranks() {
    assert_eq!(Bitboard::FILE_A, bb(&[A1, A2, A3, A4, A5, A6, A7, A8]));
    assert_eq!(Bitboard::rank(7), bb(&[A8, B8, C8, D8, E8, F8, G8, H8]));
    for sq in Square::ALL {
        assert!(Bitboard::file(sq.file()).contains(sq));
        assert!(Bitboard::rank(sq.rank()).contains(sq));
        assert_eq!(
            (Bitboard::file(sq.file()) & Bitboard::rank(sq.rank())),
            sq.bb()
        );
    }
}

#[test]
fn iterate_and_pop() {
    let b = bb(&[H8, A1, E4]);
    assert_eq!(b.count(), 3);
    assert_eq!(b.into_iter().len(), 3);
    assert_eq!(b.lsb(), A1);
    assert_eq!(b.into_iter().collect::<Vec<_>>(), [A1, E4, H8]);
    assert_eq!(Bitboard::ALL.count(), 64);
    assert!(Bitboard::ALL.into_iter().eq(Square::ALL));
    assert_eq!(Bitboard::EMPTY.into_iter().next(), None);

    let mut b = b;
    assert_eq!(b.pop_lsb(), A1);
    assert_eq!(b, bb(&[E4, H8]));
}

#[test]
fn set_ops() {
    let a = bb(&[A1, B2]);
    let b = bb(&[B2, C3]);
    assert_eq!(a & b, bb(&[B2]));
    assert_eq!(a | b, bb(&[A1, B2, C3]));
    assert_eq!(a ^ b, bb(&[A1, C3]));
    assert_eq!((!a).count(), 62);
    let mut c = a;
    c ^= B2.into();
    c |= H8.into();
    c &= !Bitboard::from(A1);
    assert_eq!(c, H8.bb());
}

#[test]
fn shifts_inside_board() {
    let e4 = E4.bb();
    assert_eq!(e4.north(), E5.bb());
    assert_eq!(e4.south(), E3.bb());
    assert_eq!(e4.east(), F4.bb());
    assert_eq!(e4.west(), D4.bb());
    assert_eq!(e4.north_east(), F5.bb());
    assert_eq!(e4.north_west(), D5.bb());
    assert_eq!(e4.south_east(), F3.bb());
    assert_eq!(e4.south_west(), D3.bb());
}

#[test]
fn shifts_do_not_wrap() {
    let shifts: [fn(Bitboard) -> Bitboard; 8] = [
        Bitboard::north,
        Bitboard::south,
        Bitboard::east,
        Bitboard::west,
        Bitboard::north_east,
        Bitboard::north_west,
        Bitboard::south_east,
        Bitboard::south_west,
    ];
    let deltas = [
        (0, 1),
        (0, -1),
        (1, 0),
        (-1, 0),
        (1, 1),
        (-1, 1),
        (1, -1),
        (-1, -1),
    ];
    for (shift, (df, dr)) in shifts.into_iter().zip(deltas) {
        for sq in Square::ALL {
            let (f, r) = (sq.file() as i8 + df, sq.rank() as i8 + dr);
            let expected = if (0..8).contains(&f) && (0..8).contains(&r) {
                Square::new(f as u8, r as u8).bb()
            } else {
                Bitboard::EMPTY
            };
            assert_eq!(shift(sq.bb()), expected, "{sq:?} by ({df}, {dr})");
        }
    }
}
