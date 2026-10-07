use std::fmt;

use crate::{PieceKind, Square};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum MoveKind {
    Normal,
    DoublePush,
    /// King move two squares; the rook is moved by `Board::make_move`.
    Castle,
    EnPassant,
    Promotion(PieceKind),
}

/// Packed move: bits 0..6 from, 6..12 to, 12..16 kind.
/// Castling is encoded as the king's move (e1g1), like in UCI.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Move(u16);

impl Move {
    pub const fn new(from: Square, to: Square, kind: MoveKind) -> Self {
        let flag = match kind {
            MoveKind::Normal => 0,
            MoveKind::DoublePush => 1,
            MoveKind::Castle => 2,
            MoveKind::EnPassant => 3,
            MoveKind::Promotion(piece) => 3 + piece as u16,
        };
        Self(from as u16 | (to as u16) << 6 | flag << 12)
    }

    pub const fn from(self) -> Square {
        Square::from_index((self.0 & 63) as usize)
    }

    pub const fn to(self) -> Square {
        Square::from_index((self.0 >> 6 & 63) as usize)
    }

    /// The packed 16 bits, to store a move compactly.
    pub const fn bits(self) -> u16 {
        self.0
    }

    /// A move from `bits`; `None` if no move packs to them.
    pub const fn from_bits(bits: u16) -> Option<Self> {
        if bits >> 12 <= 7 {
            Some(Self(bits))
        } else {
            None
        }
    }

    pub const fn kind(self) -> MoveKind {
        match self.0 >> 12 {
            0 => MoveKind::Normal,
            1 => MoveKind::DoublePush,
            2 => MoveKind::Castle,
            3 => MoveKind::EnPassant,
            flag => MoveKind::Promotion(PieceKind::ALL[flag as usize - 3]),
        }
    }
}

impl fmt::Display for Move {
    /// UCI notation: `e2e4`, `e7e8q`.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}{}", self.from(), self.to())?;
        if let MoveKind::Promotion(piece) = self.kind() {
            write!(f, "{}", piece.char())?;
        }
        Ok(())
    }
}
