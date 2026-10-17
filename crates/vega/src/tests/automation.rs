use super::*;
use crate::automation_controller::AutomationGlobal;
use vega_conversation::automation;
use vega_runtime::{MockProvider, ProviderEvent, ScriptStep, StopReason};
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
    pump_test_app(cx, |cx| {
        refresh(&f, cx);
        automation::load(&f.store, &f.thread.id)
            .unwrap()
            .first()
            .is_some_and(|task| task.status == AutomationStatus::Complete)
            && idle(&f, cx)
    });
    assert_eq!(provider.requests().len(), 4);
    assert!(draft(&f, cx).is_empty());
    let task = automation::load(&f.store, &f.thread.id).unwrap().remove(0);
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
    let provider = Arc::new(MockProvider::new_rounds(vec![
        vec![ScriptStep::delay(Duration::from_secs(30))],
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
    vega_conversation::threads::set_thread_status(&f.store, &f.thread.id, ThreadStatus::Active)
        .unwrap();
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
