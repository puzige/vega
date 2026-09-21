use super::*;
use gpui_kit::{Modifiers, VisualTestContext};

#[test]
fn commit_provider_policy_disables_retries() {
    assert_eq!(commit_retry_policy().max_retries, 0);
}

#[test]
fn bounded_agent_channel_preserves_burst_order_and_terminal() {
    let (sender, receiver) = mpsc::sync_channel(AGENT_EVENT_CAPACITY);
    let producer = std::thread::spawn(move || {
        for index in 0..(AGENT_EVENT_CAPACITY + AGENT_EVENT_BATCH + 17) {
            sender
                .send(AgentUpdate::Event(
                    vega_conversation::types::ConversationEvent::TextDelta {
                        message_id: "message".into(),
                        delta: index.to_string(),
                    },
                ))
                .expect("bounded event send");
        }
        sender
            .send(AgentUpdate::Finished {
                success: true,
                reference_failure: None,
                credential_failure: false,
            })
            .expect("terminal send");
    });
    let mut seen = Vec::new();
    let finished = loop {
        let batch = drain_agent_updates(&receiver);
        assert!(batch.events.len() <= AGENT_EVENT_BATCH);
        for event in batch.events {
            if let vega_conversation::types::ConversationEvent::TextDelta { delta, .. } = event {
                seen.push(delta.parse::<usize>().expect("ordered index"));
            }
        }
        if let Some(finished) = batch.finished {
            break finished;
        }
        std::thread::yield_now();
    };
    producer.join().expect("bounded producer");
    assert!(finished);
    assert_eq!(seen, (0..seen.len()).collect::<Vec<_>>());
    assert_eq!(seen.len(), AGENT_EVENT_CAPACITY + AGENT_EVENT_BATCH + 17);
    assert!(AGENT_EVENT_POLL < Duration::from_millis(16));
}

#[test]
fn same_batch_applies_events_before_terminal() {
    let (sender, receiver) = mpsc::sync_channel(4);
    sender
        .send(AgentUpdate::Event(
            vega_conversation::types::ConversationEvent::MessageStarted {
                message_id: "durable".into(),
                seq: 2,
            },
        ))
        .expect("event send");
    sender
        .send(AgentUpdate::Finished {
            success: false,
            reference_failure: None,
            credential_failure: false,
        })
        .expect("terminal send");
    let batch = drain_agent_updates(&receiver);
    assert_eq!(batch.events.len(), 1);
    assert_eq!(batch.finished, Some(false));
    assert!(matches!(
        &batch.events[0],
        vega_conversation::types::ConversationEvent::MessageStarted { message_id, .. }
            if message_id == "durable"
    ));
}

#[test]
fn issue73_unavailable_enabled_servers_are_not_dropped_from_worker_batch() {
    let (sender, receiver) = mpsc::sync_channel(4);
    let diagnostic = vega_conversation::types::McpServerDiagnostic {
        server_id: "01J00000000000000000000000".into(),
        code: "authorization_required".into(),
    };
    sender
        .send(AgentUpdate::McpUnavailable(vec![diagnostic.clone()]))
        .expect("content-free warning");
    sender
        .send(AgentUpdate::Finished {
            success: true,
            reference_failure: None,
            credential_failure: false,
        })
        .expect("terminal");
    let batch = drain_agent_updates(&receiver);
    assert_eq!(batch.mcp_unavailable, vec![diagnostic]);
    assert_eq!(batch.finished, Some(true));
}

struct AgentWindowHarness {
    root: Entity<VegaWindow>,
}

impl Render for AgentWindowHarness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.root.clone()
    }
}

