use super::*;
use gpui_kit::WindowHandle;
use vega_runtime::{MockProvider, ProviderEvent, ScriptStep, StopReason};
use vega_ui::conversation_stream::set_composer_predictions_enabled;

struct Fixture {
    _directory: TempDir,
    store: Store,
    thread: Thread,
    root: Entity<VegaWindow>,
    stream: Entity<ConversationStream>,
    window: WindowHandle<VegaWindow>,
    primary: Arc<MockProvider>,
    predictions: MockProvider,
}

fn fixture(
    cx: &mut gpui_kit::TestAppContext,
    prediction_script: Vec<ScriptStep>,
    enabled: bool,
) -> Fixture {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");
    super::model_selection::model_selection_config(&config);
    vega_store::config::update_from(&config, |config| {
        config.agent.composer_predictions_enabled = enabled
    })
    .unwrap();
    vega_store::keystore::set_key(directory.path(), "owned", "synthetic-prediction-owner-key")
        .unwrap();
    let database = directory.path().join("predictions.db");
    let store = Store::open(&database).unwrap();
    store.migrate().unwrap();
    let project = vega_store::projects::create(
        store.conn(),
        directory.path().to_str().unwrap(),
        "predictions",
        None,
    )
    .unwrap();
    let thread =
        vega_conversation::threads::create_thread(&store, &project.id, "gpt-5.6-terra", "confirm")
            .unwrap();
    cx.update(|cx| {
        install_diff_window_globals(Store::open(&database).unwrap(), thread.clone(), cx)
    });
    let primary = Arc::new(MockProvider::new(vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("主轮完成".into()),
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]));
    let predictions = MockProvider::new(prediction_script);
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, _| {
        root.model_selection_config_override = Some(config);
        root.agent_provider_override =
            Some(super::title_fixture::with_auxiliary_prediction_fixture(
                primary.clone(),
                predictions.clone(),
            ));
    });
    let window_root = root.clone();
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), move |_, _| window_root)
            .unwrap()
    });
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.stream_view.is_some()
                && root.agent_controller.queue_operations.is_empty()
                && !root.model_catalog_loading
                && root.configured_models.is_some()
                && matches!(
                    root.pricing_controller.state,
                    PricingControllerState::Ready { .. }
                )
        })
    });
    let stream = root.read_with(cx, |root, _| root.stream_view.as_ref().unwrap().1.clone());
    Fixture {
        _directory: directory,
        store,
        thread,
        root,
        stream,
        window,
        primary,
        predictions,
    }
}

fn completion() -> Vec<ScriptStep> {
    vec![ScriptStep::events(vec![
        ProviderEvent::TextDelta("{\"suggestion\":\"请继续检查边界\"}".into()),
        ProviderEvent::Usage {
            input: 11,
            output: 4,
            cache_read: Some(2),
            cache_write: Some(0),
        },
        ProviderEvent::Done {
            stop_reason: StopReason::End,
        },
    ])]
}

fn submit(fixture: &Fixture, cx: &mut gpui_kit::TestAppContext) {
    fixture
        .stream
        .read_with(cx, |stream, _| stream.composer_input())
        .update(cx, |input, cx| input.set_text("检查输入预测", cx));
    fixture
        .window
        .update(cx, |_, window, cx| {
            fixture
                .stream
                .update(cx, |stream, cx| stream.focus_composer(window, cx))
        })
        .unwrap();
    cx.run_until_parked();
    cx.simulate_keystrokes(fixture.window.into(), "cmd-enter");
}

#[gpui_kit::test]
async fn successful_native_round_predicts_after_queue_release_and_accepts_without_sending(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx, completion(), true);
    submit(&f, cx);
    pump_test_app(cx, |cx| {
        f.stream.read_with(cx, |stream, cx| {
            stream.composer_input().read(cx).prediction_text().is_some()
        })
    });
    assert_eq!(f.primary.requests().len(), 1);
    let requests = f.predictions.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].model, f.thread.model);
    assert!(requests[0].tools.is_empty());
    assert!(
        f.root
            .read_with(cx, |root, _| root.agent_controller.active.is_empty()
                && root.agent_controller.queue_operations.is_empty())
    );
    let before = vega_store::messages::recent(f.store.conn(), &f.thread.id, 100).unwrap();
    assert_eq!(before.len(), 2);
    assert_eq!(before[1].status, "done");
    assert!(f.stream.read_with(cx, |stream, cx| {
        stream.composer_input().read(cx).text().is_empty()
    }));
    cx.simulate_keystrokes(f.window.into(), "enter");
    cx.run_until_parked();
    assert_eq!(
        f.stream.read_with(cx, |stream, cx| stream
            .composer_input()
            .read(cx)
            .text()
            .to_owned()),
        "请继续检查边界"
    );
    assert_eq!(f.primary.requests().len(), 1);
    assert!(
        f.root
            .read_with(cx, |root, _| root.agent_controller.active.is_empty())
    );
    assert_eq!(
        vega_store::messages::recent(f.store.conn(), &f.thread.id, 100).unwrap(),
        before
    );
    let usage: (i64, i64, i64) = f.store.conn().query_row("SELECT COUNT(*),SUM(input_tokens),COUNT(pricing_version) FROM token_usage WHERE thread_id=?1 AND message_id IS NULL AND input_tokens=11", [&f.thread.id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).unwrap();
    assert_eq!(usage, (1, 11, 1));
}

