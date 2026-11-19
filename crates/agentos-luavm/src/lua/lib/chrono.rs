//! Calendar dates, times, and timezones — `lib:chrono`.
//!
//! Five `UserData` types — `Date`, `Time`, `DateTime`, `ZonedDateTime`,
//! `Duration` — mirror the `java.time` / JS `Temporal` split. Civil types
//! carry no zone; `ZonedDateTime` pairs an instant with a named IANA zone.
//! `Duration` wraps `jiff::Span` so calendar units (days, months, years) are
//! preserved through arithmetic on `Date`/`DateTime` operands.
//!
//! Flat helpers (`now`, `utc_now`, `parse`) layer scripting ergonomics over
//! the `UserData` layer — they delegate, never re-decide. `parse` recognises
//! RFC 3339 / ISO 8601 inputs and dispatches to the matching civil or zoned
//! type.
//!
//! Backed by the `jiff` crate. See docs/src/lib/reference/chrono.md for the
//! public surface contract.

use std::str::FromStr;

use jiff::civil::{Date, DateTime, Time};
use jiff::tz::TimeZone;
use jiff::{Span, Timestamp, Unit, Zoned};
use mlua::{AnyUserData, Lua, MetaMethod, Table, UserData, UserDataFields, UserDataMethods, Value};
use tracing::instrument;

use crate::lua::facade::Module;
use crate::lua::sandbox::{err, ok};

// ---------------------------------------------------------------------------
// Module
// ---------------------------------------------------------------------------

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `lib:chrono` surface through the facade.
///
/// The five constructor sub-tables (`Date`, `Time`, `DateTime`,
/// `ZonedDateTime`, `Duration`) are set verbatim with
/// [`crate::lua::facade::Module::set`]: their hand-written constructors carry
/// `chrono.<Type>.<ctor>`-named spans the facade's `type_` would rename to
/// `<Type>.<ctor>`, so routing them through the facade would change the trace
/// shape. Each type keeps its own `impl UserData` and is recorded here for the
/// header via [`crate::lua::facade::Module::userdata`]. The three flat helpers
/// (`now`, `utc_now`, `parse`) are routed through the facade — their
/// `chrono.<fn>` spans match what the facade emits, so behaviour stays
/// byte-identical.
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "lib:chrono")?;

    let date_sub = lua.create_table()?;
    date_sub.set("new", lua.create_function(date_new)?)?;
    date_sub.set("parse", lua.create_function(date_parse_ctor)?)?;
    m.set("Date", date_sub)?;

    let time_sub = lua.create_table()?;
    time_sub.set("new", lua.create_function(time_new)?)?;
    time_sub.set("parse", lua.create_function(time_parse_ctor)?)?;
    m.set("Time", time_sub)?;

    let dt_sub = lua.create_table()?;
    dt_sub.set("new", lua.create_function(datetime_new)?)?;
    dt_sub.set("parse", lua.create_function(datetime_parse_ctor)?)?;
    m.set("DateTime", dt_sub)?;

    let zdt_sub = lua.create_table()?;
    zdt_sub.set("parse", lua.create_function(zdt_parse_ctor)?)?;
    m.set("ZonedDateTime", zdt_sub)?;

    let dur_sub = lua.create_table()?;
    dur_sub.set("of", lua.create_function(duration_of)?)?;
    dur_sub.set("parse", lua.create_function(duration_parse_ctor)?)?;
    m.set("Duration", dur_sub)?;

    // `now` / `utc_now` return a `ZonedDateTime` userdata handle — not a
    // facade `DeclType` — and carry `chrono.now` / `chrono.utc_now` spans, so
    // they are set verbatim with their `#[instrument]` attributes intact.
    //
    // `parse` dispatches across handle types and routes a recognition failure
    // to the `(value, error)` channel, but the chosen handle's `into_lua` (done
    // by `ok`) raises an `mlua::Error` directly — a mix `try_func` cannot model
    // — so it too is set verbatim and recorded for the header.
    m.set("now", lua.create_function(flat_now)?)?
        .set("utc_now", lua.create_function(flat_utc_now)?)?
        .set("parse", lua.create_function(flat_parse)?)?;

    m.record_fn(
        "now",
        &[],
        "ZonedDateTime",
        "Current wall-clock instant in the system's local timezone.",
    )
    .record_fn(
        "utc_now",
        &[],
        "ZonedDateTime",
        "Current wall-clock instant in UTC.",
    )
    .record_fn(
        "parse",
        &[("s", "string")],
        "Result<LuaValue, string>",
        "Recognise an ISO 8601 / RFC 9557 string and dispatch it to the matching `Date`, `Time`, `DateTime`, or `ZonedDateTime`.",
    );

    record_userdata(&mut m);

    Ok(m)
}

