use super::*;
use gpui_kit::VisualTestContext;
use vega_conversation::types::{AssistantResponseKind, AssistantResponseMetadata, PendingMessage};

const GENERATION: &str = "run-one";

fn message(id: &str, behavior: PendingMessageBehavior) -> PendingMessage {
    PendingMessage {
        id: id.into(),
        thread_id: "issue401".into(),
        generation: GENERATION.into(),
        behavior,
        content: format!("content-{id}"),
        images: Vec::new(),
    }
}

fn committed(
    stream: &Entity<ConversationStream>,
    messages: Vec<PendingMessage>,
    generation: Option<&str>,
    cx: &mut TestAppContext,
) {
    stream.update(cx, |stream, cx| {
        stream.apply_pending_queue(
            PendingQueueProjection {
                messages,
                committed_generation: generation.map(str::to_string),
                ..Default::default()
            },
            cx,
        );
    });
    cx.run_until_parked();
}

fn delivery(pending_id: &str, message_id: &str, seq: u64) -> ConversationEvent {
    ConversationEvent::PendingMessageDelivered {
        pending_id: pending_id.into(),
        message_id: message_id.into(),
        seq,
        content: format!("content-{pending_id}"),
        images: Vec::new(),
    }
}

fn user_entries(stream: &ConversationStream) -> Vec<usize> {
    stream
        .entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| matches!(entry, StreamEntry::User { .. }).then_some(index))
        .collect()
}

fn echo_state(stream: &ConversationStream) -> Vec<(String, Vec<usize>)> {
    stream
        .pending_echoes
        .iter()
        .map(|echo| (echo.pending_id.clone(), echo.indices.clone()))
        .collect()
}

#[gpui_kit::test]
async fn issue401_committed_steer_leaves_the_rail_and_echoes_into_the_stream(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "issue401");
    stream.update(cx, ConversationStream::begin_composer_run);
    let queued = message("steer", PendingMessageBehavior::Queue);
    committed(&stream, vec![queued.clone()], None, cx);
    assert_eq!(
        stream.read_with(cx, |stream, _| stream.visible_pending_messages().len()),
        1
    );
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("pending-message-rail")
            .is_some()
    );

    let mut promoted = queued.clone();
    promoted.behavior = PendingMessageBehavior::Steer;
    committed(&stream, vec![promoted], Some(GENERATION), cx);
    stream.read_with(cx, |stream, _| {
        assert_eq!(user_entries(stream), vec![0]);
        assert!(stream.visible_pending_messages().is_empty());
        assert_eq!(stream.pending_echoes.len(), 1);
        assert_eq!(stream.pending_echoes[0].pending_id, "steer");
        assert_eq!(stream.pending_echoes[0].indices, vec![0]);
        assert!(stream.entry_identities[0].message_id.is_none());
        assert_eq!(
            stream.entry_identities[0].pending_id.as_deref(),
            Some("steer")
        );
        assert!(stream.entry_identities[0].sequence.is_none());
    });
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("pending-message-rail")
            .is_none()
    );
}

#[gpui_kit::test]
async fn issue401_only_the_active_generation_is_committed(cx: &mut TestAppContext) {
    let (_, stream, _) = open_controller_stream(cx, "issue401");
    stream.update(cx, ConversationStream::begin_composer_run);
    let mut stale = message("stale", PendingMessageBehavior::Steer);
    stale.generation = "previous-run".into();
    let active = message("active", PendingMessageBehavior::Steer);
    let queued = message("queued", PendingMessageBehavior::Queue);
    committed(&stream, vec![stale, active, queued], Some(GENERATION), cx);
    stream.read_with(cx, |stream, _| {
        let visible = stream
            .visible_pending_messages()
            .iter()
            .map(|message| message.id.clone())
            .collect::<Vec<_>>();
        assert_eq!(visible, ["stale", "queued"]);
        assert_eq!(user_entries(stream), vec![0]);
        assert_eq!(stream.pending_echoes.len(), 1);
        assert_eq!(stream.pending_echoes[0].pending_id, "active");
    });
}

