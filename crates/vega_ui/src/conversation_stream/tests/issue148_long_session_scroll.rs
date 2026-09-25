use super::*;

#[gpui_kit::test]
async fn mixed_fixtures_keep_entry_counts_and_render_callbacks_bounded(cx: &mut TestAppContext) {
    init_permission_test(cx);
    let stream = cx.new(|cx| ConversationStream::new(permission_thread(), cx));
    let (harness, visual) = cx.add_window_view(|_, _| StreamHarness {
        stream: stream.clone(),
    });
    let mut callback_counts = Vec::new();
    for count in [25, 100, 1_000, 10_000] {
        stream.update(visual, |stream, cx| {
            let mut user_seq = 0u64;
            stream.entries = (0..count)
                .map(|index| mixed_entry(index, &mut user_seq, cx))
                .collect();
            stream.entry_identities.clear();
            stream.list.reset(0);
            stream.list_append(0);
            cx.notify();
        });
        let before = stream.read_with(visual, |stream, _| {
            stream.counters.row_callbacks.load(Ordering::Relaxed)
        });
        let element = harness.clone();
        visual.draw(
            gpui_kit::point(px(0.), px(0.)),
            gpui_kit::size(px(1200.), px(800.)),
            |_, _| element.into_any_element(),
        );
        let (entry_count, callbacks) = stream.read_with(visual, |stream, _| {
            (
                stream.entries.len(),
                stream.counters.row_callbacks.load(Ordering::Relaxed) - before,
            )
        });
        assert_eq!(entry_count, count);
        assert!(callbacks > 0);
        if count >= 100 {
            assert!(
                callbacks <= 128,
                "{count} entries invoked {callbacks} list callbacks"
            );
        }
        callback_counts.push(callbacks);
    }
    assert!(
        callback_counts[3] <= callback_counts[2] + 16,
        "10,000-entry fixture callback count grew with total data: {callback_counts:?}"
    );
}

#[gpui_kit::test]
async fn durable_entry_identity_survives_prepend_and_rebuild(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "issue148-identity-thread");
    let initial = hydration_page(
        vec![
            hydration_user(8, "stable question"),
            hydration_assistant(9, "stable answer"),
        ],
        Some(1),
    );
    let thread = stream.read_with(cx, |stream, _| stream.thread.clone());
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(initial.clone(), cx)
    });
    let identities = stream.read_with(cx, |stream, _| {
        (
            stream
                .entry_identity_at(0)
                .expect("user identity")
                .to_string(),
            stream
                .entry_identity_at(1)
                .expect("assistant identity")
                .to_string(),
        )
    });
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(
            hydration_page(vec![hydration_user(6, "older question")], None),
            cx,
        );
    });
    let after_prepend = stream.read_with(cx, |stream, _| {
        (
            stream
                .entry_identity_at(1)
                .expect("user identity")
                .to_string(),
            stream
                .entry_identity_at(2)
                .expect("assistant identity")
                .to_string(),
        )
    });
    assert_eq!(after_prepend, identities);

    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "live-assistant-10".into(),
                seq: 10,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "live-assistant-10".into(),
                delta: "live answer".into(),
            },
            cx,
        );
    });
    let live_identity = stream.read_with(cx, |stream, _| {
        stream
            .entry_identity_at(4)
            .expect("live assistant identity")
            .to_string()
    });
    let rebuilt = cx.new(|cx| ConversationStream::new(thread, cx));
    rebuilt.update(cx, |stream, cx| {
        stream.apply_history_page(
            hydration_page(
                vec![
                    hydration_user(6, "older question"),
                    hydration_user(8, "stable question"),
                    hydration_assistant(9, "stable answer"),
                    HistoryEntry::AssistantText {
                        seq: 10,
                        message_id: "live-assistant-10".into(),
                        content: "live answer".into(),
                        status: vega_conversation::history::AssistantStatus::Done,
                        execution_duration_ms: None,
                    },
                ],
                None,
            ),
            cx,
        );
    });
    let rebuilt_identities = rebuilt.read_with(cx, |stream, _| {
        (
            stream
                .entry_identity_at(1)
                .expect("user identity")
                .to_string(),
            stream
                .entry_identity_at(2)
                .expect("assistant identity")
                .to_string(),
            stream
                .entry_identity_at(3)
                .expect("rehydrated assistant identity")
                .to_string(),
        )
    });
    assert_eq!(rebuilt_identities.0, identities.0);
    assert_eq!(rebuilt_identities.1, identities.1);
    assert_eq!(rebuilt_identities.2, live_identity);
}

