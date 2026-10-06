use dumbfish::eval::{
    FEATURE_COUNT, FEATURE_NAMES, MAX_PHASE, blend, evaluate, evaluate_with, explain,
    for_each_feature, format_weights, parse_weights, phase,
};
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
fn shipped_weights_are_canonical() {
    let text = include_str!("../weights.txt");
    assert_eq!(format_weights(&parse_weights(text).unwrap()), text);
}

#[test]
fn feature_names_are_unique() {
    assert_eq!(FEATURE_NAMES.len(), FEATURE_COUNT);
    let mut names = FEATURE_NAMES.clone();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), FEATURE_COUNT);
    assert_eq!(FEATURE_NAMES[5], "pawn_a1");
    assert_eq!(FEATURE_NAMES[FEATURE_COUNT - 1], "king_h8");
}

#[test]
fn symmetric_for_both_colors() {
    let weights = (0..FEATURE_COUNT as i32)
        .map(|i| [i, 2 * i, i % 7])
        .collect();
    for fen in POSITIONS {
        let mirrored = mirror(fen);
        let (a, b) = (
            Board::from_fen(fen).unwrap(),
            Board::from_fen(&mirrored).unwrap(),
        );
        assert_eq!(evaluate(&a), evaluate(&b), "{fen} vs {mirrored}");
        assert_eq!(
            evaluate_with(&a, &weights),
            evaluate_with(&b, &weights),
            "{fen} vs {mirrored} with arbitrary weights"
        );
    }
    assert_eq!(evaluate(&Board::startpos()), 0);
}

#[test]
fn side_to_move_negates() {
    let white = Board::from_fen("4k3/8/8/8/8/8/8/3QK3 w - - 0 1").unwrap();
    let black = Board::from_fen("4k3/8/8/8/8/8/8/3QK3 b - - 0 1").unwrap();
    assert!(evaluate(&white) > 0);
    assert_eq!(evaluate(&black), -evaluate(&white));
}

#[test]
fn features_of_a_position() {
    let board = Board::from_fen("4k3/8/8/8/8/8/8/3QK3 b - - 0 1").unwrap();
    let mut values = vec![0; FEATURE_COUNT];
    for_each_feature(&board, |index, value| values[index] += value);
    let active: Vec<_> = (0..FEATURE_COUNT)
        .filter(|&i| values[i] != 0)
        .map(|i| (FEATURE_NAMES[i].as_str(), values[i]))
        .collect();
    assert_eq!(
        active,
        [("queen", 1), ("queen_d1", 1)],
        "the kings on e1 and e8 cancel out"
    );
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
    let all: String = FEATURE_NAMES
        .iter()
        .map(|n| format!("{n} 1 2 3\n"))
        .collect();
    let all = all.as_str();
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
fn explain_lists_non_zero_features() {
    let board = Board::from_fen("4k3/8/8/8/8/8/8/3QK3 b - - 0 1").unwrap();
    let text = explain(&board);
    let lines: Vec<_> = text.lines().collect();
    let names: Vec<_> = lines[1..lines.len() - 2]
        .iter()
        .map(|l| l.split_whitespace().next().unwrap())
        .collect();
    assert_eq!(names, ["queen", "queen_d1"], "{text}");

    let weights = parse_weights(include_str!("../weights.txt")).unwrap();
    let queen = FEATURE_NAMES.iter().position(|n| n == "queen").unwrap();
    let [opening, middlegame, endgame] = weights[queen].map(|w| w.to_string());
    let row: Vec<_> = lines[1].split_whitespace().collect();
    assert_eq!(row[..5], ["queen", "1", &opening, &middlegame, &endgame]);
    assert_eq!(
        lines[lines.len() - 2],
        "phase 4/24: opening 0/24, middlegame 8/24, endgame 16/24"
    );
    let stm = evaluate(&board);
    assert_eq!(
        lines[lines.len() - 1],
        format!(
            "total: {} cp for white, {stm} cp for the side to move",
            -stm
        )
    );
}
