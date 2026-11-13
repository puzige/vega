use super::run_owner_matches;
use gpui_kit::EntityId;

#[test]
fn stale_run_cannot_cross_terminal_owner_fence() {
    let current = (12, "thread", EntityId::from(1));
    assert!(run_owner_matches(current, current));
    assert!(!run_owner_matches(
        current,
        (11, "thread", EntityId::from(1))
    ));
    assert!(!run_owner_matches(
        current,
        (12, "other", EntityId::from(1))
    ));
    assert!(!run_owner_matches(
        current,
        (12, "thread", EntityId::from(2))
    ));
}

#[test]
fn appended_message_does_not_change_run_owner() {
    let owner = (12, "thread", EntityId::from(1));
    assert!(run_owner_matches(owner, owner));
    let replacement = (13, "thread", EntityId::from(1));
    assert!(!run_owner_matches(replacement, owner));
    assert!(run_owner_matches(replacement, replacement));
}