/// Records the five chrono userdata types' fields, methods and constructors for the
/// emitted `LuaCATS` definitions. The runtime registration of each type stays in its
/// own `impl UserData`; this only mirrors the surface so the definitions describe
/// it. Fields (`date.year`) are recorded with `field`, constructors with `static_fn`.
fn record_userdata(m: &mut Module<'_>) {
    m.userdata("Date", |t| {
        t.field("year", "number", "Calendar year.")
            .field("month", "number", "Month of year (1-12).")
            .field("day", "number", "Day of month (1-31).")
            .field("weekday", "number", "Day of week, Monday = 1.")
            .field("ordinal", "number", "Day of year (1-366).")
            .method(
                "with_year",
                &[("year", "number")],
                "Result<Date, string>",
                "Return a copy with the year replaced.",
            )
            .method(
                "with_month",
                &[("month", "number")],
                "Result<Date, string>",
                "Return a copy with the month replaced.",
            )
            .method(
                "with_day",
                &[("day", "number")],
                "Result<Date, string>",
                "Return a copy with the day replaced.",
            )
            .method("at", &[("time", "Time")], "DateTime", "Combine with a `Time` into a `DateTime`.")
            .method(
                "add",
                &[("duration", "Duration")],
                "Result<Date, string>",
                "Add a day-resolution `Duration`.",
            )
            .method(
                "sub",
                &[("duration", "Duration")],
                "Result<Date, string>",
                "Subtract a day-resolution `Duration`.",
            )
            .method(
                "diff",
                &[("other", "Date")],
                "Result<Duration, string>",
                "Calendar span from this date until `other`.",
            )
            .method(
                "format",
                &[("fmt", "string")],
                "Result<string, string>",
                "Format via a strftime pattern.",
            )
            .static_fn(
                "new",
                &[("year", "number"), ("month", "number"), ("day", "number")],
                "Result<Date, string>",
                "Build a date from year, month and day; an out-of-range value is an error.",
            )
            .static_fn(
                "parse",
                &[("s", "string")],
                "Result<Date, string>",
                "Parse an ISO 8601 date (`2026-05-20`).",
            );
    });

    m.userdata("Time", |t| {
        t.field("hour", "number", "Hour of day (0-23).")
            .field("minute", "number", "Minute of hour (0-59).")
            .field("second", "number", "Second of minute (0-59).")
            .field("nanosecond", "number", "Sub-second nanoseconds.")
            .method("on", &[("date", "Date")], "DateTime", "Combine with a `Date` into a `DateTime`.")
            .method("add", &[("duration", "Duration")], "Result<Time, string>", "Add a `Duration`.")
            .method("sub", &[("duration", "Duration")], "Result<Time, string>", "Subtract a `Duration`.")
            .method(
                "diff",
                &[("other", "Time")],
                "Result<Duration, string>",
                "Span from this time until `other`.",
            )
            .method(
                "format",
                &[("fmt", "string")],
                "Result<string, string>",
                "Format via a strftime pattern.",
            )
            .static_fn(
                "new",
                &[("hour", "number"), ("minute", "number"), ("second", "number"), ("nanosecond", "number?")],
                "Result<Time, string>",
                "Build a time of day; an out-of-range value is an error.",
            )
            .static_fn(
                "parse",
                &[("s", "string")],
                "Result<Time, string>",
                "Parse an ISO 8601 time (`13:45:30`).",
            );
    });

    m.userdata("DateTime", |t| {
        t.field("year", "number", "Calendar year.")
            .field("month", "number", "Month of year (1-12).")
            .field("day", "number", "Day of month (1-31).")
            .field("hour", "number", "Hour of day (0-23).")
            .field("minute", "number", "Minute of hour (0-59).")
            .field("second", "number", "Second of minute (0-59).")
            .field("nanosecond", "number", "Sub-second nanoseconds.")
            .method("date", &[], "Date", "The civil `Date` component.")
            .method("time", &[], "Time", "The civil `Time` component.")
            .method(
                "in_zone",
                &[("name", "string")],
                "Result<ZonedDateTime, string>",
                "Interpret this civil datetime in the named IANA zone.",
            )
            .method("add", &[("duration", "Duration")], "Result<DateTime, string>", "Add a `Duration`.")
            .method("sub", &[("duration", "Duration")], "Result<DateTime, string>", "Subtract a `Duration`.")
            .method(
                "diff",
                &[("other", "DateTime")],
                "Result<Duration, string>",
                "Span from this datetime until `other`.",
            )
            .method(
                "format",
                &[("fmt", "string")],
                "Result<string, string>",
                "Format via a strftime pattern.",
            )
            .static_fn(
                "new",
                &[
                    ("year", "number"),
                    ("month", "number"),
                    ("day", "number"),
                    ("hour", "number"),
                    ("minute", "number"),
                    ("second", "number"),
                    ("nanosecond", "number?"),
                ],
                "Result<DateTime, string>",
                "Build a civil datetime; an out-of-range value is an error.",
            )
            .static_fn(
                "parse",
                &[("s", "string")],
                "Result<DateTime, string>",
                "Parse an ISO 8601 datetime (`2026-05-20T13:45:30`).",
            );
    });

    m.userdata("ZonedDateTime", |t| {
        t.field("year", "number", "Calendar year.")
            .field("month", "number", "Month of year (1-12).")
            .field("day", "number", "Day of month (1-31).")
            .field("hour", "number", "Hour of day (0-23).")
            .field("minute", "number", "Minute of hour (0-59).")
            .field("second", "number", "Second of minute (0-59).")
            .field("nanosecond", "number", "Sub-second nanoseconds.")
            .field("timezone", "string?", "IANA zone name, if any.")
            .field("offset_seconds", "number", "UTC offset in seconds.")
            .field("epoch_seconds", "number", "Seconds since the Unix epoch.")
            .method("date", &[], "Date", "The civil `Date` component.")
            .method("time", &[], "Time", "The civil `Time` component.")
            .method("datetime", &[], "DateTime", "The civil `DateTime` component.")
            .method(
                "in_zone",
                &[("name", "string")],
                "Result<ZonedDateTime, string>",
                "Convert to the named IANA zone, preserving the instant.",
            )
            .method("add", &[("duration", "Duration")], "Result<ZonedDateTime, string>", "Add a `Duration`.")
            .method(
                "sub",
                &[("duration", "Duration")],
                "Result<ZonedDateTime, string>",
                "Subtract a `Duration`.",
            )
            .method(
                "diff",
                &[("other", "ZonedDateTime")],
                "Result<Duration, string>",
                "Span from this instant until `other`.",
            )
            .method(
                "format",
                &[("fmt", "string")],
                "Result<string, string>",
                "Format via a strftime pattern.",
            )
            .method("to_rfc3339", &[], "string", "RFC 3339 timestamp string.")
            .static_fn(
                "parse",
                &[("s", "string")],
                "Result<ZonedDateTime, string>",
                "Parse an RFC 9557 timestamp with a zone (`2026-05-20T13:45:30+01:00[Europe/London]`).",
            );
    });

    m.userdata("Duration", |t| {
        t.field("years", "number", "Named years component.")
            .field("months", "number", "Named months component.")
            .field("weeks", "number", "Named weeks component.")
            .field("days", "number", "Named days component.")
            .field("hours", "number", "Named hours component.")
            .field("minutes", "number", "Named minutes component.")
            .field("seconds", "number", "Named seconds component.")
            .field("milliseconds", "number", "Named milliseconds component.")
            .field("microseconds", "number", "Named microseconds component.")
            .field("nanoseconds", "number", "Named nanoseconds component.")
            .method(
                "total_seconds",
                &[],
                "Result<number, string>",
                "Total span in seconds (calendar-agnostic units only).",
            )
            .method("negate", &[], "Duration", "Return the sign-flipped span.")
            .method("abs", &[], "Duration", "Return the absolute span.")
            .static_fn(
                "of",
                &[("units", "LuaValue")],
                "Result<Duration, string>",
                "Build a span from a table of named units, e.g. `{ hours = 1, minutes = 30 }`.",
            )
            .static_fn(
                "parse",
                &[("s", "string")],
                "Result<Duration, string>",
                "Parse an ISO 8601 or friendly duration (`PT1H30M`, `1h 30m`).",
            );
    });
}

