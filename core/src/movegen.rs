use std::ops::{Deref, DerefMut};

use crate::board::castling_right;
use crate::{Bitboard, Board, Color, Move, MoveKind, PieceKind, Square, attacks};

/// Fixed-capacity move list of up to 256 moves; no legal position has more than 218.
pub struct MoveList {
    moves: [Move; 256],
    len: usize,
}

impl Default for MoveList {
    fn default() -> Self {
        Self {
            moves: [Move::new(Square::A1, Square::A1, MoveKind::Normal); 256],
            len: 0,
        }
    }
}

impl MoveList {
    /// Panics if the list already holds 256 moves.
    pub fn push(&mut self, mv: Move) {
        self.moves[self.len] = mv;
        self.len += 1;
    }

    fn push_all(&mut self, from: Square, targets: Bitboard) {
        for to in targets {
            self.push(Move::new(from, to, MoveKind::Normal));
        }
    }
}

impl FromIterator<Move> for MoveList {
    /// Panics beyond 256 moves.
    fn from_iter<I: IntoIterator<Item = Move>>(moves: I) -> Self {
        let mut list = Self::default();
        for mv in moves {
            list.push(mv);
        }
        list
    }
}

impl Deref for MoveList {
    type Target = [Move];

    fn deref(&self) -> &[Move] {
        &self.moves[..self.len]
    }
}

impl DerefMut for MoveList {
    fn deref_mut(&mut self) -> &mut [Move] {
        &mut self.moves[..self.len]
    }
}

impl<'a> IntoIterator for &'a MoveList {
    type Item = &'a Move;
    type IntoIter = std::slice::Iter<'a, Move>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl Board {
    /// Enemy pieces giving check to the side to move.
    pub fn checkers(&self) -> Bitboard {
        let us = self.side_to_move();
        self.attackers_to(self.king(us), self.occupied()) & self.color_bb(!us)
    }

    pub fn in_check(&self) -> bool {
        !self.checkers().is_empty()
    }

    pub fn legal_moves(&self) -> MoveList {
        self.generate(false)
    }

    /// The legal captures, en passant captures and promotions, quiet promotions included, in
    /// the order `legal_moves` gives them.
    pub fn legal_captures(&self) -> MoveList {
        self.generate(true)
    }

    fn generate(&self, captures_only: bool) -> MoveList {
        use PieceKind::*;
        let mut list = MoveList::default();
        let us = self.side_to_move();
        let king = self.king(us);
        let occupied = self.occupied();
        let ours = self.color_bb(us);
        let theirs = self.color_bb(!us);
        let targets = if captures_only { theirs } else { !ours };
        let attacked =
            |sq: Square, occupied: Bitboard| !(self.attackers_to(sq, occupied) & theirs).is_empty();

        for to in attacks::king(king) & targets {
            if !attacked(to, occupied ^ king.bb()) {
                list.push(Move::new(king, to, MoveKind::Normal));
            }
        }
        let checkers = self.checkers();
        if checkers.count() > 1 {
            return list;
        }

        let evasions = match checkers.is_empty() {
            true => Bitboard::ALL,
            false => attacks::between(king, checkers.lsb()) | checkers,
        };
        let pinned = self.pinned(us);
        let allowed = |from: Square| match pinned.contains(from) {
            true => evasions & attacks::line(king, from),
            false => evasions,
        };

        for from in self.pieces(us, Knight) & !pinned {
            list.push_all(from, attacks::knight(from) & targets & evasions);
        }
        for from in self.pieces(us, Bishop) | self.pieces(us, Queen) {
            list.push_all(
                from,
                attacks::bishop(from, occupied) & targets & allowed(from),
            );
        }
        for from in self.pieces(us, Rook) | self.pieces(us, Queen) {
            list.push_all(
                from,
                attacks::rook(from, occupied) & targets & allowed(from),
            );
        }

        let (start_rank, last_rank) = match us {
            Color::White => (1, 7),
            Color::Black => (6, 0),
        };
        let forward = |sq: Square| match us {
            Color::White => Square::from_index(sq.index() + 8),
            Color::Black => Square::from_index(sq.index() - 8),
        };
        for from in self.pieces(us, Pawn) {
            let allowed = allowed(from);
            let one = forward(from);
            let mut targets = attacks::pawn(us, from) & theirs;
            if !occupied.contains(one) {
                if !captures_only || one.rank() == last_rank {
                    targets |= one.bb();
                }
                if !captures_only && from.rank() == start_rank {
                    let two = forward(one);
                    if !occupied.contains(two) && allowed.contains(two) {
                        list.push(Move::new(from, two, MoveKind::DoublePush));
                    }
                }
            }
            for to in targets & allowed {
                if to.rank() == last_rank {
                    for kind in [Queen, Rook, Bishop, Knight] {
                        list.push(Move::new(from, to, MoveKind::Promotion(kind)));
                    }
                } else {
                    list.push(Move::new(from, to, MoveKind::Normal));
                }
            }

            if let Some(ep) = self.en_passant()
                && attacks::pawn(us, from).contains(ep)
            {
                let captured = Square::new(ep.file(), from.rank());
                let after = occupied ^ from.bb() ^ captured.bb() ^ ep.bb();
                if (self.attackers_to(king, after) & theirs & !captured.bb()).is_empty() {
                    list.push(Move::new(from, ep, MoveKind::EnPassant));
                }
            }
        }

        if checkers.is_empty() && !captures_only {
            for kingside in [true, false] {
                if self.castling() & castling_right(us, kingside) == 0 {
                    continue;
                }
                let king_to = Square::new(if kingside { 6 } else { 2 }, king.rank());
                let mv = Move::new(king, king_to, MoveKind::Castle);
                let (rook, rook_to) = self.castling_rook(mv);
                let king_path = attacks::between(king, king_to) | king_to.bb();
                let path = king_path | attacks::between(rook, rook_to) | rook_to.bb();
                let without_rook = occupied ^ rook.bb();
                if (path & without_rook & !king.bb()).is_empty()
                    && !king_path.into_iter().any(|sq| attacked(sq, without_rook))
                {
                    list.push(mv);
                }
            }
        }
        list
    }

    /// Pieces of `color` that cannot leave the line between their king and an enemy slider.
    fn pinned(&self, color: Color) -> Bitboard {
        use PieceKind::*;
        let king = self.king(color);
        let theirs = self.color_bb(!color);
        let diagonal = self.pieces(!color, Bishop) | self.pieces(!color, Queen);
        let straight = self.pieces(!color, Rook) | self.pieces(!color, Queen);
        let snipers =
            (attacks::bishop(king, theirs) & diagonal) | (attacks::rook(king, theirs) & straight);

        let mut pinned = Bitboard::EMPTY;
        for sniper in snipers {
            let blockers = attacks::between(king, sniper) & self.occupied();
            if blockers.count() == 1 {
                pinned |= blockers & self.color_bb(color);
            }
        }
        pinned
    }
}
