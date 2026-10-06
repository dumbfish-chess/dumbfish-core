//! Playing below full strength: a node budget per move and a noisy choice among root moves.

use dumbfish_core::Move;

/// Full strength: no node budget, no noise.
pub const MAX_LEVEL: u8 = 20;

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