// ---------------------------------------------------------------------------
// Date UserData
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct DateHandle(pub Date);

impl UserData for DateHandle {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("year", |_, this| Ok(i64::from(this.0.year())));
        fields.add_field_method_get("month", |_, this| Ok(i64::from(this.0.month())));
        fields.add_field_method_get("day", |_, this| Ok(i64::from(this.0.day())));
        fields.add_field_method_get("weekday", |_, this| {
            Ok(i64::from(this.0.weekday().to_monday_one_offset()))
        });
        fields.add_field_method_get("ordinal", |_, this| Ok(i64::from(this.0.day_of_year())));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("with_year", |lua, this, y: i64| {
            let y = match clamp_i16(y, "chrono.Date.with_year") {
                Ok(v) => v,
                Err(m) => return err(lua, m),
            };
            match this.0.with().year(y).build() {
                Ok(d) => ok(lua, DateHandle(d)),
                Err(e) => err(lua, format!("chrono.Date.with_year: {e}")),
            }
        });
        methods.add_method("with_month", |lua, this, m: i64| {
            let m = match clamp_i8(m, "chrono.Date.with_month") {
                Ok(v) => v,
                Err(msg) => return err(lua, msg),
            };
            match this.0.with().month(m).build() {
                Ok(d) => ok(lua, DateHandle(d)),
                Err(e) => err(lua, format!("chrono.Date.with_month: {e}")),
            }
        });
        methods.add_method("with_day", |lua, this, d: i64| {
            let d = match clamp_i8(d, "chrono.Date.with_day") {
                Ok(v) => v,
                Err(m) => return err(lua, m),
            };
            match this.0.with().day(d).build() {
                Ok(d) => ok(lua, DateHandle(d)),
                Err(e) => err(lua, format!("chrono.Date.with_day: {e}")),
            }
        });

        methods.add_method("at", |_, this, t: AnyUserData| {
            let th = t.borrow::<TimeHandle>()?;
            Ok(DateTimeHandle(this.0.to_datetime(th.0)))
        });

        methods.add_method("add", |lua, this, d: AnyUserData| {
            let span = match d.borrow::<DurationHandle>() {
                Ok(h) => h.0,
                Err(_) => return err(lua, "chrono.Date.add: expected Duration".to_string()),
            };
            if span_has_sub_day_units(&span) {
                return err(
                    lua,
                    "chrono.Date.add: Duration carries sub-day units; Date is day-resolution".to_string(),
                );
            }
            match this.0.checked_add(span) {
                Ok(r) => ok(lua, DateHandle(r)),
                Err(e) => err(lua, format!("chrono.Date.add: {e}")),
            }
        });
        methods.add_method("sub", |lua, this, d: AnyUserData| {
            let span = match d.borrow::<DurationHandle>() {
                Ok(h) => h.0,
                Err(_) => return err(lua, "chrono.Date.sub: expected Duration".to_string()),
            };
            if span_has_sub_day_units(&span) {
                return err(
                    lua,
                    "chrono.Date.sub: Duration carries sub-day units; Date is day-resolution".to_string(),
                );
            }
            match this.0.checked_sub(span) {
                Ok(r) => ok(lua, DateHandle(r)),
                Err(e) => err(lua, format!("chrono.Date.sub: {e}")),
            }
        });

        // Named `diff` rather than `until` because `until` is a Lua keyword
        // (the `repeat … until` loop) and rejected as a method name.
        methods.add_method("diff", |lua, this, other: AnyUserData| {
            let o = match other.borrow::<DateHandle>() {
                Ok(h) => h.0,
                Err(_) => return err(lua, "chrono.Date.diff: expected Date".to_string()),
            };
            match this.0.until(o) {
                Ok(s) => ok(lua, DurationHandle(s)),
                Err(e) => err(lua, format!("chrono.Date.diff: {e}")),
            }
        });

        methods.add_method("format", |lua, this, fmt: String| {
            // jiff::fmt::strtime::format is fallible; Date::strftime returns a
            // `DisplayWithFormat` whose `Display` impl can return `fmt::Error`
            // and panics inside `to_string()` for unparseable patterns.
            match jiff::fmt::strtime::format(&fmt, this.0) {
                Ok(s) => ok(lua, s),
                Err(e) => err(lua, format!("chrono.Date.format: {e}")),
            }
        });

        methods.add_meta_function(MetaMethod::Eq, |_, args: (AnyUserData, AnyUserData)| {
            let a = args.0.borrow::<DateHandle>()?.0;
            let b = args.1.borrow::<DateHandle>()?.0;
            Ok(a == b)
        });
        methods.add_meta_function(MetaMethod::Lt, |_, args: (AnyUserData, AnyUserData)| {
            let a = args.0.borrow::<DateHandle>()?.0;
            let b = args.1.borrow::<DateHandle>()?.0;
            Ok(a < b)
        });
        methods.add_meta_function(MetaMethod::Le, |_, args: (AnyUserData, AnyUserData)| {
            let a = args.0.borrow::<DateHandle>()?.0;
            let b = args.1.borrow::<DateHandle>()?.0;
            Ok(a <= b)
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, (): ()| Ok(this.0.to_string()));
    }
}

#[instrument(name = "chrono.Date.new", target = "lua::lib::chrono", skip_all)]
fn date_new(lua: &Lua, (y, m, d): (i64, i64, i64)) -> mlua::Result<(Value, Value)> {
    let y = match clamp_i16(y, "chrono.Date.new") {
        Ok(v) => v,
        Err(msg) => return err(lua, msg),
    };
    let m = match clamp_i8(m, "chrono.Date.new") {
        Ok(v) => v,
        Err(msg) => return err(lua, msg),
    };
    let d = match clamp_i8(d, "chrono.Date.new") {
        Ok(v) => v,
        Err(msg) => return err(lua, msg),
    };
    match Date::new(y, m, d) {
        Ok(date) => ok(lua, DateHandle(date)),
        Err(e) => err(lua, format!("chrono.Date.new: {e}")),
    }
}

