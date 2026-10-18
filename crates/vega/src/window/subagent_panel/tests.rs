use super::*;
use crate::tests::install_diff_window_globals;
use crate::window::workspace::TabKey;
use gpui_kit::{
    Bounds, Modifiers, TestAppContext, VisualTestContext, WindowBounds, WindowHandle,
    WindowOptions, point, size,
};
use vega_conversation::history::{AssistantStatus, HistoryEntry, HistoryPage};
use vega_store::Store;
use vega_ui::sidebar::SelectedProject;

struct Fixture {
    _data: tempfile::TempDir,
    database: std::path::PathBuf,
    parent: Thread,
    children: Vec<Thread>,
    root: Entity<VegaWindow>,
    stream: Entity<ConversationStream>,
    window: WindowHandle<VegaWindow>,
}

fn message(store: &Store, thread: &str, seq: i64, role: &str, content: &str) {
    vega_store::messages::insert(
        store.conn(),
        &vega_store::messages::MessageRow {
            id: format!("{thread}-{seq}"),
            thread_id: thread.into(),
            seq,
            role: role.into(),
            kind: "text".into(),
            content: content.into(),
            status: "done".into(),
            created_at: seq,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .unwrap();
}

fn fixture(cx: &mut TestAppContext, names: &[&str]) -> Fixture {
    let data = tempfile::tempdir().unwrap();
    let database = data.path().join("panel.db");
    let store = Store::open(&database).unwrap();
    store.migrate().unwrap();
    vega_conversation::privacy::register_database(database.clone(), data.path().join("config"));
    let parent =
        vega_conversation::threads::create_standalone_thread(&store, "mock", "confirm").unwrap();
    message(&store, &parent.id, 1, "user", "Parent task");
    message(&store, &parent.id, 2, "assistant", "Delegating");
    for seq in 3..=205 {
        message(&store, &parent.id, seq, "user", "Later turn");
    }
    let mut children = Vec::new();
    for (index, name) in names.iter().enumerate() {
        let mut child =
            vega_conversation::threads::create_standalone_thread(&store, "mock", "confirm")
                .unwrap();
        child.parent_thread_id = Some(parent.id.clone());
        vega_store::subagents::insert(
            store.conn(),
            &vega_store::subagents::DelegationRow {
                child_thread_id: child.id.clone(),
                parent_thread_id: parent.id.clone(),
                parent_message_id: format!("{}-2", parent.id),
                spawn_call_id: format!("spawn-{index}"),
                child_message_id: format!("{}-2", child.id),
                name: name.to_string(),
                task: "Read files".into(),
                state: "completed".into(),
                result: "Original result".into(),
                truncated: false,
                created_at: index as i64,
            },
        )
        .unwrap();
        message(&store, &child.id, 1, "user", "Read files");
        message(&store, &child.id, 2, "assistant", "Full child answer");
        children.push(child);
    }
    let page = vega_conversation::history::latest_history_page(
        &store,
        &parent.id,
        vega_store::messages::PAGE_LIMIT,
    )
    .unwrap();
    assert!(page.subagents.is_empty());
    cx.update(|cx| {
        install_diff_window_globals(store, parent.clone(), cx);
        cx.set_global(SelectedProject(None));
    });
    let stream = cx.new(|cx| {
        let mut stream = ConversationStream::new(parent.clone(), cx);
        stream.apply_history_page(page, cx);
        stream
    });
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, _| {
        root.stream_view = Some((parent.id.clone(), stream.clone()));
        root.agent_provider_override = Some(std::sync::Arc::new(vega_runtime::MockProvider::new(
            Vec::new(),
        )));
    });
    let window_root = root.clone();
    let window = cx
        .update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(1400.), px(900.)),
                    ))),
                    ..Default::default()
                },
                move |_, _| window_root,
            )
        })
        .unwrap();
    cx.run_until_parked();
    Fixture {
        _data: data,
        database,
        parent,
        children,
        root,
        stream,
        window,
    }
}

