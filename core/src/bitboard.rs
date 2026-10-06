use std::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, BitXor, BitXorAssign, Not};

use crate::square::Square;

/// Set of squares, bit `i` is `Square::from_index(i)`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug)]
pub struct Bitboard(pub u64);

impl Bitboard {
    pub const EMPTY: Self = Self(0);
    pub const ALL: Self = Self(!0);
    pub const FILE_A: Self = Self::file(0);
    pub const FILE_H: Self = Self::file(7);

    /// `file` is 0..8 (A..H).
    pub const fn file(file: u8) -> Self {
        Self(0x0101_0101_0101_0101 << file)
    }

    /// `rank` is 0..8 (1..8).
    pub const fn rank(rank: u8) -> Self {
        Self(0xFF << (rank * 8))
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub const fn count(self) -> u32 {
        self.0.count_ones()
    }

    pub const fn contains(self, sq: Square) -> bool {
        self.0 & sq.bb().0 != 0
    }

    /// Lowest square. The board must be non-empty.
    pub const fn lsb(self) -> Square {
        debug_assert!(!self.is_empty());
        Square::from_index(self.0.trailing_zeros() as usize)
    }

    /// Removes and returns the lowest square. The board must be non-empty.
    pub fn pop_lsb(&mut self) -> Square {
        let sq = self.lsb();
        self.0 &= self.0 - 1;
        sq
    }

    // Shifts by one step; bits leaving the board are dropped, nothing wraps across files.
    pub const fn north(self) -> Self {
        Self(self.0 << 8)
    }

    pub const fn south(self) -> Self {
        Self(self.0 >> 8)
    }

    pub const fn east(self) -> Self {
        Self((self.0 & !Self::FILE_H.0) << 1)
    }

    pub const fn west(self) -> Self {
        Self((self.0 & !Self::FILE_A.0) >> 1)
    }

    pub const fn north_east(self) -> Self {
        Self((self.0 & !Self::FILE_H.0) << 9)
    }

    pub const fn north_west(self) -> Self {
        Self((self.0 & !Self::FILE_A.0) << 7)
    }

    pub const fn south_east(self) -> Self {
        Self((self.0 & !Self::FILE_H.0) >> 7)
    }

    pub const fn south_west(self) -> Self {
        Self((self.0 & !Self::FILE_A.0) >> 9)
    }
}

impl IntoIterator for Bitboard {
    type Item = Square;
    type IntoIter = Squares;

    fn into_iter(self) -> Squares {
        Squares(self)
    }
}

/// Squares of a bitboard from A1 to H8.
pub struct Squares(Bitboard);

impl Iterator for Squares {
    type Item = Square;

    fn next(&mut self) -> Option<Square> {
        (!self.0.is_empty()).then(|| self.0.pop_lsb())
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.0.count() as usize;
        (n, Some(n))
    }
}

impl ExactSizeIterator for Squares {}

impl From<Square> for Bitboard {
    fn from(sq: Square) -> Self {
        sq.bb()
    }
}

impl Not for Bitboard {
    type Output = Self;

    fn not(self) -> Self {
        Self(!self.0)
    }
}

macro_rules! bit_op {
    ($op:ident, $f:ident, $op_assign:ident, $f_assign:ident) => {
        impl $op for Bitboard {
            type Output = Self;

            fn $f(self, rhs: Self) -> Self {
                Self($op::$f(self.0, rhs.0))
            }
        }

        impl $op_assign for Bitboard {
            fn $f_assign(&mut self, rhs: Self) {
                $op_assign::$f_assign(&mut self.0, rhs.0)
            }
        }
    };
}

bit_op!(BitAnd, bitand, BitAndAssign, bitand_assign);
bit_op!(BitOr, bitor, BitOrAssign, bitor_assign);
bit_op!(BitXor, bitxor, BitXorAssign, bitxor_assign);

#[cfg(test)]
mod tests {
    use super::*;
    use Square::*;

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
}