#[instrument(name = "chrono.Date.parse", target = "lua::lib::chrono", skip_all)]
fn date_parse_ctor(lua: &Lua, s: String) -> mlua::Result<(Value, Value)> {
    match Date::from_str(&s) {
        Ok(d) => ok(lua, DateHandle(d)),
        Err(e) => err(lua, format!("chrono.Date.parse: {e}")),
    }
}

// ---------------------------------------------------------------------------
// Time UserData
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct TimeHandle(pub Time);

impl UserData for TimeHandle {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("hour", |_, this| Ok(i64::from(this.0.hour())));
        fields.add_field_method_get("minute", |_, this| Ok(i64::from(this.0.minute())));
        fields.add_field_method_get("second", |_, this| Ok(i64::from(this.0.second())));
        fields.add_field_method_get("nanosecond", |_, this| Ok(i64::from(this.0.subsec_nanosecond())));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("on", |_, this, d: AnyUserData| {
            let dh = d.borrow::<DateHandle>()?;
            Ok(DateTimeHandle(dh.0.to_datetime(this.0)))
        });
        methods.add_method("format", |lua, this, fmt: String| {
            match jiff::fmt::strtime::format(&fmt, this.0) {
                Ok(s) => ok(lua, s),
                Err(e) => err(lua, format!("chrono.Time.format: {e}")),
            }
        });
        methods.add_meta_function(MetaMethod::Eq, |_, args: (AnyUserData, AnyUserData)| {
            let a = args.0.borrow::<TimeHandle>()?.0;
            let b = args.1.borrow::<TimeHandle>()?.0;
            Ok(a == b)
        });
        methods.add_meta_function(MetaMethod::Lt, |_, args: (AnyUserData, AnyUserData)| {
            let a = args.0.borrow::<TimeHandle>()?.0;
            let b = args.1.borrow::<TimeHandle>()?.0;
            Ok(a < b)
        });
        methods.add_meta_function(MetaMethod::Le, |_, args: (AnyUserData, AnyUserData)| {
            let a = args.0.borrow::<TimeHandle>()?.0;
            let b = args.1.borrow::<TimeHandle>()?.0;
            Ok(a <= b)
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, (): ()| Ok(this.0.to_string()));
    }
}

#[instrument(name = "chrono.Time.new", target = "lua::lib::chrono", skip_all)]
fn time_new(lua: &Lua, (h, m, s, ns): (i64, i64, i64, Option<i64>)) -> mlua::Result<(Value, Value)> {
    let h = match clamp_i8(h, "chrono.Time.new") {
        Ok(v) => v,
        Err(msg) => return err(lua, msg),
    };
    let m = match clamp_i8(m, "chrono.Time.new") {
        Ok(v) => v,
        Err(msg) => return err(lua, msg),
    };
    let s = match clamp_i8(s, "chrono.Time.new") {
        Ok(v) => v,
        Err(msg) => return err(lua, msg),
    };
    let ns = match ns {
        Some(v) => match clamp_i32(v, "chrono.Time.new") {
            Ok(n) => n,
            Err(msg) => return err(lua, msg),
        },
        None => 0,
    };
    match Time::new(h, m, s, ns) {
        Ok(t) => ok(lua, TimeHandle(t)),
        Err(e) => err(lua, format!("chrono.Time.new: {e}")),
    }
}

#[instrument(name = "chrono.Time.parse", target = "lua::lib::chrono", skip_all)]
fn time_parse_ctor(lua: &Lua, s: String) -> mlua::Result<(Value, Value)> {
    match Time::from_str(&s) {
        Ok(t) => ok(lua, TimeHandle(t)),
        Err(e) => err(lua, format!("chrono.Time.parse: {e}")),
    }
}

// ---------------------------------------------------------------------------
// DateTime UserData
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct DateTimeHandle(pub DateTime);

impl UserData for DateTimeHandle {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("year", |_, this| Ok(i64::from(this.0.year())));
        fields.add_field_method_get("month", |_, this| Ok(i64::from(this.0.month())));
        fields.add_field_method_get("day", |_, this| Ok(i64::from(this.0.day())));
        fields.add_field_method_get("hour", |_, this| Ok(i64::from(this.0.hour())));
        fields.add_field_method_get("minute", |_, this| Ok(i64::from(this.0.minute())));
        fields.add_field_method_get("second", |_, this| Ok(i64::from(this.0.second())));
        fields.add_field_method_get("nanosecond", |_, this| Ok(i64::from(this.0.subsec_nanosecond())));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("date", |_, this, (): ()| Ok(DateHandle(this.0.date())));
        methods.add_method("time", |_, this, (): ()| Ok(TimeHandle(this.0.time())));

        methods.add_method("in_zone", |lua, this, name: String| {
            let tz = match lookup_zone(&name) {
                Ok(z) => z,
                Err(msg) => return err(lua, format!("chrono.DateTime.in_zone: {msg}")),
            };
            match this.0.to_zoned(tz) {
                Ok(z) => ok(lua, ZonedDateTimeHandle(z)),
                Err(e) => err(lua, format!("chrono.DateTime.in_zone: {e}")),
            }
        });

        methods.add_method("add", |lua, this, d: AnyUserData| {
            let span = match d.borrow::<DurationHandle>() {
                Ok(h) => h.0,
                Err(_) => return err(lua, "chrono.DateTime.add: expected Duration".to_string()),
            };
            match this.0.checked_add(span) {
                Ok(r) => ok(lua, DateTimeHandle(r)),
                Err(e) => err(lua, format!("chrono.DateTime.add: {e}")),
            }
        });
        methods.add_method("sub", |lua, this, d: AnyUserData| {
            let span = match d.borrow::<DurationHandle>() {
                Ok(h) => h.0,
                Err(_) => return err(lua, "chrono.DateTime.sub: expected Duration".to_string()),
            };
            match this.0.checked_sub(span) {
                Ok(r) => ok(lua, DateTimeHandle(r)),
                Err(e) => err(lua, format!("chrono.DateTime.sub: {e}")),
            }
        });
        methods.add_method("diff", |lua, this, other: AnyUserData| {
            let o = match other.borrow::<DateTimeHandle>() {
                Ok(h) => h.0,
                Err(_) => return err(lua, "chrono.DateTime.diff: expected DateTime".to_string()),
            };
            match this.0.until(o) {
                Ok(s) => ok(lua, DurationHandle(s)),
                Err(e) => err(lua, format!("chrono.DateTime.diff: {e}")),
            }
        });

