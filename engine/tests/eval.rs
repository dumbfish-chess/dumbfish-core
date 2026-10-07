use dumbfish::eval::{
    FEATURE_COUNT, FEATURE_NAMES, MAX_PHASE, blend, evaluate, evaluate_styled, evaluate_with,
    explain, for_each_feature, format_weights, group, parse_weights, phase,
};
use dumbfish::style::{GROUPS, PRESETS, Style, parse_presets};
use dumbfish_core::{Board, Color, Piece, PieceKind, Square};

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
    assert_eq!(FEATURE_NAMES[5 + 6 * 64 - 1], "king_h8");
    assert_eq!(FEATURE_NAMES[5 + 6 * 64 + 3 + 5], "passed_7");
    assert_eq!(FEATURE_NAMES[FEATURE_COUNT - 1], "hanging");
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
        [
            ("queen", 1),
            ("queen_d1", 1),
            ("mobility_queen_17", 1),
            ("king_attack_queen", 2),
            ("king_attackers_0", -1),
            ("king_attackers_1", 1)
        ],
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
        (
            all.lines()
                .filter(|&l| l != "queen 1 2 3")
                .map(|l| format!("{l}\n"))
                .collect(),
            "missing feature",
        ),
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
    let text = explain(&board, &Style::default());
    let lines: Vec<_> = text.lines().collect();
    let groups = lines.iter().position(|l| l.starts_with("group")).unwrap();
    let names: Vec<_> = lines[1..groups]
        .iter()
        .map(|l| l.split_whitespace().next().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "queen",
            "queen_d1",
            "mobility_queen_17",
            "king_attack_queen",
            "king_attackers_0",
            "king_attackers_1"
        ],
        "{text}"
    );

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

/// Pawn structure features recomputed square by square, from each side's own point of view.
fn reference_pawn_structure(board: &Board) -> Vec<(String, i32)> {
    let mut values = std::collections::BTreeMap::<String, i32>::new();
    for (color, sign) in [(Color::White, 1), (Color::Black, -1)] {
        let pawns_of = |c: Color| -> Vec<(i32, i32)> {
            Square::ALL
                .into_iter()
                .filter(|&sq| board.piece_on(sq) == Some(Piece::new(c, PieceKind::Pawn)))
                .map(|sq| {
                    let rank = sq.rank() as i32;
                    (
                        sq.file() as i32,
                        if c == Color::White { rank } else { 7 - rank },
                    )
                })
                .collect()
        };
        let (ours, theirs) = (pawns_of(color), pawns_of(!color));
        let theirs: Vec<_> = theirs.into_iter().map(|(f, r)| (f, 7 - r)).collect();
        for &(file, rank) in &ours {
            let mut add = |name: String| *values.entry(name).or_default() += sign;
            let doubled = ours.iter().any(|&(f, r)| f == file && r > rank);
            let isolated = !ours.iter().any(|&(f, _)| (f - file).abs() == 1);
            let supported = ours
                .iter()
                .any(|&(f, r)| (f - file).abs() == 1 && r <= rank);
            let stop_attacked = theirs
                .iter()
                .any(|&(f, r)| (f - file).abs() == 1 && r == rank + 2);
            let blocked = theirs
                .iter()
                .any(|&(f, r)| (f - file).abs() <= 1 && r > rank);
            if doubled {
                add("doubled".into());
            }
            if isolated {
                add("isolated".into());
            } else if !supported && stop_attacked {
                add("backward".into());
            }
            if !doubled && !blocked {
                add(format!("passed_{}", rank + 1));
            }
        }
    }
    values.into_iter().filter(|&(_, v)| v != 0).collect()
}

fn pawn_structure(board: &Board) -> Vec<(String, i32)> {
    let mut values = vec![0; FEATURE_COUNT];
    for_each_feature(board, |index, value| values[index] += value);
    let pawn_features = ["doubled", "isolated", "backward"];
    let mut found: Vec<_> = (0..FEATURE_COUNT)
        .filter(|&i| values[i] != 0)
        .map(|i| (FEATURE_NAMES[i].clone(), values[i]))
        .filter(|(name, _)| pawn_features.contains(&name.as_str()) || name.starts_with("passed_"))
        .collect();
    found.sort();
    found
}

