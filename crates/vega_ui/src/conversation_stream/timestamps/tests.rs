use super::{StreamEntryIdentity, TimestampState};

#[test]
fn running_assistant_time_is_hidden_without_affecting_other_messages() {
    let mut state = TimestampState::default();
    state.start_assistant("current-assistant");

    assert_eq!(state.message_time("current-assistant", Some(100)), None);
    assert_eq!(state.message_time("sent-user", Some(90)), Some(90));
    assert_eq!(state.message_time("previous-assistant", Some(80)), Some(80));
}

#[test]
fn continuation_keeps_all_current_assistant_times_hidden_until_release() {
    let mut state = TimestampState::default();
    state.start_assistant("first-assistant");
    state.start_assistant("next-assistant");

    assert_eq!(state.message_time("first-assistant", Some(100)), None);
    assert_eq!(state.message_time("next-assistant", Some(200)), None);
    assert_eq!(state.message_time("steering-user", Some(150)), Some(150));

    state.dirty = false;
    state.finish_run();
    assert!(state.dirty);
    assert_eq!(state.message_time("first-assistant", Some(100)), Some(100));
    assert_eq!(state.message_time("next-assistant", Some(200)), Some(200));
}

#[test]
fn released_run_does_not_hide_prior_assistants_in_the_next_run() {
    let mut state = TimestampState::default();
    state.start_assistant("previous-assistant");
    state.finish_run();
    state.start_assistant("current-assistant");

    assert_eq!(
        state.message_time("previous-assistant", Some(100)),
        Some(100)
    );
    assert_eq!(state.message_time("current-assistant", Some(200)), None);
}

#[test]
fn absent_timestamp_stays_absent_after_run_release() {
    let mut state = TimestampState::default();
    state.start_assistant("assistant");
    assert_eq!(state.message_time("assistant", None), None);
    state.finish_run();
    assert_eq!(state.message_time("assistant", None), None);
}

#[test]
fn running_time_cache_prevents_per_token_projection_invalidation() {
    let mut state = TimestampState::default();
    let identity = StreamEntryIdentity {
        key: "assistant-entry".into(),
        message_id: Some("assistant".into()),
        pending_id: None,
        echo_kind: None,
        sequence: None,
    };
    assert!(!state.has_message_time(0, std::slice::from_ref(&identity)));
    state.hidden_messages.insert(identity.key.clone());
    assert!(state.has_message_time(0, std::slice::from_ref(&identity)));
    assert!(!state.has_message_time(1, &[identity]));
}

#[test]
fn repeated_start_does_not_invalidate_without_a_state_change() {
    let mut state = TimestampState::default();
    state.start_assistant("assistant");
    assert!(state.dirty);
    state.dirty = false;
    state.start_assistant("assistant");
    assert!(!state.dirty);
    state.finish_run();
    assert!(state.dirty);
    state.dirty = false;
    state.finish_run();
    assert!(!state.dirty);
}