        methods.add_method("format", |lua, this, fmt: String| {
            match jiff::fmt::strtime::format(&fmt, this.0) {
                Ok(s) => ok(lua, s),
                Err(e) => err(lua, format!("chrono.DateTime.format: {e}")),
            }
        });

        methods.add_meta_function(MetaMethod::Eq, |_, args: (AnyUserData, AnyUserData)| {
            let a = args.0.borrow::<DateTimeHandle>()?.0;
            let b = args.1.borrow::<DateTimeHandle>()?.0;
            Ok(a == b)
        });
        methods.add_meta_function(MetaMethod::Lt, |_, args: (AnyUserData, AnyUserData)| {
            let a = args.0.borrow::<DateTimeHandle>()?.0;
            let b = args.1.borrow::<DateTimeHandle>()?.0;
            Ok(a < b)
        });
        methods.add_meta_function(MetaMethod::Le, |_, args: (AnyUserData, AnyUserData)| {
            let a = args.0.borrow::<DateTimeHandle>()?.0;
            let b = args.1.borrow::<DateTimeHandle>()?.0;
            Ok(a <= b)
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, (): ()| Ok(this.0.to_string()));
    }
}

#[instrument(name = "chrono.DateTime.new", target = "lua::lib::chrono", skip_all)]
fn datetime_new(
    lua: &Lua,
    args: (i64, i64, i64, i64, i64, i64, Option<i64>),
) -> mlua::Result<(Value, Value)> {
    let (y, mo, d, h, mi, s, ns) = args;
    let y = match clamp_i16(y, "chrono.DateTime.new") {
        Ok(v) => v,
        Err(m) => return err(lua, m),
    };
    let mo = match clamp_i8(mo, "chrono.DateTime.new") {
        Ok(v) => v,
        Err(m) => return err(lua, m),
    };
    let d = match clamp_i8(d, "chrono.DateTime.new") {
        Ok(v) => v,
        Err(m) => return err(lua, m),
    };
    let h = match clamp_i8(h, "chrono.DateTime.new") {
        Ok(v) => v,
        Err(m) => return err(lua, m),
    };
    let mi = match clamp_i8(mi, "chrono.DateTime.new") {
        Ok(v) => v,
        Err(m) => return err(lua, m),
    };
    let s = match clamp_i8(s, "chrono.DateTime.new") {
        Ok(v) => v,
        Err(m) => return err(lua, m),
    };
    let ns = match ns {
        Some(v) => match clamp_i32(v, "chrono.DateTime.new") {
            Ok(n) => n,
            Err(m) => return err(lua, m),
        },
        None => 0,
    };
    match DateTime::new(y, mo, d, h, mi, s, ns) {
        Ok(dt) => ok(lua, DateTimeHandle(dt)),
        Err(e) => err(lua, format!("chrono.DateTime.new: {e}")),
    }
}

#[instrument(name = "chrono.DateTime.parse", target = "lua::lib::chrono", skip_all)]
fn datetime_parse_ctor(lua: &Lua, s: String) -> mlua::Result<(Value, Value)> {
    match DateTime::from_str(&s) {
        Ok(dt) => ok(lua, DateTimeHandle(dt)),
        Err(e) => err(lua, format!("chrono.DateTime.parse: {e}")),
    }
}

// ---------------------------------------------------------------------------
// ZonedDateTime UserData
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct ZonedDateTimeHandle(pub Zoned);

impl UserData for ZonedDateTimeHandle {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("year", |_, this| Ok(i64::from(this.0.year())));
        fields.add_field_method_get("month", |_, this| Ok(i64::from(this.0.month())));
        fields.add_field_method_get("day", |_, this| Ok(i64::from(this.0.day())));
        fields.add_field_method_get("hour", |_, this| Ok(i64::from(this.0.hour())));
        fields.add_field_method_get("minute", |_, this| Ok(i64::from(this.0.minute())));
        fields.add_field_method_get("second", |_, this| Ok(i64::from(this.0.second())));
        fields.add_field_method_get("nanosecond", |_, this| Ok(i64::from(this.0.subsec_nanosecond())));
        fields.add_field_method_get("timezone", |_, this| {
            Ok(this.0.time_zone().iana_name().map(str::to_owned))
        });
        fields.add_field_method_get("offset_seconds", |_, this| Ok(i64::from(this.0.offset().seconds())));
        fields.add_field_method_get("epoch_seconds", |_, this| Ok(this.0.timestamp().as_second()));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("date", |_, this, (): ()| Ok(DateHandle(this.0.date())));
        methods.add_method("time", |_, this, (): ()| Ok(TimeHandle(this.0.time())));
        methods.add_method("datetime", |_, this, (): ()| Ok(DateTimeHandle(this.0.datetime())));

        methods.add_method("in_zone", |lua, this, name: String| {
            let tz = match lookup_zone(&name) {
                Ok(z) => z,
                Err(msg) => return err(lua, format!("chrono.ZonedDateTime.in_zone: {msg}")),
            };
            ok(lua, ZonedDateTimeHandle(this.0.with_time_zone(tz)))
        });

        methods.add_method("add", |lua, this, d: AnyUserData| {
            let span = match d.borrow::<DurationHandle>() {
                Ok(h) => h.0,
                Err(_) => {
                    return err(lua, "chrono.ZonedDateTime.add: expected Duration".to_string());
                }
            };
            match this.0.checked_add(span) {
                Ok(r) => ok(lua, ZonedDateTimeHandle(r)),
                Err(e) => err(lua, format!("chrono.ZonedDateTime.add: {e}")),
            }
        });
        methods.add_method("sub", |lua, this, d: AnyUserData| {
            let span = match d.borrow::<DurationHandle>() {
                Ok(h) => h.0,
                Err(_) => {
                    return err(lua, "chrono.ZonedDateTime.sub: expected Duration".to_string());
                }
            };
            match this.0.checked_sub(span) {
                Ok(r) => ok(lua, ZonedDateTimeHandle(r)),
                Err(e) => err(lua, format!("chrono.ZonedDateTime.sub: {e}")),
            }
        });
        methods.add_method("diff", |lua, this, other: AnyUserData| {
            let o = match other.borrow::<ZonedDateTimeHandle>() {
                Ok(h) => h.0.clone(),
                Err(_) => {
                    return err(lua, "chrono.ZonedDateTime.diff: expected ZonedDateTime".to_string());
                }
            };
            match this.0.until(&o) {
                Ok(s) => ok(lua, DurationHandle(s)),
                Err(e) => err(lua, format!("chrono.ZonedDateTime.diff: {e}")),
            }
        });

