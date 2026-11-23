//! Monotonic clock: `std:time` (mirrors `std::time`).
//!
//! Two userdata types: `Instant` (an opaque monotonic point) and `Duration` (a length of time). Independent of the
//! wall clock: they survive NTP slews, DST jumps and manual clock changes. Arithmetic saturates instead of
//! panicking, so a script cannot blow up on overflow or a negative result.
//!
//! Ported from neoc-vm's `std:time`. What changed on purpose: operators (`+ - * / < ==`) are methods (`plus`,
//! `minus`, `times`, `divided_by`, `compare`), `Duration.ZERO` is `Duration.zero()`, the counts (`as_millis`, ...)
//! saturate at 2^53 (the largest whole number a Lua number holds), a negative count is refused as an argument, and
//! every failure is a typed error table.

use std::time::Duration as StdDuration;

/// The largest whole number a Lua number holds exactly; a longer count saturates here.
const MAX_COUNT: u64 = 1 << 53;

#[mlua_ir::module("std:time")]
pub mod time {
    use std::time::{Duration as StdDuration, Instant as StdInstant};

    use mlua_ir::{LuaError, Object, Shape};
    use serde::{Deserialize, Serialize};

    use super::{MAX_COUNT, format_duration, parse_duration};

    /// How two values order.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum Order {
        /// The first is before the second.
        Less,
        /// They are the same.
        Equal,
        /// The first is after the second.
        Greater,
    }

    impl From<std::cmp::Ordering> for Order {
        fn from(ordering: std::cmp::Ordering) -> Self {
            match ordering {
                std::cmp::Ordering::Less => Order::Less,
                std::cmp::Ordering::Equal => Order::Equal,
                std::cmp::Ordering::Greater => Order::Greater,
            }
        }
    }

    /// Why a duration or an instant could not be made.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum TimeError {
        /// The seconds are negative, not a number, or too many for a duration.
        #[error("seconds must be finite, non-negative and fit a duration")]
        OutOfRange,
        /// The factor of a product is negative or not finite, or the product is too long for a duration.
        #[error("the factor must be finite and non-negative, and the product must fit a duration")]
        InvalidFactor,
        /// A duration was divided by zero.
        #[error("division by zero")]
        DivisionByZero,
        /// The instant would be past the end of the clock, or before its start.
        #[error("the instant is out of the clock's range")]
        InstantOverflow,
    }

    /// Why text is not a duration.
    #[derive(Debug, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum ParseError {
        /// There is nothing to read.
        #[error("empty input")]
        Empty,
        /// A number was expected where the text has something else.
        #[error("expected a number at {at:?}")]
        ExpectedNumber {
            /// The rest of the text from where the number should be.
            at: String,
        },
        /// A number is not one (`1.2.3`).
        #[error("invalid number {text:?}")]
        InvalidNumber {
            /// The number as written.
            text: String,
        },
        /// A number has no unit after it.
        #[error("missing unit after {number:?}")]
        MissingUnit {
            /// The number it follows.
            number: String,
        },
        /// A unit is not `ns`, `us`, `µs`, `ms`, `s`, `m` or `h`.
        #[error("unknown unit {unit:?}")]
        UnknownUnit {
            /// The unit as written.
            unit: String,
        },
        /// A duration is not negative.
        #[error("duration cannot be negative")]
        Negative,
        /// The duration is longer than about 584 years.
        #[error("duration overflow")]
        Overflow,
    }

    /// A length of time, from nanoseconds up to about 584 years. `tostring` gives the text `parse` reads
    /// (`1h30m`, `1.5s`, `500ms`).
    #[derive(Debug, Clone, Copy, Object)]
    pub struct Duration {
        pub(super) inner: StdDuration,
    }

    fn duration(inner: StdDuration) -> Duration {
        Duration { inner }
    }

    /// For the modules that take a duration (a timeout).
    impl From<Duration> for StdDuration {
        fn from(duration: Duration) -> Self {
            duration.inner
        }
    }

    /// A count of units, saturating at the largest whole number a Lua number holds.
    fn count(n: u128) -> u64 {
        u64::try_from(n).unwrap_or(u64::MAX).min(MAX_COUNT)
    }

    #[mlua_ir::object]
    impl Duration {
        /// A duration of no time.
        ///
        /// ```luau
        /// assert(time.Duration.zero():is_zero())
        /// ```
        pub fn zero() -> Duration {
            duration(StdDuration::ZERO)
        }

        /// `secs` whole seconds.
        ///
        /// ```luau
        /// assert(time.Duration.from_secs(2):as_millis() == 2000)
        /// ```
        pub fn from_secs(secs: u64) -> Duration {
            duration(StdDuration::from_secs(secs))
        }

        /// `millis` milliseconds.
        pub fn from_millis(millis: u64) -> Duration {
            duration(StdDuration::from_millis(millis))
        }

        /// `micros` microseconds.
        pub fn from_micros(micros: u64) -> Duration {
            duration(StdDuration::from_micros(micros))
        }

        /// `nanos` nanoseconds.
        pub fn from_nanos(nanos: u64) -> Duration {
            duration(StdDuration::from_nanos(nanos))
        }

        /// `secs` seconds, fractions included.
        ///
        /// ```luau
        /// local d = assert(time.Duration.from_secs_f64(1.5))
        /// assert(d:as_millis() == 1500)
        /// local _, err = time.Duration.from_secs_f64(-1)
        /// assert(err ~= nil and err.kind == "out_of_range")
        /// ```
        pub fn from_secs_f64(secs: f64) -> Result<Duration, TimeError> {
            StdDuration::try_from_secs_f64(secs).map(duration).map_err(|_| TimeError::OutOfRange)
        }

        /// A duration written as `<number><unit>` segments: `1h30m`, `1.5s`, `500ms`, `250us`. The units are `ns`,
        /// `us` (or `µs`), `ms`, `s`, `m`, `h`; `0` alone is zero.
        ///
        /// ```luau
        /// local d = assert(time.Duration.parse("1m30s"))
        /// assert(d:as_secs() == 90 and tostring(d) == "1m30s")
        /// local _, err = time.Duration.parse("5")
        /// assert(err ~= nil and err.kind == "missing_unit")
        /// ```
        pub fn parse(text: String) -> Result<Duration, ParseError> {
            parse_duration(&text).map(duration)
        }

        /// Whole seconds.
        pub fn as_secs(&self) -> u64 {
            count(u128::from(self.inner.as_secs()))
        }

        /// Total milliseconds, saturating at 2^53.
        pub fn as_millis(&self) -> u64 {
            count(self.inner.as_millis())
        }

        /// Total microseconds, saturating at 2^53.
        pub fn as_micros(&self) -> u64 {
            count(self.inner.as_micros())
        }

        /// Total nanoseconds, saturating at 2^53.
        pub fn as_nanos(&self) -> u64 {
            count(self.inner.as_nanos())
        }

        /// Total seconds, fractions included.
        pub fn as_secs_f64(&self) -> f64 {
            self.inner.as_secs_f64()
        }

        /// Whether the duration is exactly zero.
        pub fn is_zero(&self) -> bool {
            self.inner.is_zero()
        }

        /// This and `other` together; the longest a duration can be if they overflow.
        pub fn plus(&self, other: Duration) -> Duration {
            duration(self.inner.saturating_add(other.inner))
        }

        /// This less `other`; zero when `other` is longer.
        ///
        /// ```luau
        /// local short, long = time.Duration.from_millis(100), time.Duration.from_millis(500)
        /// assert(short:minus(long):is_zero())
        /// ```
        pub fn minus(&self, other: Duration) -> Duration {
            duration(self.inner.saturating_sub(other.inner))
        }

        /// This many times over: `factor` is not negative and not infinite.
        pub fn times(&self, factor: f64) -> Result<Duration, TimeError> {
            scale(self.inner, factor)
        }

        /// This split into `divisor` parts: `divisor` is not zero, negative or infinite.
        ///
        /// ```luau
        /// assert(time.Duration.from_secs(2):divided_by(4):as_millis() == 500)
        /// ```
        pub fn divided_by(&self, divisor: f64) -> Result<Duration, TimeError> {
            if divisor == 0.0 {
                return Err(TimeError::DivisionByZero);
            }
            scale(self.inner, 1.0 / divisor)
        }

        /// How this orders against `other`.
        pub fn compare(&self, other: Duration) -> Order {
            self.inner.cmp(&other.inner).into()
        }

        /// The text `parse` reads back.
        #[to_string]
        pub fn display(&self) -> String {
            format_duration(self.inner)
        }
    }

    fn scale(d: StdDuration, factor: f64) -> Result<Duration, TimeError> {
        if !factor.is_finite() || factor < 0.0 {
            return Err(TimeError::InvalidFactor);
        }
        StdDuration::try_from_secs_f64(d.as_secs_f64() * factor)
            .map(duration)
            .map_err(|_| TimeError::InvalidFactor)
    }

    /// A point on the monotonic clock. Its value means nothing alone: take the difference of two.
    #[derive(Debug, Clone, Copy, Object)]
    pub struct Instant {
        inner: StdInstant,
    }

    #[mlua_ir::object]
    impl Instant {
        /// The current point on the clock.
        ///
        /// ```luau
        /// local started = time.Instant.now()
        /// assert(started:elapsed():as_secs() < 5)
        /// ```
        pub fn now() -> Instant {
            Instant { inner: StdInstant::now() }
        }

        /// How long ago this instant was taken.
        pub fn elapsed(&self) -> Duration {
            duration(self.inner.elapsed())
        }

        /// This less `other`; zero when `other` is later.
        pub fn duration_since(&self, other: Instant) -> Duration {
            duration(self.inner.saturating_duration_since(other.inner))
        }

        /// The instant `by` later.
        ///
        /// ```luau
        /// local t0 = time.now()
        /// local later = assert(t0:plus(time.Duration.from_millis(250)))
        /// assert(later:duration_since(t0):as_millis() == 250)
        /// ```
        pub fn plus(&self, by: Duration) -> Result<Instant, TimeError> {
            self.inner.checked_add(by.inner).map(|inner| Instant { inner }).ok_or(TimeError::InstantOverflow)
        }

        /// The instant `by` earlier.
        pub fn minus(&self, by: Duration) -> Result<Instant, TimeError> {
            self.inner.checked_sub(by.inner).map(|inner| Instant { inner }).ok_or(TimeError::InstantOverflow)
        }

        /// How this orders against `other`.
        pub fn compare(&self, other: Instant) -> Order {
            self.inner.cmp(&other.inner).into()
        }

        /// `Instant`: the value is opaque.
        #[to_string]
        pub fn display(&self) -> String {
            "Instant".to_owned()
        }
    }

    /// The current point on the clock (the same as `Instant.now()`).
    pub fn now() -> Instant {
        Instant::now()
    }

    /// Wait for `duration` without blocking other scripts; the time that actually passed.
    ///
    /// ```luau
    /// local waited = time.sleep(time.Duration.from_millis(1))
    /// assert(waited:as_millis() >= 1)
    /// ```
    #[host("Timer")]
    pub async fn sleep(duration: Duration) -> Duration {
        let start = StdInstant::now();
        tokio::time::sleep(duration.inner).await;
        super::time::duration(start.elapsed())
    }
}

