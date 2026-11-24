//! Calendar dates, times and timezones: `lib:chrono` (the `jiff` crate).
//!
//! Five types, split as `java.time` and JS `Temporal` split them: `Date`, `Time` and `DateTime` are civil (no
//! zone), `ZonedDateTime` is an instant in a named IANA zone, and `Duration` is a span that keeps its calendar units
//! (days, months, years) through arithmetic. `now`, `utc_now` and `parse` are the shortcuts.
//!
//! The API's rules:
//! - There are no operators: `a:compare(b)` gives `"less" | "equal" | "greater"`, `a:equals(b)` a boolean.
//! - Arithmetic is `plus` and `minus`, as in `std:time`.
//! - Every failure is a typed error (`ValueError`, `ParseError`, `ArithmeticError`, `ZoneError`, `FormatError`,
//!   `TotalError`).
//! - `Duration.of` refuses a unit that is not a whole number and a unit it does not know.
//! - `Duration.of` raises a unit that is past what a duration holds, so its result can be passed on as it is:
//!   `date:plus(chrono.Duration.of({ days = 10 }))`.
//! - `Duration:abs` and `ZonedDateTime:to_rfc3339` cannot fail and return their value alone.
//! - Every date and time value has a `kind` field (`"date"`, `"time"`, `"date_time"`, `"zoned_date_time"`), so a script
//!   can tell which one `chrono.parse` gave it.

use jiff::Span;

/// `Date` arithmetic would round hours, minutes and seconds into days without saying so: such a span is refused.
fn span_has_sub_day_units(span: &Span) -> bool {
    span.get_hours() != 0
        || span.get_minutes() != 0
        || span.get_seconds() != 0
        || span.get_milliseconds() != 0
        || span.get_microseconds() != 0
        || span.get_nanoseconds() != 0
}

#[mlua_ir::module("lib:chrono")]
pub mod chrono {
    use std::str::FromStr;

    use jiff::fmt::strtime::BrokenDownTime;
    use jiff::tz::TimeZone;
    use jiff::{Span, Timestamp, Unit, civil};
    use mlua_ir::{LuaError, Object, OneOf, Raise, Shape};
    use serde::{Deserialize, Serialize};

    use super::span_has_sub_day_units;

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

