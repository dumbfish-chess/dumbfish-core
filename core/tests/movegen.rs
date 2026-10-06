use dumbfish_core::{Board, MoveKind};

/// The standard perft set (CPW), with the number of legal moves at the root.
const POSITIONS: [(&str, usize); 6] = [
    (
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        20,
    ),
    (
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        48,
    ),
    ("8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1", 14),
    (
        "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
        6,
    ),
    (
        "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
        44,
    ),
    (
        "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
        46,
    ),
];

/// Legal moves in UCI notation, sorted.
fn moves(fen: &str) -> Vec<String> {
    let mut moves: Vec<_> = Board::from_fen(fen)
        .unwrap()
        .legal_moves()
        .iter()
        .map(|m| m.to_string())
        .collect();
    moves.sort();
    moves
}

fn has(fen: &str, mv: &str) -> bool {
    moves(fen).iter().any(|m| m == mv)
}

#[test]
fn move_counts() {
    for (fen, count) in POSITIONS {
        assert_eq!(moves(fen).len(), count, "{fen}");
    }
}

#[test]
fn double_check_allows_only_king_moves() {
    assert_eq!(
        moves("4k3/3R4/8/8/8/3n4/8/r3K3 w - - 0 1"),
        ["e1d2", "e1e2"]
    );
}

#[test]
fn king_cannot_retreat_along_checking_ray() {
    assert_eq!(
        moves("4k3/8/8/8/8/8/4K3/4r3 w - - 0 1"),
        ["e2d2", "e2d3", "e2e1", "e2f2", "e2f3"]
    );
}

#[test]
fn pinned_piece_stays_on_pin_line() {
    let rook_moves: Vec<_> = moves("4k3/4r3/8/8/8/8/4R3/4K3 w - - 0 1")
        .into_iter()
        .filter(|m| m.starts_with("e2"))
        .collect();
    assert_eq!(rook_moves, ["e2e3", "e2e4", "e2e5", "e2e6", "e2e7"]);
}

#[test]
fn en_passant() {
    assert!(has("8/8/8/K2pP3/8/8/8/7k w - d6 0 1", "e5d6"));
    assert!(
        !has("8/8/8/K2pP2r/8/8/8/7k w - d6 0 1", "e5d6"),
        "both pawns leave the rank and expose the king"
    );
    assert!(
        has("4k3/8/8/3pP3/4K3/8/8/8 w - d6 0 1", "e5d6"),
        "en passant captures the checking pawn"
    );
    let board = Board::from_fen("4k3/8/8/3pP3/4K3/8/8/8 w - d6 0 1").unwrap();
    let ep = board
        .legal_moves()
        .iter()
        .find(|m| m.to_string() == "e5d6")
        .copied()
        .unwrap();
    assert_eq!(ep.kind(), MoveKind::EnPassant);
}

#[test]
fn castling() {
    assert!(has("4k3/8/8/8/8/8/8/R3K2R w KQ - 0 1", "e1g1"));
    assert!(has("4k3/8/8/8/8/8/8/R3K2R w KQ - 0 1", "e1c1"));
    assert!(has("r3k2r/8/8/8/8/8/8/4K3 b kq - 0 1", "e8g8"));
    assert!(has("r3k2r/8/8/8/8/8/8/4K3 b kq - 0 1", "e8c8"));
    assert!(!has("4k3/8/8/8/8/8/8/R3K2R w Q - 0 1", "e1g1"), "no right");
    assert!(
        !has("4kr2/8/8/8/8/8/8/R3K2R w KQ - 0 1", "e1g1"),
        "king passes an attacked square"
    );
    assert!(
        !has("2r1k3/8/8/8/8/8/8/R3K2R w KQ - 0 1", "e1c1"),
        "king lands on an attacked square"
    );
    assert!(
        has("1r2k3/8/8/8/8/8/8/R3K2R w KQ - 0 1", "e1c1"),
        "only the rook passes b1"
    );
    assert!(
        !has("4k3/8/8/8/8/8/8/RN2K2R w KQ - 0 1", "e1c1"),
        "b1 is occupied"
    );
    let in_check = moves("4k3/4r3/8/8/8/8/8/R3K2R w KQ - 0 1");
    assert!(
        !in_check.iter().any(|m| m == "e1g1" || m == "e1c1"),
        "castling out of check"
    );
}

#[test]
fn promotions() {
    let promotions: Vec<_> = moves("r3k3/1P6/8/8/8/8/8/4K3 w - - 0 1")
        .into_iter()
        .filter(|m| m.len() == 5)
        .collect();
    assert_eq!(
        promotions,
        [
            "b7a8b", "b7a8n", "b7a8q", "b7a8r", "b7b8b", "b7b8n", "b7b8q", "b7b8r"
        ]
    );
}

#[test]
fn mate_and_stalemate() {
    let mate =
        Board::from_fen("rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3").unwrap();
    assert!(mate.legal_moves().is_empty() && mate.in_check());
    let stalemate = Board::from_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").unwrap();
    assert!(stalemate.legal_moves().is_empty() && !stalemate.in_check());
}

/// Random games: every generated move must keep the mover's king safe and unmake exactly.
#[test]
fn random_games_keep_board_consistent() {
    // xorshift64*
    let mut state = 0x1234_5678_9ABC_DEF0u64;
    let mut next = move || {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        state.wrapping_mul(0x2545_F491_4F6C_DD1D)
    };
    for (start, _) in POSITIONS {
        for _ in 0..10 {
            let mut board = Board::from_fen(start).unwrap();
            let mut played = 0;
            for _ in 0..200 {
                let moves = board.legal_moves();
                if moves.is_empty() {
                    break;
                }
                let (fen, hash) = (board.fen(), board.hash());
                for &mv in &moves {
                    board.make_move(mv);
                    let (mover, opponent) = (!board.side_to_move(), board.side_to_move());
                    let attackers = board.attackers_to(board.king(mover), board.occupied())
                        & board.color_bb(opponent);
                    assert!(attackers.is_empty(), "{fen}: {mv} leaves the king in check");
                    board.unmake_move();
                    assert_eq!(
                        (board.fen(), board.hash()),
                        (fen.clone(), hash),
                        "{fen}: unmaking {mv}"
                    );
                }
                board.make_move(moves[next() as usize % moves.len()]);
                played += 1;
                let reparsed = Board::from_fen(&board.fen()).unwrap();
                assert_eq!(
                    board.hash(),
                    reparsed.hash(),
                    "incremental hash at {}",
                    board.fen()
                );
            }
            for _ in 0..played {
                board.unmake_move();
            }
            assert_eq!(board.fen(), Board::from_fen(start).unwrap().fen());
        }
    }
}

#[test]
fn move_list_can_be_reordered() {
    let mut moves = Board::startpos().legal_moves();
    let mut expected: Vec<_> = moves.iter().map(|m| m.to_string()).collect();
    moves.sort_unstable_by_key(|m| std::cmp::Reverse(m.to_string()));
    expected.sort_unstable_by(|a, b| b.cmp(a));
    assert_eq!(
        moves.iter().map(|m| m.to_string()).collect::<Vec<_>>(),
        expected
    );
}
