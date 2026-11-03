use super::{Backspace, Copy, Cut, Delete, Paste, Redo, TextInput, Undo};
use gpui_kit as gpui;
use gpui_kit::{AppContext, Context, EntityInputHandler, TestAppContext, Window};
use std::ops::Range;
use vega_conversation::types::{FileReferenceMention, SkillMention, SkillSelectionIntent};

fn intent() -> SkillSelectionIntent {
    SkillSelectionIntent {
        source_id: "project-reviewed-source".into(),
        name: "review".into(),
        content_sha256: "a".repeat(64),
        expected_consent_generation: 17,
    }
}

fn with_editor(
    cx: &mut TestAppContext,
    run: impl FnOnce(&mut TextInput, &mut Window, &mut Context<TextInput>),
) {
    let window = cx.add_window(|_, _| gpui_kit::Empty);
    window
        .update(cx, |_, window, cx| {
            let input = cx.new(|cx| TextInput::new_multiline(cx, "Message", 3));
            input.update(cx, |input, cx| run(input, window, cx));
        })
        .expect("headless editor window must remain open");
}

fn bind(input: &mut TextInput, cx: &mut Context<TextInput>) -> SkillMention {
    input.set_text("before $rev after", cx);
    input.insert_skill_mention(7..11, "$review [Project]", intent(), cx);
    let expected = SkillMention {
        range: 7..24,
        text: "$review [Project]".into(),
        intent: intent(),
    };
    assert_eq!(input.text(), "before $review [Project]  after");
    assert_eq!(input.skill_mentions(), vec![expected.clone()]);
    expected
}

fn replace(
    input: &mut TextInput,
    range: Range<usize>,
    text: &str,
    window: &mut Window,
    cx: &mut Context<TextInput>,
) {
    let range = input.range_to_utf16(&range);
    input.replace_text_in_range(Some(range), text, window, cx);
}

#[gpui_kit::test]
fn accepting_a_candidate_replaces_only_the_query_and_keeps_exact_identity(cx: &mut TestAppContext) {
    with_editor(cx, |input, _, cx| {
        let mention = bind(input, cx);
        assert_eq!(input.selected_range, 25..25);
        assert_eq!(input.accessible_label(), "Message; $review [Project]");
        assert_eq!(
            SkillMention::selections(input.text(), &input.skill_mentions()),
            Some(vec![mention.intent])
        );
        input.set_text("/rev retained body", cx);
        input.insert_skill_mention(0..4, "$review [Project]", intent(), cx);
        assert_eq!(input.text(), "$review [Project]  retained body");
        assert_eq!(input.skill_mentions()[0].range, 0..17);
    });
}

#[gpui_kit::test]
fn partial_name_source_and_interior_edits_invalidate_the_binding(cx: &mut TestAppContext) {
    with_editor(cx, |input, window, cx| {
        for (range, replacement, expected) in [
            (8..9, "", "before $eview [Project]  after"),
            (9..10, "X", "before $rXview [Project]  after"),
            (16..23, "Global", "before $review [Global]  after"),
            (10..10, "x", "before $rexview [Project]  after"),
        ] {
            bind(input, cx);
            replace(input, range, replacement, window, cx);
            assert_eq!(input.text(), expected);
            assert!(input.skill_mentions().is_empty());
            assert_eq!(
                SkillMention::selections(input.text(), &input.skill_mentions()),
                Some(vec![])
            );
        }
    });
}

#[gpui_kit::test]
fn deletion_actions_remove_partial_bindings_without_restoring_them_on_retyping(
    cx: &mut TestAppContext,
) {
    with_editor(cx, |input, window, cx| {
        bind(input, cx);
        input.move_to(9, cx);
        input.backspace(&Backspace, window, cx);
        assert_eq!(input.text(), "before $eview [Project]  after");
        assert!(input.skill_mentions().is_empty());
        input.replace_text_in_range(None, "r", window, cx);
        assert_eq!(input.text(), "before $review [Project]  after");
        assert!(input.skill_mentions().is_empty());

        bind(input, cx);
        input.move_to(8, cx);
        input.delete(&Delete, window, cx);
        assert_eq!(input.text(), "before $eview [Project]  after");
        assert!(input.skill_mentions().is_empty());
    });
}