#[gpui_kit::test]
async fn prediction_preference_disabled_stays_quiet_and_reenable_waits_for_next_round(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx, completion(), false);
    submit(&f, cx);
    pump_test_app(cx, |cx| {
        f.primary.requests().len() == 1
            && f.root.read_with(cx, |root, _| {
                root.agent_controller.active.is_empty()
                    && root.agent_controller.queue_operations.is_empty()
            })
    });
    assert!(f.predictions.requests().is_empty());
    assert!(f.stream.read_with(cx, |stream, cx| {
        stream.composer_input().read(cx).prediction_text().is_none()
    }));
    vega_store::config::update_from(
        f.root
            .read_with(cx, |root, _| root.model_selection_config_override.clone())
            .as_ref()
            .unwrap(),
        |config| config.agent.composer_predictions_enabled = true,
    )
    .unwrap();
    cx.update(|cx| set_composer_predictions_enabled(true, cx));
    cx.run_until_parked();
    assert!(f.predictions.requests().is_empty());
    submit(&f, cx);
    pump_test_app(cx, |cx| {
        f.stream.read_with(cx, |stream, cx| {
            stream.composer_input().read(cx).prediction_text().is_some()
        })
    });
    assert_eq!(f.predictions.requests().len(), 1);
    assert_eq!(f.primary.requests().len(), 2);
}

#[gpui_kit::test]
async fn prediction_cancel_keeps_manual_draft_and_primary_success(
    cx: &mut gpui_kit::TestAppContext,
) {
    let mut delayed = vec![ScriptStep::delay(Duration::from_secs(60))];
    delayed.extend(completion());
    let f = fixture(cx, delayed, true);
    submit(&f, cx);
    pump_test_app(cx, |_| f.predictions.requests().len() == 1);
    let input = f.stream.read_with(cx, |stream, _| stream.composer_input());
    input.update(cx, |input, cx| input.set_text("保留我的草稿", cx));
    cx.run_until_parked();
    f.root
        .update(cx, |root, cx| root.cancel_composer_prediction(cx));
    cx.run_until_parked();
    assert_eq!(
        input.read_with(cx, |input, _| input.text().to_owned()),
        "保留我的草稿"
    );
    assert!(input.read_with(cx, |input, _| input.prediction_text().is_none()));
    assert_eq!(f.primary.requests().len(), 1);
    let rows = vega_store::messages::recent(f.store.conn(), &f.thread.id, 100).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].status, "done");
}

#[gpui_kit::test]
async fn background_completion_cannot_clear_current_prediction_and_route_changes_cancel_it(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx, completion(), true);
    submit(&f, cx);
    pump_test_app(cx, |cx| {
        f.stream.read_with(cx, |stream, cx| {
            stream.composer_input().read(cx).prediction_text().is_some()
        })
    });
    let other = vega_conversation::threads::create_thread(
        &f.store,
        &f.thread.project_id,
        &f.thread.model,
        "confirm",
    )
    .unwrap();
    let background = cx.new(|cx| ConversationStream::new(other.clone(), cx));
    let unused = Arc::new(MockProvider::new(completion()));
    f.root.update(cx, |root, cx| {
        root.prepare_composer_prediction(
            &background,
            ComposerPredictionRequest::new(unused.clone(), other.model.clone(), other.mode),
            "background-completion",
            cx,
        )
    });
    cx.run_until_parked();
    assert!(unused.requests().is_empty());
    assert!(f.stream.read_with(cx, |stream, cx| {
        stream.composer_input().read(cx).prediction_text().is_some()
    }));
    cx.update(|cx| cx.set_global(PendingDeleteConfirm(Some(other.clone()))));
    f.root
        .read_with(cx, |root, _| root.sidebar.clone())
        .update(cx, |sidebar, cx| sidebar.confirm_pending_delete(cx));
    cx.run_until_parked();
    assert!(
        vega_store::threads::find(f.store.conn(), &other.id)
            .unwrap()
            .is_none()
    );
    assert!(f.stream.read_with(cx, |stream, cx| {
        stream.composer_input().read(cx).prediction_text().is_some()
    }));
    let next_route = vega_conversation::threads::create_thread(
        &f.store,
        &f.thread.project_id,
        &f.thread.model,
        "confirm",
    )
    .unwrap();
    cx.update(|cx| cx.set_global(OpenedThread(Some(next_route))));
    cx.run_until_parked();
    assert!(f.stream.read_with(cx, |stream, cx| {
        stream.composer_input().read(cx).prediction_text().is_none()
    }));
    assert!(
        f.root
            .read_with(cx, |root, _| root.composer_prediction.is_none())
    );
    cx.update(|cx| cx.set_global(OpenedThread(Some(f.thread.clone()))));
    cx.run_until_parked();
    assert_eq!(f.predictions.requests().len(), 1);
    assert!(f.stream.read_with(cx, |stream, cx| {
        stream.composer_input().read(cx).prediction_text().is_none()
    }));
}

