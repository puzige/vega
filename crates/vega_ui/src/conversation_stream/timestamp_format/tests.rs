use super::*;
use std::sync::{Mutex, MutexGuard};

static LANGUAGE_LOCK: Mutex<()> = Mutex::new(());

struct LanguageGuard {
    previous: &'static str,
    _lock: MutexGuard<'static, ()>,
}

impl LanguageGuard {
    fn set(language: &str) -> Self {
        let lock = LANGUAGE_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let previous = vega_i18n::language();
        vega_i18n::set_language(language);
        Self {
            previous,
            _lock: lock,
        }
    }
}

impl Drop for LanguageGuard {
    fn drop(&mut self) {
        vega_i18n::set_language(self.previous);
    }
}

fn civil([year, month, day, year_day, weekday]: [i32; 5], [hour, minute]: [i32; 2]) -> libc::tm {
    // SAFETY: zero is valid for every integer and null pointer field of libc::tm.
    let mut value: libc::tm = unsafe { std::mem::zeroed() };
    value.tm_year = year - 1900;
    value.tm_mon = month - 1;
    value.tm_mday = day;
    value.tm_yday = year_day;
    value.tm_wday = weekday;
    value.tm_hour = hour;
    value.tm_min = minute;
    value
}

fn fields(display: &TimestampDisplay) -> [&str; 5] {
    [
        &display.day,
        &display.time,
        &display.separator,
        &display.message,
        &display.accessible,
    ]
}

#[test]
fn english_calendar_tiers_keep_separator_and_message_boundaries_distinct() {
    let _language = LanguageGuard::set("en-US");
    let now = civil([2024, 3, 10, 69, 0], [15, 0]);
    let cases = [
        (
            [2024, 3, 10, 69, 0],
            [
                "Today",
                "1:05 PM",
                "Today 1:05 PM",
                "1:05 PM",
                "Sunday, Mar 10, 2024 at 1:05 PM",
            ],
        ),
        (
            [2024, 3, 9, 68, 6],
            [
                "Yesterday",
                "1:05 PM",
                "Yesterday 1:05 PM",
                "Saturday 1:05 PM",
                "Saturday, Mar 9, 2024 at 1:05 PM",
            ],
        ),
        (
            [2024, 3, 8, 67, 5],
            [
                "Friday",
                "1:05 PM",
                "Friday 1:05 PM",
                "Friday 1:05 PM",
                "Friday, Mar 8, 2024 at 1:05 PM",
            ],
        ),
        (
            [2024, 3, 4, 63, 1],
            [
                "Monday",
                "1:05 PM",
                "Monday 1:05 PM",
                "Monday 1:05 PM",
                "Monday, Mar 4, 2024 at 1:05 PM",
            ],
        ),
        (
            [2024, 3, 3, 62, 0],
            [
                "Sunday",
                "1:05 PM",
                "Sunday 1:05 PM",
                "Mar 3 1:05 PM",
                "Sunday, Mar 3, 2024 at 1:05 PM",
            ],
        ),
        (
            [2024, 3, 2, 61, 6],
            [
                "Sat, Mar 2",
                "1:05 PM",
                "Sat, Mar 2 at 1:05 PM",
                "Mar 2 1:05 PM",
                "Saturday, Mar 2, 2024 at 1:05 PM",
            ],
        ),
        (
            [2023, 3, 11, 69, 6],
            [
                "Sat, Mar 11",
                "1:05 PM",
                "Sat, Mar 11 at 1:05 PM",
                "Mar 11 1:05 PM",
                "Saturday, Mar 11, 2023 at 1:05 PM",
            ],
        ),
        (
            [2023, 3, 10, 68, 5],
            [
                "Mar 10, 2023",
                "1:05 PM",
                "Mar 10, 2023 at 1:05 PM",
                "Mar 10 1:05 PM",
                "Friday, Mar 10, 2023 at 1:05 PM",
            ],
        ),
    ];
    for (date, expected) in cases {
        let display = format_from_calendars(&civil(date, [13, 5]), &now).unwrap();
        assert_eq!(fields(&display), expected, "date: {date:?}");
    }
}

