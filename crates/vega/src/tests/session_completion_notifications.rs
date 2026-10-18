use super::*;
use crate::thread_reload::reload_thread_state;
use gpui_kit::{SystemNotification, SystemNotificationResponse, VisualTestContext, WindowHandle};
use vega_ui::conversation_stream::ComposerStopRequested;
use vega_ui::plan_card::PlanReviewRequested;
use vega_ui::sidebar::{SelectedProject, thread_is_running};

struct Fixture {
    _directory: TempDir,
    store: Store,
    a: Thread,
    b: Thread,
    root: Entity<VegaWindow>,
    stream: Entity<ConversationStream>,
    window: WindowHandle<VegaWindow>,
}

fn fixture(cx: &mut gpui_kit::TestAppContext, bind: bool) -> Fixture {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");
    super::model_selection::model_selection_config(&config);
    let database = directory.path().join("notifications.db");
    let store = Store::open(&database).unwrap();
    store.migrate().unwrap();
    let project = vega_store::projects::create(
        store.conn(),
        directory.path().to_str().unwrap(),
        "Synthetic",
        None,
    )
    .unwrap();
    let mut a =
        vega_conversation::threads::create_thread(&store, &project.id, "gpt-5.6-terra", "confirm")
            .unwrap();
    let mut b =
        vega_conversation::threads::create_thread(&store, &project.id, "gpt-5.6-terra", "confirm")
            .unwrap();
    for (thread, title) in [(&mut a, "  A task  "), (&mut b, "B task")] {
        vega_conversation::threads::update_thread(
            &store,
            &thread.id,
            &ThreadUpdate {
                title: Some(title.into()),
                ..Default::default()
            },
        )
        .unwrap();
        thread.title = title.into();
    }
    cx.update(|cx| install_diff_window_globals(Store::open(&database).unwrap(), a.clone(), cx));
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, _| {
        root.model_selection_config_override = Some(config)
    });
    let view = root.clone();
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), move |_, _| view)
            .unwrap()
    });
    if bind {
        cx.update(|cx| crate::app_notifications::bind(window, root.downgrade(), cx));
    }
    window
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            root.stream_view.is_some() && root.agent_controller.queue_operations.is_empty()
        })
    });
    let stream = root.read_with(cx, |root, _| root.stream_view.as_ref().unwrap().1.clone());
    Fixture {
        _directory: directory,
        store,
        a,
        b,
        root,
        stream,
        window,
    }
}

fn batch(events: Vec<ConversationEvent>, finished: Option<bool>) -> AgentBatch {
    AgentBatch {
        composer_prediction: None,
        subagents: Vec::new(),
        events,
        codex_mode_confirmed: false,
        computer_use_ready: None,
        mcp_unavailable: Vec::new(),
        reference_failure: None,
        credential_failure: false,
        finished,
    }
}

fn started(message: &str, seq: u64) -> ConversationEvent {
    ConversationEvent::MessageStarted {
        message_id: message.into(),
        seq,
    }
}

fn text(message: &str, content: &str) -> ConversationEvent {
    ConversationEvent::TextDelta {
        message_id: message.into(),
        delta: content.into(),
    }
}

fn ended(message: &str, reason: ConversationStopReason) -> ConversationEvent {
    ConversationEvent::MessageFinished {
        message_id: message.into(),
        stop_reason: reason,
        execution_duration_ms: Some(7),
    }
}

fn interrupted(message: &str) -> ConversationEvent {
    ConversationEvent::Interrupted {
        message_id: message.into(),
        execution_duration_ms: Some(7),
    }
}

impl Fixture {
    fn row(&self, id: &str) -> vega_store::threads::ThreadRow {
        vega_store::threads::find(self.store.conn(), id)
            .unwrap()
            .unwrap()
    }

    fn route(&self, a: bool, active: bool, settings: bool, cx: &mut gpui_kit::TestAppContext) {
        let mut thread = if a { self.a.clone() } else { self.b.clone() };
        thread.unread = self.row(&thread.id).unread;
        cx.update(|cx| {
            cx.set_global(SelectedProject(Some(thread.project_id.clone())));
            cx.set_global(OpenedThread(Some(thread)));
            cx.set_global(SettingsOpen(settings));
            cx.refresh_windows();
        });
        if active {
            self.window
                .update(cx, |_, window, _| window.activate_window())
                .unwrap();
        } else {
            VisualTestContext::from_window(self.window.into(), cx).deactivate_window();
        }
        pump_test_app(cx, |cx| {
            self.root.read_with(cx, |root, _| {
                root.agent_controller.queue_operations.is_empty()
            })
        });
        assert_eq!(
            self.window
                .update(cx, |_, window, _| window.is_window_active())
                .unwrap(),
            active
        );
    }