struct AgentWorkerGuard {
    cancel: tokio_util::sync::CancellationToken,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Drop for AgentWorkerGuard {
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[gpui_kit::test]
async fn issue67_concurrent_production_new_thread_enters_before_origin_finishes(
    cx: &mut gpui_kit::TestAppContext,
) {
    issue67_concurrent_scenario(cx, 0).await;
}
#[gpui_kit::test]
async fn issue67_concurrent_stop_b_only(cx: &mut gpui_kit::TestAppContext) {
    issue67_concurrent_scenario(cx, 1).await;
}
#[gpui_kit::test]
async fn issue67_concurrent_stop_a_only(cx: &mut gpui_kit::TestAppContext) {
    issue67_concurrent_scenario(cx, 2).await;
}
#[gpui_kit::test]
async fn issue67_concurrent_window_close_interrupts_both(cx: &mut gpui_kit::TestAppContext) {
    issue67_concurrent_scenario(cx, 3).await;
}

#[gpui_kit::test]
async fn issue67_concurrent_b_preprovider_failure_can_retry(cx: &mut gpui_kit::TestAppContext) {
    issue67_concurrent_scenario(cx, 4).await;
}

#[gpui_kit::test]
async fn issue67_concurrent_finishes_under_settings_without_route_change(
    cx: &mut gpui_kit::TestAppContext,
) {
    issue67_concurrent_scenario(cx, 5).await;
}

#[gpui_kit::test]
async fn issue67_concurrent_settings_fails_closed_for_both_permissions(
    cx: &mut gpui_kit::TestAppContext,
) {
    issue67_concurrent_scenario(cx, 6).await;
}

async fn issue67_concurrent_scenario(cx: &mut gpui_kit::TestAppContext, mode: u8) {
    cx.executor().allow_parking();
    let data = tempfile::tempdir().expect("issue67 data root");
    let repo = diff_controller_repo();
    let config_path = data.path().join("config.toml");
    super::model_selection::model_selection_config(&config_path);
    vega_store::keystore::set_key(data.path(), "owned", "issue67-owned-test-key")
        .expect("issue67 credential");
    let database_path = data.path().join("vega.db");
    let store = Store::open(&database_path).expect("issue67 store");
    store.migrate().expect("issue67 migrations");
    let project = vega_store::projects::create(
        store.conn(),
        repo.path().to_str().expect("UTF-8 issue67 repo"),
        "issue67-e2e",
        None,
    )
    .expect("issue67 project");
    let origin = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "gpt-5.6-terra",
        if mode == 6 {
            PermissionMode::Confirm.as_str()
        } else {
            PermissionMode::Auto.as_str()
        },
    )
    .and_then(|thread| {
        vega_conversation::threads::rename_thread(&store, &thread.id, "Issue67 Origin")
    })
    .expect("issue67 origin");
    cx.update(|cx| {
        install_diff_window_globals(
            Store::open(&database_path).expect("issue67 root store"),
            origin.clone(),
            cx,
        )
    });

    std::fs::write(
        repo.path().join("concurrent.txt"),
        "owned concurrent fixture",
    )
    .expect("read fixture");
    let make_provider = |label: &str, delay: u64, usage: u64| {
        Arc::new(vega_runtime::MockProvider::new_rounds(vec![
            vec![
                vega_runtime::ScriptStep::text(format!("{label} first ")),
                vega_runtime::ScriptStep::delay(Duration::from_millis(delay)),
                vega_runtime::ScriptStep::events(vec![
                    vega_runtime::ProviderEvent::ToolUse {
                        id: format!("{label}-read"),
                        name: if label == "origin" || mode == 6 { "write" } else { "read" }.into(),
                        input_json: if label == "origin" {
                            r#"{"path":"origin-artifact.txt","content":"origin artifact"}"#
                        } else if mode == 6 {
                            r#"{"path":"destination-artifact.txt","content":"destination artifact"}"#
                        } else {
                            r#"{"path":"concurrent.txt"}"#
                        }
                        .into(),
                    },
                    vega_runtime::ProviderEvent::Done {
                        stop_reason: vega_runtime::StopReason::ToolUse,
                    },
                ]),
            ],
            vec![vega_runtime::ScriptStep::events(vec![
                vega_runtime::ProviderEvent::TextDelta(format!("{label} complete")),
                vega_runtime::ProviderEvent::Usage {
                    input: usage,
                    output: 7,
                    cache_read: 0,
                    cache_write: 0,
                },
                vega_runtime::ProviderEvent::Done {
                    stop_reason: vega_runtime::StopReason::End,
                },
            ])],
        ]))
    };
    let provider = make_provider("origin", 600, 31);
    let provider_b = make_provider("destination", 1200, 59);
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, _| {
        root.model_selection_config_override = Some(config_path.clone());
        root.agent_provider_override = Some(with_auxiliary_title_fixture(provider.clone()));
    });
    let window_root = root.clone();
    let window = cx
        .update(|cx| cx.open_window(Default::default(), move |_, _| window_root))
        .expect("issue67 production window");
    cx.update(|cx| crate::app_palette::bind_shortcuts(window.into(), root.downgrade(), cx));
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.stream_view.is_some()
                && root.configured_models.is_some()
                && !root.model_catalog_loading
                && matches!(
                    root.pricing_controller.state,
                    PricingControllerState::Ready { .. }
                )
        })
    });
    let origin_stream = root
        .read_with(cx, |root, _| {
            root.stream_view.as_ref().map(|(_, stream)| stream.clone())
        })
        .expect("issue67 origin stream");
    let origin_input = origin_stream.read_with(cx, |stream, _| stream.composer_input());
    origin_input.update(cx, |input, cx| input.set_text("keep running", cx));
    window
        .update(cx, |_, window, cx| {
            origin_stream.update(cx, |stream, cx| stream.focus_composer(window, cx))
        })
        .expect("issue67 origin focus");

    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let probe = root.read_with(cx, |root, _| root.agent_worker_start_probe.clone());
    *probe
        .provider_construction_gate
        .lock()
        .expect("issue67 provider gate") = Some((entered_tx, release_rx));
    cx.simulate_keystrokes(window.into(), "cmd-enter");
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| !root.agent_controller.active.is_empty())
    });
    entered_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("issue67 worker reached provider boundary");
    let (generation, cancel) = root.read_with(cx, |root, _| {
        let active = root
            .agent_controller
            .active
            .values()
            .next()
            .expect("issue67 active run");
        assert_eq!(active.thread_id, origin.id);
        assert_eq!(active.stream, origin_stream);
        (active.generation, active.cancel.clone())
    });
    let artifact_a_service = root.read_with(cx, |root, _| {
        let identity = root
            .artifact_controller
            .agent_route(generation, &origin_stream)
            .expect("A capture authority");
        root.artifact_controller
            .route(&identity)
            .expect("A artifact owner")
            .service
            .clone()
    });
    assert!(!cancel.is_cancelled());

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let new_task = visual
        .debug_bounds("sidebar-new-task")
        .expect("issue67 new-task control");
    visual.simulate_click(new_task.center(), Modifiers::default());
    visual.run_until_parked();
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, cx| {
            root.draft.as_ref().is_some_and(|draft| {
                cx.global::<OpenedThread>()
                    .0
                    .as_ref()
                    .is_some_and(|opened| opened.id == draft.id)
                    && root
                        .stream_view
                        .as_ref()
                        .is_some_and(|(id, stream)| id == &draft.id && stream != &origin_stream)
            })
        })
    });
    assert!(
        !cancel.is_cancelled(),
        "opening the real new-task route must not cancel the origin run"
    );

    let draft_stream = root
        .read_with(cx, |root, _| {
            root.stream_view.as_ref().map(|(_, stream)| stream.clone())
        })
        .expect("issue67 draft stream");
    // B's model can change while A's frozen provider/model stays live.
    draft_stream.update(cx, |stream, cx| {
        stream.request_model_selection("gpt-5.6-luna", cx)
    });
    pump_test_app(cx, |cx| {
        draft_stream.read_with(cx, |stream, _| !stream.has_pending_model_selection())
    });
    assert_eq!(
        draft_stream.read_with(cx, |stream, _| stream.displayed_model().to_owned()),
        "gpt-5.6-luna"
    );
    root.update(cx, |root, _| {
        root.agent_provider_override = Some(with_auxiliary_title_fixture(provider_b.clone()))
    });
    let draft_input = draft_stream.read_with(cx, |stream, _| stream.composer_input());
    draft_input.update(cx, |input, cx| {
        input.set_text("destination draft stays", cx)
    });
    window
        .update(cx, |_, window, cx| {
            draft_stream.update(cx, |stream, cx| stream.focus_composer(window, cx))
        })
        .expect("issue67 draft focus");
    let expected_starts = if mode == 4 {
        draft_input.update(cx, |input, cx| {
            input.set_text("@missing-concurrent.txt", cx)
        });
        cx.simulate_keystrokes(window.into(), "cmd-enter");
        pump_test_app(cx, |cx| {
            root.read_with(cx, |root, _| {
                probe.load() == 2
                    && root.agent_controller.active.len() == 1
                    && !root.trusted_actions.is_busy()
            })
        });
        assert!(!cancel.is_cancelled());
        assert!(root.read_with(cx, |root, _| root.agent_controller.matches(
            generation,
            &origin.id,
            &origin_stream
        )));
        assert_eq!(
            draft_input.read_with(cx, |input, _| input.text().to_owned()),
            "@missing-concurrent.txt"
        );
        assert!(provider_b.requests().is_empty());
        draft_input.update(cx, |input, cx| {
            input.set_text("destination draft stays", cx)
        });
        3
    } else {
        2
    };
    let (entered_b_tx, entered_b_rx) = mpsc::sync_channel(1);
    let (release_b_tx, release_b_rx) = mpsc::sync_channel(1);
    *probe.provider_construction_gate.lock().expect("B gate") = Some((entered_b_tx, release_b_rx));
    cx.simulate_keystrokes(window.into(), "cmd-enter");
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| !root.trusted_actions.is_busy())
    });
    for _ in 0..200 {
        cx.executor().advance_clock(DIFF_RESULT_POLL);
        cx.run_until_parked();
        if probe.load() == expected_starts {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        probe.load(),
        expected_starts,
        "B must enter its worker while A is still active"
    );
    entered_b_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("B provider boundary");
    assert!(!cancel.is_cancelled());
    assert!(root.read_with(cx, |root, _| root.agent_controller.matches(
        generation,
        &origin.id,
        &origin_stream
    )));
    let destination = cx.update(|cx| {
        cx.global::<OpenedThread>()
            .0
            .clone()
            .expect("materialized B")
    });
    let (generation_b, cancel_b) = root.read_with(cx, |root, _| {
        assert_eq!(root.agent_controller.active.len(), 2);
        let run = root
            .agent_controller
            .active
            .get(&destination.id)
            .expect("B run");
        assert_eq!(run.stream, draft_stream);
        assert_ne!(run.stream, origin_stream);
        assert_ne!(run.generation, generation);
        (run.generation, run.cancel.clone())
    });
    if mode == 0 {
        let record = |status| ContextCompactionStatusRecord {
            generation: 1,
            status,
            updated_at: 1,
            estimated_tokens: None,
            input_budget: None,
            target_tokens: None,
            source_version: None,
            failure: None,
            usage: ContextCompactionUsageState::Pending,
        };
        let batch = |record| AgentBatch {
            events: vec![ConversationEvent::ContextCompactionStatus { record }],
            mcp_unavailable: vec![],
            reference_failure: None,
            credential_failure: false,
            finished: None,
        };
        root.update(cx, |root, cx| {
            assert!(matches!(
                root.apply_agent_batch_ingress(
                    generation,
                    &origin.id,
                    &origin_stream,
                    batch(record(ContextCompactionStatus::Compacting)),
                    cx
                ),
                AgentBatchIngress::Running
            ));
            assert!(matches!(
                root.apply_agent_batch_ingress(
                    generation_b,
                    &destination.id,
                    &draft_stream,
                    batch(record(ContextCompactionStatus::Compacting)),
                    cx
                ),
                AgentBatchIngress::Running
            ));
            assert!(origin_stream.read(cx).context_operation_busy());
            assert!(draft_stream.read(cx).context_operation_busy());
            root.apply_agent_batch_ingress(
                generation,
                &origin.id,
                &origin_stream,
                batch(record(ContextCompactionStatus::Succeeded)),
                cx,
            );
            assert!(!origin_stream.read(cx).context_operation_busy());
            assert!(draft_stream.read(cx).context_operation_busy());
            assert!(matches!(
                root.apply_agent_batch_ingress(
                    generation,
                    &destination.id,
                    &draft_stream,
                    batch(record(ContextCompactionStatus::Succeeded)),
                    cx
                ),
                AgentBatchIngress::Stale
            ));
            assert!(draft_stream.read(cx).context_operation_busy());
            root.apply_agent_batch_ingress(
                generation_b,
                &destination.id,
                &draft_stream,
                batch(record(ContextCompactionStatus::Succeeded)),
                cx,
            );
        });
    }
    let artifact_b_identity = root.read_with(cx, |root, _| {
        root.artifact_controller
            .agent_route(generation_b, &draft_stream)
            .expect("B capture authority")
    });
    // Same-thread ingress cannot replace the pending payload, stream or token.
    root.update(cx, |root, cx| {
        root.start_agent_run(
            draft_stream.clone(),
            &destination.id,
            PendingAgentRun::UserMessage("duplicate".into()),
            cx,
        )
    });
    assert_eq!(probe.load(), expected_starts);
    assert!(root.read_with(cx, |root, _| root.agent_controller.matches(
        generation_b,
        &destination.id,
        &draft_stream
    )));
    assert_eq!(
        draft_input.read_with(cx, |input, _| input.text().to_owned()),
        "destination draft stays"
    );
    for thread in [&origin, &destination] {
        cx.update(|cx| {
            cx.set_global(OpenedThread(Some(thread.clone())));
            cx.refresh_windows();
        });
        pump_test_app(cx, |cx| {
            root.read_with(cx, |root, _| {
                root.stream_view
                    .as_ref()
                    .is_some_and(|(id, _)| id == &thread.id)
            })
        });
        let expected = if thread.id == origin.id {
            &origin_stream
        } else {
            &draft_stream
        };
        assert!(root.read_with(cx, |root, _| {
            root.stream_view
                .as_ref()
                .is_some_and(|(_, stream)| stream == expected)
        }));
    }
    cx.update(|cx| {
        cx.set_global(SettingsOpen(true));
        cx.refresh_windows();
    });
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| root.settings_view.is_some())
    });
    assert!(!cancel.is_cancelled() && !cancel_b.is_cancelled());
    cx.update(|cx| {
        cx.set_global(SettingsOpen(false));
        cx.refresh_windows();
    });
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| root.settings_view.is_none())
    });
    release_b_tx.send(()).expect("release B");
    release_tx.send(()).expect("release A");
    pump_test_app(cx, |_| {
        !provider.requests().is_empty() && !provider_b.requests().is_empty()
    });
    assert_eq!(provider.requests()[0].model, "gpt-5.6-terra");
    assert_eq!(provider_b.requests()[0].model, "gpt-5.6-luna");
    assert!(root.read_with(cx, |root, _| root.agent_controller.active.len() == 2));
    pump_test_app(cx, |cx| {
        origin_stream.read_with(cx, |stream, _| stream.has_active_agent())
            && draft_stream.read_with(cx, |stream, _| stream.has_active_agent())
            && [&origin.id, &destination.id].iter().all(|id| {
                store.conn().query_row("SELECT EXISTS(SELECT 1 FROM messages WHERE thread_id = ?1 AND role = 'assistant' AND content LIKE '%first%')", [id], |row| row.get::<_, bool>(0)).expect("live partial output")
            })
    });
    assert!(root.read_with(cx, |root, _| root.agent_controller.active.len() == 2));
    for (thread, label, other) in [
        (&origin, "origin first", "destination"),
        (&destination, "destination first", "origin"),
    ] {
        let content: String = store.conn().query_row("SELECT content FROM messages WHERE thread_id = ?1 AND role = 'assistant' ORDER BY seq DESC LIMIT 1", [&thread.id], |row| row.get(0)).expect("interleaved partial");
        assert!(content.contains(label) && !content.contains(other));
    }
    pump_test_app(cx, |cx| {
        !draft_stream.read_with(cx, |stream, _| stream.composer_submission_pending())
    });
    draft_input.update(cx, |input, cx| input.set_text("next B draft", cx));
    window
        .update(cx, |_, window, cx| {
            draft_stream.update(cx, |stream, cx| stream.focus_composer(window, cx))
        })
        .expect("B focus");
    if mode == 5 {
        cx.update(|cx| {
            cx.set_global(SettingsOpen(true));
            cx.refresh_windows();
        });
        pump_test_app(cx, |cx| {
            root.read_with(cx, |root, _| root.settings_view.is_some())
        });
    }
    if mode == 3 {
        window
            .update(cx, |_, window, _| window.remove_window())
            .expect("close window");
        cx.update(|_| drop(root));
        cx.run_until_parked();
        assert!(cancel.is_cancelled() && cancel_b.is_cancelled());
        pump_test_app(cx, |_| {
            let count: i64 = store.conn().query_row("SELECT COUNT(*) FROM messages WHERE role = 'assistant' AND status = 'interrupted'", [], |row| row.get(0)).expect("interrupted count");
            count == 2
        });
        return;
    }
    if mode == 6 {
        pump_test_app(cx, |cx| {
            origin_stream.read_with(cx, |stream, _| stream.has_pending_permission())
                && draft_stream.read_with(cx, |stream, _| stream.has_pending_permission())
        });
        assert_eq!(
            root.read_with(cx, |root, _| root.agent_controller.active.len()),
            2
        );
        cx.update(|cx| {
            cx.set_global(SettingsOpen(true));
            cx.refresh_windows();
        });
        pump_test_app(cx, |cx| {
            !origin_stream.read_with(cx, |stream, _| stream.has_pending_permission())
                && !draft_stream.read_with(cx, |stream, _| stream.has_pending_permission())
        });
        assert!(!cancel.is_cancelled() && !cancel_b.is_cancelled());
        pump_test_app(cx, |cx| {
            root.read_with(cx, |root, _| root.agent_controller.active.is_empty())
        });
        assert!(!repo.path().join("origin-artifact.txt").exists());
        assert!(!repo.path().join("destination-artifact.txt").exists());
        for label in ["origin", "destination"] {
            let state = vega_store::tool_calls::find_state(store.conn(), &format!("{label}-read"))
                .expect("denied tool query")
                .expect("denied tool row");
            assert_eq!(state.status, "rejected");
        }
        assert!(cx.update(|cx| cx.global::<SettingsOpen>().0));
        return;
    }
    // Model the real navigation interval with C's preflight still holding its lease.
    let foreign_preflight = if mode == 2 {
        let mut c = destination.clone();
        c.id = "foreign-preparation".into();
        let c_stream = cx.new(|cx| ConversationStream::new(c, cx));
        Some(root.update(cx, |root, _| {
            let lease = root
                .trusted_actions
                .acquire(TrustedActionKind::AgentPreflight, 0, 0)
                .expect("C preparation lease");
            root.agent_controller.preparation_stream = Some(c_stream);
            lease
        }))
    } else {
        None
    };
    if mode == 1 || mode == 2 {
        let (target, target_stream) = if mode == 1 {
            (&destination, &draft_stream)
        } else {
            (&origin, &origin_stream)
        };
        cx.update(|cx| {
            cx.set_global(OpenedThread(Some(target.clone())));
            cx.refresh_windows();
        });
        pump_test_app(cx, |cx| {
            root.read_with(cx, |root, _| {
                root.stream_view
                    .as_ref()
                    .is_some_and(|(id, _)| id == &target.id)
            })
        });
        target_stream.update(cx, |stream, cx| stream.request_composer_stop(cx));
        cx.run_until_parked();
        assert_eq!(cancel.is_cancelled(), mode == 2);
        assert_eq!(cancel_b.is_cancelled(), mode == 1);
        if let Some(lease) = foreign_preflight {
            root.update(cx, |root, _| {
                assert_eq!(
                    root.trusted_actions.active_token(),
                    Some(lease),
                    "Stop A cannot cancel C preparation"
                );
                assert!(root.trusted_actions.release(lease));
                root.agent_controller.preparation_stream = None;
            });
        }
    } else {
        pump_test_app(cx, |cx| {
            root.read_with(cx, |root, _| {
                !root.agent_controller.active.contains_key(&origin.id)
            })
        });
        assert!(root.read_with(cx, |root, cx| {
            root.agent_controller.active.contains_key(&destination.id)
                && cx
                    .global::<OpenedThread>()
                    .0
                    .as_ref()
                    .is_some_and(|thread| thread.id == destination.id)
                && root
                    .stream_view
                    .as_ref()
                    .is_some_and(|(_, stream)| stream == &draft_stream)
        }));
        assert_eq!(
            draft_input.read_with(cx, |input, _| input.text().to_owned()),
            "next B draft"
        );
        if mode != 5 {
            assert!(
                window
                    .update(cx, |_, window, cx| draft_input
                        .read(cx)
                        .focus_handle(cx)
                        .contains_focused(window, cx))
                    .expect("B focus preserved")
            );
        }
    }
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| root.agent_controller.active.is_empty())
    });
    if mode == 5 {
        assert!(root.read_with(cx, |root, cx| {
            root.settings_view.is_some()
                && cx.global::<SettingsOpen>().0
                && cx
                    .global::<OpenedThread>()
                    .0
                    .as_ref()
                    .is_some_and(|thread| thread.id == destination.id)
        }));
    }
    if mode != 2 {
        pump_test_app(cx, |_| !artifact_a_service.cards().is_empty());
        assert_eq!(
            std::fs::read_to_string(repo.path().join("origin-artifact.txt"))
                .expect("A real artifact"),
            "origin artifact"
        );
        if mode != 1 && mode != 5 {
            assert!(root.read_with(cx, |root, _| {
                root.artifact_controller
                    .active
                    .as_ref()
                    .is_some_and(|route| route.identity == artifact_b_identity)
            }));
        }
        pump_test_app(cx, |cx| {
            root.read_with(cx, |root, _| root.artifact_controller.retained.is_empty())
        });
    }
    for (thread, label, interrupted) in [
        (&origin, "origin", mode == 2),
        (&destination, "destination", mode == 1),
    ] {
        let (status, content): (String, String) = store.conn().query_row("SELECT status, content FROM messages WHERE thread_id = ?1 AND role = 'assistant' ORDER BY seq DESC LIMIT 1", [&thread.id], |row| Ok((row.get(0)?, row.get(1)?))).expect("durable thread terminal");
        assert_eq!(status, if interrupted { "interrupted" } else { "done" });
        assert!(content.contains(label));
        assert!(!content.contains(if label == "origin" {
            "destination"
        } else {
            "origin"
        }));
        if !interrupted {
            let state = vega_store::tool_calls::find_state(store.conn(), &format!("{label}-read"))
                .expect("tool row")
                .expect("owned tool");
            assert_eq!(state.status, "success");
            let (tool_thread, message_thread): (String, String) = store.conn().query_row("SELECT t.thread_id, m.thread_id FROM tool_calls t JOIN messages m ON m.id = t.message_id WHERE t.id = ?1", [format!("{label}-read")], |row| Ok((row.get(0)?, row.get(1)?))).expect("tool ownership");
            assert_eq!(tool_thread, thread.id);
            assert_eq!(message_thread, thread.id);
            let (model, input): (String, i64) = store.conn().query_row("SELECT model, input_tokens FROM token_usage WHERE thread_id = ?1 ORDER BY id DESC LIMIT 1", [&thread.id], |row| Ok((row.get(0)?, row.get(1)?))).expect("usage ownership");
            assert_eq!(model, thread.model);
            assert_eq!(input, if label == "origin" { 31 } else { 59 });
        }
    }
}