#[test]
fn pawn_structure_examples() {
    let features = |fen| pawn_structure(&Board::from_fen(fen).unwrap());
    let named = |pairs: &[(&str, i32)]| -> Vec<(String, i32)> {
        pairs.iter().map(|&(n, v)| (n.to_string(), v)).collect()
    };
    assert_eq!(
        features("4k3/8/8/8/8/P7/P7/4K3 w - - 0 1"),
        named(&[("doubled", 1), ("isolated", 2), ("passed_3", 1)]),
        "a2 behind a3: doubled, both isolated, only a3 passed"
    );
    assert_eq!(
        features("4k3/8/8/4p3/2P5/3P4/8/4K3 w - - 0 1"),
        named(&[("backward", 1), ("isolated", -1), ("passed_4", 1)]),
        "d3 is backward: c4 is ahead and e5 controls d4; e5 is isolated and stopped by d3"
    );
    assert_eq!(features(dumbfish_core::board::STARTPOS), named(&[]));
}

#[test]
fn pawn_structure_matches_reference() {
    let mut state = 0xDEAD_BEEF_u64;
    let mut next = move || {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        state.wrapping_mul(0x2545_F491_4F6C_DD1D)
    };
    let mut checked = 0;
    for fen in POSITIONS {
        let mut board = Board::from_fen(fen).unwrap();
        for _ in 0..120 {
            assert_eq!(
                pawn_structure(&board),
                reference_pawn_structure(&board),
                "{}",
                board.fen()
            );
            checked += 1;
            let moves = board.legal_moves();
            if moves.is_empty() {
                break;
            }
            board.make_move(moves[next() as usize % moves.len()]);
        }
    }
    assert!(checked > 500, "{checked}");
}

/// Mobility features recomputed by walking rays square by square.
fn reference_mobility(board: &Board) -> Vec<(String, i32)> {
    let mut values = std::collections::BTreeMap::<String, i32>::new();
    let on_board = |f: i32, r: i32| (0..8).contains(&f) && (0..8).contains(&r);
    for (color, sign) in [(Color::White, 1), (Color::Black, -1)] {
        let enemy_pawn_attacks = |f: i32, r: i32| {
            let pawn_rank = if color == Color::White { r + 1 } else { r - 1 };
            [f - 1, f + 1].into_iter().any(|pf| {
                on_board(pf, pawn_rank)
                    && board.piece_on(Square::new(pf as u8, pawn_rank as u8))
                        == Some(Piece::new(!color, PieceKind::Pawn))
            })
        };
        for sq in Square::ALL {
            let Some(piece) = board.piece_on(sq).filter(|p| p.color == color) else {
                continue;
            };
            if !matches!(
                piece.kind,
                PieceKind::Knight | PieceKind::Bishop | PieceKind::Rook | PieceKind::Queen
            ) {
                continue;
            }
            let count = reference_attacks(board, sq)
                .into_iter()
                .filter(|&t| board.piece_on(t).is_none_or(|p| p.color != color))
                .filter(|&t| !enemy_pawn_attacks(t.file() as i32, t.rank() as i32))
                .count();
            let name = ["pawn", "knight", "bishop", "rook", "queen", "king"][piece.kind.index()];
            *values
                .entry(format!("mobility_{name}_{count}"))
                .or_default() += sign;
        }
    }
    values.into_iter().filter(|&(_, v)| v != 0).collect()
}

fn mobility(board: &Board) -> Vec<(String, i32)> {
    let mut values = vec![0; FEATURE_COUNT];
    for_each_feature(board, |index, value| values[index] += value);
    let mut found: Vec<_> = (0..FEATURE_COUNT)
        .filter(|&i| values[i] != 0 && FEATURE_NAMES[i].starts_with("mobility_"))
        .map(|i| (FEATURE_NAMES[i].clone(), values[i]))
        .collect();
    found.sort();
    found
}

#[test]
fn mobility_matches_reference() {
    let mut state = 0x5EED_u64;
    let mut next = move || {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        state.wrapping_mul(0x2545_F491_4F6C_DD1D)
    };
    let mut checked = 0;
    for fen in POSITIONS {
        let mut board = Board::from_fen(fen).unwrap();
        for _ in 0..120 {
            assert_eq!(
                mobility(&board),
                reference_mobility(&board),
                "{}",
                board.fen()
            );
            checked += 1;
            let moves = board.legal_moves();
            if moves.is_empty() {
                break;
            }
            board.make_move(moves[next() as usize % moves.len()]);
        }
    }
    assert!(checked > 500, "{checked}");
}

