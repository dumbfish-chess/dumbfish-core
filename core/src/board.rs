use std::fmt::Write as _;

use crate::{Bitboard, Color, Move, MoveKind, Piece, PieceKind, Square, attacks, zobrist};

pub const STARTPOS: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

pub const WHITE_KINGSIDE: u8 = 1;
pub const WHITE_QUEENSIDE: u8 = 2;
pub const BLACK_KINGSIDE: u8 = 4;
pub const BLACK_QUEENSIDE: u8 = 8;

/// The castling right of `color` on the king's side or the queen's side.
pub const fn castling_right(color: Color, kingside: bool) -> u8 {
    match (color, kingside) {
        (Color::White, true) => WHITE_KINGSIDE,
        (Color::White, false) => WHITE_QUEENSIDE,
        (Color::Black, true) => BLACK_KINGSIDE,
        (Color::Black, false) => BLACK_QUEENSIDE,
    }
}

/// State that `make_move` destroys and `unmake_move` restores.
#[derive(Clone, Copy, Debug)]
struct Undo {
    mv: Move,
    captured: Option<Piece>,
    castling: u8,
    en_passant: Option<Square>,
    halfmove: u16,
    hash: u64,
}

#[derive(Clone, Debug)]
pub struct Board {
    kinds: [Bitboard; 6],
    colors: [Bitboard; 2],
    mailbox: [Option<Piece>; 64],
    side: Color,
    castling: u8,
    /// Starting square of the rook of each castling right, indexed by the right's bit; any
    /// file in Chess960.
    castling_rooks: [Square; 4],
    /// Castling rights that survive a move from or to each square.
    castling_mask: [u8; 64],
    /// Set only when a pawn of the side to move attacks it, so the hash
    /// does not depend on en passant squares that cannot be used.
    en_passant: Option<Square>,
    halfmove: u16,
    fullmove: u16,
    hash: u64,
    history: Vec<Undo>,
}

impl Board {
    pub fn startpos() -> Self {
        Self::from_fen(STARTPOS).expect("valid startpos")
    }

    pub fn side_to_move(&self) -> Color {
        self.side
    }

    pub fn piece_on(&self, sq: Square) -> Option<Piece> {
        self.mailbox[sq.index()]
    }

    pub fn kind_bb(&self, kind: PieceKind) -> Bitboard {
        self.kinds[kind.index()]
    }

    pub fn color_bb(&self, color: Color) -> Bitboard {
        self.colors[color.index()]
    }

    pub fn pieces(&self, color: Color, kind: PieceKind) -> Bitboard {
        self.color_bb(color) & self.kind_bb(kind)
    }

    pub fn occupied(&self) -> Bitboard {
        self.colors[0] | self.colors[1]
    }

    pub fn king(&self, color: Color) -> Square {
        self.pieces(color, PieceKind::King).lsb()
    }

    /// Mask of `WHITE_KINGSIDE` | ... | `BLACK_QUEENSIDE`.
    pub fn castling(&self) -> u8 {
        self.castling
    }

    pub fn en_passant(&self) -> Option<Square> {
        self.en_passant
    }

    pub fn halfmove_clock(&self) -> u16 {
        self.halfmove
    }

    pub fn fullmove_number(&self) -> u16 {
        self.fullmove
    }

    pub fn hash(&self) -> u64 {
        self.hash
    }

    /// The position already occurred with the same side to move since the last capture or pawn
    /// move, counting the moves made on this board.
    pub fn is_repetition(&self) -> bool {
        self.history
            .iter()
            .rev()
            .take(self.halfmove as usize)
            .skip(1)
            .step_by(2)
            .any(|undo| undo.hash == self.hash)
    }

    /// Pieces of both colors attacking `sq`, with sliders seeing through everything not in `occupied`.
    pub fn attackers_to(&self, sq: Square, occupied: Bitboard) -> Bitboard {
        use PieceKind::*;
        let diagonal = self.kind_bb(Bishop) | self.kind_bb(Queen);
        let straight = self.kind_bb(Rook) | self.kind_bb(Queen);
        (attacks::pawn(Color::White, sq) & self.pieces(Color::Black, Pawn))
            | (attacks::pawn(Color::Black, sq) & self.pieces(Color::White, Pawn))
            | (attacks::knight(sq) & self.kind_bb(Knight))
            | (attacks::king(sq) & self.kind_bb(King))
            | (attacks::bishop(sq, occupied) & diagonal)
            | (attacks::rook(sq, occupied) & straight)
    }