#[gpui_kit::test]
async fn issue401_delivery_promotes_the_echo_in_place(cx: &mut TestAppContext) {
    let (_, stream, _) = open_controller_stream(cx, "issue401");
    stream.update(cx, ConversationStream::begin_composer_run);
    committed(
        &stream,
        vec![message("steer", PendingMessageBehavior::Steer)],
        Some(GENERATION),
        cx,
    );
    stream.update(cx, |stream, cx| {
        stream.apply_event(delivery("steer", "durable-user", 37), cx)
    });
    stream.read_with(cx, |stream, _| {
        assert_eq!(user_entries(stream), vec![0]);
        assert!(stream.pending_echoes.is_empty());
        assert_eq!(
            stream.entry_identities[0].message_id.as_deref(),
            Some("durable-user")
        );
        assert_eq!(stream.entry_identities[0].pending_id, None);
        assert_eq!(stream.entry_identities[0].sequence, Some(37));
        assert_eq!(
            stream.composer_history.last().map(String::as_str),
            Some("content-steer")
        );
        assert!(stream.pending_queue.messages.is_empty());
    });
}

#[gpui_kit::test]
async fn issue401_repeated_delivery_promotes_only_once(cx: &mut TestAppContext) {
    let (_, stream, _) = open_controller_stream(cx, "issue401");
    stream.update(cx, ConversationStream::begin_composer_run);
    committed(
        &stream,
        vec![message("steer", PendingMessageBehavior::Steer)],
        Some(GENERATION),
        cx,
    );
    let event = delivery("steer", "durable-user", 37);
    stream.update(cx, |stream, cx| {
        stream.apply_event(event.clone(), cx);
        stream.apply_event(event, cx);
    });
    stream.read_with(cx, |stream, _| {
        assert_eq!(user_entries(stream), vec![0]);
        assert_eq!(stream.entries.len(), 1);
        assert_eq!(
            stream
                .composer_history
                .iter()
                .filter(|entry| entry.as_str() == "content-steer")
                .count(),
            1
        );
    });
}

#[gpui_kit::test]
async fn issue401_run_end_returns_an_unconsumed_steer_to_the_rail(cx: &mut TestAppContext) {
    let (_, stream, _) = open_controller_stream(cx, "issue401");
    stream.update(cx, ConversationStream::begin_composer_run);
    let queued = message("steer", PendingMessageBehavior::Steer);
    committed(&stream, vec![queued.clone()], Some(GENERATION), cx);
    assert_eq!(
        stream.read_with(cx, |stream, _| stream.pending_echoes.len()),
        1
    );
    committed(&stream, vec![queued], None, cx);
    stream.read_with(cx, |stream, _| {
        assert!(stream.entries.is_empty());
        assert!(stream.pending_echoes.is_empty());
        assert_eq!(stream.visible_pending_messages().len(), 1);
        assert_eq!(stream.visible_pending_messages()[0].id, "steer");
    });
}

#[gpui_kit::test]
async fn issue401_reprojection_and_reload_never_duplicate_echoes(cx: &mut TestAppContext) {
    let (_, stream, _) = open_controller_stream(cx, "issue401");
    stream.update(cx, ConversationStream::begin_composer_run);
    let queued = message("steer", PendingMessageBehavior::Steer);
    committed(&stream, vec![queued.clone()], Some(GENERATION), cx);
    committed(&stream, vec![queued.clone()], Some(GENERATION), cx);
    stream.read_with(cx, |stream, _| {
        assert_eq!(user_entries(stream), vec![0]);
        assert_eq!(stream.pending_echoes.len(), 1);
    });
    committed(&stream, Vec::new(), Some(GENERATION), cx);
    stream.read_with(cx, |stream, _| {
        assert!(stream.entries.is_empty());
        assert!(stream.pending_echoes.is_empty());
    });
    committed(&stream, vec![queued], Some(GENERATION), cx);
    stream.read_with(cx, |stream, _| {
        assert_eq!(user_entries(stream), vec![0]);
        assert_eq!(stream.pending_echoes.len(), 1);
    });
}

