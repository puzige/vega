use super::*;

#[test]
fn session_row_actions_take_precedence_over_every_background_status() {
    for running in [false, true] {
        for unread in [false, true] {
            for timestamp in [false, true] {
                assert_eq!(
                    session_row_trailing(true, running, unread, timestamp),
                    SessionRowTrailing::Actions
                );
            }
        }
    }
}

#[test]
fn session_row_running_unread_and_existing_timestamp_are_mutually_exclusive() {
    for unread in [false, true] {
        for timestamp in [false, true] {
            assert_eq!(
                session_row_trailing(false, true, unread, timestamp),
                SessionRowTrailing::Running
            );
        }
    }
    for timestamp in [false, true] {
        assert_eq!(
            session_row_trailing(false, false, true, timestamp),
            SessionRowTrailing::Unread
        );
    }
    assert_eq!(
        session_row_trailing(false, false, false, true),
        SessionRowTrailing::Timestamp
    );
    assert_eq!(
        session_row_trailing(false, false, false, false),
        SessionRowTrailing::Empty
    );
}