/// Squares attacked by the piece on `sq`, found by walking rays; blockers are included.
fn reference_attacks(board: &Board, sq: Square) -> Vec<Square> {
    let kind = board.piece_on(sq).unwrap().kind;
    let (steps, slides): (&[(i32, i32)], bool) = match kind {
        PieceKind::Knight => (
            &[
                (1, 2),
                (2, 1),
                (2, -1),
                (1, -2),
                (-1, -2),
                (-2, -1),
                (-2, 1),
                (-1, 2),
            ],
            false,
        ),
        PieceKind::Bishop => (&[(1, 1), (1, -1), (-1, 1), (-1, -1)], true),
        PieceKind::Rook => (&[(0, 1), (0, -1), (1, 0), (-1, 0)], true),
        PieceKind::Queen => (
            &[
                (1, 1),
                (1, -1),
                (-1, 1),
                (-1, -1),
                (0, 1),
                (0, -1),
                (1, 0),
                (-1, 0),
            ],
            true,
        ),
        _ => return Vec::new(),
    };
    let mut attacked = Vec::new();
    for &(df, dr) in steps {
        let (mut f, mut r) = (sq.file() as i32 + df, sq.rank() as i32 + dr);
        while (0..8).contains(&f) && (0..8).contains(&r) {
            let target = Square::new(f as u8, r as u8);
            attacked.push(target);
            if board.piece_on(target).is_some() || !slides {
                break;
            }
            (f, r) = (f + df, r + dr);
        }
    }
    attacked
}

/// King safety features recomputed square by square.
fn reference_king_safety(board: &Board) -> Vec<(String, i32)> {
    let mut values = std::collections::BTreeMap::<String, i32>::new();
    let names = ["pawn", "knight", "bishop", "rook", "queen", "king"];
    for (color, sign) in [(Color::White, 1), (Color::Black, -1)] {
        let mut add = |name: String, value: i32| *values.entry(name).or_default() += sign * value;
        let king_of = |c: Color| {
            Square::ALL
                .into_iter()
                .find(|&sq| board.piece_on(sq) == Some(Piece::new(c, PieceKind::King)))
                .unwrap()
        };
        let enemy_king = king_of(!color);
        let in_zone = |sq: Square| {
            (sq.file() as i32 - enemy_king.file() as i32).abs() <= 1
                && (sq.rank() as i32 - enemy_king.rank() as i32).abs() <= 1
        };
        let mut attackers = 0;
        for sq in Square::ALL {
            let Some(piece) = board.piece_on(sq).filter(|p| p.color == color) else {
                continue;
            };
            let hits = reference_attacks(board, sq)
                .into_iter()
                .filter(|&t| in_zone(t))
                .count() as i32;
            if hits > 0 {
                add(format!("king_attack_{}", names[piece.kind.index()]), hits);
                attackers += 1;
            }
        }
        add(format!("king_attackers_{}", attackers.min(4)), 1);

        let king = king_of(color);
        let forward = if color == Color::White { 1 } else { -1 };
        let own_pawn = |f: i32, r: i32| {
            (0..8).contains(&r)
                && board.piece_on(Square::new(f as u8, r as u8))
                    == Some(Piece::new(color, PieceKind::Pawn))
        };
        let files: Vec<i32> = (-1..=1)
            .map(|d| king.file() as i32 + d)
            .filter(|f| (0..8).contains(f))
            .collect();
        for (steps, name) in [(1, "shield_near"), (2, "shield_far")] {
            let rank = king.rank() as i32 + forward * steps;
            add(
                name.into(),
                files.iter().filter(|&&f| own_pawn(f, rank)).count() as i32,
            );
        }
        let open = files
            .iter()
            .filter(|&&f| !(0..8).any(|r| own_pawn(f, r)))
            .count() as i32;
        add("king_open_files".into(), open);
    }
    values.into_iter().filter(|&(_, v)| v != 0).collect()
}

fn king_safety(board: &Board) -> Vec<(String, i32)> {
    let mut values = vec![0; FEATURE_COUNT];
    for_each_feature(board, |index, value| values[index] += value);
    let group = |name: &str| {
        name.starts_with("king_attack") || name.starts_with("shield_") || name == "king_open_files"
    };
    let mut found: Vec<_> = (0..FEATURE_COUNT)
        .filter(|&i| values[i] != 0 && group(&FEATURE_NAMES[i]))
        .map(|i| (FEATURE_NAMES[i].clone(), values[i]))
        .collect();
    found.sort();
    found
}

#[test]
fn king_safety_matches_reference() {
    let mut state = 0xC0FFEE_u64;
    let mut next = move || {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        state.wrapping_mul(0x2545_F491_4F6C_DD1D)
    };
    let mut checked = 0;
    for fen in POSITIONS {
        let mut board = Board::from_fen(fen).unwrap();
        for _ in 0..120 {
            assert_eq!(
                king_safety(&board),
                reference_king_safety(&board),
                "{}",
                board.fen()
            );
            checked += 1;
            let moves = board.legal_moves();
            if moves.is_empty() {
                break;
            }
            board.make_move(moves[next() as usize % moves.len()]);
        }
    }
    assert!(checked > 500, "{checked}");
}

