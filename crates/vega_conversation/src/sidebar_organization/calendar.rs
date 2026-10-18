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
