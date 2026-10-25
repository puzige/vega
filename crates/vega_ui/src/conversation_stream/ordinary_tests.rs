use super::*;
use gpui_kit::gpui;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{AppContext, Bounds, ListOffset, TestAppContext, WindowHandle, size};
use std::cell::RefCell;
use std::rc::Rc;
use vega_conversation::ordinary_submissions::for_thread;
use vega_conversation::types::{
    CommittedReceipt, OrdinarySubmission, OrdinaryWorker, PersistedPayload, RawSubmission,
    SubmissionFailure, ThreadStatus,
};
use vega_store::Store;
use vega_store::thread_admission::try_acquire;

fn thread(id: &str) -> Thread {
    Thread {
        id: id.into(),
        parent_thread_id: None,
        backend: TaskBackend::Native,
        project_id: String::new(),
        title: "Conversation".into(),
        mode: ThreadMode::Ask,
        permission_mode: PermissionMode::Confirm,
        model: "model".into(),
        status: ThreadStatus::Active,
        pinned: false,
        unread: false,
        created_at: 1,
        updated_at: 1,
    }
}

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(SettingsOpen(false));
        crate::init(cx);
    });
}

fn history(id: &str, text: &str, seq: i64) -> HistoryPage {
    HistoryPage {
        subagents: Vec::new(),
        entries: vec![HistoryEntry::UserText {
            seq,
            message_id: id.into(),
            content: text.into(),
        }],
        older_cursor: None,
        newer_cursor: None,
        newest_seq: Some(seq),
    }
}

fn open(cx: &mut TestAppContext, id: &str) -> WindowHandle<ConversationStream> {
    let thread = thread(id);
    cx.open_window(size(px(1100.), px(900.)), move |_, cx| {
        ConversationStream::new(thread, cx)
    })
}

fn frame(handle: WindowHandle<ConversationStream>, cx: &mut TestAppContext) {
    cx.update_window(handle.into(), |_, window, cx| window.render_frame(cx))
        .unwrap();
}

fn press(handle: WindowHandle<ConversationStream>, key: &str, cx: &mut TestAppContext) {
    cx.update_window(handle.into(), |_, window, cx| window.press(key, cx))
        .unwrap();
}

fn begin(store: &Store, request: &ComposerSubmitted) -> (OrdinarySubmission, OrdinaryWorker) {
    OrdinarySubmission::begin(
        store.database_identity(),
        request.thread_id.clone(),
        RawSubmission::new(
            request.content.clone(),
            request.images.clone(),
            request.draft_revision,
            "model".into(),
            None,
            request.reasoning.clone().unwrap(),
        ),
        try_acquire(store.database_identity(), &request.thread_id)
            .unwrap()
            .unwrap(),
    )
    .unwrap()
}

fn receipt(submission: &OrdinarySubmission, worker: &OrdinaryWorker) -> CommittedReceipt {
    worker
        .bind_accepted_payload(submission.raw().text().to_owned())
        .unwrap();
    worker
        .bind_persisted_payload(PersistedPayload::new(
            submission.raw().text().to_owned(),
            submission.raw().images().to_vec(),
        ))
        .unwrap();
    worker.begin_commit().unwrap();
    let snapshot = submission.snapshot();
    CommittedReceipt {
        user_message_id: submission.user_message_id().into(),
        user_seq: 2,
        assistant_message_id: snapshot
            .attempts
            .last()
            .unwrap()
            .assistant_message_id
            .clone(),
        assistant_seq: 3,
        next_seq: 4,
        persisted_text: submission.raw().text().into(),
        persisted_images: submission.raw().images().to_vec(),
    }
}

fn visible_row(stream: &ConversationStream, index: usize) -> Bounds<Pixels> {
    let bounds = stream
        .list
        .bounds_for_item(index)
        .expect("measured production row");
    let clipped = bounds.intersect(&stream.list.viewport_bounds());
    assert!(bounds.size.width > px(0.) && bounds.size.height > px(0.));
    assert!(clipped.size.width > px(0.) && clipped.size.height > px(0.));
    bounds
}

