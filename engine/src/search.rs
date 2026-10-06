use std::cmp::Reverse;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use dumbfish_core::{Board, Move, MoveKind, MoveList, PieceKind};

use crate::eval;
use crate::skill::{MAX_LEVEL, Skill};
use crate::style::Style;
use crate::tt::{Bound, Entry, TranspositionTable, score_from_tt, score_to_tt};

pub const MAX_DEPTH: u32 = 64;
const INFINITY: i32 = 32_000;
/// Being mated `ply` plies from the root scores `ply - MATE`.
pub const MATE: i32 = 31_000;

#[derive(Default)]
pub struct Limits {
    pub depth: Option<u32>,
    pub nodes: Option<u64>,
    pub deadline: Option<Instant>,
    /// Below `MAX_LEVEL`, adds the skill's node budget and picks the move with its noise.
    pub skill: Option<Skill>,
    pub style: Style,
}

/// A finished iteration of iterative deepening.
pub struct Iteration {
    pub depth: u32,
    pub score: i32,
    pub best: Move,
    pub nodes: u64,
    pub elapsed: Duration,
}

/// Iterative deepening until a limit is hit or `stop` is set. Returns the best move of the
/// last finished iteration (any legal move if none finished; `None` if there are no moves)
/// and the number of nodes searched.
pub fn search(
    board: &mut Board,
    limits: &Limits,
    stop: &AtomicBool,
    tt: &mut TranspositionTable,
    mut on_iteration: impl FnMut(&Iteration),
) -> (Option<Move>, u64) {
    let start = Instant::now();
    let skill = limits.skill.filter(|s| s.level < MAX_LEVEL);
    let mut moves = board.legal_moves();
    order_moves(board, &mut moves);
    let mut best = moves.first().copied();
    let mut searcher = Searcher {
        board,
        limits,
        node_limit: [limits.nodes, skill.and_then(Skill::nodes)]
            .into_iter()
            .flatten()
            .min(),
        stop,
        tt,
        nodes: 0,
        aborted: false,
    };
    let mut scores = Vec::new();
    for depth in 1..=limits.depth.unwrap_or(MAX_DEPTH).min(MAX_DEPTH) {
        let found = match skill {
            Some(_) => searcher.root_scores(depth, &moves),
            None => searcher.root(depth, &moves).map(|best| vec![best]),
        };
        let Some(found) = found else {
            break;
        };
        let (mv, score) = found
            .iter()
            .copied()
            .reduce(|a, b| if b.1 > a.1 { b } else { a })
            .expect("a finished iteration has moves");
        best = Some(mv);
        scores = found;
        on_iteration(&Iteration {
            depth,
            score,
            best: mv,
            nodes: searcher.nodes,
            elapsed: start.elapsed(),
        });
    }
    if let Some(skill) = skill {
        best = skill.choose(&scores).or(best);
    }
    (best, searcher.nodes)
}

/// Captures and promotions first, by most valuable victim, then least valuable attacker;
/// quiet moves keep their generation order.
fn order_moves(board: &Board, moves: &mut [Move]) {
    moves.sort_by_key(|&mv| Reverse(move_order_key(board, mv)));
}