fn click(f: &Fixture, selector: &str, cx: &mut TestAppContext) {
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(f.window.into(), cx);
    let bounds = visual
        .debug_bounds(Box::leak(selector.to_string().into_boxed_str()))
        .unwrap_or_else(|| panic!("missing {selector}"));
    visual.simulate_click(bounds.center(), Modifiers::default());
    visual.run_until_parked();
}

fn visible(f: &Fixture, selector: &'static str, cx: &mut TestAppContext) -> bool {
    cx.run_until_parked();
    VisualTestContext::from_window(f.window.into(), cx)
        .debug_bounds(selector)
        .is_some()
}

fn assert_parent(f: &Fixture, cx: &mut TestAppContext) {
    f.root.read_with(cx, |root, cx| {
        assert_eq!(
            cx.global::<OpenedThread>().0.as_ref().unwrap().id,
            f.parent.id
        );
        assert_eq!(root.stream_view.as_ref().unwrap().1, f.stream);
        assert_eq!(
            root.agent_worker_start_probe
                .load(std::sync::atomic::Ordering::SeqCst),
            0
        );
    });
}

#[gpui_kit::test]
fn panel_summary_list_inline_detail_and_full_route_preserve_parent(cx: &mut TestAppContext) {
    let f = fixture(cx, &["Read sources", "Review tests"]);
    let input = f.stream.read_with(cx, |stream, _| stream.composer_input());
    input.update(cx, |input, cx| input.set_text("Preserved parent draft", cx));
    assert!(!visible(&f, "environment-subagents", cx));
    assert!(!visible(&f, "subagent-panel", cx));
    click(&f, "main-header-environment", cx);
    assert!(visible(&f, "environment-subagents", cx));
    assert!(!visible(&f, "environment-review", cx));
    assert!(!visible(&f, "environment-terminal", cx));
    f.root.read_with(cx, |root, _| {
        assert_eq!(root.subagent_panel.agents.len(), 2);
        assert_eq!(root.subagent_counts(), "2 Completed");
    });
    click(&f, "environment-subagents-open", cx);
    assert!(visible(&f, "subagent-panel-list", cx));
    click(&f, "right-workspace-add", cx);
    click(&f, "workspace-add-subagents", cx);
    f.root.read_with(cx, |root, _| {
        assert_eq!(
            root.workspace
                .tabs
                .iter()
                .filter(|(key, _)| *key == TabKey::Subagents)
                .count(),
            1
        )
    });
    assert!(!visible(&f, "workspace-add-menu", cx));
    cx.simulate_keystrokes(f.window.into(), "tab enter");
    cx.run_until_parked();
    assert_parent(&f, cx);
    f.root.read_with(cx, |root, cx| {
        let detail = root.subagent_panel.detail.as_ref().unwrap();
        assert_eq!(detail.id, f.children[0].id);
        assert!(
            detail
                .stream
                .as_ref()
                .unwrap()
                .read(cx)
                .hydrated_entry_count()
                >= 2
        );
    });
    assert!(visible(&f, "composer-shell", cx));
    cx.simulate_keystrokes(f.window.into(), "tab space");
    cx.run_until_parked();
    assert!(visible(&f, "subagent-panel-list", cx));
    f.window
        .update(cx, |root, window, cx| {
            root.open_subagent_preview(
                f.stream.clone(),
                &SubagentOpenRequested {
                    source_thread_id: f.parent.id.clone(),
                    thread_id: f.children[1].id.clone(),
                },
                window,
                cx,
            )
        })
        .unwrap();
    cx.run_until_parked();
    assert_parent(&f, cx);
    assert_eq!(
        input.read_with(cx, |input, _| input.text().to_string()),
        "Preserved parent draft"
    );
    click(&f, "subagent-panel-open-conversation", cx);
    f.root.read_with(cx, |root, cx| {
        assert_eq!(
            cx.global::<OpenedThread>().0.as_ref().unwrap().id,
            f.children[1].id
        );
        assert!(root.subagent_panel.detail.is_none());
        assert_eq!(
            root.agent_worker_start_probe
                .load(std::sync::atomic::Ordering::SeqCst),
            0
        );
    });
    assert!(visible(&f, "composer-shell", cx));
    let store = Store::open(&f.database).unwrap();
    assert_eq!(
        vega_store::subagents::find(store.conn(), &f.children[1].id)
            .unwrap()
            .unwrap()
            .result,
        "Original result"
    );
}

