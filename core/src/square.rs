use crate::bitboard::Bitboard;

macro_rules! squares {
    ($($sq:ident),*) => {
        /// Square in little-endian rank-file order: A1 = 0, B1 = 1, ..., H8 = 63.
        #[rustfmt::skip]
        #[repr(u8)]
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
        pub enum Square { $($sq),* }

        impl Square {
            #[rustfmt::skip]
            pub const ALL: [Square; 64] = [$(Square::$sq),*];
        }
    };
}

#[rustfmt::skip]
squares!(
    A1, B1, C1, D1, E1, F1, G1, H1,
    A2, B2, C2, D2, E2, F2, G2, H2,
    A3, B3, C3, D3, E3, F3, G3, H3,
    A4, B4, C4, D4, E4, F4, G4, H4,
    A5, B5, C5, D5, E5, F5, G5, H5,
    A6, B6, C6, D6, E6, F6, G6, H6,
    A7, B7, C7, D7, E7, F7, G7, H7,
    A8, B8, C8, D8, E8, F8, G8, H8
);

impl Square {
    /// `file` and `rank` are 0..8.
    pub const fn new(file: u8, rank: u8) -> Self {
        debug_assert!(file < 8 && rank < 8);
        Self::ALL[(rank * 8 + file) as usize]
    }

    pub const fn from_index(index: usize) -> Self {
        Self::ALL[index]
    }

    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn file(self) -> u8 {
        self as u8 % 8
    }

    pub const fn rank(self) -> u8 {
        self as u8 / 8
    }

    pub const fn bb(self) -> Bitboard {
        Bitboard(1 << self as u8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_file_rank_roundtrip() {
        for (i, sq) in Square::ALL.into_iter().enumerate() {
            assert_eq!(sq.index(), i);
            assert_eq!(Square::from_index(i), sq);
            assert_eq!(Square::new(sq.file(), sq.rank()), sq);
        }
        assert_eq!((Square::E4.file(), Square::E4.rank()), (4, 3));
        assert_eq!(Square::H8.index(), 63);
    }
}