#[gpui_kit::test]
async fn issue67_production_routes_keep_one_background_run_and_origin_stream(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.executor().allow_parking();
    let data = tempfile::tempdir().expect("issue67 data root");
    let repo = diff_controller_repo();
    let config_path = data.path().join("config.toml");
    super::model_selection::model_selection_config(&config_path);
    vega_store::keystore::set_key(data.path(), "owned", "issue67-owned-test-key")
        .expect("issue67 credential");
    let database_path = data.path().join("vega.db");
    let store = Store::open(&database_path).expect("issue67 store");
    store.migrate().expect("issue67 migrations");
    let project = vega_store::projects::create(
        store.conn(),
        repo.path().to_str().expect("UTF-8 issue67 repo"),
        "issue67-e2e",
        None,
    )
    .expect("issue67 project");
    let origin = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "gpt-5.6-terra",
        PermissionMode::Confirm.as_str(),
    )
    .and_then(|thread| {
        vega_conversation::threads::rename_thread(&store, &thread.id, "Issue67 Origin")
    })
    .expect("issue67 origin");
    let destination = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "gpt-5.6-terra",
        PermissionMode::Confirm.as_str(),
    )
    .and_then(|thread| {
        vega_conversation::threads::rename_thread(&store, &thread.id, "Issue67 Destination")
    })
    .expect("issue67 destination");
    cx.update(|cx| {
        install_diff_window_globals(
            Store::open(&database_path).expect("issue67 root store"),
            origin.clone(),
            cx,
        )
    });

    let provider = Arc::new(vega_runtime::MockProvider::new(vec![
        vega_runtime::ScriptStep::events(vec![
            vega_runtime::ProviderEvent::TextDelta("background success".into()),
            vega_runtime::ProviderEvent::Done {
                stop_reason: vega_runtime::StopReason::End,
            },
        ]),
    ]));
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, _| {
        root.model_selection_config_override = Some(config_path.clone());
        root.agent_provider_override = Some(with_auxiliary_title_fixture(provider.clone()));
    });
    let window_root = root.clone();
    let window = cx
        .update(|cx| cx.open_window(Default::default(), move |_, _| window_root))
        .expect("issue67 production window");
    cx.update(|cx| crate::app_palette::bind_shortcuts(window.into(), root.downgrade(), cx));
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.stream_view.is_some()
                && root.configured_models.is_some()
                && !root.model_catalog_loading
                && matches!(
                    root.pricing_controller.state,
                    PricingControllerState::Ready { .. }
                )
        })
    });
    let origin_stream = root
        .read_with(cx, |root, _| {
            root.stream_view.as_ref().map(|(_, stream)| stream.clone())
        })
        .expect("issue67 origin stream");
    let origin_input = origin_stream.read_with(cx, |stream, _| stream.composer_input());
    origin_input.update(cx, |input, cx| input.set_text("keep running", cx));
    window
        .update(cx, |_, window, cx| {
            origin_stream.update(cx, |stream, cx| stream.focus_composer(window, cx))
        })
        .expect("issue67 origin focus");

    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let probe = root.read_with(cx, |root, _| root.agent_worker_start_probe.clone());
    *probe
        .provider_construction_gate
        .lock()
        .expect("issue67 provider gate") = Some((entered_tx, release_rx));
    cx.simulate_keystrokes(window.into(), "cmd-enter");
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| !root.agent_controller.active.is_empty())
    });
    entered_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("issue67 worker reached provider boundary");
    let (generation, cancel) = root.read_with(cx, |root, _| {
        let active = root
            .agent_controller
            .active
            .values()
            .next()
            .expect("issue67 active run");
        assert_eq!(active.thread_id, origin.id);
        assert_eq!(active.stream, origin_stream);
        (active.generation, active.cancel.clone())
    });
    assert!(!cancel.is_cancelled());

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let new_task = visual
        .debug_bounds("sidebar-new-task")
        .expect("issue67 new-task control");
    visual.simulate_click(new_task.center(), Modifiers::default());
    visual.run_until_parked();
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, cx| {
            root.draft.as_ref().is_some_and(|draft| {
                cx.global::<OpenedThread>()
                    .0
                    .as_ref()
                    .is_some_and(|opened| opened.id == draft.id)
                    && root
                        .stream_view
                        .as_ref()
                        .is_some_and(|(id, stream)| id == &draft.id && stream != &origin_stream)
            })
        })
    });
    assert!(
        !cancel.is_cancelled(),
        "opening the real new-task route must not cancel the origin run"
    );

    let draft_stream = root
        .read_with(cx, |root, _| {
            root.stream_view.as_ref().map(|(_, stream)| stream.clone())
        })
        .expect("issue67 draft stream");
    let draft_input = draft_stream.read_with(cx, |stream, _| stream.composer_input());
    draft_input.update(cx, |input, cx| {
        input.set_text("destination draft stays", cx)
    });
    window
        .update(cx, |_, window, cx| {
            draft_stream.update(cx, |stream, cx| stream.focus_composer(window, cx))
        })
        .expect("issue67 draft focus");
    assert!(root.read_with(cx, |root, _| root.agent_controller.matches(
        generation,
        &origin.id,
        &origin_stream
    )));
    assert_eq!(probe.load(), 1);

    let open_palette_task = |keys: &str, expected: &Thread, cx: &mut gpui_kit::TestAppContext| {
        cx.simulate_keystrokes(window.into(), keys);
        pump_test_app(cx, |cx| {
            root.read_with(cx, |root, _| {
                root.palette.view.is_some() && !root.palette.search_busy
            })
        });
        cx.simulate_keystrokes(window.into(), "enter");
        pump_test_app(cx, |cx| {
            root.read_with(cx, |root, cx| {
                cx.global::<OpenedThread>()
                    .0
                    .as_ref()
                    .is_some_and(|thread| thread.id == expected.id)
                    && root
                        .stream_view
                        .as_ref()
                        .is_some_and(|(id, _)| id == &expected.id)
            })
        });
    };
    open_palette_task(
        "cmd-k i s s u e 6 7 space d e s t i n a t i o n",
        &destination,
        cx,
    );
    assert!(!cancel.is_cancelled());
    let destination_stream = root
        .read_with(cx, |root, _| {
            root.stream_view.as_ref().map(|(_, stream)| stream.clone())
        })
        .expect("issue67 destination stream");
    assert_ne!(destination_stream, origin_stream);

    open_palette_task("cmd-k i s s u e 6 7 space o r i g i n", &origin, cx);
    assert_eq!(
        root.read_with(cx, |root, _| root
            .stream_view
            .as_ref()
            .map(|(_, stream)| stream.clone())),
        Some(origin_stream.clone()),
        "returning before terminal must remount the exact origin stream"
    );
    assert!(
        origin_stream.read_with(cx, |stream, cx| stream.model_selection_blocked(cx)),
        "the remounted origin stream keeps its live composer state"
    );

    cx.update(|cx| {
        cx.set_global(SettingsOpen(true));
        cx.refresh_windows();
    });
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| root.settings_view.is_some())
    });
    assert!(!cancel.is_cancelled());
    assert!(cx.update(|cx| cx.global::<SettingsOpen>().0));
    window
        .update(cx, |_, window, cx| {
            window.dispatch_action(Box::new(vega_ui::settings::CloseSettings), cx)
        })
        .expect("issue67 close settings");
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, cx| {
            root.settings_view.is_none()
                && !cx.global::<SettingsOpen>().0
                && root
                    .stream_view
                    .as_ref()
                    .is_some_and(|(id, stream)| id == &origin.id && stream == &origin_stream)
        })
    });
    assert!(!cancel.is_cancelled());

    open_palette_task(
        "cmd-k i s s u e 6 7 space d e s t i n a t i o n",
        &destination,
        cx,
    );
    let destination_input = root.read_with(cx, |root, cx| {
        root.stream_view
            .as_ref()
            .expect("issue67 destination remounted")
            .1
            .read(cx)
            .composer_input()
    });
    destination_input.update(cx, |input, cx| input.set_text("route stays here", cx));
    release_tx.send(()).expect("issue67 release provider");
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| root.agent_controller.active.is_empty())
    });
    assert!(!cancel.is_cancelled());
    assert_eq!(provider.requests().len(), 1);
    root.read_with(cx, |root, cx| {
        assert!(
            cx.global::<OpenedThread>()
                .0
                .as_ref()
                .is_some_and(|thread| thread.id == destination.id)
        );
        assert!(
            root.stream_view
                .as_ref()
                .is_some_and(|(id, stream)| id == &destination.id && stream != &origin_stream)
        );
    });
    assert_eq!(
        destination_input.read_with(cx, |input, _| input.text().to_owned()),
        "route stays here"
    );
    let (status, content): (String, String) = store
        .conn()
        .query_row(
            "SELECT status, content FROM messages WHERE thread_id = ?1 AND role = 'assistant' ORDER BY seq DESC LIMIT 1",
            [&origin.id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("issue67 durable background answer");
    assert_eq!(status, "done");
    assert!(content.contains("background success"));

    let (teardown_tx, teardown_rx) = mpsc::sync_channel(1);
    let teardown_thread = destination.clone();
    let teardown_window = cx
        .update(|cx| {
            cx.open_window(Default::default(), move |_, cx| {
                let stream = cx.new(|cx| ConversationStream::new(teardown_thread.clone(), cx));
                cx.new(move |cx| {
                    let mut root = VegaWindow::new(cx);
                    let cancel = root
                        .agent_controller
                        .begin(
                            teardown_thread.id.clone(),
                            stream,
                            Some("teardown owner".into()),
                            None,
                        )
                        .expect("thread admission")
                        .1;
                    teardown_tx.send(cancel).expect("issue67 teardown token");
                    root
                })
            })
        })
        .expect("issue67 teardown window");
    let teardown_cancel = teardown_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("issue67 receive teardown token");
    teardown_window
        .update(cx, |_, window, _| window.remove_window())
        .expect("issue67 remove teardown window");
    cx.run_until_parked();
    assert!(
        teardown_cancel.is_cancelled(),
        "dropping the production window must still cancel its exact active run"
    );
}

#[gpui_kit::test]
async fn production_agent_request_first_keeps_permission_until_proposal_ingress(
    cx: &mut gpui_kit::TestAppContext,
) {
    let repo = diff_controller_repo();
    let data = tempfile::tempdir().expect("permission order data root");
    let database_path = data.path().join("vega.db");
    let store = Store::open(&database_path).expect("permission order store");
    store.migrate().expect("permission order migrations");
    let project = vega_store::projects::create(
        store.conn(),
        repo.path().to_str().expect("UTF-8 permission order repo"),
        "permission-order-e2e",
        None,
    )
    .expect("permission order project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "mock",
        PermissionMode::Confirm.as_str(),
    )
    .expect("permission order thread");
    let queue = vega_conversation::agent::PermissionQueue::new();
    cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));
    let stream = cx
        .new(|cx| ConversationStream::new_with_permission_queue(thread.clone(), queue.clone(), cx));
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, _| {
        root.stream_view = Some((thread.id.clone(), stream.clone()));
    });
    let window_root = root.clone();
    let window = cx
        .update(|cx| {
            cx.open_window(Default::default(), move |_, cx| {
                cx.new(|_| AgentWindowHarness { root: window_root })
            })
        })
        .expect("permission order window");

    let provider = Arc::new(vega_runtime::MockProvider::new_rounds(vec![
        vec![vega_runtime::ScriptStep::events(vec![
            vega_runtime::ProviderEvent::ToolUse {
                id: "permission-order-call".into(),
                name: "bash".into(),
                input_json: r#"{"cmd":"printf once >> permission-order-recorder"}"#.into(),
            },
            vega_runtime::ProviderEvent::Done {
                stop_reason: vega_runtime::StopReason::ToolUse,
            },
        ])],
        vec![vega_runtime::ScriptStep::events(vec![
            vega_runtime::ProviderEvent::TextDelta("finished".into()),
            vega_runtime::ProviderEvent::Done {
                stop_reason: vega_runtime::StopReason::End,
            },
        ])],
    ]));
    let (generation, cancel) = root.update(cx, |root, _| {
        root.agent_controller
            .begin(
                thread.id.clone(),
                stream.clone(),
                Some("request-first".into()),
                None,
            )
            .expect("thread admission")
    });
    let (sender, receiver) = mpsc::sync_channel(AGENT_EVENT_CAPACITY);
    let worker_cancel = cancel.clone();
    let worker_thread = std::thread::spawn({
        let project_path = repo.path().to_path_buf();
        let worker_thread = thread.clone();
        let worker_queue = queue.clone();
        move || {
            run_agent_worker(
                database_path,
                project_path,
                worker_thread,
                PendingAgentRun::UserMessage("request-first".into()),
                worker_queue,
                worker_cancel,
                sender,
                None,
                None,
                None,
                None,
                Some(provider),
                Arc::new(AgentWorkerStartProbe::default()),
            );
        }
    });
    let mut worker = AgentWorkerGuard {
        cancel,
        handle: Some(worker_thread),
    };

    for _ in 0..400 {
        if queue.has_pending() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        queue.has_pending(),
        "the real worker must leave the permission latch pending before ingress"
    );
    cx.run_until_parked();
    assert!(
        queue.has_pending(),
        "a queue listener wake cannot consume a request before its proposal"
    );

    let first_batch = drain_agent_updates(&receiver);
    assert!(first_batch.finished.is_none());
    assert!(first_batch.events.iter().any(|event| matches!(
        event,
        ConversationEvent::ToolCallProposed { call } if call.id == "permission-order-call"
    )));
    assert!(matches!(
        root.update(cx, |root, cx| root.apply_agent_batch_ingress(
            generation,
            &thread.id,
            &stream,
            first_batch,
            cx,
        )),
        AgentBatchIngress::Running
    ));
    cx.run_until_parked();
    assert!(stream.read_with(cx, |stream, _| stream.has_pending_permission()));
    assert!(!repo.path().join("permission-order-recorder").exists());

    cx.simulate_keystrokes(window.into(), "enter");
    let mut terminal = false;
    for _ in 0..400 {
        let batch = drain_agent_updates(&receiver);
        if !batch.events.is_empty() || batch.finished.is_some() {
            terminal = root.update(cx, |root, cx| {
                matches!(
                    root.apply_agent_batch_ingress(generation, &thread.id, &stream, batch, cx),
                    AgentBatchIngress::Finished { .. }
                )
            });
            if terminal {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    if !terminal {
        worker.cancel.cancel();
    }
    worker
        .handle
        .take()
        .expect("permission order worker handle")
        .join()
        .expect("permission order worker");
    assert!(
        terminal,
        "the approved worker must reach the fenced terminal"
    );
    assert_eq!(
        fs::read_to_string(repo.path().join("permission-order-recorder"))
            .expect("approved command recorder"),
        "once"
    );
}

#[gpui_kit::test]
async fn issue90_production_controller_routes_invalid_bash_without_permission_and_restores_it(
    cx: &mut gpui_kit::TestAppContext,
) {
    let repo = diff_controller_repo();
    let data = tempfile::tempdir().expect("issue90 data root");
    let database_path = data.path().join("vega.db");
    let store = Store::open(&database_path).expect("issue90 store");
    store.migrate().expect("issue90 migrations");
    let project = vega_store::projects::create(
        store.conn(),
        repo.path().to_str().expect("UTF-8 issue90 repo"),
        "issue90-e2e",
        None,
    )
    .expect("issue90 project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "mock",
        PermissionMode::Confirm.as_str(),
    )
    .expect("issue90 thread");
    let queue = vega_conversation::agent::PermissionQueue::new();
    cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));
    let stream = cx
        .new(|cx| ConversationStream::new_with_permission_queue(thread.clone(), queue.clone(), cx));
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, _| {
        root.stream_view = Some((thread.id.clone(), stream.clone()));
    });
    let window_root = root.clone();
    let _window = cx
        .update(|cx| {
            cx.open_window(Default::default(), move |_, cx| {
                cx.new(|_| AgentWindowHarness { root: window_root })
            })
        })
        .expect("issue90 window");
    let provider = Arc::new(vega_runtime::MockProvider::new_rounds(vec![
        vec![vega_runtime::ScriptStep::events(vec![
            vega_runtime::ProviderEvent::ToolUse {
                id: "issue90-bash".into(),
                name: "bash".into(),
                input_json: r#"{"command":"printf SECRET_BASH > issue90-marker"}"#.into(),
            },
            vega_runtime::ProviderEvent::Done {
                stop_reason: vega_runtime::StopReason::ToolUse,
            },
        ])],
        vec![vega_runtime::ScriptStep::events(vec![
            vega_runtime::ProviderEvent::TextDelta("corrected".into()),
            vega_runtime::ProviderEvent::Done {
                stop_reason: vega_runtime::StopReason::End,
            },
        ])],
    ]));
    let (generation, cancel) = root.update(cx, |root, _| {
        root.agent_controller
            .begin(
                thread.id.clone(),
                stream.clone(),
                Some("run invalid bash".into()),
                None,
            )
            .expect("thread admission")
    });
    let (sender, receiver) = mpsc::sync_channel(AGENT_EVENT_CAPACITY);
    let worker_cancel = cancel.clone();
    let worker_thread = std::thread::spawn({
        let project_path = repo.path().to_path_buf();
        let worker_thread = thread.clone();
        let worker_queue = queue.clone();
        let worker_provider = provider.clone();
        let worker_database_path = database_path.clone();
        move || {
            run_agent_worker(
                worker_database_path,
                project_path,
                worker_thread,
                PendingAgentRun::UserMessage("run invalid bash".into()),
                worker_queue,
                worker_cancel,
                sender,
                None,
                None,
                None,
                None,
                Some(worker_provider),
                Arc::new(AgentWorkerStartProbe::default()),
            );
        }
    });
    let mut worker = AgentWorkerGuard {
        cancel,
        handle: Some(worker_thread),
    };
    let mut saw_invalid_terminal = false;
    let mut saw_proposal = false;
    let mut finished = false;
    for _ in 0..400 {
        let batch = drain_agent_updates(&receiver);
        saw_invalid_terminal |= batch.events.iter().any(|event| {
            matches!(
                event,
                ConversationEvent::ToolCallFinished { call_id, result }
                    if call_id == "issue90-bash"
                        && result.status == vega_conversation::types::ToolCallStatus::Rejected
                        && result.invalid.is_some()
            )
        });
        saw_proposal |= batch.events.iter().any(|event| {
            matches!(
                event,
                ConversationEvent::ToolCallProposed { call } if call.id == "issue90-bash"
            )
        });
        if !batch.events.is_empty() || batch.finished.is_some() {
            finished = root.update(cx, |root, cx| {
                matches!(
                    root.apply_agent_batch_ingress(generation, &thread.id, &stream, batch, cx),
                    AgentBatchIngress::Finished { success: true, .. }
                )
            });
            if finished {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    if !finished {
        worker.cancel.cancel();
    }
    worker
        .handle
        .take()
        .expect("issue90 worker")
        .join()
        .expect("issue90 join");
    assert!(finished && saw_invalid_terminal && !saw_proposal);
    assert!(!queue.has_pending());
    assert!(!stream.read_with(cx, |stream, _| stream.has_active_permission_card()));
    assert!(!repo.path().join("issue90-marker").exists());
    let feedback = provider.requests()[1]
        .messages
        .iter()
        .find(|message| message.tool_call_id.as_deref() == Some("issue90-bash"))
        .map(|message| message.content.clone());
    assert_eq!(
        feedback.as_deref(),
        Some(vega_runtime::BASH_INVALID_INPUT_OUTPUT)
    );

    let reopened = Store::open(&database_path).expect("issue90 reopened store");
    let history = vega_conversation::history::restart_history_page(&reopened, &thread.id, 20)
        .expect("history");
    assert!(history.entries.iter().any(|entry| matches!(
        entry,
        vega_conversation::history::HistoryEntry::Tool {
            call_id,
            input: None,
            result: Some(vega_conversation::types::ToolCardResultProjection::InvalidRejected {
                tool: vega_conversation::types::InvalidToolKind::Bash,
                ..
            }),
            ..
        } if call_id == "issue90-bash"
    )));
}

#[gpui_kit::test]
async fn production_agent_start_entry_surfaces_write_permission_and_continues(
    cx: &mut gpui_kit::TestAppContext,
) {
    // The production entry owns a dedicated worker thread. Permit its
    // PermissionQueue wakeup to cross the deterministic test scheduler; all
    // app-side assertions still advance through the GPUI pump below.
    cx.executor().allow_parking();
    let repo = diff_controller_repo();
    let config_root = tempfile::tempdir().expect("permission entry config root");
    let config_path = config_root.path().join("config.toml");
    fs::write(
        &config_path,
        r#"[[providers]]
name = "owned"
base_url = "https://provider.invalid/v1"
models = ["gpt-5.6-luna"]
key_ref = "owned"

[defaults]
model = "gpt-5.6-luna"
permission_mode = "confirm"

[ui]
theme = "light"
"#,
    )
    .expect("permission entry config");
    vega_store::keystore::set_key(config_root.path(), "owned", "permission-entry-test-key")
        .expect("permission entry credential");
    let data = tempfile::tempdir().expect("permission entry data root");
    let database_path = data.path().join("vega.db");
    let store = Store::open(&database_path).expect("permission entry store");
    store.migrate().expect("permission entry migrations");
    let project = vega_store::projects::create(
        store.conn(),
        repo.path().to_str().expect("UTF-8 permission entry repo"),
        "permission-entry-e2e",
        None,
    )
    .expect("permission entry project");
    let thread = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        "gpt-5.6-luna",
        PermissionMode::Confirm.as_str(),
    )
    .expect("permission entry thread");
    cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));

    const WRITE_CALL: &str = "permission-entry-write";
    const WRITE_PATH: &str = "permission-entry.txt";
    const WRITE_BODY: &str = "PERMISSION_ENTRY_ONCE";
    let provider = Arc::new(vega_runtime::MockProvider::new_rounds(vec![
        vec![vega_runtime::ScriptStep::events(vec![
            vega_runtime::ProviderEvent::ToolUse {
                id: WRITE_CALL.into(),
                name: "write".into(),
                input_json: format!(r#"{{"path":"{WRITE_PATH}","content":"{WRITE_BODY}"}}"#),
            },
            vega_runtime::ProviderEvent::Done {
                stop_reason: vega_runtime::StopReason::ToolUse,
            },
        ])],
        vec![vega_runtime::ScriptStep::events(vec![
            vega_runtime::ProviderEvent::TextDelta("write completed".into()),
            vega_runtime::ProviderEvent::Done {
                stop_reason: vega_runtime::StopReason::End,
            },
        ])],
    ]));
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, _| {
        // Both paths are owned fixture seams: catalog reads stay off the
        // user's config, while the worker never constructs a real provider.
        root.model_selection_config_override = Some(config_path.clone());
        root.agent_provider_override = Some(with_auxiliary_title_fixture(provider.clone()));
    });
    let window_root = root.clone();
    let window = cx
        .update(|cx| {
            cx.open_window(Default::default(), move |_, cx| {
                cx.new(|_| AgentWindowHarness { root: window_root })
            })
        })
        .expect("permission entry window");

    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.stream_view.is_some()
                && !root.model_catalog_loading
                && root.configured_models.is_some()
                && matches!(
                    root.pricing_controller.state,
                    PricingControllerState::Ready { .. }
                )
        })
    });
    let stream = root
        .read_with(cx, |root, _| {
            root.stream_view.as_ref().map(|(_, stream)| stream.clone())
        })
        .expect("permission entry stream");
    let input = stream.read_with(cx, |stream, _| stream.composer_input());
    input.update(cx, |input, cx| input.set_text("write once", cx));
    let input_focus = input.read_with(cx, |input, cx| input.focus_handle(cx));
    window
        .update(cx, |_, window, cx| window.focus(&input_focus, cx))
        .expect("permission entry composer focus");

    // This key dispatch goes through ConversationStream::submit_message,
    // ComposerSubmitted, VegaWindow::submit_composer and start_agent_run.
    cx.simulate_keystrokes(window.into(), "cmd-enter");
    pump_test_app(cx, |cx| {
        provider.requests().len() == 1
            && root.read_with(cx, |root, _| !root.agent_controller.active.is_empty())
            && stream.read_with(cx, |stream, _| {
                stream.has_pending_permission() && stream.has_active_permission_card()
            })
    });
    assert!(
        !repo.path().join(WRITE_PATH).exists(),
        "the write stays blocked while the PermissionCard is pending"
    );

    // The card's initial focus is the real "允许一次" action for an ordinary
    // write proposal; no queue responder is touched directly by this test.
    cx.simulate_keystrokes(window.into(), "enter");
    pump_test_app(cx, |cx| {
        provider.requests().len() == 2
            && repo.path().join(WRITE_PATH).is_file()
            && root.read_with(cx, |root, _| root.agent_controller.active.is_empty())
    });

    assert_eq!(
        fs::read_to_string(repo.path().join(WRITE_PATH)).expect("approved write output"),
        WRITE_BODY
    );
    let requests = provider.requests();
    assert_eq!(
        requests.len(),
        2,
        "one initial request and one observe round"
    );
    assert!(
        requests[1].messages.iter().any(|message| {
            message.role == vega_runtime::ChatRole::Tool
                && message.tool_call_id.as_deref() == Some(WRITE_CALL)
        }),
        "the next MockProvider request receives the write tool result"
    );

    let observer = Store::open(&database_path).expect("permission entry observer");
    let terminal = vega_store::tool_calls::terminal_results(observer.conn(), &thread.id)
        .expect("permission entry terminal audit");
    assert_eq!(terminal.len(), 1, "the run has one terminal tool audit");
    assert_eq!(
        terminal.get(WRITE_CALL).map(|call| call.status.as_str()),
        Some("success")
    );
    let state = vega_store::tool_calls::find_state(observer.conn(), WRITE_CALL)
        .expect("permission entry tool row")
        .expect("persisted write tool row");
    assert_eq!(state.status, "success");
    let approval =
        ApprovalAudit::from_json(state.approval.as_deref().expect("persisted write approval"))
            .expect("write approval audit");
    assert_eq!(approval.decision, Approval::Once);
    assert_eq!(approval.source, ApprovalSource::User);
    assert!(!stream.read_with(cx, |stream, _| stream.has_active_permission_card()));
}

