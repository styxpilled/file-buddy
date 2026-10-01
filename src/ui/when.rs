//! Times for people: a date and time in the local zone, and how long ago a
//! file changed, in words that stay the same for a while.

use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};

/// Seconds since the Unix epoch as a local date and time.
fn local(seconds: i64) -> Option<SYSTEMTIME> {
    // A FILETIME counts 100 ns steps from 1601.
    const EPOCH_GAP: i64 = 11_644_473_600;
    let ticks = u64::try_from(seconds + EPOCH_GAP).ok()? * 10_000_000;
    let file_time = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let mut utc = SYSTEMTIME::default();
    let mut local = SYSTEMTIME::default();
    // SAFETY: both calls only read and write the structs passed to them.
    unsafe {
        FileTimeToSystemTime(&file_time, &mut utc)
            .and_then(|()| SystemTimeToTzSpecificLocalTime(None, &utc, &mut local))
            .ok()?;
    }
    Some(local)
}

/// Seconds since the Unix epoch, as a date and time in the zone of this
/// machine, with the daylight saving rule of that date.
pub fn local_time(seconds: i64) -> String {
    let Some(local) = local(seconds) else {
        return String::new();
    };
    format!(
        "{}-{:02}-{:02} {:02}:{:02}",
        local.wYear, local.wMonth, local.wDay, local.wHour, local.wMinute
    )
}

/// The days from 1970-01-01 to a date, so two dates can be subtracted.
fn day_number(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// How long ago `then` was at `now`, both in seconds since the Unix epoch. The
/// words change seldom, so the line does not tick while it is on screen.
pub fn ago(then: i64, now: i64) -> String {
    let seconds = now - then;
    if seconds < 60 {
        return "just now".into();
    }
    if seconds < 3600 {
        return "minutes ago".into();
    }
    if seconds < 7200 {
        return "an hour ago".into();
    }
    let (Some(then), Some(now)) = (local(then), local(now)) else {
        return String::new();
    };
    let date =
        |time: SYSTEMTIME| day_number(time.wYear.into(), time.wMonth.into(), time.wDay.into());
    match date(now) - date(then) {
        0 => "today".into(),
        1 => "yesterday".into(),
        2..7 => "this week".into(),
        7..31 => "this month".into(),
        31..365 => "this year".into(),
        _ => then.wYear.to_string(),
    }
}

/// `ago` at the current time.
pub fn ago_now(then: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(then);
    ago(then, now)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_reads_as_a_date() {
        let text = local_time(1_758_628_800);
        assert_eq!(text.len(), "2025-09-23 12:00".len(), "{text}");
        assert!(text.starts_with("2025-09-2"), "{text}");
    }

    #[test]
    fn day_numbers_count_from_1970() {
        assert_eq!(day_number(1970, 1, 1), 0);
        assert_eq!(day_number(2000, 3, 1), 11_017);
        assert_eq!(day_number(2024, 3, 1) - day_number(2024, 2, 28), 2);
    }

    #[test]
    fn the_words_change_seldom() {
        const HOUR: i64 = 3600;
        const DAY: i64 = 24 * HOUR;
        let now = 1_758_628_800;
        assert_eq!(ago(now, now), "just now");
        assert_eq!(ago(now + 30, now), "just now");
        assert_eq!(ago(now - 10 * 60, now), "minutes ago");
        assert_eq!(ago(now - 90 * 60, now), "an hour ago");
        assert_eq!(ago(now - 3 * DAY, now), "this week");
        assert_eq!(ago(now - 20 * DAY, now), "this month");
        assert_eq!(ago(now - 200 * DAY, now), "this year");
        assert_eq!(ago(now - 800 * DAY, now), "2023");
    }
}