    /// The start and end squares of the rook in the castling move `mv`.
    pub fn castling_rook(&self, mv: Move) -> (Square, Square) {
        let (rank, kingside) = (mv.from().rank(), mv.to().file() == 6);
        let color = if rank == 0 {
            Color::White
        } else {
            Color::Black
        };
        let right = castling_right(color, kingside);
        let to = Square::new(if kingside { 5 } else { 3 }, rank);
        (self.castling_rooks[right.trailing_zeros() as usize], to)
    }

    /// Plays a move that must be at least pseudo-legal in this position.
    pub fn make_move(&mut self, mv: Move) {
        let (from, to, kind) = (mv.from(), mv.to(), mv.kind());
        let us = self.side;
        let captured_sq = match kind {
            MoveKind::EnPassant => Square::new(to.file(), from.rank()),
            _ => to,
        };
        let captured = match kind {
            MoveKind::Castle => None,
            _ => self.mailbox[captured_sq.index()],
        };
        self.history.push(Undo {
            mv,
            captured,
            castling: self.castling,
            en_passant: self.en_passant,
            halfmove: self.halfmove,
            hash: self.hash,
        });

        if let Some(ep) = self.en_passant.take() {
            self.hash ^= zobrist::en_passant(ep);
        }
        self.halfmove += 1;
        if kind == MoveKind::Castle {
            let (rook_from, rook_to) = self.castling_rook(mv);
            let king = self.remove(from);
            let rook = self.remove(rook_from);
            self.put(king, to);
            self.put(rook, rook_to);
        } else {
            if captured.is_some() {
                self.remove(captured_sq);
                self.halfmove = 0;
            }
            let piece = self.remove(from);
            if piece.kind == PieceKind::Pawn {
                self.halfmove = 0;
            }
            match kind {
                MoveKind::Promotion(promoted) => self.put(Piece::new(us, promoted), to),
                _ => self.put(piece, to),
            }
            if kind == MoveKind::DoublePush {
                let ep = Square::new(from.file(), (from.rank() + to.rank()) / 2);
                if !(attacks::pawn(us, ep) & self.pieces(!us, PieceKind::Pawn)).is_empty() {
                    self.en_passant = Some(ep);
                    self.hash ^= zobrist::en_passant(ep);
                }
            }
        }

        let castling =
            self.castling & self.castling_mask[from.index()] & self.castling_mask[to.index()];
        self.hash ^= zobrist::castling(self.castling) ^ zobrist::castling(castling);
        self.castling = castling;

        if us == Color::Black {
            self.fullmove += 1;
        }
        self.side = !us;
        self.hash ^= zobrist::side();
    }

    /// Passes the turn without moving, for null move pruning; the side to move must not be in
    /// check. Positions before it do not count for `is_repetition`. Taken back by
    /// `unmake_null_move`.
    pub fn make_null_move(&mut self) {
        self.history.push(Undo {
            mv: Move::new(Square::A1, Square::A1, MoveKind::Normal),
            captured: None,
            castling: self.castling,
            en_passant: self.en_passant,
            halfmove: self.halfmove,
            hash: self.hash,
        });
        if let Some(ep) = self.en_passant.take() {
            self.hash ^= zobrist::en_passant(ep);
        }
        self.halfmove = 0;
        self.side = !self.side;
        self.hash ^= zobrist::side();
    }

    pub fn unmake_null_move(&mut self) {
        let undo = self.history.pop().expect("no null move to unmake");
        self.side = !self.side;
        self.en_passant = undo.en_passant;
        self.halfmove = undo.halfmove;
        self.hash = undo.hash;
    }

    /// Takes back the last `make_move`.
    pub fn unmake_move(&mut self) {
        let undo = self.history.pop().expect("no move to unmake");
        let (from, to, kind) = (undo.mv.from(), undo.mv.to(), undo.mv.kind());
        self.side = !self.side;
        let us = self.side;
        if us == Color::Black {
            self.fullmove -= 1;
        }

        if kind == MoveKind::Castle {
            let (rook_from, rook_to) = self.castling_rook(undo.mv);
            let king = self.remove(to);
            let rook = self.remove(rook_to);
            self.put(king, from);
            self.put(rook, rook_from);
        } else {
            let piece = self.remove(to);
            match kind {
                MoveKind::Promotion(_) => self.put(Piece::new(us, PieceKind::Pawn), from),
                _ => self.put(piece, from),
            }
        }
        if let Some(captured) = undo.captured {
            let captured_sq = match kind {
                MoveKind::EnPassant => Square::new(to.file(), from.rank()),
                _ => to,
            };
            self.put(captured, captured_sq);
        }

        self.castling = undo.castling;
        self.en_passant = undo.en_passant;
        self.halfmove = undo.halfmove;
        self.hash = undo.hash;
    }

