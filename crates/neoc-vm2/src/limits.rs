//! Conversions from Lua numbers to Rust durations that cannot panic the host.
//!
//! The VM has no time budget, but a host panic from script input is still a bug: it fails the call
//! ([`crate::panics`]) or, in a program built with `panic = "abort"`, the process. [`sleep_duration`] turns the
//! seconds a script gave `task.wait` / `task.delay` into a [`Duration`]: NaN and negatives are zero, anything past
//! [`MAX_SLEEP_SECS`] is clamped.
//!
//! Ported from the old VM's `limits.rs`. Its buffer sizes (`size`, `zeroed`, `string_with_capacity`) are not here: on
//! mlua-ir a size is an unsigned integer argument, and the modules that allocate from one bound it themselves.

use std::time::Duration;

/// The longest a `task.wait` / `task.delay` sleeps: 100 years. Longer (and infinite) is clamped to this,
/// which is "never" for a process; `std::time::Duration` itself would panic building `inf`.
pub(crate) const MAX_SLEEP_SECS: f64 = 100.0 * 365.0 * 24.0 * 3600.0;

/// Seconds from a script as a [`Duration`]: NaN and negative are zero, and the result never exceeds
/// [`MAX_SLEEP_SECS`] (infinity included).
pub(crate) fn sleep_duration(secs: f64) -> Duration {
    if secs.is_nan() || secs <= 0.0 {
        Duration::ZERO
    } else {
        Duration::from_secs_f64(secs.min(MAX_SLEEP_SECS))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sleeps_never_panic() {
        for secs in [f64::NAN, f64::NEG_INFINITY, -1.0, -0.0, 0.0] {
            assert_eq!(sleep_duration(secs), Duration::ZERO);
        }
        for secs in [f64::INFINITY, 1.8e19, f64::MAX] {
            assert_eq!(sleep_duration(secs), Duration::from_secs_f64(MAX_SLEEP_SECS));
        }
        assert_eq!(sleep_duration(1.5), Duration::from_millis(1500));
    }
}
