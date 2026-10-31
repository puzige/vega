use vega_i18n::{t, t_args};

const MAX_TIMESTAMP_MS: i64 = 253_402_300_799_999;
const WEEKDAY_KEYS: [&str; 7] = [
    "ui.timestamp.weekday_sunday",
    "ui.timestamp.weekday_monday",
    "ui.timestamp.weekday_tuesday",
    "ui.timestamp.weekday_wednesday",
    "ui.timestamp.weekday_thursday",
    "ui.timestamp.weekday_friday",
    "ui.timestamp.weekday_saturday",
];
const SHORT_WEEKDAY_KEYS: [&str; 7] = [
    "ui.timestamp.weekday_short_sunday",
    "ui.timestamp.weekday_short_monday",
    "ui.timestamp.weekday_short_tuesday",
    "ui.timestamp.weekday_short_wednesday",
    "ui.timestamp.weekday_short_thursday",
    "ui.timestamp.weekday_short_friday",
    "ui.timestamp.weekday_short_saturday",
];
const MONTH_KEYS: [&str; 12] = [
    "ui.timestamp.month_january",
    "ui.timestamp.month_february",
    "ui.timestamp.month_march",
    "ui.timestamp.month_april",
    "ui.timestamp.month_may",
    "ui.timestamp.month_june",
    "ui.timestamp.month_july",
    "ui.timestamp.month_august",
    "ui.timestamp.month_september",
    "ui.timestamp.month_october",
    "ui.timestamp.month_november",
    "ui.timestamp.month_december",
];

pub(super) struct TimestampDisplay {
    pub(super) day: String,
    pub(super) time: String,
    pub(super) separator: String,
    pub(super) message: String,
    pub(super) accessible: String,
}

pub(super) fn format_timestamp(timestamp_ms: i64, now_ms: i64) -> Option<TimestampDisplay> {
    if timestamp_ms > now_ms {
        return None;
    }
    let timestamp = local_calendar(timestamp_ms)?;
    let now = local_calendar(now_ms)?;
    format_from_calendars(&timestamp, &now)
}

fn format_from_calendars(timestamp: &libc::tm, now: &libc::tm) -> Option<TimestampDisplay> {
    let elapsed_days = calendar_ordinal(now) - calendar_ordinal(timestamp);
    if elapsed_days < 0 {
        return None;
    }
    let time = format_time(timestamp);
    let day = match elapsed_days {
        0 => t("ui.timestamp.today").to_owned(),
        1 => t("ui.timestamp.yesterday").to_owned(),
        2..=7 => t(WEEKDAY_KEYS[timestamp.tm_wday as usize]).to_owned(),
        8..=365 => format_date(timestamp, "ui.timestamp.date_weekday_month_day"),
        _ => format_date(timestamp, "ui.timestamp.date_month_day_year"),
    };
    let separator = join_date_time(
        if elapsed_days <= 7 {
            "ui.timestamp.separator_recent"
        } else {
            "ui.timestamp.separator_older"
        },
        &day,
        &time,
    );
    let message = match elapsed_days {
        0 => time.clone(),
        1..=6 => join_date_time(
            "ui.timestamp.message",
            t(WEEKDAY_KEYS[timestamp.tm_wday as usize]),
            &time,
        ),
        _ => join_date_time(
            "ui.timestamp.message",
            &format_date(timestamp, "ui.timestamp.date_month_day"),
            &time,
        ),
    };
    let accessible = join_date_time(
        "ui.timestamp.accessible",
        &format_date(timestamp, "ui.timestamp.date_full"),
        &time,
    );
    Some(TimestampDisplay {
        day,
        time,
        separator,
        message,
        accessible,
    })
}

fn local_calendar(timestamp_ms: i64) -> Option<libc::tm> {
    if !(1..=MAX_TIMESTAMP_MS).contains(&timestamp_ms) {
        return None;
    }
    let seconds: libc::time_t = timestamp_ms.div_euclid(1000);
    let mut output = std::mem::MaybeUninit::<libc::tm>::uninit();
    // SAFETY: localtime_r receives valid pointers to seconds and writable storage for one tm.
    let result = unsafe { libc::localtime_r(&seconds, output.as_mut_ptr()) };
    if result.is_null() {
        return None;
    }
    // SAFETY: the successful non-null localtime_r call initialized the entire tm.
    let civil = unsafe { output.assume_init() };
    if !(0..=11).contains(&civil.tm_mon)
        || !(1..=31).contains(&civil.tm_mday)
        || !(0..=6).contains(&civil.tm_wday)
        || !(0..=365).contains(&civil.tm_yday)
        || !(0..=23).contains(&civil.tm_hour)
        || !(0..=59).contains(&civil.tm_min)
    {
        return None;
    }
    Some(civil)
}

fn calendar_ordinal(civil: &libc::tm) -> i64 {
    let previous_year = i64::from(civil.tm_year) + 1899;
    365 * previous_year + previous_year.div_euclid(4) - previous_year.div_euclid(100)
        + previous_year.div_euclid(400)
        + i64::from(civil.tm_yday)
}

fn format_time(civil: &libc::tm) -> String {
    let minute = format!("{:02}", civil.tm_min);
    t_args(
        "ui.timestamp.time_24",
        &[("hour", &civil.tm_hour.to_string()), ("minute", &minute)],
    )
}

fn format_date(civil: &libc::tm, template: &'static str) -> String {
    t_args(
        template,
        &[
            ("weekday", t(WEEKDAY_KEYS[civil.tm_wday as usize])),
            (
                "weekday_short",
                t(SHORT_WEEKDAY_KEYS[civil.tm_wday as usize]),
            ),
            ("month", t(MONTH_KEYS[civil.tm_mon as usize])),
            ("day", &civil.tm_mday.to_string()),
            ("year", &(i64::from(civil.tm_year) + 1900).to_string()),
        ],
    )
}

fn join_date_time(template: &'static str, date: &str, time: &str) -> String {
    t_args(template, &[("date", date), ("time", time)])
}
