//! Syzygy probing on the tables in `SYZYGY_PATH` (by default `target/syzygy`, see README):
//! `cargo test --release -p dumbfish --test syzygy -- --include-ignored`

use dumbfish::syzygy::Tablebases;
use dumbfish_core::{Board, Color, Move, Piece, PieceKind};

fn tables() -> (Tablebases, String) {
    let dir = std::env::var("SYZYGY_PATH")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../target/syzygy").into());
    let tablebases = Tablebases::open(&dir);
    assert!(!tablebases.is_empty(), "no tables in {dir}");
    (tablebases, dir)
}

/// Pseudo-random numbers (xorshift).
struct Random(u64);

impl Random {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) as usize % n
    }
}

/// A legal position with the material of the table `name` (like `KRPvKR`), pieces on random
/// squares and pawns off the first and last ranks; white is the first side unless `swap`.
/// `Board::from_fen` rejects positions where the side not to move is in check.
fn random_position(name: &str, swap: bool, random: &mut Random) -> Board {
    loop {
        let mut squares: [Option<Piece>; 64] = [None; 64];
        let (first, second) = name.split_once('v').unwrap();
        for (side, color) in [(first, Color::White), (second, Color::Black)] {
            let color = if swap { !color } else { color };
            for c in side.chars() {
                let kind = PieceKind::ALL
                    .into_iter()
                    .find(|k| k.char() == c.to_ascii_lowercase())
                    .unwrap();
                let sq = loop {
                    let sq = random.below(64);
                    let rank = sq / 8;
                    if squares[sq].is_none() && (kind != PieceKind::Pawn || (1..7).contains(&rank))
                    {
                        break sq;
                    }
                };
                squares[sq] = Some(Piece::new(color, kind));
            }
        }
        let rows: Vec<String> = (0..8)
            .rev()
            .map(|rank| {
                let mut row = String::new();
                let mut empty = 0;
                for file in 0..8 {
                    match squares[rank * 8 + file] {
                        Some(piece) => {
                            if empty > 0 {
                                row += &empty.to_string();
                                empty = 0;
                            }
                            row.push(piece.char());
                        }
                        None => empty += 1,
                    }
                }
                if empty > 0 {
                    row += &empty.to_string();
                }
                row
            })
            .collect();
        let side = ["w", "b"][random.below(2)];
        if let Ok(board) = Board::from_fen(&format!("{} {side} - - 0 1", rows.join("/"))) {
            return board;
        }
    }
}

/// Names of the tables with both files in `dir`.
fn table_names(dir: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter_map(|f| {
            f.file_name()
                .to_str()?
                .strip_suffix(".rtbw")
                .map(String::from)
        })
        .filter(|name| {
            std::path::Path::new(dir)
                .join(format!("{name}.rtbz"))
                .exists()
        })
        .collect();
    names.sort();
    names
}

fn child_wdls(tablebases: &Tablebases, board: &mut Board) -> Vec<(Move, i32)> {
    let moves = board.legal_moves();
    moves
        .iter()
        .map(|&mv| {
            board.make_move(mv);
            let wdl = tablebases
                .probe_wdl(board)
                .expect("a child of a covered position");
            board.unmake_move();
            (mv, -wdl)
        })
        .collect()
}

#[test]
#[ignore = "needs Syzygy tables"]
fn results_follow_from_the_moves() {
    let (tablebases, dir) = tables();
    let mut random = Random(0x5EED);
    let mut checked = 0;
    for name in table_names(&dir) {
        for i in 0..48 {
            let mut board = random_position(&name, i % 2 == 1, &mut random);
            let fen = board.fen();
            let wdl = tablebases
                .probe_wdl(&mut board)
                .expect("a covered position");
            let dtz = tablebases
                .probe_dtz(&mut board)
                .expect("a covered position");
            let children = child_wdls(&tablebases, &mut board);
            let best = children.iter().map(|c| c.1).max();
            match best {
                None if board.in_check() => assert_eq!((wdl, dtz), (-2, -1), "mated: {fen}"),
                None => assert_eq!((wdl, dtz), (0, 0), "stalemate: {fen}"),
                Some(best) => {
                    assert_eq!(
                        wdl.signum(),
                        best.signum(),
                        "the best move decides who wins: {fen}"
                    );
                    assert!(wdl < 2 || best == 2, "a win within the rule: {fen}");
                    assert!(
                        wdl == 2 || best < 2 || dtz == 101,
                        "a move wins within the rule, or the win is one ply beyond it: {fen}"
                    );
                }
            }
            assert_eq!(dtz.signum(), wdl.signum(), "DTZ follows WDL: {fen}");
            match wdl {
                2 => assert!((1..=100).contains(&dtz), "{dtz}: {fen}"),
                1 => assert!(dtz > 100, "{dtz}: {fen}"),
                -1 => assert!(dtz < -100, "{dtz}: {fen}"),
                -2 => assert!((-100..=-1).contains(&dtz), "{dtz}: {fen}"),
                _ => {}
            }
            if wdl == 2 {
                let all: Vec<_> = children.iter().map(|c| c.0).collect();
                let root = tablebases.rank_root(&mut board, &all).unwrap();
                assert_eq!(root.wdl, 2, "{fen}");
                for mv in root.moves {
                    let value = children.iter().find(|c| c.0 == mv).unwrap().1;
                    assert_eq!(value, 2, "{mv} keeps the win: {fen}");
                }
            }
            checked += 1;
        }
    }
    assert!(checked > 0);
}

/// Positions with WDL and exact DTZ from the lichess tablebase server, which has its own
/// prober and knows distances to mate. A table that counts in moves gives an odd number of
/// plies, one short of the exact distance when that is even.
#[test]
#[ignore = "needs Syzygy tables"]
fn matches_the_lichess_tablebase() {
    let (tablebases, _) = tables();
    for line in include_str!("syzygy.txt").lines() {
        let (fen, expected) = line.split_once(';').unwrap();
        let [wdl, dtz] = expected
            .split_whitespace()
            .map(|n| n.parse::<i32>().unwrap())
            .collect::<Vec<_>>()[..]
        else {
            panic!("{line}");
        };
        let mut board = Board::from_fen(fen).unwrap();
        assert_eq!(tablebases.probe_wdl(&mut board), Some(wdl), "WDL of {fen}");
        let probed = tablebases.probe_dtz(&mut board).expect(fen);
        assert!(
            probed == dtz || probed == dtz - dtz.signum(),
            "DTZ of {fen}: {probed}, exact {dtz}"
        );
    }
}

#[test]
fn no_tables_cover_nothing() {
    let tablebases = Tablebases::open("/nonexistent");
    let mut board = Board::from_fen("8/8/8/8/8/8/1Q6/K1k5 b - - 0 1").unwrap();
    assert!(tablebases.is_empty());
    assert_eq!(
        tablebases.probe_wdl(&mut board),
        None,
        "no table, no result"
    );
}