#[gpui_kit::test]
fn panel_live_run_stop_keeps_sibling_parent_and_later_generation_safe(cx: &mut TestAppContext) {
    let f = fixture(cx, &["First", "Second"]);
    let first = cx.new(|cx| ConversationStream::new(f.children[0].clone(), cx));
    let second = cx.new(|cx| ConversationStream::new(f.children[1].clone(), cx));
    let (generation, first_cancel, parent_cancel, second_cancel) = f.root.update(cx, |root, cx| {
        let (_, parent_cancel) = root
            .agent_controller
            .begin(f.parent.id.clone(), f.stream.clone(), None, None)
            .unwrap();
        let (generation, first_cancel) = root
            .agent_controller
            .begin(f.children[0].id.clone(), first.clone(), None, None)
            .unwrap();
        let (_, second_cancel) = root
            .agent_controller
            .begin(f.children[1].id.clone(), second.clone(), None, None)
            .unwrap();
        root.agent_controller
            .delegated_unbound
            .insert(f.children[0].id.clone());
        cx.notify();
        (generation, first_cancel, parent_cancel, second_cancel)
    });
    click(&f, "main-header-environment", cx);
    click(&f, "environment-subagents-open", cx);
    click(&f, &format!("subagent-panel-open-{}", f.children[0].id), cx);
    f.root.read_with(cx, |root, _| {
        assert_eq!(
            root.subagent_panel.detail.as_ref().unwrap().stream.as_ref(),
            Some(&first)
        );
        assert_eq!(root.subagent_counts(), "2 Running");
    });
    first.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "later-run".into(),
                seq: 3,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "later-run".into(),
                delta: "Live child content".into(),
            },
            cx,
        );
    });
    f.root.update(cx, |root, cx| {
        root.stop_panel_child(&f.children[0].id, generation, cx);
        root.clear_subagent_detail(cx);
    });
    cx.run_until_parked();
    assert!(first_cancel.is_cancelled());
    assert!(!parent_cancel.is_cancelled());
    assert!(!second_cancel.is_cancelled());
    cx.update(|cx| {
        let revoked = &cx
            .global::<crate::automation_controller::AutomationGlobal>()
            .revoked;
        assert!(revoked.contains(&(f.children[0].id.clone(), AutomationKind::Goal)));
        assert!(revoked.contains(&(f.children[0].id.clone(), AutomationKind::Loop)));
        assert!(
            !revoked
                .iter()
                .any(|(thread, _)| thread == &f.parent.id || thread == &f.children[1].id)
        );
    });
    f.root.read_with(cx, |root, _| {
        assert_eq!(root.subagent_counts(), "1 Running · 1 Stopping")
    });
    let later_cancel = f.root.update(cx, |root, cx| {
        root.stop_panel_child(&f.children[0].id, generation, cx);
        assert!(
            root.agent_controller
                .finish(generation, &f.children[0].id, &first)
                .is_some()
        );
        root.agent_controller
            .begin(f.children[0].id.clone(), first.clone(), None, None)
            .unwrap()
            .1
    });
    cx.run_until_parked();
    assert!(!later_cancel.is_cancelled());
    click(&f, "workspace-tab-close-Subagents", cx);
    assert!(!later_cancel.is_cancelled());
    assert!(!second_cancel.is_cancelled());
    assert_parent(&f, cx);
    let store = Store::open(&f.database).unwrap();
    assert_eq!(
        vega_store::subagents::find(store.conn(), &f.children[0].id)
            .unwrap()
            .unwrap()
            .state,
        "completed"
    );
    f.window
        .update(cx, |root, window, cx| {
            root.open_subagent_preview(
                f.stream.clone(),
                &SubagentOpenRequested {
                    source_thread_id: f.parent.id.clone(),
                    thread_id: f.children[0].id.clone(),
                },
                window,
                cx,
            )
        })
        .unwrap();
    cx.run_until_parked();
    click(&f, "subagent-panel-open-conversation", cx);
    f.root.read_with(cx, |root, cx| {
        assert_eq!(
            cx.global::<OpenedThread>().0.as_ref().unwrap().id,
            f.children[0].id
        );
        assert_eq!(root.stream_view.as_ref().unwrap().1, first);
        assert_eq!(root.agent_worker_start_probe.load(Ordering::SeqCst), 0);
    });
    assert!(visible(&f, "composer-shell", cx));
    assert!(!later_cancel.is_cancelled());
}

