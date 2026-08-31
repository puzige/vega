#[allow(unused_imports)]
use super::*;

#[test]
fn commit_provider_policy_disables_retries() {
    assert_eq!(commit_retry_policy().max_retries, 0);
}

#[test]
fn bounded_agent_channel_preserves_burst_order_and_terminal() {
    let (sender, receiver) = mpsc::sync_channel(AGENT_EVENT_CAPACITY);
    let producer = std::thread::spawn(move || {
        for index in 0..(AGENT_EVENT_CAPACITY + AGENT_EVENT_BATCH + 17) {
            sender
                .send(AgentUpdate::Event(
                    vega_conversation::types::ConversationEvent::TextDelta {
                        message_id: "message".into(),
                        delta: index.to_string(),
                    },
                ))
                .expect("bounded event send");
        }
        sender
            .send(AgentUpdate::Finished(true))
            .expect("terminal send");
    });
    let mut seen = Vec::new();
    let finished = loop {
        let batch = drain_agent_updates(&receiver);
        assert!(batch.events.len() <= AGENT_EVENT_BATCH);
        for event in batch.events {
            if let vega_conversation::types::ConversationEvent::TextDelta { delta, .. } = event {
                seen.push(delta.parse::<usize>().expect("ordered index"));
            }
        }
        if let Some(finished) = batch.finished {
            break finished;
        }
        std::thread::yield_now();
    };
    producer.join().expect("bounded producer");
    assert!(finished);
    assert_eq!(seen, (0..seen.len()).collect::<Vec<_>>());
    assert_eq!(seen.len(), AGENT_EVENT_CAPACITY + AGENT_EVENT_BATCH + 17);
    assert!(AGENT_EVENT_POLL < Duration::from_millis(16));
}

#[test]
fn same_batch_applies_events_before_terminal() {
    let (sender, receiver) = mpsc::sync_channel(4);
    sender
        .send(AgentUpdate::Event(
            vega_conversation::types::ConversationEvent::MessageStarted {
                message_id: "durable".into(),
                seq: 2,
            },
        ))
        .expect("event send");
    sender
        .send(AgentUpdate::Finished(false))
        .expect("terminal send");
    let batch = drain_agent_updates(&receiver);
    assert_eq!(batch.events.len(), 1);
    assert_eq!(batch.finished, Some(false));
    assert!(matches!(
        &batch.events[0],
        vega_conversation::types::ConversationEvent::MessageStarted { message_id, .. }
            if message_id == "durable"
    ));
}

#[test]
fn finished_refresh_routes_only_to_matching_current_thread_cache() {
    assert!(current_cache_matches(Some("a"), Some("a"), "a"));
    assert!(
        !current_cache_matches(Some("b"), Some("a"), "a"),
        "A→B must not overwrite B's OpenedThread"
    );
    assert!(
        !current_cache_matches(Some("a"), Some("b"), "a"),
        "a stale cache cannot receive A's authoritative refresh"
    );
    assert!(
        current_cache_matches(Some("a"), Some("a"), "a"),
        "A→B→A must refresh the rebuilt A entity"
    );
}

#[gpui::test]
async fn cancellation_keeps_active_until_durable_handshake_finishes(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        cx.set_global(Theme::light());
        cx.set_global(SettingsOpen(false));
        vega_ui::init(cx);
    });
    let (store, thread_id) = pending_plan();
    let thread =
        vega_conversation::threads::open_thread(&store, &thread_id).expect("thread projection");
    let stream = cx.new(|cx| ConversationStream::new(thread, cx));
    let mut controller = AppAgentController::default();
    let (generation, cancel) = controller.begin(
        thread_id.clone(),
        stream.clone(),
        Some("draft".into()),
        None,
    );
    controller.request_active_cancel();
    assert!(cancel.is_cancelled());
    assert!(controller.active.is_some());
    assert_eq!(
        controller.accept_durable_start(generation + 1, &thread_id, &stream),
        None
    );
    assert_eq!(
        controller.accept_durable_start(generation, &thread_id, &stream),
        Some("draft".into())
    );
    assert_eq!(
        controller.accept_durable_start(generation, &thread_id, &stream),
        None
    );
    assert!(
        controller
            .finish(generation + 1, &thread_id, &stream)
            .is_none()
    );
    assert!(controller.active.is_some());
    let finished = controller
        .finish(generation, &thread_id, &stream)
        .expect("exact terminal owns active run");
    assert!(finished.pending_user_content.is_none());
    assert!(controller.active.is_none());

    let (next_generation, next_cancel) = controller.begin(
        thread_id.clone(),
        stream.clone(),
        Some("second".into()),
        None,
    );
    assert_eq!(
        controller.accept_durable_start(next_generation, &thread_id, &stream),
        Some("second".into())
    );
    controller.request_active_cancel();
    assert!(next_cancel.is_cancelled());
    assert!(
        controller
            .finish(next_generation, &thread_id, &stream)
            .is_some()
    );

    let (prestart_generation, prestart_cancel) = controller.begin(
        thread_id.clone(),
        stream.clone(),
        Some("retryable".into()),
        None,
    );
    controller.request_active_cancel();
    assert!(prestart_cancel.is_cancelled());
    let prestart = controller
        .finish(prestart_generation, &thread_id, &stream)
        .expect("cancelled pre-start worker still reaches terminal");
    assert_eq!(prestart.pending_user_content, Some("retryable".into()));

    let (approved_generation, _) = controller.begin(
        thread_id.clone(),
        stream.clone(),
        None,
        Some("approved-instruction".into()),
    );
    let approved = controller
        .finish(approved_generation, &thread_id, &stream)
        .expect("approved pre-start failure reaches terminal");
    assert_eq!(
        approved.pending_approved_instruction.as_deref(),
        Some("approved-instruction")
    );
}

