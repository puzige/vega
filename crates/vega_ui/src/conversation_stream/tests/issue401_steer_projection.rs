use super::*;
use gpui_kit::VisualTestContext;
use vega_conversation::types::PendingMessage;

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
    stream.read_with(cx, |stream, _| {
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
        let entries_after_echo = stream.entries.len();
        let active = stream
            .active_agent_message
            .as_ref()
            .expect("active assistant message");
        assert_eq!(active.1, assistant);
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