#[gpui_kit::test]
fn panel_stale_load_detail_and_paging_callbacks_cannot_replace_current_owner(
    cx: &mut TestAppContext,
) {
    let f = fixture(cx, &["First", "Second"]);
    click(&f, "main-header-environment", cx);
    click(&f, "environment-subagents-open", cx);
    click(&f, &format!("subagent-panel-open-{}", f.children[0].id), cx);
    let (owner, stream, history_epoch) = f.root.update(cx, |root, cx| {
        let epoch = root.subagent_panel.epoch;
        let load = root.subagent_panel.load;
        let revision = root.subagent_panel.revision;
        let rows = root.subagent_panel.agents.clone();
        let mut updated = rows[0].clone();
        updated.state = SubagentState::Failed;
        root.apply_subagent_panel_update(updated, cx);
        root.finish_subagent_panel_load(&f.parent.id, epoch, load, revision, Ok(rows), cx);
        assert_eq!(root.subagent_panel.agents[0].state, SubagentState::Failed);
        assert_eq!(root.subagent_counts(), "1 Completed · 1 Failed");
        (
            root.panel_read_owner(&f.children[0].id).unwrap(),
            root.subagent_panel
                .detail
                .as_ref()
                .unwrap()
                .stream
                .clone()
                .unwrap(),
            root.subagent_panel.history_epoch,
        )
    });
    let count = stream.read_with(cx, |stream, _| stream.hydrated_entry_count());
    f.root.update(cx, |root, cx| {
        root.set_subagent_preview_visible(false, 500., cx);
        root.set_subagent_preview_visible(true, 500., cx);
        root.finish_subagent_history(
            PanelHistoryRead {
                owner: owner.clone(),
                epoch: history_epoch,
                stream: stream.clone(),
                newer: true,
            },
            Ok(page(3, "Stale page")),
            cx,
        );
    });
    assert_eq!(
        stream.read_with(cx, |stream, _| stream.hydrated_entry_count()),
        count
    );
    f.root.update(cx, |root, cx| {
        root.finish_subagent_history(
            PanelHistoryRead {
                owner: owner.clone(),
                epoch: root.subagent_panel.history_epoch,
                stream: stream.clone(),
                newer: true,
            },
            Ok(page(3, "Current page")),
            cx,
        );
    });
    assert!(stream.read_with(cx, |stream, _| stream.hydrated_entry_count()) > count);
    click(&f, "subagent-panel-back", cx);
    click(&f, &format!("subagent-panel-open-{}", f.children[1].id), cx);
    f.window
        .update(cx, |root, window, cx| {
            root.finish_subagent_detail(
                owner.clone(),
                Some(stream.clone()),
                Ok((f.children[0].clone(), None)),
                window,
                cx,
            );
            root.finish_subagent_history(
                PanelHistoryRead {
                    owner,
                    epoch: root.subagent_panel.history_epoch,
                    stream: stream.clone(),
                    newer: true,
                },
                Ok(page(4, "Wrong selection")),
                cx,
            );
            assert_eq!(
                root.subagent_panel.detail.as_ref().unwrap().id,
                f.children[1].id
            );
        })
        .unwrap();
    assert_parent(&f, cx);
    let store = Store::open(&f.database).unwrap();
    for seq in 3..=205 {
        message(
            &store,
            &f.children[0].id,
            seq,
            "user",
            "Older child history",
        );
    }
    click(&f, "subagent-panel-back", cx);
    click(&f, &format!("subagent-panel-open-{}", f.children[0].id), cx);
    let paged = f.root.read_with(cx, |root, _| {
        root.subagent_panel
            .detail
            .as_ref()
            .unwrap()
            .stream
            .clone()
            .unwrap()
    });
    let before = paged.read_with(cx, |stream, _| stream.hydrated_entry_count());
    f.root.update(cx, |root, cx| {
        root.request_subagent_history(paged.clone(), &f.children[0].id, 6, false, cx)
    });
    cx.run_until_parked();
    assert!(paged.read_with(cx, |stream, _| stream.hydrated_entry_count()) > before);
    let (epoch, load, rows) = f.root.read_with(cx, |root, _| {
        (
            root.subagent_panel.epoch,
            root.subagent_panel.load,
            root.subagent_panel.agents.clone(),
        )
    });
    f.root.update(cx, |root, cx| {
        cx.set_global(OpenedThread(Some(f.children[0].clone())));
        root.sync_subagent_panel_route(cx);
        cx.set_global(OpenedThread(Some(f.parent.clone())));
        root.sync_subagent_panel_route(cx);
        root.finish_subagent_panel_load(&f.parent.id, epoch, load, 0, Ok(rows), cx);
        assert!(root.subagent_panel.agents.is_empty());
    });
    cx.run_until_parked();
    f.root.read_with(cx, |root, _| {
        assert_eq!(root.subagent_panel.agents.len(), 2)
    });
}

