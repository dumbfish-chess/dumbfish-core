//! Time allocation for a move under a clock.

use std::time::Duration;

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
