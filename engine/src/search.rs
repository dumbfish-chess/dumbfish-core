use std::cmp::Reverse;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use dumbfish_core::{Board, Move, MoveKind, MoveList};

use crate::eval;

pub const MAX_DEPTH: u32 = 64;
const INFINITY: i32 = 32_000;
/// Being mated `ply` plies from the root scores `ply - MATE`.
pub const MATE: i32 = 31_000;

#[derive(Default)]
pub struct Limits {
    pub depth: Option<u32>,
    pub nodes: Option<u64>,
    pub deadline: Option<Instant>,
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
    mut on_iteration: impl FnMut(&Iteration),
) -> (Option<Move>, u64) {
    let start = Instant::now();
    let mut moves = board.legal_moves();
    order_moves(board, &mut moves);
    let mut best = moves.first().copied();
    let mut searcher = Searcher {
        board,
        limits,
        stop,
        nodes: 0,
        aborted: false,
    };
    for depth in 1..=limits.depth.unwrap_or(MAX_DEPTH).min(MAX_DEPTH) {
        let Some((mv, score)) = searcher.root(depth, &moves) else {
            break;
        };
        best = Some(mv);
        on_iteration(&Iteration {
            depth,
            score,
            best: mv,
            nodes: searcher.nodes,
            elapsed: start.elapsed(),
        });
    }
    (best, searcher.nodes)
}

/// Captures and promotions first, by most valuable victim, then least valuable attacker;
/// quiet moves keep their generation order.
fn order_moves(board: &Board, moves: &mut [Move]) {
    moves.sort_by_key(|&mv| Reverse(move_order_key(board, mv)));
}

fn move_order_key(board: &Board, mv: Move) -> i32 {
    // P N B R Q K
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
    stop: &'a AtomicBool,
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

    fn negamax(&mut self, depth: u32, ply: i32, mut alpha: i32, beta: i32) -> i32 {
        self.nodes += 1;
        if self.should_abort() || self.board.is_repetition() {
            return 0;
        }
        let mut moves = self.board.legal_moves();
        if moves.is_empty() {
            return if self.board.in_check() { ply - MATE } else { 0 };
        }
        if self.board.halfmove_clock() >= 100 {
            return 0;
        }
        if depth == 0 {
            return eval::evaluate(self.board);
        }
        order_moves(self.board, &mut moves);

        let mut best = -INFINITY;
        for &mv in &moves {
            self.board.make_move(mv);
            let score = -self.negamax(depth - 1, ply + 1, -beta, -alpha);
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

    /// Polls the clock and the stop flag every 1024 nodes; the node limit is exact.
    fn should_abort(&mut self) -> bool {
        self.aborted = self.aborted
            || self.limits.nodes.is_some_and(|n| self.nodes > n)
            || (self.nodes.is_multiple_of(1024)
                && (self.stop.load(Ordering::Relaxed)
                    || self.limits.deadline.is_some_and(|d| Instant::now() >= d)));
        self.aborted
    }
}
