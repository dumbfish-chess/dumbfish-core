use std::ops::Not;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Color {
    White,
    Black,
}

impl Color {
    pub const fn index(self) -> usize {
        self as usize
    }
}

impl Not for Color {
    type Output = Self;

    fn not(self) -> Self {
        match self {
            Color::White => Color::Black,
            Color::Black => Color::White,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PieceKind {
    Pawn,
    Knight,
    Bishop,
    Rook,
    Queen,
    King,
}

impl PieceKind {
    pub const ALL: [PieceKind; 6] = [
        Self::Pawn,
        Self::Knight,
        Self::Bishop,
        Self::Rook,
        Self::Queen,
        Self::King,
    ];

    pub const fn index(self) -> usize {
        self as usize
    }

    /// Lowercase FEN letter.
    pub const fn char(self) -> char {
        b"pnbrqk"[self as usize] as char
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Piece {
    pub color: Color,
    pub kind: PieceKind,
}

impl Piece {
    pub const fn new(color: Color, kind: PieceKind) -> Self {
        Self { color, kind }
    }

    /// 0..12, white pieces first.
    pub const fn index(self) -> usize {
        self.color.index() * 6 + self.kind.index()
    }

    /// FEN letter: uppercase for white.
    pub fn char(self) -> char {
        match self.color {
            Color::White => self.kind.char().to_ascii_uppercase(),
            Color::Black => self.kind.char(),
        }
    }

    pub fn from_char(c: char) -> Option<Self> {
        let kind = PieceKind::ALL[b"pnbrqk"
            .iter()
            .position(|&k| k as char == c.to_ascii_lowercase())?];
        let color = if c.is_ascii_uppercase() {
            Color::White
        } else {
            Color::Black
        };
        Some(Self::new(color, kind))
    }
}
