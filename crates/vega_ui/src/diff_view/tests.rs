use std::sync::{Arc, Mutex};

use super::*;
use gpui::{TestAppContext, WindowHandle};

struct Harness {
    view: Entity<DiffView>,
}

impl Render for Harness {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.view.clone())
    }
}

#[test]
fn default_layout_is_unified() {
    assert_eq!(DiffLayout::default(), DiffLayout::Unified);
}

fn line(kind: DiffRowKind, text: &str) -> PreparedLine {
    PreparedLine {
        kind,
        old_line: None,
        new_line: None,
        spans: vec![PreparedSpan {
            text: text.to_owned(),
            kind: None,
        }],
    }
}

fn prepared_text(line: &PreparedLine) -> String {
    line.spans.iter().map(|span| span.text.as_str()).collect()
}

#[test]
fn exact_language_tags_are_frozen() {
    assert_eq!(language_tag(DiffLanguage::Rust), "rs");
    assert_eq!(language_tag(DiffLanguage::TypeScript), "ts");
    assert_eq!(language_tag(DiffLanguage::Tsx), "tsx");
    assert_eq!(language_tag(DiffLanguage::JavaScript), "js");
    assert_eq!(language_tag(DiffLanguage::Python), "py");
    assert_eq!(language_tag(DiffLanguage::Plain), "");
}

#[test]
fn context_is_mirrored() {
    let pairs = pair_side_by_side(&[line(DiffRowKind::Context, "same")]);
    assert_eq!(pairs.len(), 1);
    assert_eq!(
        pairs[0].left.as_ref().map(prepared_text),
        Some("same".to_owned())
    );
    assert_eq!(
        pairs[0].right.as_ref().map(prepared_text),
        Some("same".to_owned())
    );
}

#[test]
fn consecutive_delete_add_runs_pair_by_ordinal() {
    let pairs = pair_side_by_side(&[
        line(DiffRowKind::Deletion, "old-1"),
        line(DiffRowKind::Deletion, "old-2"),
        line(DiffRowKind::Addition, "new-1"),
        line(DiffRowKind::Addition, "new-2"),
    ]);
    assert_eq!(pairs.len(), 2);
    assert_eq!(
        pairs[1].left.as_ref().map(prepared_text),
        Some("old-2".to_owned())
    );
    assert_eq!(
        pairs[1].right.as_ref().map(prepared_text),
        Some("new-2".to_owned())
    );
}

#[test]
fn shorter_addition_side_is_blank() {
    let pairs = pair_side_by_side(&[
        line(DiffRowKind::Deletion, "old-1"),
        line(DiffRowKind::Deletion, "old-2"),
        line(DiffRowKind::Addition, "new-1"),
    ]);
    assert_eq!(pairs.len(), 2);
    assert!(pairs[1].right.is_none());
}

#[test]
fn shorter_deletion_side_is_blank() {
    let pairs = pair_side_by_side(&[
        line(DiffRowKind::Deletion, "old-1"),
        line(DiffRowKind::Addition, "new-1"),
        line(DiffRowKind::Addition, "new-2"),
    ]);
    assert_eq!(pairs.len(), 2);
    assert!(pairs[1].left.is_none());
}

#[test]
fn context_breaks_pairing_runs() {
    let pairs = pair_side_by_side(&[
        line(DiffRowKind::Deletion, "old"),
        line(DiffRowKind::Context, "same"),
        line(DiffRowKind::Addition, "new"),
    ]);
    assert_eq!(pairs.len(), 3);
    assert!(pairs[0].right.is_none());
    assert!(pairs[2].left.is_none());
}

#[test]
fn expanded_identity_is_preserved_or_closed() {
    assert_eq!(reconcile_expanded(Some(2_u64), &[1, 2, 3]), Some(2));
    assert_eq!(reconcile_expanded(Some(4_u64), &[1, 2, 3]), None);
    assert_eq!(reconcile_expanded(Some(4_u64), &[]), None);
    assert_eq!(reconcile_expanded(None::<u64>, &[1, 2, 3]), None);
}

