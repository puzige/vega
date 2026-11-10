use super::super::super::model::{StreamCounters, StreamModel};
use super::*;
use core::prelude::v1::test;
use vega_markdown::MarkdownStream;

#[test]
fn source_trim_rebinds_surviving_row_without_losing_measured_inset() {
    let before = "before\n\nkept\n\n";
    let after = "kept\n\n";
    let previous = content(before).with_source(before.to_owned(), 0, 0);
    let current = content(after).with_source(after.to_owned(), 0, 8);
    let old_row = previous
        .row_at(previous.text.find("kept").unwrap())
        .unwrap();
    let new_row = current.row_at(current.text.find("kept").unwrap()).unwrap();
    assert_eq!(
        current.remap_row_anchor(&previous, (old_row, 17.5)),
        Some((new_row, 17.5))
    );
}

#[test]
fn removed_anchor_clamps_to_first_surviving_source_not_recycled_block_id() {
    let before = "before\n\nkept\n\n";
    let after = "kept\n\n";
    let previous = content(before).with_source(before.to_owned(), 0, 0);
    let current = content(after).with_source(after.to_owned(), 0, 8);
    let removed_row = previous.row_at(0).unwrap();
    let new_row = current.row_at(0).unwrap();
    assert_eq!(
        current.remap_row_anchor(&previous, (removed_row, 17.5)),
        Some((new_row, 0.))
    );
}

fn content(source: &str) -> Content {
    let mut stream = MarkdownStream::new();
    stream.append(source);
    let mut model = StreamModel::default();
    model.sync(&stream.snapshot(), &StreamCounters::default());
    Content::from_model(&model, 42)
}

#[test]
fn utf16_offsets_reject_surrogate_interior_and_overrun() {
    let text = "A😀中e\u{301}";
    assert_eq!(utf16_byte(text, 0), Some(0));
    assert_eq!(utf16_byte(text, 1), Some(1));
    assert_eq!(utf16_byte(text, 2), None);
    assert_eq!(utf16_byte(text, 3), Some(5));
    assert_eq!(utf16_byte(text, 6), Some(text.len()));
    assert_eq!(utf16_byte(text, 7), None);
}

#[test]
fn structured_markdown_maps_display_not_delimiters_or_destination() {
    let source = "# Heading\n\n**bold** &amp; [link](https://example.com) `code`\n\n";
    let content = content(source);
    assert!(content.text.contains("bold & link code"));
    assert!(!content.text.contains("https://example.com"));
    assert!(!content.text.contains("**"));
    let mapping = content.mapping(source);
    for word in ["Heading", "bold", "link", "code"] {
        let start = content.text.find(word).unwrap();
        assert!(mapping.iter().any(|span| span.rendered.start <= start && span.rendered.end >= start + word.len()));
    }
    assert!(
        mapping
            .iter()
            .any(|span| source.get(span.source.clone()) == Some("&amp;") && !span.literal)
    );
}

#[test]
fn repeated_code_lines_remain_separate_trusted_source_ranges() {
    let source = "```rust\nsame\nsame\n```\n\n";
    let content = content(source);
    let code = content
        .fragments
        .iter()
        .find(|fragment| fragment.code)
        .unwrap();
    let mapping = content.mapping(source);
    assert!(content.code_range_mapped(code, &mapping));
    let same: Vec<_> = mapping
        .iter()
        .filter(|span| source.get(span.source.clone()) == Some("same"))
        .collect();
    assert_eq!(same.len(), 2);
    assert_ne!(same[0].source, same[1].source);
}

#[test]
fn synthetic_marker_never_consumes_equal_source_text() {
    let source = "- same\n- same\n\n";
    let content = content(source);
    let mapping = content.mapping(source);
    for marker in content
        .fragments
        .iter()
        .filter(|fragment| fragment.synthetic)
    {
        assert!(
            !mapping
                .iter()
                .any(|span| span.rendered.start < marker.range.end
                    && span.rendered.end > marker.range.start)
        );
    }
    let occurrences: Vec<_> = mapping
        .iter()
        .filter(|span| source.get(span.source.clone()) == Some("same"))
        .collect();
    assert_eq!(occurrences.len(), 2);
    assert_ne!(occurrences[0].source, occurrences[1].source);
}

#[test]
fn retained_window_offsets_map_back_to_absolute_source() {
    let source = "```rust\nline\n";
    let content = content(source).with_source(source.to_owned(), 8, 100);
    let mapping = content.mapping("unrelated raw source");
    assert!(mapping.iter().any(|span| span.source == (100..104)));
    assert!(!mapping.iter().any(|span| span.source.start < 100));
}

#[test]
fn styled_repeated_links_bind_all_fragments_and_exclude_unsafe_targets() {
    use vega_markdown::Inline as SourceInline;
    let mut model = StreamModel::default();
    model.reading_nodes = vec![(
        7,
        1,
        Arc::new(vec![RenderNode::Paragraph {
            spans: vec![
                SourceInline::Link {
                    url: "https://example.com".into(),
                    title: None,
                    spans: vec![
                        SourceInline::Text("a".into()),
                        SourceInline::Strong(vec![SourceInline::Text("😀bold".into())]),
                        SourceInline::Text("z".into()),
                    ],
                },
                SourceInline::Text(" ".into()),
                SourceInline::Link {
                    url: "https://example.com".into(),
                    title: None,
                    spans: vec![SourceInline::Text("again".into())],
                },
                SourceInline::Text(" ".into()),
                SourceInline::Link {
                    url: "javascript:alert".into(),
                    title: None,
                    spans: vec![SourceInline::Text("bad".into())],
                },
            ],
        }]),
    )];
    let content = Content::from_model(&model, 42);
    let links = content.reading_links();
    assert_eq!(links.len(), 2);
    assert_eq!(links[0].0.target, "https://example.com");
    assert_eq!(links[0].0.label, "a😀boldz");
    assert_eq!(links[0].1.len(), 3);
    assert_eq!(&content.text[links[0].0.range.clone()], "a😀boldz");
    let mapped = links[0]
        .1
        .iter()
        .map(|id| {
            content
                .fragments
                .iter()
                .find(|fragment| &fragment.id == id)
                .unwrap()
                .text
                .as_str()
        })
        .collect::<String>();
    assert_eq!(mapped, "a😀boldz");
    assert_ne!(links[0].0.owner, links[1].0.owner);
    assert_eq!(links[1].0.label, "again");
    assert_eq!(&content.text[links[1].0.range.clone()], "again");
    assert!(
        links
            .iter()
            .all(|(link, _)| super::super::super::links::allowed_link(&link.target))
    );
}

#[test]
fn earlier_block_projection_does_not_rename_frozen_fragment() {
    let nodes = Arc::new(vec![RenderNode::Paragraph {
        spans: vec![vega_markdown::Inline::Text("kept".into())],
    }]);
    let mut first = StreamModel::default();
    first.reading_nodes = vec![(7, 1, nodes.clone())];
    let kept = Content::from_model(&first, 42).fragments[0].id.clone();
    first.reading_nodes.insert(
        0,
        (
            3,
            1,
            Arc::new(vec![RenderNode::Paragraph {
                spans: vec![vega_markdown::Inline::Text("before".into())],
            }]),
        ),
    );
    assert_eq!(
        Content::from_model(&first, 42)
            .fragments
            .iter()
            .find(|fragment| fragment.text == "kept")
            .unwrap()
            .id,
        kept
    );
}
