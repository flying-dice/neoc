//! Conversions from Lua numbers to Rust sizes and durations that cannot panic or abort the host.
//!
//! The VM has no CPU, memory or time budget, but a host panic or an allocation failure from script input is
//! still a bug: an allocation failure takes down the whole process, and with it every other script, and a panic
//! fails the call (`crate::lua::panics`) or, in a program built with `panic = "abort"`, the process. These helpers
//! are for the places where one Lua number directly sizes an allocation, a loop index or a sleep:
//!
//! - [`size`] and [`zeroed`] turn a Lua integer into a buffer length no larger than [`MAX_BUFFER`], and
//!   allocate it fallibly. A bigger request is a Lua error, not an abort. The bound is not a budget on what
//!   a script may hold (it can build bigger values piece by piece); it is the largest single size an argument
//!   may name.
//! - [`sleep_duration`] turns the seconds a script gave `task.wait` / `task.delay` into a [`Duration`]
//!   (NaN and negatives are zero, anything past [`MAX_SLEEP_SECS`] is clamped).

use std::time::Duration;

/// The largest buffer, counted in bytes or in characters (so up to four times as many bytes for text), that one
/// Lua number may size: 1 Gi.
pub(crate) const MAX_BUFFER: usize = 1 << 30;

/// The longest a `task.wait` / `task.delay` sleeps: 100 years. Longer (and infinite) is clamped to this,
/// which is "never" for a process; `std::time::Duration` itself would panic building `inf`.
pub(crate) const MAX_SLEEP_SECS: f64 = 100.0 * 365.0 * 24.0 * 3600.0;

/// `n` as a length, or the error to hand back to Lua when it is negative or beyond [`MAX_BUFFER`].
pub(crate) fn size(n: i64, what: &str) -> Result<usize, String> {
    if n < 0 {
        return Err(format!("{what} must be non-negative"));
    }
    match usize::try_from(n) {
        Ok(len) if len <= MAX_BUFFER => Ok(len),
        _ => Err(format!(
            "{what} {n} is too large (the most one call may size is {MAX_BUFFER})"
        )),
    }
}

/// A zeroed buffer of `len` bytes, allocated fallibly.
pub(crate) fn zeroed(len: usize) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    buf.try_reserve_exact(len)
        .map_err(|e| format!("cannot allocate {len} bytes: {e}"))?;
    buf.resize(len, 0);
    Ok(buf)
}

/// An empty `String` with room for `len` bytes, allocated fallibly.
pub(crate) fn string_with_capacity(len: usize) -> Result<String, String> {
    let mut s = String::new();
    s.try_reserve_exact(len)
        .map_err(|e| format!("cannot allocate {len} bytes: {e}"))?;
    Ok(s)
}

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
    fn sizes_are_bounded() {
        assert_eq!(size(0, "n"), Ok(0));
        assert_eq!(size(16, "n"), Ok(16));
        assert_eq!(size(MAX_BUFFER as i64, "n"), Ok(MAX_BUFFER));
        assert!(
            size(-1, "n")
                .unwrap_err()
                .contains("n must be non-negative")
        );
        assert!(
            size(MAX_BUFFER as i64 + 1, "n")
                .unwrap_err()
                .contains("too large")
        );
        assert!(size(-1, "n").is_err());
        assert!(size(i64::MAX, "n").is_err());
        assert!(size(MAX_BUFFER as i64 + 1, "n").is_err());
    }

    #[test]
    fn sleeps_never_panic() {
        for secs in [f64::NAN, f64::NEG_INFINITY, -1.0, -0.0, 0.0] {
            assert_eq!(sleep_duration(secs), Duration::ZERO);
        }
        for secs in [f64::INFINITY, 1.8e19, f64::MAX] {
            assert_eq!(
                sleep_duration(secs),
                Duration::from_secs_f64(MAX_SLEEP_SECS)
            );
        }
        assert_eq!(sleep_duration(1.5), Duration::from_millis(1500));
    }
}