    /// What a `Date` is, among the values `parse` gives.
    #[derive(Debug, Clone, Copy, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum DateKind {
        /// A calendar date.
        Date,
    }

    /// What a `Time` is, among the values `parse` gives.
    #[derive(Debug, Clone, Copy, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum TimeKind {
        /// A time of day.
        Time,
    }

    /// What a `DateTime` is, among the values `parse` gives.
    #[derive(Debug, Clone, Copy, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum DateTimeKind {
        /// A date and a time of day, in no zone.
        DateTime,
    }

    /// What a `ZonedDateTime` is, among the values `parse` gives.
    #[derive(Debug, Clone, Copy, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum ZonedDateTimeKind {
        /// An instant in a zone.
        ZonedDateTime,
    }

    /// Why a date or a time cannot be made from its numbers.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum ValueError {
        /// A number is not one the calendar or the clock has (month 13, 30 February, hour 25).
        #[error("{reason}")]
        OutOfRange {
            /// Which number, and the range it must be in.
            reason: String,
        },
    }

    /// Why text is not a date, a time or a duration.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum ParseError {
        /// The text is not what the type is written as.
        #[error("invalid text {text:?}: {reason}")]
        Invalid {
            /// The text as given.
            text: String,
            /// What is wrong.
            reason: String,
        },
        /// `parse` found no date, time, datetime or zoned datetime in the text.
        #[error("cannot recognise {text:?} as a Date, Time, DateTime or ZonedDateTime")]
        Unrecognised {
            /// The text as given.
            text: String,
        },
    }

    /// Why a duration could not be added, subtracted or measured.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum ArithmeticError {
        /// The duration has hours or smaller units, and a `Date` counts in days.
        #[error("the duration carries units smaller than a day, and a Date counts in days")]
        SubDayUnits,
        /// The result is past what a date or a duration holds.
        #[error("{reason}")]
        Overflow {
            /// What overflowed.
            reason: String,
        },
    }

    /// Why a value could not be put in a zone.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum ZoneError {
        /// No IANA zone has this name.
        #[error("unknown timezone {name:?}: {reason}")]
        UnknownTimezone {
            /// The name asked for.
            name: String,
            /// What the timezone database said.
            reason: String,
        },
        /// The datetime in that zone is past what an instant holds.
        #[error("{reason}")]
        OutOfRange {
            /// What is out of range.
            reason: String,
        },
    }

    /// Why a value could not be formatted.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum FormatError {
        /// The strftime pattern has a directive that does not exist, or one the value has nothing for (`%H` on a
        /// `Date`).
        #[error("invalid pattern {pattern:?}: {reason}")]
        InvalidPattern {
            /// The pattern as given.
            pattern: String,
            /// What is wrong.
            reason: String,
        },
    }

    /// Why a duration has no total.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum TotalError {
        /// The duration has years or months, whose length depends on the date they are counted from.
        #[error("{reason}")]
        CalendarUnits {
            /// What cannot be totalled.
            reason: String,
        },
    }

    /// A number as the calendar type that holds it.
    fn number<T: TryFrom<i64>>(name: &str, value: i64) -> Result<T, ValueError> {
        T::try_from(value)
            .map_err(|_| ValueError::OutOfRange { reason: format!("{name} {value} is out of range") })
    }

    fn out_of_range(error: &jiff::Error) -> ValueError {
        ValueError::OutOfRange { reason: error.to_string() }
    }

    fn overflow(error: &jiff::Error) -> ArithmeticError {
        ArithmeticError::Overflow { reason: error.to_string() }
    }

    fn invalid(text: &str, error: &jiff::Error) -> ParseError {
        ParseError::Invalid { text: text.to_owned(), reason: error.to_string() }
    }

    fn zone(name: &str) -> Result<TimeZone, ZoneError> {
        TimeZone::get(name)
            .map_err(|error| ZoneError::UnknownTimezone { name: name.to_owned(), reason: error.to_string() })
    }

    /// `jiff::fmt::strtime::format` and not `strftime`: the latter's `Display` panics on a pattern it cannot read.
    fn strftime(pattern: &str, value: impl Into<BrokenDownTime>) -> Result<String, FormatError> {
        jiff::fmt::strtime::format(pattern, value).map_err(|error| FormatError::InvalidPattern {
            pattern: pattern.to_owned(),
            reason: error.to_string(),
        })
    }

    /// A calendar date, in no zone. `tostring` gives ISO 8601 (`2026-05-20`).
    #[derive(Debug, Clone, Copy, Object)]
    pub struct Date {
        inner: civil::Date,
    }

    #[mlua_ir::object]
    impl Date {
        /// The date of a year, a month (1-12) and a day (1-31).
        ///
        /// ```luau
        /// local d = assert(chrono.Date.new(2026, 5, 20))
        /// assert(d.year == 2026 and d.weekday == 3)
        /// local _, err = chrono.Date.new(2026, 2, 30)
        /// assert(err ~= nil and err.kind == "out_of_range")
        /// ```
        pub fn new(year: i64, month: i64, day: i64) -> Result<Date, ValueError> {
            civil::Date::new(number("year", year)?, number("month", month)?, number("day", day)?)
                .map(|inner| Date { inner })
                .map_err(|error| out_of_range(&error))
        }

        /// Read an ISO 8601 date (`2026-05-20`).
        pub fn parse(text: String) -> Result<Date, ParseError> {
            civil::Date::from_str(&text).map(|inner| Date { inner }).map_err(|error| invalid(&text, &error))
        }

        /// `"date"`: what tells this from the other values `parse` gives.
        #[get]
        pub fn kind(&self) -> DateKind {
            DateKind::Date
        }

        /// The calendar year.
        #[get]
        pub fn year(&self) -> i16 {
            self.inner.year()
        }

        /// The month of the year (1-12).
        #[get]
        pub fn month(&self) -> i8 {
            self.inner.month()
        }

        /// The day of the month (1-31).
        #[get]
        pub fn day(&self) -> i8 {
            self.inner.day()
        }

        /// The day of the week: Monday is 1, Sunday 7.
        #[get]
        pub fn weekday(&self) -> i8 {
            self.inner.weekday().to_monday_one_offset()
        }

        /// The day of the year (1-366).
        #[get]
        pub fn ordinal(&self) -> i16 {
            self.inner.day_of_year()
        }

        /// This date in another year.
        pub fn with_year(&self, year: i64) -> Result<Date, ValueError> {
            let year = number("year", year)?;
            self.inner
                .with()
                .year(year)
                .build()
                .map(|inner| Date { inner })
                .map_err(|error| out_of_range(&error))
        }

        /// This date in another month.
        pub fn with_month(&self, month: i64) -> Result<Date, ValueError> {
            let month = number("month", month)?;
            self.inner
                .with()
                .month(month)
                .build()
                .map(|inner| Date { inner })
                .map_err(|error| out_of_range(&error))
        }

        /// This date on another day of its month.
        pub fn with_day(&self, day: i64) -> Result<Date, ValueError> {
            let day = number("day", day)?;
            self.inner
                .with()
                .day(day)
                .build()
                .map(|inner| Date { inner })
                .map_err(|error| out_of_range(&error))
        }

        /// This date at a time of day.
        pub fn at(&self, time: Time) -> DateTime {
            DateTime { inner: self.inner.to_datetime(time.inner) }
        }

        /// The date `duration` later. The duration counts in days or longer units.
        ///
        /// ```luau
        /// local d = assert(chrono.Date.parse("2026-05-20"))
        /// assert(tostring(assert(d:plus(chrono.Duration.of({ days = 10 })))) == "2026-05-30")
        /// local _, err = d:plus(chrono.Duration.of({ hours = 25 }))
        /// assert(err ~= nil and err.kind == "sub_day_units")
        /// ```
        pub fn plus(&self, duration: Duration) -> Result<Date, ArithmeticError> {
            if span_has_sub_day_units(&duration.inner) {
                return Err(ArithmeticError::SubDayUnits);
            }
            self.inner
                .checked_add(duration.inner)
                .map(|inner| Date { inner })
                .map_err(|error| overflow(&error))
        }

        /// The date `duration` earlier. The duration counts in days or longer units.
        pub fn minus(&self, duration: Duration) -> Result<Date, ArithmeticError> {
            if span_has_sub_day_units(&duration.inner) {
                return Err(ArithmeticError::SubDayUnits);
            }
            self.inner
                .checked_sub(duration.inner)
                .map(|inner| Date { inner })
                .map_err(|error| overflow(&error))
        }

        /// The days from this date until `other` (negative when `other` is earlier).
        pub fn diff(&self, other: Date) -> Result<Duration, ArithmeticError> {
            self.inner.until(other.inner).map(|inner| Duration { inner }).map_err(|error| overflow(&error))
        }

        /// How this orders against `other`.
        pub fn compare(&self, other: Date) -> Order {
            self.inner.cmp(&other.inner).into()
        }

        /// Whether this and `other` are the same date. (`==` asks whether they are the same object.)
        pub fn equals(&self, other: Date) -> bool {
            self.inner == other.inner
        }

        /// This date written by a strftime pattern (`%Y/%m/%d`).
        pub fn format(&self, pattern: String) -> Result<String, FormatError> {
            strftime(&pattern, self.inner)
        }

        /// ISO 8601: what `parse` reads back.
        #[to_string]
        pub fn display(&self) -> String {
            self.inner.to_string()
        }
    }

    /// A time of day, in no zone. `tostring` gives ISO 8601 (`13:45:30`).
    #[derive(Debug, Clone, Copy, Object)]
    pub struct Time {
        inner: civil::Time,
    }

    #[mlua_ir::object]
    impl Time {
        /// The time of an hour (0-23), a minute (0-59), a second (0-59) and nanoseconds (0 when not given).
        pub fn new(hour: i64, minute: i64, second: i64, nanosecond: Option<i64>) -> Result<Time, ValueError> {
            civil::Time::new(
                number("hour", hour)?,
                number("minute", minute)?,
                number("second", second)?,
                number("nanosecond", nanosecond.unwrap_or(0))?,
            )
            .map(|inner| Time { inner })
            .map_err(|error| out_of_range(&error))
        }

        /// Read an ISO 8601 time (`13:45:30`).
        pub fn parse(text: String) -> Result<Time, ParseError> {
            civil::Time::from_str(&text).map(|inner| Time { inner }).map_err(|error| invalid(&text, &error))
        }

        /// `"time"`: what tells this from the other values `parse` gives.
        #[get]
        pub fn kind(&self) -> TimeKind {
            TimeKind::Time
        }

        /// The hour of the day (0-23).
        #[get]
        pub fn hour(&self) -> i8 {
            self.inner.hour()
        }

        /// The minute of the hour (0-59).
        #[get]
        pub fn minute(&self) -> i8 {
            self.inner.minute()
        }

        /// The second of the minute (0-59).
        #[get]
        pub fn second(&self) -> i8 {
            self.inner.second()
        }

        /// The nanoseconds past the second.
        #[get]
        pub fn nanosecond(&self) -> i32 {
            self.inner.subsec_nanosecond()
        }

        /// This time on a date.
        pub fn on(&self, date: Date) -> DateTime {
            DateTime { inner: date.inner.to_datetime(self.inner) }
        }

        /// How this orders against `other`.
        pub fn compare(&self, other: Time) -> Order {
            self.inner.cmp(&other.inner).into()
        }

        /// Whether this and `other` are the same time. (`==` asks whether they are the same object.)
        pub fn equals(&self, other: Time) -> bool {
            self.inner == other.inner
        }

        /// This time written by a strftime pattern (`%H:%M`).
        pub fn format(&self, pattern: String) -> Result<String, FormatError> {
            strftime(&pattern, self.inner)
        }

        /// ISO 8601: what `parse` reads back.
        #[to_string]
        pub fn display(&self) -> String {
            self.inner.to_string()
        }
    }

    /// A date and a time of day, in no zone. `tostring` gives ISO 8601 (`2026-05-20T13:45:30`).
    #[derive(Debug, Clone, Copy, Object)]
    pub struct DateTime {
        inner: civil::DateTime,
    }

    #[mlua_ir::object]
    impl DateTime {
        /// The datetime of a year, month, day, hour, minute, second and nanoseconds (0 when not given).
        #[allow(clippy::too_many_arguments)] // the seven numbers of a datetime, in their usual order
        pub fn new(
            year: i64,
            month: i64,
            day: i64,
            hour: i64,
            minute: i64,
            second: i64,
            nanosecond: Option<i64>,
        ) -> Result<DateTime, ValueError> {
            civil::DateTime::new(
                number("year", year)?,
                number("month", month)?,
                number("day", day)?,
                number("hour", hour)?,
                number("minute", minute)?,
                number("second", second)?,
                number("nanosecond", nanosecond.unwrap_or(0))?,
            )
            .map(|inner| DateTime { inner })
            .map_err(|error| out_of_range(&error))
        }

        /// Read an ISO 8601 datetime (`2026-05-20T13:45:30`).
        pub fn parse(text: String) -> Result<DateTime, ParseError> {
            civil::DateTime::from_str(&text)
                .map(|inner| DateTime { inner })
                .map_err(|error| invalid(&text, &error))
        }

        /// `"date_time"`: what tells this from the other values `parse` gives.
        #[get]
        pub fn kind(&self) -> DateTimeKind {
            DateTimeKind::DateTime
        }

        /// The calendar year.
        #[get]
        pub fn year(&self) -> i16 {
            self.inner.year()
        }

        /// The month of the year (1-12).
        #[get]
        pub fn month(&self) -> i8 {
            self.inner.month()
        }

        /// The day of the month (1-31).
        #[get]
        pub fn day(&self) -> i8 {
            self.inner.day()
        }

        /// The hour of the day (0-23).
        #[get]
        pub fn hour(&self) -> i8 {
            self.inner.hour()
        }

        /// The minute of the hour (0-59).
        #[get]
        pub fn minute(&self) -> i8 {
            self.inner.minute()
        }

        /// The second of the minute (0-59).
        #[get]
        pub fn second(&self) -> i8 {
            self.inner.second()
        }

        /// The nanoseconds past the second.
        #[get]
        pub fn nanosecond(&self) -> i32 {
            self.inner.subsec_nanosecond()
        }

        /// The date.
        pub fn date(&self) -> Date {
            Date { inner: self.inner.date() }
        }

        /// The time of day.
        pub fn time(&self) -> Time {
            Time { inner: self.inner.time() }
        }

        /// This datetime as the clocks of an IANA zone show it. A time the zone skips (a spring-forward gap) is
        /// moved forward by the gap; a time it has twice is the first of the two.
        ///
        /// ```luau
        /// local dt = assert(chrono.DateTime.parse("2026-05-20T12:00:00"))
        /// local z = assert(dt:in_zone("UTC"))
        /// assert(z.timezone == "UTC" and z.offset_seconds == 0)
        /// local _, err = dt:in_zone("Mars/Olympus_Mons")
        /// assert(err ~= nil and err.kind == "unknown_timezone")
        /// ```
        pub fn in_zone(&self, name: String) -> Result<ZonedDateTime, ZoneError> {
            let zone = zone(&name)?;
            self.inner
                .to_zoned_datetime(zone)
                .map(|inner| ZonedDateTime { inner })
                .map_err(|error| ZoneError::OutOfRange { reason: error.to_string() })
        }

        /// The datetime `duration` later.
        pub fn plus(&self, duration: Duration) -> Result<DateTime, ArithmeticError> {
            self.inner
                .checked_add(duration.inner)
                .map(|inner| DateTime { inner })
                .map_err(|error| overflow(&error))
        }

        /// The datetime `duration` earlier.
        pub fn minus(&self, duration: Duration) -> Result<DateTime, ArithmeticError> {
            self.inner
                .checked_sub(duration.inner)
                .map(|inner| DateTime { inner })
                .map_err(|error| overflow(&error))
        }

        /// The span from this datetime until `other` (negative when `other` is earlier).
        pub fn diff(&self, other: DateTime) -> Result<Duration, ArithmeticError> {
            self.inner.until(other.inner).map(|inner| Duration { inner }).map_err(|error| overflow(&error))
        }

        /// How this orders against `other`.
        pub fn compare(&self, other: DateTime) -> Order {
            self.inner.cmp(&other.inner).into()
        }

        /// Whether this and `other` are the same datetime. (`==` asks whether they are the same object.)
        pub fn equals(&self, other: DateTime) -> bool {
            self.inner == other.inner
        }

        /// This datetime written by a strftime pattern (`%Y-%m-%d %H:%M`).
        pub fn format(&self, pattern: String) -> Result<String, FormatError> {
            strftime(&pattern, self.inner)
        }

        /// ISO 8601: what `parse` reads back.
        #[to_string]
        pub fn display(&self) -> String {
            self.inner.to_string()
        }
    }

    /// An instant as the clocks of an IANA zone show it. `tostring` gives RFC 9557
    /// (`2026-01-20T16:30:00-05:00[America/New_York]`).
    #[derive(Debug, Clone, Object)]
    pub struct ZonedDateTime {
        inner: jiff::ZonedDateTime,
    }

    #[mlua_ir::object]
    impl ZonedDateTime {
        /// Read an RFC 9557 timestamp: RFC 3339 and the zone in brackets
        /// (`2026-01-20T16:30:00-05:00[America/New_York]`). The offset must be the zone's at that time.
        pub fn parse(text: String) -> Result<ZonedDateTime, ParseError> {
            jiff::ZonedDateTime::from_str(&text)
                .map(|inner| ZonedDateTime { inner })
                .map_err(|error| invalid(&text, &error))
        }

        /// `"zoned_date_time"`: what tells this from the other values `parse` gives.
        #[get]
        pub fn kind(&self) -> ZonedDateTimeKind {
            ZonedDateTimeKind::ZonedDateTime
        }

        /// The calendar year.
        #[get]
        pub fn year(&self) -> i16 {
            self.inner.year()
        }

        /// The month of the year (1-12).
        #[get]
        pub fn month(&self) -> i8 {
            self.inner.month()
        }

        /// The day of the month (1-31).
        #[get]
        pub fn day(&self) -> i8 {
            self.inner.day()
        }

        /// The hour of the day (0-23).
        #[get]
        pub fn hour(&self) -> i8 {
            self.inner.hour()
        }

        /// The minute of the hour (0-59).
        #[get]
        pub fn minute(&self) -> i8 {
            self.inner.minute()
        }

        /// The second of the minute (0-59).
        #[get]
        pub fn second(&self) -> i8 {
            self.inner.second()
        }

        /// The nanoseconds past the second.
        #[get]
        pub fn nanosecond(&self) -> i32 {
            self.inner.subsec_nanosecond()
        }

        /// The zone's IANA name; `nil` for a zone that is only an offset.
        #[get]
        pub fn timezone(&self) -> Option<String> {
            self.inner.time_zone().iana_name().map(str::to_owned)
        }

        /// The zone's offset from UTC at this instant, in seconds.
        #[get]
        pub fn offset_seconds(&self) -> i32 {
            self.inner.offset().seconds()
        }

        /// Seconds since the Unix epoch.
        #[get]
        pub fn epoch_seconds(&self) -> i64 {
            self.inner.timestamp().as_second()
        }

        /// The date the zone's clocks show.
        pub fn date(&self) -> Date {
            Date { inner: self.inner.date() }
        }

        /// The time of day the zone's clocks show.
        pub fn time(&self) -> Time {
            Time { inner: self.inner.time() }
        }

        /// The date and time the zone's clocks show.
        pub fn datetime(&self) -> DateTime {
            DateTime { inner: self.inner.datetime() }
        }

        /// The same instant as the clocks of another IANA zone show it.
        pub fn in_zone(&self, name: String) -> Result<ZonedDateTime, ZoneError> {
            Ok(ZonedDateTime { inner: self.inner.with_time_zone(zone(&name)?) })
        }

        /// The instant `duration` later. Hours and smaller units are elapsed time (two hours later is 7200 seconds
        /// later, whatever the clocks did); days and longer units follow the calendar.
        pub fn plus(&self, duration: Duration) -> Result<ZonedDateTime, ArithmeticError> {
            self.inner
                .checked_add(duration.inner)
                .map(|inner| ZonedDateTime { inner })
                .map_err(|error| overflow(&error))
        }

        /// The instant `duration` earlier.
        pub fn minus(&self, duration: Duration) -> Result<ZonedDateTime, ArithmeticError> {
            self.inner
                .checked_sub(duration.inner)
                .map(|inner| ZonedDateTime { inner })
                .map_err(|error| overflow(&error))
        }

        /// The span from this instant until `other` (negative when `other` is earlier).
        pub fn diff(&self, other: ZonedDateTime) -> Result<Duration, ArithmeticError> {
            self.inner.until(&other.inner).map(|inner| Duration { inner }).map_err(|error| overflow(&error))
        }

        /// How this instant orders against `other`, whatever their zones.
        pub fn compare(&self, other: ZonedDateTime) -> Order {
            self.inner.cmp(&other.inner).into()
        }

        /// Whether this and `other` are the same instant, whatever their zones. (`==` asks whether they are the
        /// same object.)
        pub fn equals(&self, other: ZonedDateTime) -> bool {
            self.inner == other.inner
        }

        /// This value written by a strftime pattern (`%Y-%m-%dT%H:%M:%S%:z` is strict RFC 3339).
        pub fn format(&self, pattern: String) -> Result<String, FormatError> {
            strftime(&pattern, &self.inner)
        }

        /// The instant in UTC, as RFC 3339 (`2026-01-20T21:30:00Z`).
        pub fn to_rfc3339(&self) -> String {
            self.inner.timestamp().to_string()
        }

        /// RFC 9557: what `parse` reads back.
        #[to_string]
        pub fn display(&self) -> String {
            self.inner.to_string()
        }
    }

    /// The units of a duration; one not given is zero.
    #[derive(Debug, Default, Serialize, Deserialize, Shape)]
    #[serde(deny_unknown_fields)]
    pub struct DurationUnits {
        /// Calendar years.
        pub years: Option<i64>,
        /// Calendar months.
        pub months: Option<i64>,
        /// Weeks.
        pub weeks: Option<i64>,
        /// Days.
        pub days: Option<i64>,
        /// Hours.
        pub hours: Option<i64>,
        /// Minutes.
        pub minutes: Option<i64>,
        /// Seconds.
        pub seconds: Option<i64>,
        /// Milliseconds.
        pub milliseconds: Option<i64>,
        /// Microseconds.
        pub microseconds: Option<i64>,
        /// Nanoseconds.
        pub nanoseconds: Option<i64>,
    }

    /// A span of time that keeps its units: an hour and a half is `hours = 1, minutes = 30`, and a month stays a
    /// month until it is added to a date. `tostring` gives ISO 8601 (`PT1H30M`).
    #[derive(Debug, Clone, Copy, Object)]
    pub struct Duration {
        inner: Span,
    }

    #[mlua_ir::object]
    impl Duration {
        /// A duration of the units given. A unit past what a duration holds (about 20 000 years) is raised, as a
        /// wrong argument is.
        ///
        /// ```luau
        /// local d = chrono.Duration.of({ hours = 1, minutes = 30 })
        /// assert(d.hours == 1 and d.minutes == 30 and d:total_seconds() == 5400)
        /// ```
        pub fn of(units: DurationUnits) -> Result<Duration, Raise<ValueError>> {
            let mut span = Span::new();
            macro_rules! apply {
                ($field:ident, $method:ident) => {
                    if let Some(value) = units.$field {
                        span = span.$method(value).map_err(|error| ValueError::OutOfRange {
                            reason: format!("{}: {error}", stringify!($field)),
                        })?;
                    }
                };
            }
            apply!(years, try_years);
            apply!(months, try_months);
            apply!(weeks, try_weeks);
            apply!(days, try_days);
            apply!(hours, try_hours);
            apply!(minutes, try_minutes);
            apply!(seconds, try_seconds);
            apply!(milliseconds, try_milliseconds);
            apply!(microseconds, try_microseconds);
            apply!(nanoseconds, try_nanoseconds);
            Ok(Duration { inner: span })
        }

        /// Read an ISO 8601 duration (`PT1H30M`, `P1Y2M`) or a friendly one (`1h 30m`).
        pub fn parse(text: String) -> Result<Duration, ParseError> {
            Span::from_str(&text).map(|inner| Duration { inner }).map_err(|error| invalid(&text, &error))
        }

        /// The years as given: units are not folded into each other (36 hours is `hours == 36`, not a day and a
        /// half).
        #[get]
        pub fn years(&self) -> i64 {
            i64::from(self.inner.get_years())
        }

        /// The months as given.
        #[get]
        pub fn months(&self) -> i64 {
            i64::from(self.inner.get_months())
        }

        /// The weeks as given.
        #[get]
        pub fn weeks(&self) -> i64 {
            i64::from(self.inner.get_weeks())
        }

        /// The days as given.
        #[get]
        pub fn days(&self) -> i64 {
            i64::from(self.inner.get_days())
        }

        /// The hours as given.
        #[get]
        pub fn hours(&self) -> i64 {
            i64::from(self.inner.get_hours())
        }

        /// The minutes as given.
        #[get]
        pub fn minutes(&self) -> i64 {
            self.inner.get_minutes()
        }

        /// The seconds as given.
        #[get]
        pub fn seconds(&self) -> i64 {
            self.inner.get_seconds()
        }

        /// The milliseconds as given.
        #[get]
        pub fn milliseconds(&self) -> i64 {
            self.inner.get_milliseconds()
        }

        /// The microseconds as given.
        #[get]
        pub fn microseconds(&self) -> i64 {
            self.inner.get_microseconds()
        }

        /// The nanoseconds as given.
        #[get]
        pub fn nanoseconds(&self) -> i64 {
            self.inner.get_nanoseconds()
        }

        /// The whole duration in seconds. A duration with years or months has none: their length depends on the
        /// date they are counted from.
        pub fn total_seconds(&self) -> Result<f64, TotalError> {
            self.inner
                .total(Unit::Second)
                .map_err(|error| TotalError::CalendarUnits { reason: error.to_string() })
        }

        /// The same length the other way: every unit's sign is flipped.
        pub fn negate(&self) -> Duration {
            Duration { inner: self.inner.negate() }
        }

        /// The same length, forward.
        pub fn abs(&self) -> Duration {
            Duration { inner: self.inner.abs() }
        }

        /// Whether this and `other` have the same units: an hour is not 60 minutes. (`==` asks whether they are
        /// the same object.)
        pub fn equals(&self, other: Duration) -> bool {
            self.inner.fieldwise() == other.inner.fieldwise()
        }

        /// ISO 8601: what `parse` reads back.
        #[to_string]
        pub fn display(&self) -> String {
            self.inner.to_string()
        }
    }

    /// What `parse` read: `kind` says which.
    #[derive(Debug, Clone, OneOf)]
    pub enum Temporal {
        /// A date alone (`2026-05-20`).
        Date(Date),
        /// A time alone (`13:45:30`).
        Time(Time),
        /// A date and a time with no offset (`2026-05-20T13:45:30`).
        DateTime(DateTime),
        /// A timestamp with a zone or an offset (`2026-05-20T16:30:00Z`).
        Zoned(ZonedDateTime),
    }

    /// The current instant in the system's timezone.
    pub fn now() -> ZonedDateTime {
        ZonedDateTime { inner: jiff::ZonedDateTime::now() }
    }

    /// The current instant in UTC.
    pub fn utc_now() -> ZonedDateTime {
        ZonedDateTime { inner: Timestamp::now().to_zoned_datetime(TimeZone::UTC) }
    }

    /// Read ISO 8601 / RFC 9557 text as what it is the shape of: a `Date`, a `Time`, a `DateTime`, or a
    /// `ZonedDateTime` (a timestamp with an offset and no zone name is put in UTC).
    ///
    /// ```luau
    /// local value = assert(chrono.parse("2026-05-20T16:30:00Z"))
    /// if value.kind == "zoned_date_time" then
    ///     assert(value.timezone == "UTC" and value.hour == 16)
    /// end
    /// ```
    pub fn parse(text: String) -> Result<Temporal, ParseError> {
        // By the text's shape and not by trying each parser in turn: `DateTime` reads a bare `2026-05-20` as
        // midnight, which would turn every date into a datetime.
        if text.contains('T') || text.contains(' ') {
            if let Ok(inner) = jiff::ZonedDateTime::from_str(&text) {
                return Ok(Temporal::Zoned(ZonedDateTime { inner }));
            }
            // RFC 3339 with `Z` or an offset and no `[zone]`: the instant is known, the zone is not.
            if let Ok(timestamp) = Timestamp::from_str(&text) {
                return Ok(Temporal::Zoned(ZonedDateTime {
                    inner: timestamp.to_zoned_datetime(TimeZone::UTC),
                }));
            }
            if let Ok(inner) = civil::DateTime::from_str(&text) {
                return Ok(Temporal::DateTime(DateTime { inner }));
            }
        } else if text.contains(':') {
            if let Ok(inner) = civil::Time::from_str(&text) {
                return Ok(Temporal::Time(Time { inner }));
            }
        } else if let Ok(inner) = civil::Date::from_str(&text) {
            return Ok(Temporal::Date(Date { inner }));
        }
        Err(ParseError::Unrecognised { text })
    }
}

