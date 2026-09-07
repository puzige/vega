//! Local civil-date bucketing; elapsed hours cannot model DST calendar boundaries.
use crate::types::{SidebarOrganizationError, SidebarTimelineBucket};

/// Buckets a unix-millisecond timestamp by system-local calendar dates.
/// Future dates belong to Today. Seven/thirty-day windows include today.
pub fn local_calendar_bucket(
    timestamp_ms: i64,
    now_ms: i64,
) -> Result<SidebarTimelineBucket, SidebarOrganizationError> {
    let elapsed = local_day(now_ms)? - local_day(timestamp_ms)?;
    Ok(bucket_days(elapsed))
}
fn bucket_days(elapsed: i64) -> SidebarTimelineBucket {
    match elapsed {
        ..=0 => SidebarTimelineBucket::Today,
        1 => SidebarTimelineBucket::Yesterday,
        2..=6 => SidebarTimelineBucket::Last7Days,
        7..=29 => SidebarTimelineBucket::Last30Days,
        _ => SidebarTimelineBucket::Earlier,
    }
}
fn local_day(timestamp_ms: i64) -> Result<i64, SidebarOrganizationError> {
    let seconds: libc::time_t = timestamp_ms.div_euclid(1000);
    let mut output = std::mem::MaybeUninit::<libc::tm>::uninit();
    // SAFETY: localtime_r writes one valid tm to the provided allocation on success.
    let result = unsafe { libc::localtime_r(&seconds, output.as_mut_ptr()) };
    if result.is_null() {
        return Err(SidebarOrganizationError::Invalid(
            "timestamp exceeds the local calendar range".into(),
        ));
    }
    // SAFETY: the successful non-null call above initialized the entire tm.
    let civil = unsafe { output.assume_init() };
    let year = i64::from(civil.tm_year) + 1900;
    let previous = year - 1;
    Ok(
        365 * previous + previous.div_euclid(4) - previous.div_euclid(100)
            + previous.div_euclid(400)
            + i64::from(civil.tm_yday),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn calendar_windows_have_exact_inclusive_boundaries() {
        use SidebarTimelineBucket::*;
        for (days, expected) in [
            (-1, Today),
            (0, Today),
            (1, Yesterday),
            (2, Last7Days),
            (6, Last7Days),
            (7, Last30Days),
            (29, Last30Days),
            (30, Earlier),
        ] {
            assert_eq!(bucket_days(days), expected);
        }
    }
    #[test]
    fn local_midnight_is_a_calendar_boundary() {
        let seconds: libc::time_t = 1_710_100_800;
        let mut output = std::mem::MaybeUninit::<libc::tm>::uninit();
        assert!(!unsafe { libc::localtime_r(&seconds, output.as_mut_ptr()) }.is_null());
        let mut civil = unsafe { output.assume_init() };
        civil.tm_hour = 0;
        civil.tm_min = 0;
        civil.tm_sec = 0;
        civil.tm_isdst = -1;
        let midnight = unsafe { libc::mktime(&mut civil) } * 1000;
        assert_eq!(
            local_calendar_bucket(midnight - 1, midnight).unwrap(),
            SidebarTimelineBucket::Yesterday
        );
        assert_eq!(
            local_calendar_bucket(midnight + 1, midnight).unwrap(),
            SidebarTimelineBucket::Today
        );
    }
}

#[cfg(test)]
mod dst_tests {
    use super::*;
    fn midnight(month: i32, day: i32) -> i64 {
        // SAFETY: zero is valid for every integer/pointer field of libc tm; mktime
        // reads the civil input fields initialized below and fills derived fields.
        let mut civil: libc::tm = unsafe { std::mem::zeroed() };
        civil.tm_year = 124;
        civil.tm_mon = month - 1;
        civil.tm_mday = day;
        civil.tm_isdst = -1;
        (unsafe { libc::mktime(&mut civil) }) * 1000
    }
    #[test]
    fn calendar_dates_survive_spring_and_autumn_dst_transitions() {
        let spring = (midnight(3, 10), midnight(3, 11));
        let autumn = (midnight(11, 3), midnight(11, 4));
        for (before, after) in [spring, autumn] {
            assert_eq!(
                local_calendar_bucket(before, after).unwrap(),
                SidebarTimelineBucket::Yesterday
            );
            assert_eq!(
                local_calendar_bucket(after - 1, after).unwrap(),
                SidebarTimelineBucket::Yesterday
            );
        }
        if std::env::var("TZ").as_deref() == Ok("America/New_York") {
            assert_eq!(spring.1 - spring.0, 23 * 60 * 60 * 1000);
            assert_eq!(autumn.1 - autumn.0, 25 * 60 * 60 * 1000);
        }
    }
}
