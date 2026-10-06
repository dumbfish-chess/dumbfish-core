use dumbfish_core::MoveKind::*;
use dumbfish_core::Square::*;
use dumbfish_core::board::STARTPOS;
use dumbfish_core::{Board, Color, Move, PieceKind, Square};

const KIWIPETE: &str = "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1";

fn assert_consistent(board: &Board) {
    for sq in Square::ALL {
        let piece = board.piece_on(sq);
        for kind in PieceKind::ALL {
            assert_eq!(
                board.kind_bb(kind).contains(sq),
                piece.is_some_and(|p| p.kind == kind),
                "{sq}"
            );
        }
        for color in [Color::White, Color::Black] {
            assert_eq!(
                board.color_bb(color).contains(sq),
                piece.is_some_and(|p| p.color == color),
                "{sq}"
            );
        }
    }
    assert_eq!(
        board.hash(),
        Board::from_fen(&board.fen()).unwrap().hash(),
        "hash from FEN"
    );
}

#[test]
fn fen_roundtrip() {
    for fen in [
        STARTPOS,
        KIWIPETE,
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
        "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
        "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
        "rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPP1PPP/RNBQKBNR w KQkq f6 0 3",
    ] {
        let board = Board::from_fen(fen).unwrap();
        assert_eq!(board.fen(), fen);
        assert_consistent(&board);
    }
}

#[test]
fn fen_normalization() {
    // No black pawn can take on e3, so the square is dropped.
    let board =
        Board::from_fen("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1").unwrap();
    assert_eq!(
        board.fen(),
        "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1"
    );
    // Move counters are optional.
    assert_eq!(
        Board::from_fen("4k3/8/8/8/8/8/8/4K3 w - -").unwrap().fen(),
        "4k3/8/8/8/8/8/8/4K3 w - - 0 1"
    );
}

#[test]
fn fen_errors() {
    for fen in [
        "",
        "8/8/8/8/8/8/8/8 w - - 0 1",           // no kings
        "4k3/8/8/8/8/8/8/4K2K w - - 0 1",      // two white kings
        "4k3/8/8/8/8/8/8 w - - 0 1",           // 7 ranks
        "4k3/8/8/8/8/8/8/4K4 w - - 0 1",       // 9 files
        "4k3/8/8/8/8/8/8/4K2 w - - 0 1",       // 7 files
        "4k3/8/8/8/8/8/8/4KX2 w - - 0 1",      // bad piece
        "4k3/8/8/8/8/8/8/4K3 x - - 0 1",       // bad side
        "4k3/8/8/8/8/8/8/4K3 w",               // missing fields
        "4k3/8/8/8/8/8/8/4K3 w X - 0 1",       // bad castling
        "4k3/8/8/8/8/8/8/4K3 w K - 0 1",       // right without rook
        "4k3/8/8/8/8/8/8/4K3 w - e9 0 1",      // bad square
        "4k3/8/8/8/8/8/8/4K3 w - e6 0 1",      // no pawn behind ep
        "4k3/8/8/8/8/8/8/4K3 w - - x 1",       // bad clock
        "4k3/8/8/8/8/8/8/4K3 w - - 0 1 extra", // trailing field
        "4k3/8/8/8/8/8/8/P3K3 w - - 0 1",      // pawn on rank 1
        "4k2R/8/8/8/8/8/8/4K3 w - - 0 1",      // black in check, white to move
    ] {
        assert!(Board::from_fen(fen).is_err(), "{fen:?}");
    }
}

/// Makes `mv`, checks the result and that unmaking restores the position.
fn check_move(fen: &str, mv: Move, expected: &str) {
    let mut board = Board::from_fen(fen).unwrap();
    let hash = board.hash();
    board.make_move(mv);
    assert_eq!(board.fen(), expected, "after {mv}");
    assert_consistent(&board);
    board.unmake_move();
    assert_eq!(board.fen(), fen, "after unmaking {mv}");
    assert_eq!(board.hash(), hash);
    assert_consistent(&board);
}

#[test]
fn make_unmake() {
    let cases = [
        (
            STARTPOS,
            Move::new(G1, F3, Normal),
            "rnbqkbnr/pppppppp/8/8/8/5N2/PPPPPPPP/RNBQKB1R b KQkq - 1 1",
        ),
        (
            STARTPOS,
            Move::new(E2, E4, DoublePush),
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1",
        ),
        (
            "4k3/8/8/8/3p4/8/4P3/4K3 w - - 0 1",
            Move::new(E2, E4, DoublePush),
            "4k3/8/8/8/3pP3/8/8/4K3 b - e3 0 1",
        ),
        (
            "rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPP1PPP/RNBQKBNR w KQkq f6 0 3",
            Move::new(E5, F6, EnPassant),
            "rnbqkbnr/ppp1p1pp/5P2/3p4/8/8/PPPP1PPP/RNBQKBNR b KQkq - 0 3",
        ),
        (
            KIWIPETE,
            Move::new(E1, G1, Castle),
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R4RK1 b kq - 1 1",
        ),
        (
            KIWIPETE,
            Move::new(E1, C1, Castle),
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/2KR3R b kq - 1 1",
        ),
        (
            KIWIPETE,
            Move::new(E1, F1, Normal),
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R4K1R b kq - 1 1",
        ),
        (
            KIWIPETE,
            Move::new(H1, G1, Normal),
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K1R1 b Qkq - 1 1",
        ),
        (
            "r3k2r/8/8/8/8/8/8/R3K2R b KQkq - 0 1",
            Move::new(E8, C8, Castle),
            "2kr3r/8/8/8/8/8/8/R3K2R w KQ - 1 2",
        ),
        (
            "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1",
            Move::new(A1, A8, Normal),
            "R3k2r/8/8/8/8/8/8/4K2R b Kk - 0 1",
        ),
        (
            "4k3/8/8/8/8/8/1p6/R3K3 b Q - 0 1",
            Move::new(B2, A1, Promotion(PieceKind::Knight)),
            "4k3/8/8/8/8/8/8/n3K3 w - - 0 2",
        ),
        (
            "4k3/P7/8/8/8/8/8/4K3 w - - 5 9",
            Move::new(A7, A8, Promotion(PieceKind::Queen)),
            "Q3k3/8/8/8/8/8/8/4K3 b - - 0 9",
        ),
    ];
    for (fen, mv, expected) in cases {
        check_move(fen, mv, expected);
    }
}

#[test]
fn transposition_has_same_hash() {
    let mut board = Board::startpos();
    let start = board.hash();
    for (from, to) in [(G1, F3), (G8, F6), (F3, G1)] {
        board.make_move(Move::new(from, to, Normal));
        assert_ne!(board.hash(), start);
    }
    board.make_move(Move::new(F6, G8, Normal));
    assert_eq!(board.hash(), start);
}
