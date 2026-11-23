//! Monotonic clock — `std:time` (mirrors `std::time`).
//!
//! Two UserData types: `Instant` (opaque monotonic point) and `Duration`
//! (typed time interval). Independent of wall-clock — survives NTP slews,
//! DST jumps, manual clock adjustments. Arithmetic saturates instead of
//! panicking so scripts can't blow up on overflow or negative results.
//!
//! Surface:
//! - `Instant.now()` / flat `time.now()` — current monotonic point
//! - `Instant:elapsed()` — `Duration` since the instant was taken
//! - `Instant:duration_since(other)` — saturating diff
//! - `Instant ± Duration` → `Instant`, `Instant - Instant` → `Duration`
//! - `Duration.from_{secs,millis,micros,nanos,secs_f64}` constructors
//! - `Duration.parse(s)` / `tostring(d)` — Go-style "1h30m", "1.5s", "500ms"
//! - `Duration:as_{secs,millis,micros,nanos,secs_f64}` accessors
//! - `Duration ± Duration`, `Duration * number`, `Duration / number`
//! - `time.sleep(Duration)` — async, mirrors `std::thread::sleep`

use std::cmp::Ordering;
use std::time::{Duration as StdDuration, Instant as StdInstant};

use mlua::{AnyUserData, Lua, MetaMethod, Table, UserData, UserDataMethods, Value};

use crate::lua::facade::{Handle, Module};
use crate::lua::lifecycle::{Role, async_function};
use crate::lua::sandbox::{Fallible, err, ok};

// ---------------------------------------------------------------------------
// Duration UserData
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct DurationHandle(pub StdDuration);

#[neoc_macros::lua_userdata("std:time", "Duration")]
impl UserData for DurationHandle {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        /// Whole seconds component (integer).
        methods.add_method("as_secs", |_, this, ()| Ok(this.0.as_secs() as i64));
        /// Total milliseconds (integer, saturating).
        methods.add_method("as_millis", |_, this, ()| {
            Ok(u128_to_i64_saturating(this.0.as_millis()))
        });
        /// Total microseconds (integer, saturating).
        methods.add_method("as_micros", |_, this, ()| {
            Ok(u128_to_i64_saturating(this.0.as_micros()))
        });
        /// Total nanoseconds (integer, saturating).
        methods.add_method("as_nanos", |_, this, ()| {
            Ok(u128_to_i64_saturating(this.0.as_nanos()))
        });
        /// Total seconds as a float.
        methods.add_method("as_secs_f64", |_, this, ()| Ok(this.0.as_secs_f64()));
        /// True if the duration is exactly zero.
        methods.add_method("is_zero", |_, this, ()| Ok(this.0.is_zero()));

        // Saturating arithmetic — Rust's `+`/`-` panic on overflow/underflow,
        // which is a footgun in a scripting context. Saturating keeps scripts
        // alive at the cost of silently capping at MAX or ZERO.
        methods.add_meta_function(MetaMethod::Add, |_, args: (AnyUserData, AnyUserData)| {
            let (a, b) = borrow_pair::<DurationHandle>(args, "Duration")?;
            Ok(DurationHandle(a.0.saturating_add(b.0)))
        });
        methods.add_meta_function(MetaMethod::Sub, |_, args: (AnyUserData, AnyUserData)| {
            let (a, b) = borrow_pair::<DurationHandle>(args, "Duration")?;
            Ok(DurationHandle(a.0.saturating_sub(b.0)))
        });

        // Scalar mul/div — accept f64 to match Lua's number type. Reject
        // NaN, negative, and infinity so the result is always a valid
        // non-negative Duration. Silently producing zero on negative input
        // would mask user error.
        //
        // `__mul` is commutative (`2 * d` and `d * 2` both work); `add_meta_function`
        // is needed because `add_meta_method` would bind `self` to whichever side
        // Lua dispatches from, and `(number, Duration)` fails the userdata coerce.
        methods.add_meta_function(MetaMethod::Mul, |_, args: (Value, Value)| {
            let (dur, factor) = split_duration_scalar(args, "time.Duration.__mul")?;
            scale_duration(dur, factor, "time.Duration.__mul").map(DurationHandle)
        });
        methods.add_meta_method(MetaMethod::Div, |_, this, divisor: f64| {
            if divisor == 0.0 {
                return Err(mlua::Error::external(
                    "time.Duration.__div: division by zero",
                ));
            }
            scale_duration(this.0, 1.0 / divisor, "time.Duration.__div").map(DurationHandle)
        });