#[gpui_kit::test]
fn unicode_edits_outside_and_at_binding_boundaries_shift_byte_ranges_only(cx: &mut TestAppContext) {
    with_editor(cx, |input, window, cx| {
        let mut mention = bind(input, cx);
        replace(input, 0..7, "你好🙂 ", window, cx);
        mention.range = 11..28;
        assert_eq!(input.text(), "你好🙂 $review [Project]  after");
        assert_eq!(input.skill_mentions(), vec![mention.clone()]);

        replace(input, 11..11, "é", window, cx);
        mention.range = 13..30;
        assert_eq!(input.skill_mentions(), vec![mention.clone()]);
        replace(input, 30..30, "界", window, cx);
        assert_eq!(input.text(), "你好🙂 é$review [Project]界  after");
        assert_eq!(input.skill_mentions(), vec![mention.clone()]);

        replace(input, 0..13, "", window, cx);
        mention.range = 0..17;
        assert_eq!(input.text(), "$review [Project]界  after");
        assert_eq!(input.skill_mentions(), vec![mention.clone()]);
        let end = input.text().len();
        replace(input, 17..end, "", window, cx);
        assert_eq!(input.text(), "$review [Project]");
        assert_eq!(input.skill_mentions(), vec![mention]);
    });
}

#[gpui_kit::test]
fn unicode_inside_a_mention_uses_utf16_replacement_and_invalidates_identity(
    cx: &mut TestAppContext,
) {
    with_editor(cx, |input, window, cx| {
        input.set_text("🙂 $rev tail", cx);
        input.insert_skill_mention(5..9, "$审阅🙂 [项目]", intent(), cx);
        let mention = input.skill_mentions().remove(0);
        assert_eq!(mention.range, 5..25);
        input.replace_text_in_range(Some(6..8), "界", window, cx);
        assert_eq!(input.text(), "🙂 $审阅界 [项目]  tail");
        assert!(input.skill_mentions().is_empty());
        input.undo(&Undo, window, cx);
        assert_eq!(input.skill_mentions(), vec![mention]);
        assert_eq!(input.text(), "🙂 $审阅🙂 [项目]  tail");
    });
}

#[gpui_kit::test]
fn duplicate_mentions_keep_the_remaining_intent_until_the_last_is_removed(cx: &mut TestAppContext) {
    with_editor(cx, |input, window, cx| {
        input.insert_skill_mention(0..0, "$review", intent(), cx);
        input.insert_skill_mention(8..8, "$review", intent(), cx);
        assert_eq!(input.text(), "$review $review ");
        assert_eq!(input.skill_mentions().len(), 2);
        assert_eq!(
            SkillMention::selections(input.text(), &input.skill_mentions()),
            Some(vec![intent()])
        );
        replace(input, 0..8, "", window, cx);
        assert_eq!(input.text(), "$review ");
        assert_eq!(input.skill_mentions()[0].range, 0..7);
        assert_eq!(
            SkillMention::selections(input.text(), &input.skill_mentions()),
            Some(vec![intent()])
        );
        replace(input, 0..7, "", window, cx);
        assert_eq!(input.text(), " ");
        assert!(input.skill_mentions().is_empty());
        assert_eq!(
            SkillMention::selections(input.text(), &input.skill_mentions()),
            Some(vec![])
        );
    });
}

