use super::Reveal;

#[test]
fn escape_dismissal_survives_repeated_hover_and_focus_until_they_leave() {
    let mut reveal = Reveal::default();
    assert!(reveal.hover(true));
    assert!(reveal.focus(true));
    assert!(reveal.visible());
    reveal.dismiss();
    assert!(!reveal.visible());
    for _ in 0..32 {
        assert!(!reveal.hover(true));
        assert!(!reveal.focus(true));
        assert!(!reveal.visible());
    }
    assert!(reveal.hover(false));
    assert!(!reveal.visible());
    assert!(reveal.hover(true));
    assert!(reveal.visible());
    reveal.dismiss();
    assert!(reveal.focus(false));
    assert!(!reveal.visible());
    assert!(reveal.focus(true));
    assert!(reveal.visible());
}

#[test]
fn leaving_one_reveal_source_preserves_the_other_real_source() {
    let mut reveal = Reveal::default();
    reveal.hover(true);
    reveal.focus(true);
    reveal.hover(false);
    assert!(reveal.visible());
    reveal.hover(true);
    reveal.focus(false);
    assert!(reveal.visible());
    reveal.hover(false);
    assert!(!reveal.visible());
}

#[test]
fn revoked_mount_reset_does_not_keep_previous_hover_or_escape_state() {
    let mut reveal = Reveal::default();
    reveal.hover(true);
    reveal.focus(true);
    reveal.dismiss();
    reveal = Reveal::default();
    assert!(!reveal.visible());
    assert!(!reveal.hovered);
    assert!(!reveal.focused);
    assert!(reveal.focus(true));
    assert!(reveal.visible());
}
