// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Hybrid logical clock (ADR 0008).
//!
//! A clock value is `(ms, counter, dev)`. It is written as
//! `<13-digit ms>-<4-digit counter>-<dev>` and always compared as a tuple.

use std::cmp::Ordering;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// Largest logical counter before the clock borrows the next millisecond.
pub const MAX_COUNTER: u16 = 9999;

/// Source of physical time in milliseconds since the Unix epoch.
pub trait Clock {
    /// Current physical time in milliseconds.
    fn now_ms(&self) -> u64;
}

/// The system wall clock.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or(0)
    }
}

/// A fixed clock, for tests.
#[derive(Debug, Clone, Copy)]
pub struct FixedClock(pub u64);

impl Clock for FixedClock {
    fn now_ms(&self) -> u64 {
        self.0
    }
}

/// One hybrid logical clock value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Hlc {
    /// Physical component, milliseconds since the Unix epoch.
    pub ms: u64,
    /// Logical counter, 0 to [`MAX_COUNTER`].
    pub counter: u16,
    /// Device identifier that issued the value.
    pub dev: String,
}

impl Hlc {
    /// Parses `<ms>-<counter>-<dev>`. The device part may itself contain `-`.
    pub fn parse(text: &str) -> Option<Hlc> {
        let mut parts = text.splitn(3, '-');
        let ms_text = parts.next()?;
        let counter_text = parts.next()?;
        let dev = parts.next()?;
        if ms_text.len() != 13 || counter_text.len() != 4 || dev.is_empty() {
            return None;
        }
        if !ms_text.bytes().all(|b| b.is_ascii_digit())
            || !counter_text.bytes().all(|b| b.is_ascii_digit())
        {
            return None;
        }
        let counter: u16 = counter_text.parse().ok()?;
        if counter > MAX_COUNTER {
            return None;
        }
        Some(Hlc {
            ms: ms_text.parse().ok()?,
            counter,
            dev: dev.to_owned(),
        })
    }

    /// Issues the next local clock value.
    ///
    /// `last` is the greatest value this device has issued or observed.
    pub fn tick(last: Option<&Hlc>, now_ms: u64, dev: &str) -> Hlc {
        let (ms, counter) = match last {
            None => (now_ms, 0),
            Some(prev) if now_ms > prev.ms => (now_ms, 0),
            Some(prev) if prev.counter >= MAX_COUNTER => (prev.ms + 1, 0),
            Some(prev) => (prev.ms, prev.counter + 1),
        };
        Hlc {
            ms,
            counter,
            dev: dev.to_owned(),
        }
    }

    /// The greater of two optional values.
    pub fn max_of(a: Option<Hlc>, b: Option<Hlc>) -> Option<Hlc> {
        match (a, b) {
            (Some(x), Some(y)) => Some(if y > x { y } else { x }),
            (x, None) => x,
            (None, y) => y,
        }
    }
}

impl Ord for Hlc {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.ms, self.counter, &self.dev).cmp(&(other.ms, other.counter, &other.dev))
    }
}

impl PartialOrd for Hlc {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Hlc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:013}-{:04}-{}", self.ms, self.counter, self.dev)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_text() {
        let h = Hlc::parse("1762872000123-0003-d-7f2c").unwrap();
        assert_eq!(h.ms, 1_762_872_000_123);
        assert_eq!(h.counter, 3);
        assert_eq!(h.dev, "d-7f2c");
        assert_eq!(h.to_string(), "1762872000123-0003-d-7f2c");
    }

    #[test]
    fn rejects_malformed_values() {
        for bad in [
            "",
            "1-2-3",
            "1762872000123-3-d",
            "1762872000123-0003-",
            "x762872000123-0003-d",
        ] {
            assert!(Hlc::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn compares_as_tuple_not_text() {
        let a = Hlc::parse("1762872000123-0009-d-b").unwrap();
        let b = Hlc::parse("1762872000123-0010-d-a").unwrap();
        assert!(a < b);
    }

    #[test]
    fn tick_advances_and_borrows_on_overflow() {
        let t0 = Hlc::tick(None, 100, "d-a");
        assert_eq!((t0.ms, t0.counter), (100, 0));
        let t1 = Hlc::tick(Some(&t0), 50, "d-a");
        assert_eq!((t1.ms, t1.counter), (100, 1));
        let full = Hlc {
            ms: 100,
            counter: MAX_COUNTER,
            dev: "d-a".into(),
        };
        let t2 = Hlc::tick(Some(&full), 100, "d-a");
        assert_eq!((t2.ms, t2.counter), (101, 0));
        assert!(t2 > full);
        let t3 = Hlc::tick(Some(&t2), 500, "d-a");
        assert_eq!((t3.ms, t3.counter), (500, 0));
    }
}