#[gpui_kit::test]
async fn scroll_anchor_snapshot_restores_message_identity_and_offset_after_rebuild(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "issue148-scroll-anchor-thread");
    let thread = stream.read_with(cx, |stream, _| stream.thread.clone());
    let entries = (1..=80)
        .map(|seq| hydration_assistant(seq, &format!("message {seq} ").repeat(24)))
        .collect::<Vec<_>>();
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(hydration_page(entries.clone(), None), cx);
        stream.list.set_follow_mode(gpui_kit::FollowMode::Normal);
        stream.list.scroll_to(gpui_kit::ListOffset {
            item_ix: 30,
            offset_in_item: px(11.),
        });
    });
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    visual.draw(
        gpui_kit::point(px(0.), px(0.)),
        gpui_kit::size(px(1200.), px(800.)),
        |_, _| stream.clone().into_any_element(),
    );
    let anchor = stream.read_with(&visual, |stream, _| stream.scroll_anchor_snapshot());
    assert_eq!(anchor.message_id.as_deref(), Some("assistant-31"));
    assert!(!anchor.following_tail);
    assert!((anchor.offset_in_item_px - 11.).abs() < 1.0);

    let rebuilt = cx.new(|cx| ConversationStream::new(thread, cx));
    rebuilt.update(cx, |stream, cx| {
        stream.apply_history_page(hydration_page(entries, None), cx);
        assert!(stream.restore_scroll_anchor(&anchor));
    });
    let restored = rebuilt.read_with(cx, |stream, _| {
        let snapshot = stream.scroll_anchor_snapshot();
        let top = stream.list.logical_scroll_top();
        (
            snapshot,
            stream.entry_identity_at(top.item_ix).map(str::to_string),
        )
    });
    assert_eq!(restored.0.identity, anchor.identity);
    assert_eq!(restored.0.message_id, anchor.message_id);
    assert_eq!(restored.1, anchor.identity);
    assert!((restored.0.offset_in_item_px - 11.).abs() < 1.0);
}