#[test]
fn excluded_primary_rounds_never_release_prediction_authority() {
    for case in [
        "failed",
        "stopped",
        "plan",
        "child",
        "disabled",
        "automation",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("config.toml");
        super::model_selection::model_selection_config(&config);
        vega_store::config::update_from(&config, |config| {
            config.agent.composer_predictions_enabled = case != "disabled"
        })
        .unwrap();
        vega_store::keystore::set_key(directory.path(), "owned", "synthetic-excluded-round-key")
            .unwrap();
        let database = directory.path().join("excluded.db");
        let store = Store::open(&database).unwrap();
        store.migrate().unwrap();
        let project = vega_store::projects::create(
            store.conn(),
            directory.path().to_str().unwrap(),
            "excluded",
            None,
        )
        .unwrap();
        let mut thread = vega_conversation::threads::create_thread(
            &store,
            &project.id,
            "gpt-5.6-terra",
            "confirm",
        )
        .unwrap();
        if case == "plan" {
            vega_conversation::threads::set_thread_mode(&store, &thread.id, ThreadMode::Plan)
                .unwrap();
            thread.mode = ThreadMode::Plan;
        }
        if case == "child" {
            let parent = vega_conversation::threads::create_thread(
                &store,
                &project.id,
                &thread.model,
                "confirm",
            )
            .unwrap();
            store
                .conn()
                .execute(
                    "UPDATE threads SET parent_thread_id=?1 WHERE id=?2",
                    [&parent.id, &thread.id],
                )
                .unwrap();
            thread.parent_thread_id = Some(parent.id);
        }
        let run = if case == "automation" {
            let now = vega_conversation::automation::now_ms();
            let command = vega_conversation::automation::parse_command("/loop 1m wait for change")
                .unwrap()
                .unwrap();
            vega_conversation::automation::command(&store, &thread.id, &command, now).unwrap();
            let claim = vega_conversation::automation::claim(
                &store,
                &thread.id,
                Some(AutomationKind::Loop),
                now + 120_000,
            )
            .unwrap()
            .unwrap();
            PendingAgentRun::Automation(Box::new(claim))
        } else {
            PendingAgentRun::UserMessage("fixture input".into())
        };
        let script = if case == "failed" {
            vec![ScriptStep::Error {
                status: Some(400),
                message: "fixture failure".into(),
                retryable: false,
            }]
        } else {
            vec![ScriptStep::events(vec![
                ProviderEvent::TextDelta("completed body".into()),
                ProviderEvent::Done {
                    stop_reason: StopReason::End,
                },
            ])]
        };
        let provider = Arc::new(MockProvider::new(script));
        let cancel = tokio_util::sync::CancellationToken::new();
        if case == "stopped" {
            cancel.cancel();
        }
        let (sender, receiver) = mpsc::sync_channel(AGENT_EVENT_CAPACITY);
        run_agent_worker(
            database,
            directory.path().into(),
            thread,
            run,
            vega_conversation::agent::PermissionQueue::new(),
            cancel,
            sender,
            None,
            Some(config),
            None,
            None,
            Some(provider.clone()),
            Arc::new(AgentWorkerStartProbe::default()),
        );
        let batch = drain_agent_updates(&receiver);
        assert!(batch.finished.is_some(), "{case}");
        assert!(batch.composer_prediction.is_none(), "{case}");
        assert_eq!(
            provider.requests().len(),
            usize::from(case != "stopped"),
            "{case}"
        );
    }
}