        methods.add_method("format", |lua, this, fmt: String| {
            match jiff::fmt::strtime::format(&fmt, &this.0) {
                Ok(s) => ok(lua, s),
                Err(e) => err(lua, format!("chrono.ZonedDateTime.format: {e}")),
            }
        });

        // RFC 3339 — `2026-05-20T16:30:00-05:00[America/New_York]` shape;
        // the trailing `[zone]` is jiff's extension. Use `format("%FT%T%:z")`
        // for strict RFC 3339 without the suffix.
        methods.add_method("to_rfc3339", |lua, this, (): ()| ok(lua, this.0.timestamp().to_string()));

        methods.add_meta_function(MetaMethod::Eq, |_, args: (AnyUserData, AnyUserData)| {
            let a = args.0.borrow::<ZonedDateTimeHandle>()?.0.clone();
            let b = args.1.borrow::<ZonedDateTimeHandle>()?.0.clone();
            Ok(a == b)
        });
        methods.add_meta_function(MetaMethod::Lt, |_, args: (AnyUserData, AnyUserData)| {
            let a = args.0.borrow::<ZonedDateTimeHandle>()?.0.clone();
            let b = args.1.borrow::<ZonedDateTimeHandle>()?.0.clone();
            Ok(a < b)
        });
        methods.add_meta_function(MetaMethod::Le, |_, args: (AnyUserData, AnyUserData)| {
            let a = args.0.borrow::<ZonedDateTimeHandle>()?.0.clone();
            let b = args.1.borrow::<ZonedDateTimeHandle>()?.0.clone();
            Ok(a <= b)
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, (): ()| Ok(this.0.to_string()));
    }
}

#[instrument(name = "chrono.ZonedDateTime.parse", target = "lua::lib::chrono", skip_all)]
fn zdt_parse_ctor(lua: &Lua, s: String) -> mlua::Result<(Value, Value)> {
    match Zoned::from_str(&s) {
        Ok(z) => ok(lua, ZonedDateTimeHandle(z)),
        Err(e) => err(lua, format!("chrono.ZonedDateTime.parse: {e}")),
    }
}

// ---------------------------------------------------------------------------
// Duration UserData (wraps jiff::Span — calendar-aware)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct DurationHandle(pub Span);

impl UserData for DurationHandle {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        // Field-style accessors return the *named* component of the span,
        // not a normalised total — `Duration.of({hours = 36}).hours` is
        // `36`, not `12`. Use `total_seconds` for a normalised number.
        fields.add_field_method_get("years", |_, this| Ok(i64::from(this.0.get_years())));
        fields.add_field_method_get("months", |_, this| Ok(i64::from(this.0.get_months())));
        fields.add_field_method_get("weeks", |_, this| Ok(i64::from(this.0.get_weeks())));
        fields.add_field_method_get("days", |_, this| Ok(i64::from(this.0.get_days())));
        fields.add_field_method_get("hours", |_, this| Ok(this.0.get_hours()));
        fields.add_field_method_get("minutes", |_, this| Ok(this.0.get_minutes()));
        fields.add_field_method_get("seconds", |_, this| Ok(this.0.get_seconds()));
        fields.add_field_method_get("milliseconds", |_, this| Ok(this.0.get_milliseconds()));
        fields.add_field_method_get("microseconds", |_, this| Ok(this.0.get_microseconds()));
        fields.add_field_method_get("nanoseconds", |_, this| Ok(this.0.get_nanoseconds()));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        // `total_seconds` normalises across calendar-agnostic units only
        // (week and below). Years and months are calendar-dependent and
        // refuse to collapse without a reference instant — match jiff and
        // surface the error rather than picking a fictional value.
        methods.add_method("total_seconds", |lua, this, (): ()| match this.0.total(Unit::Second) {
            Ok(v) => ok(lua, v),
            Err(e) => err(lua, format!("chrono.Duration.total_seconds: {e}")),
        });

        methods.add_method("negate", |_, this, (): ()| Ok(DurationHandle(this.0.negate())));
        methods.add_method("abs", |lua, this, (): ()| {
            // `Span::abs` is an `i64`-amount-wise sign flip — infallible for
            // every span jiff is willing to construct in the first place.
            ok(lua, DurationHandle(this.0.abs()))
        });

        methods.add_meta_function(MetaMethod::Eq, |_, args: (AnyUserData, AnyUserData)| {
            let a = args.0.borrow::<DurationHandle>()?.0;
            let b = args.1.borrow::<DurationHandle>()?.0;
            Ok(a.fieldwise() == b.fieldwise())
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, (): ()| Ok(this.0.to_string()));
    }
}

#[instrument(name = "chrono.Duration.of", target = "lua::lib::chrono", skip_all)]
fn duration_of(lua: &Lua, t: Table) -> mlua::Result<(Value, Value)> {
    let mut span = Span::new();
    macro_rules! apply {
        ($name:literal, $method:ident) => {
            if let Ok(v) = t.get::<i64>($name) {
                span = match span.$method(v) {
                    Ok(s) => s,
                    Err(e) => return err(lua, format!("chrono.Duration.of: {}: {e}", $name)),
                };
            }
        };
    }
    apply!("years", try_years);
    apply!("months", try_months);
    apply!("weeks", try_weeks);
    apply!("days", try_days);
    apply!("hours", try_hours);
    apply!("minutes", try_minutes);
    apply!("seconds", try_seconds);
    apply!("milliseconds", try_milliseconds);
    apply!("microseconds", try_microseconds);
    apply!("nanoseconds", try_nanoseconds);
    ok(lua, DurationHandle(span))
}

#[instrument(name = "chrono.Duration.parse", target = "lua::lib::chrono", skip_all)]
fn duration_parse_ctor(lua: &Lua, s: String) -> mlua::Result<(Value, Value)> {
    match Span::from_str(&s) {
        Ok(sp) => ok(lua, DurationHandle(sp)),
        Err(e) => err(lua, format!("chrono.Duration.parse: {e}")),
    }
}

// ---------------------------------------------------------------------------
// Flat helpers
// ---------------------------------------------------------------------------

