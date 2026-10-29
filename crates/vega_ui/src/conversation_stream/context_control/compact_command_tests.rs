use super::*;
use gpui_kit::{AppContext, TestAppContext};
use std::sync::{Arc, Mutex};
use vega_conversation::types::{PermissionMode, ThreadMode, ThreadStatus};

fn thread() -> Thread {
    Thread {
        parent_thread_id: None,
        forked_from_thread_id: None,
        forked_from_message_id: None,
        id: "synthetic-thread".into(),
        backend: TaskBackend::Native,
        project_id: String::new(),
        title: "Synthetic context lifecycle".into(),
        mode: ThreadMode::Execute,
        permission_mode: PermissionMode::ReadOnly,
        permission_revision: 0,
        draft_authority_epoch: None,
        full_access_grant: None,
        default_permission: None,
        reasoning_preferences: Some(Vec::new()),
        model: "gpt-4.1".into(),
        status: ThreadStatus::Active,
        pinned: false,
        unread: false,
        created_at: 1,
        updated_at: 1,
    }
}

fn fixture(cx: &mut TestAppContext) -> Entity<ConversationStream> {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(vega_theme::Theme::light());
        cx.new(|cx| {
            let mut stream = ConversationStream::new(thread(), cx);
            stream.context_control.compactable = true;
            stream.set_compaction_source("source".into());
            stream
        })
    })
}

#[gpui_kit::test]
fn compact_submit_emits_only_manual_request_and_restores_unedited_failure(cx: &mut TestAppContext) {
    let stream = fixture(cx);
    let requests = Arc::new(Mutex::new(Vec::<ContextCompactionRequested>::new()));
    let ordinary = Arc::new(Mutex::new(0));
    let observer = cx.new(|cx| {
        let requests = requests.clone();
        cx.subscribe(
            &stream,
            move |_: &mut (), _, request: &ContextCompactionRequested, _| {
                requests.lock().unwrap().push(request.clone())
            },
        )
        .detach();
        let ordinary = ordinary.clone();
        cx.subscribe(&stream, move |_: &mut (), _, _: &ComposerSubmitted, _| {
            *ordinary.lock().unwrap() += 1
        })
        .detach();
    });
    stream.update(cx, |stream, cx| {
        stream
            .input
            .update(cx, |input, cx| input.set_text("/compact  保留下一步  ", cx));
        stream.submit_message(cx);
        assert!(stream.input.read(cx).text().is_empty());
        stream
            .input
            .update(cx, |input, cx| input.set_text("/compact duplicate", cx));
        stream.submit_message(cx);
        assert!(stream.controller_error.is_some());
        stream.input.update(cx, |input, cx| input.set_text("", cx));
    });
    cx.run_until_parked();
    let request = requests.lock().unwrap()[0].clone();
    assert_eq!(requests.lock().unwrap().len(), 1);
    assert_eq!(*ordinary.lock().unwrap(), 0);
    assert_eq!(request.summary_requirements, "保留下一步");
    assert!(request.selection.is_none());
    assert_eq!(request.thread_id, thread().id);
    stream.update(cx, |stream, cx| {
        stream.finish_compact_command(request.request_id, true, cx);
        assert!(
            stream.input.read(cx).text().is_empty(),
            "edited empty draft must not be overwritten"
        );
        stream.context_control.compact_pending = None;
        stream
            .input
            .update(cx, |input, cx| input.set_text("/compact retry", cx));
        stream.submit_message(cx);
        let id = stream.context_control.compact_pending.unwrap();
        stream.finish_compact_command(id + 1, true, cx);
        assert!(stream.input.read(cx).text().is_empty());
        stream.finish_compact_command(id, true, cx);
        assert_eq!(stream.input.read(cx).text(), "/compact retry");
    });
    drop(observer);
}

#[gpui_kit::test]
fn compact_utf8_bounds_running_and_references_never_become_ordinary_messages(
    cx: &mut TestAppContext,
) {
    let stream = fixture(cx);
    stream.update(cx, |stream, cx| {
        assert_eq!(compact_requirements("/compact"), Some(""));
        assert_eq!(
            compact_requirements("/compact\nkeep details"),
            Some("keep details")
        );
        assert!(compact_requirements("/compactness").is_none());
        assert!(compact_requirements(" /compact").is_none());
        assert!(compact_requirements("/Compact").is_none());
        let large = format!("/compact {}", "中".repeat(2731));
        stream
            .input
            .update(cx, |input, cx| input.set_text(&large, cx));
        stream.submit_message(cx);
        assert!(stream.context_control.compact_pending.is_none());
        assert_eq!(stream.input.read(cx).text(), large);
        stream.actions.running = true;
        stream
            .input
            .update(cx, |input, cx| input.set_text("/compact", cx));
        stream.submit_message(cx);
        assert!(stream.context_control.compact_pending.is_none());
        assert!(!stream.composer_submit_pending);
        stream.actions.running = false;
        stream.permissions_revoked = true;
        stream.submit_message(cx);
        assert!(stream.context_control.compact_pending.is_none());
        stream.permissions_revoked = false;
        stream.skill_intent = Some(vega_conversation::types::SkillSelectionIntent {
            source_id: "source".into(),
            name: "skill".into(),
            content_sha256: "digest".into(),
            expected_consent_generation: 1,
        });
        stream.submit_message(cx);
        assert!(stream.context_control.compact_pending.is_none());
        stream.skill_intent = None;
        stream.input.update(cx, |input, cx| {
            input.set_text_with_references(
                "/compact @file",
                &[vega_conversation::types::FileReferenceMention {
                    range: 9..14,
                    path: "file".into(),
                }],
                cx,
            )
        });
        stream.submit_message(cx);
        assert!(stream.context_control.compact_pending.is_none());
        assert_eq!(stream.input.read(cx).reference_mentions().len(), 1);
        let exact = format!("/compact {}", "x".repeat(8192));
        stream
            .input
            .update(cx, |input, cx| input.set_text(&exact, cx));
        stream.submit_message(cx);
        let id = stream.context_control.compact_pending.unwrap();
        stream
            .input
            .update(cx, |input, cx| input.set_text("new draft", cx));
        stream.finish_compact_command(id, true, cx);
        assert_eq!(stream.input.read(cx).text(), "new draft");
        assert!(!stream.composer_submit_pending);
    });
}

#[gpui_kit::test]
fn compact_terminal_status_restores_cancelled_or_failed_but_not_successful_drafts(
    cx: &mut TestAppContext,
) {
    for status in [Status::Failed, Status::Cancelled, Status::Succeeded] {
        let stream = fixture(cx);
        stream.update(cx, |stream, cx| {
            stream
                .input
                .update(cx, |input, cx| input.set_text("/compact retain goal", cx));
            stream.submit_message(cx);
            let id = stream.context_control.compact_pending.unwrap();
            assert!(stream.apply_context_status(
                &thread().id,
                &thread().model,
                ContextCompactionStatusRecord {
                    generation: id,
                    status,
                    updated_at: 1,
                    estimated_tokens: None,
                    input_budget: None,
                    target_tokens: None,
                    source_version: None,
                    failure: None,
                    usage: vega_conversation::types::ContextCompactionUsageState::Pending
                },
                cx
            ));
            assert_eq!(
                stream.input.read(cx).text(),
                if status == Status::Succeeded {
                    ""
                } else {
                    "/compact retain goal"
                }
            );
            assert!(stream.context_control.compact_pending.is_none());
        });
    }
}