#[gpui_kit::test]
async fn prepend_and_column_remeasure_restore_stable_pixel_anchor(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue148-anchor-thread");
    stream.update(cx, |stream, cx| {
        stream.set_workspace_width(1100., cx);
        let mut entries: Vec<_> = (10..110)
            .map(|seq| hydration_assistant(seq, &format!("content {seq} ").repeat(12)))
            .collect();
        entries[35] = HistoryEntry::Tool {
            seq: 45,
            message_id: "assistant-45".into(),
            call_id: "anchor-tool".into(),
            status: ToolCallStatus::Success,
            approval: None,
            input: Some(ToolCardInputProjection::ReadOnly {
                tool: ReadOnlyToolKind::Read,
                permission_path: None,
            }),
            result: Some(ToolCardResultProjection::ReadOnly {
                status: ToolCallStatus::Success,
                output: "dynamic detail row\n".repeat(64),
                reused: false,
            }),
        };
        stream.apply_history_page(hydration_page(entries, None), cx);
    });
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    visual.simulate_resize(gpui_kit::size(px(1100.), px(800.)));
    cx.run_until_parked();
    stream.update(cx, |stream, cx| {
        stream.list.set_follow_mode(gpui_kit::FollowMode::Normal);
        stream.list.scroll_to(gpui_kit::ListOffset {
            item_ix: 35,
            offset_in_item: px(7.),
        });
        cx.notify();
    });
    visual.draw(
        gpui_kit::point(px(0.), px(0.)),
        gpui_kit::size(px(1100.), px(800.)),
        |_, _| stream.clone().into_any_element(),
    );
    let before = stream.read_with(&visual, |stream, _| {
        let top = stream.list.logical_scroll_top();
        (
            stream.entry_identity_at(top.item_ix).unwrap().to_string(),
            f32::from(top.offset_in_item),
        )
    });
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(
            hydration_page(
                (0..10)
                    .map(|seq| hydration_assistant(seq, &format!("older {seq}")))
                    .collect(),
                None,
            ),
            cx,
        );
    });
    visual.draw(
        gpui_kit::point(px(0.), px(0.)),
        gpui_kit::size(px(1100.), px(800.)),
        |_, _| stream.clone().into_any_element(),
    );
    let after_prepend = stream.read_with(&visual, |stream, _| {
        let top = stream.list.logical_scroll_top();
        (
            stream.entry_identity_at(top.item_ix).unwrap().to_string(),
            f32::from(top.offset_in_item),
        )
    });
    assert_eq!(after_prepend.0, before.0);
    assert!((after_prepend.1 - before.1).abs() < 1.0);

    stream.update(cx, |stream, _cx| {
        let top_index = stream.list.logical_scroll_top().item_ix;
        if let Some(StreamEntry::Tool { card }) = stream.entries.get(top_index) {
            let card = card.clone();
            card.update(_cx, |card, cx| card.set_expanded(true, cx));
        }
        stream.invalidate_item(Some(top_index));
    });
    visual.draw(
        gpui_kit::point(px(0.), px(0.)),
        gpui_kit::size(px(1100.), px(800.)),
        |_, _| stream.clone().into_any_element(),
    );
    let after_content_change = stream.read_with(&visual, |stream, _| {
        let top = stream.list.logical_scroll_top();
        (
            stream.entry_identity_at(top.item_ix).unwrap().to_string(),
            f32::from(top.offset_in_item),
        )
    });
    assert_eq!(after_content_change.0, before.0);
    assert!((after_content_change.1 - before.1).abs() < 1.0);

    visual.simulate_resize(gpui_kit::size(px(720.), px(800.)));
    stream.update(cx, |stream, cx| stream.set_workspace_width(720., cx));
    visual.draw(
        gpui_kit::point(px(0.), px(0.)),
        gpui_kit::size(px(720.), px(800.)),
        |_, _| stream.clone().into_any_element(),
    );
    let after_remeasure = stream.read_with(&visual, |stream, _| {
        let top = stream.list.logical_scroll_top();
        (
            stream.entry_identity_at(top.item_ix).unwrap().to_string(),
            f32::from(top.offset_in_item),
        )
    });
    assert_eq!(after_remeasure.0, before.0);
    assert!((after_remeasure.1 - before.1).abs() < 1.0);
}

#[gpui_kit::test]
async fn production_render_samples_remain_bounded(_cx: &mut TestAppContext) {
    let counters = StreamCounters::default();
    for value in 0..10_000 {
        counters.record_render(Instant::now());
        counters.record_row_callback(value);
    }
    assert_eq!(counters.render_ns.len(), STREAM_SAMPLE_CAPACITY);
    assert_eq!(counters.row_build_ns.len(), STREAM_SAMPLE_CAPACITY);
    assert_eq!(counters.row_callbacks.load(Ordering::Relaxed), 10_000);
}

#[gpui_kit::test]
async fn loaded_message_reveal_keeps_the_entry_model_and_scrolls_to_target(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "issue148-loaded-target");
    let entries = (1..=100)
        .map(|seq| hydration_assistant(seq, &format!("message {seq}")))
        .collect::<Vec<_>>();
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(hydration_page(entries, None), cx);
    });
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    visual.draw(
        gpui_kit::point(px(0.), px(0.)),
        gpui_kit::size(px(1200.), px(800.)),
        |_, _| stream.clone().into_any_element(),
    );
    stream.update(&mut visual, |stream, _| {
        stream.list.set_follow_mode(gpui_kit::FollowMode::Normal);
        stream.list.scroll_to(gpui_kit::ListOffset {
            item_ix: 0,
            offset_in_item: px(0.),
        });
    });
    visual.draw(
        gpui_kit::point(px(0.), px(0.)),
        gpui_kit::size(px(1200.), px(800.)),
        |_, _| stream.clone().into_any_element(),
    );
    let before = stream.read_with(&visual, |stream, _| {
        (
            stream.entries.len(),
            stream.entry_identities.clone(),
            stream.list.logical_scroll_top().item_ix,
        )
    });
    let found = stream.update(&mut visual, |stream, cx| {
        stream.reveal_loaded_message("assistant-75", cx)
    });
    assert!(found);
    visual.draw(
        gpui_kit::point(px(0.), px(0.)),
        gpui_kit::size(px(1200.), px(800.)),
        |_, _| stream.clone().into_any_element(),
    );
    let after = stream.read_with(&visual, |stream, _| {
        (
            stream.entries.len(),
            stream.entry_identities.clone(),
            stream.list.logical_scroll_top().item_ix,
            stream.message_location_status(),
        )
    });
    assert_eq!(after.0, before.0);
    assert_eq!(after.1, before.1);
    assert!(after.2 > before.2);
    assert_eq!(after.3, Some(MessageLocationStatus::Located));
}

