//! Static exchange evaluation: the material outcome of a capture sequence on one square.

use dumbfish_core::{Board, Move, MoveKind, PieceKind, Square};

/// Pawn, knight, bishop, rook, queen, king.
const VALUE: [i32; 6] = [100, 300, 300, 500, 900, 20_000];

/// The value of a piece in exchanges, in centipawns.
pub fn piece_value(kind: PieceKind) -> i32 {
    VALUE[kind.index()]
}

/// Material won by `mv`, a capture or promotion legal on `board`, when both sides keep
/// recapturing on its square with their least valuable attacker and may stop whenever that
/// is better. Sliders behind a capturing piece join in as it leaves the line. `gain[d]` is
/// what the side making the `d`-th capture has won if the exchange stopped there; the
/// backward pass lets each side stop instead of capturing.
pub fn see(board: &Board, mv: Move) -> i32 {
    let (from, to) = (mv.from(), mv.to());
    let us = board.side_to_move();
    let mut occupied = board.occupied() ^ from.bb();
    let mut gain = [0; 32];
    gain[0] = match mv.kind() {
        MoveKind::EnPassant => {
            occupied ^= Square::new(to.file(), from.rank()).bb();
            VALUE[PieceKind::Pawn.index()]
        }
        _ => board.piece_on(to).map_or(0, |p| VALUE[p.kind.index()]),
    };
    let mut on_square = board.piece_on(from).expect("a piece moves").kind;
    if let MoveKind::Promotion(kind) = mv.kind() {
        gain[0] += VALUE[kind.index()] - VALUE[PieceKind::Pawn.index()];
        on_square = kind;
    }

    let mut side = !us;
    let mut depth = 0;
    loop {
        let attackers = board.attackers_to(to, occupied) & occupied & board.color_bb(side);
        let Some((sq, kind)) = PieceKind::ALL.iter().find_map(|&kind| {
            let candidates = attackers & board.kind_bb(kind);
            (!candidates.is_empty()).then(|| (candidates.lsb(), kind))
        }) else {
            break;
        };
        depth += 1;
        gain[depth] = VALUE[on_square.index()] - gain[depth - 1];
        occupied ^= sq.bb();
        on_square = kind;
        side = !side;
    }
    while depth > 0 {
        gain[depth - 1] = -(-gain[depth - 1]).max(gain[depth]);
        depth -= 1;
    }
    gain[0]
}