#[test]
fn chinese_calendar_tiers_use_localized_dates_and_twenty_four_hour_time() {
    let _language = LanguageGuard::set("zh-CN");
    let now = civil([2024, 3, 10, 69, 0], [15, 0]);
    let cases = [
        (
            [2024, 3, 10, 69, 0],
            [
                "今天",
                "13:05",
                "今天 13:05",
                "13:05",
                "2024年3月10日 星期日 13:05",
            ],
        ),
        (
            [2024, 3, 9, 68, 6],
            [
                "昨天",
                "13:05",
                "昨天 13:05",
                "星期六 13:05",
                "2024年3月9日 星期六 13:05",
            ],
        ),
        (
            [2024, 3, 8, 67, 5],
            [
                "星期五",
                "13:05",
                "星期五 13:05",
                "星期五 13:05",
                "2024年3月8日 星期五 13:05",
            ],
        ),
        (
            [2024, 3, 4, 63, 1],
            [
                "星期一",
                "13:05",
                "星期一 13:05",
                "星期一 13:05",
                "2024年3月4日 星期一 13:05",
            ],
        ),
        (
            [2024, 3, 3, 62, 0],
            [
                "星期日",
                "13:05",
                "星期日 13:05",
                "3月3日 13:05",
                "2024年3月3日 星期日 13:05",
            ],
        ),
        (
            [2024, 3, 2, 61, 6],
            [
                "3月2日 周六",
                "13:05",
                "3月2日 周六 13:05",
                "3月2日 13:05",
                "2024年3月2日 星期六 13:05",
            ],
        ),
        (
            [2023, 3, 11, 69, 6],
            [
                "3月11日 周六",
                "13:05",
                "3月11日 周六 13:05",
                "3月11日 13:05",
                "2023年3月11日 星期六 13:05",
            ],
        ),
        (
            [2023, 3, 10, 68, 5],
            [
                "2023年3月10日",
                "13:05",
                "2023年3月10日 13:05",
                "3月10日 13:05",
                "2023年3月10日 星期五 13:05",
            ],
        ),
    ];
    for (date, expected) in cases {
        let display = format_from_calendars(&civil(date, [13, 5]), &now).unwrap();
        assert_eq!(fields(&display), expected, "date: {date:?}");
    }
}

#[test]
fn locale_clock_conventions_handle_midnight_noon_and_padded_minutes() {
    for (language, expected) in [
        (
            "en",
            [
                "12:00 AM", "12:05 AM", "11:09 AM", "12:00 PM", "12:05 PM", "11:59 PM",
            ],
        ),
        (
            "zh-CN",
            ["0:00", "0:05", "11:09", "12:00", "12:05", "23:59"],
        ),
    ] {
        let _language = LanguageGuard::set(language);
        let now = civil([2024, 3, 10, 69, 0], [23, 59]);
        for (time, expected) in [[0, 0], [0, 5], [11, 9], [12, 0], [12, 5], [23, 59]]
            .into_iter()
            .zip(expected)
        {
            let display = format_from_calendars(&civil([2024, 3, 10, 69, 0], time), &now).unwrap();
            assert_eq!(display.time, expected);
            assert_eq!(display.message, expected);
        }
    }
}

#[test]
fn crossing_midnight_uses_calendar_days_across_leap_days_and_year_boundaries() {
    let _language = LanguageGuard::set("en");
    let cases = [
        (
            [2023, 12, 31, 364, 0],
            [2024, 1, 1, 0, 1],
            "Sunday 11:59 PM",
            "Sunday, Dec 31, 2023 at 11:59 PM",
        ),
        (
            [2024, 2, 28, 58, 3],
            [2024, 2, 29, 59, 4],
            "Wednesday 11:59 PM",
            "Wednesday, Feb 28, 2024 at 11:59 PM",
        ),
        (
            [2024, 2, 29, 59, 4],
            [2024, 3, 1, 60, 5],
            "Thursday 11:59 PM",
            "Thursday, Feb 29, 2024 at 11:59 PM",
        ),
        (
            [2000, 12, 31, 365, 0],
            [2001, 1, 1, 0, 1],
            "Sunday 11:59 PM",
            "Sunday, Dec 31, 2000 at 11:59 PM",
        ),
        (
            [2100, 12, 31, 364, 5],
            [2101, 1, 1, 0, 6],
            "Friday 11:59 PM",
            "Friday, Dec 31, 2100 at 11:59 PM",
        ),
    ];
    for (before, after, message, accessible) in cases {
        let display =
            format_from_calendars(&civil(before, [23, 59]), &civil(after, [0, 1])).unwrap();
        assert_eq!(
            fields(&display),
            [
                "Yesterday",
                "11:59 PM",
                "Yesterday 11:59 PM",
                message,
                accessible
            ]
        );
    }
}

