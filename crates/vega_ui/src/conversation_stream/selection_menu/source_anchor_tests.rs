use super::*;
use crate::conversation_stream::model::{StreamCounters, StreamModel};
use core::prelude::v1::test;
use vega_markdown::MarkdownStream;

fn content(source: &str, start: usize, domain: u64) -> Content {
    let mut stream = MarkdownStream::new();
    stream.append(source);
    let mut model = StreamModel::default();
    model.sync(&stream.snapshot(), &StreamCounters::default());
    Content::from_model(&model, 42)
        .with_source(source.to_owned(), 0, start)
        .with_source_domain(domain)
}

fn at(content: &Content, needle: &str, local: usize, affinity: Affinity, inset: f32) -> Anchor {
    let byte = content.text.find(needle).unwrap() + local;
    Anchor::from_display(content, &content.mapping(""), byte, affinity, inset).unwrap()
}

#[test]
fn partial_paragraph_trim_preserves_absolute_caret_affinity_and_negative_inset() {
    let old = content("before 😀retained tail\n\n", 0, 1);
    let new = content("😀retained tail\n\n", 7, 1);
    let anchor = at(&old, "😀retained", "😀ret".len(), Affinity::Upstream, -6.25);
    assert_eq!(anchor.position.source, 14..14);
    let restored = anchor.remap(&new, &old, "", "").unwrap();
    assert_eq!(restored, anchor);
    assert_eq!(restored.affinity, Affinity::Upstream);
    assert_eq!(restored.inset, -6.25);
    assert_eq!(
        new.mapping("")
            .iter()
            .find_map(|span| span.rendered_position(&restored.position)),
        Some(7)
    );
}

#[test]
fn removed_caret_clamps_to_next_retained_source_not_equal_repeated_text() {
    let old = content("same\n\nsame\n\n", 0, 1);
    let new = content("same\n\n", 6, 1);
    let anchor = at(&old, "same", 2, Affinity::Upstream, 9.5);
    let restored = anchor.remap(&new, &old, "", "").unwrap();
    assert_eq!(restored.position.source, 6..6);
    assert_eq!(restored.affinity, Affinity::Downstream);
    assert_eq!(restored.inset, 0.);
}

#[test]
fn summary_domain_equal_offsets_and_changed_or_disjoint_overlap_reject() {
    let old = content("before retained\n\n", 0, 1);
    let anchor = at(&old, "retained", 3, Affinity::Downstream, -3.);
    assert!(
        anchor
            .remap(&content("before retained\n\n", 0, 2), &old, "", "")
            .is_none()
    );
    assert!(
        anchor
            .remap(&content("before RETAINED\n\n", 0, 1), &old, "", "")
            .is_none()
    );
    assert!(
        anchor
            .remap(&content("retained\n\n", 99, 1), &old, "", "")
            .is_none()
    );
}

#[test]
fn decoded_entity_keeps_source_span_instead_of_markdown_or_display_offset() {
    let old = content("before &amp; after\n\n", 0, 1);
    let new = content("&amp; after\n\n", 7, 1);
    let anchor = at(&old, "&", 0, Affinity::Downstream, 2.);
    assert_eq!(
        anchor.position,
        SourcePosition {
            source: 7..12,
            offset: 0
        }
    );
    let restored = anchor.remap(&new, &old, "", "").unwrap();
    assert_eq!(restored, anchor);
    let byte = new
        .mapping("")
        .iter()
        .find_map(|span| span.rendered_position(&restored.position))
        .unwrap();
    assert_eq!(new.text.get(byte..byte + 1), Some("&"));
}

#[test]
fn synthetic_prefix_never_becomes_absolute_thinking_source() {
    let source = "```rust\nline\n";
    let old = content(source, 0, 1).with_source(source.to_owned(), 8, 100);
    let anchor = at(&old, "line", 2, Affinity::Downstream, -2.);
    assert_eq!(anchor.position.source, 102..102);
    assert_eq!(anchor.position.offset, 0);
}

#[test]
fn split_surrogate_interior_and_nonfinite_inset_cannot_create_anchor() {
    let text = content("A😀中e\u{301}\n\n", 0, 1);
    let byte = text.text.find('😀').unwrap();
    assert!(
        Anchor::from_display(&text, &text.mapping(""), byte + 1, Affinity::Downstream, 0.)
            .is_none()
    );
    assert!(
        Anchor::from_display(
            &text,
            &text.mapping(""),
            byte,
            Affinity::Downstream,
            f32::NAN
        )
        .is_none()
    );
    assert_eq!(utf16_byte("A😀中e\u{301}", 2), None);
    assert_eq!(utf16_byte("A😀中e\u{301}", 3), Some(5));
}

#[test]
fn wrap_reflow_uses_signed_unclipped_caret_delta_not_row_top() {
    assert_eq!(correction(37.5, 12., -4.25), Some(29.75));
    assert_eq!(correction(7.75, 12., -4.25), Some(0.));
    assert_eq!(correction(-16., 12., 3.), Some(-31.));
    assert_eq!(correction(f32::INFINITY, 12., 0.), None);
    assert_eq!(correction(10., f32::NAN, 0.), None);
}

#[test]
fn unmappable_but_untrimmed_caret_does_not_silently_clamp_forward() {
    let old = content("prefix retained\n\n", 0, 1);
    let invalid = Anchor {
        position: SourcePosition {
            source: 7..10,
            offset: 999,
        },
        affinity: Affinity::Downstream,
        inset: 4.,
    };
    assert!(invalid.remap(&old, &old, "", "").is_none());
}
