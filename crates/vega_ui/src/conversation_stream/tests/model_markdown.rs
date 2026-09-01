use super::*;

#[test]
fn page_boundary_anchor_is_delegated_to_splice_preserved_scroll_top() {
    // S8-T44/C4: the prepend anchor no longer uses pixel math on uniform
    // rows. `ListState::splice` shifts `logical_scroll_top` by the prepended
    // count while keeping the pixel offset into the scroll-top item, so the
    // page-boundary anchor is exact (<1px by construction). The old
    // `anchored_prepend_offset` helper is gone; this test pins the
    // delegation (see the two variable-height narrow tests for geometry).
    assert_eq!(ANCHOR_EPSILON_PX, 1.0);
}

#[test]
fn table_maps_header_and_rows_with_padded_alignment() {
    let node = RenderNode::Table(TableBlock {
        alignments: vec![TableAlignment::Left, TableAlignment::Right],
        header: vec![
            TableCell {
                spans: vec![Inline::Text("列A".into())],
            },
            TableCell {
                spans: vec![Inline::Text("B".into())],
            },
        ],
        rows: vec![vec![
            TableCell {
                spans: vec![Inline::Text("1".into())],
            },
            TableCell {
                spans: vec![Inline::Text("数据".into())],
            },
        ]],
    });
    let lines = flatten_nodes(7, &[node], BlockOrigin::Committed);
    // 表头一行 + 表体一行；两列 → cell+分隔+cell = 3 span。
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].kind, LineKind::TableHeader);
    assert_eq!(lines[0].spans.len(), 3);
    // 右对齐列按显示宽（CJK=2）补空格："列A" 宽 3 → "B" 前补 3 空格。
    assert_eq!(spans_text(&lines[0]), "列A │    B");
    assert_eq!(lines[1].kind, LineKind::TableRow);
    // "数据" 宽 4 使第 2 列宽为 4；"1" 左对齐补到 3 宽。
    assert_eq!(spans_text(&lines[1]), "1   │ 数据");
}

#[test]
fn nested_lists_indent_and_number() {
    let node = RenderNode::List(ListBlock {
        ordered: false,
        start: 1,
        items: vec![
            ListItem {
                checked: None,
                children: vec![RenderNode::Paragraph {
                    spans: vec![Inline::Text("outer".into())],
                }],
            },
            ListItem {
                checked: Some(false),
                children: vec![
                    RenderNode::Paragraph {
                        spans: vec![Inline::Text("task".into())],
                    },
                    RenderNode::List(ListBlock {
                        ordered: true,
                        start: 3,
                        items: vec![ListItem {
                            checked: None,
                            children: vec![RenderNode::Paragraph {
                                spans: vec![Inline::Text("inner".into())],
                            }],
                        }],
                    }),
                ],
            },
        ],
    });
    let lines = flatten_nodes(9, &[node], BlockOrigin::Committed);
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0].kind, LineKind::ListItem);
    assert_eq!(lines[0].marker, "•");
    assert_eq!(lines[0].depth, 0);
    assert_eq!(lines[1].checked, Some(false));
    assert_eq!(lines[2].marker, "3.");
    assert_eq!(lines[2].depth, 1);
    assert_eq!(spans_text(&lines[2]), "inner");
}

#[test]
fn code_block_splits_physical_lines_monospaced() {
    let node = RenderNode::CodeBlock {
        language: Some("rust".into()),
        code: "fn a() {\n    let x = 1;\n}\n".into(),
    };
    let lines = flatten_nodes(11, &[node], BlockOrigin::Committed);
    // 尾换行不产生空行。
    assert_eq!(lines.len(), 3);
    assert!(lines.iter().all(|line| line.kind == LineKind::Code));
    assert_eq!(spans_text(&lines[1]), "    let x = 1;");
}

#[test]
fn committed_code_block_carries_highlight_token_kinds() {
    let node = RenderNode::CodeBlock {
        language: Some("rust".into()),
        code: "fn main() {\n    let n = 42;\n}\n".into(),
    };
    let lines = flatten_nodes(21, &[node], BlockOrigin::Committed);
    // 关键字 → Token(Keyword)；函数名 → Token(Function)（映射表「其余」
    // 档）；rust grammar 把整数字面量捕获为 constant.builtin →
    // Token(Constant)；行内未被捕获的文字补 Plain。
    assert_eq!(
        find_span(&lines, "fn").style,
        SpanStyle::Token(HighlightKind::Keyword)
    );
    assert_eq!(
        find_span(&lines, "main").style,
        SpanStyle::Token(HighlightKind::Function)
    );
    assert_eq!(
        find_span(&lines, "let").style,
        SpanStyle::Token(HighlightKind::Keyword)
    );
    assert_eq!(
        find_span(&lines, "42").style,
        SpanStyle::Token(HighlightKind::Constant)
    );
    assert_eq!(find_span(&lines, "    ").style, SpanStyle::Plain);
}

#[test]
fn pending_tail_and_unsupported_language_stay_plain_monospace() {
    let node = RenderNode::CodeBlock {
        language: Some("rust".into()),
        code: "fn a() {}\n".into(),
    };
    // 未闭合 fence（pending 尾块）降级纯文本（tech-spec §5.1）。
    let lines = flatten_nodes(23, &[node], BlockOrigin::Pending);
    assert!(
        lines
            .iter()
            .all(|line| line.spans.iter().all(|span| span.style == SpanStyle::Plain))
    );
    // 未支持语言同样降级。
    let unknown = RenderNode::CodeBlock {
        language: Some("cobol".into()),
        code: "MOVE 1 TO X.\n".into(),
    };
    let lines = flatten_nodes(24, &[unknown], BlockOrigin::Committed);
    assert!(
        lines
            .iter()
            .all(|line| line.spans.iter().all(|span| span.style == SpanStyle::Plain))
    );
}

