use super::*;

#[gpui::test]
async fn hydrated_page_fills_durable_entries_in_sequence_position(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "hydration-thread");
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(
            hydration_page(
                vec![
                    hydration_user(1, "第一问 · CJK"),
                    hydration_assistant(2, "第一答 **markdown**"),
                    hydration_user(3, "第二问"),
                    hydration_assistant(4, "第二答"),
                ],
                None,
            ),
            cx,
        );
    });
    let (kinds, exhausted, rows) = stream.read_with(cx, |stream, cx| {
        let rows: usize = stream.entries.iter().map(|e| e.row_count(cx)).sum();
        (
            hydrated_entry_kinds(stream),
            stream.hydration.older_cursor,
            rows,
        )
    });
    assert_eq!(
        kinds,
        vec!["user", "assistant", "user", "assistant"],
        "durable rows hydrate in seq position"
    );
    assert_eq!(exhausted, None, "older_cursor None marks thread head");
    // Assistant turns materialize at apply time, so the page-boundary
    // anchor sees the real prepended height (not a pre-sync zero).
    let assistant_rows: usize = stream.read_with(cx, |stream, _| {
        stream
            .entries
            .iter()
            .filter_map(|entry| match entry {
                StreamEntry::Assistant { model, .. } => Some(model.row_count()),
                _ => None,
            })
            .sum()
    });
    assert!(assistant_rows > 0, "hydrated turns materialize eagerly");
    let user_rows: usize = rows - assistant_rows;
    assert_eq!(
        user_rows, 6,
        "each user echo is label + 1 card line + spacer (CJK included)"
    );
}

#[gpui::test]
async fn scroll_up_page_prepends_and_keeps_streaming_turn_on_target(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "hydration-prepend");
    // A live agent turn is streaming when the user scrolls up.
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "live-turn".into(),
                seq: 1,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "live-turn".into(),
                delta: "直播中".into(),
            },
            cx,
        );
    });
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(
            hydration_page(
                vec![
                    hydration_user(1, "更早的历史"),
                    hydration_assistant(2, "更早的回答"),
                ],
                Some(1),
            ),
            cx,
        );
        // The live turn keeps receiving deltas after the prepend.
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "live-turn".into(),
                delta: "继续".into(),
            },
            cx,
        );
    });
    let (kinds, live_text, cursor) = stream.read_with(cx, |stream, cx| {
        let kinds = hydrated_entry_kinds(stream);
        let live_text = stream
            .entries
            .last()
            .map(|entry| match entry {
                StreamEntry::Assistant { model, .. } => model
                    .rows_in(0..model.row_count(), &vega_theme::theme(cx).colors)
                    .len(),
                _ => 0,
            })
            .unwrap_or(0);
        (kinds, live_text, stream.hydration.older_cursor)
    });
    assert_eq!(
        kinds,
        vec!["user", "assistant", "assistant"],
        "the page prepends above the live turn"
    );
    assert!(live_text > 0, "the live turn still materializes rows");
    assert_eq!(cursor, Some(1));
}

#[gpui::test]
async fn hydrated_durable_cards_reconcile_first_wins(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "hydration-dedup");
    let page_entries = vec![
        hydration_user(1, "问题"),
        hydration_assistant(2, "回答"),
        hydration_summary("assistant-2"),
    ];
    stream.update(cx, |stream, cx| {
        stream.apply_history_page(hydration_page(page_entries.clone(), None), cx);
        // A repeated page (stale re-delivery) must not duplicate cards.
        stream.apply_history_page(hydration_page(page_entries, None), cx);
    });
    let (kinds, summaries) = stream.read_with(cx, |stream, _| {
        (hydrated_entry_kinds(stream), stream.summary_cards.len())
    });
    // Production never re-delivers a page to the same stream (single
    // in-flight request + route fence); the registry dedup matters for
    // typed cards that can also arrive through the live path
    // (`apply_task_summary` after the page carried the same reference).
    // The repeated page prepends its rows; its already-registered summary
    // card is skipped and stays at its original sequence position.
    assert_eq!(
        kinds,
        vec!["user", "assistant", "user", "assistant", "summary"],
    );
    assert_eq!(summaries, 1, "the summary card stays first-wins unique");
}

#[gpui::test]
async fn foreign_thread_summary_is_dropped_and_failure_pauses(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "hydration-fence");
    // Failure: the in-flight slot releases, auto-retry pauses.
    stream.update(cx, |stream, cx| {
        stream.apply_history_load_failed(cx);
    });
    let (request_at_top, cursor) = stream.read_with(cx, |stream, _| {
        (
            stream.history_page_request(true),
            stream.hydration.older_cursor,
        )
    });
    assert_eq!(request_at_top, None, "a failed load pauses auto-retry");
    assert_eq!(cursor, None);
}
