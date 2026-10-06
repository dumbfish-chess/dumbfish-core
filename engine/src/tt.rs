use dumbfish_core::Move;

use crate::search::{MATE, MAX_DEPTH};

pub const DEFAULT_MEGABYTES: usize = 16;
pub const MAX_MEGABYTES: usize = 65_536;

/// What the stored score says about the true score.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bound {
    Exact,
    /// The true score is at least the stored one.
    Lower,
    /// The true score is at most the stored one.
    Upper,
}

#[derive(Clone, Copy, Debug)]
pub struct Entry {
    pub key: u64,
    pub best: Option<Move>,
    pub score: i32,
    pub depth: u8,
    pub bound: Bound,
}

/// Search results by Zobrist hash; a new entry always replaces the old one in its slot.
pub struct TranspositionTable {
    entries: Vec<Option<Entry>>,
}

impl TranspositionTable {
    /// The largest power-of-two number of entries that fits into `megabytes`.
    pub fn new(megabytes: usize) -> Self {
        let count = (megabytes * 1024 * 1024 / size_of::<Option<Entry>>()).max(1);
        Self {
            entries: vec![None; 1 << count.ilog2()],
        }
    }

    pub fn clear(&mut self) {
        self.entries.fill(None);
    }

    pub fn probe(&self, key: u64) -> Option<Entry> {
        self.entries[self.index(key)].filter(|e| e.key == key)
    }

    pub fn store(&mut self, entry: Entry) {
        let index = self.index(entry.key);
        self.entries[index] = Some(entry);
    }

    fn index(&self, key: u64) -> usize {
        key as usize & (self.entries.len() - 1)
    }
}

/// Mate scores are stored relative to the node rather than the root, so that an entry stays
/// valid when the position is reached at another ply. `ply` is the node's distance from the root.
pub fn score_to_tt(score: i32, ply: i32) -> i32 {
    match score {
        s if s >= MATE - MAX_DEPTH as i32 => s + ply,
        s if s <= MAX_DEPTH as i32 - MATE => s - ply,
        s => s,
    }
}

/// Inverse of `score_to_tt`.
pub fn score_from_tt(score: i32, ply: i32) -> i32 {
    match score {
        s if s >= MATE - MAX_DEPTH as i32 => s - ply,
        s if s <= MAX_DEPTH as i32 - MATE => s + ply,
        s => s,
    }
}