#[test]
fn projection_preservation_requires_same_generation_and_expansion() {
    assert!(should_preserve_projection(Some(7), 7, Some(2_u64), Some(2)));
    assert!(!should_preserve_projection(
        Some(7),
        8,
        Some(2_u64),
        Some(2)
    ));
    assert!(!should_preserve_projection(
        Some(7),
        7,
        Some(2_u64),
        Some(3)
    ));
}

#[test]
fn stale_candidate_requires_exact_expanded_current_id() {
    assert!(exact_current_file(Some(2_u64), [1, 2, 3], 2));
    assert!(!exact_current_file(Some(2_u64), [1, 3, 4], 2));
    assert!(!exact_current_file(Some(3_u64), [1, 2, 3], 2));
}

#[test]
fn hunk_navigation_stops_at_both_boundaries() {
    let mut current = None;
    assert_eq!(navigate_hunk(&[3, 8], &mut current, true), Some(3));
    assert_eq!(navigate_hunk(&[3, 8], &mut current, true), Some(8));
    assert_eq!(navigate_hunk(&[3, 8], &mut current, true), Some(8));
    assert_eq!(navigate_hunk(&[3, 8], &mut current, false), Some(3));
    assert_eq!(navigate_hunk(&[3, 8], &mut current, false), Some(3));
}

#[test]
fn empty_hunk_navigation_is_inert() {
    let mut current = Some(9);
    assert_eq!(navigate_hunk(&[], &mut current, true), None);
    assert_eq!(current, None);
}

#[test]
fn hunk_heading_is_structured_not_raw_patch() {
    assert_eq!(
        hunk_label(2, 3, 5, 7, Some("fn demo")),
        "@@ -2,3 +5,7 @@ fn demo"
    );
}

#[test]
fn prepared_line_preserves_long_text_as_one_row() {
    let long = "x".repeat(64 * 1024);
    let pairs = pair_side_by_side(&[line(DiffRowKind::Addition, &long)]);
    assert_eq!(pairs.len(), 1);
    assert_eq!(
        pairs[0]
            .right
            .as_ref()
            .map(|line| line.spans.iter().map(|span| span.text.len()).sum()),
        Some(long.len())
    );
}

#[test]
fn frozen_layout_constants_are_exact() {
    assert_eq!(DIFF_REFRESH_INTERVAL, Duration::from_millis(750));
    assert_eq!(DIFF_ROW_HEIGHT, 24.0);
    assert_eq!(DIFF_MIN_WINDOW_WIDTH, 960.0);
    assert_eq!(DIFF_MIN_WINDOW_HEIGHT, 600.0);
    assert_eq!(DIFF_CHANGE_BACKGROUND_OPACITY, 0.08);
}

#[gpui::test]
async fn focused_escape_closes_the_exact_diff_route(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        crate::init(cx);
    });
    let view = cx.new(|cx| DiffView::new("thread".into(), "project".into(), cx));
    let root_view = view.clone();
    let events = Arc::new(Mutex::new(Vec::new()));
    let captured = events.clone();
    let window: WindowHandle<Harness> = cx.update(|cx| {
        cx.open_window(Default::default(), move |_, cx| {
            cx.new(|cx| {
                cx.subscribe(&root_view, move |_, _, event: &DiffClosed, _| {
                    if let Ok(mut events) = captured.lock() {
                        events.push(event.clone());
                    }
                })
                .detach();
                Harness { view: root_view }
            })
        })
        .expect("diff test window")
    });
    window
        .update(cx, |_, window, cx| {
            let focus = view.read(cx).focus_handle(cx);
            window.focus(&focus, cx);
        })
        .expect("diff focus window");
    cx.simulate_keystrokes(window.into(), "] [ escape");
    let events = events.lock().expect("diff close events");
    assert_eq!(
        events.as_slice(),
        &[DiffClosed {
            thread_id: "thread".into(),
            project_id: "project".into(),
        }]
    );
}
