use super::*;
use crate::automation_controller::AutomationGlobal;
use vega_conversation::automation;
use vega_runtime::{
    MockProvider, ProviderEvent, RuntimeUserQuestionHook, RuntimeUserQuestionRequest,
    RuntimeUserQuestionResponse, ScriptStep, StopReason, UserQuestionInput,
};
use vega_ui::conversation_stream::AutomationRequested;

fn final_round() -> Vec<ScriptStep> {
    vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("Verified the task".into()),
        ProviderEvent::Usage {
            input: 10,
            output: 2,
            cache_read: None,
            cache_write: None,
        },
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]
}

fn report_round(id: &str, complete: bool) -> Vec<ScriptStep> {
    vec![ScriptStep::events(vec![
        ProviderEvent::ToolUse {
            id: id.into(), name: "update_goal".into(),
            input_json: if complete {
                r#"{"status":"complete","reason":"All requested checks finished"}"#
            } else {
                r#"{"status":"active","progress":"progress","reason":"First check completed; second remains"}"#
            }.into(),
        },
        ProviderEvent::Usage { input: 10, output: 2, cache_read: None, cache_write: None },
        ProviderEvent::Done { stop_reason: StopReason::ToolUse },
    ])]
}

fn question_input() -> UserQuestionInput {
    UserQuestionInput::from_json(r#"{"questions":[{"id":"approach","header":"Approach","question":"Which approach?","options":[{"label":"A","description":"Keep it"},{"label":"B","description":"Change it"}]}]}"#).unwrap()
}

fn question_round() -> Vec<ScriptStep> {
    vec![ScriptStep::events(vec![
        ProviderEvent::ToolUse {
            id: "loop-question".into(),
            name: vega_runtime::USER_QUESTION_TOOL.into(),
            input_json: question_input().to_json().unwrap(),
        },
        ProviderEvent::Usage {
            input: 10,
            output: 2,
            cache_read: None,
            cache_write: None,
        },
        ProviderEvent::Done {
            stop_reason: StopReason::ToolUse,
        },
    ])]
}

fn submit(f: &Fixture, text: &str, cx: &mut gpui_kit::TestAppContext) {
    edit(f, text, cx);
    cx.simulate_keystrokes(f.window.into(), "cmd-enter");
}

fn control(
    f: &Fixture,
    kind: AutomationKind,
    action: AutomationControl,
    cx: &mut gpui_kit::TestAppContext,
) {
    f.root.update(cx, |root, cx| {
        root.automation_requested(
            f.stream.clone(),
            &AutomationRequested {
                thread_id: f.thread.id.clone(),
                command: AutomationCommand::Control(kind, action),
            },
            cx,
        )
    });
}

fn refresh(f: &Fixture, cx: &mut gpui_kit::TestAppContext) {
    f.root.update(cx, |root, cx| {
        root.bind_automation(f.stream.clone(), f.thread.id.clone(), cx)
    });
}

fn idle(f: &Fixture, cx: &mut gpui_kit::TestAppContext) -> bool {
    f.root.read_with(cx, |root, _| {
        root.agent_controller.active.is_empty() && root.automation_controller.pending.is_empty()
    }) && cx.update(|cx| {
        !cx.global::<AutomationGlobal>()
            .mutations
            .contains_key(&f.thread.id)
    })
}

#[gpui_kit::test]
async fn composer_goal_persists_runs_continues_and_completes_through_native_worker(
    cx: &mut gpui_kit::TestAppContext,
) {
    let provider = Arc::new(MockProvider::new_rounds(vec![
        report_round("first", false),
        final_round(),
        report_round("second", true),
        final_round(),
    ]));
    let f = fixture(cx, provider.clone());
    submit(&f, "/goal --budget 100 verify two checks", cx);
    pump_test_app(cx, |cx| provider.requests().len() == 2 && idle(&f, cx));
    assert_eq!(
        automation::load(&f.store, &f.thread.id).unwrap()[0].status,
        AutomationStatus::Active
    );
    f.root.update(cx, |root, _| {
        root.automation_controller.now_override = Some(automation::now_ms() + 2000)
    });
    refresh(&f, cx);
    pump_test_app_with_diagnostics(
        cx,
        |cx| {
            automation::load(&f.store, &f.thread.id)
                .unwrap()
                .first()
                .is_some_and(|task| task.status == AutomationStatus::Complete)
                && idle(&f, cx)
        },
        |cx| {
            let tasks = automation::load(&f.store, &f.thread.id).map(|tasks| {
                tasks
                    .into_iter()
                    .map(|task| (task.status, task.used_tokens, task.next_due_ms))
                    .collect::<Vec<_>>()
            });
            let (active, polling) = f.root.read_with(cx, |root, _| {
                (
                    root.agent_controller.active.contains_key(&f.thread.id),
                    root.automation_controller.pending.contains(&f.thread.id),
                )
            });
            let mutation = cx.update(|cx| {
                cx.global::<AutomationGlobal>()
                    .mutations
                    .contains_key(&f.thread.id)
            });
            let claim = vega_store::automation::active_run(f.store.conn(), &f.thread.id)
                .map(|run| run.is_some());
            format!(
                "requests={}, tasks={tasks:?}, active={active}, polling={polling}, mutation={mutation}, claim={claim:?}",
                provider.requests().len()
            )
        },
    );
    assert_eq!(provider.requests().len(), 4);
    assert!(draft(&f, cx).is_empty());
    let task = automation::load(&f.store, &f.thread.id).unwrap().remove(0);
    assert_eq!(task.status, AutomationStatus::Complete);
    assert_eq!(task.used_tokens, 48);
    assert!(task.usage_complete);
    assert!(
        vega_store::automation::active_run(f.store.conn(), &f.thread.id)
            .unwrap()
            .is_none()
    );
    assert!(cx.update(|cx| {
        !cx.global::<AutomationGlobal>()
            .runs
            .contains_key(&f.thread.id)
    }));
    refresh(&f, cx);
    cx.executor().advance_clock(Duration::from_secs(3));
    cx.run_until_parked();
    assert_eq!(provider.requests().len(), 4);
}

#[gpui_kit::test]
async fn invalid_commands_preserve_draft_without_provider_or_storage(
    cx: &mut gpui_kit::TestAppContext,
) {
    let provider = Arc::new(MockProvider::new(Vec::new()));
    let f = fixture(cx, provider.clone());
    submit(&f, "/goal --budget 0 check", cx);
    assert_eq!(draft(&f, cx), "/goal --budget 0 check");
    assert!(f.stream.read_with(cx, |stream, _| {
        stream.automation_projection().error.is_some()
    }));
    assert!(provider.requests().is_empty());
    assert!(automation::load(&f.store, &f.thread.id).unwrap().is_empty());
    let count: i64 = f
        .store
        .conn()
        .query_row("SELECT count(*) FROM messages", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[gpui_kit::test]
async fn stop_revokes_running_loop_and_resume_is_explicit(cx: &mut gpui_kit::TestAppContext) {
    cx.executor().allow_parking();
    let provider = Arc::new(MockProvider::new_rounds(vec![
        vec![ScriptStep::delay(Duration::from_secs(30))],
        final_round(),
        question_round(),
        final_round(),
    ]));
    let f = fixture(cx, provider.clone());
    submit(&f, "/loop 1m check current status", cx);
    pump_test_app(cx, |_| provider.requests().len() == 1);
    f.root.update(cx, |root, cx| {
        root.stop_composer(
            f.stream.clone(),
            &vega_ui::conversation_stream::ComposerStopRequested {
                thread_id: f.thread.id.clone(),
            },
            cx,
        )
    });
    pump_test_app(cx, |cx| {
        idle(&f, cx)
            && automation::load(&f.store, &f.thread.id).unwrap()[0].status
                == AutomationStatus::Paused
    });
    assert!(
        vega_store::automation::active_run(f.store.conn(), &f.thread.id)
            .unwrap()
            .is_none()
    );
    refresh(&f, cx);
    cx.run_until_parked();
    assert_eq!(provider.requests().len(), 1);
    control(&f, AutomationKind::Loop, AutomationControl::Resume, cx);
    pump_test_app(cx, |cx| provider.requests().len() == 2 && idle(&f, cx));
    let task = automation::load(&f.store, &f.thread.id).unwrap().remove(0);
    assert_eq!(task.status, AutomationStatus::Active);
    assert!(task.next_due_ms > automation::now_ms());
    control(&f, AutomationKind::Loop, AutomationControl::Clear, cx);
    pump_test_app(cx, |cx| {
        idle(&f, cx) && automation::load(&f.store, &f.thread.id).unwrap().is_empty()
    });
    submit(&f, "/loop 1m ask before continuing", cx);
    pump_test_app(cx, |cx| {
        provider.requests().len() == 3
            && f.stream
                .read_with(cx, |stream, _| stream.has_pending_user_question())
    });
    refresh(&f, cx);
    cx.run_until_parked();
    assert_eq!(provider.requests().len(), 3);
    assert_eq!(
        automation::load(&f.store, &f.thread.id).unwrap()[0].status,
        AutomationStatus::Active
    );
    f.root.update(cx, |root, cx| {
        root.stop_composer(
            f.stream.clone(),
            &vega_ui::conversation_stream::ComposerStopRequested {
                thread_id: f.thread.id.clone(),
            },
            cx,
        )
    });
    pump_test_app(cx, |cx| {
        idle(&f, cx)
            && !f
                .stream
                .read_with(cx, |stream, _| stream.has_pending_user_question())
            && automation::load(&f.store, &f.thread.id).unwrap()[0].status
                == AutomationStatus::Paused
    });
    let question_status: String = f
        .store
        .conn()
        .query_row(
            "SELECT status FROM tool_calls WHERE id='loop-question'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(question_status, "cancelled");
    assert_eq!(provider.requests().len(), 3);
    control(&f, AutomationKind::Loop, AutomationControl::Resume, cx);
    pump_test_app(cx, |cx| provider.requests().len() == 4 && idle(&f, cx));
    control(&f, AutomationKind::Loop, AutomationControl::Clear, cx);
    pump_test_app(cx, |cx| {
        idle(&f, cx) && automation::load(&f.store, &f.thread.id).unwrap().is_empty()
    });
}

#[gpui_kit::test]
async fn archive_and_delete_cancel_the_active_automatic_worker(cx: &mut gpui_kit::TestAppContext) {
    let provider = Arc::new(MockProvider::new(vec![ScriptStep::delay(
        Duration::from_secs(30),
    )]));
    let f = fixture(cx, provider.clone());
    submit(&f, "/loop check until cancelled", cx);
    pump_test_app(cx, |_| provider.requests().len() == 1);
    let cancel = cx.update(|cx| {
        cx.global::<AutomationGlobal>()
            .runs
            .get(&f.thread.id)
            .unwrap()
            .cancel
            .clone()
    });
    vega_conversation::threads::set_thread_status(&f.store, &f.thread.id, ThreadStatus::Archived)
        .unwrap();
    refresh(&f, cx);
    pump_test_app(cx, |cx| cancel.is_cancelled() && idle(&f, cx));
    assert_eq!(
        automation::load(&f.store, &f.thread.id).unwrap()[0].status,
        AutomationStatus::Paused
    );
    assert!(
        !f.stream
            .read_with(cx, |stream, cx| stream.automation_can_run(cx))
    );
    let archived = vega_conversation::threads::open_thread(&f.store, &f.thread.id).unwrap();
    assert_eq!(archived.status, ThreadStatus::Archived);
    let other_root = cx.new(VegaWindow::new);
    let other_stream = cx.new(|cx| ConversationStream::new(archived, cx));
    other_root.update(cx, |root, cx| {
        root.bind_automation(other_stream.clone(), f.thread.id.clone(), cx)
    });
    assert!(!other_stream.read_with(cx, |stream, cx| stream.automation_can_run(cx)));
    vega_conversation::threads::set_thread_status(&f.store, &f.thread.id, ThreadStatus::Active)
        .unwrap();
    let reopened = vega_conversation::threads::visit_thread(&f.store, &f.thread.id).unwrap();
    assert_eq!(reopened.status, ThreadStatus::Active);
    cx.update(|cx| {
        cx.set_global(vega_ui::sidebar::SelectedProject(
            reopened.project_binding().map(str::to_owned),
        ));
        cx.set_global(OpenedThread(Some(reopened)));
        cx.refresh_windows();
    });
    pump_test_app(cx, |cx| {
        f.stream
            .read_with(cx, |stream, cx| stream.automation_can_run(cx))
            && other_stream.read_with(cx, |stream, cx| stream.automation_can_run(cx))
    });
    assert!(cx.update(|cx| {
        cx.global::<AutomationGlobal>()
            .streams_ready(&f.thread.id, cx)
    }));
    assert!(f.root.read_with(cx, |root, _| {
        root.stream_view
            .as_ref()
            .is_some_and(|(id, stream)| id == &f.thread.id && stream == &f.stream)
    }));
    assert_eq!(
        automation::load(&f.store, &f.thread.id).unwrap()[0].status,
        AutomationStatus::Paused
    );
    assert_eq!(provider.requests().len(), 1);
    control(&f, AutomationKind::Loop, AutomationControl::Resume, cx);
    pump_test_app(cx, |_| provider.requests().len() == 2);
    let cancel = cx.update(|cx| {
        cx.global::<AutomationGlobal>()
            .runs
            .get(&f.thread.id)
            .unwrap()
            .cancel
            .clone()
    });
    vega_conversation::threads::delete_thread(&f.store, &f.thread.id).unwrap();
    refresh(&f, cx);
    pump_test_app(cx, |cx| cancel.is_cancelled() && idle(&f, cx));
    assert!(automation::load(&f.store, &f.thread.id).unwrap().is_empty());
    assert!(
        vega_store::automation::active_run(f.store.conn(), &f.thread.id)
            .unwrap()
            .is_none()
    );
}

#[gpui_kit::test]
async fn failed_start_releases_claim_and_restart_recovery_gate_prevents_dispatch(
    cx: &mut gpui_kit::TestAppContext,
) {
    let provider = Arc::new(MockProvider::new(Vec::new()));
    let f = fixture(cx, provider.clone());
    cx.update(|cx| cx.update_global::<AutomationGlobal, _>(|global, _| global.ready = false));
    submit(&f, "/loop 1m wait for readiness", cx);
    pump_test_app(cx, |cx| {
        idle(&f, cx) && !automation::load(&f.store, &f.thread.id).unwrap().is_empty()
    });
    refresh(&f, cx);
    pump_test_app(cx, |cx| {
        f.stream.read_with(cx, |stream, _| {
            stream.automation_projection().error.is_some()
        })
    });
    assert!(provider.requests().is_empty());
    assert!(
        vega_store::automation::active_run(f.store.conn(), &f.thread.id)
            .unwrap()
            .is_none()
    );
    f.root.update(cx, |root, _| {
        root.model_selection_config_override = Some(f._data.path().join("missing.toml"));
        root.agent_provider_override = None;
    });
    cx.update(|cx| cx.update_global::<AutomationGlobal, _>(|global, _| global.ready = true));
    refresh(&f, cx);
    pump_test_app(cx, |cx| {
        idle(&f, cx)
            && automation::load(&f.store, &f.thread.id).unwrap()[0].status
                == AutomationStatus::Paused
    });
    assert!(provider.requests().is_empty());
    assert!(
        vega_store::automation::active_run(f.store.conn(), &f.thread.id)
            .unwrap()
            .is_none()
    );
}

#[gpui_kit::test]
async fn shared_stream_guard_yields_to_other_window_draft_and_preflight(
    cx: &mut gpui_kit::TestAppContext,
) {
    let provider = Arc::new(MockProvider::new_rounds(vec![final_round()]));
    let f = fixture(cx, provider.clone());
    let other_root = cx.new(VegaWindow::new);
    other_root.update(cx, |root, _| {
        root.model_selection_config_override = Some(f._data.path().join("config.toml"));
        root.agent_provider_override = Some(with_auxiliary_title_fixture(provider.clone()));
    });
    let retained = other_root.clone();
    let other_window = cx.update(|cx| {
        cx.open_window(Default::default(), move |_, _| retained)
            .unwrap()
    });
    pump_test_app(cx, |cx| {
        other_root.read_with(cx, |root, _| {
            root.stream_view.is_some()
                && !root.model_catalog_loading
                && root.agent_controller.queue_operations.is_empty()
        })
    });
    let other = other_root.read_with(cx, |root, _| root.stream_view.as_ref().unwrap().1.clone());
    other.update(cx, |stream, cx| {
        stream
            .composer_input()
            .update(cx, |input, cx| input.set_text("pending user draft", cx))
    });
    submit(&f, "/loop 1m check", cx);
    pump_test_app(cx, |cx| {
        idle(&f, cx) && !automation::load(&f.store, &f.thread.id).unwrap().is_empty()
    });
    assert!(provider.requests().is_empty());
    other.update(cx, |stream, cx| {
        stream
            .composer_input()
            .update(cx, |input, cx| input.set_text("", cx));
        stream.set_trusted_action_busy(true, cx);
    });
    refresh(&f, cx);
    pump_test_app(cx, |cx| idle(&f, cx));
    assert!(provider.requests().is_empty());
    other.update(cx, |stream, cx| stream.set_trusted_action_busy(false, cx));
    let question_queue = other.read_with(cx, |stream, _| stream.user_question_queue());
    let response = question_queue.request(
        RuntimeUserQuestionRequest {
            call_id: "peer-question".into(),
            input: question_input(),
        },
        tokio_util::sync::CancellationToken::new(),
    );
    refresh(&f, cx);
    pump_test_app(cx, |cx| idle(&f, cx));
    assert!(provider.requests().is_empty());
    assert!(other.read_with(cx, |stream, _| stream.has_pending_user_question()));
    other.update(cx, |stream, cx| stream.cancel_user_questions(cx));
    assert!(matches!(
        response.await,
        RuntimeUserQuestionResponse::Cancelled
    ));
    refresh(&f, cx);
    pump_test_app(cx, |cx| provider.requests().len() == 1 && idle(&f, cx));
    control(&f, AutomationKind::Loop, AutomationControl::Clear, cx);
    pump_test_app(cx, |cx| idle(&f, cx));
    other_window
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();
}

#[gpui_kit::test]
async fn failed_pause_stays_revoked_and_other_kind_first_persists_the_pending_pause(
    cx: &mut gpui_kit::TestAppContext,
) {
    for existing_loop in [false, true] {
        let provider = Arc::new(MockProvider::new_rounds(vec![final_round()]));
        let f = fixture(cx, provider.clone());
        edit(&f, "hold automatic start", cx);
        automation::command(
            &f.store,
            &f.thread.id,
            &automation::parse_command("/goal old objective")
                .unwrap()
                .unwrap(),
            automation::now_ms(),
        )
        .unwrap();
        if existing_loop {
            automation::command(
                &f.store,
                &f.thread.id,
                &automation::parse_command("/loop 1m existing loop")
                    .unwrap()
                    .unwrap(),
                automation::now_ms(),
            )
            .unwrap();
        }
        refresh(&f, cx);
        pump_test_app(cx, |cx| idle(&f, cx));
        f.store.conn().execute_batch("CREATE TRIGGER fail_pause BEFORE UPDATE ON automation_tasks BEGIN SELECT RAISE(ABORT,'pause unavailable'); END;").unwrap();
        control(&f, AutomationKind::Goal, AutomationControl::Pause, cx);
        pump_test_app(cx, |cx| {
            idle(&f, cx)
                && f.stream.read_with(cx, |stream, _| {
                    stream.automation_projection().error.is_some()
                })
        });
        assert_eq!(
            automation::load(&f.store, &f.thread.id).unwrap()[0].status,
            AutomationStatus::Active
        );
        edit(&f, "", cx);
        refresh(&f, cx);
        pump_test_app(cx, |cx| idle(&f, cx));
        assert!(provider.requests().is_empty());
        f.store
            .conn()
            .execute_batch("DROP TRIGGER fail_pause;")
            .unwrap();
        if existing_loop {
            refresh(&f, cx);
        } else {
            submit(&f, "/loop 1m new loop", cx);
        }
        pump_test_app(cx, |cx| provider.requests().len() == 1 && idle(&f, cx));
        let tasks = automation::load(&f.store, &f.thread.id).unwrap();
        assert_eq!(
            tasks
                .iter()
                .find(|task| task.kind == AutomationKind::Goal)
                .unwrap()
                .status,
            AutomationStatus::Paused
        );
        let goal = tasks
            .iter()
            .find(|task| task.kind == AutomationKind::Goal)
            .unwrap();
        assert_eq!(goal.used_tokens, 0);
        assert_eq!(goal.elapsed_ms, 0);
        assert!(
            !provider.requests()[0]
                .tools
                .iter()
                .any(|tool| tool.name == "get_goal")
        );
        assert!(
            !provider.requests()[0]
                .messages
                .iter()
                .any(|message| message.content.contains("old objective"))
        );
        control(&f, AutomationKind::Loop, AutomationControl::Clear, cx);
        pump_test_app(cx, |cx| idle(&f, cx));
        f.window
            .update(cx, |_, window, _| window.remove_window())
            .unwrap();
    }
}

#[gpui_kit::test]
async fn pending_claim_is_released_after_window_disappears_and_another_owner_can_run(
    cx: &mut gpui_kit::TestAppContext,
) {
    let provider = Arc::new(MockProvider::new_rounds(vec![final_round()]));
    let f = fixture(cx, provider.clone());
    let claim = automation::command(
        &f.store,
        &f.thread.id,
        &automation::parse_command("/loop 1m check")
            .unwrap()
            .unwrap(),
        automation::now_ms(),
    )
    .unwrap();
    assert_eq!(claim.len(), 1);
    refresh(&f, cx);
    let window = f.window;
    let store_path = f.store.database_path().unwrap().to_path_buf();
    let thread = f.thread.clone();
    let config = f._data.path().join("config.toml");
    window
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();
    drop(f.root);
    pump_test_app(cx, |_| {
        vega_store::automation::active_run(f.store.conn(), &thread.id)
            .unwrap()
            .is_none()
    });
    let other = cx.new(VegaWindow::new);
    other.update(cx, |root, _| {
        root.model_selection_config_override = Some(config);
        root.agent_provider_override = Some(with_auxiliary_title_fixture(provider.clone()));
    });
    let retained = other.clone();
    let other_window = cx.update(|cx| {
        cx.open_window(Default::default(), move |_, _| retained)
            .unwrap()
    });
    pump_test_app(cx, |cx| {
        other.read_with(cx, |root, _| {
            root.stream_view.is_some() && !root.model_catalog_loading
        })
    });
    let other_stream = other.read_with(cx, |root, _| root.stream_view.as_ref().unwrap().1.clone());
    other.update(cx, |root, cx| {
        root.bind_automation(other_stream.clone(), thread.id.clone(), cx)
    });
    pump_test_app(cx, |cx| {
        provider.requests().len() == 1
            && other.read_with(cx, |root, _| root.agent_controller.active.is_empty())
    });
    assert!(
        vega_store::automation::active_run(Store::open(store_path).unwrap().conn(), &thread.id)
            .unwrap()
            .is_none()
    );
    other_window
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();
}

#[gpui_kit::test]
async fn successful_durable_finish_does_not_cancel_pending_ui_terminal(
    cx: &mut gpui_kit::TestAppContext,
) {
    let provider = Arc::new(MockProvider::new(Vec::new()));
    let f = fixture(cx, provider.clone());
    edit(&f, "keep scheduler idle", cx);
    automation::command(
        &f.store,
        &f.thread.id,
        &automation::parse_command("/goal check").unwrap().unwrap(),
        automation::now_ms(),
    )
    .unwrap();
    let claim = automation::claim(
        &f.store,
        &f.thread.id,
        Some(AutomationKind::Goal),
        automation::now_ms(),
    )
    .unwrap()
    .unwrap();
    assert!(
        automation::activate_claim(&f.store, &claim)
            .unwrap()
            .is_some()
    );
    let cancel = tokio_util::sync::CancellationToken::new();
    let owner = f
        .root
        .read_with(cx, |root, _| Arc::downgrade(&root.window_alive));
    cx.update(|cx| {
        cx.update_global::<AutomationGlobal, _>(|global, _| {
            global.runs.insert(
                f.thread.id.clone(),
                crate::automation_controller::AutomationRunOwner {
                    started_at: None,
                    generation: 10,
                    owner,
                    claim: Some(claim.clone()),
                    cancel: cancel.clone(),
                },
            );
        })
    });
    automation::finish(&f.store, &claim, true, true, 1, automation::now_ms()).unwrap();
    refresh(&f, cx);
    pump_test_app(cx, |cx| idle(&f, cx));
    assert!(!cancel.is_cancelled());
    assert!(provider.requests().is_empty());
    let owner = f.root.read_with(cx, |root, _| root.window_alive.clone());
    cx.update(|cx| {
        cx.update_global::<AutomationGlobal, _>(|global, _| {
            global.finish_run(&f.thread.id, 10, &owner)
        })
    });
}

#[gpui_kit::test]
async fn new_task_command_materializes_the_existing_draft_identity_before_running(
    cx: &mut gpui_kit::TestAppContext,
) {
    let provider = Arc::new(MockProvider::new_rounds(vec![
        report_round("draft-complete", true),
        final_round(),
    ]));
    let mut f = fixture(cx, provider.clone());
    click(&f, "sidebar-new-task", cx);
    pump_test_app(cx, |cx| {
        f.root.read_with(cx, |root, _| {
            root.draft.as_ref().is_some_and(|draft| {
                root.stream_view
                    .as_ref()
                    .is_some_and(|(id, _)| id == &draft.id)
            })
        })
    });
    let (thread, stream) = f.root.read_with(cx, |root, _| {
        (
            root.draft.as_ref().unwrap().clone(),
            root.stream_view.as_ref().unwrap().1.clone(),
        )
    });
    assert!(
        vega_store::threads::find(f.store.conn(), &thread.id)
            .unwrap()
            .is_none()
    );
    f.thread = thread;
    f.stream = stream;
    submit(&f, "/goal check the new task", cx);
    pump_test_app(cx, |cx| provider.requests().len() == 2 && idle(&f, cx));
    assert!(
        vega_store::threads::find(f.store.conn(), &f.thread.id)
            .unwrap()
            .is_some()
    );
    assert!(f.root.read_with(cx, |root, _| root.draft.is_none()));
    assert_eq!(
        automation::load(&f.store, &f.thread.id).unwrap()[0].instruction,
        "check the new task"
    );
    assert!(draft(&f, cx).is_empty());
    control(&f, AutomationKind::Goal, AutomationControl::Clear, cx);
    pump_test_app(cx, |cx| idle(&f, cx));
}

fn open_goal_editor(
    f: &Fixture,
    cx: &mut gpui_kit::TestAppContext,
) -> Entity<vega_ui::automation_editor::AutomationEditor> {
    cx.run_until_parked();
    let mut visual = gpui_kit::VisualTestContext::from_window(f.window.into(), cx);
    let bounds = visual.debug_bounds("automation-goal-details").unwrap();
    visual.simulate_click(bounds.center(), Default::default());
    visual.run_until_parked();
    let task = automation::load(&f.store, &f.thread.id)
        .unwrap()
        .into_iter()
        .find(|task| task.kind == AutomationKind::Goal)
        .unwrap();
    f.root.read_with(cx, |root, _| {
        root.automation_editors[&vega_ui::automation_editor::AutomationEditorKey::for_task(&task)]
            .clone()
    })
}

fn edit_goal(
    view: &Entity<vega_ui::automation_editor::AutomationEditor>,
    instruction: &str,
    cx: &mut gpui_kit::TestAppContext,
) {
    view.update(cx, |view, cx| {
        view.input()
            .update(cx, |input, cx| input.set_text(instruction, cx))
    });
    cx.run_until_parked();
}

#[gpui_kit::test]
async fn workspace_editor_saves_paused_goal_retains_drafts_and_rejects_credentials(
    cx: &mut gpui_kit::TestAppContext,
) {
    let provider = Arc::new(MockProvider::new(Vec::new()));
    let f = fixture(cx, provider.clone());
    automation::command(
        &f.store,
        &f.thread.id,
        &automation::parse_command("/goal --budget 100 original")
            .unwrap()
            .unwrap(),
        automation::now_ms(),
    )
    .unwrap();
    automation::command(
        &f.store,
        &f.thread.id,
        &AutomationCommand::Control(AutomationKind::Goal, AutomationControl::Pause),
        automation::now_ms(),
    )
    .unwrap();
    refresh(&f, cx);
    pump_test_app(cx, |cx| idle(&f, cx));
    let editor = open_goal_editor(&f, cx);
    edit_goal(&editor, "first edit\nsecond line", cx);
    let original_pane = gpui_kit::VisualTestContext::from_window(f.window.into(), cx)
        .debug_bounds("right-workspace-pane")
        .unwrap();
    cx.update(|cx| cx.set_global(vega_ui::sidebar::SidebarExplicitlyShown(true)));
    let mut visual = gpui_kit::VisualTestContext::from_window(f.window.into(), cx);
    visual.simulate_resize(gpui_kit::size(gpui_kit::px(800.), gpui_kit::px(600.)));
    visual.run_until_parked();
    f.window
        .update(cx, |root, window, cx| {
            assert_eq!(
                window.viewport_size(),
                gpui_kit::size(gpui_kit::px(800.), gpui_kit::px(600.))
            );
            assert!(!root.auto_collapsed(window, cx));
        })
        .unwrap();
    let main = visual.debug_bounds("main-content-panel").unwrap();
    let expanded = visual.debug_bounds("automation-editor").unwrap();
    let save = visual.debug_bounds("automation-editor-save").unwrap();
    assert!(main.size.width < gpui_kit::px(610.));
    assert_eq!(expanded.left(), main.left());
    assert_eq!(expanded.size.width, main.size.width);
    assert!(expanded.bottom() <= main.bottom());
    assert!(save.left() >= expanded.left() && save.right() <= expanded.right());
    assert!(save.top() >= expanded.top() && save.bottom() <= expanded.bottom());
    assert!(visual.debug_bounds("right-workspace-pane").is_none());
    assert!(visual.debug_bounds("composer-shell").is_none());
    assert!(
        !f.stream
            .read_with(cx, |stream, cx| stream.automation_can_run(cx))
    );
    cx.simulate_keystrokes(f.window.into(), "escape");
    cx.run_until_parked();
    assert!(
        f.stream
            .read_with(cx, |stream, cx| stream.automation_can_run(cx))
    );
    let visual = gpui_kit::VisualTestContext::from_window(f.window.into(), cx);
    visual.simulate_resize(gpui_kit::size(gpui_kit::px(1200.), gpui_kit::px(900.)));
    visual.run_until_parked();
    f.window
        .update(cx, |_, window, _| {
            assert_eq!(
                window.viewport_size(),
                gpui_kit::size(gpui_kit::px(1200.), gpui_kit::px(900.))
            )
        })
        .unwrap();
    cx.run_until_parked();
    f.window
        .update(cx, |_, window, cx| {
            assert!(
                f.stream
                    .read(cx)
                    .composer_input()
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(window)
            )
        })
        .unwrap();
    let reopened = open_goal_editor(&f, cx);
    assert_eq!(
        gpui_kit::VisualTestContext::from_window(f.window.into(), cx)
            .debug_bounds("right-workspace-pane")
            .unwrap()
            .size
            .width,
        original_pane.size.width
    );
    let mut visual = gpui_kit::VisualTestContext::from_window(f.window.into(), cx);
    let main = visual.debug_bounds("main-content-panel").unwrap();
    let restored = visual.debug_bounds("automation-editor").unwrap();
    assert!(main.size.width >= gpui_kit::px(610.));
    assert_eq!(restored.size.width, original_pane.size.width);
    assert!(restored.size.width < main.size.width);
    assert!(visual.debug_bounds("composer-shell").is_some());
    assert_eq!(editor, reopened);
    assert_eq!(
        editor.read_with(cx, |view, cx| view.instruction(cx).to_string()),
        "first edit\nsecond line"
    );
    cx.simulate_keystrokes(f.window.into(), "cmd-s");
    pump_test_app(cx, |cx| {
        !editor.read_with(cx, |view, _| view.is_saving()) && idle(&f, cx)
    });
    let saved = automation::load(&f.store, &f.thread.id).unwrap().remove(0);
    assert_eq!(saved.instruction, "first edit\nsecond line");
    assert_eq!(saved.status, AutomationStatus::Paused);
    assert_eq!(saved.token_budget, Some(100));
    assert!(!editor.read_with(cx, |view, cx| view.is_dirty(cx)));
    assert!(provider.requests().is_empty());

    edit_goal(&editor, "draft across navigation", cx);
    cx.update(|cx| cx.set_global(OpenedThread(None)));
    cx.run_until_parked();
    assert!(
        f.stream
            .read_with(cx, |stream, cx| stream.automation_can_run(cx))
    );
    cx.update(|cx| cx.set_global(OpenedThread(Some(f.thread.clone()))));
    pump_test_app(cx, |cx| {
        f.root.read_with(cx, |root, _| {
            root.stream_view
                .as_ref()
                .is_some_and(|(id, _)| *id == f.thread.id)
        })
    });
    let reopened = open_goal_editor(&f, cx);
    assert_eq!(reopened, editor);
    assert_eq!(
        editor.read_with(cx, |view, cx| view.instruction(cx).to_string()),
        "draft across navigation"
    );
    f.root
        .update(cx, |root, _| root.agent_provider_override = None);
    edit_goal(&editor, "do not persist composer-test-key", cx);
    editor.update(cx, |view, cx| view.save(cx));
    pump_test_app(cx, |cx| {
        !editor.read_with(cx, |view, _| view.is_saving()) && idle(&f, cx)
    });
    assert_eq!(automation::load(&f.store, &f.thread.id).unwrap()[0], saved);
    assert!(editor.read_with(cx, |view, cx| view.is_dirty(cx)));
    assert!(
        gpui_kit::VisualTestContext::from_window(f.window.into(), cx)
            .debug_bounds("automation-editor-error")
            .is_some()
    );
    editor.update(cx, |view, cx| view.revert(cx));
    assert!(!editor.read_with(cx, |view, cx| view.is_dirty(cx)));
    let service = vega_conversation::McpServerSettingsService::new(
        f.store.database_path().unwrap().to_path_buf(),
        f._data.path().to_path_buf(),
    );
    let server = service
        .create(vega_conversation::types::McpServerForm {
            display_name: "Owned edit credential".into(),
            transport: vega_conversation::types::McpServerTransport::Remote {
                endpoint: "https://mcp.invalid/mcp".into(),
                allow_loopback_http: false,
                authorization: vega_conversation::types::McpRemoteAuthorization::Bearer,
            },
        })
        .unwrap();
    service
        .set_bearer_secret(
            &server.id,
            server.config_revision,
            "owned-mcp-edit-secret-value".into(),
        )
        .unwrap();
    f.root
        .update(cx, |root, _| root.mcp_settings = Some(service));
    edit_goal(&editor, "reject owned-mcp-edit-secret-value", cx);
    editor.update(cx, |view, cx| view.save(cx));
    pump_test_app(cx, |cx| {
        !editor.read_with(cx, |view, _| view.is_saving()) && idle(&f, cx)
    });
    assert_eq!(automation::load(&f.store, &f.thread.id).unwrap()[0], saved);
    assert!(editor.read_with(cx, |view, cx| view.is_dirty(cx)));
    edit_goal(&editor, "saved while navigating away", cx);
    editor.update(cx, |view, cx| view.save(cx));
    cx.update(|cx| cx.set_global(OpenedThread(None)));
    pump_test_app(cx, |cx| {
        !editor.read_with(cx, |view, _| view.is_saving()) && idle(&f, cx)
    });
    assert_eq!(
        automation::load(&f.store, &f.thread.id).unwrap()[0].instruction,
        "saved while navigating away"
    );
    assert!(!editor.read_with(cx, |view, cx| view.is_dirty(cx)));
    assert!(provider.requests().is_empty());
}

#[gpui_kit::test]
async fn editing_running_goal_cancels_only_after_commit_and_next_run_uses_new_instruction(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.executor().allow_parking();
    let provider = Arc::new(MockProvider::new_rounds(vec![
        vec![ScriptStep::delay(Duration::from_secs(30))],
        report_round("edited", true),
        final_round(),
    ]));
    let f = fixture(cx, provider.clone());
    submit(&f, "/goal original objective", cx);
    pump_test_app(cx, |_| provider.requests().len() == 1);
    refresh(&f, cx);
    pump_test_app(cx, |cx| {
        f.stream.read_with(cx, |stream, _| {
            !stream.automation_projection().tasks.is_empty()
        })
    });
    let cancel = cx.update(|cx| {
        cx.global::<AutomationGlobal>().runs[&f.thread.id]
            .cancel
            .clone()
    });
    let editor = open_goal_editor(&f, cx);
    edit_goal(&editor, "new reviewed objective", cx);
    f.store.conn().execute_batch("CREATE TRIGGER reject_edit BEFORE UPDATE ON automation_tasks BEGIN SELECT RAISE(FAIL, 'owned failure'); END;").unwrap();
    editor.update(cx, |view, cx| view.save(cx));
    pump_test_app(cx, |cx| !editor.read_with(cx, |view, _| view.is_saving()));
    assert!(!cancel.is_cancelled());
    assert_eq!(
        automation::load(&f.store, &f.thread.id).unwrap()[0].instruction,
        "original objective"
    );
    assert_eq!(provider.requests().len(), 1);
    f.store
        .conn()
        .execute_batch("DROP TRIGGER reject_edit;")
        .unwrap();
    editor.update(cx, |view, cx| view.save(cx));
    pump_test_app(cx, |cx| {
        cancel.is_cancelled() && !editor.read_with(cx, |view, _| view.is_saving()) && idle(&f, cx)
    });
    assert_eq!(
        automation::load(&f.store, &f.thread.id).unwrap()[0].instruction,
        "new reviewed objective"
    );
    refresh(&f, cx);
    pump_test_app_with_diagnostics(
        cx,
        |cx| {
            automation::load(&f.store, &f.thread.id).unwrap()[0].status
                == AutomationStatus::Complete
                && idle(&f, cx)
        },
        |_| {
            format!(
                "requests={}, tasks={:?}",
                provider.requests().len(),
                automation::load(&f.store, &f.thread.id)
            )
        },
    );
    assert_eq!(provider.requests().len(), 3);
    assert!(
        provider.requests()[1]
            .messages
            .iter()
            .any(|message| message.content.contains("new reviewed objective"))
    );
    let task = automation::load(&f.store, &f.thread.id).unwrap().remove(0);
    assert_eq!(task.used_tokens, 24);
    assert!(task.elapsed_ms > 0);
    assert!(
        vega_store::automation::active_run(f.store.conn(), &f.thread.id)
            .unwrap()
            .is_none()
    );
    refresh(&f, cx);
    pump_test_app(cx, |cx| idle(&f, cx));
    assert_eq!(
        f.stream.read_with(cx, |stream, _| stream
            .automation_timing(&task.identity)
            .unwrap()
            .elapsed_ms),
        task.elapsed_ms
    );
    assert_eq!(provider.requests().len(), 3);
}

#[gpui_kit::test]
async fn editing_attached_goal_does_not_cancel_user_or_loop_owned_run(
    cx: &mut gpui_kit::TestAppContext,
) {
    let provider = Arc::new(MockProvider::new(Vec::new()));
    let f = fixture(cx, provider.clone());
    edit(&f, "keep new dispatch paused", cx);
    automation::command(
        &f.store,
        &f.thread.id,
        &automation::parse_command("/goal original")
            .unwrap()
            .unwrap(),
        automation::now_ms(),
    )
    .unwrap();
    automation::command(
        &f.store,
        &f.thread.id,
        &automation::parse_command("/loop 1m inspect")
            .unwrap()
            .unwrap(),
        automation::now_ms(),
    )
    .unwrap();
    refresh(&f, cx);
    pump_test_app(cx, |cx| idle(&f, cx));
    let editor = open_goal_editor(&f, cx);
    for (index, source) in [None, Some(AutomationKind::Loop)].into_iter().enumerate() {
        let claim = automation::claim(&f.store, &f.thread.id, source, automation::now_ms())
            .unwrap()
            .unwrap();
        let claim = if source.is_some() {
            automation::activate_claim(&f.store, &claim)
                .unwrap()
                .unwrap()
        } else {
            claim
        };
        let cancel = tokio_util::sync::CancellationToken::new();
        let owner = f.root.read_with(cx, |root, _| root.window_alive.clone());
        let generation = u64::try_from(index + 10).unwrap();
        cx.update(|cx| {
            cx.update_global::<AutomationGlobal, _>(|global, _| {
                global.runs.insert(
                    f.thread.id.clone(),
                    crate::automation_controller::AutomationRunOwner {
                        generation,
                        owner: Arc::downgrade(&owner),
                        claim: Some(claim.clone()),
                        cancel: cancel.clone(),
                        started_at: Some(std::time::Instant::now()),
                    },
                );
            })
        });
        edit_goal(&editor, &format!("changed while {source:?}"), cx);
        editor.update(cx, |view, cx| view.save(cx));
        pump_test_app(cx, |cx| {
            !editor.read_with(cx, |view, _| view.is_saving()) && idle(&f, cx)
        });
        refresh(&f, cx);
        pump_test_app(cx, |cx| idle(&f, cx));
        assert!(!cancel.is_cancelled());
        assert!(provider.requests().is_empty());
        automation::finish(&f.store, &claim, true, true, 100, automation::now_ms()).unwrap();
        cx.update(|cx| {
            cx.update_global::<AutomationGlobal, _>(|global, _| {
                global.finish_run(&f.thread.id, generation, &owner)
            })
        });
        refresh(&f, cx);
        pump_test_app(cx, |cx| idle(&f, cx));
    }
    assert_eq!(
        automation::load(&f.store, &f.thread.id)
            .unwrap()
            .iter()
            .find(|task| task.kind == AutomationKind::Goal)
            .unwrap()
            .elapsed_ms,
        200
    );
}

#[gpui_kit::test]
async fn visible_peer_editor_blocks_dispatch_and_conflict_reload_resumes_eligibility(
    cx: &mut gpui_kit::TestAppContext,
) {
    let provider = Arc::new(MockProvider::new_rounds(vec![
        report_round("complete", true),
        final_round(),
    ]));
    let f = fixture(cx, provider.clone());
    edit(&f, "hold dispatch until editors open", cx);
    automation::command(
        &f.store,
        &f.thread.id,
        &automation::parse_command("/goal initial objective")
            .unwrap()
            .unwrap(),
        automation::now_ms(),
    )
    .unwrap();
    refresh(&f, cx);
    pump_test_app(cx, |cx| idle(&f, cx));
    let first = open_goal_editor(&f, cx);
    let other_root = cx.new(VegaWindow::new);
    other_root.update(cx, |root, _| {
        root.model_selection_config_override = Some(f._data.path().join("config.toml"));
        root.agent_provider_override = Some(with_auxiliary_title_fixture(provider.clone()));
    });
    let retained = other_root.clone();
    let other_window = cx.update(|cx| {
        cx.open_window(Default::default(), move |_, _| retained)
            .unwrap()
    });
    pump_test_app(cx, |cx| {
        other_root.read_with(cx, |root, _| {
            root.stream_view.is_some() && !root.model_catalog_loading
        })
    });
    let other_stream =
        other_root.read_with(cx, |root, _| root.stream_view.as_ref().unwrap().1.clone());
    let task = automation::load(&f.store, &f.thread.id).unwrap().remove(0);
    other_root.update(cx, |root, cx| {
        root.bind_automation(other_stream.clone(), f.thread.id.clone(), cx)
    });
    pump_test_app(cx, |cx| {
        other_stream.read_with(cx, |stream, _| {
            !stream.automation_projection().tasks.is_empty()
        })
    });
    other_stream.update(cx, |_, cx| {
        cx.emit(vega_ui::conversation_stream::AutomationEditorRequested {
            thread_id: f.thread.id.clone(),
            identity: task.identity.clone(),
        })
    });
    cx.run_until_parked();
    let second = other_root.read_with(cx, |root, _| {
        root.automation_editors[&vega_ui::automation_editor::AutomationEditorKey::for_task(&task)]
            .clone()
    });
    edit_goal(&second, "peer unsaved draft", cx);
    edit(&f, "", cx);
    refresh(&f, cx);
    pump_test_app(cx, |cx| idle(&f, cx));
    assert!(provider.requests().is_empty());
    assert!(!cx.update(|cx| {
        cx.global::<AutomationGlobal>()
            .streams_ready(&f.thread.id, cx)
    }));
    edit_goal(&first, "first window saved objective", cx);
    first.update(cx, |view, cx| view.save(cx));
    pump_test_app(cx, |cx| {
        !first.read_with(cx, |view, _| view.is_saving()) && idle(&f, cx)
    });
    other_root.update(cx, |root, cx| {
        root.bind_automation(other_stream.clone(), f.thread.id.clone(), cx)
    });
    pump_test_app(cx, |cx| second.read_with(cx, |view, _| view.has_conflict()));
    assert_eq!(
        second.read_with(cx, |view, cx| view.instruction(cx).to_string()),
        "peer unsaved draft"
    );
    assert!(!second.read_with(cx, |view, cx| view.can_save(cx)));
    assert!(provider.requests().is_empty());
    second.update(cx, |view, cx| view.revert(cx));
    cx.run_until_parked();
    refresh(&f, cx);
    pump_test_app(cx, |cx| {
        automation::load(&f.store, &f.thread.id).unwrap()[0].status == AutomationStatus::Complete
            && idle(&f, cx)
    });
    assert_eq!(provider.requests().len(), 2);
    assert!(
        provider.requests()[0]
            .messages
            .iter()
            .any(|message| message.content.contains("first window saved objective"))
    );
    assert!(!second.read_with(cx, |view, cx| view.is_dirty(cx)));
    other_window
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();

    let loop_task = automation::command(
        &f.store,
        &f.thread.id,
        &automation::parse_command("/loop 5m original loop")
            .unwrap()
            .unwrap(),
        automation::now_ms(),
    )
    .unwrap()
    .into_iter()
    .find(|task| task.kind == AutomationKind::Loop)
    .unwrap();
    automation::command(
        &f.store,
        &f.thread.id,
        &AutomationCommand::Control(AutomationKind::Loop, AutomationControl::Pause),
        automation::now_ms(),
    )
    .unwrap();
    refresh(&f, cx);
    pump_test_app(cx, |cx| idle(&f, cx));
    f.stream.update(cx, |_, cx| {
        cx.emit(vega_ui::conversation_stream::AutomationEditorRequested {
            thread_id: f.thread.id.clone(),
            identity: loop_task.identity.clone(),
        })
    });
    cx.run_until_parked();
    let loop_editor = f.root.read_with(cx, |root, _| {
        root.automation_editors
            [&vega_ui::automation_editor::AutomationEditorKey::for_task(&loop_task)]
            .clone()
    });
    edit_goal(&loop_editor, "edited loop\nsecond line", cx);
    loop_editor.update(cx, |view, cx| view.save(cx));
    pump_test_app(cx, |cx| {
        !loop_editor.read_with(cx, |view, _| view.is_saving()) && idle(&f, cx)
    });
    let saved_loop = automation::load(&f.store, &f.thread.id)
        .unwrap()
        .into_iter()
        .find(|task| task.kind == AutomationKind::Loop)
        .unwrap();
    assert_eq!(saved_loop.instruction, "edited loop\nsecond line");
    assert_eq!(saved_loop.status, AutomationStatus::Paused);
    assert_eq!(saved_loop.interval_seconds, loop_task.interval_seconds);
    assert_eq!(saved_loop.expires_at_ms, loop_task.expires_at_ms);
    assert_eq!(provider.requests().len(), 2);
    edit_goal(&loop_editor, "old identity draft", cx);
    automation::command(
        &f.store,
        &f.thread.id,
        &AutomationCommand::Control(AutomationKind::Loop, AutomationControl::Clear),
        automation::now_ms(),
    )
    .unwrap();
    automation::command(
        &f.store,
        &f.thread.id,
        &automation::parse_command("/loop 10m replacement loop")
            .unwrap()
            .unwrap(),
        automation::now_ms(),
    )
    .unwrap();
    let replacement = automation::command(
        &f.store,
        &f.thread.id,
        &AutomationCommand::Control(AutomationKind::Loop, AutomationControl::Pause),
        automation::now_ms(),
    )
    .unwrap()
    .into_iter()
    .find(|task| task.kind == AutomationKind::Loop)
    .unwrap();
    refresh(&f, cx);
    pump_test_app(cx, |cx| idle(&f, cx));
    assert!(loop_editor.read_with(cx, |view, cx| view.has_conflict() && !view.can_save(cx)));
    loop_editor.update(cx, |view, cx| view.revert(cx));
    cx.run_until_parked();
    let replacement_key = vega_ui::automation_editor::AutomationEditorKey::for_task(&replacement);
    let previous_key = vega_ui::automation_editor::AutomationEditorKey::for_task(&loop_task);
    let old_selector =
        Box::leak(format!("workspace-tab-Automation({previous_key:?})").into_boxed_str());
    let new_selector =
        Box::leak(format!("workspace-tab-Automation({replacement_key:?})").into_boxed_str());
    let mut visual = gpui_kit::VisualTestContext::from_window(f.window.into(), cx);
    assert!(visual.debug_bounds(old_selector).is_none());
    assert!(visual.debug_bounds(new_selector).is_some());
    f.root.read_with(cx, |root, cx| {
        assert_eq!(
            root.automation_editors[&replacement_key]
                .read(cx)
                .instruction(cx),
            "replacement loop"
        );
    });
    assert_eq!(
        loop_editor.read_with(cx, |view, cx| view.instruction(cx).to_owned()),
        "old identity draft"
    );
    assert_eq!(provider.requests().len(), 2);
}