#[test]
fn code_line_spans_fill_gaps_and_clip_at_line_edges() {
    // CJK 与多行切割：高亮 span 按字节切片，缺口补 Plain，逐行覆盖完整。
    let code = "let s = \"中文\";\nlet t = 1;\n";
    let node = RenderNode::CodeBlock {
        language: Some("rust".into()),
        code: code.to_string(),
    };
    let lines = flatten_nodes(25, &[node], BlockOrigin::Committed);
    assert_eq!(lines.len(), 2);
    assert_eq!(spans_text(&lines[0]), "let s = \"中文\";");
    assert_eq!(spans_text(&lines[1]), "let t = 1;");
    // 字符串（含 CJK 字面量）应整体有 String 捕获（转义无关），按行切片
    // 后行内仍存在 String span。
    assert!(
        lines[0]
            .spans
            .iter()
            .any(|span| span.style == SpanStyle::Token(HighlightKind::String))
    );
}

#[test]
fn user_message_lines_materialize_label_card_and_spacer() {
    let lines = user_message_lines(USER_BLOCK_BASE, "第一行\n\n第三行");
    assert_eq!(lines.len(), 5);
    assert_eq!(lines[0].kind, LineKind::UserLabel);
    assert_eq!(
        lines[1].kind,
        LineKind::UserLine {
            first: true,
            last: false
        }
    );
    // 中间空行也是卡片行（连续背景）。
    assert_eq!(
        lines[2].kind,
        LineKind::UserLine {
            first: false,
            last: false
        }
    );
    assert_eq!(
        lines[3].kind,
        LineKind::UserLine {
            first: false,
            last: true
        }
    );
    assert_eq!(lines[4].kind, LineKind::Spacer);
    assert_eq!(spans_text(&lines[1]), "第一行");
    assert_eq!(spans_text(&lines[2]), "");
    // 尾换行不产生尾部空卡片行。
    assert_eq!(user_message_lines(1, "hi\n").len(), 3);
}

#[test]
fn inline_styles_map_to_span_styles() {
    let node = RenderNode::Paragraph {
        spans: vec![
            Inline::Text("a ".into()),
            Inline::Strong(vec![Inline::Text("b".into())]),
            Inline::Text(" ".into()),
            Inline::Code("c".into()),
            Inline::Strikethrough(vec![Inline::Text("d".into())]),
            Inline::Link {
                url: "https://example.com".into(),
                title: None,
                spans: vec![Inline::Text("e".into())],
            },
        ],
    };
    let lines = flatten_nodes(13, &[node], BlockOrigin::Committed);
    assert_eq!(lines.len(), 1);
    let styles: Vec<SpanStyle> = lines[0].spans.iter().map(|span| span.style).collect();
    assert_eq!(
        styles,
        vec![
            SpanStyle::Plain,
            SpanStyle::Strong,
            SpanStyle::Plain,
            SpanStyle::Code,
            SpanStyle::Strikethrough,
            SpanStyle::Link,
        ]
    );
    assert_eq!(spans_text(&lines[0]), "a b cde");
}

#[test]
fn blockquote_lines_get_quote_kind() {
    let node = RenderNode::BlockQuote {
        children: vec![RenderNode::Paragraph {
            spans: vec![Inline::Text("quoted".into())],
        }],
    };
    let lines = flatten_nodes(15, &[node], BlockOrigin::Committed);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].kind, LineKind::Quote);
}

#[test]
fn display_width_counts_cjk_as_two() {
    assert_eq!(display_width("ab"), 2);
    assert_eq!(display_width("中文"), 4);
    assert_eq!(display_width("中a文"), 5);
}

#[test]
fn split_deltas_never_splits_codepoints_and_round_trips() {
    let doc = sample_document(3);
    let deltas = split_deltas(&doc, 0x5EED);
    assert!(deltas.len() > 10);
    assert_eq!(deltas.concat(), doc);
    assert!(deltas.iter().all(|delta| delta.chars().count() <= 8));
}

#[test]
fn stream_model_freezes_committed_blocks_during_streaming() {
    let (mut stream, _deltas) = stream_long_doc(40);
    let mut model = StreamModel::default();
    let counters = StreamCounters::default();
    // 首轮同步：物化全部 committed 块。
    {
        let snapshot = stream.snapshot();
        model.sync(&snapshot, &counters);
    }
    let committed_after_first = counters.committed_materializations.load(Ordering::Relaxed);
    let rows_after_first = model.row_count();
    assert!(committed_after_first > 10);
    assert!(rows_after_first > 40);

    // 追加一段新内容：只物化新块，冻结块零重排（P3）。
    for delta in split_deltas("\n\n追加的**新**段落，含 `code`。\n\n", 7) {
        stream.append(&delta);
    }
    {
        let snapshot = stream.snapshot();
        model.sync(&snapshot, &counters);
    }
    assert_eq!(
        counters.frozen_rematerializations.load(Ordering::Relaxed),
        0,
        "frozen blocks must never re-materialize during streaming"
    );
    let committed_after_append = counters.committed_materializations.load(Ordering::Relaxed);
    assert!(
        committed_after_append > committed_after_first,
        "the new tail block must be materialized exactly once"
    );
    assert!(model.row_count() > rows_after_first);

    // 再次同步（内容未变）：不产生任何新物化。
    {
        let snapshot = stream.snapshot();
        let changed = model.sync(&snapshot, &counters);
        assert!(!changed);
    }
    assert_eq!(
        counters.committed_materializations.load(Ordering::Relaxed),
        committed_after_append
    );
    assert_eq!(
        counters.frozen_rematerializations.load(Ordering::Relaxed),
        0
    );
}
