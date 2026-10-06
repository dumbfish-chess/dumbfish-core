use std::process::ExitCode;
use std::time::Instant;

use dumbfish_core::board::STARTPOS;
use dumbfish_core::perft::divide;
use dumbfish_core::{Bitboard, Board, Square};

const USAGE: &str = "usage:
  dumbfish-tools magics
  dumbfish-tools perft <depth> [fen]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("magics") => {
            magics();
            Ok(())
        }
        Some("perft") => perft(&args[1..]),
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

/// Prints leaf counts per root move, sorted, then the total.
fn perft(args: &[String]) -> Result<(), String> {
    let depth = args.first().and_then(|d| d.parse().ok()).ok_or(USAGE)?;
    let fen = if args.len() > 1 {
        args[1..].join(" ")
    } else {
        STARTPOS.to_string()
    };
    let mut board = Board::from_fen(&fen)?;

    let start = Instant::now();
    let mut split: Vec<_> = divide(&mut board, depth)
        .into_iter()
        .map(|(mv, n)| (mv.to_string(), n))
        .collect();
    let elapsed = start.elapsed();
    split.sort();
    for (mv, nodes) in &split {
        println!("{mv}: {nodes}");
    }
    let total: u64 = split.iter().map(|(_, n)| n).sum();
    println!(
        "\nnodes {total} time {}ms nps {}",
        elapsed.as_millis(),
        (total as f64 / elapsed.as_secs_f64()) as u64
    );
    Ok(())
}

/// Prints `BISHOP_MAGICS` and `ROOK_MAGICS` for `core/src/attacks.rs`.
fn magics() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let bishop = [(1, 1), (1, -1), (-1, 1), (-1, -1)];
    let rook = [(0, 1), (0, -1), (1, 0), (-1, 0)];
    for (name, dirs) in [("BISHOP", &bishop), ("ROOK", &rook)] {
        println!("const {name}_MAGICS: [u64; 64] = [");
        for sq in Square::ALL {
            println!("    {:#018x},", find_magic(sq, dirs, &mut rng));
        }
        println!("];");
    }
}

/// Random sparse multipliers until one maps every relevant occupancy without a harmful collision.
fn find_magic(sq: Square, dirs: &[(i8, i8)], rng: &mut Rng) -> u64 {
    let mask = relevant_mask(sq, dirs);
    let shift = 64 - mask.count();
    let cases: Vec<_> = subsets(mask)
        .map(|o| (o, slider_attacks(sq, o, dirs)))
        .collect();
    let mut table = vec![Bitboard::EMPTY; 1 << mask.count()];
    let mut filled_at = vec![0u32; 1 << mask.count()];
    for attempt in 1.. {
        let magic = rng.next() & rng.next() & rng.next();
        if (mask.0.wrapping_mul(magic) >> 56).count_ones() < 6 {
            continue;
        }
        let fits = cases.iter().all(|&(occupied, attacks)| {
            let i = (occupied.0.wrapping_mul(magic) >> shift) as usize;
            if filled_at[i] != attempt {
                filled_at[i] = attempt;
                table[i] = attacks;
                true
            } else {
                table[i] == attacks
            }
        });
        if fits {
            return magic;
        }
    }
    unreachable!()
}

fn offset(sq: Square, df: i8, dr: i8) -> Option<Square> {
    let (f, r) = (sq.file() as i8 + df, sq.rank() as i8 + dr);
    ((0..8).contains(&f) && (0..8).contains(&r)).then(|| Square::new(f as u8, r as u8))
}

fn slider_attacks(sq: Square, occupied: Bitboard, dirs: &[(i8, i8)]) -> Bitboard {
    let mut attacks = Bitboard::EMPTY;
    for &(df, dr) in dirs {
        let mut cur = sq;
        while let Some(to) = offset(cur, df, dr) {
            attacks |= to.bb();
            if occupied.contains(to) {
                break;
            }
            cur = to;
        }
    }
    attacks
}

/// Ray squares except the last one in each direction.
fn relevant_mask(sq: Square, dirs: &[(i8, i8)]) -> Bitboard {
    slider_attacks(sq, Bitboard::EMPTY, dirs)
        & !dirs.iter().fold(Bitboard::EMPTY, |acc, &(df, dr)| {
            let mut cur = sq;
            while let Some(to) = offset(cur, df, dr) {
                cur = to;
            }
            if cur == sq { acc } else { acc | cur.bb() }
        })
}

/// All subsets of `mask` (Carry-Rippler).
fn subsets(mask: Bitboard) -> impl Iterator<Item = Bitboard> {
    std::iter::successors(Some(0u64), move |&s| {
        let next = s.wrapping_sub(mask.0) & mask.0;
        (next != 0).then_some(next)
    })
    .map(Bitboard)
}

/// xorshift64*
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}
