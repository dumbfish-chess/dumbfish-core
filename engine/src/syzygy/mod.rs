//! Syzygy endgame tablebases: the result with the 50-move rule (WDL) and the distance to the
//! next capture or pawn move (DTZ) of positions with few pieces, read from `.rtbw` and `.rtbz`
//! files on first use.

mod table;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use dumbfish_core::{Board, Move, MoveKind, PieceKind};

use table::{Kind, Material, Table, board_key};

/// A result for the side to move: 2 win, 1 win that the 50-move rule turns into a draw, 0 draw,
/// -1 loss that the rule saves, -2 loss.
pub type Wdl = i32;

/// Root moves ranked above any distance.
const MAX_DTZ: i32 = 1 << 18;

struct Entry {
    material: Material,
    /// WDL and DTZ files.
    files: [Option<PathBuf>; 2],
    tables: [OnceLock<Option<Table>>; 2],
}

/// The tables found in some directories.
#[derive(Default)]
pub struct Tablebases {
    entries: Vec<Entry>,
    by_key: HashMap<u64, usize>,
    max_pieces: usize,
}

/// The root moves that keep the best tablebase result, and that result.
pub struct RootProbe {
    pub moves: Vec<Move>,
    pub wdl: Wdl,
    /// Plies to the next capture or pawn move along the best line, 0 for a draw.
    pub dtz: i32,
}

impl Tablebases {
    /// Finds the tables in the directories of `paths`, separated by `:`. A table needs its WDL
    /// file; the first directory that has a file wins.
    pub fn open(paths: &str) -> Self {
        let mut found: HashMap<String, [Option<PathBuf>; 2]> = HashMap::new();
        for dir in paths.split(':').filter(|dir| !dir.is_empty()) {
            let Ok(dir) = std::fs::read_dir(dir) else {
                continue;
            };
            for path in dir.flatten().map(|file| file.path()) {
                let (Some(stem), Some(extension)) = (
                    path.file_stem().and_then(|s| s.to_str()),
                    path.extension().and_then(|e| e.to_str()),
                ) else {
                    continue;
                };
                let slot = match extension {
                    "rtbw" => 0,
                    "rtbz" => 1,
                    _ => continue,
                };
                found.entry(stem.to_string()).or_default()[slot].get_or_insert(path);
            }
        }
        let mut tablebases = Self::default();
        let mut names: Vec<_> = found.into_iter().collect();
        names.sort();
        for (name, files) in names {
            let Some(material) = Material::parse(&name).filter(|_| files[0].is_some()) else {
                continue;
            };
            tablebases.max_pieces = tablebases.max_pieces.max(material.pieces);
            for key in material.keys {
                tablebases.by_key.insert(key, tablebases.entries.len());
            }
            tablebases.entries.push(Entry {
                material,
                files,
                tables: Default::default(),
            });
        }
        tablebases
    }

    /// Number of tables found.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Most pieces, kings included, of a table found.
    pub fn max_pieces(&self) -> usize {
        self.max_pieces
    }

    /// Positions that may be in the tables: few enough pieces and no castling rights.
    pub fn covers(&self, board: &Board) -> bool {
        board.occupied().count() as usize <= self.max_pieces && board.castling() == 0
    }

    /// The result of `board` with the 50-move counter at 0; `None` if a table is missing.
    pub fn probe_wdl(&self, board: &mut Board) -> Option<Wdl> {
        if !self.covers(board) {
            return None;
        }
        self.search(board, false).map(|(wdl, _)| wdl)
    }

    /// Plies to the next capture or pawn move with the 50-move counter at 0, positive if the
    /// side to move wins and negative if it loses; beyond 100 the rule draws. 0 for a draw,
    /// -1 when mated. May be one ply longer than the truth when the table counts in moves.
    pub fn probe_dtz(&self, board: &mut Board) -> Option<i32> {
        if !self.covers(board) {
            return None;
        }
        self.dtz(board)
    }

