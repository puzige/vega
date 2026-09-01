//! S8-T44 headless 主干 E2E（决策 7 从简：恰一条）：
//! 10k 混合语义项构建 → 顺序与内容保真 → 贴底跟随 → 上翻 detach →
//! 前插页锚定 → 回底 resume → 冻结区 remat=0 与稳定 ID。
//! 沿用既有 harness 模式（真实 GPUI 窗口 + `StreamHarness` 根视图）。

use super::*;

const ITEM_COUNT: usize = 10_000;

fn entry_kinds_at(stream: &ConversationStream, indices: &[usize]) -> Vec<&'static str> {
    indices
        .iter()
        .filter_map(|index| {
            stream.entries.get(*index).map(|entry| match entry {
                StreamEntry::User { .. } => "user",
                StreamEntry::Assistant { .. } => "assistant",
                StreamEntry::Tool { .. } => "tool",
                StreamEntry::Artifact { .. } => "artifact",
                StreamEntry::Permission { .. } => "permission",
                StreamEntry::Plan { .. } => "plan",
                StreamEntry::Summary { .. } => "summary",
            })
        })
        .collect()
}

/// Joined text of an assistant entry's committed+pending lines.
fn assistant_line_text(entry: &StreamEntry) -> String {
    match entry {
        StreamEntry::Assistant { model, .. } => model
            .committed_lines
            .iter()
            .chain(model.pending_lines.iter())
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.text.as_str())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn user_entry_text(entry: &StreamEntry) -> String {
    match entry {
        StreamEntry::User { lines } => lines
            .iter()
            .filter(|line| matches!(line.kind, LineKind::UserLine { .. }))
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.text.as_str())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

#[gpui::test]
async fn ten_k_mixed_items_trunk_e2e(cx: &mut TestAppContext) {
    init_permission_test(cx);
    let stream = cx.new(|cx| ConversationStream::new(permission_thread(), cx));

    // ── 1) 10k 混合语义项构建（一项=一个语义 entry，一次性 splice 装载） ──
    stream.update(cx, |stream, cx| {
        let mut user_seq = 0u64;
        let entries: Vec<StreamEntry> = (0..ITEM_COUNT)
            .map(|index| mixed_entry(index, &mut user_seq, cx))
            .collect();
        let count = entries.len();
        stream.entries = entries;
        stream.list_prepend(count);
        assert_eq!(stream.list.item_count(), count);
    });
    // 生产事件链开一条 live assistant turn（mutable tail，S7 事件链不变）。
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "live".into(),
                seq: 1,
            },
            cx,
        );
    });

    let total_entries = stream.read_with(cx, |stream, _| stream.entries.len());
    assert_eq!(total_entries, ITEM_COUNT + 1, "fixture + one live turn");

    // ── 2) 打开窗口并完成首帧布局 ──
    let (harness, visual) = cx.add_window_view(|_, _| StreamHarness {
        stream: stream.clone(),
    });
    let draw = |visual: &mut gpui::VisualTestContext, harness: &Entity<StreamHarness>| {
        let element = harness.clone();
        visual.draw(
            gpui::point(px(0.), px(0.)),
            gpui::size(px(1200.), px(800.)),
            |_, _| element.into_any_element(),
        );
    };
    draw(visual, &harness);

    // ── 3) 顺序与内容保真（抽样断言；全文保留，禁截断） ──
    let kinds = stream.read_with(visual, |stream, _| {
        entry_kinds_at(stream, &[0, 9, 12, 15, 18, 20, 21, 23, 24, 25])
    });
    assert_eq!(
        kinds,
        vec![
            "assistant", // 0: markdown turn
            "assistant", // 9: code turn
            "assistant", // 12: table turn
            "assistant", // 15: wrapped-CJK turn
            "user",      // 18: user echo
            "tool",      // 20: tool card
            "user",      // 21: user echo
            "plan",      // 23: plan card
            "summary",   // 24: summary card
            "assistant", // 25: next markdown cycle
        ]
    );
    stream.read_with(visual, |stream, _| {
        let markdown = assistant_line_text(&stream.entries[0]);
        assert!(markdown.contains("与 CJK 混排"), "markdown body preserved");
        assert!(markdown.contains("任务甲 0"), "list content preserved");

        let code = assistant_line_text(&stream.entries[9]);
        assert!(code.contains("fn bench_9() -> u64 {"), "code preserved");
        assert!(code.contains("let value = 9 * 42;"), "code body preserved");

        let table = assistant_line_text(&stream.entries[12]);
        assert!(table.contains("中文数据 12"), "table cell preserved");

        let wrapped = assistant_line_text(&stream.entries[15]);
        // C4 禁截断：长 CJK 段落一字不少（整段在一个变高 item 内）。
        assert!(wrapped.contains("长段落 15："), "wrapped head preserved");
        assert!(
            wrapped.contains("段落编号 15 结束。"),
            "wrapped tail preserved — no truncation"
        );
        assert!(wrapped.len() > 120, "the full paragraph survived");

        assert_eq!(
            user_entry_text(&stream.entries[18]),
            mixed_user_echo(18),
            "user echo text preserved"
        );
        let tool = &stream.entries[20];
        if let StreamEntry::Tool { card } = tool {
            // card.read needs App; use visible text via read_with below.
            let _ = card;
        }
    });
    // Card content (needs App access).
    stream.read_with(visual, |stream, cx| {
        if let Some(StreamEntry::Tool { card }) = stream.entries.get(20) {
            assert!(
                !card.read(cx).visible_text().is_empty(),
                "tool card header/summary preserved"
            );
        }
        if let Some(StreamEntry::Plan { card }) = stream.entries.get(23) {
            assert!(
                card.read(cx).plan_id() == "test-plan-23",
                "plan card identity preserved"
            );
        }
        if let Some(StreamEntry::Summary { card }) = stream.entries.get(24) {
            assert!(
                card.read(cx).visible_text().contains("任务摘要"),
                "summary card content preserved"
            );
        }
    });

    // ── 4) 贴底跟随（P4）：初始 Tail 模式贴底，注入后仍贴底 ──
    let initial = stream.read_with(visual, |stream, _| {
        (stream.following_tail(), stream.list.logical_scroll_top())
    });
    assert!(initial.0, "a fresh stream starts pinned to the bottom");
    assert!(
        tail_item_visible(&stream, visual),
        "the last item must be on screen at the bottom"
    );

    stream.update(visual, |stream, cx| {
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "live".into(),
                delta: "流式增量 **一**。".into(),
            },
            cx,
        );
    });
    draw(visual, &harness);
    stream.update(visual, |stream, cx| {
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "live".into(),
                delta: "流式增量 **二**。".into(),
            },
            cx,
        );
    });
    draw(visual, &harness);
    let following = stream.read_with(visual, |stream, _| {
        (
            stream.following_tail(),
            stream.list.logical_scroll_top(),
            stream
                .counters
                .pending_materializations
                .load(Ordering::Relaxed),
            stream
                .counters
                .frozen_rematerializations
                .load(Ordering::Relaxed),
        )
    });
    assert!(following.0, "tail streaming keeps the anchor pinned");
    assert!(
        tail_item_visible(&stream, visual),
        "the tail item stays visible while following"
    );
    assert!(following.2 > 0, "the mutable tail re-materialized");
    assert_eq!(
        following.3, 0,
        "frozen region re-materializations must stay 0 (P3)"
    );
    // The live tail's content landed.
    stream.read_with(visual, |stream, _| {
        let last = assistant_line_text(stream.entries.last().expect("live turn"));
        assert!(last.contains("流式增量 一。"));
        assert!(last.contains("流式增量 二。"));
    });

    // ── 5) 上翻 detach：上翻后新内容不再自动跳底 ──
    stream.update(visual, |stream, _| {
        stream.list.scroll_by(px(-600.0));
    });
    draw(visual, &harness);
    let detached = stream.read_with(visual, |stream, _| {
        (stream.following_tail(), stream.list.logical_scroll_top())
    });
    assert!(!detached.0, "scrolling up detaches the tail anchor");
    let top_while_detached = detached.1;

    // Detached 期间的晚到 delta：viewport 停在原地，冻结区 remat 仍为 0。
    stream.update(visual, |stream, cx| {
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "live".into(),
                delta: "脱离期间的内容。".into(),
            },
            cx,
        );
    });
    draw(visual, &harness);
    let still_detached = stream.read_with(visual, |stream, _| {
        (
            stream.following_tail(),
            stream.list.logical_scroll_top(),
            stream
                .counters
                .frozen_rematerializations
                .load(Ordering::Relaxed),
        )
    });
    assert!(!still_detached.0, "detached state persists");
    assert_eq!(
        still_detached.1.item_ix, top_while_detached.item_ix,
        "the viewport item must not move while detached"
    );
    assert_eq!(
        still_detached.1.offset_in_item, top_while_detached.offset_in_item,
        "the viewport pixel offset must not move while detached"
    );
    assert_eq!(still_detached.2, 0, "frozen remat stays 0 while detached");

    // ── 6) 前插页锚定（S8-T45/C7）：splice 前插后视口内容原地不动 ──
    stream.update(visual, |stream, cx| {
        stream.apply_history_page(
            hydration_page(
                vec![
                    hydration_user(1, "更早的问题"),
                    hydration_assistant(2, "更早的回答"),
                ],
                Some(1),
            ),
            cx,
        );
    });
    draw(visual, &harness);
    let after_prepend = stream.read_with(visual, |stream, _| {
        (
            stream.list.logical_scroll_top(),
            hydrated_entry_kinds(stream),
        )
    });
    assert_eq!(
        after_prepend.0.item_ix,
        top_while_detached.item_ix + 2,
        "the scroll-top item index follows the prepend"
    );
    assert_eq!(
        after_prepend.0.offset_in_item, top_while_detached.offset_in_item,
        "page-boundary anchor: pixel offset preserved exactly (<1px drift)"
    );
    assert_eq!(&after_prepend.1[..2], &["user", "assistant"]);
    assert_eq!(
        after_prepend.1[2], "assistant",
        "the 10k fixture follows the prepended page"
    );

    // ── 7) 回底 resume：滚回底部后恢复跟随 ──
    stream.update(visual, |stream, _| {
        stream.list.scroll_by(px(10_000_000.0));
    });
    draw(visual, &harness);
    let resumed = stream.read_with(visual, |stream, _| {
        (
            stream.following_tail(),
            stream.list.logical_scroll_top(),
            stream
                .counters
                .frozen_rematerializations
                .load(Ordering::Relaxed),
        )
    });
    assert!(
        resumed.0,
        "returning to the bottom re-engages the tail anchor"
    );
    assert!(
        tail_item_visible(&stream, visual),
        "the tail item is visible after resume"
    );
    assert_eq!(resumed.2, 0, "frozen remat stays 0 across the full journey");

    // ── 8) 稳定 ID：滚动全程后抽样内容与首帧一致 ──
    stream.read_with(visual, |stream, _| {
        // Prepend shifted the fixture by 2; the same fixture indices now sit
        // at +2 with identical content.
        assert!(assistant_line_text(&stream.entries[2]).contains("与 CJK 混排"));
        assert!(assistant_line_text(&stream.entries[11]).contains("fn bench_9() -> u64 {"));
        assert!(assistant_line_text(&stream.entries[17]).contains("段落编号 15 结束。"));
        assert_eq!(user_entry_text(&stream.entries[20]), mixed_user_echo(18));
    });
}

/// Whether the last entry is on screen (the tail-follow visibility proof).
fn tail_item_visible(
    stream: &Entity<ConversationStream>,
    visual: &mut gpui::VisualTestContext,
) -> bool {
    stream.read_with(visual, |stream, _| {
        let count = stream.entries.len();
        let above = stream.list.item_is_above_viewport(count - 1);
        let below = stream.list.item_is_below_viewport(count - 1);
        above == Some(false) && below == Some(false)
    })
}