#[cfg(test)]
mod tests {
    use crate::make_engine;

    fn eval<T: mlua::FromLuaMulti>(code: &str) -> T {
        let code = format!("local chrono = require(\"lib:chrono\")\n{code}");
        make_engine().unwrap().load(code).eval().unwrap()
    }

    #[test]
    fn date_new_and_fields() {
        let (y, m, d, weekday, ordinal): (i64, i64, i64, i64, i64) = eval(
            r"
            local d = chrono.Date.new(2026, 5, 20)
            local first = chrono.Date.new(2026, 1, 1)
            return d.year, d.month, d.day, first.weekday, first.ordinal
        ",
        );
        // 2026-01-01 is a Thursday.
        assert_eq!((y, m, d, weekday, ordinal), (2026, 5, 20, 4, 1));
    }

    #[test]
    fn date_invalid_is_a_typed_error() {
        let (nil, kind, text): (mlua::Value, String, String) = eval(
            r"
            local v, e = chrono.Date.new(2026, 2, 30)
            return v, e.kind, tostring(e)
        ",
        );
        assert!(nil.is_nil());
        assert_eq!(kind, "out_of_range");
        assert!(text.starts_with("lib:chrono.Date.new: out_of_range: "), "{text}");
    }

    #[test]
    fn a_number_too_large_for_the_calendar_is_out_of_range_too() {
        let (a, b, c): (String, String, String) = eval(
            r"
            local _, a = chrono.Date.new(2026, 300, 1)
            local _, b = chrono.Date.new(2026, 5, 20):with_year(99999)
            local _, c = chrono.Time.new(1, 2, 3, 2^40)
            return a.kind, b.kind, c.kind
        ",
        );
        assert_eq!((a.as_str(), b.as_str(), c.as_str()), ("out_of_range", "out_of_range", "out_of_range"));
    }

