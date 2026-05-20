--!strict
--
-- Type definitions for `std:time` — paired with `src/lua/std/time.rs`.
-- Co-located with the Rust source so any change to the Lua-facing surface
-- is reviewed alongside the implementation.
--
-- ## Monotonic clock
--
-- Two opaque types — `Instant` (point in time) and `Duration` (interval).
-- Independent of wall-clock: survives NTP slews, DST jumps, manual clock
-- adjustments. Arithmetic saturates instead of panicking, so scripts never
-- blow up on overflow or negative results.
--
-- ## Editor diagnostics
--
-- `declare` is a `.d.luau` keyword, not regular Luau syntax. Editors flag
-- these lines as errors; luau-lsp consumes them correctly. Don't remove them.

--[=[
    Opaque monotonic point in time, mirroring `std::time::Instant`.

    Construct with `time.Instant.now()` or the flat alias `time.now()`.
    Compare with `<`, `<=`, `==`. Diff with `-` to get a `Duration`.

    ```lua
    local time = require("std:time")
    local t0 = time.now()
    -- ... work ...
    local elapsed = t0:elapsed()      -- Duration since t0
    local diff    = time.now() - t0   -- Duration (same value)
    ```
]=]
export type Instant = {
    --[=[
        Returns the `Duration` elapsed since this `Instant` was taken.
        Saturating: never negative even on system clock weirdness.
    ]=]
    elapsed: (self: Instant) -> Duration,

    --[=[
        Saturating difference: `self - other`. Returns `ZERO` if `other`
        is later than `self`.
    ]=]
    duration_since: (self: Instant, other: Instant) -> Duration,
}

--[=[
    Typed time interval, mirroring `std::time::Duration`. Non-negative,
    nanosecond-resolution. Construct from a unit or by parsing.

    ```lua
    local time = require("std:time")
    local d1 = time.Duration.from_millis(250)
    local d2 = time.Duration.from_secs_f64(1.5)
    local d3 = time.Duration.parse("1m30s")
    tostring(d3)  -- "1m30s"
    ```

    Arithmetic saturates — `a - b` where `b > a` returns `ZERO` rather
    than panicking. `Duration * number` and `Duration / number` reject
    NaN, negative, and infinity.
]=]
export type Duration = {
    --[=[ Whole seconds component (integer). ]=]
    as_secs: (self: Duration) -> number,
    --[=[ Total milliseconds (integer, may saturate at 2^53). ]=]
    as_millis: (self: Duration) -> number,
    --[=[ Total microseconds (integer, may saturate at 2^53). ]=]
    as_micros: (self: Duration) -> number,
    --[=[ Total nanoseconds (integer, may saturate at 2^53). ]=]
    as_nanos: (self: Duration) -> number,
    --[=[ Total seconds as a float (lossy beyond ~285 years). ]=]
    as_secs_f64: (self: Duration) -> number,
    --[=[ True if the duration is exactly zero. ]=]
    is_zero: (self: Duration) -> boolean,
}

--[=[
    `Instant` factory.
]=]
export type InstantStatic = {
    --[=[ Current monotonic point in time. ]=]
    now: () -> Instant,
}

--[=[
    `Duration` factory.
]=]
export type DurationStatic = {
    --[=[ Zero duration constant. ]=]
    ZERO: Duration,
    --[=[ Construct from whole seconds. Errors on negative input. ]=]
    from_secs: (secs: number) -> Duration,
    --[=[ Construct from whole milliseconds. Errors on negative input. ]=]
    from_millis: (ms: number) -> Duration,
    --[=[ Construct from whole microseconds. Errors on negative input. ]=]
    from_micros: (us: number) -> Duration,
    --[=[ Construct from whole nanoseconds. Errors on negative input. ]=]
    from_nanos: (ns: number) -> Duration,
    --[=[
        Construct from fractional seconds. Errors on NaN, infinity, or
        negative input.
    ]=]
    from_secs_f64: (secs: number) -> Duration,
    --[=[
        Parse a Go-style duration string. Units: `ns`, `us` / `µs`, `ms`,
        `s`, `m`, `h`. Segments are summed: `"1h30m"`, `"1.5s"`, `"500ms"`.
        Returns `(Duration, nil)` on success or `(nil, err)` on failure.
    ]=]
    parse: (input: string) -> (Duration?, string?),
}

--[=[
    The module table returned by `require("std:time")`.
]=]
export type TimeModule = {
    Instant: InstantStatic,
    Duration: DurationStatic,


    --[=[ Flat alias for `Instant.now()`. ]=]
    now: () -> Instant,

    --[=[
        Cooperative sleep for the given duration. Returns the actual
        elapsed `Duration` so callers can detect overruns. Must be called
        from within a spawned/deferred task.
    ]=]
    sleep: (duration: Duration) -> Duration,
}

declare function require(name: "std:time"): TimeModule
