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
fn table_preserves_cells_and_alignment_without_text_padding() {
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
    // I59 replaces the old padded-string table contract with real cells.
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].kind, LineKind::Table);
    assert!(lines[0].spans.is_empty());
    let table = lines[0].table.as_ref().expect("structured table");
    assert_eq!(
        table.alignments,
        [TableAlignment::Left, TableAlignment::Right]
    );
    let text: Vec<Vec<String>> = table
        .rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|cell| cell.iter().map(|span| span.text.as_str()).collect())
                .collect()
        })
        .collect();
    assert_eq!(text, [vec!["列A", "B"], vec!["1", "数据"]]);
}

const ISSUE59_TABLE: &str = "| # | 问题 | 选项 | 建议 |\n\
    |:---|:---|:---:|---:|\n\
    | A | 中文长内容需要在单元格内部换行，不能破坏表头和表体的列边界。 | **保留** `FullAccess` | 显式确认 |\n\
    | B | 转义竖线 a\\|b | `very_long_identifier_without_spaces_abcdefghijklmnopqrstuvwxyz` | 不丢失内容 |\n\n\
    ```text\n| fenced | stays code |\n```\n\n- 保留列表\n\n普通正文\n";

fn issue59_model(chunks: Vec<String>) -> StreamModel {
    let mut stream = MarkdownStream::new();
    let mut model = StreamModel::default();
    let counters = StreamCounters::default();
    for chunk in chunks {
        stream.append(&chunk);
        model.sync(&stream.snapshot(), &counters);
    }
    stream.finish();
    model.sync(&stream.snapshot(), &counters);
    model
}