fn user(stream: &ConversationStream, index: usize) -> &MessageCopy {
    match &stream.entries[index] {
        StreamEntry::User { copy, .. } => copy,
        _ => panic!("expected production user row"),
    }
}

fn image() -> ImageAttachment {
    ImageAttachment::from_bytes(
        include_bytes!("../../../../assets/logo/raster/vega-icon-f1-original.png").to_vec(),
    )
    .unwrap()
}

#[gpui_kit::test]
fn historical_positive_control_then_pending_precedes_preparation_and_repeat_submit(
    cx: &mut TestAppContext,
) {
    init(cx);
    let handle = open(cx, "a");
    let stream = handle.root(cx).unwrap();
    let captured = Rc::new(RefCell::new(Vec::<ComposerSubmitted>::new()));
    let events = captured.clone();
    cx.update(|cx| {
        cx.subscribe(&stream, move |_, request: &ComposerSubmitted, _| {
            events.borrow_mut().push(request.clone())
        })
        .detach();
    });
    handle
        .update(cx, |stream, _, cx| {
            stream.apply_history_page(
                history("historical", "formal history positive control", 1),
                cx,
            )
        })
        .unwrap();
    frame(handle, cx);
    handle
        .update(cx, |stream, window, cx| {
            assert_eq!(
                user(stream, 0).visible_text().as_deref(),
                Some("formal history positive control")
            );
            visible_row(stream, 0);
            stream
                .input
                .update(cx, |input, cx| input.set_text("first ordinary message", cx));
            stream.focus_composer(window, cx);
        })
        .unwrap();
    frame(handle, cx);
    press(handle, "cmd-enter", cx);
    assert_eq!(captured.borrow().len(), 1);
    let request = captured.borrow()[0].clone();
    let store = Store::open(":memory:").unwrap();
    let (submission, worker) = begin(&store, &request);
    handle
        .update(cx, |stream, _, cx| {
            assert!(stream.begin_ordinary_pending(submission.clone(), &request, cx))
        })
        .unwrap();
    frame(handle, cx);
    handle
        .update(cx, |stream, window, cx| {
            assert_eq!(stream.entries.len(), 2);
            assert_eq!(stream.list.item_count(), 2);
            assert_eq!(
                user(stream, 1).visible_text().as_deref(),
                Some("first ordinary message")
            );
            visible_row(stream, 1);
            assert!(stream.input.read(cx).text().is_empty());
            assert!(stream.ordinary_pending_before_run());
            stream
                .input
                .update(cx, |input, cx| input.set_text("editable next draft", cx));
            stream.focus_composer(window, cx);
        })
        .unwrap();
    frame(handle, cx);
    press(handle, "cmd-enter", cx);
    press(handle, "cmd-enter", cx);
    handle
        .update(cx, |stream, _, cx| {
            stream.project_ordinary_submission(submission.clone(), cx);
            assert!(!stream.begin_ordinary_pending(submission.clone(), &request, cx));
            assert_eq!(stream.entries.len(), 2);
            assert_eq!(stream.input.read(cx).text(), "editable next draft");
        })
        .unwrap();
    assert_eq!(captured.borrow().len(), 1);
    let facts = submission.snapshot();
    assert_eq!(facts.attempts.len(), 1);
    assert_eq!(
        facts.attempts[0].commit,
        vega_conversation::types::CommitFact::NotAttempted
    );
    assert_eq!(facts.attempts[0].workers, 1);
    assert!(facts.accepted.is_none());
    worker.complete(Err(SubmissionFailure::Cancelled)).unwrap();
}

