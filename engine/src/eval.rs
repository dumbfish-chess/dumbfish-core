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
//!   the rank is counted from the pawn's own side, 2 ..= 7);
//! - `mobility_<piece>_<n>`: one per knight, bishop, rook and queen that can go to `n` squares
//!   not occupied by its own pieces and not attacked by enemy pawns;
//! - king safety, scored for the attacker: `king_attack_<piece>` (squares of the enemy king zone,
//!   the king and its neighbours, attacked by each of our knights, bishops, rooks and queens),
//!   `king_attackers_<n>` (one per side: how many of those pieces attack the zone, 4 and more
//!   counted as 4); and for the defender: `shield_near` and `shield_far` (own pawns one and two
//!   squares in front of the king on its file and the adjacent ones), `king_open_files` (those
//!   files without own pawns);
//! - `bishop_pair` (two bishops or more), `rook_open_file` and `rook_semi_open_file` (per rook on
//!   a file without pawns, or without own pawns only);
//! - threats to enemy pieces other than pawns: `threat_by_pawn` (attacked by a pawn),
//!   `threat_by_minor` (rooks and queens attacked by a knight or bishop), `threat_by_rook`
//!   (queens attacked by a rook), `hanging` (attacked and not defended);
//! - passed pawn details, per passed pawn: `passed_supported` (defended by an own pawn),
//!   `passed_own_king` and `passed_enemy_king` (distance in king moves from the king to the
//!   square in front of the pawn, times the pawn's rank counted from its side minus 2);
//! - `space`: squares on files c to f, ranks 2 to 4 counted from the own side, without own pawns
//!   and not attacked by enemy pawns.

use std::fmt::Write as _;
use std::sync::LazyLock;

use dumbfish_core::{Bitboard, Board, Color, PieceKind, Square, attacks};

use crate::style::{GROUPS, Style};

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
/// Pieces with mobility features and their largest number of reachable squares.
const MOBILITY_PIECES: [(PieceKind, usize); 4] = [
    (PieceKind::Knight, 8),
    (PieceKind::Bishop, 13),
    (PieceKind::Rook, 14),
    (PieceKind::Queen, 27),
];
const MOBILITY: usize = PASSED + 6;
/// `king_attack_knight` ..= `king_attack_queen`.
const KING_ATTACK: usize = MOBILITY + 4 + 8 + 13 + 14 + 27;
/// `king_attackers_0` ..= `king_attackers_4`.
const KING_ATTACKERS: usize = KING_ATTACK + 4;
const SHIELD_NEAR: usize = KING_ATTACKERS + 5;
const SHIELD_FAR: usize = SHIELD_NEAR + 1;
const KING_OPEN_FILES: usize = SHIELD_FAR + 1;
const BISHOP_PAIR: usize = KING_OPEN_FILES + 1;
const ROOK_OPEN_FILE: usize = BISHOP_PAIR + 1;
const ROOK_SEMI_OPEN_FILE: usize = ROOK_OPEN_FILE + 1;
const THREAT_BY_PAWN: usize = ROOK_SEMI_OPEN_FILE + 1;
const THREAT_BY_MINOR: usize = THREAT_BY_PAWN + 1;
const THREAT_BY_ROOK: usize = THREAT_BY_MINOR + 1;
const HANGING: usize = THREAT_BY_ROOK + 1;
const PASSED_SUPPORTED: usize = HANGING + 1;
const PASSED_OWN_KING: usize = PASSED_SUPPORTED + 1;
const PASSED_ENEMY_KING: usize = PASSED_OWN_KING + 1;
const SPACE: usize = PASSED_ENEMY_KING + 1;
pub const FEATURE_COUNT: usize = SPACE + 1;

/// Index in `style::GROUPS` of the group a feature belongs to.
pub fn group(index: usize) -> usize {
    match index {
        BISHOP_PAIR => 0,
        ROOK_OPEN_FILE | ROOK_SEMI_OPEN_FILE => 3,
        THREAT_BY_PAWN..=HANGING => 3,
        PASSED_SUPPORTED..=PASSED_ENEMY_KING => 2,
        SPACE => 1,
        i if i < PIECE_SQUARE => 0,
        i if i < DOUBLED => 1,
        i if i < MOBILITY => 2,
        i if i < KING_ATTACK => 3,
        i if i < SHIELD_NEAR => 4,
        _ => 5,
    }
}

/// `group` of every feature, by index.
static GROUP_OF: LazyLock<[u8; FEATURE_COUNT]> =
    LazyLock::new(|| std::array::from_fn(|index| group(index) as u8));

