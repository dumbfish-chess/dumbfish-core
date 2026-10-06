//! Evaluation: a linear function of position features. Each feature has three weights, for the
//! opening, middlegame and endgame, blended by the game phase. The weights live in
//! `engine/weights.txt`, one line per feature: `name opening middlegame endgame`.
//!
//! Features, from white's point of view:
//! - `pawn` .. `queen`: piece count difference;
//! - `<piece>_<square>` (piece-square tables): +1 for a white piece on the square, -1 for a
//!   black piece on the square mirrored vertically;
//! - pawn structure, counted per pawn: `doubled` (an own pawn ahead on the file), `isolated`
//!   (no own pawns on the adjacent files), `backward` (not isolated, every own pawn on the
//!   adjacent files is ahead, and an enemy pawn attacks the square in front), `passed_<rank>`
//!   (no enemy pawns ahead on the file or the adjacent files and no own pawn ahead on the file;
//!   the rank is counted from the pawn's own side, 2 ..= 7).

use std::fmt::Write as _;
use std::sync::LazyLock;

use dumbfish_core::{Bitboard, Board, Color, PieceKind, Square, attacks};

/// Phase of a position with all pieces: knights and bishops count 1, rooks 2, queens 4.
pub const MAX_PHASE: i32 = 24;

const PIECE_NAMES: [&str; 6] = ["pawn", "knight", "bishop", "rook", "queen", "king"];
const MATERIAL: usize = 0;
const PIECE_SQUARE: usize = MATERIAL + 5;
const DOUBLED: usize = PIECE_SQUARE + 6 * 64;
const ISOLATED: usize = DOUBLED + 1;
const BACKWARD: usize = ISOLATED + 1;
/// `passed_2` ..= `passed_7`.
const PASSED: usize = BACKWARD + 1;
pub const FEATURE_COUNT: usize = PASSED + 6;

/// Feature names by index.
pub static FEATURE_NAMES: LazyLock<Vec<String>> = LazyLock::new(|| {
    let mut names: Vec<_> = PIECE_NAMES[..5].iter().map(|n| n.to_string()).collect();
    for piece in PIECE_NAMES {
        names.extend(Square::ALL.iter().map(|sq| format!("{piece}_{sq}")));
    }
    names.extend(["doubled", "isolated", "backward"].map(String::from));
    names.extend((2..=7).map(|rank| format!("passed_{rank}")));
    names
});

/// Calls `f(index, value)` for the features of `board` that may be non-zero. An index can come
/// up more than once; its values add up.
pub fn for_each_feature(board: &Board, mut f: impl FnMut(usize, i32)) {
    for kind in &PieceKind::ALL[..5] {
        let value = board.pieces(Color::White, *kind).count() as i32
            - board.pieces(Color::Black, *kind).count() as i32;
        if value != 0 {
            f(MATERIAL + kind.index(), value);
        }
    }
    for (color, sign, mirror) in [(Color::White, 1, 0), (Color::Black, -1, 56)] {
        for kind in PieceKind::ALL {
            for sq in board.pieces(color, kind) {
                f(
                    PIECE_SQUARE + 64 * kind.index() + (sq.index() ^ mirror),
                    sign,
                );
            }
        }
        pawn_structure(board, color, |index| f(index, sign));
    }
}

/// Calls `f(index)` once per pawn of `color` and per pawn structure feature it has.
fn pawn_structure(board: &Board, color: Color, mut f: impl FnMut(usize)) {
    let ours = board.pieces(color, PieceKind::Pawn);
    let theirs = board.pieces(!color, PieceKind::Pawn);
    for sq in ours {
        let (file, rank) = (sq.file(), sq.rank());
        let relative_rank = match color {
            Color::White => rank,
            Color::Black => 7 - rank,
        };
        let ahead = Bitboard(match color {
            Color::White => (!0u64).checked_shl(8 * u32::from(rank + 1)).unwrap_or(0),
            Color::Black => (1u64 << (8 * rank)) - 1,
        });
        let own_file = Bitboard::file(file);
        let adjacent = own_file.east() | own_file.west();

        let doubled = !(ours & own_file & ahead).is_empty();
        if doubled {
            f(DOUBLED);
        }
        if (ours & adjacent).is_empty() {
            f(ISOLATED);
        } else if (ours & adjacent & !ahead).is_empty() {
            let stop = match color {
                Color::White => Square::new(file, rank + 1),
                Color::Black => Square::new(file, rank - 1),
            };
            if !(attacks::pawn(color, stop) & theirs).is_empty() {
                f(BACKWARD);
            }
        }
        if !doubled && (theirs & (own_file | adjacent) & ahead).is_empty() {
            f(PASSED + usize::from(relative_rank) - 1);
        }
    }
}

/// `[opening, middlegame, endgame]` weights, indexed like `FEATURE_NAMES`.
pub type Weights = Vec<[i32; 3]>;

static WEIGHTS: LazyLock<Weights> = LazyLock::new(|| {
    parse_weights(include_str!("../weights.txt")).expect("valid engine/weights.txt")
});

/// Parses `name opening middlegame endgame` lines; blank lines and `#` comments are skipped.
/// Every feature must appear exactly once.
pub fn parse_weights(text: &str) -> Result<Weights, String> {
    let mut weights = vec![None; FEATURE_COUNT];
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
        let index = FEATURE_NAMES
            .iter()
            .position(|n| n == name)
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
        .zip(FEATURE_NAMES.iter())
        .map(|(w, name)| w.ok_or_else(|| format!("missing feature {name:?}")))
        .collect()
}

/// Canonical text of a weights file; `parse_weights` reads it back unchanged.
pub fn format_weights(weights: &Weights) -> String {
    let mut out = format!(
        "{:<10} {:>8} {:>11} {:>8}\n",
        "# feature", "opening", "middlegame", "endgame"
    );
    for (name, [opening, middlegame, endgame]) in FEATURE_NAMES.iter().zip(weights) {
        writeln!(out, "{name:<10} {opening:>8} {middlegame:>11} {endgame:>8}").unwrap();
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
    evaluate_with(board, &WEIGHTS)
}

/// `evaluate` with the given weights instead of the shipped ones.
pub fn evaluate_with(board: &Board, weights: &Weights) -> i32 {
    let blend = blend(phase(board));
    let mut white = 0;
    for_each_feature(board, |index, value| {
        white += value * dot(blend, &weights[index])
    });
    white /= MAX_PHASE;
    match board.side_to_move() {
        Color::White => white,
        Color::Black => -white,
    }
}

/// Table for the `eval` command: every non-zero feature's value, its contribution with the
/// weights of each phase, and its actual contribution in this position (white's point of view,
/// centipawns).
pub fn explain(board: &Board) -> String {
    let phase = phase(board);
    let blend = blend(phase);
    let mut values = vec![0; FEATURE_COUNT];
    for_each_feature(board, |index, value| values[index] += value);

    let mut out = format!(
        "{:<12}{:>8}{:>10}{:>12}{:>10}{:>10}\n",
        "feature", "value", "opening", "middlegame", "endgame", "blended"
    );
    for (index, &value) in values.iter().enumerate().filter(|&(_, &v)| v != 0) {
        let weights = &WEIGHTS[index];
        let [opening, middlegame, endgame] = weights.map(|w| value * w);
        let blended = f64::from(value * dot(blend, weights)) / f64::from(MAX_PHASE);
        writeln!(
            out,
            "{:<12}{value:>8}{opening:>10}{middlegame:>12}{endgame:>10}{blended:>10.2}",
            FEATURE_NAMES[index]
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