#[gpui_kit::test]
async fn issue401_echo_appends_after_the_live_assistant_segment(cx: &mut TestAppContext) {
    let (_, stream, _) = open_controller_stream(cx, "issue401");
    stream.update(cx, |stream, cx| {
        stream.begin_composer_run(cx);
        stream.apply_event(delivery("first", "first-user", 1), cx);
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "assistant".into(),
                seq: 2,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "assistant".into(),
                delta: "working".into(),
            },
            cx,
        );
    });
    committed(
        &stream,
        vec![message("steer", PendingMessageBehavior::Steer)],
        Some(GENERATION),
        cx,
    );
    let entries_after_echo = stream.read_with(cx, |stream, _| {
        let user = user_entries(stream);
        assert_eq!(user.len(), 2);
        let assistant = stream
            .entries
            .iter()
            .position(|entry| matches!(entry, StreamEntry::Assistant { .. }))
            .expect("assistant segment");
        assert!(
            user[1] > assistant,
            "the steer echo joins the conversation at the live bottom"
        );
        assert_eq!(
            user[1],
            stream.entries.len() - 1,
            "the steer echo is the last entry"
        );
        let active = stream
            .active_agent_message
            .as_ref()
            .expect("active assistant message");
        assert_eq!(active.1, assistant);
        stream.entries.len()
    });
    stream.update(cx, |stream, cx| {
        stream.apply_event(delivery("steer", "durable-user", 3), cx)
    });
    stream.read_with(cx, |stream, _| {
        assert_eq!(stream.entries.len(), entries_after_echo);
        let user = user_entries(stream);
        assert_eq!(user.len(), 2);
        assert_eq!(
            stream.entry_identities[user[1]].message_id.as_deref(),
            Some("durable-user")
        );
        assert_eq!(stream.entry_identities[user[1]].sequence, Some(3));
    });
}

#[gpui_kit::test]
async fn issue401_echo_with_images_promotes_both_blocks(cx: &mut TestAppContext) {
    let (_, stream, _) = open_controller_stream(cx, "issue401");
    stream.update(cx, ConversationStream::begin_composer_run);
    let mut queued = message("steer", PendingMessageBehavior::Steer);
    queued.images.push(
        ImageAttachment::from_bytes(
            include_bytes!("../../../../../assets/logo/raster/vega-icon-f1-original.png").to_vec(),
        )
        .unwrap(),
    );
    committed(&stream, vec![queued], Some(GENERATION), cx);
    stream.read_with(cx, |stream, _| {
        assert_eq!(user_entries(stream), vec![0]);
        assert_eq!(stream.pending_echoes[0].indices, vec![0, 1]);
        assert!(matches!(stream.entries[1], StreamEntry::UserImages { .. }));
    });
    stream.update(cx, |stream, cx| {
        stream.apply_event(delivery("steer", "durable-user", 5), cx)
    });
    stream.read_with(cx, |stream, _| {
        assert_eq!(stream.entries.len(), 2);
        assert_eq!(
            stream.entry_identities[0].message_id.as_deref(),
            Some("durable-user")
        );
        assert_eq!(
            stream.entry_identities[1].message_id.as_deref(),
            Some("durable-user")
        );
        assert_eq!(stream.entry_identities[0].sequence, Some(5));
        assert_eq!(stream.entry_identities[1].sequence, Some(5));
        assert_ne!(
            stream.entry_identities[0].key,
            stream.entry_identities[1].key
        );
    });
}

#[gpui_kit::test]
async fn issue401_echo_has_no_message_anchor_until_it_promotes(cx: &mut TestAppContext) {
    let (_, stream, _) = open_controller_stream(cx, "issue401");
    stream.update(cx, ConversationStream::begin_composer_run);
    committed(
        &stream,
        vec![message("steer", PendingMessageBehavior::Steer)],
        Some(GENERATION),
        cx,
    );
    stream.read_with(cx, |stream, _| {
        assert!(stream.message_anchor_projections().is_empty());
    });
    stream.update(cx, |stream, cx| {
        stream.apply_event(delivery("steer", "durable-user", 9), cx)
    });
    stream.read_with(cx, |stream, _| {
        let anchors = stream.message_anchor_projections();
        assert_eq!(anchors.len(), 1);
        assert_eq!(anchors[0].message_id, "durable-user");
        assert_eq!(anchors[0].entry_index, 0);
    });
}

