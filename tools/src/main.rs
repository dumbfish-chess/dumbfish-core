use std::process::ExitCode;

use dumbfish_core::{Bitboard, Square};

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("magics") => magics(),
        _ => {
            eprintln!("usage: dumbfish-tools magics");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
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