        methods.add_meta_function(MetaMethod::Lt, |_, args: (AnyUserData, AnyUserData)| {
            cmp_duration_pair(args).map(|o| o == Ordering::Less)
        });
        methods.add_meta_function(MetaMethod::Le, |_, args: (AnyUserData, AnyUserData)| {
            cmp_duration_pair(args).map(|o| o != Ordering::Greater)
        });
        methods.add_meta_function(MetaMethod::Eq, |_, args: (AnyUserData, AnyUserData)| {
            cmp_duration_pair(args).map(|o| o == Ordering::Equal)
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format_duration(this.0))
        });
    }
}

/// Copy the inner value out of each userdata before taking the next borrow.
/// mlua's UserData borrow is exclusive — holding the first borrow while
/// attempting the second on the *same* underlying userdata (e.g. `a <= a`, or
/// `table.sort` with adjacent equal entries) would fail with a borrow conflict.
fn borrow_pair<T: mlua::UserData + Copy + 'static>(
    (a, b): (AnyUserData, AnyUserData),
    type_label: &'static str,
) -> mlua::Result<(T, T)> {
    let av = *a
        .borrow::<T>()
        .map_err(|_| mlua::Error::external(format!("time: expected {type_label} on left")))?;
    let bv = *b
        .borrow::<T>()
        .map_err(|_| mlua::Error::external(format!("time: expected {type_label} on right")))?;
    Ok((av, bv))
}

fn cmp_duration_pair(args: (AnyUserData, AnyUserData)) -> mlua::Result<Ordering> {
    let (a, b) = borrow_pair::<DurationHandle>(args, "Duration")?;
    Ok(a.0.cmp(&b.0))
}

/// Pick the Duration and scalar from either argument order. Used by `__mul`
/// to support `2 * d` and `d * 2` symmetrically — Lua dispatches via
/// Duration's metatable in either case, but the userdata can land in either slot.
fn split_duration_scalar(
    (a, b): (Value, Value),
    ctx: &'static str,
) -> mlua::Result<(StdDuration, f64)> {
    let (dur_v, num_v) = match (&a, &b) {
        (Value::UserData(ud), _) if ud.is::<DurationHandle>() => (&a, &b),
        (_, Value::UserData(ud)) if ud.is::<DurationHandle>() => (&b, &a),
        _ => {
            return Err(mlua::Error::external(format!(
                "{ctx}: expected Duration and number"
            )));
        }
    };
    let dur = dur_v
        .as_userdata()
        .expect("duration side checked above")
        .borrow::<DurationHandle>()?
        .0;
    let factor = match num_v {
        Value::Number(n) => *n,
        Value::Integer(i) => *i as f64,
        _ => {
            return Err(mlua::Error::external(format!(
                "{ctx}: expected number operand"
            )));
        }
    };
    Ok((dur, factor))
}

/// Saturate a `u128` count (e.g. nanos, micros, millis) to `i64::MAX` instead
/// of wrapping. The `as` cast on its own truncates the low 64 bits and then
/// reinterprets as signed, so giant durations silently became negative.
fn u128_to_i64_saturating(v: u128) -> i64 {
    if v > i64::MAX as u128 {
        i64::MAX
    } else {
        v as i64
    }
}

fn scale_duration(d: StdDuration, factor: f64, ctx: &'static str) -> mlua::Result<StdDuration> {
    if !factor.is_finite() {
        return Err(mlua::Error::external(format!(
            "{ctx}: factor must be finite, got {factor}"
        )));
    }
    if factor < 0.0 {
        return Err(mlua::Error::external(format!(
            "{ctx}: factor must be non-negative, got {factor}"
        )));
    }
    StdDuration::try_from_secs_f64(d.as_secs_f64() * factor)
        .map_err(|e| mlua::Error::external(format!("{ctx}: {e}")))
}

// ---------------------------------------------------------------------------
// Instant UserData
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct InstantHandle(pub StdInstant);

