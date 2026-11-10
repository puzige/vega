use super::*;

#[test]
fn native_sidebar_relative_time_boundaries() {
    for (time, label) in [(-1, "now"), (59_999, "now"), (60_000, "1m"), (3_599_999, "59m"), (3_600_000, "1h"), (86_399_999, "23h"), (86_400_000, "1d"), (604_799_999, "6d"), (604_800_000, "1970-01-01")] {
        assert_eq!(relative_time_from(0, time), label);
    }
}

#[test]
fn native_sidebar_schedules_the_first_visible_timestamp_change() {
    let now = now_ms();
    let deadline = std::cell::Cell::new(i64::MAX);
    assert_eq!(relative_time(now + 600_000, Some(&deadline)), "now");
    assert_eq!(deadline.get(), now + 660_000);
    assert_eq!(relative_time(now - 70_000, Some(&deadline)), "1m");
    assert_eq!(deadline.get(), now + 50_000);
    let before = deadline.get();
    let _ = relative_time(now - 8 * 86_400_000, Some(&deadline));
    assert_eq!(deadline.get(), before);
}
