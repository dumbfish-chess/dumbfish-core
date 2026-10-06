use crate::{Board, Move};

/// Number of leaf nodes `depth` plies below `board`; the last ply is counted from the move list.
pub fn perft(board: &mut Board, depth: u32) -> u64 {
    if depth == 0 {
        return 1;
    }
    let moves = board.legal_moves();
    if depth == 1 {
        return moves.len() as u64;
    }
    let mut nodes = 0;
    for &mv in &moves {
        board.make_move(mv);
        nodes += perft(board, depth - 1);
        board.unmake_move();
    }
    nodes
}

/// Perft split by root move, for finding the branch where two generators disagree.
pub fn divide(board: &mut Board, depth: u32) -> Vec<(Move, u64)> {
    if depth == 0 {
        return Vec::new();
    }
    let moves = board.legal_moves();
    moves
        .iter()
        .map(|&mv| {
            board.make_move(mv);
            let nodes = perft(board, depth - 1);
            board.unmake_move();
            (mv, nodes)
        })
        .collect()
}