#[neoc_macros::lua_userdata("std:time", "Instant")]
impl UserData for InstantHandle {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        /// Return the `Duration` elapsed since this `Instant` was taken (saturating).
        methods.add_method("elapsed", |_, this, ()| {
            Ok(DurationHandle(this.0.elapsed()))
        });
        // Plain `add_function` (not `add_method`) so `this:duration_since(this)`
        // doesn't deadlock on overlapping userdata borrows.
        /// Saturating difference `self - other`; returns `ZERO` if `other` is later.
        #[lua("Duration", method, takes("other: Instant"))]
        methods.add_function("duration_since", |_, args: (AnyUserData, AnyUserData)| {
            let (a, b) = args;
            let this = a
                .borrow::<InstantHandle>()
                .map_err(|_| {
                    mlua::Error::external("time.Instant.duration_since: expected Instant self")
                })?
                .0;
            let other = b
                .borrow::<InstantHandle>()
                .map_err(|_| {
                    mlua::Error::external("time.Instant.duration_since: expected Instant argument")
                })?
                .0;
            Ok(DurationHandle(this.saturating_duration_since(other)))
        });

        // Instant + Duration → Instant. Duration on the left dispatches here
        // too because Duration's __add rejects non-Duration operands; Lua
        // then re-attempts via the right metatable.
        methods.add_meta_function(MetaMethod::Add, |_, args: (Value, Value)| {
            let (instant, dur) = split_instant_duration(args, "time.Instant.__add")?;
            instant
                .checked_add(dur)
                .map(InstantHandle)
                .ok_or_else(|| mlua::Error::external("time.Instant.__add: overflow"))
        });

        // Instant - Duration → Instant   |   Instant - Instant → Duration.
        // The operand mix decides the return type. Duration on the left of an
        // Instant subtract is unreachable: Duration's __sub fires first.
        // Copy `left` out before touching `r` so `a - a` doesn't double-borrow.
        methods.add_meta_function(MetaMethod::Sub, |lua, args: (AnyUserData, AnyUserData)| {
            let (l, r) = args;
            let left = l
                .borrow::<InstantHandle>()
                .map_err(|_| mlua::Error::external("time.Instant.__sub: expected Instant on left"))?
                .0;
            if let Ok(other) = r.borrow::<InstantHandle>() {
                let diff = left.saturating_duration_since(other.0);
                return Ok(Value::UserData(lua.create_userdata(DurationHandle(diff))?));
            }
            if let Ok(dur) = r.borrow::<DurationHandle>() {
                let result = left
                    .checked_sub(dur.0)
                    .ok_or_else(|| mlua::Error::external("time.Instant.__sub: underflow"))?;
                return Ok(Value::UserData(lua.create_userdata(InstantHandle(result))?));
            }
            Err(mlua::Error::external(
                "time.Instant.__sub: right operand must be Instant or Duration",
            ))
        });

        methods.add_meta_function(MetaMethod::Lt, |_, args: (AnyUserData, AnyUserData)| {
            cmp_instant_pair(args).map(|o| o == Ordering::Less)
        });
        methods.add_meta_function(MetaMethod::Le, |_, args: (AnyUserData, AnyUserData)| {
            cmp_instant_pair(args).map(|o| o != Ordering::Greater)
        });
        methods.add_meta_function(MetaMethod::Eq, |_, args: (AnyUserData, AnyUserData)| {
            cmp_instant_pair(args).map(|o| o == Ordering::Equal)
        });
        methods.add_meta_method(MetaMethod::ToString, |_, _, ()| {
            // Opaque — the underlying nanos are not portable or stable, so
            // exposing them would be a footgun. Diff two Instants to get a
            // Duration if you need a number.
            Ok("Instant".to_string())
        });
    }
}