#[test]
fn reference_rejection_is_carried_by_the_terminal_update() {
    let (sender, receiver) = mpsc::sync_channel(2);
    sender
        .send(AgentUpdate::Finished {
            success: false,
            reference_failure: Some(FileReferenceFailureCode::Missing),
            credential_failure: false,
        })
        .expect("terminal rejection send");
    let batch = drain_agent_updates(&receiver);
    assert_eq!(batch.finished, Some(false));
    assert_eq!(
        batch.reference_failure,
        Some(FileReferenceFailureCode::Missing)
    );
}

#[test]
fn finished_refresh_routes_only_to_matching_current_thread_cache() {
    assert!(current_cache_matches(Some("a"), Some("a"), "a"));
    assert!(
        !current_cache_matches(Some("b"), Some("a"), "a"),
        "A→B must not overwrite B's OpenedThread"
    );
    assert!(
        !current_cache_matches(Some("a"), Some("b"), "a"),
        "a stale cache cannot receive A's authoritative refresh"
    );
    assert!(
        current_cache_matches(Some("a"), Some("a"), "a"),
        "A→B→A must refresh the rebuilt A entity"
    );
}

#[gpui_kit::test]
async fn cancellation_keeps_active_until_durable_handshake_finishes(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(|cx| {
        cx.set_global(Theme::light());
        cx.set_global(SettingsOpen(false));
        vega_ui::init(cx);
    });
    let (store, thread_id) = pending_plan();
    let thread =
        vega_conversation::threads::open_thread(&store, &thread_id).expect("thread projection");
    let stream = cx.new(|cx| ConversationStream::new(thread, cx));
    let mut controller = AppAgentController::default();
    let (generation, cancel) = controller
        .begin(
            thread_id.clone(),
            stream.clone(),
            Some("draft".into()),
            None,
        )
        .expect("thread admission");
    controller.request_active_cancel(&thread_id);
    assert!(cancel.is_cancelled());
    assert!(!controller.active.is_empty());
    assert_eq!(
        controller.accept_durable_start(generation + 1, &thread_id, &stream),
        None
    );
    assert_eq!(
        controller.accept_durable_start(generation, &thread_id, &stream),
        Some("draft".into())
    );
    assert_eq!(
        controller.accept_durable_start(generation, &thread_id, &stream),
        None
    );
    assert!(
        controller
            .finish(generation + 1, &thread_id, &stream)
            .is_none()
    );
    assert!(!controller.active.is_empty());
    let finished = controller
        .finish(generation, &thread_id, &stream)
        .expect("exact terminal owns active run");
    assert!(finished.pending_user_content.is_none());
    assert!(controller.active.is_empty());

    let (next_generation, next_cancel) = controller
        .begin(
            thread_id.clone(),
            stream.clone(),
            Some("second".into()),
            None,
        )
        .expect("thread admission");
    assert_eq!(
        controller.accept_durable_start(next_generation, &thread_id, &stream),
        Some("second".into())
    );
    controller.request_active_cancel(&thread_id);
    assert!(next_cancel.is_cancelled());
    assert!(
        controller
            .finish(next_generation, &thread_id, &stream)
            .is_some()
    );

    let (prestart_generation, prestart_cancel) = controller
        .begin(
            thread_id.clone(),
            stream.clone(),
            Some("retryable".into()),
            None,
        )
        .expect("thread admission");
    controller.request_active_cancel(&thread_id);
    assert!(prestart_cancel.is_cancelled());
    let prestart = controller
        .finish(prestart_generation, &thread_id, &stream)
        .expect("cancelled pre-start worker still reaches terminal");
    assert_eq!(prestart.pending_user_content, Some("retryable".into()));

    let (approved_generation, _) = controller
        .begin(
            thread_id.clone(),
            stream.clone(),
            None,
            Some("approved-instruction".into()),
        )
        .expect("thread admission");
    let approved = controller
        .finish(approved_generation, &thread_id, &stream)
        .expect("approved pre-start failure reaches terminal");
    assert_eq!(
        approved.pending_approved_instruction.as_deref(),
        Some("approved-instruction")
    );
}

