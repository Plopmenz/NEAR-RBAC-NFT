//! Time source abstraction, so token and code expiry can be tested deterministically.

use std::time::{SystemTime, UNIX_EPOCH};

/// Returns the current Unix time in seconds.
pub trait Clock: Send + Sync {
    fn now(&self) -> u64;
}

/// Wall-clock implementation.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is before the Unix epoch")
            .as_secs()
    }
}
