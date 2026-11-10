use super::super::super::super::model::{StreamCounters, StreamModel};
use super::*;
use core::prelude::v1::test;
use vega_markdown::MarkdownStream;

fn model(source: &str) -> StreamModel {
    let mut stream = MarkdownStream::new();
    stream.append(source);
    let mut model = StreamModel::default();
    model.sync(&stream.snapshot(), &StreamCounters::default());
    model
}

fn mapped_text(content: &Content, part: &Part) -> String {
    part.segments
        .iter()
        .map(|segment| {
            let fragment = content
                .fragments
                .iter()
                .find(|fragment| fragment.id == segment.fragment)
                .unwrap();
            fragment.text[segment.range.clone()].to_owned()
        })
        .collect()
}

#[test]
fn nested_list_marker_uses_the_model_owner_once_without_duplicate_ranges() {
    let model = model("- outer\n  - inner\n\n");
    let content = Content::from_model(&model, 17);
    let markers: Vec<_> = content
        .opacity
        .0
        .iter()
        .filter(|part| part.key.3 == TextPart::Marker)
        .collect();
    assert_eq!(markers.len(), 2);
    assert_ne!(markers[0].key, markers[1].key);
    for part in markers {
        assert_eq!(mapped_text(&content, part), "•");
    }
}

#[test]
fn styled_display_parts_preserve_scalar_ranges_without_markdown_offsets() {
    let model = model("A **😀中** [e\u{301}](https://example.com) &amp; `code`\n\n");
    let content = Content::from_model(&model, 17);
    let part = content
        .opacity
        .0
        .iter()
        .find(|part| part.key.3 == TextPart::Body)
        .unwrap();
    assert_eq!(part.text.as_ref(), "A 😀中 e\u{301} & code");
    assert_eq!(mapped_text(&content, part), part.text.as_ref());
    assert!(part.segments.len() >= 5);
    let emoji = part
        .segments
        .iter()
        .find(|segment| {
            content
                .fragments
                .iter()
                .any(|fragment| fragment.id == segment.fragment && fragment.text == "😀中")
        })
        .unwrap();
    assert_eq!(emoji.part, 2..9);
    assert_eq!(native_range("😀中", 0..4, 7), Some(7..9));
    assert_eq!(native_range("😀中", 4..7, 7), Some(9..10));
    assert_eq!(native_range("😀中", 1..4, 7), None);
    assert_eq!(native_range("😀中", 0..3, 7), None);
    assert_eq!(native_range("😀中", 0..8, 7), None);
    assert_eq!(native_range("😀中", 0..7, u32::MAX), None);
}

#[test]
fn repeated_body_and_cells_keep_distinct_exact_part_keys_and_fragment_ids() {
    let model = model("same\n\nsame\n\n| same | same |\n| --- | --- |\n| same | same |\n\n");
    let content = Content::from_model(&model, 17);
    let bodies: Vec<_> = content
        .opacity
        .0
        .iter()
        .filter(|part| part.key.3 == TextPart::Body && part.text.as_ref() == "same")
        .collect();
    assert_eq!(bodies.len(), 2);
    assert_ne!(bodies[0].key, bodies[1].key);
    assert_ne!(
        bodies[0].segments[0].fragment,
        bodies[1].segments[0].fragment
    );
    let cells: Vec<_> = content
        .opacity
        .0
        .iter()
        .filter(|part| matches!(part.key.3, TextPart::Cell(_)))
        .collect();
    assert_eq!(cells.len(), 4);
    for (index, part) in cells.iter().enumerate() {
        assert_eq!(part.key.2, index / 2);
        assert_eq!(part.key.3, TextPart::Cell(index % 2));
        assert_eq!(part.text.as_ref(), "same");
        assert_eq!(mapped_text(&content, part), "same");
    }
}

#[test]
fn markers_checkboxes_and_code_lines_map_only_their_native_ink() {
    let model = model("- same\n- [x] task\n\n```text\nsame\n😀\n```\n\n");
    let content = Content::from_model(&model, 17);
    let marker = content
        .opacity
        .0
        .iter()
        .find(|part| part.key.3 == TextPart::Marker)
        .unwrap();
    assert_eq!(marker.text.as_ref(), "• ");
    assert_eq!(mapped_text(&content, marker), "•");
    assert_eq!(marker.segments[0].part.end, "•".len());
    let checkbox = content
        .opacity
        .0
        .iter()
        .find(|part| part.key.3 == TextPart::Checkbox)
        .unwrap();
    assert_eq!(checkbox.text.as_ref(), "[x]");
    assert_eq!(mapped_text(&content, checkbox), "[x]");
    let code: Vec<_> = content
        .opacity
        .0
        .iter()
        .filter(|part| {
            part.segments.iter().any(|segment| {
                content
                    .fragments
                    .iter()
                    .any(|fragment| fragment.id == segment.fragment && fragment.code)
            })
        })
        .collect();
    assert_eq!(code.len(), 2);
    assert_eq!(code[0].text.as_ref(), "same");
    assert_eq!(code[1].text.as_ref(), "😀");
    assert_eq!(code[0].segments[0].fragment, code[1].segments[0].fragment);
    assert_ne!(code[0].key.1, code[1].key.1);
    assert_eq!(code[0].segments[0].range, 0..4);
    assert_eq!(code[1].segments[0].range, 5..9);
}

#[test]
fn mismatched_display_model_cannot_bind_equal_text_from_a_different_part() {
    let mut model = model("first\n\nsecond\n\n");
    model.committed_lines[0].spans[0].text = "second".into();
    let content = Content::from_model(&model, 17);
    assert!(
        !content
            .opacity
            .0
            .iter()
            .any(|part| part.text.as_ref() == "first")
    );
    let second: Vec<_> = content
        .opacity
        .0
        .iter()
        .filter(|part| part.text.as_ref() == "second")
        .collect();
    assert_eq!(second.len(), 1);
    assert_eq!(mapped_text(&content, second[0]), "second");
}