#[gpui_kit::test]
async fn issue401_queue_items_keep_the_existing_rail_behaviour(cx: &mut TestAppContext) {
    let (_, stream, _) = open_controller_stream(cx, "issue401");
    stream.update(cx, ConversationStream::begin_composer_run);
    committed(
        &stream,
        vec![message("queued", PendingMessageBehavior::Queue)],
        Some(GENERATION),
        cx,
    );
    stream.read_with(cx, |stream, _| {
        assert!(stream.entries.is_empty());
        assert!(stream.pending_echoes.is_empty());
        assert_eq!(stream.visible_pending_messages().len(), 1);
    });
    assert_eq!(
        stream.read_with(cx, |stream, _| echo_state(stream)),
        Vec::new()
    );
}

#[gpui_kit::test]
async fn issue401_promotion_requires_the_active_generation(cx: &mut TestAppContext) {
    let (_, stream, _) = open_controller_stream(cx, "issue401");
    stream.update(cx, ConversationStream::begin_composer_run);
    committed(
        &stream,
        vec![message("steer", PendingMessageBehavior::Queue)],
        Some(GENERATION),
        cx,
    );
    stream.update(cx, |stream, cx| {
        stream.apply_pending_promotion("steer", "another-generation", cx)
    });
    stream.read_with(cx, |stream, _| {
        assert!(stream.entries.is_empty());
        assert_eq!(stream.visible_pending_messages().len(), 1);
    });
    stream.update(cx, |stream, cx| {
        stream.apply_pending_promotion("steer", GENERATION, cx)
    });
    stream.read_with(cx, |stream, _| {
        assert_eq!(user_entries(stream), vec![0]);
        assert_eq!(stream.pending_echoes.len(), 1);
        assert!(stream.visible_pending_messages().is_empty());
    });
}

#[gpui_kit::test]
async fn issue401_paused_steer_stays_in_the_rail_without_an_active_run(cx: &mut TestAppContext) {
    let (_, stream, _) = open_controller_stream(cx, "issue401");
    stream.update(cx, |stream, cx| {
        stream.apply_pending_queue(
            PendingQueueProjection {
                messages: vec![message("steer", PendingMessageBehavior::Steer)],
                paused: true,
                ..Default::default()
            },
            cx,
        );
    });
    stream.read_with(cx, |stream, _| {
        assert!(stream.entries.is_empty());
        assert!(stream.pending_echoes.is_empty());
        assert!(stream.pending_queue.paused);
        assert_eq!(stream.visible_pending_messages().len(), 1);
    });
}