/// Feature names by index.
pub static FEATURE_NAMES: LazyLock<Vec<String>> = LazyLock::new(|| {
    let mut names: Vec<_> = PIECE_NAMES[..5].iter().map(|n| n.to_string()).collect();
    for piece in PIECE_NAMES {
        names.extend(Square::ALL.iter().map(|sq| format!("{piece}_{sq}")));
    }
    names.extend(["doubled", "isolated", "backward"].map(String::from));
    names.extend((2..=7).map(|rank| format!("passed_{rank}")));
    for (kind, most) in MOBILITY_PIECES {
        let piece = PIECE_NAMES[kind.index()];
        names.extend((0..=most).map(|n| format!("mobility_{piece}_{n}")));
    }
    names.extend(
        PIECE_NAMES[1..5]
            .iter()
            .map(|piece| format!("king_attack_{piece}")),
    );
    names.extend((0..=4).map(|n| format!("king_attackers_{n}")));
    names.extend(["shield_near", "shield_far", "king_open_files"].map(String::from));
    names.extend(["bishop_pair", "rook_open_file", "rook_semi_open_file"].map(String::from));
    names.extend(
        [
            "threat_by_pawn",
            "threat_by_minor",
            "threat_by_rook",
            "hanging",
        ]
        .map(String::from),
    );
    names.extend(["passed_supported", "passed_own_king", "passed_enemy_king"].map(String::from));
    names.push("space".into());
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
    let sides = [(Color::White, 1, 0), (Color::Black, -1, 56)];
    let mut maps = [AttackMaps::default(); 2];
    for (color, sign, mirror) in sides {
        for kind in PieceKind::ALL {
            for sq in board.pieces(color, kind) {
                f(
                    PIECE_SQUARE + 64 * kind.index() + (sq.index() ^ mirror),
                    sign,
                );
            }
        }
        pawn_structure(board, color, |index, value| f(index, sign * value));
        maps[color.index()] = activity(board, color, |index, value| f(index, sign * value));
        king_shelter(board, color, |index, value| f(index, sign * value));
        pieces(board, color, |index| f(index, sign));
    }
    for (color, sign, _) in sides {
        let (ours, theirs) = (&maps[color.index()], &maps[(!color).index()]);
        threats(board, color, ours, theirs, |index, value| {
            f(index, sign * value)
        });
        let space = space(board, color, theirs);
        if space > 0 {
            f(SPACE, sign * space);
        }
    }
}

/// The squares each kind of piece of one color attacks.
#[derive(Clone, Copy, Default)]
struct AttackMaps([Bitboard; 6]);

impl AttackMaps {
    fn of(&self, kind: PieceKind) -> Bitboard {
        self.0[kind.index()]
    }

    fn all(&self) -> Bitboard {
        self.0.iter().fold(Bitboard::EMPTY, |acc, &b| acc | b)
    }
}

/// Squares the pawns of `color` attack.
fn pawn_attacks(board: &Board, color: Color) -> Bitboard {
    let pawns = board.pieces(color, PieceKind::Pawn);
    match color {
        Color::White => pawns.north_east() | pawns.north_west(),
        Color::Black => pawns.south_east() | pawns.south_west(),
    }
}

/// Squares on files c to f, ranks 2 to 4 counted from the side of `color`, without own pawns and
/// not attacked by enemy pawns.
fn space(board: &Board, color: Color, theirs: &AttackMaps) -> i32 {
    let files = Bitboard::file(2) | Bitboard::file(3) | Bitboard::file(4) | Bitboard::file(5);
    let ranks = match color {
        Color::White => Bitboard::rank(1) | Bitboard::rank(2) | Bitboard::rank(3),
        Color::Black => Bitboard::rank(4) | Bitboard::rank(5) | Bitboard::rank(6),
    };
    let safe = !board.pieces(color, PieceKind::Pawn) & !theirs.of(PieceKind::Pawn);
    (files & ranks & safe).count() as i32
}

/// Enemy pieces other than pawns that pieces of `color` threaten: attacked by a pawn, a rook
/// or queen attacked by a minor piece, a queen attacked by a rook, and any attacked while
/// undefended. `ours` and `theirs` are the attacks of `color` and of its opponent.
fn threats(
    board: &Board,
    color: Color,
    ours: &AttackMaps,
    theirs: &AttackMaps,
    mut f: impl FnMut(usize, i32),
) {
    use PieceKind::*;
    let them = !color;
    let majors = board.pieces(them, Rook) | board.pieces(them, Queen);
    let pieces = majors | board.pieces(them, Knight) | board.pieces(them, Bishop);
    for (index, targets) in [
        (THREAT_BY_PAWN, ours.of(Pawn) & pieces),
        (
            THREAT_BY_MINOR,
            (ours.of(Knight) | ours.of(Bishop)) & majors,
        ),
        (THREAT_BY_ROOK, ours.of(Rook) & board.pieces(them, Queen)),
        (HANGING, ours.all() & pieces & !theirs.all()),
    ] {
        if !targets.is_empty() {
            f(index, targets.count() as i32);
        }
    }
}

/// Calls `f(index)` for the bishop pair of `color` and once per rook of `color` on an open or
/// half-open file.
fn pieces(board: &Board, color: Color, mut f: impl FnMut(usize)) {
    if board.pieces(color, PieceKind::Bishop).count() >= 2 {
        f(BISHOP_PAIR);
    }
    let own_pawns = board.pieces(color, PieceKind::Pawn);
    let pawns = board.kind_bb(PieceKind::Pawn);
    for rook in board.pieces(color, PieceKind::Rook) {
        let file = Bitboard::file(rook.file());
        if (pawns & file).is_empty() {
            f(ROOK_OPEN_FILE);
        } else if (own_pawns & file).is_empty() {
            f(ROOK_SEMI_OPEN_FILE);
        }
    }
}

/// Mobility of the knights, bishops, rooks and queens of `color`, one feature per piece, and
/// their attacks on the enemy king zone; returns the squares each kind of piece of `color`
/// attacks.
fn activity(board: &Board, color: Color, mut f: impl FnMut(usize, i32)) -> AttackMaps {
    let available = !board.color_bb(color) & !pawn_attacks(board, !color);
    let enemy_king = board.king(!color);
    let zone = attacks::king(enemy_king) | enemy_king.bb();
    let occupied = board.occupied();
    let mut maps = AttackMaps::default();
    maps.0[PieceKind::Pawn.index()] = pawn_attacks(board, color);
    maps.0[PieceKind::King.index()] = attacks::king(board.king(color));
    let (mut offset, mut attackers) = (MOBILITY, 0);
    for (i, (kind, most)) in MOBILITY_PIECES.into_iter().enumerate() {
        for sq in board.pieces(color, kind) {
            let reachable = match kind {
                PieceKind::Knight => attacks::knight(sq),
                PieceKind::Bishop => attacks::bishop(sq, occupied),
                PieceKind::Rook => attacks::rook(sq, occupied),
                _ => attacks::queen(sq, occupied),
            };
            maps.0[kind.index()] |= reachable;
            f(offset + (reachable & available).count() as usize, 1);
            let hits = (reachable & zone).count() as i32;
            if hits > 0 {
                f(KING_ATTACK + i, hits);
                attackers += 1;
            }
        }
        offset += most + 1;
    }
    f(KING_ATTACKERS + attackers.min(4), 1);
    maps
}

/// The pawn cover of the king of `color`.
fn king_shelter(board: &Board, color: Color, mut f: impl FnMut(usize, i32)) {
    let king = board.king(color);
    let pawns = board.pieces(color, PieceKind::Pawn);
    let files = Bitboard::file(king.file());
    let files = files | files.east() | files.west();
    let rank_ahead = |steps: u8| match color {
        Color::White => king.rank().checked_add(steps).filter(|&r| r < 8),
        Color::Black => king.rank().checked_sub(steps),
    };
    for (steps, index) in [(1, SHIELD_NEAR), (2, SHIELD_FAR)] {
        if let Some(rank) = rank_ahead(steps) {
            let shield = (pawns & files & Bitboard::rank(rank)).count() as i32;
            if shield > 0 {
                f(index, shield);
            }
        }
    }
    let open = (0..8)
        .filter(|&file| !(files & Bitboard::file(file)).is_empty())
        .filter(|&file| (pawns & Bitboard::file(file)).is_empty())
        .count() as i32;
    if open > 0 {
        f(KING_OPEN_FILES, open);
    }
}

/// Distance between squares in king moves.
fn distance(a: Square, b: Square) -> i32 {
    i32::from(a.file().abs_diff(b.file()).max(a.rank().abs_diff(b.rank())))
}

/// `b` and every square ahead of it on its file, from the side of `color`.
fn fill_forward(mut b: Bitboard, color: Color) -> Bitboard {
    for shift in [8, 16, 32] {
        b.0 |= match color {
            Color::White => b.0 << shift,
            Color::Black => b.0 >> shift,
        };
    }
    b
}

/// The squares strictly ahead of `b` on its file, from the side of `color`.
fn front_span(b: Bitboard, color: Color) -> Bitboard {
    match color {
        Color::White => fill_forward(b.north(), color),
        Color::Black => fill_forward(b.south(), color),
    }
}

/// Calls `f(index, value)` for the pawn structure features of the pawns of `color`; for the
/// counts of doubled, isolated and backward pawns, and once per passed pawn and feature.
fn pawn_structure(board: &Board, color: Color, mut f: impl FnMut(usize, i32)) {
    let ours = board.pieces(color, PieceKind::Pawn);
    let theirs = board.pieces(!color, PieceKind::Pawn);
    let files = fill_forward(ours, color) | fill_forward(ours, !color);
    let neighbours = files.east() | files.west();
    let beside = ours.east() | ours.west();
    let behind_stop = match color {
        Color::White => pawn_attacks(board, !color).south(),
        Color::Black => pawn_attacks(board, !color).north(),
    };

    let doubled = ours & front_span(ours, !color);
    let isolated = ours & !neighbours;
    let backward = ours & neighbours & !fill_forward(beside, color) & behind_stop;
    let blocked = front_span(theirs | theirs.east() | theirs.west(), !color);
    for (index, pawns) in [
        (DOUBLED, doubled),
        (ISOLATED, isolated),
        (BACKWARD, backward),
    ] {
        if !pawns.is_empty() {
            f(index, pawns.count() as i32);
        }
    }

    for sq in ours & !doubled & !blocked {
        let relative_rank = match color {
            Color::White => sq.rank(),
            Color::Black => 7 - sq.rank(),
        };
        f(PASSED + usize::from(relative_rank) - 1, 1);
        if !(attacks::pawn(!color, sq) & ours).is_empty() {
            f(PASSED_SUPPORTED, 1);
        }
        let scale = i32::from(relative_rank) - 1;
        if scale > 0 {
            let stop = match color {
                Color::White => Square::new(sq.file(), sq.rank() + 1),
                Color::Black => Square::new(sq.file(), sq.rank() - 1),
            };
            f(PASSED_OWN_KING, scale * distance(board.king(color), stop));
            f(
                PASSED_ENEMY_KING,
                scale * distance(board.king(!color), stop),
            );
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
    evaluate_full(board, &WEIGHTS, &Style::default())
}

/// `evaluate` with the given weights instead of the shipped ones.
pub fn evaluate_with(board: &Board, weights: &Weights) -> i32 {
    evaluate_full(board, weights, &Style::default())
}

/// `evaluate` with each feature group scaled by the style.
pub fn evaluate_styled(board: &Board, style: &Style) -> i32 {
    evaluate_full(board, &WEIGHTS, style)
}

fn evaluate_full(board: &Board, weights: &Weights, style: &Style) -> i32 {
    let white = group_sums(board, weights)
        .iter()
        .zip(style.multipliers)
        .map(|(sum, percent)| sum * percent)
        .sum::<i32>()
        / (100 * MAX_PHASE);
    match board.side_to_move() {
        Color::White => white,
        Color::Black => -white,
    }
}

/// Per group, white's point of view, in centipawns times `MAX_PHASE`.
fn group_sums(board: &Board, weights: &Weights) -> [i32; GROUPS.len()] {
    let blend = blend(phase(board));
    let mut sums = [0; GROUPS.len()];
    for_each_feature(board, |index, value| {
        sums[usize::from(GROUP_OF[index])] += value * dot(blend, &weights[index])
    });
    sums
}

/// Table for the `eval` command: every non-zero feature's value, its contribution with the
/// weights of each phase, and its actual contribution in this position; then the groups scaled
/// by the style (white's point of view, centipawns).
pub fn explain(board: &Board, style: &Style) -> String {
    let phase = phase(board);
    let blend = blend(phase);
    let mut values = vec![0; FEATURE_COUNT];
    for_each_feature(board, |index, value| values[index] += value);

    let mut out = format!(
        "{:<20}{:>8}{:>10}{:>12}{:>10}{:>10}\n",
        "feature", "value", "opening", "middlegame", "endgame", "blended"
    );
    for (index, &value) in values.iter().enumerate().filter(|&(_, &v)| v != 0) {
        let weights = &WEIGHTS[index];
        let [opening, middlegame, endgame] = weights.map(|w| value * w);
        let blended = f64::from(value * dot(blend, weights)) / f64::from(MAX_PHASE);
        writeln!(
            out,
            "{:<20}{value:>8}{opening:>10}{middlegame:>12}{endgame:>10}{blended:>10.2}",
            FEATURE_NAMES[index]
        )
        .unwrap();
    }
    writeln!(
        out,
        "{:<20}{:>10}{:>8}{:>10}",
        "group", "tuned", "style", "scaled"
    )
    .unwrap();
    for ((name, sum), percent) in GROUPS
        .iter()
        .zip(group_sums(board, &WEIGHTS))
        .zip(style.multipliers)
    {
        let tuned = f64::from(sum) / f64::from(MAX_PHASE);
        let scaled = tuned * f64::from(percent) / 100.0;
        writeln!(
            out,
            "{name:<20}{tuned:>10.2}{:>8}{scaled:>10.2}",
            format!("{percent}%")
        )
        .unwrap();
    }
    let [opening, middlegame, endgame] = blend;
    writeln!(
        out,
        "phase {phase}/{MAX_PHASE}: opening {opening}/{MAX_PHASE}, middlegame {middlegame}/{MAX_PHASE}, endgame {endgame}/{MAX_PHASE}"
    )
    .unwrap();
    let stm = evaluate_styled(board, style);
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
