use std::cmp::Reverse;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use dumbfish_core::{Board, Move, MoveKind, MoveList, PieceKind, Square};

use crate::eval;
use crate::skill::{MAX_LEVEL, Skill};
use crate::style::Style;
use crate::tt::{Bound, Entry, TranspositionTable, score_from_tt, score_to_tt};

pub const MAX_DEPTH: u32 = 64;
const MAX_HISTORY: i32 = 16_384;
/// Plies of the main search; quiescence search does not count them.
const MAX_PLY: usize = MAX_DEPTH as usize + 1;
const INFINITY: i32 = 32_000;
/// Being mated `ply` plies from the root scores `ply - MATE`.
pub const MATE: i32 = 31_000;

#[derive(Default)]
pub struct Limits {
    pub depth: Option<u32>,
    pub nodes: Option<u64>,
    /// The search stops here.
    pub deadline: Option<Instant>,
    /// No new iteration starts after this.
    pub soft_deadline: Option<Instant>,
    /// Below `MAX_LEVEL`, adds the skill's node budget and picks the move with its noise; the
    /// search then runs on one thread.
    pub skill: Option<Skill>,
    pub style: Style,
    /// Search threads sharing the transposition table (Lazy SMP); 0 counts as 1.
    pub threads: usize,
}

/// A finished iteration of iterative deepening.
pub struct Iteration {
    pub depth: u32,
    pub score: i32,
    pub best: Move,
    /// The principal variation, starting with `best`.
    pub pv: Vec<Move>,
    pub nodes: u64,
    pub elapsed: Duration,
}

/// Iterative deepening until a limit is hit or `stop` is set. Returns the best move of the
/// last finished iteration (any legal move if none finished; `None` if there are no moves)
/// and the number of nodes searched by all threads.
///
/// Helper threads search their own copies of the board to the same limits and share only the
/// transposition table; the calling thread alone reports iterations and picks the move, and the
/// helpers stop when it is done.
pub fn search(
    board: &mut Board,
    limits: &Limits,
    stop: &AtomicBool,
    tt: &TranspositionTable,
    mut on_iteration: impl FnMut(&Iteration),
) -> (Option<Move>, u64) {
    let start = Instant::now();
    let skill = limits.skill.filter(|s| s.level < MAX_LEVEL);
    let threads = if skill.is_some() {
        1
    } else {
        limits.threads.max(1)
    };
    let max_depth = limits.depth.unwrap_or(MAX_DEPTH).min(MAX_DEPTH);
    let finished = AtomicBool::new(false);
    let total_nodes = AtomicU64::new(0);

    thread::scope(|scope| {
        for helper in 1..threads {
            let mut board = board.clone();
            let (finished, total_nodes) = (&finished, &total_nodes);
            scope.spawn(move || {
                let mut moves = board.legal_moves();
                order_moves(&board, &mut moves);
                let mut searcher =
                    Searcher::new(&mut board, limits, None, [stop, finished], tt, total_nodes);
                for depth in (1 + helper % 2) as u32..=max_depth {
                    let Some((best, _)) = searcher.root(depth, &moves, -INFINITY, INFINITY) else {
                        break;
                    };
                    put_first(&mut moves, best);
                }
                searcher.flush_nodes();
            });
        }

        let node_limit = [limits.nodes, skill.and_then(Skill::nodes)]
            .into_iter()
            .flatten()
            .min();
        let searcher = Searcher::new(
            board,
            limits,
            node_limit,
            [stop, &finished],
            tt,
            &total_nodes,
        );
        let best = main_search(searcher, skill, max_depth, start, &mut on_iteration);
        finished.store(true, Ordering::Relaxed);
        best
    })
    .map_or((None, total_nodes.load(Ordering::Relaxed)), |best| {
        (Some(best), total_nodes.load(Ordering::Relaxed))
    })
}