#[gpui_kit::test]
async fn issue401_steer_segment_reuses_one_live_activity_and_terminal_owns_the_run(
    cx: &mut TestAppContext,
) {
    let (_, stream, _) = open_controller_stream(cx, "issue401-shared-activity");
    stream.update(cx, |stream, cx| {
        let started_at = Instant::now() - Duration::from_secs(15);
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "assistant-one".into(),
                seq: 2,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::RunStarted {
                message_id: "assistant-one".into(),
                started_at,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id: "assistant-one".into(),
                delta: "first thought".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "assistant-one".into(),
                delta: "first answer".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::AssistantResponseCompleted {
                message_id: "assistant-one".into(),
                response: AssistantResponseMetadata {
                    response_id: "first-commentary".into(),
                    text_start_bytes: 0,
                    text_end_bytes: 6,
                    kind: AssistantResponseKind::Commentary,
                },
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::AssistantResponseCompleted {
                message_id: "assistant-one".into(),
                response: AssistantResponseMetadata {
                    response_id: "first-final".into(),
                    text_start_bytes: 0,
                    text_end_bytes: "first answer".len(),
                    kind: AssistantResponseKind::Final,
                },
            },
            cx,
        );
        let group = stream.run_activity_groups["assistant-one"].clone();
        let initial_clock = group.read(cx).test_clock().0;
        assert_eq!(initial_clock, Some(started_at));
        stream.apply_event(
            ConversationEvent::AssistantMessageContinues {
                message_id: "assistant-one".into(),
                next_message_id: "assistant-two".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: "assistant-one".into(),
                stop_reason: vega_conversation::types::ConversationStopReason::End,
                execution_duration_ms: None,
            },
            cx,
        );
        assert_eq!(group.read(cx).status(), RunActivityStatus::Running);
        assert_eq!(
            (0..stream.entries.len())
                .filter(|index| stream.process_entry_hidden(*index, cx))
                .count(),
            0
        );
        stream.apply_event(delivery("steer", "steer-user", 3), cx);
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "assistant-two".into(),
                seq: 4,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::RunStarted {
                message_id: "assistant-two".into(),
                started_at: Instant::now(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ThinkingDelta {
                message_id: "assistant-two".into(),
                delta: "continued thought".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::ToolCallProposed {
                call: bash_call("steer-tool", "pwd"),
            },
            cx,
        );
        let group_children = group.read(cx).children();
        assert!(
            group_children
                .iter()
                .any(|child| matches!(child, RunActivityChild::Thinking(_)))
        );
        assert!(
            group_children
                .iter()
                .any(|child| matches!(child, RunActivityChild::Tool(_)))
        );
        assert_eq!(stream.run_activity_groups["assistant-two"], group);
        assert_eq!(stream.run_activity_groups["assistant-one"], group);
        assert!(stream.run_responses.contains_key("assistant-one"));
        assert!(!stream.run_responses.contains_key("assistant-two"));
        // Both bodies stay visible until the run's own final collapses them.
        assert_eq!(
            (0..stream.entries.len())
                .filter(|index| stream.process_entry_hidden(*index, cx))
                .count(),
            0
        );
        assert_eq!(group.read(cx).test_clock().0, initial_clock);
        assert_eq!(group.read(cx).status(), RunActivityStatus::Running);
        assert_eq!(
            stream
                .entries
                .iter()
                .filter(|entry| matches!(entry, StreamEntry::RunActivity { .. }))
                .count(),
            1
        );
        assert_eq!(
            stream
                .entries
                .iter()
                .zip(&stream.entry_identities)
                .filter(|(entry, identity)| {
                    identity.message_id.as_deref() == Some("assistant-one")
                        && matches!(entry, StreamEntry::Assistant { .. })
                })
                .count(),
            1
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "assistant-two".into(),
                delta: "second answer".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::AssistantResponseCompleted {
                message_id: "assistant-two".into(),
                response: AssistantResponseMetadata {
                    response_id: "second-final".into(),
                    text_start_bytes: 0,
                    text_end_bytes: "second answer".len(),
                    kind: AssistantResponseKind::Final,
                },
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: "assistant-two".into(),
                stop_reason: vega_conversation::types::ConversationStopReason::End,
                execution_duration_ms: Some(15_000),
            },
            cx,
        );
        assert!(stream.run_responses.contains_key("assistant-one"));
        assert!(stream.run_responses.contains_key("assistant-two"));
        assert_eq!(
            group.read(cx).test_projection(),
            (RunActivityStatus::Completed, Some(15_000), false, true)
        );
        stream.apply_event(
            ConversationEvent::Error {
                message_id: Some("assistant-one".into()),
                error: Arc::new(vega_runtime::VegaError::Provider {
                    status: Some(503),
                    message: "stale segment error".into(),
                    retryable: false,
                }),
                execution_duration_ms: Some(30_000),
            },
            cx,
        );
        assert_eq!(
            group.read(cx).test_projection(),
            (RunActivityStatus::Completed, Some(15_000), false, true)
        );
        let texts = stream
            .entries
            .iter()
            .zip(&stream.entry_identities)
            .filter_map(|(entry, identity)| {
                if !matches!(entry, StreamEntry::Assistant { .. }) {
                    return None;
                }
                let message_id = identity.message_id.as_deref()?;
                let text = match entry {
                    StreamEntry::Assistant { copy, .. } => copy.original_text(),
                    _ => unreachable!(),
                };
                Some((message_id.to_owned(), text))
            })
            .collect::<Vec<_>>();
        assert_eq!(
            texts,
            [
                ("assistant-one".into(), "first answer".into()),
                ("assistant-two".into(), "second answer".into()),
            ]
        );
    });
}

#[gpui_kit::test]
async fn issue401_shared_activity_uses_only_the_outer_failure_or_interruption(
    cx: &mut TestAppContext,
) {
    let (_, stream, _) = open_controller_stream(cx, "issue401-shared-terminals");
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "failed-one".into(),
                seq: 1,
            },
            cx,
        );
        let failed_group = stream.run_activity_groups["failed-one"].clone();
        stream.apply_event(
            ConversationEvent::AssistantMessageContinues {
                message_id: "failed-one".into(),
                next_message_id: "failed-two".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: "failed-one".into(),
                stop_reason: vega_conversation::types::ConversationStopReason::End,
                execution_duration_ms: None,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "failed-two".into(),
                seq: 3,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::Error {
                message_id: Some("failed-two".into()),
                error: Arc::new(vega_runtime::VegaError::Provider {
                    status: Some(503),
                    message: "provider unavailable".into(),
                    retryable: false,
                }),
                execution_duration_ms: Some(22_000),
            },
            cx,
        );
        assert_eq!(stream.run_activity_groups["failed-one"], failed_group);
        assert_eq!(stream.run_activity_groups["failed-two"], failed_group);
        assert_eq!(
            failed_group.read(cx).test_projection(),
            (RunActivityStatus::Failed, Some(22_000), false, true)
        );
        let failures = stream.run_notice_projection(cx);
        assert_eq!(failures.len(), 1);
        let (failure_index, notice) = failures.iter().next().expect("run failure notice");
        assert_eq!(notice.message_id, "failed-two");
        assert_eq!(
            stream.entry_identities[*failure_index]
                .message_id
                .as_deref(),
            Some("failed-two")
        );

        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "stopped-one".into(),
                seq: 4,
            },
            cx,
        );
        let interrupted_group = stream.run_activity_groups["stopped-one"].clone();
        stream.apply_event(
            ConversationEvent::AssistantMessageContinues {
                message_id: "stopped-one".into(),
                next_message_id: "stopped-two".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: "stopped-one".into(),
                stop_reason: vega_conversation::types::ConversationStopReason::End,
                execution_duration_ms: None,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "stopped-two".into(),
                seq: 6,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::Interrupted {
                message_id: "stopped-two".into(),
                execution_duration_ms: Some(31_000),
            },
            cx,
        );
        assert_eq!(stream.run_activity_groups["stopped-one"], interrupted_group);
        assert_eq!(stream.run_activity_groups["stopped-two"], interrupted_group);
        assert_eq!(
            interrupted_group.read(cx).test_projection(),
            (RunActivityStatus::Interrupted, Some(31_000), false, true)
        );
    });
}