    fn begin(&self, cx: &mut gpui_kit::TestAppContext) -> (u64, String) {
        let generation = self.root.update(cx, |root, _| {
            root.agent_controller
                .begin(self.a.id.clone(), self.stream.clone(), None, None)
                .unwrap()
                .0
        });
        self.stream
            .update(cx, ConversationStream::begin_composer_run);
        cx.update(|cx| vega_ui::sidebar::set_thread_running(&self.a.id, true, cx));
        let message = format!("assistant-{generation}");
        assert!(!self.ingress(
            generation,
            vec![started(&message, generation * 2)],
            None,
            cx
        ));
        assert!(self.root.read_with(cx, |root, _| {
            root.agent_controller.active.contains_key(&self.a.id)
        }));
        assert!(cx.read(|cx| thread_is_running(&self.a.id, cx)));
        (generation, message)
    }

    fn ingress(
        &self,
        generation: u64,
        events: Vec<ConversationEvent>,
        finished: Option<bool>,
        cx: &mut gpui_kit::TestAppContext,
    ) -> bool {
        self.root.update(cx, |root, cx| {
            match root.apply_agent_batch_ingress(
                generation,
                &self.a.id,
                &self.stream,
                batch(events, finished),
                cx,
            ) {
                AgentBatchIngress::Finished { run, .. } => {
                    self.stream.update(cx, |stream, cx| {
                        stream.finish_composer_run(run.cancel.is_cancelled(), cx)
                    });
                    true
                }
                AgentBatchIngress::Running | AgentBatchIngress::Stale => false,
            }
        })
    }

    fn complete(&self, content: &str, cx: &mut gpui_kit::TestAppContext) -> u64 {
        let (generation, message) = self.begin(cx);
        assert!(self.ingress(
            generation,
            vec![
                text(&message, content),
                ended(&message, ConversationStopReason::End)
            ],
            Some(true),
            cx
        ));
        self.assert_stopped(cx);
        generation
    }

    fn assert_stopped(&self, cx: &gpui_kit::TestAppContext) {
        assert!(self.root.read_with(cx, |root, _| {
            !root.agent_controller.active.contains_key(&self.a.id)
        }));
        assert!(!cx.read(|cx| thread_is_running(&self.a.id, cx)));
        assert!(
            !self
                .stream
                .read_with(cx, |stream, _| stream.has_active_agent())
        );
    }

    fn visit_a(&self, cx: &mut gpui_kit::TestAppContext) {
        let mut a = self.a.clone();
        a.unread = self.row(&self.a.id).unread;
        assert!(
            self.root
                .update(cx, |root, cx| root.accept_palette_thread(a, cx))
        );
    }

    fn message_count(&self) -> i64 {
        self.store
            .conn()
            .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
            .unwrap()
    }

    fn click(&self, notification: &SystemNotification, cx: &mut gpui_kit::TestAppContext) {
        cx.simulate_system_notification_response(SystemNotificationResponse {
            tag: notification.tag.clone(),
            action_id: None,
        });
        pump_test_app(cx, |cx| {
            cx.read(|cx| {
                cx.global::<OpenedThread>()
                    .0
                    .as_ref()
                    .is_some_and(|thread| thread.id == self.a.id)
                    && !cx.global::<SettingsOpen>().0
            })
        });
    }
}

