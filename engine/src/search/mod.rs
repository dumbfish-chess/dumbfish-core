use std::cmp::Reverse;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use dumbfish_core::{Board, Color, Move, MoveKind, MoveList, PieceKind, Square};

use crate::eval;
use crate::see::{piece_value, see};
use crate::skill::{MAX_LEVEL, Skill};
use crate::style::Style;
use crate::syzygy::{RootProbe, Tablebases};
use crate::time::Deadline;
use crate::tt::{Bound, Entry, TranspositionTable, score_from_tt, score_to_tt};

pub const MAX_DEPTH: u32 = 64;
/// Plies of the main search; quiescence search does not count them.
const MAX_PLY: usize = MAX_DEPTH as usize + 1;
const INFINITY: i32 = 32_000;
/// Being mated `ply` plies from the root scores `ply - MATE`.
pub const MATE: i32 = 31_000;
/// A tablebase win `ply` plies from the root scores `TB_WIN - ply`, below any mate.
pub const TB_WIN: i32 = 30_000;
/// The share of the soft time limit to use after the best move stayed the same for 0, 1, 2,
/// 3, 4 or more iterations.
const STABILITY: [f64; 5] = [2.5, 1.2, 0.9, 0.8, 0.75];

/// Pawn structures per side in the correction history.
const CORRECTION_SIZE: usize = 1 << 14;
/// The correction history counts in this fraction of a centipawn.
const CORRECTION_GRAIN: i32 = 256;
const MAX_CORRECTION: i32 = 64 * CORRECTION_GRAIN;

#[derive(Default)]
pub struct Limits {
    pub depth: Option<u32>,
    pub nodes: Option<u64>,
    /// Stop once a mate in this many moves or fewer is found.
    pub mate: Option<u32>,
    /// The search stops here.
    pub deadline: Deadline,
    /// No new iteration starts after this, as stretched or shrunk by the search: see
    /// `STABILITY`.
    pub soft_deadline: Deadline,
    /// When the clock behind `soft_deadline` started: at `go`, or at `ponderhit`; if unset, the
    /// soft deadline is not stretched or shrunk.
    pub clock_start: Deadline,
    /// Root moves to choose from; all legal moves if empty.
    pub searchmoves: Vec<Move>,
    /// Below `MAX_LEVEL`, adds the skill's node budget and picks the move with its noise; the
    /// search then runs on one thread.
    pub skill: Option<Skill>,
    pub style: Style,
    /// Search threads sharing the transposition table (Lazy SMP); 0 counts as 1.
    pub threads: usize,
    /// Best root moves to report per iteration, each with its own score and line; 0 counts as 1.
    /// Ignored below full skill.
    pub multipv: usize,
    /// Endgame tables, used at full skill: a root position in them keeps only the moves with
    /// the best result, and otherwise the search takes results from them.
    pub tablebases: Option<Arc<Tablebases>>,
}

/// A finished iteration of iterative deepening.
pub struct Iteration {
    pub depth: u32,
    /// Rank of this line among the best root moves, from 1; see `Limits::multipv`.
    pub multipv: usize,
    /// Deepest ply the main thread reached, quiescence search included.
    pub seldepth: u32,
    /// Used transposition table entries, per mille.
    pub hashfull: usize,
    pub score: i32,
    pub best: Move,
    /// The principal variation, starting with `best`.
    pub pv: Vec<Move>,
    pub nodes: u64,
    /// Successful tablebase probes in the search, by all threads.
    pub tb_hits: u64,
    pub elapsed: Duration,
}

/// Counters shared by the search threads.
#[derive(Default)]
struct Counters {
    nodes: AtomicU64,
    tb_hits: AtomicU64,
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
    on_iteration: impl FnMut(&Iteration),
) -> (Option<Move>, u64) {
    search_reporting(board, limits, stop, tt, on_iteration, |_, _, _| {})
}