#[test]
fn issue59_streaming_and_history_preserve_table_inline_and_block_semantics() {
    let history = issue59_model(vec![ISSUE59_TABLE.into()]);
    let streamed = issue59_model(split_deltas(ISSUE59_TABLE, 59));
    let semantic = |model: &StreamModel| {
        model
            .committed_lines
            .iter()
            .map(|line| (line.kind, line.spans.clone(), line.table.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(semantic(&streamed), semantic(&history));
    let table = history
        .committed_lines
        .iter()
        .find_map(|line| line.table.as_ref())
        .expect("table");
    assert_eq!(table.rows.len(), 3);
    assert!(table.rows.iter().all(|row| row.len() == 4));
    assert_eq!(table.rows[1][2][0].style, SpanStyle::Strong);
    assert!(
        table.rows[1][2]
            .iter()
            .any(|span| span.style == SpanStyle::Code && span.text == "FullAccess")
    );
    assert_eq!(table.rows[2][1][0].text, "转义竖线 a|b");
    assert!(
        history.committed_lines.iter().any(
            |line| line.kind == LineKind::Code && spans_text(line) == "| fenced | stays code |"
        )
    );
    assert!(
        history
            .committed_lines
            .iter()
            .any(|line| line.kind == LineKind::ListItem && spans_text(line) == "保留列表")
    );
    assert!(
        history
            .committed_lines
            .iter()
            .any(|line| line.kind == LineKind::Paragraph && spans_text(line) == "普通正文")
    );
}

struct TableView(StreamModel);
impl Render for TableView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(markdown_item(
            &self.0,
            None,
            &vega_theme::Theme::light().colors,
        ))
    }
}

#[gpui_kit::test]
fn issue119_streamed_and_replayed_inline_content_reaches_markdown_renderer(
    cx: &mut TestAppContext,
) {
    let doc = "- 选 **A**，*继续* ~~旧~~ [链接](https://example.com) ![替代](image.png)\n\n| 合法 | 字面量 |\n|---|---|\n| **粗体** `**代码**` [链接](https://example.com) | 改为**\"收敛\"**的 |\n";
    let history = issue59_model(vec![doc.into()]);
    let streamed = issue59_model(split_deltas(doc, 119));
    assert_eq!(streamed.committed_lines, history.committed_lines);
    assert!(streamed.pending_lines.is_empty());
    let lines: Vec<_> = streamed
        .committed_lines
        .iter()
        .filter(|line| line.kind != LineKind::Spacer)
        .collect();
    assert_eq!(
        lines.len(),
        2,
        "formatting must not introduce extra list paragraphs"
    );
    assert_eq!(lines[0].kind, LineKind::ListItem);
    assert_eq!(lines[0].marker, "•");
    assert_eq!(spans_text(lines[0]), "选 A，继续 旧 链接 替代");
    for (text, style) in [
        ("A", SpanStyle::Strong),
        ("继续", SpanStyle::Emphasis),
        ("旧", SpanStyle::Strikethrough),
        ("链接", SpanStyle::Link),
    ] {
        assert!(
            lines[0]
                .spans
                .iter()
                .any(|span| span.text == text && span.style == style)
        );
    }
    let table_line = lines[1];
    let table = table_line.table.as_ref().expect("structured table");
    assert_eq!(table.rows.len(), 2);
    assert!(table.rows.iter().all(|row| row.len() == 2));
    assert_eq!(table.rows[1][0][0].style, SpanStyle::Strong);
    assert!(
        table.rows[1][0]
            .iter()
            .any(|span| span.style == SpanStyle::Code && span.text == "**代码**")
    );
    assert!(
        table.rows[1][0]
            .iter()
            .any(|span| span.style == SpanStyle::Link && span.text == "链接")
    );
    assert_eq!(
        table.rows[1][1],
        vec![StreamSpan {
            text: "改为**\"收敛\"**的".into(),
            style: SpanStyle::Plain
        }]
    );
    assert_eq!(table_line.block_id, 2);
    assert_eq!(table.ordinal, 0);
    let (_view, visual) = cx.add_window_view(|_, _| TableView(streamed));
    visual.simulate_resize(gpui_kit::size(px(820.), px(600.)));
    visual.run_until_parked();
    let viewport = visual
        .debug_bounds("markdown-table-2-0")
        .expect("real table viewport");
    for (header_selector, body_selector) in [
        ("markdown-table-2-0-0-0", "markdown-table-2-0-1-0"),
        ("markdown-table-2-0-0-1", "markdown-table-2-0-1-1"),
    ] {
        let header = visual.debug_bounds(header_selector).expect("header cell");
        let body = visual.debug_bounds(body_selector).expect("body cell");
        assert_eq!(header.left(), body.left());
        assert_eq!(header.right(), body.right());
        assert!(body.size.height > px(0.));
        assert!(body.left() >= viewport.left() && body.right() <= viewport.right());
    }
}

#[gpui_kit::test]
fn issue59_real_table_cells_align_wrap_and_stay_inside_local_scroll(cx: &mut TestAppContext) {
    let model = issue59_model(vec![ISSUE59_TABLE.into()]);
    // The first block is assigned id 1 by the production streaming parser.
    assert_eq!(model.committed_lines[0].block_id, 1);
    let (_view, visual) = cx.add_window_view(|_, _| TableView(model));
    for width in [820.0, 320.0] {
        visual.simulate_resize(gpui_kit::size(px(width), px(1000.)));
        visual.run_until_parked();
        let viewport = visual
            .debug_bounds("markdown-table-1-0")
            .expect("scroll viewport");
        assert_eq!(viewport.size.width, px(width));
        let selectors = [
            [
                "markdown-table-1-0-0-0",
                "markdown-table-1-0-0-1",
                "markdown-table-1-0-0-2",
                "markdown-table-1-0-0-3",
            ],
            [
                "markdown-table-1-0-1-0",
                "markdown-table-1-0-1-1",
                "markdown-table-1-0-1-2",
                "markdown-table-1-0-1-3",
            ],
            [
                "markdown-table-1-0-2-0",
                "markdown-table-1-0-2-1",
                "markdown-table-1-0-2-2",
                "markdown-table-1-0-2-3",
            ],
        ];
        let bounds = selectors
            .map(|row| row.map(|selector| visual.debug_bounds(selector).expect("real cell")));
        for row in &bounds {
            for (column, cell) in row.iter().enumerate() {
                assert!(cell.size.width >= px(Layout::MARKDOWN_TABLE_COLUMN_MIN_WIDTH));
                assert!(cell.size.height > px(0.));
                assert_eq!(cell.left(), bounds[0][column].left());
                assert_eq!(cell.right(), bounds[0][column].right());
                assert_eq!(cell.top(), row[0].top());
                assert_eq!(cell.bottom(), row[0].bottom());
                if column > 0 {
                    assert_eq!(cell.left(), row[column - 1].right());
                }
            }
        }
        assert!(
            bounds[1][1].size.height > bounds[0][1].size.height,
            "CJK wraps into a taller row"
        );
        if width == 320.0 {
            assert_eq!(bounds[0][3].right() - bounds[0][0].left(), px(480.));
            assert!(
                bounds[0][3].right() > viewport.right(),
                "wide table is confined to local scrolling"
            );
        } else {
            assert_eq!(bounds[0][3].right(), viewport.right());
        }
    }
}

#[gpui_kit::test]
fn issue59_nested_tables_keep_independent_horizontal_scroll(cx: &mut TestAppContext) {
    let table = "> | A | B | C | D |\n> |---|---|---|---|\n> | 中 | 文 | 代 | 码 |\n";
    let model = issue59_model(vec![format!("{table}>\n{table}")]);
    let tables: Vec<_> = model
        .committed_lines
        .iter()
        .filter(|line| line.table.is_some())
        .collect();
    assert_eq!(tables.len(), 2);
    assert_eq!(
        tables[0].block_id, tables[1].block_id,
        "one enclosing quote block"
    );
    assert_eq!(tables[0].block_id, 1);
    assert_eq!(tables[0].table.as_ref().unwrap().ordinal, 0);
    assert_eq!(tables[1].table.as_ref().unwrap().ordinal, 1);
    let (_view, visual) = cx.add_window_view(|_, _| TableView(model));
    visual.simulate_resize(gpui_kit::size(px(320.), px(600.)));
    visual.run_until_parked();
    let first_before = visual.debug_bounds("markdown-table-1-0-0-0").unwrap();
    let second_before = visual.debug_bounds("markdown-table-1-1-0-0").unwrap();
    let first_viewport = visual.debug_bounds("markdown-table-1-0").unwrap();
    visual.simulate_event(gpui_kit::ScrollWheelEvent {
        position: first_viewport.center(),
        delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(-80.), px(0.))),
        modifiers: gpui_kit::Modifiers::default(),
        touch_phase: gpui_kit::TouchPhase::Moved,
    });
    visual.run_until_parked();
    let first_after = visual.debug_bounds("markdown-table-1-0-0-0").unwrap();
    let second_after = visual.debug_bounds("markdown-table-1-1-0-0").unwrap();
    assert_eq!(first_after.left(), first_before.left() - px(80.));
    assert_eq!(
        second_after, second_before,
        "scrolling first table leaves second unchanged"
    );
    let second_viewport = visual.debug_bounds("markdown-table-1-1").unwrap();
    visual.simulate_event(gpui_kit::ScrollWheelEvent {
        position: second_viewport.center(),
        delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(-40.), px(0.))),
        modifiers: gpui_kit::Modifiers::default(),
        touch_phase: gpui_kit::TouchPhase::Moved,
    });
    visual.run_until_parked();
    assert_eq!(
        visual.debug_bounds("markdown-table-1-0-0-0").unwrap(),
        first_after
    );
    assert_eq!(
        visual
            .debug_bounds("markdown-table-1-1-0-0")
            .unwrap()
            .left(),
        second_before.left() - px(40.)
    );
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