#[gpui_kit::test]
async fn terminal_attention_follows_actual_route_window_activation_and_outcome(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx, true);
    assert_eq!(cx.app_identity(), Some(("ai.vega".into(), "Vega".into())));
    for active in [true, false] {
        for current_a in [true, false] {
            for outcome in [
                "end",
                "error",
                "interrupted",
                "length",
                "turn-limit",
                "failed-worker",
            ] {
                vega_conversation::threads::visit_thread(&f.store, &f.a.id).unwrap();
                f.route(current_a, active, false, cx);
                let before = f.row(&f.a.id);
                let b_before = f.row(&f.b.id);
                let shown = cx.shown_system_notifications().len();
                let (generation, message) = f.begin(cx);
                let terminal = match outcome {
                    "end" => Some(ended(&message, ConversationStopReason::End)),
                    "error" => Some(ConversationEvent::Error {
                        message_id: Some(message.clone()),
                        error: Arc::new(vega_runtime::VegaError::Cancelled),
                        execution_duration_ms: Some(7),
                    }),
                    "interrupted" => Some(interrupted(&message)),
                    "length" => Some(ended(&message, ConversationStopReason::Length)),
                    "turn-limit" => Some(ended(&message, ConversationStopReason::TurnLimit)),
                    "failed-worker" => None,
                    _ => unreachable!(),
                };
                let mut events = vec![text(&message, "synthetic result")];
                events.extend(terminal);
                assert!(f.ingress(
                    generation,
                    events,
                    Some(!matches!(outcome, "error" | "failed-worker")),
                    cx
                ));
                f.assert_stopped(cx);
                let background = !active || !current_a;
                assert_eq!(
                    f.row(&f.a.id).unread,
                    background,
                    "{active} {current_a} {outcome}"
                );
                assert_eq!(f.row(&f.a.id).updated_at, before.updated_at);
                assert_eq!(f.row(&f.b.id), b_before);
                assert_eq!(
                    cx.shown_system_notifications().len(),
                    shown + usize::from(background)
                );
                if background {
                    let notice = cx.shown_system_notifications().pop().unwrap();
                    assert_eq!(
                        notice.tag.as_ref(),
                        format!("vega-run-{}-{generation}", f.a.id)
                    );
                    assert_eq!(notice.title.as_ref(), "A task");
                    assert!(notice.body.contains("synthetic result"));
                    match outcome {
                        "end" => assert_eq!(notice.body.as_ref(), "synthetic result"),
                        "error" | "failed-worker" => assert!(notice.body.starts_with("任务失败")),
                        _ => assert!(notice.body.starts_with("任务已中断")),
                    }
                }
            }
        }
    }
}

#[gpui_kit::test]
async fn settings_hides_a_cached_session_and_activation_only_visits_visible_results(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx, true);
    f.route(true, false, true, cx);
    f.complete("settings result", cx);
    assert!(f.row(&f.a.id).unread);
    assert_eq!(
        cx.shown_system_notifications()
            .last()
            .unwrap()
            .body
            .as_ref(),
        "settings result"
    );
    f.window
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    cx.run_until_parked();
    assert!(f.row(&f.a.id).unread);
    cx.update(|cx| {
        cx.set_global(SettingsOpen(false));
        cx.refresh_windows();
    });
    pump_test_app(cx, |_| !f.row(&f.a.id).unread);
    vega_conversation::threads::update_thread(
        &f.store,
        &f.b.id,
        &ThreadUpdate {
            unread: Some(true),
            ..Default::default()
        },
    )
    .unwrap();
    f.route(true, false, false, cx);
    f.complete("background result", cx);
    assert!(f.row(&f.a.id).unread);
    f.window
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    pump_test_app(cx, |_| !f.row(&f.a.id).unread);
    assert!(f.row(&f.b.id).unread);
}

#[gpui_kit::test]
async fn explicit_stop_suppresses_notification_without_hiding_background_unread(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx, true);
    for background in [false, true] {
        vega_conversation::threads::visit_thread(&f.store, &f.a.id).unwrap();
        f.route(true, true, false, cx);
        let (generation, message) = f.begin(cx);
        f.ingress(generation, vec![text(&message, "partial result")], None, cx);
        f.root.update(cx, |root, cx| {
            root.stop_composer(
                f.stream.clone(),
                &ComposerStopRequested {
                    thread_id: f.a.id.clone(),
                },
                cx,
            )
        });
        if background {
            f.route(false, true, false, cx);
        }
        assert!(f.ingress(generation, vec![interrupted(&message)], Some(true), cx));
        f.assert_stopped(cx);
        assert_eq!(f.row(&f.a.id).unread, background);
        assert!(cx.shown_system_notifications().is_empty());
    }
}