    #[test]
    fn date_parse_iso() {
        let s: String = eval(r#"return tostring(chrono.Date.parse("2026-05-20"))"#);
        assert_eq!(s, "2026-05-20");
    }

    #[test]
    fn date_parse_invalid_is_a_typed_error() {
        let (nil, kind, text, op): (mlua::Value, String, String, String) = eval(
            r#"
            local v, e = chrono.Date.parse("not a date")
            return v, e.kind, e.text, e.op
        "#,
        );
        assert!(nil.is_nil());
        assert_eq!(
            (kind.as_str(), text.as_str(), op.as_str()),
            ("invalid", "not a date", "lib:chrono.Date.parse")
        );
    }

    #[test]
    fn date_arithmetic() {
        let (plus, minus): (String, String) = eval(
            r#"
            local d = chrono.Date.parse("2026-05-20")
            local dur = chrono.Duration.of({ days = 10 })
            return tostring(d:plus(dur)), tostring(d:minus(dur))
        "#,
        );
        assert_eq!((plus.as_str(), minus.as_str()), ("2026-05-30", "2026-05-10"));
    }

    #[test]
    fn date_arithmetic_refuses_sub_day_units() {
        let (a, b): (String, String) = eval(
            r#"
            local d = chrono.Date.parse("2026-05-20")
            local _, a = d:plus(chrono.Duration.of({ hours = 25 }))
            local _, b = d:minus(chrono.Duration.of({ minutes = 60 }))
            return a.kind, b.kind
        "#,
        );
        assert_eq!((a.as_str(), b.as_str()), ("sub_day_units", "sub_day_units"));
    }

    #[test]
    fn date_diff_returns_duration() {
        let days: i64 = eval(
            r#"
            local a = chrono.Date.parse("2026-05-20")
            local b = chrono.Date.parse("2026-05-30")
            return a:diff(b).days
        "#,
        );
        assert_eq!(days, 10);
    }

    #[test]
    fn date_comparison_methods() {
        let (eq, lt, gt, same, differ): (String, String, String, bool, bool) = eval(
            r#"
            local a = chrono.Date.parse("2026-05-20")
            local b = chrono.Date.parse("2026-05-30")
            local c = chrono.Date.parse("2026-05-20")
            return a:compare(c), a:compare(b), b:compare(a), a:equals(c), a:equals(b)
        "#,
        );
        assert_eq!((eq.as_str(), lt.as_str(), gt.as_str()), ("equal", "less", "greater"));
        assert!(same && !differ);
    }

    #[test]
    fn date_with_fields() {
        let (y, m, d): (String, String, String) = eval(
            r"
            local d = chrono.Date.new(2026, 5, 20)
            return tostring(d:with_year(2030)), tostring(d:with_month(1)), tostring(d:with_day(1))
        ",
        );
        assert_eq!((y.as_str(), m.as_str(), d.as_str()), ("2030-05-20", "2026-01-20", "2026-05-01"));
    }

    #[test]
    fn time_new_and_fields() {
        let (h, m, s, ns): (i64, i64, i64, i64) = eval(
            r"
            local t = chrono.Time.new(13, 45, 30)
            return t.hour, t.minute, t.second, t.nanosecond
        ",
        );
        assert_eq!((h, m, s, ns), (13, 45, 30, 0));
    }

    #[test]
    fn time_invalid_is_a_typed_error() {
        let (nil, kind): (mlua::Value, String) = eval(
            r"
            local v, e = chrono.Time.new(25, 0, 0)
            return v, e.kind
        ",
        );
        assert!(nil.is_nil());
        assert_eq!(kind, "out_of_range");
    }

    #[test]
    fn time_orders() {
        let (order, same): (String, bool) = eval(
            r"
            local a, b = chrono.Time.new(1, 0, 0), chrono.Time.new(2, 0, 0)
            return a:compare(b), a:equals((chrono.Time.new(1, 0, 0)))
        ",
        );
        assert_eq!(order, "less");
        assert!(same);
    }

    #[test]
    fn datetime_compose_from_date_and_time() {
        let (at, on): (String, String) = eval(
            r"
            local d = chrono.Date.new(2026, 5, 20)
            local t = chrono.Time.new(13, 45, 30)
            return tostring(d:at(t)), tostring(t:on(d))
        ",
        );
        assert_eq!(at, "2026-05-20T13:45:30");
        assert_eq!(on, at);
    }

    #[test]
    fn datetime_new_parts_and_arithmetic() {
        let (date, time, later, hours, order): (String, String, String, i64, String) = eval(
            r"
            local dt = chrono.DateTime.new(2026, 5, 20, 13, 45, 30)
            local later = dt:plus(chrono.Duration.of({ hours = 12 }))
            return tostring(dt:date()), tostring(dt:time()), tostring(later), dt:diff(later).hours,
                later:minus(chrono.Duration.of({ hours = 12 })):compare(dt)
        ",
        );
        assert_eq!(date, "2026-05-20");
        assert_eq!(time, "13:45:30");
        assert_eq!(later, "2026-05-21T01:45:30");
        assert_eq!(hours, 12);
        assert_eq!(order, "equal");
    }

    #[test]
    fn datetime_in_zone_returns_zoned() {
        let (s, tz, offset): (String, Option<String>, i64) = eval(
            r#"
            local dt = chrono.DateTime.parse("2026-05-20T12:00:00")
            local z = dt:in_zone("UTC")
            return tostring(z), z.timezone, z.offset_seconds
        "#,
        );
        assert!(s.starts_with("2026-05-20T12:00:00+00:00"));
        assert_eq!(tz.as_deref(), Some("UTC"));
        assert_eq!(offset, 0);
    }

    #[test]
    fn in_zone_moves_a_time_in_a_dst_gap_forward() {
        // 2026-03-08 02:30 does not exist in America/New_York (spring forward): it becomes 03:30 EDT.
        let (hour, minute, offset): (i64, i64, i64) = eval(
            r#"
            local z = chrono.DateTime.parse("2026-03-08T02:30:00"):in_zone("America/New_York")
            return z.hour, z.minute, z.offset_seconds
        "#,
        );
        assert_eq!((hour, minute, offset), (3, 30, -4 * 3600));
    }

    #[test]
    fn unknown_zone_is_a_typed_error() {
        let (nil, kind, name, op): (mlua::Value, String, String, String) = eval(
            r#"
            local dt = chrono.DateTime.parse("2026-05-20T12:00:00")
            local v, e = dt:in_zone("Mars/Olympus_Mons")
            return v, e.kind, e.name, e.op
        "#,
        );
        assert!(nil.is_nil());
        assert_eq!(kind, "unknown_timezone");
        assert_eq!(name, "Mars/Olympus_Mons");
        assert_eq!(op, "lib:chrono.DateTime:in_zone");
    }

    #[test]
    fn zdt_parse_round_trip() {
        // A winter date: `-05:00` is America/New_York's standard offset, and jiff refuses an offset that is not the
        // zone's.
        let (s, tz, offset, rfc): (String, String, i64, String) = eval(
            r#"
            local z = chrono.ZonedDateTime.parse("2026-01-20T16:30:00-05:00[America/New_York]")
            return tostring(z), z.timezone, z.offset_seconds, z:to_rfc3339()
        "#,
        );
        assert_eq!(s, "2026-01-20T16:30:00-05:00[America/New_York]");
        assert_eq!(tz, "America/New_York");
        assert_eq!(offset, -5 * 3600);
        assert_eq!(rfc, "2026-01-20T21:30:00Z");
    }

    #[test]
    fn zdt_in_zone_preserves_instant() {
        let (a, b, order, same): (i64, i64, String, bool) = eval(
            r#"
            local z = chrono.ZonedDateTime.parse("2026-01-20T16:30:00-05:00[America/New_York]")
            local w = z:in_zone("UTC")
            return z.epoch_seconds, w.epoch_seconds, z:compare(w), z:equals(w)
        "#,
        );
        assert_eq!(a, b);
        assert_eq!(order, "equal");
        assert!(same, "the same instant, whatever the zone");
    }

    #[test]
    fn zdt_plus_is_elapsed_time_across_a_dst_gap() {
        let (elapsed, back): (i64, i64) = eval(
            r#"
            local z = chrono.ZonedDateTime.parse("2026-03-08T01:30:00-05:00[America/New_York]")
            local two = chrono.Duration.of({ hours = 2 })
            local r = z:plus(two)
            return r.epoch_seconds - z.epoch_seconds, z:diff((r:minus(two))):total_seconds()
        "#,
        );
        assert_eq!((elapsed, back), (2 * 3600, 0));
    }

    #[test]
    fn zdt_parts() {
        let (date, time, datetime): (String, String, String) = eval(
            r#"
            local z = chrono.ZonedDateTime.parse("2026-01-20T16:30:00-05:00[America/New_York]")
            return tostring(z:date()), tostring(z:time()), tostring(z:datetime())
        "#,
        );
        assert_eq!(
            (date.as_str(), time.as_str(), datetime.as_str()),
            ("2026-01-20", "16:30:00", "2026-01-20T16:30:00")
        );
    }

    #[test]
    fn duration_of_and_total_seconds() {
        let (hours, minutes, total): (i64, i64, f64) = eval(
            r"
            local d = chrono.Duration.of({ hours = 1, minutes = 30 })
            return d.hours, d.minutes, d:total_seconds()
        ",
        );
        assert_eq!((hours, minutes), (1, 30));
        assert!((total - 5400.0).abs() < 1e-9);
    }

    #[test]
    fn duration_total_seconds_refuses_calendar_units() {
        let (nil, kind): (mlua::Value, String) = eval(
            r"
            local v, e = chrono.Duration.of({ months = 1 }):total_seconds()
            return v, e.kind
        ",
        );
        assert!(nil.is_nil());
        assert_eq!(kind, "calendar_units");
    }

    #[test]
    fn duration_of_refuses_what_is_not_a_unit() {
        let lua = make_engine().unwrap();
        for units in ["{ hours = 1.5 }", "{ hours = 'one' }", "{ hour = 1 }", "5"] {
            let err = lua
                .load(format!(r#"return require("lib:chrono").Duration.of({units})"#))
                .eval::<mlua::Value>()
                .expect_err("not units")
                .to_string();
            assert!(err.contains("lib:chrono.Duration.of: argument #1 'units'"), "{units}: {err}");
        }
    }

    #[test]
    fn duration_of_raises_a_unit_too_large() {
        let (ok, text): (bool, String) = eval(
            r"
            local ok, err = pcall(chrono.Duration.of, { years = 1000000 })
            return ok, tostring(err)
        ",
        );
        assert!(!ok);
        assert!(text.contains("lib:chrono.Duration.of: out_of_range: years: "), "{text}");
    }

    #[test]
    fn duration_parse_iso8601() {
        // jiff writes its canonical form back.
        let s: String = eval(r#"return tostring(chrono.Duration.parse("PT1H30M"))"#);
        assert_eq!(s, "PT1H30M");
    }

    #[test]
    fn duration_negate_abs_equals() {
        let (negated, abs, same, differ): (i64, i64, bool, bool) = eval(
            r"
            local d = chrono.Duration.of({ hours = 2 })
            return d:negate().hours, d:negate():abs().hours, d:equals(chrono.Duration.of({ hours = 2 })),
                d:equals(chrono.Duration.of({ minutes = 120 }))
        ",
        );
        assert_eq!((negated, abs), (-2, 2));
        assert!(same && !differ);
    }

    #[test]
    fn flat_parse_dispatches_by_shape() {
        let kinds: Vec<String> = eval(
            r#"
            local kinds = {}
            for _, text in {
                "2026-01-20T16:30:00-05:00[America/New_York]",
                "2026-05-20T16:30:00Z",
                "2026-05-20T16:30:00+00:00",
                "2026-05-20T13:45:30",
                "2026-05-20",
                "13:45:30",
            } do
                table.insert(kinds, chrono.parse(text).kind)
            end
            return kinds
        "#,
        );
        assert_eq!(
            kinds,
            ["zoned_date_time", "zoned_date_time", "zoned_date_time", "date_time", "date", "time"]
        );
    }

    #[test]
    fn flat_parse_puts_an_offset_without_a_zone_in_utc() {
        let (tz, offset, hour): (String, i64, i64) = eval(
            r#"
            local v = chrono.parse("2026-05-20T16:30:00Z")
            return v.timezone, v.offset_seconds, v.hour
        "#,
        );
        assert_eq!((tz.as_str(), offset, hour), ("UTC", 0, 16));
    }

    #[test]
    fn flat_parse_unknown_is_a_typed_error() {
        let (nil, kind, text): (mlua::Value, String, String) = eval(
            r#"
            local v, e = chrono.parse("not a timestamp")
            return v, e.kind, e.text
        "#,
        );
        assert!(nil.is_nil());
        assert_eq!((kind.as_str(), text.as_str()), ("unrecognised", "not a timestamp"));
    }

    #[test]
    fn utc_now_zone_is_utc() {
        let tz: Option<String> = eval(r"return chrono.utc_now().timezone");
        assert_eq!(tz.as_deref(), Some("UTC"));
    }

    #[test]
    fn now_returns_zoned() {
        let (first, positive): (String, bool) =
            eval(r"return tostring(chrono.now()):sub(1, 1), chrono.now().epoch_seconds > 0");
        assert!(first.chars().next().unwrap().is_ascii_digit());
        assert!(positive);
    }

    #[test]
    fn strftime_formats_each_type() {
        let (date, time, datetime, zoned): (String, String, String, String) = eval(
            r#"
            local z = chrono.ZonedDateTime.parse("2026-01-20T16:30:00-05:00[America/New_York]")
            return chrono.Date.new(2026, 5, 20):format("%Y/%m/%d"), chrono.Time.new(13, 45, 30):format("%H.%M"),
                chrono.DateTime.new(2026, 5, 20, 13, 45, 30):format("%Y %H"), z:format("%Y-%m-%dT%H:%M:%S%:z")
        "#,
        );
        assert_eq!(date, "2026/05/20");
        assert_eq!(time, "13.45");
        assert_eq!(datetime, "2026 13");
        assert_eq!(zoned, "2026-01-20T16:30:00-05:00");
    }

    #[test]
    fn an_invalid_pattern_is_a_typed_error_for_each_type() {
        let kinds: Vec<String> = eval(
            r#"
            local z = chrono.ZonedDateTime.parse("2026-01-20T16:30:00-05:00[America/New_York]")
            local kinds = {}
            for _, value in { chrono.Date.new(2026, 5, 20), chrono.Time.new(1, 2, 3), z:datetime(), z } do
                local text, err = value:format("%@@@invalid")
                assert(text == nil and err.pattern == "%@@@invalid")
                table.insert(kinds, err.kind)
            end
            return kinds
        "#,
        );
        assert_eq!(kinds, ["invalid_pattern"; 4]);
    }
}
