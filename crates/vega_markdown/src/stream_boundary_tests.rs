use crate::{BlockView, Inline, ListBlock, ListItem, MarkdownStream, RenderNode};

fn paragraph(text: &str) -> RenderNode {
    RenderNode::Paragraph {
        spans: vec![Inline::Text(text.to_owned())],
    }
}

fn code(text: &str) -> RenderNode {
    RenderNode::CodeBlock {
        language: Some("text".to_owned()),
        code: text.to_owned(),
    }
}

fn list(child: RenderNode) -> RenderNode {
    RenderNode::List(ListBlock {
        ordered: false,
        start: 1,
        items: vec![ListItem {
            checked: None,
            children: vec![child],
        }],
    })
}

fn quoted_source(source: &str, quoted: bool) -> String {
    if quoted {
        source
            .split_inclusive('\n')
            .map(|line| format!("> {line}"))
            .collect()
    } else {
        source.to_owned()
    }
}

fn quoted_nodes(children: Vec<RenderNode>, quoted: bool) -> Vec<RenderNode> {
    if quoted {
        vec![RenderNode::BlockQuote { children }]
    } else {
        children
    }
}

fn visible_nodes(stream: &MarkdownStream) -> Vec<RenderNode> {
    let snapshot = stream.snapshot();
    snapshot
        .blocks
        .iter()
        .flat_map(|block| block.nodes.iter())
        .chain(
            snapshot
                .pending
                .into_iter()
                .flat_map(|block| block.nodes.iter()),
        )
        .cloned()
        .collect()
}

fn identity(block: &BlockView<'_>) -> (u64, u64, *const RenderNode) {
    (block.block_id, block.version, block.nodes.as_ptr())
}