#[gpui_kit::test]
fn repeated_receipt_preserves_copy_selection_neighbors_scroll_and_same_text_new_draft(
    cx: &mut TestAppContext,
) {
    init(cx);
    let handle = open(cx, "a");
    let store = Store::open(":memory:").unwrap();
    let request = handle
        .update(cx, |stream, _, cx| {
            stream.apply_history_page(history("historical", &"older neighbor\n".repeat(80), 1), cx);
            stream
                .input
                .update(cx, |input, cx| input.set_text("same content", cx));
            ComposerSubmitted {
                request_id: 1,
                draft_revision: stream.input.read(cx).content_revision(),
                attachment_ids: Vec::new(),
                pending_behavior: None,
                images: Vec::new(),
                thread_id: "a".into(),
                content: "same content".into(),
                skill_intent: None,
                reasoning: Ok(None),
            }
        })
        .unwrap();
    let (submission, worker) = begin(&store, &request);
    handle
        .update(cx, |stream, _, cx| {
            assert!(stream.begin_ordinary_pending(submission.clone(), &request, cx))
        })
        .unwrap();
    frame(handle, cx);
    let (key, copy, neighbor) = handle
        .update(cx, |stream, _, cx| {
            visible_row(stream, 1);
            stream
                .input
                .update(cx, |input, cx| input.set_text("different", cx));
            stream
                .input
                .update(cx, |input, cx| input.set_text("same content", cx));
            assert_ne!(
                stream.input.read(cx).content_revision(),
                request.draft_revision
            );
            let copy = user(stream, 1).clone();
            copy.set_local_selection_range(0..4);
            copy.set_selected_text(copy.selection_generation(), "same".into());
            stream.list.scroll_to(ListOffset {
                item_ix: 0,
                offset_in_item: px(0.),
            });
            (
                stream.entry_identities[1].key.clone(),
                copy,
                stream.entry_identities[0].key.clone(),
            )
        })
        .unwrap();
    frame(handle, cx);
    let anchor = handle
        .update(cx, |stream, _, _| stream.scroll_anchor_snapshot())
        .unwrap();
    assert!(!anchor.following_tail);
    let committed = receipt(&submission, &worker);
    worker.record_committed(committed.clone()).unwrap();
    worker.record_committed(committed.clone()).unwrap();
    handle
        .update(cx, |stream, _, cx| {
            assert!(!stream.begin_ordinary_pending(submission.clone(), &request, cx));
            stream.refresh_ordinary_submissions(cx);
            stream.refresh_ordinary_submissions(cx);
            stream.apply_history_page(history(submission.user_message_id(), "same content", 2), cx);
        })
        .unwrap();
    frame(handle, cx);
    handle
        .update(cx, |stream, _, cx| {
            assert_eq!(stream.entries.len(), 2);
            assert_eq!(stream.list.item_count(), 2);
            assert_eq!(stream.entry_identities[0].key, neighbor);
            assert_eq!(stream.entry_identities[1].key, key);
            assert_eq!(user(stream, 1).id, copy.id);
            assert_eq!(user(stream, 1).selection_scope(), copy.selection_scope());
            assert_eq!(user(stream, 1).local_selection_range(), Some(0..4));
            assert!(user(stream, 1).has_selected_text());
            user(stream, 1).copy_selected_text(cx);
            assert_eq!(
                cx.read_from_clipboard().unwrap().text().as_deref(),
                Some("same")
            );
            assert_eq!(stream.scroll_anchor_snapshot(), anchor);
            assert_eq!(stream.input.read(cx).text(), "same content");
            assert_eq!(stream.entry_identities[1].sequence, Some(2));
            stream.apply_event(
                ConversationEvent::MessageStarted {
                    message_id: committed.assistant_message_id.clone(),
                    seq: 3,
                },
                cx,
            );
            stream.apply_event(
                ConversationEvent::MessageStarted {
                    message_id: committed.assistant_message_id.clone(),
                    seq: 3,
                },
                cx,
            );
            assert_eq!(
                stream
                    .entries
                    .iter()
                    .filter(|entry| matches!(entry, StreamEntry::User { .. }))
                    .count(),
                2
            );
            assert_eq!(stream.entry_identities[1].key, key);
            assert_eq!(user(stream, 1).id, copy.id);
        })
        .unwrap();
    worker
        .complete(Err(SubmissionFailure::LaunchFailed))
        .unwrap();
}

