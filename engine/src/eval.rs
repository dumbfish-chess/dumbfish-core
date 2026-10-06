//! Evaluation: a linear function of position features. Each feature has three weights, for the
//! opening, middlegame and endgame, blended by the game phase. The weights live in
//! `engine/weights.txt`, one line per feature: `name opening middlegame endgame`.

use std::fmt::Write as _;
use std::sync::LazyLock;

use dumbfish_core::{Board, Color, PieceKind};

/// Phase of a position with all pieces: knights and bishops count 1, rooks 2, queens 4.
pub const MAX_PHASE: i32 = 24;

pub struct Feature {
    pub name: &'static str,
    /// Value from white's point of view.
    pub value: fn(&Board) -> i32,
}

pub static FEATURES: [Feature; 5] = [
    Feature {
        name: "pawn",
        value: material::<0>,
    },
    Feature {
        name: "knight",
        value: material::<1>,
    },
    Feature {
        name: "bishop",
        value: material::<2>,
    },
    Feature {
        name: "rook",
        value: material::<3>,
    },
    Feature {
        name: "queen",
        value: material::<4>,
    },
];

/// Piece count difference for `PieceKind::ALL[KIND]`.
fn material<const KIND: usize>(board: &Board) -> i32 {
    let kind = PieceKind::ALL[KIND];
    board.pieces(Color::White, kind).count() as i32
        - board.pieces(Color::Black, kind).count() as i32
}

/// `[opening, middlegame, endgame]` weights, indexed like `FEATURES`.
pub type Weights = Vec<[i32; 3]>;

static WEIGHTS: LazyLock<Weights> = LazyLock::new(|| {
    parse_weights(include_str!("../weights.txt")).expect("valid engine/weights.txt")
});

/// Parses `name opening middlegame endgame` lines; blank lines and `#` comments are skipped.
/// Every feature must appear exactly once.
pub fn parse_weights(text: &str) -> Result<Weights, String> {
    let mut weights = vec![None; FEATURES.len()];
    for (number, line) in (1..).zip(text.lines()) {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = line.split_whitespace().collect();
        let &[name, opening, middlegame, endgame] = &fields[..] else {
            return Err(format!(
                "line {number}: expected `name opening middlegame endgame`"
            ));
        };
        let index = FEATURES
            .iter()
            .position(|f| f.name == name)
            .ok_or_else(|| format!("line {number}: unknown feature {name:?}"))?;
        if weights[index].is_some() {
            return Err(format!("line {number}: duplicate feature {name:?}"));
        }
        let parse = |s: &str| {
            s.parse::<i32>()
                .map_err(|_| format!("line {number}: invalid weight {s:?}"))
        };
        weights[index] = Some([parse(opening)?, parse(middlegame)?, parse(endgame)?]);
    }
    weights
        .into_iter()
        .zip(&FEATURES)
        .map(|(w, f)| w.ok_or_else(|| format!("missing feature {:?}", f.name)))
        .collect()
}

/// Canonical text of a weights file; `parse_weights` reads it back unchanged.
pub fn format_weights(weights: &Weights) -> String {
    let mut out = format!(
        "{:<10} {:>8} {:>11} {:>8}\n",
        "# feature", "opening", "middlegame", "endgame"
    );
    for (feature, [opening, middlegame, endgame]) in FEATURES.iter().zip(weights) {
        writeln!(
            out,
            "{:<10} {opening:>8} {middlegame:>11} {endgame:>8}",
            feature.name
        )
        .unwrap();
    }
    out
}

/// 0 (kings and pawns only) ..= `MAX_PHASE`.
pub fn phase(board: &Board) -> i32 {
    use PieceKind::*;
    let count = |kind| board.kind_bb(kind).count() as i32;
    (count(Knight) + count(Bishop) + 2 * count(Rook) + 4 * count(Queen)).min(MAX_PHASE)
}

/// `[opening, middlegame, endgame]` shares of `MAX_PHASE`, summing to `MAX_PHASE`. Pure opening
/// at `MAX_PHASE`, pure middlegame at half of it, pure endgame at 0, linear in between.
pub fn blend(phase: i32) -> [i32; 3] {
    let twice = 2 * phase - MAX_PHASE;
    [twice.max(0), MAX_PHASE - twice.abs(), (-twice).max(0)]
}

/// Centipawns from the side to move's point of view.
pub fn evaluate(board: &Board) -> i32 {
    let blend = blend(phase(board));
    let white: i32 = FEATURES
        .iter()
        .zip(WEIGHTS.iter())
        .map(|(feature, weights)| (feature.value)(board) * dot(blend, weights))
        .sum::<i32>()
        / MAX_PHASE;
    match board.side_to_move() {
        Color::White => white,
        Color::Black => -white,
    }
}

/// Table for the `eval` command: every feature's value, its contribution with the weights of each
/// phase, and its actual contribution in this position (white's point of view, centipawns).
pub fn explain(board: &Board) -> String {
    let phase = phase(board);
    let blend = blend(phase);
    let mut out = format!(
        "{:<12}{:>8}{:>10}{:>12}{:>10}{:>10}\n",
        "feature", "value", "opening", "middlegame", "endgame", "blended"
    );
    for (feature, weights) in FEATURES.iter().zip(WEIGHTS.iter()) {
        let value = (feature.value)(board);
        let [opening, middlegame, endgame] = weights.map(|w| value * w);
        let blended = f64::from(value * dot(blend, weights)) / f64::from(MAX_PHASE);
        writeln!(
            out,
            "{:<12}{value:>8}{opening:>10}{middlegame:>12}{endgame:>10}{blended:>10.2}",
            feature.name
        )
        .unwrap();
    }
    let [opening, middlegame, endgame] = blend;
    writeln!(
        out,
        "phase {phase}/{MAX_PHASE}: opening {opening}/{MAX_PHASE}, middlegame {middlegame}/{MAX_PHASE}, endgame {endgame}/{MAX_PHASE}"
    )
    .unwrap();
    let stm = evaluate(board);
    let white = match board.side_to_move() {
        Color::White => stm,
        Color::Black => -stm,
    };
    write!(
        out,
        "total: {white} cp for white, {stm} cp for the side to move"
    )
    .unwrap();
    out
}

fn dot(a: [i32; 3], b: &[i32; 3]) -> i32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