#[test]
fn explain_scales_groups_by_style() {
    let board = Board::from_fen("4k3/8/8/8/8/8/8/3QK3 b - - 0 1").unwrap();
    let mut style = Style::default();
    style.multipliers[0] = 50;
    let text = explain(&board, &style);
    let material = text.lines().find(|l| l.starts_with("material")).unwrap();
    let fields: Vec<_> = material.split_whitespace().collect();
    let tuned: f64 = fields[1].parse().unwrap();
    assert_eq!(fields[2], "50%");
    let scaled: f64 = fields[3].parse().unwrap();
    assert!(
        (scaled - tuned / 2.0).abs() <= 0.01,
        "{scaled} is half of {tuned} up to rounding"
    );
    let stm = evaluate_styled(&board, &style);
    assert!(text.ends_with(&format!(
        "total: {} cp for white, {stm} cp for the side to move",
        -stm
    )));
}

#[test]
fn default_style_is_the_plain_evaluation() {
    assert_eq!(PRESETS[0].style, Style::default());
    for fen in POSITIONS {
        let board = Board::from_fen(fen).unwrap();
        assert_eq!(
            evaluate_styled(&board, &Style::default()),
            evaluate(&board),
            "{fen}"
        );
    }
}

#[test]
fn style_scales_groups() {
    let board = Board::from_fen(POSITIONS[1]).unwrap();
    let only = |group: usize, percent: i32| {
        let mut style = Style {
            multipliers: [0; 6],
            contempt: 0,
        };
        style.multipliers[group] = percent;
        evaluate_styled(&board, &style)
    };
    let mut total = 0;
    for (group, name) in GROUPS.iter().enumerate() {
        assert_eq!(only(group, 0), 0);
        assert!(
            (only(group, 200) - 2 * only(group, 100)).abs() <= 1,
            "{name}"
        );
        total += only(group, 100);
    }
    assert!(
        (total - evaluate(&board)).abs() <= GROUPS.len() as i32,
        "groups add up"
    );
}

#[test]
fn presets_format() {
    assert!(PRESETS.len() >= 2);
    let header = "# name m p pa mo ka kd contempt\n";
    assert_eq!(
        parse_presets(&format!("{header}a 1 2 3 4 5 6 -7\n")).unwrap()[0]
            .style
            .contempt,
        -7
    );
    for (bad, error) in [
        ("", "no presets"),
        ("a 1 2 3 4 5 6\n", "expected"),
        ("a 1 2 3 4 5 301 0\n", "not in"),
        ("a 1 2 3 4 5 6 201\n", "not in"),
        ("a 1 2 3 4 5 6 0\na 1 2 3 4 5 6 0\n", "duplicate"),
    ] {
        let message = parse_presets(bad).err().unwrap();
        assert!(message.contains(error), "{message:?} for {bad:?}");
    }
}

/// Bishop pair and rook file features recomputed square by square.
fn reference_pieces(board: &Board) -> Vec<(String, i32)> {
    let mut values = std::collections::BTreeMap::<String, i32>::new();
    let count = |piece: Piece| {
        Square::ALL
            .into_iter()
            .filter(|&sq| board.piece_on(sq) == Some(piece))
            .count()
    };
    for (color, sign) in [(Color::White, 1), (Color::Black, -1)] {
        if count(Piece::new(color, PieceKind::Bishop)) >= 2 {
            *values.entry("bishop_pair".into()).or_default() += sign;
        }
        for sq in Square::ALL {
            if board.piece_on(sq) != Some(Piece::new(color, PieceKind::Rook)) {
                continue;
            }
            let on_file = |c: Color| {
                (0..8).any(|rank| {
                    board.piece_on(Square::new(sq.file(), rank))
                        == Some(Piece::new(c, PieceKind::Pawn))
                })
            };
            let name = match (on_file(color), on_file(!color)) {
                (false, false) => "rook_open_file",
                (false, true) => "rook_semi_open_file",
                _ => continue,
            };
            *values.entry(name.into()).or_default() += sign;
        }
    }
    values.into_iter().filter(|&(_, v)| v != 0).collect()
}

