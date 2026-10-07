//! The root of the search: aspiration windows and the scores of all root moves.

use super::*;

impl Searcher<'_> {
    /// Root search with an aspiration window around the previous iteration's score, widened
    /// on the failing side until the score falls inside it.
    pub(super) fn aspiration(
        &mut self,
        depth: u32,
        moves: &MoveList,
        previous: Option<i32>,
    ) -> Option<(Move, i32)> {
        let mut delta = 25;
        let (mut alpha, mut beta) = match previous {
            Some(score) if depth >= 5 && score.abs() < MATE - MAX_DEPTH as i32 => {
                (score - delta, score + delta)
            }
            _ => (-INFINITY, INFINITY),
        };
        loop {
            let (mv, score) = self.root(depth, moves, alpha, beta)?;
            if score <= alpha && alpha > -INFINITY {
                alpha = (alpha - delta).max(-INFINITY);
            } else if score >= beta && beta < INFINITY {
                beta = (beta + delta).min(INFINITY);
            } else {
                return Some((mv, score));
            }
            delta *= 2;
        }
    }

    /// Best root move and its fail-soft score within `alpha..beta`; `None` if aborted or there
    /// are no moves.
    pub(super) fn root(
        &mut self,
        depth: u32,
        moves: &MoveList,
        mut alpha: i32,
        beta: i32,
    ) -> Option<(Move, i32)> {
        self.nodes += 1;
        let mut best: Option<(Move, i32)> = None;
        for (i, &mv) in moves.iter().enumerate() {
            self.played[0] = Some(mv);
            self.board.make_move(mv);
            let score = self.principal_variation(i == 0, depth - 1, 1, alpha, beta);
            self.board.unmake_move();
            if self.aborted {
                return None;
            }
            if best.is_none_or(|(_, b)| score > b) {
                best = Some((mv, score));
            }
            if score > alpha {
                alpha = score;
                self.update_pv(0, mv);
                if score >= beta {
                    break;
                }
            }
        }
        best
    }

    /// Exact score of every root move, each searched with a full window; `None` if aborted or
    /// there are no moves.
    pub(super) fn root_scores(&mut self, depth: u32, moves: &MoveList) -> Option<Vec<(Move, i32)>> {
        self.nodes += 1;
        let mut scores = Vec::with_capacity(moves.len());
        for &mv in moves {
            self.played[0] = Some(mv);
            self.board.make_move(mv);
            let score = -self.negamax(depth - 1, 1, -INFINITY, INFINITY);
            self.board.unmake_move();
            if self.aborted {
                return None;
            }
            scores.push((mv, score));
        }
        (!scores.is_empty()).then_some(scores)
    }
}