#[gpui_kit::test]
async fn review_handoff_stays_quiet_and_the_following_run_can_notify(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx, true);
    let (generation, message) = f.begin(cx);
    f.ingress(
        generation,
        vec![text(&message, "plan transition")],
        None,
        cx,
    );
    let review = PlanReviewRequested {
        thread_id: f.a.id.clone(),
        plan_id: "synthetic-plan".into(),
        action: PlanReviewAction::Approve,
    };
    f.root.update(cx, |root, cx| {
        root.review_plan(f.stream.clone(), &review, cx)
    });
    assert!(f.root.read_with(cx, |root, _| {
        root.agent_controller.pending_review.contains_key(&f.a.id)
    }));
    f.route(false, true, false, cx);
    assert!(f.ingress(generation, vec![interrupted(&message)], Some(true), cx));
    f.assert_stopped(cx);
    assert!(!f.row(&f.a.id).unread);
    assert!(cx.shown_system_notifications().is_empty());
    f.root.update(cx, |root, _| {
        root.agent_controller.pending_review.remove(&f.a.id);
    });
    f.complete("approved execution result", cx);
    assert!(f.row(&f.a.id).unread);
    assert_eq!(
        cx.shown_system_notifications()
            .last()
            .unwrap()
            .body
            .as_ref(),
        "approved execution result"
    );
}

#[gpui_kit::test]
async fn delegated_child_does_not_emit_top_level_completion_attention(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx, true);
    f.route(false, true, false, cx);
    let (generation, message) = f.begin(cx);
    f.root.update(cx, |root, _| {
        let run = root.agent_controller.active.get_mut(&f.a.id).unwrap();
        run.delegated_parent_thread = Some(f.b.id.clone());
        run.delegated_message_id = Some(message.clone());
    });
    assert!(f.ingress(
        generation,
        vec![
            text(&message, "child result"),
            ended(&message, ConversationStopReason::End)
        ],
        Some(true),
        cx
    ));
    f.assert_stopped(cx);
    assert!(!f.row(&f.a.id).unread);
    assert!(cx.shown_system_notifications().is_empty());
}

#[gpui_kit::test]
async fn duplicate_and_stale_terminals_cannot_notify_again_or_stop_a_new_run(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx, true);
    f.route(false, true, false, cx);
    let first = f.complete("first result", cx);
    assert!(!f.ingress(first, Vec::new(), Some(true), cx));
    assert_eq!(cx.shown_system_notifications().len(), 1);
    let (second, message) = f.begin(cx);
    assert!(!f.ingress(
        first,
        vec![text("assistant-stale", "old output")],
        Some(false),
        cx
    ));
    assert!(f.root.read_with(cx, |root, _| {
        root.agent_controller
            .active
            .get(&f.a.id)
            .is_some_and(|run| run.generation == second)
    }));
    assert!(cx.read(|cx| thread_is_running(&f.a.id, cx)));
    assert!(f.ingress(
        second,
        vec![
            text(&message, "second result"),
            ended(&message, ConversationStopReason::End)
        ],
        Some(true),
        cx
    ));
    f.assert_stopped(cx);
    let shown = cx.shown_system_notifications();
    assert_eq!(shown.len(), 2);
    assert_ne!(shown[0].tag, shown[1].tag);
    assert_eq!(shown[0].body.as_ref(), "first result");
    assert_eq!(shown[1].body.as_ref(), "second result");
}

#[gpui_kit::test]
async fn steering_final_notification_belongs_to_the_last_message_in_this_run(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx, true);
    f.route(false, true, false, cx);
    let (generation, message) = f.begin(cx);
    assert!(f.ingress(
        generation,
        vec![
            text(&message, "before steering"),
            ended(&message, ConversationStopReason::End),
            started("steered-message", 20),
            text("steered-message", "steered progress"),
            ConversationEvent::AssistantResponseCompleted {
                message_id: "steered-message".into(),
                response: AssistantResponseMetadata {
                    response_id: "steered-commentary".into(),
                    text_start_bytes: 0,
                    text_end_bytes: "steered progress".len(),
                    kind: AssistantResponseKind::Commentary,
                },
            },
            text("steered-message", "after steering"),
            ConversationEvent::AssistantResponseCompleted {
                message_id: "steered-message".into(),
                response: AssistantResponseMetadata {
                    response_id: "steered-final".into(),
                    text_start_bytes: "steered progress".len(),
                    text_end_bytes: "steered progressafter steering".len(),
                    kind: AssistantResponseKind::Final,
                },
            },
            ended("steered-message", ConversationStopReason::End)
        ],
        Some(true),
        cx
    ));
    f.assert_stopped(cx);
    assert!(f.row(&f.a.id).unread);
    assert_eq!(
        cx.shown_system_notifications()
            .last()
            .unwrap()
            .body
            .as_ref(),
        "after steering"
    );
}

