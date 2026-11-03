use super::*;
use gpui_kit::{AppContext, EntityInputHandler, TestAppContext};
use vega_conversation::types::{SkillComposerCandidate, SkillUiScope, ThreadStatus};

fn with_stream(
    cx: &mut TestAppContext,
    run: impl FnOnce(&mut ConversationStream, &mut Window, &mut Context<ConversationStream>),
) {
    let window = cx.add_window(|_, _| gpui_kit::Empty);
    window
        .update(cx, |_, window, cx| {
            let stream = cx.new(|cx| {
                ConversationStream::new(
                    Thread {
                        reasoning_preferences: None,
                        parent_thread_id: None,
                        forked_from_thread_id: None,
                        forked_from_message_id: None,
                        id: "01M6G2WTVY488P8K7GDY1D2XZ6".into(),
                        backend: TaskBackend::Native,
                        project_id: String::new(),
                        title: "Query regression".into(),
                        mode: ThreadMode::Execute,
                        permission_mode: PermissionMode::Confirm,
                        permission_revision: 0,
                        draft_authority_epoch: None,
                        full_access_grant: None,
                        default_permission: None,
                        model: String::new(),
                        status: ThreadStatus::Active,
                        pinned: false,
                        unread: false,
                        created_at: 0,
                        updated_at: 0,
                    },
                    cx,
                )
            });
            stream.update(cx, |stream, cx| run(stream, window, cx));
        })
        .expect("headless composer window remains open");
}

fn query(stream: &mut ConversationStream, text: &str, cx: &mut Context<ConversationStream>) {
    let input = stream.input.clone();
    input.update(cx, |input, cx| input.set_text(text, cx));
    stream.sync_composer_actions(&input, cx);
}

fn projection(stream: &ConversationStream) -> SkillComposerProjection {
    SkillComposerProjection {
        thread_id: stream.thread.id.clone(),
        consent_generation: 19,
        pins: Vec::new(),
        candidates: [
            ("alpha", "Global"),
            ("review", "Project"),
            ("reviewer", "Imported"),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (name, source))| SkillComposerCandidate {
            source_id: format!("source-{index}"),
            name: name.into(),
            description: "Only presentation, not searched".into(),
            source_label: source.into(),
            content_sha256: "a".repeat(64),
            scope: SkillUiScope::VegaGlobal,
            selected: false,
        })
        .collect(),
    }
}

fn install(stream: &mut ConversationStream, cx: &mut Context<ConversationStream>) {
    stream.apply_skill_projection(
        stream.skill_projection_generation(),
        Ok(projection(stream)),
        cx,
    );
}

fn skill_rows(stream: &ConversationStream, cx: &App) -> Vec<usize> {
    stream
        .composer_action_rows(cx)
        .into_iter()
        .filter_map(|row| match row {
            ComposerActionRow::Skill(index) => Some(index),
            _ => None,
        })
        .collect()
}

#[gpui_kit::test]
fn bare_symbols_request_and_display_available_skills(cx: &mut TestAppContext) {
    for symbol in ["$", "/"] {
        with_stream(cx, |stream, _, cx| {
            query(stream, symbol, cx);
            assert_eq!(stream.skill_projection_generation(), 1);
            assert!(stream.actions.skill_projection_requested);
            assert_eq!(stream.skill_mention_query(cx), Some((0..1, String::new())));
            stream.sync_composer_actions(&stream.input.clone(), cx);
            assert_eq!(stream.skill_projection_generation(), 1);
            install(stream, cx);
            assert_eq!(skill_rows(stream, cx), vec![0, 1, 2]);
            assert!(stream.composer_actions_visible(cx));
            let rows = stream.composer_action_rows(cx);
            if symbol == "/" {
                assert!(matches!(
                    rows[0],
                    ComposerActionRow::Command(CommandId::Ask)
                ));
                assert!(matches!(
                    rows[1],
                    ComposerActionRow::Command(CommandId::Plan)
                ));
                assert!(matches!(
                    rows[2],
                    ComposerActionRow::Command(CommandId::Execute)
                ));
                assert!(!rows.iter().any(|row| matches!(
                    row,
                    ComposerActionRow::Command(CommandId::Goal | CommandId::Loop | CommandId::Fork)
                )));
                assert!(
                    rows.iter()
                        .skip_while(|row| !matches!(row, ComposerActionRow::Skill(_)))
                        .all(|row| matches!(row, ComposerActionRow::Skill(_)))
                );
            } else {
                assert!(
                    rows.iter()
                        .all(|row| matches!(row, ComposerActionRow::Skill(_)))
                );
            }
        });
    }
}