#[gpui_kit::test]
async fn stop_resume_fences_drop_every_late_callback_per_c5_fence_class(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(|cx| {
        cx.set_global(Theme::light());
        cx.set_global(SettingsOpen(false));
        vega_ui::init(cx);
    });
    let (store, thread_id) = pending_plan();
    let thread =
        vega_conversation::threads::open_thread(&store, &thread_id).expect("thread projection");
    let other_thread = vega_conversation::threads::create_thread(
        &store,
        &thread.project_id,
        "mock",
        PermissionMode::Confirm.as_str(),
    )
    .expect("second thread for the run fence");
    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let other_stream = cx.new(|cx| ConversationStream::new(other_thread.clone(), cx));
    let mut controller = AppAgentController::default();
    let (generation, cancel) = controller
        .begin(
            thread_id.clone(),
            stream.clone(),
            Some("draft".into()),
            None,
        )
        .expect("thread admission");

    // Stop is visible first-wins: the token is cancelled, the run stays
    // owned until the durable handshake, and every fenced lookup fails.
    controller.request_active_cancel(&thread_id);
    assert!(cancel.is_cancelled());

    // 1) generation fence: a stale/foreign generation is refused.
    assert_eq!(
        controller.accept_durable_start(generation + 7, &thread_id, &stream),
        None
    );
    controller.observe_terminal_message(
        generation + 7,
        &thread_id,
        &stream,
        &ConversationEvent::Interrupted {
            message_id: "late-message".into(),
        },
    );
    assert!(
        controller
            .active
            .values()
            .next()
            .expect("stale observe must not consume the run")
            .terminal_message_id
            .is_none(),
        "stale-generation terminal observation is dropped"
    );
    assert!(
        controller
            .finish(generation + 7, &thread_id, &stream)
            .is_none()
    );

    // 2) run fence: same generation, wrong thread id is refused.
    assert_eq!(
        controller.accept_durable_start(generation, &other_thread.id, &stream),
        None
    );
    assert!(
        controller
            .finish(generation, &other_thread.id, &stream)
            .is_none()
    );

    // 3) route fence: same generation+thread, wrong stream is refused.
    assert_eq!(
        controller.accept_durable_start(generation, &thread_id, &other_stream),
        None
    );
    assert!(
        controller
            .finish(generation, &thread_id, &other_stream)
            .is_none()
    );

    // The exact run consumes the durable start exactly once and owns the
    // terminal observation.
    assert_eq!(
        controller.accept_durable_start(generation, &thread_id, &stream),
        Some("draft".into())
    );
    assert_eq!(
        controller.accept_durable_start(generation, &thread_id, &stream),
        None
    );
    controller.observe_terminal_message(
        generation,
        &thread_id,
        &stream,
        &ConversationEvent::Interrupted {
            message_id: "terminal-message".into(),
        },
    );
    assert_eq!(
        controller
            .active
            .values()
            .next()
            .expect("owned run")
            .terminal_message_id
            .as_deref(),
        Some("terminal-message")
    );

    // 4) terminal fence: after the exact finish, every late callback for
    // the finished run is refused (no double consume, no late start).
    let finished = controller
        .finish(generation, &thread_id, &stream)
        .expect("exact terminal owns the run");
    assert!(finished.pending_user_content.is_none());
    assert!(controller.active.is_empty());
    assert_eq!(
        controller.accept_durable_start(generation, &thread_id, &stream),
        None
    );
    assert!(controller.finish(generation, &thread_id, &stream).is_none());

    // Resume: a new generation with a fresh token; the previous run's
    // cancelled token stays cancelled and cannot bleed into the new run.
    let (next_generation, next_cancel) = controller
        .begin(
            thread_id.clone(),
            stream.clone(),
            Some("resumed".into()),
            None,
        )
        .expect("thread admission");
    assert_ne!(next_generation, generation);
    assert!(cancel.is_cancelled());
    assert!(!next_cancel.is_cancelled());
    assert_eq!(
        controller.accept_durable_start(next_generation, &thread_id, &stream),
        Some("resumed".into())
    );

    // Window/route cache fence: a stale cache cannot receive another
    // thread's authoritative refresh (A→B switch invalidates A).
    assert!(!current_cache_matches(
        Some(&other_thread.id),
        Some(&thread_id),
        &thread_id
    ));
    assert!(current_cache_matches(
        Some(&thread_id),
        Some(&thread_id),
        &thread_id
    ));
}