#[gpui_kit::test]
fn undo_redo_restores_text_selection_and_original_authorization_intent(cx: &mut TestAppContext) {
    with_editor(cx, |input, window, cx| {
        input.set_text("$rev", cx);
        input.selected_range = 0..4;
        input.selection_reversed = true;
        input.insert_skill_mention(0..4, "$review [Project]", intent(), cx);
        let bound = input.skill_mentions();
        input.undo(&Undo, window, cx);
        assert_eq!(input.text(), "$rev");
        assert_eq!(input.selected_range, 0..4);
        assert!(input.selection_reversed);
        assert!(input.skill_mentions().is_empty());
        input.redo(&Redo, window, cx);
        assert_eq!(input.text(), "$review [Project] ");
        assert_eq!(input.selected_range, 18..18);
        assert!(!input.selection_reversed);
        assert_eq!(input.skill_mentions(), bound);

        replace(input, 2..3, "x", window, cx);
        assert!(input.skill_mentions().is_empty());
        input.undo(&Undo, window, cx);
        assert_eq!(input.skill_mentions(), bound);
        assert_eq!(
            input.skill_mentions()[0].intent.expected_consent_generation,
            17
        );
        input.redo(&Redo, window, cx);
        assert_eq!(input.text(), "$rxview [Project] ");
        assert!(input.skill_mentions().is_empty());
    });
}

#[gpui_kit::test]
fn typing_after_undo_discards_redo_identity(cx: &mut TestAppContext) {
    with_editor(cx, |input, window, cx| {
        input.set_text("$rev", cx);
        input.insert_skill_mention(0..4, "$review", intent(), cx);
        input.undo(&Undo, window, cx);
        input.select_all(&super::SelectAll, window, cx);
        input.replace_text_in_range(None, "$review ", window, cx);
        input.redo(&Redo, window, cx);
        assert_eq!(input.text(), "$review ");
        assert!(input.skill_mentions().is_empty());
    });
}

#[gpui_kit::test]
fn copy_and_paste_transfer_only_text_even_inside_the_same_draft(cx: &mut TestAppContext) {
    with_editor(cx, |input, window, cx| {
        let mention = bind(input, cx);
        input.selected_range = mention.range.clone();
        input.copy(&Copy, window, cx);
        let clipboard = cx
            .read_from_clipboard()
            .expect("copy must write clipboard text");
        assert_eq!(clipboard.text().as_deref(), Some("$review [Project]"));
        assert_eq!(clipboard.entries().len(), 1);
        assert!(clipboard.metadata().is_none());
        assert_eq!(input.skill_mentions(), vec![mention.clone()]);
        input.move_to(input.text().len(), cx);
        input.paste(&Paste, window, cx);
        assert_eq!(
            input.text(),
            "before $review [Project]  after$review [Project]"
        );
        assert_eq!(input.skill_mentions(), vec![mention]);

        input.clear(cx);
        input.paste(&Paste, window, cx);
        assert_eq!(input.text(), "$review [Project]");
        assert!(input.skill_mentions().is_empty());
    });
}

#[gpui_kit::test]
fn cut_paste_does_not_reconstruct_identity_but_undo_restores_the_bound_edit(
    cx: &mut TestAppContext,
) {
    with_editor(cx, |input, window, cx| {
        let mention = bind(input, cx);
        input.selected_range = mention.range.clone();
        input.cut(&Cut, window, cx);
        assert_eq!(input.text(), "before   after");
        assert!(input.skill_mentions().is_empty());
        input.paste(&Paste, window, cx);
        assert_eq!(input.text(), "before $review [Project]  after");
        assert!(input.skill_mentions().is_empty());
        input.undo(&Undo, window, cx);
        assert_eq!(input.text(), "before   after");
        assert!(input.skill_mentions().is_empty());
        input.undo(&Undo, window, cx);
        assert_eq!(input.skill_mentions(), vec![mention]);
        input.redo(&Redo, window, cx);
        assert!(input.skill_mentions().is_empty());
        input.redo(&Redo, window, cx);
        assert_eq!(input.text(), "before $review [Project]  after");
        assert!(input.skill_mentions().is_empty());
    });
}

