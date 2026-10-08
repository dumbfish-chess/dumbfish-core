use std::sync::atomic::{AtomicU64, Ordering};

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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Entry {
    pub key: u64,
    pub best: Option<Move>,
    pub score: i32,
    pub depth: u8,
    pub bound: Bound,
    /// The node's static evaluation before correction, if known and within `i16` (except
    /// `i16::MIN`); stored so that it need not be computed again.
    pub eval: Option<i32>,
}

/// Search results by Zobrist hash, shared by search threads without locks; a new entry always
/// replaces the old one in its slot. A slot keeps the key XORed with the packed entry, so an entry
/// torn by two threads writing at once fails the key check and reads as missing.
pub struct TranspositionTable {
    slots: Vec<Slot>,
}

#[derive(Default)]
struct Slot {
    key: AtomicU64,
    data: AtomicU64,
}

impl TranspositionTable {
    /// The largest power-of-two number of entries that fits into `megabytes`.
    pub fn new(megabytes: usize) -> Self {
        let count = (megabytes * 1024 * 1024 / size_of::<Slot>()).max(1);
        Self {
            slots: (0..1usize << count.ilog2())
                .map(|_| Slot::default())
                .collect(),
        }
    }

    pub fn clear(&mut self) {
        for slot in &mut self.slots {
            *slot.key.get_mut() = 0;
            *slot.data.get_mut() = 0;
        }
    }

    pub fn probe(&self, key: u64) -> Option<Entry> {
        let slot = &self.slots[self.index(key)];
        let data = slot.data.load(Ordering::Relaxed);
        if slot.key.load(Ordering::Relaxed) ^ data != key {
            return None;
        }
        unpack(key, data)
    }

    /// `entry.score` must fit into `i16`.
    pub fn store(&self, entry: Entry) {
        let slot = &self.slots[self.index(entry.key)];
        let data = pack(&entry);
        slot.key.store(entry.key ^ data, Ordering::Relaxed);
        slot.data.store(data, Ordering::Relaxed);
    }

    /// Used entries among the first thousand slots, per mille.
    pub fn hashfull(&self) -> usize {
        let sample = &self.slots[..self.slots.len().min(1000)];
        let used = sample
            .iter()
            .filter(|slot| slot.data.load(Ordering::Relaxed) >> 40 & 3 != 0)
            .count();
        used * 1000 / sample.len()
    }

    fn index(&self, key: u64) -> usize {
        key as usize & (self.slots.len() - 1)
    }
}

/// Bits 0..16 move (0 for none), 16..32 score, 32..40 depth, 40..42 bound (0 for an empty slot),
/// 42..58 static evaluation (`i16::MIN` for none).
fn pack(entry: &Entry) -> u64 {
    debug_assert!(i16::try_from(entry.score).is_ok(), "{}", entry.score);
    let bound = match entry.bound {
        Bound::Exact => 1,
        Bound::Lower => 2,
        Bound::Upper => 3,
    };
    u64::from(entry.best.map_or(0, Move::bits))
        | u64::from(entry.score as i16 as u16) << 16
        | u64::from(entry.depth) << 32
        | bound << 40
        | u64::from(
            entry
                .eval
                .and_then(|e| i16::try_from(e).ok())
                .unwrap_or(i16::MIN) as u16,
        ) << 42
}

fn unpack(key: u64, data: u64) -> Option<Entry> {
    let bound = match data >> 40 & 3 {
        1 => Bound::Exact,
        2 => Bound::Lower,
        3 => Bound::Upper,
        _ => return None,
    };
    let best = match data as u16 {
        0 => None,
        bits => Move::from_bits(bits),
    };
    Some(Entry {
        key,
        best,
        score: i32::from((data >> 16) as u16 as i16),
        depth: (data >> 32) as u8,
        bound,
        eval: Some(i32::from((data >> 42) as u16 as i16)).filter(|&e| e != i32::from(i16::MIN)),
    })
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