fn split_instant_duration(
    (a, b): (Value, Value),
    ctx: &'static str,
) -> mlua::Result<(StdInstant, StdDuration)> {
    let (instant_v, dur_v) = match (&a, &b) {
        (Value::UserData(ud), _) if ud.is::<InstantHandle>() => (&a, &b),
        (_, Value::UserData(ud)) if ud.is::<InstantHandle>() => (&b, &a),
        _ => {
            return Err(mlua::Error::external(format!(
                "{ctx}: expected Instant + Duration"
            )));
        }
    };
    let instant = instant_v
        .as_userdata()
        .expect("instant side checked above")
        .borrow::<InstantHandle>()?
        .0;
    let dur = dur_v
        .as_userdata()
        .ok_or_else(|| mlua::Error::external(format!("{ctx}: expected Duration operand")))?
        .borrow::<DurationHandle>()
        .map_err(|_| mlua::Error::external(format!("{ctx}: expected Duration operand")))?
        .0;
    Ok((instant, dur))
}

fn cmp_instant_pair(args: (AnyUserData, AnyUserData)) -> mlua::Result<Ordering> {
    let (a, b) = borrow_pair::<InstantHandle>(args, "Instant")?;
    Ok(a.0.cmp(&b.0))
}

// ---------------------------------------------------------------------------
// Duration parse/format — Go-style: sequence of `<number><unit>` segments.
// Units: ns, us / µs, ms, s, m, h. Each segment number may be fractional.
// The literal `"0"` is accepted as a zero-duration shortcut.
// ---------------------------------------------------------------------------

fn parse_duration(input: &str) -> Result<StdDuration, String> {
    if input.is_empty() {
        return Err("empty input".into());
    }
    if input == "0" {
        return Ok(StdDuration::ZERO);
    }

    let mut total = StdDuration::ZERO;
    let mut rest = input;
    let mut segments = 0usize;

    while !rest.is_empty() {
        let num_end = rest
            .char_indices()
            .find(|(_, c)| !c.is_ascii_digit() && *c != '.')
            .map(|(i, _)| i)
            .unwrap_or(rest.len());
        if num_end == 0 {
            return Err(format!("expected number at {rest:?}"));
        }
        let num_str = &rest[..num_end];
        let value: f64 = num_str
            .parse()
            .map_err(|_| format!("invalid number {num_str:?}"))?;
        if value < 0.0 {
            return Err("duration cannot be negative".into());
        }
        rest = &rest[num_end..];

        let nanos_per_unit: f64 = if let Some(stripped) = rest.strip_prefix("µs") {
            rest = stripped;
            1_000.0
        } else {
            let unit_end = rest
                .char_indices()
                .find(|(_, c)| !c.is_ascii_alphabetic())
                .map(|(i, _)| i)
                .unwrap_or(rest.len());
            if unit_end == 0 {
                return Err(format!("missing unit after {num_str:?}"));
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
                other => return Err(format!("unknown unit {other:?}")),
            }
        };

        let nanos = value * nanos_per_unit;
        if !nanos.is_finite() {
            return Err("duration overflow".into());
        }
        // `nanos as u64` saturates per Rust ≥1.45 cast semantics — silently caps
        // huge inputs at `u64::MAX` (~584 years) instead of failing. Bound check
        // so the caller hears about it.
        if nanos.round() > u64::MAX as f64 {
            return Err("duration overflow".into());
        }
        let segment = StdDuration::from_nanos(nanos.round() as u64);
        total = total
            .checked_add(segment)
            .ok_or_else(|| "duration overflow".to_string())?;
        segments += 1;
    }

    if segments == 0 {
        return Err("no segments parsed".into());
    }
    Ok(total)
}

/// Canonical text form: largest non-zero unit first, mirroring Go's
/// `Duration.String`. Sub-second values collapse to a single unit (`"500ms"`,
/// `"1.5s"`); >= 1s values walk down through hours/minutes/seconds with the
/// fractional remainder pinned to seconds.
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
        let frac = frac.trim_end_matches('0');
        out.push_str(&format!("{secs}.{frac}s"));
    }
    out
}

fn format_fraction(nanos: u128, divisor: u128, unit: &str) -> String {
    let whole = nanos / divisor;
    let rem = nanos % divisor;
    if rem == 0 {
        return format!("{whole}{unit}");
    }
    let frac_width = match divisor {
        1_000 => 3,
        1_000_000 => 6,
        _ => 0,
    };
    let frac = format!("{rem:0>frac_width$}");
    let frac = frac.trim_end_matches('0');
    format!("{whole}.{frac}{unit}")
}