fn chunk_schedules(source: &str) -> [(&str, Vec<&str>); 3] {
    let characters = source
        .char_indices()
        .map(|(index, character)| &source[index..index + character.len_utf8()])
        .collect();
    let mut boundaries = vec![0, source.len()];
    for (index, character) in source.char_indices() {
        if matches!(character, '-' | '`' | '~' | '>' | '\n') {
            boundaries.extend([index, index + character.len_utf8()]);
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    let syntax = boundaries
        .windows(2)
        .map(|pair| &source[pair[0]..pair[1]])
        .collect();
    [
        ("whole", vec![source]),
        ("unicode_characters", characters),
        ("syntax_boundaries", syntax),
    ]
}

fn finish_with_expected(stream: &mut MarkdownStream, expected: &[RenderNode], context: &str) {
    stream.finish();
    assert!(stream.snapshot().pending.is_none(), "{context}");
    assert_eq!(visible_nodes(stream), expected, "{context}");
    let committed: Vec<_> = stream.snapshot().blocks.iter().map(identity).collect();
    stream.finish();
    let snapshot = stream.snapshot();
    assert!(snapshot.pending.is_none(), "{context}");
    assert_eq!(
        snapshot.blocks.iter().map(identity).collect::<Vec<_>>(),
        committed,
        "{context}"
    );
    assert_eq!(visible_nodes(stream), expected, "{context}");
}

#[test]
fn list_fences_keep_following_prose_outside_code_across_chunk_schedules() {
    for fence in ["```", "~~~"] {
        for quoted in [false, true] {
            let source = quoted_source(
                &format!(
                    "before\n\n- {fence}text\n  code-first中🙂\n  second-line\n  {fence}\n\nafter\n"
                ),
                quoted,
            );
            let expected = quoted_nodes(
                vec![
                    paragraph("before"),
                    list(code("code-first中🙂\nsecond-line\n")),
                    paragraph("after"),
                ],
                quoted,
            );
            for (schedule, chunks) in chunk_schedules(&source) {
                let mut stream = MarkdownStream::new();
                for chunk in chunks {
                    stream.append(chunk);
                }
                finish_with_expected(
                    &mut stream,
                    &expected,
                    &format!("fence={fence} quoted={quoted} schedule={schedule}"),
                );
            }
        }
    }
}

#[test]
fn list_fences_preserve_internal_and_terminal_blank_code_lines_verbatim() {
    for fence in ["```", "~~~"] {
        for quoted in [false, true] {
            let source = quoted_source(
                &format!(
                    "before\n\n- {fence}text\n  first中\n  \n  second🙂\n  \n  {fence}\n\nafter\n"
                ),
                quoted,
            );
            let expected = quoted_nodes(
                vec![
                    paragraph("before"),
                    list(code("first中\n\nsecond🙂\n\n")),
                    paragraph("after"),
                ],
                quoted,
            );
            for (schedule, chunks) in chunk_schedules(&source) {
                let mut stream = MarkdownStream::new();
                for chunk in chunks {
                    stream.append(chunk);
                }
                finish_with_expected(
                    &mut stream,
                    &expected,
                    &format!("fence={fence} quoted={quoted} schedule={schedule}"),
                );
            }
        }
    }
}

#[test]
fn top_level_fences_after_lists_remain_separate_blocks() {
    for fence in ["```", "~~~"] {
        let source = format!("- item\n{fence}text\noutside中\n{fence}\n\nafter\n");
        let expected = vec![
            list(paragraph("item")),
            code("outside中\n"),
            paragraph("after"),
        ];
        for (schedule, chunks) in chunk_schedules(&source) {
            let mut stream = MarkdownStream::new();
            for chunk in chunks {
                stream.append(chunk);
            }
            finish_with_expected(
                &mut stream,
                &expected,
                &format!("fence={fence} schedule={schedule}"),
            );
            let snapshot = stream.snapshot();
            assert_eq!(snapshot.blocks.len(), expected.len());
            for (block, node) in snapshot.blocks.iter().zip(&expected) {
                assert_eq!(block.nodes, std::slice::from_ref(node));
            }
        }
    }
}

#[test]
fn list_fence_snapshots_keep_unrelated_committed_nodes_frozen() {
    for fence in ["```", "~~~"] {
        for quoted in [false, true] {
            let mut stream = MarkdownStream::new();
            stream.append("stable\n\n");
            stream.append(&quoted_source(
                &format!("before\n\n- {fence}text\n  first中\n"),
                quoted,
            ));
            let stable = {
                let snapshot = stream.snapshot();
                let block = snapshot.blocks.first().expect("committed stable paragraph");
                assert_eq!(block.nodes, &[paragraph("stable")]);
                assert!(snapshot.pending.is_some());
                identity(block)
            };
            for (delta, code_text, following_prose) in [
                (String::new(), "first中\n", false),
                (
                    "  second-line\n".to_owned(),
                    "first中\nsecond-line\n",
                    false,
                ),
                (format!("  {fence}\n"), "first中\nsecond-line\n", false),
                ("\nafter\n".to_owned(), "first中\nsecond-line\n", true),
            ] {
                if !delta.is_empty() {
                    stream.append(&quoted_source(&delta, quoted));
                }
                let mut children = vec![paragraph("before"), list(code(code_text))];
                if following_prose {
                    children.push(paragraph("after"));
                }
                let mut expected = vec![paragraph("stable")];
                expected.extend(quoted_nodes(children, quoted));
                let context = format!("fence={fence} quoted={quoted} delta={delta:?}");
                assert_eq!(visible_nodes(&stream), expected, "{context}");
                let snapshot = stream.snapshot();
                assert_eq!(identity(&snapshot.blocks[0]), stable, "{context}");
                assert_eq!(
                    snapshot.blocks[0].nodes,
                    &[paragraph("stable")],
                    "{context}"
                );
                assert!(snapshot.pending.is_some(), "{context}");
            }
            let mut expected = vec![paragraph("stable")];
            expected.extend(quoted_nodes(
                vec![
                    paragraph("before"),
                    list(code("first中\nsecond-line\n")),
                    paragraph("after"),
                ],
                quoted,
            ));
            finish_with_expected(
                &mut stream,
                &expected,
                &format!("fence={fence} quoted={quoted}"),
            );
            assert_eq!(identity(&stream.snapshot().blocks[0]), stable);
            assert_eq!(stream.snapshot().blocks[0].nodes, &[paragraph("stable")]);
        }
    }
}

#[test]
fn late_reference_definitions_update_only_the_affected_committed_block() {
    let mut stream = MarkdownStream::new();
    stream.append("stable\n\n[label][target]\n\npending\n");
    assert_eq!(
        visible_nodes(&stream),
        vec![
            paragraph("stable"),
            paragraph("[label][target]"),
            paragraph("pending"),
        ]
    );
    let (stable, unresolved) = {
        let snapshot = stream.snapshot();
        assert_eq!(snapshot.blocks.len(), 2);
        assert_eq!(snapshot.blocks[0].nodes, &[paragraph("stable")]);
        assert_eq!(snapshot.blocks[1].nodes, &[paragraph("[label][target]")]);
        assert!(snapshot.pending.is_some());
        (identity(&snapshot.blocks[0]), identity(&snapshot.blocks[1]))
    };
    stream.append("\n[target]: https://example.com/reference\n\nlater\n");
    let resolved = RenderNode::Paragraph {
        spans: vec![Inline::Link {
            url: "https://example.com/reference".to_owned(),
            title: None,
            spans: vec![Inline::Text("label".to_owned())],
        }],
    };
    let expected = vec![
        paragraph("stable"),
        resolved.clone(),
        paragraph("pending"),
        paragraph("later"),
    ];
    assert_eq!(visible_nodes(&stream), expected);
    {
        let snapshot = stream.snapshot();
        assert_eq!(identity(&snapshot.blocks[0]), stable);
        assert_eq!(snapshot.blocks[0].nodes, &[paragraph("stable")]);
        assert_eq!(snapshot.blocks[1].block_id, unresolved.0);
        assert_eq!(snapshot.blocks[1].version, unresolved.1 + 1);
        assert_eq!(snapshot.blocks[1].nodes, &[resolved]);
    }
    finish_with_expected(&mut stream, &expected, "late reference definition");
    assert_eq!(identity(&stream.snapshot().blocks[0]), stable);
    assert_eq!(stream.snapshot().blocks[0].nodes, &[paragraph("stable")]);
}

mod list_fence_ownership_tests {
    use super::*;

    fn ordered_list(start: u64, children: Vec<RenderNode>) -> RenderNode {
        RenderNode::List(ListBlock {
            ordered: true,
            start,
            items: vec![ListItem {
                checked: None,
                children,
            }],
        })
    }

    fn schedules(source: &str) -> [(&str, Vec<&str>); 3] {
        let [whole, characters, _] = chunk_schedules(source);
        let mut boundaries = vec![0, source.len()];
        for (index, character) in source.char_indices() {
            if character.is_ascii_digit() || matches!(character, '.' | ' ' | '`' | '~' | '#' | '\n')
            {
                boundaries.extend([index, index + character.len_utf8()]);
            }
        }
        boundaries.sort_unstable();
        boundaries.dedup();
        let chunks = boundaries
            .windows(2)
            .map(|pair| &source[pair[0]..pair[1]])
            .collect();
        [whole, characters, ("ordered_syntax_boundaries", chunks)]
    }

    #[test]
    fn dedented_fences_after_wide_ordered_markers_keep_heading_syntax_as_code() {
        for spaces in [2, 3] {
            let indent = " ".repeat(spaces);
            for fence in ["```", "~~~"] {
                let source = format!(
                    "10. item中\n{indent}{fence}text\n# literal\n{indent}{fence}\n\nafter🙂\n"
                );
                let expected = vec![
                    ordered_list(10, vec![paragraph("item中")]),
                    code("# literal\n"),
                    paragraph("after🙂"),
                ];
                for (schedule, chunks) in schedules(&source) {
                    let mut stream = MarkdownStream::new();
                    for chunk in chunks {
                        stream.append(chunk);
                    }
                    let context = format!("spaces={spaces} fence={fence} schedule={schedule}");
                    assert_eq!(visible_nodes(&stream), expected, "{context}");
                    finish_with_expected(&mut stream, &expected, &context);
                }
            }
        }
    }

    #[test]
    fn ordered_list_fence_continuations_preserve_code_blank_lines() {
        for (start, spaces) in [(10, 4), (100, 5)] {
            let indent = " ".repeat(spaces);
            for fence in ["```", "~~~"] {
                let source = format!(
                    "{start}. {fence}text\n{indent}first🙂\n{indent}\n{indent}second\n{indent}\n{indent}{fence}\n\nafter中\n"
                );
                let expected = vec![
                    ordered_list(start, vec![code("first🙂\n\nsecond\n\n")]),
                    paragraph("after中"),
                ];
                for (schedule, chunks) in schedules(&source) {
                    let mut stream = MarkdownStream::new();
                    for chunk in chunks {
                        stream.append(chunk);
                    }
                    let context =
                        format!("start={start} spaces={spaces} fence={fence} schedule={schedule}");
                    assert_eq!(visible_nodes(&stream), expected, "{context}");
                    finish_with_expected(&mut stream, &expected, &context);
                }
            }
        }
    }

    #[test]
    fn list_fence_bodies_keep_block_syntax_literal_in_pending_and_final_snapshots() {
        for (marker, start, spaces) in [("-", None, 2), ("10.", Some(10), 4)] {
            let indent = " ".repeat(spaces);
            for fence in ["```", "~~~"] {
                let expected_list = |text: &str| match start {
                    Some(start) => ordered_list(start, vec![code(text)]),
                    None => list(code(text)),
                };
                let mut source = format!("{marker} {fence}text\n");
                let mut stream = MarkdownStream::new();
                stream.append(&source);
                for (delta, text) in [
                    (format!("{indent}# literal\n"), "# literal\n"),
                    (format!("{indent}\n"), "# literal\n\n"),
                    (format!("{indent}---\n"), "# literal\n\n---\n"),
                    (format!("{indent}second\n"), "# literal\n\n---\nsecond\n"),
                    (format!("{indent}{fence}\n"), "# literal\n\n---\nsecond\n"),
                ] {
                    source.push_str(&delta);
                    stream.append(&delta);
                    assert_eq!(
                        visible_nodes(&stream),
                        vec![expected_list(text)],
                        "marker={marker} fence={fence} delta={delta:?}"
                    );
                }
                source.push_str("\nafter🙂\n");
                stream.append("\nafter🙂\n");
                let expected = vec![
                    expected_list("# literal\n\n---\nsecond\n"),
                    paragraph("after🙂"),
                ];
                assert_eq!(visible_nodes(&stream), expected);
                finish_with_expected(&mut stream, &expected, "list code literal snapshots");
                for (schedule, chunks) in schedules(&source) {
                    let mut stream = MarkdownStream::new();
                    for chunk in chunks {
                        stream.append(chunk);
                    }
                    let context = format!("marker={marker} fence={fence} schedule={schedule}");
                    assert_eq!(visible_nodes(&stream), expected, "{context}");
                    finish_with_expected(&mut stream, &expected, &context);
                }
            }
        }
    }
}

mod prose_fence_repair_tests {
    use super::*;

    fn expected(prose: &str, repaired: bool, code_text: &str) -> Vec<RenderNode> {
        let prose = if repaired {
            RenderNode::Paragraph {
                spans: vec![
                    Inline::Text("before ".to_owned()),
                    Inline::Strikethrough(vec![Inline::Text("unfinished".to_owned())]),
                ],
            }
        } else {
            paragraph(prose)
        };
        vec![RenderNode::List(ListBlock {
            ordered: false,
            start: 1,
            items: vec![ListItem {
                checked: None,
                children: vec![prose, code(code_text)],
            }],
        })]
    }

    fn assert_pending(stream: &MarkdownStream, expected: &[RenderNode], context: &str) {
        let snapshot = stream.snapshot();
        assert!(
            snapshot.blocks.is_empty(),
            "{context}: list committed before finish"
        );
        assert_eq!(
            snapshot.pending.expect("pending list").nodes,
            expected,
            "{context}"
        );
    }

    fn verify_fence(fence: &str, closed_prose: bool) {
        let prose = if closed_prose {
            "before ~~unfinished~~"
        } else {
            "before ~~unfinished"
        };
        let stages = [
            (
                format!("- {prose}\n\n  {fence}text\n  literal\n"),
                "literal\n",
            ),
            ("  \n  尾🙂\n".to_owned(), "literal\n\n尾🙂\n"),
            (format!("  {fence}\n"), "literal\n\n尾🙂\n"),
        ];
        let mut incremental = MarkdownStream::new();
        let mut source = String::new();
        for (stage, (delta, code_text)) in stages.iter().enumerate() {
            source.push_str(delta);
            incremental.append(delta);
            let repaired = expected(prose, true, code_text);
            let raw = expected(prose, closed_prose, code_text);
            let context = format!("fence={fence} closed_prose={closed_prose} stage={stage}");
            assert_pending(&incremental, &repaired, &context);
            for (schedule, chunks) in chunk_schedules(&source) {
                let mut stream = MarkdownStream::new();
                for chunk in chunks {
                    stream.append(chunk);
                }
                let context = format!("{context} schedule={schedule}");
                assert_pending(&stream, &repaired, &context);
                finish_with_expected(&mut stream, &raw, &context);
            }
        }
        finish_with_expected(
            &mut incremental,
            &expected(prose, closed_prose, "literal\n\n尾🙂\n"),
            &format!("fence={fence} closed_prose={closed_prose} incremental finish"),
        );
    }

    #[test]
    fn pending_prose_repair_keeps_backtick_fences_on_their_original_line() {
        verify_fence("```", false);
    }

    #[test]
    fn pending_prose_repair_keeps_tilde_fences_on_their_original_line() {
        verify_fence("~~~", false);
    }

    #[test]
    fn closed_strikethrough_before_list_fences_preserves_code_and_real_blank_lines() {
        for fence in ["```", "~~~"] {
            verify_fence(fence, true);
        }
    }
}

#[test]
fn quote_first_list_continuations_preserve_owned_quote_paragraphs_across_chunks() {
    let quote = |children| RenderNode::BlockQuote { children };
    let cases = [
        (
            "dedented_independent_quote",
            "- > first\n\n> outside\n",
            vec![list(quote(vec![paragraph("first")]))],
            vec![quote(vec![paragraph("outside")])],
        ),
        (
            "depth_zero_continuation",
            "- > first\n  >\n  > second\n",
            vec![],
            vec![list(quote(vec![paragraph("first"), paragraph("second")]))],
        ),
        (
            "depth_one_continuation",
            "- parent\n  - > first\n    >\n    > second\n",
            vec![],
            vec![RenderNode::List(ListBlock {
                ordered: false,
                start: 1,
                items: vec![ListItem {
                    checked: None,
                    children: vec![
                        paragraph("parent"),
                        list(quote(vec![paragraph("first"), paragraph("second")])),
                    ],
                }],
            })],
        ),
    ];
    for (case, source, committed, pending) in cases {
        let expected: Vec<_> = committed.iter().chain(&pending).cloned().collect();
        for (schedule, chunks) in chunk_schedules(source) {
            let mut stream = MarkdownStream::new();
            for chunk in chunks {
                stream.append(chunk);
            }
            let context = format!("{case} schedule={schedule}");
            let snapshot = stream.snapshot();
            assert_eq!(
                snapshot
                    .blocks
                    .iter()
                    .flat_map(|block| block.nodes.iter())
                    .cloned()
                    .collect::<Vec<_>>(),
                committed,
                "{context}: committed tree"
            );
            assert_eq!(
                snapshot.pending.expect("pending quote or list").nodes,
                pending,
                "{context}: pending tree"
            );
            assert_eq!(visible_nodes(&stream), expected, "{context}");
            finish_with_expected(&mut stream, &expected, &context);
        }
    }
}
