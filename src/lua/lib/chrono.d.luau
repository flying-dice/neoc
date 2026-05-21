--!strict
--
-- Type definitions for `lib:chrono` — paired with `src/lua/lib/chrono.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface
-- is reviewed alongside the implementation.

--[=[
    A civil calendar date — year, month, day with no time-of-day and no zone.
    Construct with `chrono.Date.new(y, m, d)` or `chrono.Date.parse(s)`
    (`YYYY-MM-DD`).

    Immutable: every `with_*` returns a fresh `Date`. `tostring(d)` produces
    the canonical `YYYY-MM-DD` form.

    Comparison metamethods (`==`, `<`, `<=`) order chronologically.
]=]
export type Date = {
    --[=[ Proleptic Gregorian year. Negative for BCE. ]=]
    year: number,
    --[=[ Month, `1..=12`. ]=]
    month: number,
    --[=[ Day-of-month, `1..=31` (bounded by the actual month length). ]=]
    day: number,
    --[=[ ISO weekday, `1` = Monday … `7` = Sunday. ]=]
    weekday: number,
    --[=[ Day-of-year, `1..=366`. ]=]
    ordinal: number,

    with_year: (self: Date, year: number) -> (Date?, string?),
    with_month: (self: Date, month: number) -> (Date?, string?),
    with_day: (self: Date, day: number) -> (Date?, string?),

    --[=[ Compose with a `Time` to produce a `DateTime` at this date. ]=]
    at: (self: Date, time: Time) -> DateTime,

    --[=[
        Return a new `Date` shifted by `duration`. Calendar units (years,
        months) are interpreted relative to this date — adding `1 month` to
        Jan 31 lands on Feb 28/29, not Mar 3.
    ]=]
    add: (self: Date, duration: Duration) -> (Date?, string?),
    sub: (self: Date, duration: Duration) -> (Date?, string?),

    --[=[
        Return the `Duration` from `self` to `other`. Result is negative
        when `other` precedes `self`. Named `diff` (not `until`) because
        `until` is a Luau keyword.
    ]=]
    diff: (self: Date, other: Date) -> (Duration?, string?),

    --[=[
        Format the date using a `strftime` pattern (`%Y`, `%m`, `%d`, `%A`,
        `%j`, …). Returns `(nil, err)` for unparseable patterns — see
        jiff's strtime documentation for the full conversion table.
    ]=]
    format: (self: Date, pattern: string) -> (string?, string?),
}

--[=[
    A civil time-of-day — hour, minute, second, nanosecond. No date, no zone.
    Construct with `chrono.Time.new(h, m, s, ns?)` or `chrono.Time.parse(s)`
    (`HH:MM:SS[.fff]`).
]=]
export type Time = {
    --[=[ Hour, `0..=23`. ]=]
    hour: number,
    --[=[ Minute, `0..=59`. ]=]
    minute: number,
    --[=[ Second, `0..=59`. ]=]
    second: number,
    --[=[ Sub-second nanoseconds, `0..=999_999_999`. ]=]
    nanosecond: number,

    --[=[ Compose with a `Date` to produce a `DateTime`. ]=]
    on: (self: Time, date: Date) -> DateTime,
    format: (self: Time, pattern: string) -> (string?, string?),
}

--[=[
    Civil date + time without a zone. Useful for wall-clock arithmetic when
    the zone is implied by context. Attach a zone with `:in_zone(name)` to
    get a `ZonedDateTime`.

    Construct via `chrono.DateTime.new(y, mo, d, h, mi, s, ns?)` or
    `chrono.DateTime.parse(s)` (`YYYY-MM-DDTHH:MM:SS[.fff]`).
]=]
export type DateTime = {
    year: number,
    month: number,
    day: number,
    hour: number,
    minute: number,
    second: number,
    nanosecond: number,

    date: (self: DateTime) -> Date,
    time: (self: DateTime) -> Time,

    --[=[
        Attach an IANA timezone (`"UTC"`, `"America/New_York"`, …) and
        promote to `ZonedDateTime`. Returns `(nil, err)` for unknown zones.

        Ambiguous local times (DST gap or fold) are resolved silently using
        jiff's `Compatible` disambiguation — gap times bump forward to the
        next valid instant, fold times pick the earlier of the two
        candidates. Matches JS `Temporal`, Python `zoneinfo`, and Java
        `ZonedDateTime.ofLocal` defaults.
    ]=]
    in_zone: (self: DateTime, name: string) -> (ZonedDateTime?, string?),

    add: (self: DateTime, duration: Duration) -> (DateTime?, string?),
    sub: (self: DateTime, duration: Duration) -> (DateTime?, string?),
    diff: (self: DateTime, other: DateTime) -> (Duration?, string?),

    format: (self: DateTime, pattern: string) -> (string?, string?),
}

--[=[
    A wall-clock instant paired with an IANA timezone — the only type that
    survives across DST transitions and zone changes without ambiguity.

    Construct via `chrono.ZonedDateTime.parse(s)` (RFC 9557 / jiff form
    `2026-01-20T16:30:00-05:00[America/New_York]`) or by attaching a zone
    to a `DateTime` with `:in_zone(name)`. Get the current instant with
    `chrono.now()` (local zone) or `chrono.utc_now()` (UTC).
]=]
export type ZonedDateTime = {
    year: number,
    month: number,
    day: number,
    hour: number,
    minute: number,
    second: number,
    nanosecond: number,
    --[=[ IANA zone name, e.g. `"America/New_York"`. `nil` for fixed-offset
        zones that have no canonical name. ]=]
    timezone: string?,
    --[=[ Offset from UTC in seconds at this instant. ]=]
    offset_seconds: number,
    --[=[ Unix timestamp (seconds since 1970-01-01T00:00:00Z). ]=]
    epoch_seconds: number,

    date: (self: ZonedDateTime) -> Date,
    time: (self: ZonedDateTime) -> Time,
    datetime: (self: ZonedDateTime) -> DateTime,

    --[=[
        Convert this instant to a different timezone. The underlying epoch
        instant is preserved; the displayed wall-clock changes. Returns
        `(nil, err)` for unknown zone names.
    ]=]
    in_zone: (self: ZonedDateTime, name: string) -> (ZonedDateTime?, string?),

    add: (self: ZonedDateTime, duration: Duration) -> (ZonedDateTime?, string?),
    sub: (self: ZonedDateTime, duration: Duration) -> (ZonedDateTime?, string?),
    diff: (self: ZonedDateTime, other: ZonedDateTime) -> (Duration?, string?),

    format: (self: ZonedDateTime, pattern: string) -> (string?, string?),
    --[=[ Strict RFC 3339 timestamp (`2026-01-20T21:30:00Z`) for the
        underlying epoch instant — no zone suffix, always UTC. ]=]
    to_rfc3339: (self: ZonedDateTime) -> string,
}

--[=[
    A calendar-aware span built from a mix of years, months, weeks, days,
    hours, minutes, seconds, milliseconds, microseconds, nanoseconds.
    Field accessors return the *named* component as it was stored — not
    a normalised total. Use `total_seconds()` to collapse to a single
    number (only valid for calendar-agnostic units — week and below).

    Construct from a table with `chrono.Duration.of({ ... })` or from an
    ISO 8601 duration string (`PT1H30M`, `P1Y2M3D`) with
    `chrono.Duration.parse(s)`. `tostring(d)` produces the ISO 8601 form.
]=]
export type Duration = {
    years: number,
    months: number,
    weeks: number,
    days: number,
    hours: number,
    minutes: number,
    seconds: number,
    milliseconds: number,
    microseconds: number,
    nanoseconds: number,

    --[=[
        Total seconds for spans that contain only week-and-below units.
        Returns `(nil, err)` when years or months are present — those
        units cannot be collapsed without a reference instant.
    ]=]
    total_seconds: (self: Duration) -> (number?, string?),
    negate: (self: Duration) -> Duration,
    abs: (self: Duration) -> Duration,
}

--[=[
    Per-component initialiser for `Duration.of`. Every field is optional;
    omitted fields default to `0`.
]=]
export type DurationParts = {
    years: number?,
    months: number?,
    weeks: number?,
    days: number?,
    hours: number?,
    minutes: number?,
    seconds: number?,
    milliseconds: number?,
    microseconds: number?,
    nanoseconds: number?,
}

export type DateConstructor = {
    new: (year: number, month: number, day: number) -> (Date?, string?),
    parse: (s: string) -> (Date?, string?),
}
export type TimeConstructor = {
    new: (hour: number, minute: number, second: number, nanosecond: number?) -> (Time?, string?),
    parse: (s: string) -> (Time?, string?),
}
export type DateTimeConstructor = {
    new: (year: number, month: number, day: number, hour: number, minute: number, second: number, nanosecond: number?) -> (DateTime?, string?),
    parse: (s: string) -> (DateTime?, string?),
}
export type ZonedDateTimeConstructor = {
    parse: (s: string) -> (ZonedDateTime?, string?),
}
export type DurationConstructor = {
    of: (parts: DurationParts) -> (Duration?, string?),
    parse: (s: string) -> (Duration?, string?),
}

--[=[
    The module table returned by `require("lib:chrono")`.
]=]
export type ChronoModule = {
    Date: DateConstructor,
    Time: TimeConstructor,
    DateTime: DateTimeConstructor,
    ZonedDateTime: ZonedDateTimeConstructor,
    Duration: DurationConstructor,

    --[=[ Current wall-clock instant in the system's local timezone. ]=]
    now: () -> ZonedDateTime,
    --[=[ Current wall-clock instant in UTC. ]=]
    utc_now: () -> ZonedDateTime,
    --[=[
        Auto-detect the shape of an ISO 8601 / RFC 3339 string and return
        the richest matching type — `ZonedDateTime` for zoned input,
        falling back through `DateTime`, `Date`, `Time` in that order.
        Returns `(nil, err)` if no type recognises the input.
    ]=]
    parse: (s: string) -> ((ZonedDateTime | DateTime | Date | Time)?, string?),
}

declare function require(name: "lib:chrono"): ChronoModule
