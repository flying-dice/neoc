//! Calendar arithmetic without a time library: the host hands apps `ctx.now` as an ISO-8601 UTC string.

const MS_PER_DAY: u64 = 86_400_000;

/// `2026-11-18T09:12:00.123Z` for Unix milliseconds.
#[must_use]
pub fn iso8601_utc(unix_ms: u64) -> String {
    let days = i64::try_from(unix_ms / MS_PER_DAY).unwrap_or(i64::MAX / 2);
    let ms_of_day = unix_ms % MS_PER_DAY;
    let (year, month, day) = civil_from_days(days);
    let (h, rem) = (ms_of_day / 3_600_000, ms_of_day % 3_600_000);
    let (min, rem) = (rem / 60_000, rem % 60_000);
    let (s, ms) = (rem / 1000, rem % 1000);
    format!("{year:04}-{month:02}-{day:02}T{h:02}:{min:02}:{s:02}.{ms:03}Z")
}

/// Days since 1970-01-01 -> (year, month, day) in the proleptic Gregorian calendar (Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097); // day of era, 0..=146096
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let month = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_known_instants() {
        assert_eq!(iso8601_utc(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso8601_utc(1_000_000_000_123), "2001-09-09T01:46:40.123Z");
        assert_eq!(iso8601_utc(1_763_457_120_000), "2025-11-18T09:12:00.000Z");
    }

    #[test]
    fn handles_leap_days_and_century_rules() {
        assert_eq!(iso8601_utc(951_782_400_000), "2000-02-29T00:00:00.000Z"); // divisible by 400
        assert_eq!(iso8601_utc(1_709_164_800_000), "2024-02-29T00:00:00.000Z");
        assert_eq!(iso8601_utc(4_107_542_400_000), "2100-03-01T00:00:00.000Z"); // 2100 is not a leap year
    }

    #[test]
    fn sorts_like_the_instants_it_formats() {
        let a = iso8601_utc(1_700_000_000_000);
        let b = iso8601_utc(1_700_000_000_001);
        assert!(a < b, "app SQL compares these strings");
    }
}