#[gpui_kit::test]
async fn outputless_or_internal_only_rounds_never_reuse_a_previous_answer(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx, true);
    f.route(false, true, false, cx);
    f.complete("historical answer", cx);
    for kind in ["empty", "thinking", "tool", "commentary", "whitespace"] {
        let (generation, message) = f.begin(cx);
        let mut events = match kind {
            "thinking" => vec![ConversationEvent::ThinkingDelta {
                message_id: message.clone(),
                delta: "private thinking".into(),
            }],
            "tool" => vec![ConversationEvent::ToolCallProposed {
                call: ToolCall {
                    id: format!("tool-{generation}"),
                    tool: "bash".into(),
                    input_json: r#"{"command":"synthetic"}"#.into(),
                },
            }],
            "commentary" => vec![
                text(&message, "progress"),
                ConversationEvent::AssistantResponseCompleted {
                    message_id: message.clone(),
                    response: AssistantResponseMetadata {
                        response_id: format!("response-{generation}"),
                        text_start_bytes: 0,
                        text_end_bytes: "progress".len(),
                        kind: AssistantResponseKind::Commentary,
                    },
                },
            ],
            "whitespace" => vec![text(&message, " \n\t ")],
            _ => Vec::new(),
        };
        events.push(ended(&message, ConversationStopReason::End));
        assert!(f.ingress(generation, events, Some(true), cx));
        f.assert_stopped(cx);
        assert!(f.row(&f.a.id).unread);
        assert_eq!(cx.shown_system_notifications().len(), 1, "{kind}");
    }
}

#[gpui_kit::test]
async fn notification_text_is_single_line_bounded_and_uses_a_title_fallback(
    cx: &mut gpui_kit::TestAppContext,
) {
    let mut f = fixture(cx, true);
    f.a.title = " \t ".into();
    vega_conversation::threads::update_thread(
        &f.store,
        &f.a.id,
        &ThreadUpdate {
            title: Some(f.a.title.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    f.stream
        .update(cx, |stream, cx| stream.apply_thread(f.a.clone(), cx));
    f.route(false, true, false, cx);
    f.complete("答案\n  中文\t😀\0尾", cx);
    let first = cx.shown_system_notifications().pop().unwrap();
    assert_eq!(first.title.as_ref(), "新任务");
    assert_eq!(first.body.as_ref(), "答案 中文 😀尾");
    f.complete(&"中😀".repeat(100), cx);
    let long = cx.shown_system_notifications().pop().unwrap();
    assert_eq!(long.body.chars().count(), 160);
    assert!(long.body.ends_with('…'));
    let (generation, message) = f.begin(cx);
    assert!(f.ingress(
        generation,
        vec![text(&message, &"长".repeat(200)), interrupted(&message)],
        Some(true),
        cx
    ));
    let interrupted = cx.shown_system_notifications().pop().unwrap();
    assert!(interrupted.body.starts_with("任务已中断"));
    assert_eq!(interrupted.body.chars().count(), 160);
    assert!(interrupted.body.ends_with('…'));
}

#[gpui_kit::test]
async fn unavailable_system_delivery_keeps_durable_unread_and_normal_run_cleanup(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx, false);
    assert!(cx.app_identity().is_none());
    f.route(false, true, false, cx);
    f.complete("unavailable platform result", cx);
    assert!(f.row(&f.a.id).unread);
    assert!(cx.shown_system_notifications().is_empty());
    assert!(cx.delivered_system_notifications().is_empty());
}

#[gpui_kit::test]
async fn metadata_and_organization_refreshes_preserve_unread_until_a_genuine_visit(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx, true);
    f.route(false, true, false, cx);
    f.complete("unread result", cx);
    let before = f.row(&f.a.id);
    let snapshot = vega_conversation::sidebar_organization::snapshot(&f.store).unwrap();
    vega_conversation::sidebar_organization::apply(
        &f.store,
        snapshot.revision,
        SidebarOrganizationAction::SetCollapsed {
            target: SidebarCollapseTarget::Project(f.a.project_id.clone()),
            collapsed: true,
        },
    )
    .unwrap();
    let snapshot = vega_conversation::sidebar_organization::snapshot(&f.store).unwrap();
    vega_conversation::sidebar_organization::apply(
        &f.store,
        snapshot.revision,
        SidebarOrganizationAction::SetCollapsed {
            target: SidebarCollapseTarget::Project(f.a.project_id.clone()),
            collapsed: false,
        },
    )
    .unwrap();
    let snapshot = vega_conversation::sidebar_organization::snapshot(&f.store).unwrap();
    vega_conversation::sidebar_organization::apply(
        &f.store,
        snapshot.revision,
        SidebarOrganizationAction::ReorderThread {
            thread_id: f.a.id.clone(),
            before_id: Some(f.b.id.clone()),
        },
    )
    .unwrap();
    assert_eq!(f.row(&f.a.id), before);
    let refresh = reload_thread_state(&f.store, &f.a.id).unwrap();
    f.root.update(cx, |_, cx| {
        VegaWindow::apply_stream_state(
            &f.stream,
            refresh.thread,
            refresh.plans,
            refresh.history,
            cx,
        )
    });
    let refreshed = f.row(&f.a.id);
    assert!(refreshed.unread);
    assert_eq!(refreshed.id, before.id);
    assert_eq!(refreshed.project_id, before.project_id);
    assert_eq!(refreshed.parent_thread_id, before.parent_thread_id);
    assert_eq!(
        refreshed.workspace_owner_thread_id,
        before.workspace_owner_thread_id
    );
    f.visit_a(cx);
    assert!(!f.row(&f.a.id).unread);
    f.route(false, true, false, cx);
    cx.run_until_parked();
    assert!(!f.row(&f.a.id).unread);
}

#[gpui_kit::test]
async fn notification_click_activates_and_visits_only_its_session_without_starting_a_run(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx, true);
    f.route(false, false, true, cx);
    let before_b = f.row(&f.b.id);
    let messages = f.message_count();
    f.complete("clickable result", cx);
    let notice = cx.shown_system_notifications().pop().unwrap();
    f.click(&notice, cx);
    assert!(
        f.window
            .update(cx, |_, window, _| window.is_window_active())
            .unwrap()
    );
    assert!(!f.row(&f.a.id).unread);
    assert_eq!(f.row(&f.b.id), before_b);
    assert_eq!(f.message_count(), messages);
    f.assert_stopped(cx);
    assert!(
        f.root
            .read_with(cx, |root, _| root.agent_controller.active.is_empty())
    );
}