    pub fn from_fen(fen: &str) -> Result<Self, String> {
        let mut board = Self {
            kinds: [Bitboard::EMPTY; 6],
            colors: [Bitboard::EMPTY; 2],
            mailbox: [None; 64],
            side: Color::White,
            castling: 0,
            castling_rooks: [Square::H1, Square::A1, Square::H8, Square::A8],
            castling_mask: [!0; 64],
            en_passant: None,
            halfmove: 0,
            fullmove: 1,
            hash: 0,
            history: Vec::new(),
        };
        let mut fields = fen.split_whitespace();

        let placement = fields.next().ok_or("empty FEN")?;
        let rows: Vec<_> = placement.split('/').collect();
        if rows.len() != 8 {
            return Err(format!("expected 8 ranks: {placement:?}"));
        }
        for (row, rank) in rows.into_iter().zip((0..8).rev()) {
            let mut file = 0;
            for c in row.chars() {
                match c {
                    '1'..='8' => file += c as u8 - b'0',
                    _ => {
                        let piece =
                            Piece::from_char(c).ok_or_else(|| format!("invalid piece {c:?}"))?;
                        if file < 8 {
                            board.put(piece, Square::new(file, rank));
                        }
                        file += 1;
                    }
                }
                if file > 8 {
                    return Err(format!("rank {} is longer than 8 squares", rank + 1));
                }
            }
            if file != 8 {
                return Err(format!("rank {} is shorter than 8 squares", rank + 1));
            }
        }

        board.side = match fields.next() {
            Some("w") => Color::White,
            Some("b") => Color::Black,
            other => return Err(format!("invalid side to move: {other:?}")),
        };

        match fields.next() {
            Some("-") => {}
            Some(rights) => {
                for c in rights.chars() {
                    board.add_castling_right(c)?;
                }
            }
            None => return Err("missing castling rights".into()),
        }

        board.en_passant = match fields.next() {
            Some("-") => None,
            Some(s) => Some(s.parse()?),
            None => return Err("missing en passant square".into()),
        };

        let mut number = |name: &str, default: u16| match fields.next() {
            Some(s) => s.parse().map_err(|_| format!("invalid {name}: {s:?}")),
            None => Ok(default),
        };
        board.halfmove = number("halfmove clock", 0)?;
        board.fullmove = number("fullmove number", 1)?.max(1);
        if let Some(extra) = fields.next() {
            return Err(format!("unexpected trailing field: {extra:?}"));
        }

        board.validate()?;
        if let Some(ep) = board.en_passant {
            let us = board.side;
            if (attacks::pawn(!us, ep) & board.pieces(us, PieceKind::Pawn)).is_empty() {
                board.en_passant = None;
            }
        }
        board.hash = board.compute_hash();
        Ok(board)
    }

    /// Adds the castling right written as `c`: `K`/`Q` for the outermost rook on the king's or
    /// the queen's side of the king, or the rook's file (`A`..`H`, Shredder-FEN and X-FEN for
    /// Chess960); lowercase for black.
    fn add_castling_right(&mut self, c: char) -> Result<(), String> {
        use PieceKind::*;
        let color = if c.is_ascii_uppercase() {
            Color::White
        } else {
            Color::Black
        };
        let rank = if color == Color::White { 0 } else { 7 };
        let kings = self.pieces(color, King);
        if kings.count() != 1 || kings.lsb().rank() != rank {
            return Err(format!(
                "castling right {c:?} without a king on its first rank"
            ));
        }
        let king = kings.lsb();
        let rooks = self.pieces(color, Rook) & Bitboard::rank(rank);
        let rook = match c.to_ascii_uppercase() {
            'K' => rooks.into_iter().filter(|r| r.file() > king.file()).last(),
            'Q' => rooks.into_iter().find(|r| r.file() < king.file()),
            file @ 'A'..='H' => Some(Square::new(file as u8 - b'A', rank))
                .filter(|&r| rooks.contains(r) && r.file() != king.file()),
            _ => return Err(format!("invalid castling right {c:?}")),
        };
        let rook = rook.ok_or_else(|| format!("castling right {c:?} without its rook"))?;
        let right = castling_right(color, rook.file() > king.file());
        self.castling |= right;
        self.castling_rooks[right.trailing_zeros() as usize] = rook;
        self.castling_mask[rook.index()] &= !right;
        self.castling_mask[king.index()] &= !right;
        Ok(())
    }