/// Compares the features `names` with `reference` along random games from `POSITIONS`.
fn assert_matches_reference(names: &[&str], reference: fn(&Board) -> Vec<(String, i32)>) {
    let mut state = 0xB15B_u64;
    let mut next = move || {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        state.wrapping_mul(0x2545_F491_4F6C_DD1D)
    };
    let mut checked = 0;
    for fen in POSITIONS {
        let mut board = Board::from_fen(fen).unwrap();
        for _ in 0..120 {
            let mut values = vec![0; FEATURE_COUNT];
            for_each_feature(&board, |index, value| values[index] += value);
            let mut found: Vec<_> = (0..FEATURE_COUNT)
                .filter(|&i| values[i] != 0 && names.contains(&FEATURE_NAMES[i].as_str()))
                .map(|i| (FEATURE_NAMES[i].clone(), values[i]))
                .collect();
            found.sort();
            assert_eq!(found, reference(&board), "{}", board.fen());
            checked += 1;
            let moves = board.legal_moves();
            if moves.is_empty() {
                break;
            }
            board.make_move(moves[next() as usize % moves.len()]);
        }
    }
    assert!(checked > 500, "{checked}");
}

#[test]
fn piece_features_match_reference() {
    assert_matches_reference(
        &["bishop_pair", "rook_open_file", "rook_semi_open_file"],
        reference_pieces,
    );
}

/// Whether a piece of `color` attacks `target`, checked piece by piece.
fn reference_attacked_by(board: &Board, color: Color, target: Square, kinds: &[PieceKind]) -> bool {
    Square::ALL.into_iter().any(|sq| {
        let Some(piece) = board
            .piece_on(sq)
            .filter(|p| p.color == color && kinds.contains(&p.kind))
        else {
            return false;
        };
        let (df, dr) = (
            target.file() as i32 - sq.file() as i32,
            target.rank() as i32 - sq.rank() as i32,
        );
        match piece.kind {
            PieceKind::Pawn => df.abs() == 1 && dr == if color == Color::White { 1 } else { -1 },
            PieceKind::King => df.abs() <= 1 && dr.abs() <= 1 && (df, dr) != (0, 0),
            _ => reference_attacks(board, sq).contains(&target),
        }
    })
}

/// Threat features recomputed square by square.
fn reference_threats(board: &Board) -> Vec<(String, i32)> {
    use PieceKind::*;
    let mut values = std::collections::BTreeMap::<String, i32>::new();
    for (color, sign) in [(Color::White, 1), (Color::Black, -1)] {
        for sq in Square::ALL {
            let Some(piece) = board.piece_on(sq).filter(|p| p.color == !color) else {
                continue;
            };
            if matches!(piece.kind, Pawn | King) {
                continue;
            }
            let by = |kinds: &[PieceKind]| reference_attacked_by(board, color, sq, kinds);
            let mut hit = |name: &str| *values.entry(name.into()).or_default() += sign;
            if by(&[Pawn]) {
                hit("threat_by_pawn");
            }
            if matches!(piece.kind, Rook | Queen) && by(&[Knight, Bishop]) {
                hit("threat_by_minor");
            }
            if piece.kind == Queen && by(&[Rook]) {
                hit("threat_by_rook");
            }
            if by(&PieceKind::ALL) && !reference_attacked_by(board, !color, sq, &PieceKind::ALL) {
                hit("hanging");
            }
        }
    }
    values.into_iter().filter(|&(_, v)| v != 0).collect()
}

#[test]
fn threat_features_match_reference() {
    assert_matches_reference(
        &[
            "threat_by_pawn",
            "threat_by_minor",
            "threat_by_rook",
            "hanging",
        ],
        reference_threats,
    );
}

#[test]
fn threat_examples() {
    let board = Board::from_fen("8/5k2/2qpr3/3P4/8/5N2/8/R3K3 w - - 0 1").unwrap();
    assert_eq!(
        reference_threats(&board),
        [("hanging", 1), ("threat_by_pawn", 2)].map(|(n, v)| (n.to_string(), v)),
        "the pawn forks the queen and the rook; only the rook is defended"
    );
}

#[test]
fn feature_groups() {
    let group_of =
        |name: &str| GROUPS[group(FEATURE_NAMES.iter().position(|n| n == name).unwrap())];
    assert_eq!(group_of("queen"), "material");
    assert_eq!(group_of("bishop_pair"), "material");
    assert_eq!(group_of("knight_e4"), "positional");
    assert_eq!(group_of("passed_7"), "pawns");
    assert_eq!(group_of("mobility_rook_3"), "mobility");
    assert_eq!(group_of("rook_open_file"), "mobility");
    assert_eq!(group_of("king_attackers_4"), "king_attack");
    assert_eq!(group_of("king_open_files"), "king_defense");
    assert_eq!(group_of("hanging"), "mobility");
}