#[gpui_kit::test]
async fn notification_navigation_preserves_a_draft_and_respects_capacity_guard(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx, true);
    f.route(false, true, false, cx);
    let b_stream = f
        .root
        .read_with(cx, |root, _| root.stream_view.as_ref().unwrap().1.clone());
    b_stream
        .read_with(cx, |stream, _| stream.composer_input())
        .update(cx, |input, cx| input.set_text("keep this draft", cx));
    f.complete("first destination", cx);
    f.click(&cx.shown_system_notifications().pop().unwrap(), cx);
    assert!(
        f.root
            .update(cx, |root, cx| root.accept_palette_thread(f.b.clone(), cx))
    );
    pump_test_app(cx, |cx| {
        f.root.read_with(cx, |root, _| {
            root.stream_view
                .as_ref()
                .is_some_and(|(id, _)| id == &f.b.id)
                && root.agent_controller.queue_operations.is_empty()
        })
    });
    let b_stream = f
        .root
        .read_with(cx, |root, _| root.stream_view.as_ref().unwrap().1.clone());
    assert_eq!(
        b_stream.read_with(cx, |stream, cx| stream
            .composer_input()
            .read(cx)
            .text()
            .to_owned()),
        "keep this draft"
    );
    f.route(false, true, true, cx);
    let oversized = "x".repeat(1024 * 1024 + 1);
    b_stream
        .read_with(cx, |stream, _| stream.composer_input())
        .update(cx, |input, cx| input.set_text(&oversized, cx));
    cx.update(|cx| cx.refresh_windows());
    cx.run_until_parked();
    f.complete("guarded destination", cx);
    let notice = cx.shown_system_notifications().pop().unwrap();
    cx.simulate_system_notification_response(SystemNotificationResponse {
        tag: notice.tag,
        action_id: None,
    });
    cx.run_until_parked();
    assert_eq!(
        cx.read(|cx| cx.global::<OpenedThread>().0.as_ref().unwrap().id.clone()),
        f.b.id
    );
    assert!(f.row(&f.a.id).unread);
    assert_eq!(
        b_stream.read_with(cx, |stream, cx| stream
            .composer_input()
            .read(cx)
            .text()
            .to_owned()),
        oversized
    );
    assert!(cx.read(|cx| {
        cx.global::<vega_ui::navigation::NavigationState>()
            .error
            .is_some()
    }));
    f.assert_stopped(cx);
}

