use super::*;
use gpui::TestAppContext;

#[test]
fn history_navigation_preserves_failure_owner_until_the_next_worker_starts() {
    let mut cx = TestAppContext::single();
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(SettingsOpen(false));
        crate::init(cx);
    });
    let thread = Thread {
        last_response_failed: true,
        reasoning_preferences: None,
        parent_thread_id: None,
        forked_from_thread_id: None,
        forked_from_message_id: None,
        id: "thread".into(),
        backend: TaskBackend::Native,
        project_id: "project".into(),
        title: "test".into(),
        mode: ThreadMode::Execute,
        permission_mode: PermissionMode::Confirm,
        permission_revision: 0,
        draft_authority_epoch: None,
        full_access_grant: None,
        default_permission: None,
        model: "synthetic".into(),
        status: vega_conversation::types::ThreadStatus::Active,
        pinned: false,
        unread: false,
        created_at: 1,
        updated_at: 1,
    };
    let stream = cx.new(|cx| ConversationStream::new(thread, cx));
    stream.update(&mut cx, |stream, cx| {
        stream.apply_agent_runtime_error_for(Some("old"), RunFailureKind::RuntimeStorage, cx);
        let page = HistoryPage {
            interrupted_tail: None,
            subagents: vec![],
            entries: vec![HistoryEntry::UserText {
                seq: 1,
                message_id: "user".into(),
                content: "synthetic".into(),
            }],
            older_cursor: None,
            newer_cursor: None,
            newest_seq: Some(1),
        };
        stream.replace_history_window(page, "user", cx);
        assert_eq!(
            stream.controller_runtime_error,
            Some(("old".into(), RunFailureKind::RuntimeStorage))
        );
        assert_eq!(
            stream.controller_error.as_ref().unwrap().render(),
            RunFailureKind::RuntimeStorage.message()
        );
        stream.begin_composer_run(cx);
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "new".into(),
                seq: 4,
            },
            cx,
        );
        stream.apply_agent_runtime_error_for(Some("old"), RunFailureKind::RuntimeStorage, cx);
        assert!(stream.actions.running);
        assert!(stream.controller_error.is_none());
        stream.apply_agent_runtime_error_for(Some("new"), RunFailureKind::ConfigurationBusy, cx);
        assert!(stream.actions.running);
        stream.finish_composer_run(false, cx);
        assert!(!stream.actions.running);
        assert_eq!(
            stream.controller_runtime_error,
            Some(("new".into(), RunFailureKind::ConfigurationBusy))
        );
    });
}
