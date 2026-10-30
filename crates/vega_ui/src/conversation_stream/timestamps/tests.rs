use super::TimestampAdjacency;

#[test]
fn loaded_first_user_uses_strict_one_hour_threshold() {
    for (age, expected) in [(3_599_999, false), (3_600_000, false), (3_600_001, true)] {
        let mut adjacency = TimestampAdjacency::default();
        assert_eq!(
            adjacency.push(true, Some(1_000_000), 1_000_000 + age),
            expected
        );
    }
}

#[test]
fn assistant_to_user_uses_strict_one_hour_gap() {
    for (gap, expected) in [(3_600_000, false), (3_600_001, true)] {
        let mut adjacency = TimestampAdjacency::default();
        assert!(!adjacency.push(true, Some(900_000), 900_000));
        assert!(!adjacency.push(false, Some(1_000_000), 9_000_000));
        assert_eq!(
            adjacency.push(true, Some(1_000_000 + gap), 9_000_000),
            expected
        );
    }
}

#[test]
fn assistant_to_assistant_uses_strict_ten_minute_gap() {
    for (gap, expected) in [(600_000, false), (600_001, true)] {
        let mut adjacency = TimestampAdjacency::default();
        assert!(!adjacency.push(false, Some(1_000_000), 9_000_000));
        assert_eq!(
            adjacency.push(false, Some(1_000_000 + gap), 9_000_000),
            expected
        );
    }
}

#[test]
fn user_predecessor_does_not_trigger_assistant_gap() {
    let mut adjacency = TimestampAdjacency::default();
    assert!(!adjacency.push(true, Some(1_000_000), 1_000_000));
    assert!(!adjacency.push(false, Some(9_000_000), 9_000_000));
}

#[test]
fn independent_boundary_suppresses_first_user_and_previous_assistant() {
    let mut adjacency = TimestampAdjacency::default();
    assert!(!adjacency.push(false, Some(1_000_000), 9_000_000));
    adjacency.reset();
    assert!(!adjacency.push(true, Some(2_000_000), 9_000_000));
}

#[test]
fn absent_time_does_not_compare_across_gap() {
    let mut adjacency = TimestampAdjacency::default();
    assert!(!adjacency.push(false, Some(1_000_000), 9_000_000));
    assert!(!adjacency.push(false, None, 9_000_000));
    assert!(!adjacency.push(true, Some(8_000_000), 9_000_000));
}

#[test]
fn reverse_time_does_not_create_separator() {
    let mut adjacency = TimestampAdjacency::default();
    assert!(!adjacency.push(false, Some(8_000_000), 9_000_000));
    assert!(!adjacency.push(false, Some(1_000_000), 9_000_000));
}

#[test]
fn rebuilding_loaded_window_replaces_page_header_with_real_adjacency() {
    let mut page = TimestampAdjacency::default();
    assert!(page.push(true, Some(5_000_000), 10_000_000));
    let mut combined = TimestampAdjacency::default();
    assert!(combined.push(true, Some(4_000_000), 10_000_000));
    assert!(!combined.push(false, Some(4_999_999), 10_000_000));
    assert!(!combined.push(true, Some(5_000_000), 10_000_000));
}

#[test]
fn missing_assistant_time_suppresses_first_user_special_case() {
    let mut adjacency = TimestampAdjacency::default();
    assert!(!adjacency.push(false, None, 10_000_000));
    assert!(!adjacency.push(true, Some(1_000_000), 10_000_000));
}