#[gpui_kit::test]
async fn issue401_superseded_segments_never_end_the_shared_activity(cx: &mut TestAppContext) {
    let (_, stream, _) = open_controller_stream(cx, "issue401-superseded-terminals");
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "segment-one".into(),
                seq: 1,
            },
            cx,
        );
        let group = stream.run_activity_groups["segment-one"].clone();
        stream.apply_event(
            ConversationEvent::AssistantMessageContinues {
                message_id: "segment-one".into(),
                next_message_id: "segment-two".into(),
            },
            cx,
        );
        // Segment boundary: body only, the run keeps running.
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: "segment-one".into(),
                stop_reason: vega_conversation::types::ConversationStopReason::End,
                execution_duration_ms: None,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "segment-two".into(),
                seq: 3,
            },
            cx,
        );
        // A late interrupt or failure naming the superseded segment is a
        // stale segment event, not the run's terminal.
        stream.apply_event(
            ConversationEvent::Interrupted {
                message_id: "segment-one".into(),
                execution_duration_ms: Some(5_000),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::Error {
                message_id: Some("segment-one".into()),
                error: Arc::new(vega_runtime::VegaError::Provider {
                    status: Some(503),
                    message: "stale segment failure".into(),
                    retryable: false,
                }),
                execution_duration_ms: Some(6_000),
            },
            cx,
        );
        assert_eq!(
            group.read(cx).test_projection(),
            (RunActivityStatus::Running, None, false, false)
        );
        assert!(stream.run_notice_projection(cx).is_empty());
        // The run's own terminal still lands exactly once.
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: "segment-two".into(),
                stop_reason: vega_conversation::types::ConversationStopReason::End,
                execution_duration_ms: Some(9_000),
            },
            cx,
        );
        assert_eq!(
            group.read(cx).test_projection(),
            (RunActivityStatus::Completed, Some(9_000), false, true)
        );
        // A duplicate terminal never re-completes or rewrites the duration.
        stream.apply_event(
            ConversationEvent::Interrupted {
                message_id: "segment-two".into(),
                execution_duration_ms: Some(12_000),
            },
            cx,
        );
        assert_eq!(
            group.read(cx).test_projection(),
            (RunActivityStatus::Completed, Some(9_000), false, true)
        );
    });
}

