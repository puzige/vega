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