/// The iterative deepening of the calling thread; `None` if there are no moves.
fn main_search(
    mut searcher: Searcher,
    skill: Option<Skill>,
    max_depth: u32,
    start: Instant,
    on_iteration: &mut impl FnMut(&Iteration),
) -> Option<Move> {
    let mut moves = searcher.board.legal_moves();
    order_moves(searcher.board, &mut moves);
    let mut best = moves.first().copied();
    let mut scores: Vec<(Move, i32)> = Vec::new();
    for depth in 1..=max_depth {
        if depth > 1
            && searcher
                .limits
                .soft_deadline
                .is_some_and(|d| Instant::now() >= d)
        {
            break;
        }
        let previous = scores.iter().map(|&(_, score)| score).max();
        let found = match skill {
            Some(_) => searcher.root_scores(depth, &moves),
            None => searcher
                .aspiration(depth, &moves, previous)
                .map(|best| vec![best]),
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
        put_first(&mut moves, mv);
        on_iteration(&Iteration {
            depth,
            score,
            best: mv,
            pv: searcher.root_pv(mv),
            nodes: searcher.total_nodes.load(Ordering::Relaxed) + searcher.nodes - searcher.flushed,
            elapsed: start.elapsed(),
        });
    }
    searcher.flush_nodes();
    if let Some(skill) = skill {
        best = skill.choose(&scores).or(best);
    }
    best
}

/// Moves `mv` to the front, keeping the order of the others.
fn put_first(moves: &mut [Move], mv: Move) {
    if let Some(i) = moves.iter().position(|&m| m == mv) {
        moves[..=i].rotate_right(1);
    }
}

/// Depth reduction of the `number`-th move (from 1) of a node searched to `depth`.
fn reduction(depth: u32, number: usize) -> u32 {
    (0.75 + f64::from(depth).ln() * (number as f64).ln() / 2.25) as u32
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
    /// The caller's stop flag and the flag that ends helper threads.
    stops: [&'a AtomicBool; 2],
    tt: &'a TranspositionTable,
    nodes: u64,
    /// Nodes already added to `total_nodes`.
    flushed: u64,
    total_nodes: &'a AtomicU64,
    aborted: bool,
    /// Two quiet moves per ply that caused beta cutoffs, most recent first.
    killers: [[Option<Move>; 2]; MAX_PLY],
    /// How often a quiet move, by side, from and to square, caused cutoffs; within
    /// `-MAX_HISTORY..=MAX_HISTORY`.
    history: Box<[[[i32; 64]; 64]; 2]>,
    /// Triangular table of principal variations: `pv[ply][..pv_len[ply]]` for each ply.
    pv: Box<[[Move; MAX_PLY + 1]; MAX_PLY + 1]>,
    pv_len: [usize; MAX_PLY + 1],
}

impl<'a> Searcher<'a> {
    fn new(
        board: &'a mut Board,
        limits: &'a Limits,
        node_limit: Option<u64>,
        stops: [&'a AtomicBool; 2],
        tt: &'a TranspositionTable,
        total_nodes: &'a AtomicU64,
    ) -> Self {
        Self {
            board,
            limits,
            node_limit,
            stops,
            tt,
            nodes: 0,
            flushed: 0,
            total_nodes,
            aborted: false,
            killers: [[None; 2]; MAX_PLY],
            history: Box::new([[[0; 64]; 64]; 2]),
            pv: Box::new(
                [[Move::new(Square::A1, Square::A1, MoveKind::Normal); MAX_PLY + 1]; MAX_PLY + 1],
            ),
            pv_len: [0; MAX_PLY + 1],
        }
    }

    fn flush_nodes(&mut self) {
        self.total_nodes
            .fetch_add(self.nodes - self.flushed, Ordering::Relaxed);
        self.flushed = self.nodes;
    }

    /// Root search with an aspiration window around the previous iteration's score, widened
    /// on the failing side until the score falls inside it.
    fn aspiration(
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
    fn root(
        &mut self,
        depth: u32,
        moves: &MoveList,
        mut alpha: i32,
        beta: i32,
    ) -> Option<(Move, i32)> {
        self.nodes += 1;
        let mut best: Option<(Move, i32)> = None;
        for (i, &mv) in moves.iter().enumerate() {
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

    fn negamax(&mut self, mut depth: u32, ply: i32, mut alpha: i32, beta: i32) -> i32 {
        self.pv_len[ply as usize] = 0;
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

        let in_check = self.board.in_check();
        if in_check && ply as u32 + depth < MAX_DEPTH {
            depth += 1;
        }

        let pv = beta - alpha > 1;
        let static_eval = (!in_check && depth > 0).then(|| self.evaluate());
        if let Some(eval) = static_eval
            && !pv
            && depth <= 6
            && beta.abs() < MATE - MAX_DEPTH as i32
            && eval - 80 * depth as i32 >= beta
        {
            return eval;
        }

        if depth >= 3 && self.has_pieces() && static_eval.is_some_and(|eval| eval >= beta) {
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
            return self.quiescence(alpha, beta);
        }
        self.order(&mut moves, entry.and_then(|e| e.best), ply);

        let original_alpha = alpha;
        let (mut best, mut best_move) = (-INFINITY, None);
        let mut quiets_tried = MoveList::default();
        for (i, &mv) in moves.iter().enumerate() {
            let quiet = move_order_key(self.board, mv) == 0;
            self.board.make_move(mv);
            let gives_check = self.board.in_check();
            let futile = !pv
                && depth <= 3
                && static_eval.is_some_and(|eval| eval + 100 + 100 * depth as i32 <= alpha);
            if futile && i > 0 && quiet && !gives_check {
                self.board.unmake_move();
                continue;
            }
            let late = i >= 3 && depth >= 3 && quiet && !in_check && !gives_check;
            let score = if late {
                let reduced = (depth - 1).saturating_sub(reduction(depth, i + 1));
                let score = -self.negamax(reduced, ply + 1, -alpha - 1, -alpha);
                if score > alpha && !self.aborted {
                    self.principal_variation(false, depth - 1, ply + 1, alpha, beta)
                } else {
                    score
                }
            } else {
                self.principal_variation(i == 0, depth - 1, ply + 1, alpha, beta)
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
                    self.update_history(mv, &quiets_tried, depth);
                }
                break;
            }
            if quiet {
                quiets_tried.push(mv);
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

    /// The move from the transposition table first, then captures and promotions by
    /// `move_order_key`, then the killer moves of this ply, then the other quiet moves by
    /// history.
    fn order(&self, moves: &mut [Move], tt_move: Option<Move>, ply: i32) {
        let killers = self.killers[ply as usize];
        let history = &self.history[self.board.side_to_move().index()];
        moves.sort_by_key(|&mv| {
            Reverse(match move_order_key(self.board, mv) {
                _ if Some(mv) == tt_move => i32::MAX,
                tactical if tactical > 0 => 1_000_000 + tactical,
                _ if Some(mv) == killers[0] => 2 * MAX_HISTORY + 2,
                _ if Some(mv) == killers[1] => 2 * MAX_HISTORY + 1,
                _ => history[mv.from().index()][mv.to().index()],
            })
        });
    }

    /// Rewards the quiet move that caused a beta cutoff and penalizes the quiet moves tried
    /// before it, by `depth²`; values decay towards zero as they approach `MAX_HISTORY`.
    fn update_history(&mut self, cutoff: Move, tried: &[Move], depth: u32) {
        let bonus = (depth * depth).min(MAX_HISTORY as u32) as i32;
        let history = &mut self.history[self.board.side_to_move().index()];
        let mut update = |mv: Move, bonus: i32| {
            let h = &mut history[mv.from().index()][mv.to().index()];
            *h += bonus - *h * bonus.abs() / MAX_HISTORY;
        };
        update(cutoff, bonus);
        for &mv in tried {
            update(mv, -bonus);
        }
    }

    /// `mv` followed by the principal variation of the node it leads to becomes the principal
    /// variation of the node at `ply`.
    fn update_pv(&mut self, ply: i32, mv: Move) {
        let ply = ply as usize;
        let child_len = self.pv_len[ply + 1];
        let (head, tail) = self.pv.split_at_mut(ply + 1);
        head[ply][0] = mv;
        head[ply][1..=child_len].copy_from_slice(&tail[0][..child_len]);
        self.pv_len[ply] = child_len + 1;
    }

    /// The principal variation of the root, or just `best` if none is known.
    fn root_pv(&self, best: Move) -> Vec<Move> {
        match self.pv_len[0] {
            0 => vec![best],
            len => self.pv[0][..len].to_vec(),
        }
    }

    /// A quiet move that caused a beta cutoff, to be tried early in sibling nodes.
    fn remember_killer(&mut self, mv: Move, ply: i32) {
        let killers = &mut self.killers[ply as usize];
        if killers[0] != Some(mv) {
            killers[1] = killers[0];
            killers[0] = Some(mv);
        }
    }

    /// Score of the move just made, for the side that made it. The first move gets the full
    /// window; later ones are only tested against a null window at `alpha` and searched again
    /// with the full window if they beat it.
    fn principal_variation(
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

    /// Polls the clock and the stop flags, and reports nodes, every 1024 nodes; the node limit
    /// is exact.
    fn should_abort(&mut self) -> bool {
        if self.nodes.is_multiple_of(1024) {
            self.flush_nodes();
            self.aborted = self.aborted
                || self.stops.iter().any(|s| s.load(Ordering::Relaxed))
                || self.limits.deadline.is_some_and(|d| Instant::now() >= d);
        }
        self.aborted = self.aborted || self.node_limit.is_some_and(|n| self.nodes > n);
        self.aborted
    }
}