/// `search` that also calls `on_root_move(depth, move, number from 1)` as the main thread starts
/// on each root move.
pub fn search_reporting(
    board: &mut Board,
    limits: &Limits,
    stop: &AtomicBool,
    tt: &TranspositionTable,
    mut on_iteration: impl FnMut(&Iteration),
    mut on_root_move: impl FnMut(u32, Move, usize),
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
    let counters = Counters::default();
    let mut moves = root_moves(board, limits);
    let tablebases = limits.tablebases.as_deref().filter(|_| skill.is_none());
    let tb_root = tablebases.and_then(|tb| tb.rank_root(board, &moves));
    if let Some(probe) = &tb_root {
        moves = probe.moves.iter().copied().collect();
    }
    let in_search = tablebases.filter(|_| tb_root.is_none());

    thread::scope(|scope| {
        for helper in 1..threads {
            let mut board = board.clone();
            let mut moves: MoveList = moves.iter().copied().collect();
            let (finished, counters) = (&finished, &counters);
            scope.spawn(move || {
                let mut searcher = Searcher::new(
                    &mut board,
                    limits,
                    None,
                    [stop, finished],
                    tt,
                    counters,
                    in_search,
                );
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
        let mut searcher = Searcher::new(
            board,
            limits,
            node_limit,
            [stop, &finished],
            tt,
            &counters,
            in_search,
        );
        searcher.on_root_move = Some(&mut on_root_move);
        let best = main_search(
            searcher,
            moves,
            tb_root.as_ref(),
            skill,
            max_depth,
            start,
            &mut on_iteration,
        );
        finished.store(true, Ordering::Relaxed);
        best
    })
    .map_or((None, counters.nodes.load(Ordering::Relaxed)), |best| {
        (Some(best), counters.nodes.load(Ordering::Relaxed))
    })
}

/// The score of a root move to report: the tablebase result if the root is in the tables,
/// unless the search found a mate.
fn reported(tb: Option<&RootProbe>, score: i32) -> i32 {
    match tb {
        Some(tb) if score.abs() < MATE - MAX_DEPTH as i32 => match tb.wdl {
            2 => TB_WIN - tb.dtz.abs(),
            -2 => tb.dtz.abs() - TB_WIN,
            wdl => wdl,
        },
        _ => score,
    }
}

/// The iterative deepening of the calling thread over `moves`, whose tablebase result is `tb`;
/// `None` if there are no moves.
fn main_search(
    mut searcher: Searcher,
    mut moves: MoveList,
    tb: Option<&RootProbe>,
    skill: Option<Skill>,
    max_depth: u32,
    start: Instant,
    on_iteration: &mut impl FnMut(&Iteration),
) -> Option<Move> {
    let mut best = moves.first().copied();
    let lines = searcher.limits.multipv.clamp(1, moves.len().max(1));
    let mut scores: Vec<(Move, i32)> = Vec::new();
    let (mut stable, mut previous, mut share) = (0, None, 1.0);
    for depth in 1..=max_depth {
        let limits = searcher.limits;
        if let Some(soft) = limits.soft_deadline.get()
            && depth > 1
        {
            let clock = limits.clock_start.get().unwrap_or(soft);
            if Instant::now() >= clock + soft.saturating_duration_since(clock).mul_f64(share) {
                break;
            }
        }
        let mut found = Vec::new();
        if skill.is_some() {
            let Some(all) = searcher.root_scores(depth, &moves) else {
                break;
            };
            let (mv, score) = all
                .iter()
                .copied()
                .reduce(|a, b| if b.1 > a.1 { b } else { a })
                .expect("a finished iteration has moves");
            found.push((mv, score, vec![mv]));
            scores = all;
        } else {
            for line in 0..lines {
                let mut remaining = MoveList::default();
                for &mv in moves.iter() {
                    if !found.iter().any(|&(f, _, _)| f == mv) {
                        remaining.push(mv);
                    }
                }
                let previous = scores.get(line).map(|&(_, score)| score);
                let Some((mv, score)) = searcher.aspiration(depth, &remaining, previous) else {
                    break;
                };
                found.push((mv, score, searcher.root_pv(mv)));
            }
            found.sort_by_key(|&(_, score, _)| Reverse(score));
            if found.len() == lines {
                scores = found.iter().map(|&(mv, score, _)| (mv, score)).collect();
            }
        }
        let Some(&(mv, top, _)) = found.first() else {
            break;
        };
        best = Some(mv);
        stable = match previous {
            Some((m, _)) if m == mv => stable + 1,
            _ => 0,
        };
        let drop = previous.map_or(0, |(_, score)| score - top);
        share = STABILITY[stable.min(4)]
            * if drop > 50 {
                1.5
            } else if drop > 25 {
                1.25
            } else {
                1.0
            };
        previous = Some((mv, top));
        for &(mv, _, _) in found.iter().rev() {
            put_first(&mut moves, mv);
        }
        for (line, (mv, score, pv)) in found.into_iter().enumerate() {
            on_iteration(&Iteration {
                depth,
                multipv: line + 1,
                seldepth: searcher.seldepth,
                hashfull: searcher.tt.hashfull(),
                score: reported(tb, score),
                best: mv,
                pv,
                nodes: searcher.counters.nodes.load(Ordering::Relaxed) + searcher.nodes
                    - searcher.flushed,
                tb_hits: searcher.counters.tb_hits.load(Ordering::Relaxed),
                elapsed: start.elapsed(),
            });
        }
        let mated = searcher
            .limits
            .mate
            .is_some_and(|moves| top >= MATE - (2 * moves as i32 - 1));
        if searcher.aborted || mated {
            break;
        }
    }
    searcher.flush_nodes();
    if let Some(skill) = skill {
        best = skill.choose(&scores).or(best);
    }
    best
}

/// The legal moves in search order, only those in `limits.searchmoves` if it names any legal
/// move.
fn root_moves(board: &Board, limits: &Limits) -> MoveList {
    let legal = board.legal_moves();
    let mut moves: MoveList = legal
        .iter()
        .copied()
        .filter(|mv| limits.searchmoves.contains(mv))
        .collect();
    if moves.is_empty() {
        moves = legal;
    }
    order_moves(board, &mut moves);
    moves
}

/// Moves `mv` to the front, keeping the order of the others.
fn put_first(moves: &mut [Move], mv: Move) {
    if let Some(i) = moves.iter().position(|&m| m == mv) {
        moves[..=i].rotate_right(1);
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
    /// Nodes already added to `counters`.
    flushed: u64,
    counters: &'a Counters,
    aborted: bool,
    /// Two quiet moves per ply that caused beta cutoffs, most recent first.
    killers: [[Option<Move>; 2]; MAX_PLY],
    /// How often a quiet move, by side, from and to square, caused cutoffs; within
    /// `-MAX_HISTORY..=MAX_HISTORY`.
    history: Box<[[[i32; 64]; 64]; 2]>,
    /// History of a quiet move after a given previous move, see `continuation_index`.
    continuation: Vec<i32>,
    /// How often a capture, by moving piece, target square and captured kind, caused cutoffs;
    /// see `capture_index`.
    capture_history: Vec<i32>,
    /// Triangular table of principal variations: `pv[ply][..pv_len[ply]]` for each ply.
    pv: Box<[[Move; MAX_PLY + 1]; MAX_PLY + 1]>,
    pv_len: [usize; MAX_PLY + 1],
    /// Deepest ply reached, quiescence search included.
    seldepth: u32,
    /// The move made at each ply of the current line; `None` for a null move.
    played: [Option<Move>; MAX_PLY + 1],
    /// A move to leave out at each ply, for the singular extension search.
    excluded: [Option<Move>; MAX_PLY + 1],
    /// The static evaluation at each ply of the current line; `None` in check.
    static_evals: [Option<i32>; MAX_PLY + 1],
    /// How far searches from each pawn structure, by side to move, ended up from the static
    /// evaluation, in `CORRECTION_GRAIN` units; added to the evaluation.
    correction: Box<[[i32; CORRECTION_SIZE]; 2]>,
    /// The quiet move that last refuted a move, by the piece that made it and its target square.
    countermoves: Box<[[Option<Move>; 64]; 12]>,
    /// Endgame tables to take results from, after captures and pawn moves.
    tablebases: Option<&'a Tablebases>,
    /// See `search_reporting`; only for the main thread.
    on_root_move: Option<&'a mut dyn FnMut(u32, Move, usize)>,
}

mod alphabeta;
mod ordering;
mod root;

use ordering::{MAX_HISTORY, move_order_key, order_moves};

impl<'a> Searcher<'a> {
    fn new(
        board: &'a mut Board,
        limits: &'a Limits,
        node_limit: Option<u64>,
        stops: [&'a AtomicBool; 2],
        tt: &'a TranspositionTable,
        counters: &'a Counters,
        tablebases: Option<&'a Tablebases>,
    ) -> Self {
        Self {
            board,
            limits,
            node_limit,
            stops,
            tt,
            nodes: 0,
            flushed: 0,
            counters,
            aborted: false,
            killers: [[None; 2]; MAX_PLY],
            history: Box::new([[[0; 64]; 64]; 2]),
            continuation: vec![0; 12 * 64 * 12 * 64],
            capture_history: vec![0; 12 * 64 * 6],
            pv: Box::new(
                [[Move::new(Square::A1, Square::A1, MoveKind::Normal); MAX_PLY + 1]; MAX_PLY + 1],
            ),
            pv_len: [0; MAX_PLY + 1],
            seldepth: 0,
            played: [None; MAX_PLY + 1],
            excluded: [None; MAX_PLY + 1],
            static_evals: [None; MAX_PLY + 1],
            correction: Box::new([[0; CORRECTION_SIZE]; 2]),
            countermoves: Box::new([[None; 64]; 12]),
            tablebases,
            on_root_move: None,
        }
    }

    fn flush_nodes(&mut self) {
        self.counters
            .nodes
            .fetch_add(self.nodes - self.flushed, Ordering::Relaxed);
        self.flushed = self.nodes;
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

    /// The side to move and its pawn structure's slot in `correction`.
    fn correction_slot(&self) -> (usize, usize) {
        let pawns = |color| self.board.pieces(color, PieceKind::Pawn).0;
        let key = pawns(Color::White).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ pawns(Color::Black).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
        let slot = (key >> (64 - CORRECTION_SIZE.trailing_zeros())) as usize;
        (self.board.side_to_move().index(), slot)
    }

    /// The static evaluation `raw` plus the correction of the pawn structure.
    fn corrected(&self, raw: i32) -> i32 {
        let (side, slot) = self.correction_slot();
        raw + self.correction[side][slot] / CORRECTION_GRAIN
    }

    /// Moves the correction of the pawn structure towards `score - raw`, the more the deeper
    /// the search that found `score`.
    fn update_correction(&mut self, raw: i32, score: i32, depth: u32) {
        let (side, slot) = self.correction_slot();
        let weight = (depth as i32 + 1).min(16);
        let entry = &mut self.correction[side][slot];
        *entry = ((*entry * (256 - weight) + (score - raw) * CORRECTION_GRAIN * weight) / 256)
            .clamp(-MAX_CORRECTION, MAX_CORRECTION);
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
                || self.limits.deadline.passed();
        }
        self.aborted = self.aborted || self.node_limit.is_some_and(|n| self.nodes > n);
        self.aborted
    }
}
