use super::*;
use gpui_kit::{AppContext, FollowMode, ListOffset, TestAppContext};
use vega_conversation::ordinary_submissions::{OrdinaryWorker, RawSubmission};
use vega_conversation::types::ThreadStatus;
use vega_store::{Store, thread_admission};

fn fixture(
    cx: &mut TestAppContext,
    text: &str,
    images: Vec<ImageAttachment>,
) -> (
    Entity<ConversationStream>,
    OrdinarySubmission,
    OrdinaryWorker,
    ComposerSubmitted,
) {
    let store = Store::open(":memory:").unwrap();
    let database = store.database_identity();
    let thread_id = database.permission_fence_key();
    let thread = Thread {
        id: thread_id.clone(),
        backend: TaskBackend::Native,
        project_id: String::new(),
        title: "Placement regression".into(),
        mode: ThreadMode::Execute,
        permission_mode: PermissionMode::Confirm,
        permission_revision: 0,
        draft_authority_epoch: None,
        full_access_grant: None,
        default_permission: None,
        model: "fixture-model".into(),
        status: ThreadStatus::Active,
        pinned: false,
        unread: false,
        created_at: 0,
        updated_at: 0,
        reasoning_preferences: None,
        parent_thread_id: None,
        forked_from_thread_id: None,
        forked_from_message_id: None,
    };
    let admission = thread_admission::try_acquire(database, &thread_id)
        .unwrap()
        .unwrap();
    let raw = RawSubmission::new(
        text.into(),
        images.clone(),
        Vec::new(),
        0,
        thread.model.clone(),
        None,
        None,
    );
    let (submission, worker) =
        OrdinarySubmission::begin(database, thread_id.clone(), raw, admission).unwrap();
    let request = ComposerSubmitted {
        request_id: 1,
        draft_revision: 0,
        attachment_ids: Vec::new(),
        pending_behavior: None,
        images,
        thread_id,
        content: text.into(),
        references: Vec::new(),
        reference_mentions: Vec::new(),
        skill_intent: None,
        reasoning: Ok(None),
    };
    let stream = cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        cx.new(|cx| ConversationStream::new(thread, cx))
    });
    stream.update(cx, |stream, _| {
        for _ in 0..3 {
            stream.entries.push(StreamEntry::User {
                copy: MessageCopy::new("Earlier turn"),
                lines: user_message_lines(0, "Earlier turn"),
                model: user_markdown_model("Earlier turn"),
            });
        }
        stream.list_append(0);
        stream.list.reset_with_uniform_height(3, px(80.));
        stream.list.set_follow_mode(FollowMode::Tail);
    });
    (stream, submission, worker, request)
}

fn image() -> ImageAttachment {
    ImageAttachment::from_bytes(vec![
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6,
        0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 11, 73, 68, 65, 84, 120, 156, 99, 248, 15, 4, 0, 9,
        251, 3, 253, 251, 94, 107, 43, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ])
    .unwrap()
}

#[gpui_kit::test]
fn ordinary_send_places_first_new_item_at_top(cx: &mut TestAppContext) {
    for (text, images, added) in [
        ("New turn", Vec::new(), 1),
        ("", vec![image()], 1),
        ("With image", vec![image()], 2),
    ] {
        let (stream, submission, _worker, request) = fixture(cx, text, images);
        stream.update(cx, |stream, cx| {
            assert!(stream.begin_ordinary_pending(submission, &request, cx));
            assert_eq!(stream.entries.len(), 3 + added);
            assert_eq!(stream.list.item_count(), 3 + added);
            let top = stream.list.logical_scroll_top();
            assert_eq!(top.item_ix, 3);
            assert_eq!(top.offset_in_item, px(0.));
            assert!(!stream.list.is_following_tail());
            assert!(stream.turn_placement.borrow().active());
            if !request.images.is_empty() {
                assert!(matches!(stream.entries[3], StreamEntry::UserImages { .. }));
            }
        });
    }
}

#[gpui_kit::test]
fn repeated_projection_preserves_user_scroll(cx: &mut TestAppContext) {
    let (stream, submission, _worker, request) = fixture(cx, "New turn", Vec::new());
    stream.update(cx, |stream, cx| {
        assert!(stream.begin_ordinary_pending(submission.clone(), &request, cx));
        stream.turn_placement.borrow_mut().reset();
        stream.list.set_follow_mode(FollowMode::Normal);
        stream.list.scroll_to(ListOffset {
            item_ix: 1,
            offset_in_item: px(17.),
        });
        let anchor = stream.scroll_anchor_snapshot();
        for _ in 0..2 {
            stream.project_ordinary_submission(submission.clone(), cx);
            assert!(!stream.begin_ordinary_pending(submission.clone(), &request, cx));
            assert_eq!(stream.entries.len(), 4);
            let actual = stream.scroll_anchor_snapshot();
            assert_eq!(actual.identity, anchor.identity);
            assert_eq!(actual.offset_in_item_px, anchor.offset_in_item_px);
            assert_eq!(actual.following_tail, anchor.following_tail);
            assert!(!stream.turn_placement.borrow().active());
        }
    });
}

#[gpui_kit::test]
fn history_recovery_projects_missing_turn_without_placement(cx: &mut TestAppContext) {
    let (stream, submission, _worker, _) = fixture(cx, "Recovered turn", Vec::new());
    stream.update(cx, |stream, cx| {
        stream.project_ordinary_submission(submission.clone(), cx);
        assert_eq!(stream.entries.len(), 4);
        assert!(stream.list.is_following_tail());
        assert!(!stream.turn_placement.borrow().active());
        let retained = stream.take_ordinary_history_entries();
        assert_eq!(retained.len(), 1);
        drop(retained);
        assert!(stream.entries.is_empty());
        for _ in 0..3 {
            stream.entries.push(StreamEntry::User {
                copy: MessageCopy::new("Restored history"),
                lines: user_message_lines(0, "Restored history"),
                model: user_markdown_model("Restored history"),
            });
        }
        stream.list.reset_with_uniform_height(3, px(80.));
        stream.ensure_entry_identities();
        stream.list.set_follow_mode(FollowMode::Normal);
        stream.list.scroll_to(ListOffset {
            item_ix: 1,
            offset_in_item: px(19.),
        });
        let anchor = stream.scroll_anchor_snapshot();
        stream.restore_ordinary_history_entries(Vec::new(), cx);
        assert_eq!(stream.entries.len(), 4);
        assert_eq!(stream.list.item_count(), 4);
        let actual = stream.scroll_anchor_snapshot();
        assert_eq!(actual.identity, anchor.identity);
        assert_eq!(actual.offset_in_item_px, anchor.offset_in_item_px);
        assert_eq!(actual.following_tail, anchor.following_tail);
        assert!(!stream.turn_placement.borrow().active());
        assert!(stream.entry_identities.iter().any(|identity| {
            identity.message_id.as_deref() == Some(submission.user_message_id())
        }));
    });
}