#[gpui_kit::test]
async fn target_window_replacement_drops_old_cards_and_preserves_thread_state(
    cx: &mut TestAppContext,
) {
    let (_window, stream, _) = open_controller_stream(cx, "issue148-target-window");
    let thread = stream.read_with(cx, |stream, _| stream.thread.clone());
    let input = stream.read_with(cx, |stream, _| stream.input.clone());
    let meter = stream.read_with(cx, |stream, _| stream.meter_snapshot());
    let old_page = HistoryPage {
        entries: vec![
            hydration_user(1, "old user"),
            hydration_assistant(2, "old assistant"),
            HistoryEntry::Tool {
                seq: 3,
                message_id: "assistant-2".into(),
                call_id: "old-tool".into(),
                status: ToolCallStatus::Success,
                approval: None,
                input: Some(ToolCardInputProjection::ReadOnly {
                    tool: ReadOnlyToolKind::Read,
                    permission_path: None,
                }),
                result: Some(ToolCardResultProjection::ReadOnly {
                    status: ToolCallStatus::Success,
                    output: "old output".into(),
                    reused: false,
                }),
            },
            hydration_summary("assistant-2"),
            HistoryEntry::Plan {
                seq: 4,
                plan: Plan {
                    id: "old-plan".into(),
                    thread_id: thread.id.clone(),
                    content: "old plan".into(),
                    status: PlanStatus::Approved,
                    review_note: None,
                    reviewed_at: Some(1),
                },
                execution_duration_ms: None,
            },
        ],
        older_cursor: Some(1),
        newer_cursor: None,
        newest_seq: Some(4),
    };
    stream.update(cx, |stream, cx| stream.apply_history_page(old_page, cx));
    let old_cards = stream.read_with(cx, |stream, _| {
        (
            stream.tool_cards.len(),
            stream.plan_cards.len(),
            stream.summary_cards.len(),
        )
    });
    assert_eq!(old_cards, (1, 1, 1));
    let target_page = HistoryPage {
        entries: vec![
            hydration_user(900, "target question"),
            hydration_assistant(901, "target answer"),
        ],
        older_cursor: Some(800),
        newer_cursor: Some(901),
        newest_seq: Some(901),
    };
    let found = stream.update(cx, |stream, cx| {
        stream.replace_history_window(target_page, "assistant-901", cx)
    });
    assert!(found);
    let after = stream.read_with(cx, |stream, _| {
        (
            stream.entries.len(),
            stream.tool_cards.len(),
            stream.artifact_cards.len(),
            stream.plan_cards.len(),
            stream.summary_cards.len(),
            stream.thread.clone(),
            stream.input.clone(),
            stream.meter_snapshot(),
            stream.hydration_cursor(),
            stream.newer_history_cursor(),
            stream
                .entry_identities
                .iter()
                .any(|identity| identity.message_id.as_deref() == Some("assistant-901")),
            stream.message_location_status(),
        )
    });
    assert_eq!(after.0, 2);
    assert_eq!((after.1, after.2, after.3, after.4), (0, 0, 0, 0));
    assert_eq!(after.5, thread);
    assert_eq!(after.6, input);
    assert_eq!(after.7, meter);
    assert_eq!(after.8, Some(800));
    assert_eq!(after.9, Some(901));
    assert!(after.10);
    assert_eq!(after.11, Some(MessageLocationStatus::Located));
}