#[test]
fn spring_forward_civil_hours_do_not_change_the_calendar_day_tier() {
    let _language = LanguageGuard::set("en");
    let mut before = civil([2024, 3, 10, 69, 0], [1, 59]);
    let mut after = civil([2024, 3, 10, 69, 0], [3, 1]);
    before.tm_isdst = 0;
    after.tm_isdst = 1;
    let display = format_from_calendars(&before, &after).unwrap();
    assert_eq!(
        fields(&display),
        [
            "Today",
            "1:59 AM",
            "Today 1:59 AM",
            "1:59 AM",
            "Sunday, Mar 10, 2024 at 1:59 AM"
        ]
    );
    let next_day = civil([2024, 3, 11, 70, 1], [0, 1]);
    let display = format_from_calendars(&after, &next_day).unwrap();
    assert_eq!(display.separator, "Yesterday 3:01 AM");
}

#[test]
fn fall_back_repeated_civil_hours_keep_the_same_local_display() {
    let _language = LanguageGuard::set("en");
    let mut first = civil([2024, 11, 3, 307, 0], [1, 30]);
    let mut repeated = first;
    first.tm_isdst = 1;
    repeated.tm_isdst = 0;
    let display = format_from_calendars(&first, &repeated).unwrap();
    assert_eq!(
        fields(&display),
        [
            "Today",
            "1:30 AM",
            "Today 1:30 AM",
            "1:30 AM",
            "Sunday, Nov 3, 2024 at 1:30 AM"
        ]
    );
    let next_day = civil([2024, 11, 4, 308, 1], [0, 1]);
    let display = format_from_calendars(&repeated, &next_day).unwrap();
    assert_eq!(display.separator, "Yesterday 1:30 AM");
}

#[test]
fn later_civil_calendar_dates_are_not_formatted_as_today() {
    let _language = LanguageGuard::set("en");
    assert!(
        format_from_calendars(
            &civil([2024, 3, 11, 70, 1], [0, 0]),
            &civil([2024, 3, 10, 69, 0], [23, 59]),
        )
        .is_none()
    );
}

#[test]
fn invalid_out_of_range_and_future_unix_values_are_hidden() {
    let _language = LanguageGuard::set("en");
    let now_ms = 1_710_054_060_000;
    for timestamp_ms in [i64::MIN, -1, 0, 253_402_300_800_000, i64::MAX] {
        assert!(
            format_timestamp(timestamp_ms, now_ms).is_none(),
            "timestamp: {timestamp_ms}"
        );
        assert!(
            format_timestamp(now_ms, timestamp_ms).is_none(),
            "now: {timestamp_ms}"
        );
        assert!(
            format_timestamp(timestamp_ms, timestamp_ms).is_none(),
            "equal invalid timestamps: {timestamp_ms}"
        );
    }
    assert!(format_timestamp(now_ms + 1, now_ms).is_none());
    assert!(format_timestamp(1, now_ms).is_some());
    assert!(format_timestamp(now_ms, now_ms).is_some());
    assert!(format_timestamp(253_402_300_799_999, 253_402_300_799_999).is_some());
}

fn system_calendar(timestamp_ms: i64) -> libc::tm {
    let seconds = (timestamp_ms / 1000) as libc::time_t;
    let mut output = std::mem::MaybeUninit::<libc::tm>::uninit();
    // SAFETY: seconds and output are valid pointers to time_t and writable tm storage.
    let result = unsafe { libc::localtime_r(&seconds, output.as_mut_ptr()) };
    assert!(
        !result.is_null(),
        "system localtime_r rejected {timestamp_ms}"
    );
    // SAFETY: a non-null localtime_r return guarantees output was initialized.
    unsafe { output.assume_init() }
}

#[test]
fn fixed_unix_instants_use_each_system_local_calendar_without_timezone_overrides() {
    for language in ["en", "zh-CN"] {
        let _language = LanguageGuard::set(language);
        for (timestamp_ms, now_ms) in [
            (1_710_053_940_000, 1_710_054_060_000),
            (1_730_611_800_000, 1_730_615_400_000),
            (1_704_067_140_000, 1_704_067_260_000),
        ] {
            let timestamp = system_calendar(timestamp_ms);
            let now = system_calendar(now_ms);
            let expected = format_from_calendars(&timestamp, &now).unwrap();
            let display = format_timestamp(timestamp_ms, now_ms).unwrap();
            assert_eq!(
                fields(&display),
                fields(&expected),
                "language: {language}, timestamp: {timestamp_ms}, now: {now_ms}"
            );
        }
    }
}