#[test]
fn deferred_provider_construction_cancel_starts_no_request_or_durable_message() {
    let repo = diff_controller_repo();
    let data = tempfile::tempdir().expect("owned preparation data");
    let database_path = data.path().join("vega.db");
    let store = Store::open(&database_path).expect("owned preparation store");
    store.migrate().expect("owned preparation migrations");
    let project = vega_store::projects::create(
        store.conn(),
        repo.path().to_str().expect("fixture path"),
        "prepare",
        None,
    )
    .expect("project");
    let thread = vega_conversation::threads::create_thread(&store, &project.id, "mock", "confirm")
        .expect("thread");
    let provider = Arc::new(vega_runtime::MockProvider::new_rounds(vec![vec![
        vega_runtime::ScriptStep::events(vec![
            vega_runtime::ProviderEvent::TextDelta("fixture response".into()),
            vega_runtime::ProviderEvent::Done {
                stop_reason: vega_runtime::StopReason::End,
            },
        ]),
    ]]));
    let (entered_sender, entered_receiver) = mpsc::sync_channel(1);
    let (release_sender, release_receiver) = mpsc::sync_channel(1);
    let probe = Arc::new(AgentWorkerStartProbe::default());
    *probe.provider_construction_gate.lock().expect("gate") =
        Some((entered_sender, release_receiver));
    let cancel = tokio_util::sync::CancellationToken::new();
    let (sender, receiver) = mpsc::sync_channel(AGENT_EVENT_CAPACITY);
    let worker = std::thread::spawn({
        let project_path = repo.path().to_path_buf();
        let database_path = database_path.clone();
        let thread = thread.clone();
        let cancel = cancel.clone();
        let provider = provider.clone();
        move || {
            run_agent_worker(
                database_path,
                project_path,
                thread,
                PendingAgentRun::UserMessage("owned pending draft".into()),
                vega_conversation::agent::PermissionQueue::new(),
                cancel,
                sender,
                None,
                None,
                None,
                None,
                Some(provider),
                probe,
            )
        }
    });
    entered_receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("worker reached deferred provider construction");
    cancel.cancel();
    release_sender.send(()).expect("return from construction");
    let update = receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("bounded terminal");
    assert!(
        matches!(
            update,
            AgentUpdate::Finished {
                success: false,
                reference_failure: None,
                credential_failure: false,
            }
        ),
        "no durable event may precede cancellation terminal"
    );
    worker.join().expect("preparation worker");
    assert!(
        provider.requests().is_empty(),
        "late provider construction must not send"
    );
    let count: i64 = store
        .conn()
        .query_row(
            "SELECT count(*) FROM messages WHERE thread_id = ?1",
            [&thread.id],
            |row| row.get(0),
        )
        .expect("owned message count");
    assert_eq!(
        count, 0,
        "cancellation before runtime must not create a local echo or assistant row"
    );
}