fn move_order_key(board: &Board, mv: Move) -> i32 {
    const VALUE: [i32; 6] = [1, 3, 3, 5, 9, 0];
    let value = |sq| board.piece_on(sq).map_or(0, |p| VALUE[p.kind.index()]);
    let gain = match mv.kind() {
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

struct Searcher<'a> {
    board: &'a mut Board,
    limits: &'a Limits,
    node_limit: Option<u64>,
    stop: &'a AtomicBool,
    tt: &'a mut TranspositionTable,
    nodes: u64,
    aborted: bool,
}

impl Searcher<'_> {
    /// Best root move and its score; `None` if aborted or there are no moves.
    fn root(&mut self, depth: u32, moves: &MoveList) -> Option<(Move, i32)> {
        self.nodes += 1;
        let mut best = None;
        let mut alpha = -INFINITY;
        for &mv in moves {
            self.board.make_move(mv);
            let score = -self.negamax(depth - 1, 1, -INFINITY, -alpha);
            self.board.unmake_move();
            if self.aborted {
                return None;
            }
            if score > alpha {
                alpha = score;
                best = Some((mv, score));
            }
        }
        best
    }

    /// Exact score of every root move, each searched with a full window; `None` if aborted or
    /// there are no moves.
    fn root_scores(&mut self, depth: u32, moves: &MoveList) -> Option<Vec<(Move, i32)>> {
        self.nodes += 1;
        let mut scores = Vec::with_capacity(moves.len());
        for &mv in moves {
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

    fn negamax(&mut self, depth: u32, ply: i32, mut alpha: i32, beta: i32) -> i32 {
        self.nodes += 1;
        if self.should_abort() {
            return 0;
        }
        if self.board.is_repetition() {
            return self.draw_score(ply);
        }
        let key = self.board.hash();
        let entry = self.tt.probe(key);
        if let Some(e) = entry
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

        if depth >= 3 && !self.board.in_check() && self.has_pieces() && self.evaluate() >= beta {
            self.board.make_null_move();
            let score = -self.negamax(depth - 3, ply + 1, -beta, -beta + 1);
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
            return if self.board.in_check() {
                ply - MATE
            } else {
                self.draw_score(ply)
            };
        }
        if self.board.halfmove_clock() >= 100 {
            return self.draw_score(ply);
        }
        if depth == 0 {
            return self.quiescence(alpha, beta);
        }
        order_moves(self.board, &mut moves);
        if let Some(i) = moves
            .iter()
            .position(|&mv| Some(mv) == entry.and_then(|e| e.best))
        {
            moves[..=i].rotate_right(1);
        }

        let original_alpha = alpha;
        let (mut best, mut best_move) = (-INFINITY, None);
        for &mv in &moves {
            self.board.make_move(mv);
            let score = -self.negamax(depth - 1, ply + 1, -beta, -alpha);
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
            depth: depth as u8,
            bound,
        });
        best
    }

    /// Searches captures only, until the position is quiet; the side to move may also stand pat
    /// on the static evaluation.
    fn quiescence(&mut self, mut alpha: i32, beta: i32) -> i32 {
        self.nodes += 1;
        if self.should_abort() {
            return 0;
        }
        let stand_pat = self.evaluate();
        if stand_pat >= beta {
            return stand_pat;
        }
        alpha = alpha.max(stand_pat);

        let mut moves = self.board.legal_moves();
        order_moves(self.board, &mut moves);
        let captures = moves
            .iter()
            .take_while(|&&mv| move_order_key(self.board, mv) > 0)
            .count();
        let mut best = stand_pat;
        for &mv in &moves[..captures] {
            self.board.make_move(mv);
            let score = -self.quiescence(-beta, -alpha);
            self.board.unmake_move();
            if self.aborted {
                return 0;
            }
            best = best.max(score);
            alpha = alpha.max(score);
            if alpha >= beta {
                break;
            }
        }
        best
    }

    /// The side to move has a piece other than pawns and the king, so a null move is unlikely
    /// to miss a zugzwang.
    fn has_pieces(&self) -> bool {
        let us = self.board.side_to_move();
        let pawns_and_king =
            self.board.kind_bb(PieceKind::Pawn) | self.board.kind_bb(PieceKind::King);
        !(self.board.color_bb(us) & !pawns_and_king).is_empty()
    }

    fn evaluate(&self) -> i32 {
        eval::evaluate_styled(self.board, &self.limits.style)
    }

    /// A draw costs the side to move at the root `contempt`, and gains its opponent as much.
    fn draw_score(&self, ply: i32) -> i32 {
        if ply % 2 == 0 {
            -self.limits.style.contempt
        } else {
            self.limits.style.contempt
        }
    }

    /// Polls the clock and the stop flag every 1024 nodes; the node limit is exact.
    fn should_abort(&mut self) -> bool {
        self.aborted = self.aborted
            || self.node_limit.is_some_and(|n| self.nodes > n)
            || (self.nodes.is_multiple_of(1024)
                && (self.stop.load(Ordering::Relaxed)
                    || self.limits.deadline.is_some_and(|d| Instant::now() >= d)));
        self.aborted
    }
}
