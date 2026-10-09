//! Playing below full strength: a node budget per move and a noisy choice among root moves.

use std::sync::LazyLock;

use dumbfish_core::Move;

/// Full strength: no node budget, no noise.
pub const MAX_LEVEL: u8 = 20;

/// Elo of each level, measured by `scripts/calibrate.sh`.
pub type EloTable = [i32; MAX_LEVEL as usize + 1];

/// The levels' Elo from `engine/skill-elo.txt`, for `UCI_Elo`.
pub static ELO: LazyLock<EloTable> = LazyLock::new(|| {
    parse_elo(include_str!("../skill-elo.txt")).expect("valid engine/skill-elo.txt")
});

/// Parses `level elo` lines, skipping blank lines and `#` comments. Every level must appear
/// once, and the Elo must not fall as the level grows.
pub fn parse_elo(text: &str) -> Result<EloTable, String> {
    let mut table = [None; MAX_LEVEL as usize + 1];
    let lines = text.lines().map(str::trim);
    for line in lines.filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let parsed = match line.split_whitespace().collect::<Vec<_>>()[..] {
            [level, elo] => level.parse::<usize>().ok().zip(elo.parse::<i32>().ok()),
            _ => None,
        };
        let (level, elo) = parsed.ok_or_else(|| format!("expected `level elo`: {line:?}"))?;
        let slot = table
            .get_mut(level)
            .filter(|slot| slot.is_none())
            .ok_or_else(|| format!("level out of 0..={MAX_LEVEL} or repeated: {line:?}"))?;
        *slot = Some(elo);
    }
    let table: Vec<i32> = table
        .into_iter()
        .collect::<Option<_>>()
        .ok_or_else(|| format!("expected every level 0..={MAX_LEVEL}"))?;
    if table.windows(2).any(|pair| pair[0] > pair[1]) {
        return Err(format!("Elo must not fall as the level grows: {table:?}"));
    }
    Ok(table.try_into().expect("one Elo per level"))
}

/// The level whose Elo in `table` is closest to `elo`, the lowest of equals.
pub fn level_for_elo(table: &EloTable, elo: i32) -> u8 {
    (0..=MAX_LEVEL)
        .min_by_key(|&level| (table[level as usize] - elo).abs())
        .expect("levels exist")
}

#[derive(Clone, Copy, Debug)]
pub struct Skill {
    /// 0 ..= `MAX_LEVEL`.
    pub level: u8,
    /// Seed of the noise, so that a game can be replayed.
    pub seed: u64,
}

impl Skill {
    /// Nodes per move: 256 at level 0, doubling every two levels; unlimited at `MAX_LEVEL`.
    pub fn nodes(self) -> Option<u64> {
        (self.level < MAX_LEVEL).then(|| 256 << (self.level / 2))
    }

    /// Largest random bonus a root move can get, in centipawns: 15 per level below `MAX_LEVEL`.
    pub fn noise(self) -> i32 {
        15 * i32::from(MAX_LEVEL.saturating_sub(self.level))
    }

    /// The move with the highest score plus a uniform random bonus in `0..=noise()`.
    pub fn choose(self, scored: &[(Move, i32)]) -> Option<Move> {
        let mut state = self.seed;
        let range = self.noise() as u64 + 1;
        scored
            .iter()
            .map(|&(mv, score)| (mv, score + (splitmix64(&mut state) % range) as i32))
            .max_by_key(|&(_, noisy)| noisy)
            .map(|(mv, _)| mv)
    }
}

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}