    /// The root moves of `moves` that keep the best result, the shortest win or the longest
    /// loss, given the position's 50-move counter; `None` if the root is not covered.
    pub fn rank_root(&self, board: &mut Board, moves: &[Move]) -> Option<RootProbe> {
        if !self.covers(board) || moves.is_empty() {
            return None;
        }
        let clock = i32::from(board.halfmove_clock());
        let mut ranked = Vec::new();
        for &mv in moves {
            board.make_move(mv);
            let dtz = if board.halfmove_clock() == 0 {
                self.probe_wdl(board).map(|wdl| before_zeroing(-wdl))
            } else if board.is_repetition() || board.halfmove_clock() >= 100 {
                Some(0)
            } else {
                self.dtz(board).map(|dtz| -dtz - dtz.signum())
            };
            let mate = dtz == Some(2) && board.in_check() && board.legal_moves().is_empty();
            board.unmake_move();
            let dtz = if mate { 1 } else { dtz? };
            let rank = match dtz {
                d if d > 0 && d + clock <= 99 => MAX_DTZ - d,
                d if d > 0 => MAX_DTZ / 2 - (d + clock),
                d if d < 0 && -2 * d + clock < 100 => -MAX_DTZ - d,
                d if d < 0 => -MAX_DTZ / 2 + (-d + clock),
                _ => 0,
            };
            ranked.push((mv, rank, dtz));
        }
        let (_, best, dtz) = *ranked.iter().max_by_key(|r| r.1)?;
        let bound = MAX_DTZ / 2 - 100;
        Some(RootProbe {
            moves: ranked.iter().filter(|r| r.1 == best).map(|r| r.0).collect(),
            wdl: match best {
                r if r >= bound => 2,
                r if r > 0 => 1,
                0 => 0,
                r if r > -bound => -1,
                _ => -2,
            },
            dtz,
        })
    }

    fn table(&self, board: &Board, kind: Kind) -> Option<&Table> {
        let entry = &self.entries[*self.by_key.get(&board_key(board))?];
        let slot = kind as usize;
        entry.tables[slot]
            .get_or_init(|| {
                let bytes = std::fs::read(entry.files[slot].as_ref()?).ok()?;
                Table::new(bytes, kind, entry.material.clone())
            })
            .as_ref()
    }

    fn stored_wdl(&self, board: &Board) -> Option<Wdl> {
        if board.occupied().count() == 2 {
            return Some(0);
        }
        self.table(board, Kind::Wdl)?.probe(board, 0)
    }

    /// The result of `board`, from the captures (and with `zeroing` the pawn moves) and the
    /// stored value, which the generator may have set to anything those moves already decide.
    /// The flag tells that a capture or pawn move is best, so the DTZ table does not know.
    fn search(&self, board: &mut Board, zeroing: bool) -> Option<(Wdl, bool)> {
        let moves = board.legal_moves();
        let (mut best, mut tried) = (-2, 0);
        for &mv in moves.iter() {
            if !is_capture(board, mv) && !(zeroing && is_pawn_move(board, mv)) {
                continue;
            }
            tried += 1;
            board.make_move(mv);
            let value = self.search(board, false);
            board.unmake_move();
            let value = -value?.0;
            if value > best {
                best = value;
                if value == 2 {
                    return Some((2, true));
                }
            }
        }
        let all = tried > 0 && tried == moves.len();
        let stored = if all { best } else { self.stored_wdl(board)? };
        Some(if best >= stored {
            (best, best > 0 || all)
        } else {
            (stored, false)
        })
    }

    fn dtz(&self, board: &mut Board) -> Option<i32> {
        let (wdl, zeroing_best) = self.search(board, true)?;
        if wdl == 0 {
            return Some(0);
        }
        if zeroing_best {
            return Some(before_zeroing(wdl));
        }
        if let Some(dtz) = self.table(board, Kind::Dtz)?.probe(board, wdl) {
            let rule = if wdl.abs() == 1 { 100 } else { 0 };
            return Some((dtz + rule) * wdl.signum());
        }
        let mut best: Option<i32> = None;
        for &mv in board.legal_moves().iter() {
            let zeroing = is_capture(board, mv) || is_pawn_move(board, mv);
            board.make_move(mv);
            let dtz = if zeroing {
                self.search(board, false)
                    .map(|(wdl, _)| -before_zeroing(wdl))
            } else {
                self.dtz(board).map(|dtz| -dtz)
            };
            let mate = dtz == Some(1) && board.in_check() && board.legal_moves().is_empty();
            board.unmake_move();
            let mut dtz = dtz?;
            if mate {
                best = Some(1);
            }
            if !zeroing {
                dtz += dtz.signum();
            }
            if dtz.signum() == wdl.signum() && best.is_none_or(|b| dtz < b) {
                best = Some(dtz);
            }
        }
        Some(best.unwrap_or(-1))
    }
}

/// The DTZ of a position whose best move is a capture or pawn move leading to `wdl`.
fn before_zeroing(wdl: Wdl) -> i32 {
    match wdl {
        2 => 1,
        1 => 101,
        -1 => -101,
        -2 => -1,
        _ => 0,
    }
}

fn is_capture(board: &Board, mv: Move) -> bool {
    mv.kind() == MoveKind::EnPassant || board.piece_on(mv.to()).is_some()
}

fn is_pawn_move(board: &Board, mv: Move) -> bool {
    board
        .piece_on(mv.from())
        .is_some_and(|p| p.kind == PieceKind::Pawn)
}