#[gpui_kit::test]
fn original_images_survive_new_attachments_pure_image_and_view_rehydration(
    cx: &mut TestAppContext,
) {
    init(cx);
    let store = Store::open(":memory:").unwrap();
    let handle = open(cx, "a");
    let attachment = image();
    let (request, original_view) = handle
        .update(cx, |stream, _, cx| {
            stream.attachments.push((
                1,
                attachments::ImagePreview::new(attachment.clone(), "original.png".into(), cx),
            ));
            stream.submitted_attachments = stream.attachments.clone();
            let original_view = stream.submitted_attachments[0].1.view.clone();
            stream.attachments.push((
                2,
                attachments::ImagePreview::new(attachment.clone(), "next.png".into(), cx),
            ));
            (
                ComposerSubmitted {
                    request_id: 2,
                    draft_revision: stream.input.read(cx).content_revision(),
                    attachment_ids: vec![1],
                    pending_behavior: None,
                    images: vec![attachment.clone()],
                    thread_id: "a".into(),
                    content: String::new(),
                    skill_intent: None,
                    reasoning: Ok(None),
                },
                original_view,
            )
        })
        .unwrap();
    let (submission, worker) = begin(&store, &request);
    handle
        .update(cx, |stream, _, cx| {
            assert!(stream.begin_ordinary_pending(submission.clone(), &request, cx))
        })
        .unwrap();
    frame(handle, cx);
    handle
        .update(cx, |stream, _, _| {
            assert_eq!(stream.entries.len(), 1);
            assert_eq!(stream.attachments.len(), 1);
            assert_eq!(stream.attachments[0].0, 2);
            match &stream.entries[0] {
                StreamEntry::UserImages { images } => {
                    assert_eq!(images.len(), 1);
                    assert_eq!(images[0].image, attachment);
                    assert_eq!(images[0].view, original_view);
                }
                _ => panic!("pure image must not create empty text row"),
            }
            visible_row(stream, 0);
        })
        .unwrap();
    let b = open(cx, "b");
    b.update(cx, |stream, _, cx| {
        stream
            .input
            .update(cx, |input, cx| input.set_text("B draft", cx));
        stream.project_ordinary_submission(submission.clone(), cx);
        stream.refresh_ordinary_submissions(cx);
        assert!(stream.entries.is_empty());
        assert_eq!(stream.input.read(cx).text(), "B draft");
    })
    .unwrap();
    cx.update_window(handle.into(), |_, window, _| window.remove_window())
        .unwrap();
    let committed = receipt(&submission, &worker);
    worker.record_committed(committed).unwrap();
    let reopened = open(cx, "a");
    reopened
        .update(cx, |stream, _, cx| {
            stream.apply_history_page(
                HistoryPage {
                    subagents: Vec::new(),
                    entries: vec![HistoryEntry::UserImages {
                        seq: 2,
                        message_id: submission.user_message_id().into(),
                        images: vec![attachment.clone()],
                    }],
                    older_cursor: None,
                    newer_cursor: None,
                    newest_seq: Some(2),
                },
                cx,
            );
            for record in for_thread(store.database_identity(), "a").unwrap() {
                stream.project_ordinary_submission(record, cx);
            }
            stream.refresh_ordinary_submissions(cx);
        })
        .unwrap();
    frame(reopened, cx);
    reopened
        .update(cx, |stream, _, _| {
            assert_eq!(stream.entries.len(), 1);
            assert_eq!(stream.list.item_count(), 1);
            assert_eq!(
                stream.entry_identities[0].message_id.as_deref(),
                Some(submission.user_message_id())
            );
            visible_row(stream, 0);
        })
        .unwrap();
    b.update(cx, |stream, _, cx| {
        stream.refresh_ordinary_submissions(cx);
        assert!(stream.entries.is_empty());
        assert_eq!(stream.input.read(cx).text(), "B draft");
    })
    .unwrap();
    worker
        .complete(Err(SubmissionFailure::LaunchFailed))
        .unwrap();
}