// ---------------------------------------------------------------------------
// Module constructor
// ---------------------------------------------------------------------------

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Parse a human duration such as `1h30m` or `250ms`.
#[neoc_macros::lua_fn(name = "parse")]
fn duration_parse(_: &Lua, s: String) -> mlua::Result<Fallible<DurationHandle>> {
    match parse_duration(&s) {
        Ok(d) => ok(DurationHandle(d)),
        Err(msg) => err(format!("time.Duration.parse: {msg}")),
    }
}

/// The current monotonic point in time.
#[neoc_macros::lua_fn(name = "now")]
fn instant_now(_: &Lua, (): ()) -> mlua::Result<InstantHandle> {
    Ok(InstantHandle(StdInstant::now()))
}

/// Declares the `std:time` surface through the facade.
///
/// The `Duration` and `Instant` constructor sub-tables are built verbatim and
/// set with [`crate::lua::facade::Module::set`]: their constructors carried
/// **no** `#[instrument]` span and raise `mlua::Error` directly (or bake the
/// `"time.Duration.parse"` prefix), so routing through the facade's `type_` /
/// `func` would add a span and change behaviour. The flat `now` and `sleep`
/// functions likewise carried no span and are set verbatim. The `Duration` /
/// `Instant` userdata keep their own `impl UserData` and are recorded here via
/// [`crate::lua::facade::Module::userdata`]; `now` and `sleep` are read by `neoc_macros`
/// from the implementation.
#[neoc_macros::lua_module]
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "std:time")?;

    let dur = lua.create_table()?;
    /// The zero-length duration.
    #[lua(constant, on(m, "Duration"))]
    dur.set("ZERO", DurationHandle(StdDuration::ZERO))?;
    dur.set("from_secs", lua.create_function(from_secs)?)?;
    dur.set("from_millis", lua.create_function(from_millis)?)?;
    dur.set("from_micros", lua.create_function(from_micros)?)?;
    dur.set("from_nanos", lua.create_function(from_nanos)?)?;
    dur.set("from_secs_f64", lua.create_function(from_secs_f64)?)?;
    dur.set("parse", lua.create_function(duration_parse)?)?;
    m.set("Duration", dur)?;

    // `Instant.now` and the flat `time.now` alias share one closure so the
    // two surfaces can never diverge.
    let now_fn = lua.create_function(instant_now)?;
    let inst = lua.create_table()?;
    inst.set("now", now_fn.clone())?;
    m.set("Instant", inst)?;
    /// Flat alias for `Instant.now()` — the current monotonic point in time.
    #[lua("Instant", takes())]
    m.set("now", now_fn)?;

    // time.sleep(Duration) — async, mirrors `std::thread::sleep` but
    // cooperative. Returns the actual elapsed Duration so callers can
    // detect overruns.
    /// Cooperative sleep for `duration` — yields to other tasks while waiting. Returns the actual elapsed `Duration`.
    #[lua]
    m.set(
        "sleep",
        async_function(
            lua,
            Role::Timer,
            |_, duration: Handle<DurationHandle>| async move {
                let target = duration
                    .borrow::<DurationHandle>()
                    .map_err(|_| mlua::Error::external("time.sleep: expected Duration argument"))?
                    .0;
                let start = StdInstant::now();
                tokio::time::sleep(target).await;
                Ok(DurationHandle(start.elapsed()))
            },
        )?,
    )?;

    m.userdata("Duration", |d| {
        DurationHandle::lua(d);
        d.static_of(from_secs::lua())
            .static_of(from_millis::lua())
            .static_of(from_micros::lua())
            .static_of(from_nanos::lua())
            .static_of(from_secs_f64::lua())
            .static_of(duration_parse::lua());
    });

    m.userdata("Instant", |i| {
        InstantHandle::lua(i);
        i.static_of(instant_now::lua());
    });

    Ok(m)
}

fn reject_negative(value: i64, ctx: &'static str) -> mlua::Result<u64> {
    if value < 0 {
        return Err(mlua::Error::external(format!(
            "{ctx}: value must be non-negative"
        )));
    }
    Ok(value as u64)
}

