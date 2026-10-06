use dumbfish::eval::{FEATURES, MAX_PHASE, blend, evaluate, explain, parse_weights, phase};
use dumbfish_core::Board;

const POSITIONS: [&str; 9] = [
    "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
    "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
    "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
    "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
    "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
    "rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPP1PPP/RNBQKBNR w KQkq f6 0 3",
    "4k3/8/8/8/8/8/8/3QK3 b - - 0 1",
    "8/5pk1/6p1/8/3R4/6PP/5PK1/r7 w - - 0 40",
];

/// The same position with colors swapped and the board flipped.
fn mirror(fen: &str) -> String {
    let fields: Vec<_> = fen.split_whitespace().collect();
    let swap_case = |s: &str| -> String {
        s.chars()
            .map(|c| match c.is_ascii_uppercase() {
                true => c.to_ascii_lowercase(),
                false => c.to_ascii_uppercase(),
            })
            .collect()
    };
    let placement: Vec<_> = fields[0].split('/').rev().map(swap_case).collect();
    let side = if fields[1] == "w" { "b" } else { "w" };
    let castling = swap_case(fields[2]);
    let en_passant = match fields[3] {
        "-" => "-".to_string(),
        sq => format!("{}{}", &sq[..1], if &sq[1..] == "3" { "6" } else { "3" }),
    };
    format!(
        "{} {side} {castling} {en_passant} {} {}",
        placement.join("/"),
        fields[4],
        fields[5]
    )
}

#[test]
fn shipped_weights_parse() {
    assert_eq!(
        parse_weights(include_str!("../weights.txt")).unwrap().len(),
        FEATURES.len()
    );
}

#[test]
fn symmetric_for_both_colors() {
    for fen in POSITIONS {
        let mirrored = mirror(fen);
        let (a, b) = (
            Board::from_fen(fen).unwrap(),
            Board::from_fen(&mirrored).unwrap(),
        );
        assert_eq!(evaluate(&a), evaluate(&b), "{fen} vs {mirrored}");
    }
    assert_eq!(evaluate(&Board::startpos()), 0);
}

#[test]
fn material() {
    let white_queen = Board::from_fen("4k3/8/8/8/8/8/8/3QK3 w - - 0 1").unwrap();
    assert_eq!(evaluate(&white_queen), 900);
    let black_to_move = Board::from_fen("4k3/8/8/8/8/8/8/3QK3 b - - 0 1").unwrap();
    assert_eq!(evaluate(&black_to_move), -900);
}

#[test]
fn phase_and_blend() {
    assert_eq!(phase(&Board::startpos()), MAX_PHASE);
    assert_eq!(
        phase(&Board::from_fen("4k3/pppppppp/8/8/8/8/PPPPPPPP/4K3 w - - 0 1").unwrap()),
        0
    );
    let many_queens = Board::from_fen("QQQQkQQQ/8/8/8/8/8/8/4K3 b - - 0 1").unwrap();
    assert_eq!(phase(&many_queens), MAX_PHASE);

    assert_eq!(blend(MAX_PHASE), [MAX_PHASE, 0, 0]);
    assert_eq!(blend(MAX_PHASE / 2), [0, MAX_PHASE, 0]);
    assert_eq!(blend(0), [0, 0, MAX_PHASE]);
    for p in 0..=MAX_PHASE {
        let b = blend(p);
        assert_eq!(b.iter().sum::<i32>(), MAX_PHASE, "phase {p}");
        assert!(b.iter().all(|&x| x >= 0), "phase {p}");
        assert!(
            b[0] == 0 || b[2] == 0,
            "opening and endgame never mix, phase {p}"
        );
    }
}

#[test]
fn weights_format() {
    let all = "pawn 1 2 3\nknight 1 2 3\nbishop 1 2 3\nrook 1 2 3\nqueen 1 2 3\n";
    assert_eq!(parse_weights(all).unwrap()[0], [1, 2, 3]);
    assert!(parse_weights(&format!("# header\n\n  {all}")).is_ok());

    for (bad, error) in [
        (format!("{all}king 1 2 3"), "unknown feature"),
        (format!("{all}pawn 1 2 3"), "duplicate feature"),
        (all.replace("queen 1 2 3\n", ""), "missing feature"),
        (all.replace("pawn 1 2 3", "pawn 1 2 x"), "invalid weight"),
        (all.replace("pawn 1 2 3", "pawn 1 2"), "expected"),
    ] {
        let message = parse_weights(&bad).unwrap_err();
        assert!(message.contains(error), "{message:?} for {bad:?}");
    }
}

#[test]
fn explain_lists_every_feature() {
    let board = Board::from_fen("4k3/8/8/8/8/8/8/3QK3 b - - 0 1").unwrap();
    let text = explain(&board);
    let lines: Vec<_> = text.lines().collect();
    assert_eq!(lines.len(), FEATURES.len() + 3, "{text}");
    for (feature, line) in FEATURES.iter().zip(&lines[1..]) {
        assert!(line.starts_with(feature.name), "{line:?}");
    }
    let queen: Vec<_> = lines[5].split_whitespace().collect();
    assert_eq!(queen, ["queen", "1", "900", "900", "900", "900.00"]);
    assert_eq!(
        lines[6],
        "phase 4/24: opening 0/24, middlegame 8/24, endgame 16/24"
    );
    assert_eq!(
        lines[7],
        "total: 900 cp for white, -900 cp for the side to move"
    );
}
