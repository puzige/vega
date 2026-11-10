use super::Authority;

#[test]
fn resized_and_restored_width_rejects_old_layout_authority() {
    let mut current = Authority::default();
    current.update(320., 2.);
    let observed = current.clone();
    assert!(current.matches(&observed));
    current.update(110., 2.);
    assert!(!current.matches(&observed));
    assert!(!current.accepts_size([320., 84.]));
    current.update(320., 2.);
    assert!(!current.matches(&observed));
    assert!(current.accepts_size([320., 84.]));
}

#[test]
fn scale_change_and_remount_cannot_adopt_previous_geometry() {
    let mut first = Authority::default();
    first.update(320., 1.);
    let observed = first.clone();
    first.update(320., 2.);
    assert!(!first.matches(&observed));
    first.update(320., 1.);
    assert!(!first.matches(&observed));
    let mut remounted = Authority::default();
    remounted.update(320., 1.);
    assert!(!remounted.matches(&observed));
    assert!(remounted.accepts_size([320., 53.]));
}

#[test]
fn only_actual_finite_positive_geometry_for_current_width_is_accepted() {
    let mut authority = Authority::default();
    assert!(!authority.accepts_size([320., 84.]));
    authority.update(320., 2.);
    for invalid in [
        [0., 84.],
        [320., 0.],
        [320., -1.],
        [f32::NAN, 84.],
        [320., f32::INFINITY],
        [110., 84.],
    ] {
        assert!(!authority.accepts_size(invalid));
    }
    assert!(authority.accepts_size([320., 29.25]));
    assert!(authority.accepts_size([320., 281.75]));
}