#[gpui_kit::test]
async fn issue401_reopened_steer_pages_share_one_activity_header(cx: &mut TestAppContext) {
    let (_, stream, _) = open_controller_stream(cx, "issue401-reopened-activity");
    let status = vega_conversation::history::AssistantStatus::Done;
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(
            HistoryPage {
                entries: vec![HistoryEntry::AssistantText {
                    seq: 4,
                    message_id: "assistant-two".into(),
                    content: "second answer".into(),
                    status,
                    execution_duration_ms: Some(15_000),
                }],
                assistant_run_activities: vec![vega_conversation::history::AssistantRunActivity {
                    message_id: "assistant-two".into(),
                    run_id: "assistant-one".into(),
                    terminal_message_id: "assistant-two".into(),
                    terminal_status: Some(status),
                    execution_duration_ms: Some(15_000),
                    member_count: 2,
                }],
                older_cursor: Some(2),
                newer_cursor: None,
                newest_seq: Some(4),
            },
            cx,
        );
        stream.apply_history_page(
            HistoryPage {
                entries: vec![HistoryEntry::AssistantText {
                    seq: 2,
                    message_id: "assistant-one".into(),
                    content: "first answer".into(),
                    status,
                    execution_duration_ms: None,
                }],
                assistant_run_activities: vec![vega_conversation::history::AssistantRunActivity {
                    message_id: "assistant-one".into(),
                    run_id: "assistant-one".into(),
                    terminal_message_id: "assistant-two".into(),
                    terminal_status: Some(status),
                    execution_duration_ms: Some(15_000),
                    member_count: 2,
                }],
                older_cursor: None,
                newer_cursor: Some(2),
                newest_seq: Some(2),
            },
            cx,
        );
        assert_eq!(
            stream.run_activity_groups["assistant-one"],
            stream.run_activity_groups["assistant-two"]
        );
        assert_eq!(
            stream
                .entries
                .iter()
                .filter(|entry| matches!(entry, StreamEntry::RunActivity { .. }))
                .count(),
            1
        );
        assert_eq!(
            stream
                .entries
                .iter()
                .filter(|entry| matches!(entry, StreamEntry::Assistant { .. }))
                .count(),
            2
        );
        let header_index = stream
            .entries
            .iter()
            .position(|entry| matches!(entry, StreamEntry::RunActivity { .. }))
            .expect("run activity header");
        let first_assistant_index = stream
            .entries
            .iter()
            .zip(&stream.entry_identities)
            .position(|(entry, identity)| {
                identity.message_id.as_deref() == Some("assistant-one")
                    && matches!(entry, StreamEntry::Assistant { .. })
            })
            .expect("first assistant segment");
        assert!(header_index < first_assistant_index);
        assert_eq!(
            stream.run_activity_groups["assistant-one"]
                .read(cx)
                .test_projection(),
            (RunActivityStatus::Completed, Some(15_000), false, true)
        );
    });
}