fn page(seq: i64, text: &str) -> HistoryPage {
    HistoryPage {
        subagents: Vec::new(),
        entries: vec![HistoryEntry::AssistantText {
            seq,
            message_id: format!("extra-{seq}"),
            content: text.into(),
            status: AssistantStatus::Done,
            execution_duration_ms: None,
        }],
        older_cursor: None,
        newer_cursor: None,
        newest_seq: Some(seq),
    }
}

#[gpui_kit::test]
fn panel_errors_retry_detached_child_and_narrow_restore_remain_usable(cx: &mut TestAppContext) {
    let f = fixture(cx, &["First"]);
    f.root.update(cx, |root, cx| {
        root.finish_subagent_panel_load(
            &f.parent.id,
            root.subagent_panel.epoch,
            root.subagent_panel.load,
            root.subagent_panel.revision,
            Err(()),
            cx,
        )
    });
    click(&f, "main-header-environment", cx);
    click(&f, "environment-subagents-open", cx);
    assert!(visible(&f, "subagent-panel-retry", cx));
    click(&f, "subagent-panel-retry", cx);
    assert!(visible(&f, "subagent-panel-list", cx));
    let store = Store::open(&f.database).unwrap();
    store
        .conn()
        .execute(
            "UPDATE threads SET parent_thread_id=NULL WHERE id=?1",
            [&f.children[0].id],
        )
        .unwrap();
    click(&f, &format!("subagent-panel-open-{}", f.children[0].id), cx);
    assert!(visible(&f, "subagent-detail-retry", cx));
    assert_parent(&f, cx);
    click(&f, "subagent-panel-back", cx);
    cx.update(|cx| cx.set_global(vega_ui::sidebar::SidebarExplicitlyShown(true)));
    let mut visual = VisualTestContext::from_window(f.window.into(), cx);
    visual.simulate_resize(size(px(800.), px(700.)));
    visual.run_until_parked();
    assert!(visual.debug_bounds("subagent-panel").is_some());
    assert!(visual.debug_bounds("right-workspace-pane").is_none());
    click(&f, "right-workspace-hide", cx);
    assert!(!visible(&f, "subagent-panel", cx));
    f.window
        .update(cx, |root, window, cx| {
            root.restore_hidden_workspace(0, window, cx);
        })
        .unwrap();
    assert!(visible(&f, "subagent-panel", cx));
    let mut visual = VisualTestContext::from_window(f.window.into(), cx);
    visual.simulate_resize(size(px(1400.), px(900.)));
    visual.run_until_parked();
    assert!(visual.debug_bounds("right-workspace-pane").is_some());
    assert!(visual.debug_bounds("composer-shell").is_some());
    click(&f, "right-workspace-dock", cx);
    assert!(visible(&f, "bottom-workspace-pane", cx));
    assert_parent(&f, cx);
}