#[instrument(name = "chrono.now", target = "lua::lib::chrono", skip_all)]
fn flat_now(_: &Lua, _: ()) -> mlua::Result<ZonedDateTimeHandle> {
    Ok(ZonedDateTimeHandle(Zoned::now()))
}

#[instrument(name = "chrono.utc_now", target = "lua::lib::chrono", skip_all)]
fn flat_utc_now(_: &Lua, _: ()) -> mlua::Result<ZonedDateTimeHandle> {
    Ok(ZonedDateTimeHandle(Timestamp::now().to_zoned(TimeZone::UTC)))
}

// Recognise whichever ISO 8601 / RFC 9557 shape the caller gave us and
// dispatch to the matching UserData. Shape-gated rather than try-all-in-order
// because jiff's `DateTime::from_str` accepts a bare `YYYY-MM-DD` as midnight —
// trying it before `Date` would swallow date-only input into a fake DateTime
// with `00:00:00`. We use the string's structural markers (`T`, `:`, `[`) to
// route to the right narrower parser.
#[instrument(name = "chrono.parse", target = "lua::lib::chrono", skip_all)]
fn flat_parse(lua: &Lua, s: String) -> mlua::Result<(Value, Value)> {
    let has_datetime_shape = s.contains('T') || s.contains(' ');
    if has_datetime_shape {
        if let Ok(z) = Zoned::from_str(&s) {
            return ok(lua, ZonedDateTimeHandle(z));
        }
        // RFC 3339 with `Z` or numeric offset but no `[zone]` suffix —
        // `Zoned::from_str` rejects it, but `Timestamp::from_str` accepts.
        // Normalise to a UTC-zoned instant so the instant is preserved.
        if let Ok(ts) = Timestamp::from_str(&s) {
            return ok(lua, ZonedDateTimeHandle(ts.to_zoned(TimeZone::UTC)));
        }
        if let Ok(dt) = DateTime::from_str(&s) {
            return ok(lua, DateTimeHandle(dt));
        }
    } else if s.contains(':') {
        if let Ok(t) = Time::from_str(&s) {
            return ok(lua, TimeHandle(t));
        }
    } else if let Ok(d) = Date::from_str(&s) {
        return ok(lua, DateHandle(d));
    }
    err(lua, format!("chrono.parse: cannot recognise {s:?} as Date, Time, DateTime, or ZonedDateTime"))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn lookup_zone(name: &str) -> Result<TimeZone, String> {
    TimeZone::get(name).map_err(|e| format!("unknown timezone {name:?}: {e}"))
}

// `Date::checked_add` silently rounds hours/minutes/seconds into days; reject
// at the boundary so callers don't hit invisible data loss.
fn span_has_sub_day_units(s: &Span) -> bool {
    s.get_hours() != 0
        || s.get_minutes() != 0
        || s.get_seconds() != 0
        || s.get_milliseconds() != 0
        || s.get_microseconds() != 0
        || s.get_nanoseconds() != 0
}

fn clamp_i16(v: i64, prefix: &str) -> Result<i16, String> {
    i16::try_from(v).map_err(|_| format!("{prefix}: value {v} out of range for i16"))
}

fn clamp_i8(v: i64, prefix: &str) -> Result<i8, String> {
    i8::try_from(v).map_err(|_| format!("{prefix}: value {v} out of range for i8"))
}

fn clamp_i32(v: i64, prefix: &str) -> Result<i32, String> {
    i32::try_from(v).map_err(|_| format!("{prefix}: value {v} out of range for i32"))
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

#[cfg(feature = "bench")]
pub fn benches(c: &mut criterion::Criterion) {
    use criterion::black_box;

    let lua = crate::lua::sandbox::make_engine().expect("engine");

    let mut g = c.benchmark_group("chrono");

    g.bench_function("library/parse_rfc3339", |b| {
        b.iter(|| {
            let _ = Zoned::from_str(black_box("2026-05-20T16:30:00-04:00[America/New_York]")).unwrap();
        });
    });

    g.bench_function("library/now_utc", |b| {
        b.iter(|| {
            let _ = Timestamp::now().to_zoned(TimeZone::UTC);
        });
    });

    let parse: mlua::Function = lua
        .load(r#"return function(s) return (require("lib:chrono")).parse(s) end"#)
        .eval()
        .expect("parse chunk");
    g.bench_function("through_lua/parse", |b| {
        b.iter(|| {
            let _: mlua::Value = parse.call(black_box("2026-05-20T16:30:00Z".to_string())).unwrap();
        });
    });

    g.finish();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use mlua::{Lua, Table, Value};

    fn engine() -> Lua {
        let lua = crate::lua::sandbox::make_engine().expect("engine");
        let m: Table = lua.load(r#"return require("lib:chrono")"#).eval().unwrap();
        lua.globals().set("chrono", m).unwrap();
        lua
    }

    #[test]
    fn date_new_and_fields() {
        let lua = engine();
        let (y, m, d): (i64, i64, i64) = lua
            .load(
                r"
                local d = chrono.Date.new(2026, 5, 20)
                return d.year, d.month, d.day
            ",
            )
            .eval()
            .unwrap();
        assert_eq!((y, m, d), (2026, 5, 20));
    }

    #[test]
    fn date_invalid_returns_err() {
        let lua = engine();
        let (v, e): (Value, Option<String>) =
            lua.load(r"return chrono.Date.new(2026, 2, 30)").eval().unwrap();
        assert!(matches!(v, Value::Nil));
        assert!(e.unwrap().starts_with("chrono.Date.new:"));
    }

    #[test]
    fn date_parse_iso() {
        let lua = engine();
        let s: String = lua.load(r#"return tostring(chrono.Date.parse("2026-05-20"))"#).eval().unwrap();
        assert_eq!(s, "2026-05-20");
    }

    #[test]
    fn date_parse_invalid_returns_err() {
        let lua = engine();
        let (v, e): (Value, Option<String>) =
            lua.load(r#"return chrono.Date.parse("not a date")"#).eval().unwrap();
        assert!(matches!(v, Value::Nil));
        assert!(e.unwrap().starts_with("chrono.Date.parse:"));
    }

    #[test]
    fn date_arithmetic() {
        let lua = engine();
        let s: String = lua
            .load(
                r#"
                local d = chrono.Date.parse("2026-05-20")
                local dur = chrono.Duration.of({ days = 10 })
                return tostring(d:add(dur))
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(s, "2026-05-30");
    }

    #[test]
    fn date_diff_returns_duration() {
        let lua = engine();
        let days: i64 = lua
            .load(
                r#"
                local a = chrono.Date.parse("2026-05-20")
                local b = chrono.Date.parse("2026-05-30")
                return a:diff(b).days
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(days, 10);
    }

    #[test]
    fn date_comparison_metamethods() {
        let lua = engine();
        let (eq, lt, le): (bool, bool, bool) = lua
            .load(
                r#"
                local a = chrono.Date.parse("2026-05-20")
                local b = chrono.Date.parse("2026-05-30")
                local c = chrono.Date.parse("2026-05-20")
                return a == c, a < b, a <= c
            "#,
            )
            .eval()
            .unwrap();
        assert!(eq);
        assert!(lt);
        assert!(le);
    }

    #[test]
    fn time_new_and_fields() {
        let lua = engine();
        let (h, m, s): (i64, i64, i64) = lua
            .load(
                r"
                local t = chrono.Time.new(13, 45, 30)
                return t.hour, t.minute, t.second
            ",
            )
            .eval()
            .unwrap();
        assert_eq!((h, m, s), (13, 45, 30));
    }

    #[test]
    fn time_invalid_returns_err() {
        let lua = engine();
        let (v, e): (Value, Option<String>) = lua.load(r"return chrono.Time.new(25, 0, 0)").eval().unwrap();
        assert!(matches!(v, Value::Nil));
        assert!(e.unwrap().starts_with("chrono.Time.new:"));
    }

    #[test]
    fn datetime_compose_from_date_and_time() {
        let lua = engine();
        let s: String = lua
            .load(
                r"
                local d = chrono.Date.new(2026, 5, 20)
                local t = chrono.Time.new(13, 45, 30)
                return tostring(d:at(t))
            ",
            )
            .eval()
            .unwrap();
        assert_eq!(s, "2026-05-20T13:45:30");
    }

    #[test]
    fn datetime_in_zone_returns_zoned() {
        let lua = engine();
        let (s, tz): (String, Option<String>) = lua
            .load(
                r#"
                local dt = chrono.DateTime.parse("2026-05-20T12:00:00")
                local z = dt:in_zone("UTC")
                return tostring(z), z.timezone
            "#,
            )
            .eval()
            .unwrap();
        assert!(s.starts_with("2026-05-20T12:00:00+00:00"));
        assert_eq!(tz.as_deref(), Some("UTC"));
    }

    #[test]
    fn zdt_unknown_zone_returns_err() {
        let lua = engine();
        let (v, e): (Value, Option<String>) = lua
            .load(
                r#"
                local dt = chrono.DateTime.parse("2026-05-20T12:00:00")
                return dt:in_zone("Mars/Olympus_Mons")
            "#,
            )
            .eval()
            .unwrap();
        assert!(matches!(v, Value::Nil));
        assert!(e.unwrap().starts_with("chrono.DateTime.in_zone:"));
    }

    #[test]
    fn zdt_parse_round_trip() {
        let lua = engine();
        // Winter date — `-05:00` is the standard-time offset for
        // America/New_York. Summer dates would need `-04:00` (EDT); jiff
        // rejects offset/zone mismatches at parse time.
        let s: String = lua
            .load(
                r#"
                local z = chrono.ZonedDateTime.parse("2026-01-20T16:30:00-05:00[America/New_York]")
                return tostring(z)
            "#,
            )
            .eval()
            .unwrap();
        assert!(s.contains("America/New_York"));
    }

    #[test]
    fn zdt_in_zone_preserves_instant() {
        let lua = engine();
        let (a, b): (i64, i64) = lua
            .load(
                r#"
                local z = chrono.ZonedDateTime.parse("2026-01-20T16:30:00-05:00[America/New_York]")
                local w = z:in_zone("UTC")
                return z.epoch_seconds, w.epoch_seconds
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn duration_of_and_total_seconds() {
        let lua = engine();
        let total: f64 = lua
            .load(
                r"
                return chrono.Duration.of({ hours = 1, minutes = 30 }):total_seconds()
            ",
            )
            .eval()
            .unwrap();
        assert!((total - 5400.0).abs() < 1e-9);
    }

    #[test]
    fn duration_total_seconds_rejects_calendar_units() {
        let lua = engine();
        let (v, e): (Value, Option<String>) =
            lua.load(r"return chrono.Duration.of({ months = 1 }):total_seconds()").eval().unwrap();
        assert!(matches!(v, Value::Nil));
        assert!(e.unwrap().starts_with("chrono.Duration.total_seconds:"));
    }

    #[test]
    fn duration_parse_iso8601() {
        let lua = engine();
        let s: String = lua.load(r#"return tostring(chrono.Duration.parse("PT1H30M"))"#).eval().unwrap();
        // jiff round-trips to its canonical form — uppercase, no truncation.
        assert_eq!(s, "PT1H30M");
    }

    #[test]
    fn flat_parse_dispatches_to_zoned() {
        let lua = engine();
        let s: String = lua
            .load(
                r#"
                local v = chrono.parse("2026-01-20T16:30:00-05:00[America/New_York]")
                return tostring(v)
            "#,
            )
            .eval()
            .unwrap();
        assert!(s.contains("America/New_York"));
    }

    #[test]
    fn flat_parse_unknown_returns_err() {
        let lua = engine();
        let (v, e): (Value, Option<String>) =
            lua.load(r#"return chrono.parse("not a timestamp")"#).eval().unwrap();
        assert!(matches!(v, Value::Nil));
        assert!(e.unwrap().starts_with("chrono.parse:"));
    }

    #[test]
    fn utc_now_zone_is_utc() {
        let lua = engine();
        let tz: Option<String> = lua.load(r"return chrono.utc_now().timezone").eval().unwrap();
        assert_eq!(tz.as_deref(), Some("UTC"));
    }

    #[test]
    fn now_returns_zoned() {
        let lua = engine();
        let s: String = lua.load(r"return tostring(chrono.now()):sub(1, 1)").eval().unwrap();
        // First char of an ISO 8601 timestamp is always a digit.
        assert!(s.chars().next().unwrap().is_ascii_digit());
    }

    #[test]
    fn strftime_formats_date() {
        let lua = engine();
        let s: String = lua
            .load(
                r#"
                return chrono.Date.new(2026, 5, 20):format("%Y/%m/%d")
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(s, "2026/05/20");
    }
}