#[gpui_kit::test]
async fn newer_page_appends_and_preserves_the_existing_anchor(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue148-newer-page");
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(
            HistoryPage {
                entries: (1..=40)
                    .map(|seq| hydration_assistant(seq, &format!("message {seq} ").repeat(5)))
                    .collect(),
                older_cursor: Some(1),
                newer_cursor: Some(40),
                newest_seq: Some(40),
            },
            cx,
        );
        stream.list.set_follow_mode(gpui_kit::FollowMode::Normal);
        stream.list.scroll_to(gpui_kit::ListOffset {
            item_ix: 20,
            offset_in_item: px(9.),
        });
    });
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    visual.draw(
        gpui_kit::point(px(0.), px(0.)),
        gpui_kit::size(px(1200.), px(800.)),
        |_, _| stream.clone().into_any_element(),
    );
    let before = stream.read_with(&visual, |stream, _| {
        (
            stream.scroll_anchor_snapshot(),
            stream.entry_identities[..40].to_vec(),
        )
    });
    stream.update(&mut visual, |stream, cx| {
        stream.apply_newer_history_page(
            HistoryPage {
                entries: (41..=55)
                    .map(|seq| hydration_assistant(seq, &format!("message {seq} ").repeat(5)))
                    .collect(),
                older_cursor: None,
                newer_cursor: None,
                newest_seq: Some(55),
            },
            cx,
        );
    });
    visual.draw(
        gpui_kit::point(px(0.), px(0.)),
        gpui_kit::size(px(1200.), px(800.)),
        |_, _| stream.clone().into_any_element(),
    );
    let after = stream.read_with(&visual, |stream, _| {
        (
            stream.scroll_anchor_snapshot(),
            stream.entry_identities[..40].to_vec(),
            stream.entries.len(),
            stream.hydration_cursor(),
            stream.newer_history_cursor(),
        )
    });
    assert_eq!(after.0.identity, before.0.identity);
    assert_eq!(after.0.message_id, before.0.message_id);
    assert!((after.0.offset_in_item_px - before.0.offset_in_item_px).abs() < 1.0);
    assert_eq!(after.1, before.1);
    assert_eq!(after.2, 55);
    assert_eq!(after.3, Some(1));
    assert_eq!(after.4, None);
}

#[gpui_kit::test]
async fn newer_page_request_waits_until_the_list_reaches_the_bottom(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "issue148-newer-boundary");
    let requests = Arc::new(Mutex::new(Vec::<NewerHistoryPageRequested>::new()));
    let captured = requests.clone();
    cx.update(|cx| {
        cx.subscribe(&stream, move |_, event: &NewerHistoryPageRequested, _| {
            captured
                .lock()
                .expect("newer request capture")
                .push(event.clone());
        })
        .detach();
    });
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(
            HistoryPage {
                entries: (1..=40)
                    .map(|seq| hydration_assistant(seq, &format!("message {seq} ").repeat(5)))
                    .collect(),
                older_cursor: None,
                newer_cursor: Some(40),
                newest_seq: Some(40),
            },
            cx,
        );
        stream.hydration.loading = true;
    });
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    visual.draw(
        gpui_kit::point(px(0.), px(0.)),
        gpui_kit::size(px(1200.), px(800.)),
        |_, _| stream.clone().into_any_element(),
    );
    stream.update(&mut visual, |stream, cx| {
        stream.list.scroll_by(px(-600.));
        stream.hydration.loading = false;
        cx.notify();
    });
    visual.draw(
        gpui_kit::point(px(0.), px(0.)),
        gpui_kit::size(px(1200.), px(800.)),
        |_, _| stream.clone().into_any_element(),
    );
    assert!(!stream.read_with(&visual, |stream, _| stream.scroll_at_bottom()));
    assert!(requests.lock().expect("newer request capture").is_empty());

    stream.update(&mut visual, |stream, cx| {
        stream.list.scroll_by(px(10_000_000.));
        cx.notify();
    });
    visual.draw(
        gpui_kit::point(px(0.), px(0.)),
        gpui_kit::size(px(1200.), px(800.)),
        |_, _| stream.clone().into_any_element(),
    );
    assert!(stream.read_with(&visual, |stream, _| stream.scroll_at_bottom()));
    assert_eq!(
        requests.lock().expect("newer request capture").as_slice(),
        &[NewerHistoryPageRequested {
            thread_id: "issue148-newer-boundary".into(),
            after: 40,
        }]
    );
}