    /// Rejects positions the move generator cannot handle.
    fn validate(&self) -> Result<(), String> {
        use PieceKind::*;
        for color in [Color::White, Color::Black] {
            if self.pieces(color, King).count() != 1 {
                return Err(format!("{color:?} must have exactly one king"));
            }
        }
        if !(self.kind_bb(Pawn) & (Bitboard::rank(0) | Bitboard::rank(7))).is_empty() {
            return Err("pawn on the first or last rank".into());
        }

        if let Some(ep) = self.en_passant {
            let (ep_rank, pawn_rank, origin_rank) = match self.side {
                Color::White => (5, 4, 6),
                Color::Black => (2, 3, 1),
            };
            if ep.rank() != ep_rank
                || self.piece_on(Square::new(ep.file(), pawn_rank))
                    != Some(Piece::new(!self.side, Pawn))
                || self.piece_on(ep).is_some()
                || self.piece_on(Square::new(ep.file(), origin_rank)).is_some()
            {
                return Err(format!("invalid en passant square: {ep}"));
            }
        }

        let them = !self.side;
        if !(self.attackers_to(self.king(them), self.occupied()) & self.color_bb(self.side))
            .is_empty()
        {
            return Err("side not to move is in check".into());
        }
        Ok(())
    }

    pub fn fen(&self) -> String {
        let mut fen = String::new();
        for rank in (0..8).rev() {
            let mut empty = 0;
            for file in 0..8 {
                match self.piece_on(Square::new(file, rank)) {
                    Some(piece) => {
                        if empty > 0 {
                            write!(fen, "{empty}").unwrap();
                            empty = 0;
                        }
                        fen.push(piece.char());
                    }
                    None => empty += 1,
                }
            }
            if empty > 0 {
                write!(fen, "{empty}").unwrap();
            }
            if rank > 0 {
                fen.push('/');
            }
        }

        fen.push_str(if self.side == Color::White {
            " w "
        } else {
            " b "
        });
        if self.castling == 0 {
            fen.push('-');
        }
        for (color, kingside) in [
            (Color::White, true),
            (Color::White, false),
            (Color::Black, true),
            (Color::Black, false),
        ] {
            let right = castling_right(color, kingside);
            if self.castling & right == 0 {
                continue;
            }
            let rook = self.castling_rooks[right.trailing_zeros() as usize];
            let rooks = self.pieces(color, PieceKind::Rook) & Bitboard::rank(rook.rank());
            let outermost = !rooks.into_iter().any(|r| match kingside {
                true => r.file() > rook.file(),
                false => r.file() < rook.file(),
            });
            let c = match (outermost, kingside) {
                (true, true) => 'K',
                (true, false) => 'Q',
                (false, _) => (b'A' + rook.file()) as char,
            };
            fen.push(match color {
                Color::White => c,
                Color::Black => c.to_ascii_lowercase(),
            });
        }
        match self.en_passant {
            Some(ep) => write!(fen, " {ep}").unwrap(),
            None => fen.push_str(" -"),
        }
        write!(fen, " {} {}", self.halfmove, self.fullmove).unwrap();
        fen
    }

    fn compute_hash(&self) -> u64 {
        let mut hash = zobrist::castling(self.castling);
        for sq in self.occupied() {
            hash ^= zobrist::piece(self.mailbox[sq.index()].unwrap(), sq);
        }
        if let Some(ep) = self.en_passant {
            hash ^= zobrist::en_passant(ep);
        }
        if self.side == Color::Black {
            hash ^= zobrist::side();
        }
        hash
    }

    fn put(&mut self, piece: Piece, sq: Square) {
        debug_assert!(self.mailbox[sq.index()].is_none(), "{sq} is occupied");
        self.kinds[piece.kind.index()] ^= sq.bb();
        self.colors[piece.color.index()] ^= sq.bb();
        self.mailbox[sq.index()] = Some(piece);
        self.hash ^= zobrist::piece(piece, sq);
    }

    fn remove(&mut self, sq: Square) -> Piece {
        let piece = self.mailbox[sq.index()].take().expect("square is empty");
        self.kinds[piece.kind.index()] ^= sq.bb();
        self.colors[piece.color.index()] ^= sq.bb();
        self.hash ^= zobrist::piece(piece, sq);
        piece
    }
}