/// A duration of `secs` whole seconds; negative raises.
#[neoc_macros::lua_fn]
fn from_secs(_: &Lua, secs: i64) -> mlua::Result<DurationHandle> {
    reject_negative(secs, "time.Duration.from_secs")
        .map(|u| DurationHandle(StdDuration::from_secs(u)))
}

/// A duration of `millis` milliseconds; negative raises.
#[neoc_macros::lua_fn]
fn from_millis(_: &Lua, millis: i64) -> mlua::Result<DurationHandle> {
    reject_negative(millis, "time.Duration.from_millis")
        .map(|u| DurationHandle(StdDuration::from_millis(u)))
}

/// A duration of `micros` microseconds; negative raises.
#[neoc_macros::lua_fn]
fn from_micros(_: &Lua, micros: i64) -> mlua::Result<DurationHandle> {
    reject_negative(micros, "time.Duration.from_micros")
        .map(|u| DurationHandle(StdDuration::from_micros(u)))
}

/// A duration of `nanos` nanoseconds; negative raises.
#[neoc_macros::lua_fn]
fn from_nanos(_: &Lua, nanos: i64) -> mlua::Result<DurationHandle> {
    reject_negative(nanos, "time.Duration.from_nanos")
        .map(|u| DurationHandle(StdDuration::from_nanos(u)))
}

