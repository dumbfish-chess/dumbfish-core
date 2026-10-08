//! Move ordering: hash move, captures by MVV-LVA and SEE, killers, countermoves, history.

use super::*;

pub(super) const MAX_HISTORY: i32 = 16_384;

/// Index into `continuation` of the quiet move `mv` after the move given by
/// `Searcher::countermove_slot`: previous piece and square, then this piece and square.
pub(super) fn continuation_index(board: &Board, (piece, to): (usize, usize), mv: Move) -> usize {
    let moving = board.piece_on(mv.from()).map_or(0, |p| p.index());
    ((piece * 64 + to) * 12 + moving) * 64 + mv.to().index()
}

/// Index into `capture_history` of `mv`: the moving piece, the target square and the
/// captured kind; `None` if `mv` captures nothing.
pub(super) fn capture_index(board: &Board, mv: Move) -> Option<usize> {
    let piece = board.piece_on(mv.from())?;
    let captured = match mv.kind() {
        MoveKind::EnPassant => PieceKind::Pawn,
        MoveKind::Castle => return None,
        _ => board.piece_on(mv.to())?.kind,
    };
    Some((piece.index() * 64 + mv.to().index()) * 6 + captured.index())
}

/// Captures and promotions first, by most valuable victim, then least valuable attacker;
/// quiet moves keep their generation order.
pub(super) fn order_moves(board: &Board, moves: &mut [Move]) {
    moves.sort_by_key(|&mv| Reverse(move_order_key(board, mv)));
}

pub(super) fn move_order_key(board: &Board, mv: Move) -> i32 {
    const VALUE: [i32; 6] = [1, 3, 3, 5, 9, 0];
    let value = |sq| board.piece_on(sq).map_or(0, |p| VALUE[p.kind.index()]);
    let gain = match mv.kind() {
        MoveKind::Castle => 0,
        MoveKind::EnPassant => VALUE[0],
        MoveKind::Promotion(kind) => value(mv.to()) + VALUE[kind.index()] - VALUE[0],
        _ => value(mv.to()),
    };
    if gain > 0 {
        10 * gain - value(mv.from())
    } else {
        0
    }
}

impl Searcher<'_> {
    /// The move from the transposition table first, then captures and promotions that do not
    /// lose material by `move_order_key` and capture history, then the killer moves of this ply
    /// and the countermove to the last move, then the other quiet moves by history and
    /// continuation history, and the captures that lose material last.
    pub(super) fn order(&self, moves: &mut [Move], tt_move: Option<Move>, ply: i32) {
        let killers = self.killers[ply as usize];
        let previous = self.countermove_slot(ply);
        let countermove = previous.and_then(|(piece, to)| self.countermoves[piece][to]);
        moves.sort_by_key(|&mv| {
            Reverse(match move_order_key(self.board, mv) {
                _ if Some(mv) == tt_move => i32::MAX,
                tactical if tactical > 0 => {
                    let history =
                        capture_index(self.board, mv).map_or(0, |i| self.capture_history[i]);
                    let key = 1024 * tactical + history;
                    if see(self.board, mv) >= 0 {
                        1_000_000 + key
                    } else {
                        -1_000_000 + key
                    }
                }
                _ if Some(mv) == killers[0] => 4 * MAX_HISTORY + 2,
                _ if Some(mv) == killers[1] => 4 * MAX_HISTORY + 1,
                _ if Some(mv) == countermove => 4 * MAX_HISTORY,
                _ => self.quiet_history(mv, ply),
            })
        });
    }

    /// History plus continuation history of the quiet move `mv` at `ply`, within
    /// `-2 * MAX_HISTORY..=2 * MAX_HISTORY`.
    pub(super) fn quiet_history(&self, mv: Move, ply: i32) -> i32 {
        let history = &self.history[self.board.side_to_move().index()];
        history[mv.from().index()][mv.to().index()]
            + self.countermove_slot(ply).map_or(0, |p| {
                self.continuation[continuation_index(self.board, p, mv)]
            })
    }

    /// Rewards the quiet move that caused a beta cutoff and penalizes the quiet moves tried
    /// before it, by `depth²`, in the history and in the continuation history of the move that
    /// led to the node at `ply`; values decay towards zero as they approach `MAX_HISTORY`.
    pub(super) fn update_history(&mut self, cutoff: Move, tried: &[Move], depth: u32, ply: i32) {
        let bonus = (depth * depth).min(MAX_HISTORY as u32) as i32;
        let previous = self.countermove_slot(ply);
        let history = &mut self.history[self.board.side_to_move().index()];
        let continuation = &mut self.continuation;
        let board = &*self.board;
        let mut update = |mv: Move, bonus: i32| {
            let h = &mut history[mv.from().index()][mv.to().index()];
            *h += bonus - *h * bonus.abs() / MAX_HISTORY;
            if let Some(p) = previous {
                let c = &mut continuation[continuation_index(board, p, mv)];
                *c += bonus - *c * bonus.abs() / MAX_HISTORY;
            }
        };
        update(cutoff, bonus);
        for &mv in tried {
            update(mv, -bonus);
        }
    }

    /// Rewards `cutoff`, if it is a capture, and penalizes the captures tried before it, by
    /// `depth²` with the same decay as `update_history`.
    pub(super) fn update_capture_history(
        &mut self,
        cutoff: Option<Move>,
        tried: &[Move],
        depth: u32,
    ) {
        let bonus = (depth * depth).min(MAX_HISTORY as u32) as i32;
        let board = &*self.board;
        let table = &mut self.capture_history;
        let mut update = |mv: Move, bonus: i32| {
            if let Some(i) = capture_index(board, mv) {
                let h = &mut table[i];
                *h += bonus - *h * bonus.abs() / MAX_HISTORY;
            }
        };
        if let Some(mv) = cutoff {
            update(mv, bonus);
        }
        for &mv in tried {
            update(mv, -bonus);
        }
    }

    /// A quiet move that caused a beta cutoff, to be tried early in sibling nodes.
    pub(super) fn remember_killer(&mut self, mv: Move, ply: i32) {
        let killers = &mut self.killers[ply as usize];
        if killers[0] != Some(mv) {
            killers[1] = killers[0];
            killers[0] = Some(mv);
        }
    }

    /// Index into `countermoves` for the move that led to the node at `ply`: the piece that made
    /// it and its target square; `None` at the root and after a null move.
    pub(super) fn countermove_slot(&self, ply: i32) -> Option<(usize, usize)> {
        let previous = self.played[(ply as usize).checked_sub(1)?]?;
        let piece = self.board.piece_on(previous.to())?;
        Some((piece.index(), previous.to().index()))
    }
}
