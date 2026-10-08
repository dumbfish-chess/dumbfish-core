//! Principal variation search with its pruning, reductions and extensions, and quiescence search.

use super::*;

/// A capture in quiescence search is skipped if even winning the captured piece and this much
/// more cannot raise the score to alpha.
const DELTA_MARGIN: i32 = 200;

/// Depth reduction of the `number`-th move (from 1) of a node searched to `depth`.
fn reduction(depth: u32, number: usize) -> u32 {
    (0.75 + f64::from(depth).ln() * (number as f64).ln() / 2.25) as u32
}

impl Searcher<'_> {
    pub(super) fn negamax(&mut self, mut depth: u32, ply: i32, mut alpha: i32, beta: i32) -> i32 {
        self.pv_len[ply as usize] = 0;
        self.nodes += 1;
        self.seldepth = self.seldepth.max(ply as u32);
        if self.should_abort() {
            return 0;
        }
        if self.board.is_repetition() {
            return self.draw_score(ply);
        }
        let key = self.board.hash();
        let entry = self.tt.probe(key);
        let excluded = self.excluded[ply as usize];
        if let Some(e) = entry
            && excluded.is_none()
            && u32::from(e.depth) >= depth
        {
            let score = score_from_tt(e.score, ply);
            match e.bound {
                Bound::Exact => return score,
                Bound::Lower if score >= beta => return score,
                Bound::Upper if score <= alpha => return score,
                _ => {}
            }
        }

        if let Some(tb) = self.tablebases
            && excluded.is_none()
            && self.board.halfmove_clock() == 0
            && let Some(wdl) = tb.probe_wdl(self.board)
        {
            self.counters.tb_hits.fetch_add(1, Ordering::Relaxed);
            let (score, bound) = match wdl {
                2 => (TB_WIN - ply, Bound::Lower),
                -2 => (ply - TB_WIN, Bound::Upper),
                _ => (self.draw_score(ply), Bound::Exact),
            };
            let cutoff = match bound {
                Bound::Lower => score >= beta,
                Bound::Upper => score <= alpha,
                _ => true,
            };
            if cutoff {
                self.tt.store(Entry {
                    key,
                    best: None,
                    score: score_to_tt(score, ply),
                    depth: (depth + 6).min(MAX_DEPTH) as u8,
                    bound,
                });
                return score;
            }
        }

        let in_check = self.board.in_check();
        if in_check && ply as u32 + depth < MAX_DEPTH {
            depth += 1;
        }
        if depth >= 4 && entry.and_then(|e| e.best).is_none() {
            depth -= 1;
        }

        let pv = beta - alpha > 1;
        let static_eval = (!in_check && depth > 0).then(|| self.evaluate());
        self.static_evals[ply as usize] = static_eval;
        let improving = ply >= 2
            && static_eval.is_some_and(|eval| {
                self.static_evals[ply as usize - 2].is_some_and(|before| eval > before)
            });
        if let Some(eval) = static_eval
            && !pv
            && excluded.is_none()
            && depth <= 6
            && beta.abs() < MATE - MAX_DEPTH as i32
            && eval - 80 * (depth - u32::from(improving)) as i32 >= beta
        {
            return eval;
        }

        if let Some(eval) = static_eval
            && !pv
            && depth >= 3
            && eval >= beta
            && excluded.is_none()
            && self.played[ply as usize - 1].is_some()
            && self.has_pieces()
        {
            let reduction = 4 + depth / 6 + ((eval - beta) / 200).min(3) as u32;
            self.played[ply as usize] = None;
            self.board.make_null_move();
            let score = -self.negamax(depth.saturating_sub(reduction), ply + 1, -beta, -beta + 1);
            self.board.unmake_null_move();
            if self.aborted {
                return 0;
            }
            if score >= beta {
                return if score >= MATE - MAX_DEPTH as i32 {
                    beta
                } else {
                    score
                };
            }
        }

        let mut moves = self.board.legal_moves();
        if moves.is_empty() {
            return if in_check {
                ply - MATE
            } else {
                self.draw_score(ply)
            };
        }
        if self.board.halfmove_clock() >= 100 {
            return self.draw_score(ply);
        }
        if depth == 0 {
            return self.quiescence(ply, alpha, beta);
        }
        let tt_move = entry.and_then(|e| e.best);
        self.order(&mut moves, tt_move, ply);

        let original_alpha = alpha;
        let (mut best, mut best_move) = (-INFINITY, None);
        let mut quiets_tried = MoveList::default();
        for (i, &mv) in moves.iter().enumerate() {
            if Some(mv) == excluded {
                continue;
            }
            let extension = match entry {
                Some(e) if i == 0 && Some(mv) == tt_move && excluded.is_none() => {
                    self.singular_extension(e, depth, ply)
                }
                _ => 0,
            };
            if self.aborted {
                return 0;
            }
            let quiet = move_order_key(self.board, mv) == 0;
            let history = if quiet {
                self.quiet_history(mv, ply)
            } else {
                0
            };
            self.played[ply as usize] = Some(mv);
            self.board.make_move(mv);
            let gives_check = self.board.in_check();
            let futile = !pv
                && depth <= 3
                && static_eval.is_some_and(|eval| eval + 100 + 100 * depth as i32 <= alpha);
            let late_quiet = !pv
                && !in_check
                && depth <= 3
                && quiets_tried.len() as u32 >= (3 + depth * depth) / (2 - u32::from(improving));
            if (futile || late_quiet) && i > 0 && quiet && !gives_check {
                self.board.unmake_move();
                continue;
            }
            let late = i >= 3 && depth >= 3 && quiet && !in_check && !gives_check;
            let score = if late {
                let reduction =
                    reduction(depth, i + 1) as i32 + i32::from(!improving) + i32::from(!pv)
                        - history / MAX_HISTORY;
                let reduced = (depth - 1).saturating_sub(reduction.max(0) as u32);
                let score = -self.negamax(reduced, ply + 1, -alpha - 1, -alpha);
                if score > alpha && !self.aborted {
                    self.principal_variation(false, depth - 1, ply + 1, alpha, beta)
                } else {
                    score
                }
            } else {
                self.principal_variation(i == 0, depth - 1 + extension, ply + 1, alpha, beta)
            };
            self.board.unmake_move();
            if self.aborted {
                return 0;
            }
            if score > best {
                (best, best_move) = (score, Some(mv));
            }
            if score > alpha {
                alpha = score;
                self.update_pv(ply, mv);
            }
            if alpha >= beta {
                if quiet {
                    self.remember_killer(mv, ply);
                    if let Some(slot) = self.countermove_slot(ply) {
                        self.countermoves[slot.0][slot.1] = Some(mv);
                    }
                    self.update_history(mv, &quiets_tried, depth, ply);
                }
                break;
            }
            if quiet {
                quiets_tried.push(mv);
            }
        }

        if excluded.is_none() {
            let bound = match best {
                s if s <= original_alpha => Bound::Upper,
                s if s >= beta => Bound::Lower,
                _ => Bound::Exact,
            };
            self.tt.store(Entry {
                key,
                best: best_move,
                score: score_to_tt(best, ply),
                depth: depth as u8,
                bound,
            });
        }
        best
    }

    /// One more ply for the move from the transposition table `entry` if a reduced search of
    /// every other move stays clearly below its score.
    fn singular_extension(&mut self, entry: Entry, depth: u32, ply: i32) -> u32 {
        let tt_score = score_from_tt(entry.score, ply);
        if depth < 6
            || entry.bound == Bound::Upper
            || u32::from(entry.depth) + 3 < depth
            || tt_score.abs() >= MATE - MAX_DEPTH as i32
            || ply as u32 + depth >= MAX_DEPTH
        {
            return 0;
        }
        let singular_beta = tt_score - 2 * depth as i32;
        self.excluded[ply as usize] = entry.best;
        let score = self.negamax((depth - 1) / 2, ply, singular_beta - 1, singular_beta);
        self.excluded[ply as usize] = None;
        self.pv_len[ply as usize] = 0;
        u32::from(score < singular_beta)
    }

    /// Score of the move just made, for the side that made it. The first move gets the full
    /// window; later ones are only tested against a null window at `alpha` and searched again
    /// with the full window if they beat it.
    pub(super) fn principal_variation(
        &mut self,
        first: bool,
        depth: u32,
        ply: i32,
        alpha: i32,
        beta: i32,
    ) -> i32 {
        if first {
            return -self.negamax(depth, ply, -beta, -alpha);
        }
        let score = -self.negamax(depth, ply, -alpha - 1, -alpha);
        if score > alpha && score < beta && !self.aborted {
            -self.negamax(depth, ply, -beta, -alpha)
        } else {
            score
        }
    }

    /// Searches captures and promotions that do not lose material by `see`, until the position
    /// is quiet; the side to move may also stand pat on the static evaluation.
    pub(super) fn quiescence(&mut self, ply: i32, mut alpha: i32, beta: i32) -> i32 {
        self.nodes += 1;
        self.seldepth = self.seldepth.max(ply as u32);
        if self.should_abort() {
            return 0;
        }
        let key = self.board.hash();
        if let Some(e) = self.tt.probe(key) {
            let score = score_from_tt(e.score, ply);
            match e.bound {
                Bound::Exact => return score,
                Bound::Lower if score >= beta => return score,
                Bound::Upper if score <= alpha => return score,
                _ => {}
            }
        }
        let original_alpha = alpha;
        let stand_pat = self.evaluate();
        if stand_pat >= beta {
            return stand_pat;
        }
        alpha = alpha.max(stand_pat);

        let mut moves = self.board.legal_captures();
        order_moves(self.board, &mut moves);
        let (mut best, mut best_move) = (stand_pat, None);
        for &mv in moves.iter() {
            let captured = match mv.kind() {
                MoveKind::EnPassant => Some(PieceKind::Pawn),
                _ => self.board.piece_on(mv.to()).map(|p| p.kind),
            };
            let promotion = matches!(mv.kind(), MoveKind::Promotion(_));
            if !promotion && stand_pat + captured.map_or(0, piece_value) + DELTA_MARGIN <= alpha {
                continue;
            }
            if see(self.board, mv) < 0 {
                continue;
            }
            self.board.make_move(mv);
            let score = -self.quiescence(ply + 1, -beta, -alpha);
            self.board.unmake_move();
            if self.aborted {
                return 0;
            }
            if score > best {
                (best, best_move) = (score, Some(mv));
            }
            alpha = alpha.max(score);
            if alpha >= beta {
                break;
            }
        }
        let bound = match best {
            s if s <= original_alpha => Bound::Upper,
            s if s >= beta => Bound::Lower,
            _ => Bound::Exact,
        };
        self.tt.store(Entry {
            key,
            best: best_move,
            score: score_to_tt(best, ply),
            depth: 0,
            bound,
        });
        best
    }
}