use time::ParseError;

/// Parses `<number><unit>` segments (`ns`, `us`/`µs`, `ms`, `s`, `m`, `h`; each number may be fractional); `0` alone
/// is zero.
fn parse_duration(input: &str) -> Result<StdDuration, ParseError> {
    if input.is_empty() {
        return Err(ParseError::Empty);
    }
    if input == "0" {
        return Ok(StdDuration::ZERO);
    }
    let mut total = StdDuration::ZERO;
    let mut rest = input;
    while !rest.is_empty() {
        let number_end = rest
            .char_indices()
            .find(|(_, c)| !c.is_ascii_digit() && *c != '.')
            .map_or(rest.len(), |(i, _)| i);
        if number_end == 0 {
            return Err(ParseError::ExpectedNumber { at: rest.to_owned() });
        }
        let number = &rest[..number_end];
        let value: f64 = number.parse().map_err(|_| ParseError::InvalidNumber { text: number.to_owned() })?;
        if value < 0.0 {
            return Err(ParseError::Negative);
        }
        rest = &rest[number_end..];

        let nanos_per_unit: f64 = if let Some(stripped) = rest.strip_prefix("µs") {
            rest = stripped;
            1_000.0
        } else {
            let unit_end =
                rest.char_indices().find(|(_, c)| !c.is_ascii_alphabetic()).map_or(rest.len(), |(i, _)| i);
            if unit_end == 0 {
                return Err(ParseError::MissingUnit { number: number.to_owned() });
            }
            let unit = &rest[..unit_end];
            rest = &rest[unit_end..];
            match unit {
                "ns" => 1.0,
                "us" => 1_000.0,
                "ms" => 1_000_000.0,
                "s" => 1_000_000_000.0,
                "m" => 60.0 * 1_000_000_000.0,
                "h" => 3_600.0 * 1_000_000_000.0,
                other => return Err(ParseError::UnknownUnit { unit: other.to_owned() }),
            }
        };

        let nanos = value * nanos_per_unit;
        // `as u64` saturates, which would silently cap a huge input at ~584 years: bound it so the caller hears.
        #[allow(clippy::cast_precision_loss)]
        if !nanos.is_finite() || nanos.round() > u64::MAX as f64 {
            return Err(ParseError::Overflow);
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // bounded above
        let segment = StdDuration::from_nanos(nanos.round() as u64);
        total = total.checked_add(segment).ok_or(ParseError::Overflow)?;
    }
    Ok(total)
}

/// The text form `parse_duration` reads: the largest non-zero unit first, as Go's `Duration.String`. Under a second
/// it is a single unit (`500ms`, `1.5µs`); from a second up it walks down through hours, minutes and seconds with
/// the fraction on the seconds.
fn format_duration(d: StdDuration) -> String {
    if d.is_zero() {
        return "0s".into();
    }
    let nanos = d.as_nanos();
    if nanos < 1_000 {
        return format!("{nanos}ns");
    }
    if nanos < 1_000_000 {
        return format_fraction(nanos, 1_000, "µs");
    }
    if nanos < 1_000_000_000 {
        return format_fraction(nanos, 1_000_000, "ms");
    }
    let total_secs = d.as_secs();
    let sub_nanos = d.subsec_nanos();
    let hours = total_secs / 3_600;
    let minutes = (total_secs % 3_600) / 60;
    let secs = total_secs % 60;

    let mut out = String::new();
    if hours > 0 {
        out.push_str(&format!("{hours}h"));
    }
    if minutes > 0 || (hours > 0 && (secs > 0 || sub_nanos > 0)) {
        out.push_str(&format!("{minutes}m"));
    }
    if sub_nanos == 0 {
        out.push_str(&format!("{secs}s"));
    } else {
        let frac = format!("{sub_nanos:09}");
        out.push_str(&format!("{secs}.{}s", frac.trim_end_matches('0')));
    }
    out
}

fn format_fraction(nanos: u128, divisor: u128, unit: &str) -> String {
    let whole = nanos / divisor;
    let rem = nanos % divisor;
    if rem == 0 {
        return format!("{whole}{unit}");
    }
    let width = if divisor == 1_000 { 3 } else { 6 };
    let frac = format!("{rem:0>width$}");
    format!("{whole}.{}{unit}", frac.trim_end_matches('0'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::make_engine;

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Runtime::new().unwrap()
    }

    #[test]
    fn parse_basic_units() {
        assert_eq!(parse_duration("0").unwrap(), StdDuration::ZERO);
        assert_eq!(parse_duration("500ms").unwrap(), StdDuration::from_millis(500));
        assert_eq!(parse_duration("2s").unwrap(), StdDuration::from_secs(2));
        assert_eq!(parse_duration("1m30s").unwrap(), StdDuration::from_secs(90));
        assert_eq!(parse_duration("1h").unwrap(), StdDuration::from_secs(3_600));
        assert_eq!(parse_duration("1h30m").unwrap(), StdDuration::from_secs(5_400));
        assert_eq!(parse_duration("1.5s").unwrap(), StdDuration::from_millis(1_500));
        assert_eq!(parse_duration("500us").unwrap(), StdDuration::from_micros(500));
        assert_eq!(parse_duration("500µs").unwrap(), StdDuration::from_micros(500));
        assert_eq!(parse_duration("250ns").unwrap(), StdDuration::from_nanos(250));
    }

    #[test]
    fn parse_rejects_bad_input_with_the_reason() {
        assert_eq!(parse_duration(""), Err(ParseError::Empty));
        assert_eq!(parse_duration("5"), Err(ParseError::MissingUnit { number: "5".into() }));
        assert_eq!(parse_duration("s"), Err(ParseError::ExpectedNumber { at: "s".into() }));
        assert_eq!(parse_duration("5xs"), Err(ParseError::UnknownUnit { unit: "xs".into() }));
        assert_eq!(parse_duration("1.2.3s"), Err(ParseError::InvalidNumber { text: "1.2.3".into() }));
        assert!(parse_duration("-5s").is_err());
    }

    #[test]
    fn parse_rejects_overflow() {
        assert_eq!(parse_duration("99999999999999999999h"), Err(ParseError::Overflow));
    }

    #[test]
    fn format_roundtrip() {
        for text in ["500ns", "500µs", "500ms", "2s", "1.5s", "1m30s", "1h", "1h30m", "0s"] {
            let parsed = parse_duration(text).unwrap_or_else(|e| panic!("parse {text}: {e}"));
            let formatted = format_duration(parsed);
            let reparsed = parse_duration(&formatted).unwrap_or_else(|e| panic!("reparse {formatted}: {e}"));
            assert_eq!(parsed, reparsed, "roundtrip mismatch: {text} -> {formatted}");
        }
    }

    fn eval<T: mlua::FromLuaMulti>(code: &str) -> T {
        make_engine().unwrap().load(code).eval().unwrap()
    }

    #[test]
    fn duration_arithmetic_saturates() {
        let (sum, diff): (u64, u64) = eval(
            r#"
            local time = require("std:time")
            local a = time.Duration.from_millis(500)
            local b = time.Duration.from_millis(300)
            return a:plus(b):as_millis(), b:minus(a):as_millis()
        "#,
        );
        assert_eq!((sum, diff), (800, 0));
    }

    #[test]
    fn duration_times_and_divided_by() {
        let (a, b): (u64, u64) = eval(
            r#"
            local d = require("std:time").Duration.from_secs(2)
            return assert(d:times(1.5)):as_millis(), assert(d:divided_by(4)):as_millis()
        "#,
        );
        assert_eq!((a, b), (3_000, 500));
    }

    #[test]
    fn times_and_divided_by_refuse_what_is_not_a_duration() {
        let (neg, zero, nan): (String, String, String) = eval(
            r#"
            local d = require("std:time").Duration.from_secs(1)
            local _, a = d:times(-1)
            local _, b = d:divided_by(0)
            local _, c = d:times(0/0)
            return a.kind, b.kind, c.kind
        "#,
        );
        assert_eq!(
            (neg.as_str(), zero.as_str(), nan.as_str()),
            ("invalid_factor", "division_by_zero", "invalid_factor")
        );
    }

    #[test]
    fn a_negative_count_is_refused_as_an_argument() {
        let lua = make_engine().unwrap();
        let err = lua
            .load(r#"return require("std:time").Duration.from_secs(-1)"#)
            .eval::<mlua::Value>()
            .expect_err("from_secs(-1) must be refused")
            .to_string();
        assert!(
            err.contains("std:time.Duration.from_secs: argument #1 'secs': expected a whole number"),
            "{err}"
        );
    }

    #[test]
    fn compare_orders() {
        let (lt, eq, gt): (String, String, String) = eval(
            r#"
            local D = require("std:time").Duration
            local a, b = D.from_millis(100), D.from_millis(200)
            return a:compare(b), a:compare(D.from_millis(100)), b:compare(a)
        "#,
        );
        assert_eq!((lt.as_str(), eq.as_str(), gt.as_str()), ("less", "equal", "greater"));
    }

    #[test]
    fn tostring_matches_parse() {
        let text: String = eval(r#"return tostring(assert(require("std:time").Duration.parse("1m30s")))"#);
        assert_eq!(text, "1m30s");
    }

    #[test]
    fn counts_saturate_instead_of_wrapping() {
        let (nanos, micros, millis): (u64, u64, u64) = eval(
            r#"
            local d = assert(require("std:time").Duration.parse("9000h"))
            return d:as_nanos(), d:as_micros(), d:as_millis()
        "#,
        );
        assert_eq!(nanos, MAX_COUNT, "~3.2e16 ns is past 2^53");
        assert!(micros > 0 && millis > 0);
    }

    #[test]
    fn instant_arithmetic() {
        let (diff, back, order): (u64, bool, String) = eval(
            r#"
            local time = require("std:time")
            local t0 = time.now()
            local later = assert(t0:plus(time.Duration.from_millis(250)))
            return later:duration_since(t0):as_millis(), t0:duration_since(later):is_zero(), t0:compare(later)
        "#,
        );
        assert_eq!((diff, back, order.as_str()), (250, true, "less"));
    }

    #[test]
    fn instant_overflow_is_a_typed_error() {
        let kind: String = eval(
            r#"
            local time = require("std:time")
            local _, err = time.now():minus(time.Duration.from_secs(2^53))
            return err.kind
        "#,
        );
        assert_eq!(kind, "instant_overflow");
    }

    #[test]
    fn instant_diff_across_a_task_wait_is_a_duration() {
        rt().block_on(async {
            let lua = make_engine().unwrap();
            let ms: u64 = lua
                .load(
                    r#"
                    local time = require("std:time")
                    local t0 = time.now()
                    time.sleep(time.Duration.from_millis(50))
                    return t0:elapsed():as_millis()
                "#,
                )
                .eval_async()
                .await
                .unwrap();
            assert!(ms >= 50, "elapsed {ms}ms < 50ms");
        });
    }

    #[test]
    fn sleep_returns_elapsed_duration() {
        rt().block_on(async {
            let lua = make_engine().unwrap();
            let ms: u64 = lua
                .load(
                    r#"
                    local time = require("std:time")
                    return time.sleep(time.Duration.from_millis(50)):as_millis()
                "#,
                )
                .eval_async()
                .await
                .unwrap();
            assert!(ms >= 50, "slept {ms}ms < 50ms");
        });
    }

    #[test]
    fn sleep_takes_a_duration_only() {
        rt().block_on(async {
            let lua = make_engine().unwrap();
            let err = lua
                .load(r#"return require("std:time").sleep(5)"#)
                .eval_async::<mlua::Value>()
                .await
                .expect_err("a number is not a Duration")
                .to_string();
            assert!(err.contains("std:time.sleep: argument #1 'duration'"), "{err}");
        });
    }
}