#[gpui_kit::test]
fn ime_replacement_invalidates_binding_and_undo_groups_the_entire_composition(
    cx: &mut TestAppContext,
) {
    with_editor(cx, |input, window, cx| {
        let mention = bind(input, cx);
        input.replace_and_mark_text_in_range(Some(8..10), "审", Some(1..1), window, cx);
        assert!(input.is_composing());
        assert!(input.skill_mentions().is_empty());
        assert_eq!(input.text(), "before $审view [Project]  after");
        assert_eq!(input.selected_range, 11..11);
        assert_eq!(input.marked_text_range(window, cx), Some(8..9));
        input.replace_and_mark_text_in_range(None, "审阅", Some(2..2), window, cx);
        assert_eq!(input.text(), "before $审阅view [Project]  after");
        input.replace_text_in_range(None, "审阅完成", window, cx);
        assert!(!input.is_composing());
        assert!(input.skill_mentions().is_empty());
        assert_eq!(input.text(), "before $审阅完成view [Project]  after");
        input.undo(&Undo, window, cx);
        assert_eq!(input.text(), "before $review [Project]  after");
        assert_eq!(input.skill_mentions(), vec![mention]);
        input.redo(&Redo, window, cx);
        assert_eq!(input.text(), "before $审阅完成view [Project]  after");
        assert!(input.skill_mentions().is_empty());
    });
}

#[gpui_kit::test]
fn ime_before_a_binding_shifts_it_and_blocks_candidate_insertion_until_unmarked(
    cx: &mut TestAppContext,
) {
    with_editor(cx, |input, window, cx| {
        let mut mention = bind(input, cx);
        input.replace_and_mark_text_in_range(Some(0..6), "你好🙂", Some(4..4), window, cx);
        mention.range = 11..28;
        assert_eq!(input.text(), "你好🙂 $review [Project]  after");
        assert_eq!(input.skill_mentions(), vec![mention.clone()]);
        assert_eq!(input.selected_range, 10..10);
        assert!(input.trailing_skill_query().is_none());
        input.insert_skill_mention(0..0, "$other", intent(), cx);
        input.replace_composer_token(0..0, "unexpected", cx);
        input.undo(&Undo, window, cx);
        assert_eq!(input.text(), "你好🙂 $review [Project]  after");
        assert_eq!(input.skill_mentions(), vec![mention]);
        input.unmark_text(window, cx);
        assert!(!input.is_composing());
        input.undo(&Undo, window, cx);
        assert_eq!(input.text(), "before $review [Project]  after");
        assert_eq!(input.skill_mentions()[0].range, 7..24);
    });
}

#[gpui_kit::test]
fn dollar_literals_code_urls_and_paths_do_not_open_skill_queries(cx: &mut TestAppContext) {
    with_editor(cx, |input, _, cx| {
        for text in [
            "$",
            "$5",
            "$５",
            "cost $12.50",
            "word$review",
            "\\$review",
            "$$review",
            "$review next",
            "$review\t",
            "$review\n",
            "$review\u{7f}",
            "$review/name",
            "$review\\name",
            "$review:name",
            "$review`",
            "https://example.com/$review",
            "https://$review",
            "/tmp/$review",
            "./$review",
            "C:\\skills\\$review",
            "` $review",
            "`` $review",
            "```rust\n$review",
            "~~~text\n$review",
            "    $review",
            "\t$review",
            "```\ncode\n~~~\n$review",
        ] {
            input.set_text(text, cx);
            assert_eq!(input.trailing_skill_query(), None, "literal: {text:?}");
        }
    });
}

#[gpui_kit::test]
fn skill_queries_allow_unicode_and_multiline_prose_outside_closed_code(cx: &mut TestAppContext) {
    with_editor(cx, |input, _, cx| {
        for (text, query) in [
            ("$review", "review"),
            ("please $rev", "rev"),
            ("first line\n$review", "review"),
            ("你好🙂\n  $审阅", "审阅"),
            ("`literal` $review", "review"),
            ("``literal ` code`` $review", "review"),
            ("```rust\ncode\n```\n$review", "review"),
            ("~~~text\ncode\n~~~\n$review", "review"),
            ("````\n```\n````\n$review", "review"),
        ] {
            input.set_text(text, cx);
            let start = text.rfind('$').expect("fixture has a query token");
            assert_eq!(
                input.trailing_skill_query(),
                Some((start..text.len(), query.into())),
                "prose: {text:?}"
            );
        }
        input.set_text("前文\n$rev suffix", cx);
        input.move_to("前文\n$rev".len(), cx);
        assert_eq!(input.trailing_skill_query(), Some((7..11, "rev".into())));
    });
}

