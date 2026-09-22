//! Turning the provider's clock into something readable.
//!
//! No date crate, and no `SystemTime::now()` either: the time comes from
//! [`crate::provider::SystemProvider::now_unix_seconds`], so a test can pin it
//! and a journal written during a test is byte-for-byte predictable.

/// `YYYY-MM-DDTHH:MM:SSZ` from seconds since the Unix epoch.
pub fn iso_utc(seconds: u64) -> String {
    let (year, month, day) = civil_from_days(seconds / 86_400);
    let time_of_day = seconds % 86_400;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        time_of_day / 3600,
        (time_of_day % 3600) / 60,
        time_of_day % 60
    )
}

/// `YYYYMMDD-HHMMSS`, for a folder name that sorts chronologically.
pub fn stamp(seconds: u64) -> String {
    let (year, month, day) = civil_from_days(seconds / 86_400);
    let time_of_day = seconds % 86_400;
    format!(
        "{year:04}{month:02}{day:02}-{:02}{:02}{:02}",
        time_of_day / 3600,
        (time_of_day % 3600) / 60,
        time_of_day % 60
    )
}

/// Days since 1970-01-01 as a calendar date.
///
/// Howard Hinnant's `civil_from_days`, which shifts the year to start in March
/// so that the leap day lands at the end and the month lengths follow a
/// pattern instead of a table.
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let days = days as i64 + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 };
    let year = if month <= 2 { year + 1 } else { year };

    (year.max(0) as u64, month as u64, day as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_known_instants() {
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(stamp(1_700_000_000), "20231114-221320");
    }

    #[test]
    fn handles_the_leap_day() {
        // 2024-02-29, the date every hand-rolled calendar gets wrong.
        assert_eq!(iso_utc(1_709_164_800), "2024-02-29T00:00:00Z");
    }
}
