//! Time allocation for a move under a clock.

use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Time kept back for the GUI and process latency.
pub const MOVE_OVERHEAD: Duration = Duration::from_millis(30);

/// Moves assumed to remain when the GUI does not say.
const DEFAULT_MOVES_TO_GO: u32 = 30;

/// `(soft, hard)` limits for a move with `time` left on the clock and `inc` added per move: no
/// new iteration starts after `soft`, and the search stops at `hard`. Neither exceeds the time
/// left minus `MOVE_OVERHEAD`.
pub fn allocate(time: Duration, inc: Duration, moves_to_go: Option<u32>) -> (Duration, Duration) {
    let available = time.saturating_sub(MOVE_OVERHEAD);
    let moves = moves_to_go
        .filter(|&n| n > 0)
        .unwrap_or(DEFAULT_MOVES_TO_GO);
    let soft = (time / moves + inc * 3 / 4).min(available);
    let hard = (soft * 3).min(available);
    (soft, hard)
}

/// A moment that another thread may set or clear while a search runs, as for `ponderhit`.
pub struct Deadline(AtomicU64);

/// Origin of the nanosecond counts `Deadline` stores.
static ORIGIN: LazyLock<Instant> = LazyLock::new(Instant::now);

const UNSET: u64 = u64::MAX;

impl Default for Deadline {
    fn default() -> Self {
        Self(AtomicU64::new(UNSET))
    }
}

impl Deadline {
    pub fn at(instant: Instant) -> Self {
        let deadline = Self::default();
        deadline.set(Some(instant));
        deadline
    }

    pub fn set(&self, instant: Option<Instant>) {
        let nanos = instant.map_or(UNSET, |i| {
            i.saturating_duration_since(*ORIGIN).as_nanos() as u64
        });
        self.0.store(nanos, Ordering::Relaxed);
    }

    pub fn get(&self) -> Option<Instant> {
        let nanos = self.0.load(Ordering::Relaxed);
        (nanos != UNSET).then(|| *ORIGIN + Duration::from_nanos(nanos))
    }

    pub fn is_set(&self) -> bool {
        self.0.load(Ordering::Relaxed) != UNSET
    }

    pub fn passed(&self) -> bool {
        let nanos = self.0.load(Ordering::Relaxed);
        nanos != UNSET && ORIGIN.elapsed().as_nanos() as u64 >= nanos
    }
}