#[gpui::test]
async fn stop_resume_fences_drop_every_late_callback_per_c5_fence_class(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(|cx| {
        cx.set_global(Theme::light());
        cx.set_global(SettingsOpen(false));
        vega_ui::init(cx);
    });
    let (store, thread_id) = pending_plan();
    let thread =
        vega_conversation::threads::open_thread(&store, &thread_id).expect("thread projection");
    let other_thread = vega_conversation::threads::create_thread(
        &store,
        &thread.project_id,
        "mock",
        PermissionMode::Confirm.as_str(),
    )
    .expect("second thread for the run fence");
    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let other_stream = cx.new(|cx| ConversationStream::new(other_thread.clone(), cx));
    let mut controller = AppAgentController::default();
    let (generation, cancel) = controller.begin(
        thread_id.clone(),
        stream.clone(),
        Some("draft".into()),
        None,
    );

    // Stop is visible first-wins: the token is cancelled, the run stays
    // owned until the durable handshake, and every fenced lookup fails.
    controller.request_active_cancel();
    assert!(cancel.is_cancelled());

    // 1) generation fence: a stale/foreign generation is refused.
    assert_eq!(
        controller.accept_durable_start(generation + 7, &thread_id, &stream),
        None
    );
    controller.observe_terminal_message(
        generation + 7,
        &thread_id,
        &stream,
        &ConversationEvent::Interrupted {
            message_id: "late-message".into(),
        },
    );
    assert!(
        controller
            .active
            .as_ref()
            .expect("stale observe must not consume the run")
            .terminal_message_id
            .is_none(),
        "stale-generation terminal observation is dropped"
    );
    assert!(
        controller
            .finish(generation + 7, &thread_id, &stream)
            .is_none()
    );

    // 2) run fence: same generation, wrong thread id is refused.
    assert_eq!(
        controller.accept_durable_start(generation, &other_thread.id, &stream),
        None
    );
    assert!(
        controller
            .finish(generation, &other_thread.id, &stream)
            .is_none()
    );

    // 3) route fence: same generation+thread, wrong stream is refused.
    assert_eq!(
        controller.accept_durable_start(generation, &thread_id, &other_stream),
        None
    );
    assert!(
        controller
            .finish(generation, &thread_id, &other_stream)
            .is_none()
    );

    // The exact run consumes the durable start exactly once and owns the
    // terminal observation.
    assert_eq!(
        controller.accept_durable_start(generation, &thread_id, &stream),
        Some("draft".into())
    );
    assert_eq!(
        controller.accept_durable_start(generation, &thread_id, &stream),
        None
    );
    controller.observe_terminal_message(
        generation,
        &thread_id,
        &stream,
        &ConversationEvent::Interrupted {
            message_id: "terminal-message".into(),
        },
    );
    assert_eq!(
        controller
            .active
            .as_ref()
            .expect("owned run")
            .terminal_message_id
            .as_deref(),
        Some("terminal-message")
    );

    // 4) terminal fence: after the exact finish, every late callback for
    // the finished run is refused (no double consume, no late start).
    let finished = controller
        .finish(generation, &thread_id, &stream)
        .expect("exact terminal owns the run");
    assert!(finished.pending_user_content.is_none());
    assert!(controller.active.is_none());
    assert_eq!(
        controller.accept_durable_start(generation, &thread_id, &stream),
        None
    );
    assert!(controller.finish(generation, &thread_id, &stream).is_none());

    // Resume: a new generation with a fresh token; the previous run's
    // cancelled token stays cancelled and cannot bleed into the new run.
    let (next_generation, next_cancel) = controller.begin(
        thread_id.clone(),
        stream.clone(),
        Some("resumed".into()),
        None,
    );
    assert_ne!(next_generation, generation);
    assert!(cancel.is_cancelled());
    assert!(!next_cancel.is_cancelled());
    assert_eq!(
        controller.accept_durable_start(next_generation, &thread_id, &stream),
        Some("resumed".into())
    );

    // Window/route cache fence: a stale cache cannot receive another
    // thread's authoritative refresh (A→B switch invalidates A).
    assert!(!current_cache_matches(
        Some(&other_thread.id),
        Some(&thread_id),
        &thread_id
    ));
    assert!(current_cache_matches(
        Some(&thread_id),
        Some(&thread_id),
        &thread_id
    ));
}