#[test]
fn local_credential_preparation_failure_has_no_durable_messages() {
    let root = tempfile::tempdir().expect("owned fixture");
    let database_path = root.path().join("store.db");
    let store = Store::open(&database_path).expect("store");
    store.migrate().expect("migrate");
    let project = vega_store::projects::create(
        store.conn(),
        root.path().to_str().expect("path"),
        "owned",
        None,
    )
    .expect("project");
    let thread =
        vega_conversation::threads::create_thread(&store, &project.id, "owned-model", "confirm")
            .expect("thread");
    let config_path = root.path().join("config.toml");
    let mut config = vega_store::config::AppConfig::default();
    config.providers.push(vega_store::config::ProviderConfig {
        enabled: true,
        name: "owned".into(),
        base_url: "https://owned.invalid/v1".into(),
        models: vec!["owned-model".into()],
        key_ref: "owned".into(),
    });
    config.save_to(&config_path).expect("owned config");
    for unreadable in [false, true] {
        if unreadable {
            vega_store::keystore::set_key(root.path(), "owned", "owned-test-value").unwrap();
            std::fs::write(
                root.path().join("credentials/credentials.toml"),
                "invalid TOML",
            )
            .unwrap();
        }
        let (sender, receiver) = mpsc::sync_channel(AGENT_EVENT_CAPACITY);
        run_agent_worker(
            database_path.clone(),
            root.path().into(),
            thread.clone(),
            PendingAgentRun::UserMessage("draft survives missing credentials".into()),
            vega_conversation::agent::PermissionQueue::new(),
            tokio_util::sync::CancellationToken::new(),
            sender,
            None,
            Some(config_path.clone()),
            None,
            None,
            None,
            Arc::new(AgentWorkerStartProbe::default()),
        );
        assert!(matches!(
            receiver.recv_timeout(Duration::from_secs(5)).unwrap(),
            AgentUpdate::Finished {
                success: false,
                reference_failure: None,
                credential_failure: true,
            }
        ));
        let messages: i64 = store
            .conn()
            .query_row(
                "SELECT count(*) FROM messages WHERE thread_id = ?1",
                [&thread.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(messages, 0, "production prep must stop before durable run");
        assert!(commit_provider(&thread, Some(&config_path)).is_err());
    }
}