#[gpui_kit::test]
fn panel_privacy_cleanup_invalidates_cached_names_and_old_reads(cx: &mut TestAppContext) {
    let token = "ghp_syntheticDelegatedSecret0123456789";
    let f = fixture(cx, &[token]);
    let store = Store::open(&f.database).unwrap();
    let review = vega_conversation::privacy::review(&store, &f.parent.id).unwrap();
    let index = review
        .findings
        .iter()
        .position(|finding| finding.field == "delegation.name")
        .unwrap();
    vega_conversation::privacy::set_exception(&store, &f.parent.id, &review, index, true).unwrap();
    f.root.update(cx, |root, cx| {
        root.refresh_subagents_after_privacy(&f.parent.id, cx)
    });
    cx.run_until_parked();
    let (epoch, load, rows) = f.root.read_with(cx, |root, _| {
        assert_eq!(root.subagent_panel.agents[0].name, token);
        (
            root.subagent_panel.epoch,
            root.subagent_panel.load,
            root.subagent_panel.agents.clone(),
        )
    });
    let review = vega_conversation::privacy::review(&store, &f.parent.id).unwrap();
    f.root.update(cx, |root, cx| {
        root.request_privacy(
            f.stream.clone(),
            &PrivacyRequested {
                thread_id: f.parent.id.clone(),
                action: PrivacyAction::Exception {
                    snapshot: review,
                    index,
                    allow: false,
                },
            },
            cx,
        )
    });
    cx.run_until_parked();
    let review = vega_conversation::privacy::review(&store, &f.parent.id).unwrap();
    f.root.update(cx, |root, cx| {
        root.request_privacy(
            f.stream.clone(),
            &PrivacyRequested {
                thread_id: f.parent.id.clone(),
                action: PrivacyAction::Clean { snapshot: review },
            },
            cx,
        )
    });
    cx.run_until_parked();
    f.root.update(cx, |root, cx| {
        assert!(root.subagent_panel.epoch > epoch);
        assert!(!root.subagent_panel.agents[0].name.contains(token));
        assert!(root.subagent_panel.detail.is_none());
        root.finish_subagent_panel_load(&f.parent.id, epoch, load, 0, Ok(rows), cx);
        assert!(!root.subagent_panel.agents[0].name.contains(token));
    });
}

#[gpui_kit::test]
fn panel_empty_memory_and_file_backed_tasks_keep_overview_unavailable(cx: &mut TestAppContext) {
    let f = fixture(cx, &[]);
    f.root.read_with(cx, |root, cx| {
        assert!(!root.has_subagents(cx));
        assert!(!root.environment_available(cx));
    });
    let store = Store::open(":memory:").unwrap();
    store.migrate().unwrap();
    let parent =
        vega_conversation::threads::create_standalone_thread(&store, "mock", "confirm").unwrap();
    cx.update(|cx| {
        cx.set_global(VegaStore(Ok(store)));
        cx.set_global(OpenedThread(Some(parent)));
    });
    f.root.update(cx, |root, cx| {
        root.sync_subagent_panel_route(cx);
        assert!(!root.subagent_panel.error);
        assert!(!root.environment_available(cx));
    });
}