/// A duration of `secs` seconds as a float; negative or non-finite raises.
#[neoc_macros::lua_fn]
fn from_secs_f64(_: &Lua, secs: f64) -> mlua::Result<DurationHandle> {
    if !secs.is_finite() {
        return Err(mlua::Error::external(
            "time.Duration.from_secs_f64: value must be finite",
        ));
    }
    if secs < 0.0 {
        return Err(mlua::Error::external(
            "time.Duration.from_secs_f64: value must be non-negative",
        ));
    }
    StdDuration::try_from_secs_f64(secs)
        .map(DurationHandle)
        .map_err(|e| mlua::Error::external(format!("time.Duration.from_secs_f64: {e}")))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lua::sandbox::make_engine;

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Runtime::new().unwrap()
    }

    #[test]
    fn parse_basic_units() {
        assert_eq!(parse_duration("0").unwrap(), StdDuration::ZERO);
        assert_eq!(
            parse_duration("500ms").unwrap(),
            StdDuration::from_millis(500)
        );
        assert_eq!(parse_duration("2s").unwrap(), StdDuration::from_secs(2));
        assert_eq!(parse_duration("1m30s").unwrap(), StdDuration::from_secs(90));
        assert_eq!(parse_duration("1h").unwrap(), StdDuration::from_secs(3_600));
        assert_eq!(
            parse_duration("1h30m").unwrap(),
            StdDuration::from_secs(5_400)
        );
        assert_eq!(
            parse_duration("1.5s").unwrap(),
            StdDuration::from_millis(1_500)
        );
        assert_eq!(
            parse_duration("500us").unwrap(),
            StdDuration::from_micros(500)
        );
        assert_eq!(
            parse_duration("500µs").unwrap(),
            StdDuration::from_micros(500)
        );
        assert_eq!(
            parse_duration("250ns").unwrap(),
            StdDuration::from_nanos(250)
        );
    }

    #[test]
    fn parse_rejects_bad_input() {
        assert!(parse_duration("").is_err());
        assert!(parse_duration("5").is_err()); // missing unit
        assert!(parse_duration("s").is_err()); // missing number
        assert!(parse_duration("5xs").is_err()); // unknown unit
        assert!(parse_duration("-5s").is_err());
    }

    #[test]
    fn format_roundtrip() {
        let cases = [
            "500ns", "500µs", "500ms", "2s", "1.5s", "1m30s", "1h", "1h30m", "0s",
        ];
        for c in cases {
            let parsed = parse_duration(c).unwrap_or_else(|e| panic!("parse {c}: {e}"));
            let formatted = format_duration(parsed);
            let reparsed =
                parse_duration(&formatted).unwrap_or_else(|e| panic!("reparse {formatted}: {e}"));
            assert_eq!(parsed, reparsed, "roundtrip mismatch: {c} → {formatted}");
        }
    }

    #[test]
    fn duration_arithmetic_saturates() {
        let lua = make_engine().unwrap();
        let (sum_ms, diff_ms): (i64, i64) = lua
            .load(
                r#"
                local time = require("std:time")
                local a = time.Duration.from_millis(500)
                local b = time.Duration.from_millis(300)
                local sum = a + b
                local diff = b - a   -- underflow → ZERO
                return sum:as_millis(), diff:as_millis()
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(sum_ms, 800);
        assert_eq!(diff_ms, 0);
    }

    #[test]
    fn instant_diff_gives_duration() {
        rt().block_on(async {
            let lua = make_engine().unwrap();
            let ms: i64 = lua
                .load(
                    r#"
                    local time = require("std:time")
                    local task = require("std:task")
                    local h = task.spawn(function()
                        local t0 = time.now()
                        task.wait(0.05)
                        local elapsed = time.now() - t0
                        return elapsed:as_millis()
                    end)
                    return h:await()
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
            let ms: i64 = lua
                .load(
                    r#"
                    local time = require("std:time")
                    local task = require("std:task")
                    local h = task.spawn(function()
                        local d = time.Duration.from_millis(50)
                        local actual = time.sleep(d)
                        return actual:as_millis()
                    end)
                    return h:await()
                "#,
                )
                .eval_async()
                .await
                .unwrap();
            assert!(ms >= 50, "slept {ms}ms < 50ms");
        });
    }

    #[test]
    fn instant_plus_duration_is_instant() {
        let lua = make_engine().unwrap();
        let diff_ms: i64 = lua
            .load(
                r#"
                local time = require("std:time")
                local t0 = time.now()
                local later = t0 + time.Duration.from_millis(100)
                return (later - t0):as_millis()
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(diff_ms, 100);
    }

    #[test]
    fn duration_mul_div() {
        let lua = make_engine().unwrap();
        let (a, b): (i64, i64) = lua
            .load(
                r#"
                local time = require("std:time")
                local d = time.Duration.from_secs(2)
                return (d * 1.5):as_millis(), (d / 4):as_millis()
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(a, 3_000);
        assert_eq!(b, 500);
    }

    #[test]
    fn duration_tostring_matches_parse() {
        let lua = make_engine().unwrap();
        let formatted: String = lua
            .load(
                r#"
                local time = require("std:time")
                return tostring(time.Duration.parse("1m30s"))
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(formatted, "1m30s");
    }

    #[test]
    fn mul_is_commutative() {
        let lua = make_engine().unwrap();
        let (left, right, frac): (i64, i64, i64) = lua
            .load(
                r#"
                local time = require("std:time")
                local d = time.Duration.from_secs(1)
                return (d * 2):as_millis(), (2 * d):as_millis(), (0.5 * d):as_millis()
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(left, 2_000);
        assert_eq!(right, 2_000);
        assert_eq!(frac, 500);
    }

    #[test]
    fn as_nanos_saturates_instead_of_wrapping() {
        let lua = make_engine().unwrap();
        let (nanos, micros, millis): (i64, i64, i64) = lua
            .load(
                r#"
                local time = require("std:time")
                -- 1e15 ns ≈ 11.5 days; safe in i64. Pick a value that would wrap
                -- if u128 → i64 truncated (anything > 2^63 nanos ≈ 292 years).
                local d = time.Duration.parse("9000h")  -- ~3.24e16 ns, still in i64
                return d:as_nanos(), d:as_micros(), d:as_millis()
            "#,
            )
            .eval()
            .unwrap();
        assert!(nanos > 0, "as_nanos went negative: {nanos}");
        assert!(micros > 0);
        assert!(millis > 0);
    }

    #[test]
    fn parse_rejects_overflow() {
        let err = parse_duration("99999999999999999999h").unwrap_err();
        assert!(
            err.contains("overflow"),
            "expected overflow error, got: {err}"
        );
    }

    #[test]
    fn negative_duration_constructor_errors() {
        let lua = make_engine().unwrap();
        let result = lua
            .load(
                r#"
                local time = require("std:time")
                return time.Duration.from_secs(-1)
            "#,
            )
            .eval::<mlua::Value>();
        let err = result.expect_err("from_secs(-1) must error");
        let msg = err.to_string();
        assert!(
            msg.contains("non-negative"),
            "expected non-negative error, got: {msg}"
        );
    }
}
