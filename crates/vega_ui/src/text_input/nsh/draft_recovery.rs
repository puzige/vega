use super::DraftRecovery;

#[test]
fn synchronized_draft_stays_without_a_recovery_action_across_two_mounts() {
    let draft = DraftRecovery::new(String::new());
    draft.preserve_unsynchronized("");
    assert_eq!(draft.saved(), None);
    draft.local("新草稿");
    draft.preserve_unsynchronized("新草稿");
    let next = draft.restarted("新草稿".into());
    next.preserve_unsynchronized("新草稿");
    assert_eq!(next.saved(), None);
}

#[test]
fn unsynchronized_deletion_remains_recoverable_after_a_restart() {
    let draft = DraftRecovery::new("before".into());
    draft.local("");
    draft.preserve_unsynchronized("before");
    let next = draft.restarted("before".into());
    next.preserve_unsynchronized("before");
    assert_eq!(next.saved().as_deref(), Some(""));
}

#[test]
fn restored_copy_is_consumed_without_removing_a_different_pending_copy() {
    let draft = DraftRecovery::new("server".into());
    draft.local("A");
    draft.preserve_unsynchronized("server");
    let next = draft.restarted("server".into());
    assert!(!next.consume_restored("stale"));
    assert_eq!(next.saved().as_deref(), Some("A"));
    assert!(next.consume_restored("A"));
    let edited = next.restarted("A".into());
    edited.local("B");
    edited.preserve_unsynchronized("B");
    assert_eq!(edited.restarted("B".into()).saved(), None);
    edited.local("C");
    edited.preserve_unsynchronized("B");
    assert!(!edited.consume_restored("A"));
    assert_eq!(edited.saved().as_deref(), Some("C"));
}