#[gpui_kit::test]
async fn unknown_deleted_and_closed_window_notification_responses_cannot_create_tasks(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx, true);
    f.route(false, true, false, cx);
    let b = f.row(&f.b.id);
    cx.simulate_system_notification_response(SystemNotificationResponse {
        tag: format!("vega-run-{}-unknown", f.a.id).into(),
        action_id: None,
    });
    cx.run_until_parked();
    assert_eq!(
        cx.read(|cx| cx.global::<OpenedThread>().0.as_ref().unwrap().id.clone()),
        f.b.id
    );
    f.complete("deleted destination", cx);
    let notice = cx.shown_system_notifications().pop().unwrap();
    vega_conversation::threads::delete_thread(&f.store, &f.a.id).unwrap();
    cx.simulate_system_notification_response(SystemNotificationResponse {
        tag: notice.tag.clone(),
        action_id: None,
    });
    cx.run_until_parked();
    assert!(
        vega_store::threads::find(f.store.conn(), &f.a.id)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        cx.read(|cx| cx.global::<OpenedThread>().0.as_ref().unwrap().id.clone()),
        f.b.id
    );
    assert_eq!(f.row(&f.b.id), b);
    assert_eq!(f.message_count(), 0);
    f.window
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    cx.simulate_system_notification_response(SystemNotificationResponse {
        tag: notice.tag,
        action_id: None,
    });
    cx.run_until_parked();
    assert!(
        vega_store::threads::find(f.store.conn(), &f.a.id)
            .unwrap()
            .is_none()
    );
    assert_eq!(f.row(&f.b.id), b);
}

#[gpui_kit::test]
async fn failed_visit_keeps_unread_and_a_recoverable_navigation_error(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx, true);
    f.route(false, true, false, cx);
    f.complete("unread until saved", cx);
    cx.update(|cx| cx.set_global(VegaStore(Err("synthetic unavailable store".into()))));
    f.visit_a(cx);
    assert!(f.row(&f.a.id).unread);
    assert!(cx.read(|cx| {
        cx.global::<vega_ui::navigation::NavigationState>()
            .error
            .is_some()
    }));
    f.assert_stopped(cx);
}

#[gpui_kit::test]
async fn marking_a_visible_session_unread_survives_render_until_a_new_real_visit(
    cx: &mut gpui_kit::TestAppContext,
) {
    let f = fixture(cx, true);
    f.route(true, true, false, cx);
    vega_conversation::threads::update_thread(
        &f.store,
        &f.a.id,
        &ThreadUpdate {
            unread: Some(true),
            ..Default::default()
        },
    )
    .unwrap();
    let mut marked = f.a.clone();
    marked.unread = true;
    cx.update(|cx| {
        cx.set_global(OpenedThread(Some(marked.clone())));
        cx.refresh_windows();
    });
    f.stream
        .update(cx, |stream, cx| stream.apply_thread(marked, cx));
    cx.run_until_parked();
    assert!(f.row(&f.a.id).unread);
    let refresh = reload_thread_state(&f.store, &f.a.id).unwrap();
    f.root.update(cx, |_, cx| {
        VegaWindow::apply_stream_state(
            &f.stream,
            refresh.thread,
            refresh.plans,
            refresh.history,
            cx,
        )
    });
    cx.run_until_parked();
    assert!(f.row(&f.a.id).unread);
    let before = f.row(&f.a.id);
    cx.update(|cx| cx.refresh_windows());
    cx.run_until_parked();
    assert_eq!(f.row(&f.a.id), before);
    VisualTestContext::from_window(f.window.into(), cx).deactivate_window();
    assert!(f.row(&f.a.id).unread);
    f.window
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    pump_test_app(cx, |_| !f.row(&f.a.id).unread);
    assert!(!f.row(&f.b.id).unread);
}