#[gpui_kit::test]
fn typing_filters_and_backspacing_restores_both_lists(cx: &mut TestAppContext) {
    with_stream(cx, |stream, _, cx| {
        query(stream, "$", cx);
        install(stream, cx);
        for symbol in ["$", "/"] {
            for (suffix, expected) in [
                ("", vec![0, 1, 2]),
                ("REV", vec![1, 2]),
                ("roject", vec![1]),
                ("missing", vec![]),
                ("", vec![0, 1, 2]),
            ] {
                query(stream, &format!("{symbol}{suffix}"), cx);
                assert_eq!(skill_rows(stream, cx), expected);
            }
        }
        assert_eq!(stream.skill_projection_generation(), 1);
    });
}

#[gpui_kit::test]
fn accepting_bare_symbol_inserts_exact_binding_without_submission(cx: &mut TestAppContext) {
    for (text, prefix) in [("$", ""), ("/", ""), ("正文 $", "正文 ")] {
        with_stream(cx, |stream, _, cx| {
            query(stream, text, cx);
            install(stream, cx);
            let candidate = projection(stream).candidates.remove(1);
            let entries = stream.entries.len();
            stream.choose_skill_candidate(&candidate, cx);
            assert_eq!(
                stream.input.read(cx).text(),
                format!("{prefix}$review [Project] ")
            );
            let mentions = stream.input.read(cx).skill_mentions();
            assert_eq!(mentions.len(), 1);
            assert_eq!(
                mentions[0].range,
                prefix.len()..prefix.len() + "$review [Project]".len()
            );
            assert_eq!(mentions[0].intent.source_id, "source-1");
            assert_eq!(mentions[0].intent.expected_consent_generation, 19);
            assert_eq!(stream.entries.len(), entries);
            assert!(!stream.composer_submit_pending);
            assert!(!stream.skill_mutation_pending);
        });
    }
}

#[gpui_kit::test]
fn empty_projection_preserves_only_default_slash_commands(cx: &mut TestAppContext) {
    with_stream(cx, |stream, _, cx| {
        query(stream, "$", cx);
        let mut empty = projection(stream);
        empty.candidates.clear();
        stream.apply_skill_projection(stream.skill_projection_generation(), Ok(empty), cx);
        assert!(!stream.composer_actions_visible(cx));
        query(stream, "/", cx);
        assert!(stream.composer_actions_visible(cx));
        assert!(skill_rows(stream, cx).is_empty());
        assert!(
            stream
                .composer_action_rows(cx)
                .iter()
                .all(|row| matches!(row, ComposerActionRow::Command(_)))
        );
    });
}

#[gpui_kit::test]
fn stale_projection_and_codex_do_not_expose_skills(cx: &mut TestAppContext) {
    with_stream(cx, |stream, _, cx| {
        query(stream, "$", cx);
        let old = stream.skill_projection_generation();
        stream.suspend_skill_projection(cx);
        query(stream, "/", cx);
        stream.apply_skill_projection(old, Ok(projection(stream)), cx);
        assert!(stream.skill_projection.is_none());
        assert!(stream.actions.skill_projection_requested);
        install(stream, cx);
        for symbol in ["$", "/"] {
            stream.thread.backend = TaskBackend::Codex;
            query(stream, symbol, cx);
            assert!(!stream.composer_actions_visible(cx));
            assert!(stream.skill_mention_query(cx).is_none());
        }
    });
}

#[gpui_kit::test]
fn bare_dollar_keeps_numeric_code_path_selection_and_ime_guards(cx: &mut TestAppContext) {
    with_stream(cx, |stream, window, cx| {
        for text in [
            "$5",
            "`$",
            "```\n$",
            "~~~\n$",
            "path/$",
            "https://example/$",
            "word$",
            "    $",
            "$ ",
        ] {
            query(stream, text, cx);
            assert!(
                stream.input.read(cx).trailing_skill_query().is_none(),
                "{text}"
            );
            assert!(stream.actions.skill_query.is_none(), "{text}");
        }
        query(stream, "$", cx);
        stream.input.update(cx, |input, cx| {
            input.replace_and_mark_text_in_range(Some(0..1), "$", Some(1..1), window, cx);
            assert!(input.trailing_skill_query().is_none());
            input.unmark_text(window, cx);
            assert_eq!(input.trailing_skill_query(), Some((0..1, String::new())));
            input.replace_and_mark_text_in_range(Some(0..1), "$", Some(0..1), window, cx);
            input.unmark_text(window, cx);
            assert!(input.trailing_skill_query().is_none());
        });
    });
}