#[gpui_kit::test]
async fn codex_and_historical_routes_cannot_launch_prediction_requests(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx, completion(), true);
    assert!(f.predictions.requests().is_empty());
    let now = vega_conversation::automation::now_ms();
    for (id, role, seq, content) in [
        ("historical-user", "user", 1, "old question"),
        ("historical-answer", "assistant", 2, "old answer"),
    ] {
        insert(
            f.store.conn(),
            &MessageRow {
                id: id.into(),
                thread_id: f.thread.id.clone(),
                seq,
                role: role.into(),
                kind: "text".into(),
                content: content.into(),
                status: "done".into(),
                created_at: now,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
    }
    f.root.update(cx, |root, cx| {
        root.stream_view = None;
        cx.notify();
    });
    cx.update(|cx| cx.set_global(OpenedThread(Some(f.thread.clone()))));
    cx.run_until_parked();
    assert!(f.predictions.requests().is_empty());
    assert!(f.primary.requests().is_empty());
    let stream = f
        .root
        .read_with(cx, |root, _| root.stream_view.as_ref().unwrap().1.clone());
    let mut codex = f.thread.clone();
    codex.backend = TaskBackend::Codex;
    stream.update(cx, |stream, cx| stream.apply_thread(codex.clone(), cx));
    cx.update(|cx| cx.set_global(OpenedThread(Some(codex))));
    let unused = Arc::new(MockProvider::new(completion()));
    f.root.update(cx, |root, cx| {
        root.prepare_composer_prediction(
            &stream,
            ComposerPredictionRequest::new(unused.clone(), f.thread.model.clone(), f.thread.mode),
            "historical-answer",
            cx,
        )
    });
    cx.run_until_parked();
    assert!(unused.requests().is_empty());
    assert!(
        f.root
            .read_with(cx, |root, _| root.composer_prediction.is_none())
    );
}

#[gpui_kit::test]
async fn prediction_provider_failure_stays_silent_and_preserves_success_and_usage(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(
        cx,
        vec![
            ScriptStep::events(vec![ProviderEvent::Usage {
                input: 13,
                output: 2,
                cache_read: None,
                cache_write: None,
            }]),
            ScriptStep::Error {
                status: Some(503),
                message: "prediction fixture failure".into(),
                retryable: true,
            },
        ],
        true,
    );
    submit(&f, cx);
    pump_test_app(cx, |cx| {
        f.predictions.requests().len() == 1
            && f.root.read_with(cx, |root, _| {
                root.composer_prediction.is_none() && root.agent_controller.active.is_empty()
            })
    });
    assert_eq!(f.primary.requests().len(), 1);
    assert!(
        f.stream
            .read_with(cx, |stream, cx| stream.controller_error_message().is_none()
                && stream.composer_input().read(cx).prediction_text().is_none()
                && stream.composer_input().read(cx).text().is_empty())
    );
    let rows = vega_store::messages::recent(f.store.conn(), &f.thread.id, 100).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].status, "done");
    let usage: i64 = f.store.conn().query_row("SELECT COUNT(*) FROM token_usage WHERE thread_id=?1 AND message_id IS NULL AND input_tokens=13", [&f.thread.id], |row| row.get(0)).unwrap();
    assert_eq!(usage, 1);
}

#[gpui_kit::test]
async fn deleting_prediction_owner_clears_ghost_and_cancels_controller(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx, completion(), true);
    submit(&f, cx);
    pump_test_app(cx, |cx| {
        f.stream.read_with(cx, |stream, cx| {
            stream.composer_input().read(cx).prediction_text().is_some()
        })
    });
    cx.update(|cx| cx.set_global(PendingDeleteConfirm(Some(f.thread.clone()))));
    f.root
        .read_with(cx, |root, _| root.sidebar.clone())
        .update(cx, |sidebar, cx| sidebar.confirm_pending_delete(cx));
    cx.run_until_parked();
    assert!(
        vega_store::threads::find(f.store.conn(), &f.thread.id)
            .unwrap()
            .is_none()
    );
    assert!(
        f.root
            .read_with(cx, |root, _| root.composer_prediction.is_none())
    );
    assert!(f.stream.read_with(cx, |stream, cx| {
        stream.composer_input().read(cx).prediction_text().is_none()
    }));
}

#[gpui_kit::test]
async fn window_terminal_cleanup_releases_prediction_and_refuses_new_work(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx, completion(), true);
    submit(&f, cx);
    pump_test_app(cx, |cx| {
        f.stream.read_with(cx, |stream, cx| {
            stream.composer_input().read(cx).prediction_text().is_some()
        })
    });
    f.root.update(cx, |root, _| root.window_terminal_cleanup());
    f.stream.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    assert!(
        f.root
            .read_with(cx, |root, _| root.composer_prediction.is_none())
    );
    assert!(f.stream.read_with(cx, |stream, cx| {
        stream.composer_input().read(cx).prediction_text().is_none()
    }));
    let unused = Arc::new(MockProvider::new(completion()));
    f.root.update(cx, |root, cx| {
        root.prepare_composer_prediction(
            &f.stream,
            ComposerPredictionRequest::new(unused.clone(), f.thread.model.clone(), f.thread.mode),
            "closed-window",
            cx,
        )
    });
    assert!(unused.requests().is_empty());
    assert!(
        f.root
            .read_with(cx, |root, _| root.composer_prediction.is_none())
    );
}