#[gpui_kit::test]
fn selections_composition_and_existing_mentions_exclude_skill_queries(cx: &mut TestAppContext) {
    with_editor(cx, |input, window, cx| {
        input.set_text("$review", cx);
        input.selected_range = 0..7;
        assert!(input.trailing_skill_query().is_none());
        input.selection_reversed = true;
        assert!(input.trailing_skill_query().is_none());
        input.move_to(7, cx);
        assert_eq!(input.trailing_skill_query(), Some((0..7, "review".into())));
        input.replace_and_mark_text_in_range(Some(0..7), "$review", None, window, cx);
        assert!(input.trailing_skill_query().is_none());
        input.unmark_text(window, cx);
        assert_eq!(input.trailing_skill_query(), Some((0..7, "review".into())));

        input.set_text("", cx);
        input.insert_skill_mention(0..0, "$review", intent(), cx);
        input.move_to(4, cx);
        assert!(input.trailing_skill_query().is_none());
        input.move_to(7, cx);
        assert!(input.trailing_skill_query().is_none());
        input.set_text_with_references(
            "@file $review",
            &[FileReferenceMention {
                range: 0..13,
                path: "file $review".into(),
            }],
            cx,
        );
        assert_eq!(input.reference_mentions().len(), 1);
        assert!(input.trailing_skill_query().is_none());

        let mut masked = TextInput::new(cx, "Secret", true);
        masked.set_text("$review", cx);
        assert!(masked.trailing_skill_query().is_none());
    });
}

#[gpui_kit::test]
fn whole_draft_replacement_and_clear_are_undo_redo_identity_boundaries(cx: &mut TestAppContext) {
    with_editor(cx, |input, window, cx| {
        for boundary in 0..4 {
            bind(input, cx);
            replace(input, 8..9, "x", window, cx);
            input.undo(&Undo, window, cx);
            let expected = match boundary {
                0 => {
                    input.clear(cx);
                    ""
                }
                1 => {
                    input.set_text("$review [Project]", cx);
                    "$review [Project]"
                }
                2 => {
                    input.set_text_with_references(
                        "@file",
                        &[FileReferenceMention {
                            range: 0..5,
                            path: "file".into(),
                        }],
                        cx,
                    );
                    "@file"
                }
                _ => {
                    input.set_text_with_bindings("next message", &[], &[], cx);
                    "next message"
                }
            };
            input.undo(&Undo, window, cx);
            input.redo(&Redo, window, cx);
            assert_eq!(input.text(), expected);
            assert!(input.skill_mentions().is_empty());
            input.move_to(input.text().len(), cx);
            input.replace_text_in_range(None, "!", window, cx);
            input.undo(&Undo, window, cx);
            assert_eq!(input.text(), expected);
            assert!(input.skill_mentions().is_empty());
        }
    });
}

#[gpui_kit::test]
fn restoring_a_draft_keeps_exact_bindings_but_cannot_undo_into_the_previous_message(
    cx: &mut TestAppContext,
) {
    with_editor(cx, |input, window, cx| {
        bind(input, cx);
        let restored = SkillMention {
            range: 6..13,
            text: "$review".into(),
            intent: intent(),
        };
        input.set_text_with_bindings(
            "@file $review",
            &[FileReferenceMention {
                range: 0..5,
                path: "file".into(),
            }],
            std::slice::from_ref(&restored),
            cx,
        );
        input.undo(&Undo, window, cx);
        assert_eq!(input.text(), "@file $review");
        assert_eq!(input.skill_mentions(), vec![restored.clone()]);
        replace(input, 7..8, "x", window, cx);
        assert!(input.skill_mentions().is_empty());
        input.undo(&Undo, window, cx);
        assert_eq!(input.text(), "@file $review");
        assert_eq!(input.skill_mentions(), vec![restored]);
        assert_eq!(input.reference_paths(), vec!["file"]);
        input.undo(&Undo, window, cx);
        assert_eq!(input.text(), "@file $review");
    });
}
