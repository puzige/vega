use super::VegaWindow;
use crate::app_agent::{AgentBatch, AgentBatchIngress, PendingPlanReview};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, Bounds, Entity, MouseButton, Pixels, TestAppContext, VisualTestContext, WindowBounds,
    WindowHandle, WindowOptions, point, px, size,
};
use std::path::PathBuf;
use vega_conversation::sidebar_organization as organization;
use vega_conversation::types::{
    ConversationEvent, ConversationStopReason, PlanReviewAction, PlanStatus,
    SidebarOrganizationAction, SidebarOrganizationError, SidebarOrganizationSnapshot, Thread,
    ThreadMode,
};
use vega_store::Store;
use vega_store::messages::{self, MessageRow};
use vega_theme::{Layout, Theme};
use vega_ui::conversation_stream::ConversationStream;
use vega_ui::navigation::NavigationState;
use vega_ui::plan_card::PlanReviewRequested;
use vega_ui::settings::SettingsOpen;
use vega_ui::sidebar::{
    OpenedThread, PendingDeleteConfirm, ProjectsCollapsed, SelectedProject, SessionsCollapsed,
    SidebarCollapsed, SidebarWidth, VegaStore,
};

const FOREGROUND_DRAFT: &str = "Keep this unsent foreground draft";

struct Fixture {
    _data: tempfile::TempDir,
    database: PathBuf,
    rows: [Thread; 3],
    foreground: Thread,
    root: Entity<VegaWindow>,
    foreground_stream: Entity<ConversationStream>,
    background_stream: Entity<ConversationStream>,
    window: WindowHandle<VegaWindow>,
}

fn install_fixture_globals(store: Store, thread: Thread, cx: &mut App) {
    gpui_kit::init(cx);
    cx.set_global(Theme::light());
    cx.set_global(SettingsOpen(false));
    cx.set_global(SidebarCollapsed(false));
    cx.set_global(SidebarWidth(Layout::SIDEBAR_WIDTH));
    cx.set_global(SelectedProject(Some(thread.project_id.clone())));
    cx.set_global(OpenedThread(Some(thread)));
    cx.set_global(PendingDeleteConfirm(None));
    cx.set_global(ProjectsCollapsed(false));
    cx.set_global(SessionsCollapsed(false));
    cx.set_global(VegaStore(Ok(store)));
    vega_ui::init(cx);
}

fn fixture(cx: &mut TestAppContext) -> Fixture {
    let data = tempfile::tempdir().unwrap();
    let database = data.path().join("message-order.db");
    let config = data.path().join("config.toml");
    std::fs::write(&config, "").unwrap();
    let store = Store::open(&database).unwrap();
    store.migrate().unwrap();
    vega_conversation::privacy::register_database(
        database.clone(),
        data.path().join("credentials"),
    );
    let mut projects = Vec::new();
    for name in ["Background", "Foreground"] {
        let path = data.path().join(name);
        std::fs::create_dir(&path).unwrap();
        projects.push(
            vega_store::projects::create(store.conn(), path.to_str().unwrap(), name, None).unwrap(),
        );
    }
    let rows = ["A", "B", "C"].map(|title| {
        let thread =
            vega_conversation::threads::create_thread(&store, &projects[0].id, "mock", "confirm")
                .unwrap();
        vega_conversation::threads::rename_thread(&store, &thread.id, title).unwrap()
    });
    let foreground =
        vega_conversation::threads::create_thread(&store, &projects[1].id, "mock", "confirm")
            .unwrap();
    let mut before_id = None;
    for thread in rows.iter().rev() {
        let state = organization::snapshot(&store).unwrap();
        organization::apply(
            &store,
            state.revision,
            SidebarOrganizationAction::ReorderThread {
                thread_id: thread.id.clone(),
                before_id,
            },
        )
        .unwrap();
        before_id = Some(thread.id.clone());
    }
    let state = organization::snapshot(&store).unwrap();
    organization::apply(
        &store,
        state.revision,
        SidebarOrganizationAction::MoveProject {
            project_id: projects[0].id.clone(),
            before_id: Some(projects[1].id.clone()),
        },
    )
    .unwrap();
    cx.update(|cx| {
        install_fixture_globals(store, foreground.clone(), cx);
    });
    let foreground_stream = cx.new(|cx| ConversationStream::new(foreground.clone(), cx));
    foreground_stream
        .read_with(cx, |stream, _| stream.composer_input())
        .update(cx, |input, cx| input.set_text(FOREGROUND_DRAFT, cx));
    let background_stream = cx.new(|cx| ConversationStream::new(rows[2].clone(), cx));
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, _| {
        root.model_selection_config_override = Some(config);
        root.stream_view = Some((foreground.id.clone(), foreground_stream.clone()));
    });
    let window_root = root.clone();
    let window = cx.update(|cx| {
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(1400.), px(1000.)),
                ))),
                ..Default::default()
            },
            move |_, _| window_root,
        )
        .unwrap()
    });
    let fixture = Fixture {
        _data: data,
        database,
        rows,
        foreground,
        root,
        foreground_stream,
        background_stream,
        window,
    };
    assert_rows(&fixture, [0, 1, 2], cx);
    assert_navigation(&fixture, cx);
    fixture
}

fn snapshot(f: &Fixture) -> SidebarOrganizationSnapshot {
    organization::snapshot(&Store::open(&f.database).unwrap()).unwrap()
}

fn expected_after_background_completion(
    f: &Fixture,
    before: &SidebarOrganizationSnapshot,
) -> SidebarOrganizationSnapshot {
    let mut expected = before.clone();
    expected
        .threads
        .iter_mut()
        .find(|thread| thread.id == f.rows[2].id)
        .unwrap()
        .unread = true;
    expected
}

fn project_order(f: &Fixture, state: &SidebarOrganizationSnapshot) -> Vec<String> {
    organization::sorted_threads(&state.threads, &state.preferences)
        .into_iter()
        .filter(|thread| thread.project_id == f.rows[0].project_id && !thread.pinned)
        .map(|thread| thread.id)
        .collect()
}

fn expected_ids(f: &Fixture, order: [usize; 3]) -> Vec<String> {
    order.map(|index| f.rows[index].id.clone()).into()
}

fn row_bounds(f: &Fixture, index: usize, cx: &mut TestAppContext) -> Bounds<Pixels> {
    let selector = Box::leak(format!("project-thread-row-{}", f.rows[index].id).into_boxed_str());
    VisualTestContext::from_window(f.window.into(), cx)
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing conversation row {}", f.rows[index].id))
}

fn assert_rows(f: &Fixture, order: [usize; 3], cx: &mut TestAppContext) {
    cx.run_until_parked();
    let mut positions = [0, 1, 2].map(|index| (row_bounds(f, index, cx).top(), index));
    positions.sort_by(|left, right| left.0.partial_cmp(&right.0).unwrap());
    assert_eq!(
        positions
            .map(|(_, index)| f.rows[index].id.clone())
            .to_vec(),
        expected_ids(f, order),
        "committed order must be rendered without waiting for the sidebar polling timer"
    );
}

fn assert_navigation(f: &Fixture, cx: &mut TestAppContext) {
    f.root.read_with(cx, |root, cx| {
        assert_eq!(
            cx.global::<SelectedProject>().0.as_deref(),
            Some(f.foreground.project_id.as_str())
        );
        assert_eq!(cx.global::<OpenedThread>().0.as_ref(), Some(&f.foreground));
        assert!(cx.global::<PendingDeleteConfirm>().0.is_none());
        let (thread_id, stream) = root.stream_view.as_ref().unwrap();
        assert_eq!(thread_id, &f.foreground.id);
        assert_eq!(stream, &f.foreground_stream);
        assert_eq!(
            stream.read(cx).composer_input().read(cx).text(),
            FOREGROUND_DRAFT
        );
        let navigation = cx.global::<NavigationState>();
        assert!(!navigation.back);
        assert!(!navigation.forward);
        assert!(navigation.error.is_none());
    });
}

fn begin(f: &Fixture, cx: &mut TestAppContext) -> u64 {
    let generation = f.root.update(cx, |root, cx| {
        let (generation, _) = root
            .agent_controller
            .begin(
                f.rows[2].id.clone(),
                f.background_stream.clone(),
                None,
                None,
            )
            .unwrap();
        vega_ui::sidebar::set_thread_running(&f.rows[2].id, true, cx);
        generation
    });
    f.background_stream
        .update(cx, ConversationStream::begin_composer_run);
    generation
}

fn commit_message(f: &Fixture, id: &str, seq: i64) -> MessageRow {
    let row = MessageRow {
        id: id.into(),
        thread_id: f.rows[2].id.clone(),
        seq,
        role: "assistant".into(),
        kind: "text".into(),
        content: String::new(),
        status: "streaming".into(),
        created_at: 1,
        plan_status: None,
        plan_review_note: None,
        plan_reviewed_at: None,
    };
    let store = Store::open(&f.database).unwrap();
    let transaction = store.immediate_transaction().unwrap();
    messages::insert(&transaction, &row).unwrap();
    transaction.commit().unwrap();
    assert_eq!(
        messages::find(Store::open(&f.database).unwrap().conn(), id).unwrap(),
        Some(row.clone())
    );
    row
}

fn persist_delta(f: &Fixture, row: &mut MessageRow, content: &str) {
    let store = Store::open(&f.database).unwrap();
    assert_eq!(
        messages::update_streaming_content(store.conn(), &row.id, content).unwrap(),
        1
    );
    row.content = content.into();
}

fn started(row: &MessageRow) -> ConversationEvent {
    ConversationEvent::MessageStarted {
        message_id: row.id.clone(),
        seq: row.seq.try_into().unwrap(),
    }
}

fn delta(row: &MessageRow, text: &str) -> ConversationEvent {
    ConversationEvent::TextDelta {
        message_id: row.id.clone(),
        delta: text.into(),
    }
}

fn ingress(
    f: &Fixture,
    generation: u64,
    events: Vec<ConversationEvent>,
    finished: Option<bool>,
    cx: &mut TestAppContext,
) -> AgentBatchIngress {
    f.root.update(cx, |root, cx| {
        root.apply_agent_batch_ingress(
            generation,
            &f.rows[2].id,
            &f.background_stream,
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
            },
            cx,
        )
    })
}

fn assert_projected_message(f: &Fixture, row: &MessageRow, cx: &mut TestAppContext) {
    f.background_stream.read_with(cx, |stream, _| {
        assert_eq!(
            stream
                .projected_assistant_messages()
                .into_iter()
                .filter(|(id, _)| id.as_deref() == Some(row.id.as_str()))
                .collect::<Vec<_>>(),
            vec![(Some(row.id.clone()), row.content.clone())]
        );
    });
}

fn assert_projected_messages(f: &Fixture, rows: &[&MessageRow], cx: &mut TestAppContext) {
    f.background_stream.read_with(cx, |stream, _| {
        assert_eq!(
            stream.projected_assistant_messages(),
            rows.iter()
                .map(|row| (Some(row.id.clone()), row.content.clone()))
                .collect::<Vec<_>>()
        );
    });
}

fn assert_live_message(f: &Fixture, row: &MessageRow, cx: &mut TestAppContext) {
    f.background_stream.read_with(cx, |stream, cx| {
        assert!(stream.has_active_agent());
        assert!(vega_ui::sidebar::thread_is_running(&row.thread_id, cx));
        assert!(stream.controller_error_message().is_none());
    });
    assert_projected_message(f, row, cx);
    assert_eq!(
        messages::find(Store::open(&f.database).unwrap().conn(), &row.id).unwrap(),
        Some(row.clone())
    );
}

fn finish(f: &Fixture, generation: u64, row: &mut MessageRow, cx: &mut TestAppContext) {
    assert_eq!(
        messages::finish_streaming(
            Store::open(&f.database).unwrap().conn(),
            &row.id,
            &row.content,
            "done",
            None,
        )
        .unwrap(),
        1
    );
    row.status = "done".into();
    let receipt = ingress(
        f,
        generation,
        vec![ConversationEvent::MessageFinished {
            message_id: row.id.clone(),
            stop_reason: ConversationStopReason::End,
            execution_duration_ms: None,
        }],
        Some(true),
        cx,
    );
    match receipt {
        AgentBatchIngress::Finished {
            success,
            run,
            reference_failure,
            credential_failure,
        } => {
            assert!(success);
            assert_eq!(run.generation, generation);
            assert_eq!(run.thread_id, row.thread_id);
            assert_eq!(run.terminal_message_id.as_deref(), Some(row.id.as_str()));
            assert!(run.terminal_failure.is_none());
            assert!(reference_failure.is_none());
            assert!(!credential_failure);
        }
        _ => panic!("a committed message must finish normally"),
    }
    f.root.read_with(cx, |root, cx| {
        assert!(!root.agent_controller.active.contains_key(&row.thread_id));
        assert!(!vega_ui::sidebar::thread_is_running(&row.thread_id, cx));
    });
    f.background_stream.update(cx, |stream, cx| {
        assert!(!stream.has_active_agent());
        assert!(stream.controller_error_message().is_none());
        assert!(!stream.finish_composer_run(false, cx));
    });
    assert_projected_message(f, row, cx);
    assert_eq!(
        messages::find(Store::open(&f.database).unwrap().conn(), &row.id).unwrap(),
        Some(row.clone())
    );
}

fn drag_after(f: &Fixture, source: usize, target: usize, cx: &mut TestAppContext) {
    let source = row_bounds(f, source, cx);
    {
        let mut visual = VisualTestContext::from_window(f.window.into(), cx);
        visual.simulate_mouse_move(source.center(), None, Default::default());
        visual.simulate_mouse_down(source.center(), MouseButton::Left, Default::default());
        visual.simulate_mouse_move(
            source.center() + point(px(12.), px(0.)),
            Some(MouseButton::Left),
            Default::default(),
        );
        visual.run_until_parked();
    }
    assert!(cx.update(|cx| cx.has_active_drag()));
    let target = row_bounds(f, target, cx);
    let position = point(target.center().x, target.top() + target.size.height * 0.75);
    let mut visual = VisualTestContext::from_window(f.window.into(), cx);
    visual.simulate_mouse_move(position, Some(MouseButton::Left), Default::default());
    visual.run_until_parked();
    visual.simulate_mouse_up(position, MouseButton::Left, Default::default());
}

#[gpui_kit::test]
fn committed_background_message_refreshes_rows_before_poll_without_writing_order(
    cx: &mut TestAppContext,
) {
    let f = fixture(cx);
    let generation = begin(&f, cx);
    let mut row = commit_message(&f, "committed-background", 1);
    persist_delta(&f, &mut row, "Committed background answer");
    let store = Store::open(&f.database).unwrap();
    let state = organization::snapshot(&store).unwrap();
    let committed = organization::apply(
        &store,
        state.revision,
        SidebarOrganizationAction::ReorderThread {
            thread_id: f.rows[2].id.clone(),
            before_id: Some(f.rows[0].id.clone()),
        },
    )
    .unwrap()
    .snapshot;
    assert_eq!(project_order(&f, &committed), expected_ids(&f, [2, 0, 1]));
    assert_rows(&f, [0, 1, 2], cx);
    assert!(matches!(
        ingress(
            &f,
            generation,
            vec![started(&row), delta(&row, &row.content)],
            None,
            cx,
        ),
        AgentBatchIngress::Running
    ));
    assert_rows(&f, [2, 0, 1], cx);
    assert_live_message(&f, &row, cx);
    assert_projected_messages(&f, &[&row], cx);
    assert_eq!(snapshot(&f), committed);
    assert_navigation(&f, cx);
    finish(&f, generation, &mut row, cx);
    assert_projected_messages(&f, &[&row], cx);
    assert_eq!(
        snapshot(&f),
        expected_after_background_completion(&f, &committed)
    );
    assert_rows(&f, [2, 0, 1], cx);
    assert_navigation(&f, cx);
}

#[gpui_kit::test]
fn new_messages_interleaved_with_drag_keep_order_through_deltas_and_finish(
    cx: &mut TestAppContext,
) {
    let f = fixture(cx);
    let original = snapshot(&f);
    let first_generation = begin(&f, cx);
    let mut first = commit_message(&f, "first-background", 1);
    let first_commit = snapshot(&f);
    assert!(first_commit.revision > original.revision);
    assert_eq!(
        project_order(&f, &first_commit),
        expected_ids(&f, [2, 0, 1])
    );
    persist_delta(&f, &mut first, "First answer");
    assert!(matches!(
        ingress(
            &f,
            first_generation,
            vec![started(&first), delta(&first, &first.content)],
            None,
            cx,
        ),
        AgentBatchIngress::Running
    ));
    assert_rows(&f, [2, 0, 1], cx);
    assert_live_message(&f, &first, cx);
    persist_delta(&f, &mut first, "First answer while dragging");
    drag_after(&f, 2, 1, cx);
    assert!(matches!(
        ingress(
            &f,
            first_generation,
            vec![delta(&first, " while dragging")],
            None,
            cx,
        ),
        AgentBatchIngress::Running
    ));
    assert_rows(&f, [0, 1, 2], cx);
    assert_live_message(&f, &first, cx);
    let first_drag = snapshot(&f);
    assert!(first_drag.revision > first_commit.revision);
    assert_eq!(project_order(&f, &first_drag), expected_ids(&f, [0, 1, 2]));
    finish(&f, first_generation, &mut first, cx);
    assert_rows(&f, [0, 1, 2], cx);
    assert_eq!(
        snapshot(&f),
        expected_after_background_completion(&f, &first_drag)
    );
    assert_navigation(&f, cx);

    let second_generation = begin(&f, cx);
    assert_ne!(second_generation, first_generation);
    let mut second = commit_message(&f, "second-background", 2);
    let second_commit = snapshot(&f);
    assert!(second_commit.revision > first_drag.revision);
    assert_eq!(
        project_order(&f, &second_commit),
        expected_ids(&f, [2, 0, 1])
    );
    persist_delta(&f, &mut second, "Second answer");
    assert!(matches!(
        ingress(
            &f,
            second_generation,
            vec![started(&second), delta(&second, &second.content)],
            None,
            cx,
        ),
        AgentBatchIngress::Running
    ));
    assert_rows(&f, [2, 0, 1], cx);
    assert_live_message(&f, &second, cx);
    drag_after(&f, 2, 0, cx);
    assert_rows(&f, [0, 2, 1], cx);
    let second_drag = snapshot(&f);
    assert!(second_drag.revision > second_commit.revision);
    assert_eq!(project_order(&f, &second_drag), expected_ids(&f, [0, 2, 1]));
    f.root.read_with(cx, |root, _| {
        assert!(root.agent_controller.matches(
            second_generation,
            &f.rows[2].id,
            &f.background_stream,
        ));
    });
    assert_live_message(&f, &second, cx);
    persist_delta(&f, &mut second, "Second answer continues");
    assert!(matches!(
        ingress(
            &f,
            second_generation,
            vec![delta(&second, " continues")],
            None,
            cx,
        ),
        AgentBatchIngress::Running
    ));
    assert_rows(&f, [0, 2, 1], cx);
    assert_live_message(&f, &second, cx);
    assert_projected_messages(&f, &[&first, &second], cx);
    assert_eq!(snapshot(&f), second_drag);
    finish(&f, second_generation, &mut second, cx);
    assert_rows(&f, [0, 2, 1], cx);
    assert_eq!(snapshot(&f), second_drag);
    assert_projected_messages(&f, &[&first, &second], cx);
    assert_eq!(
        messages::recent(
            Store::open(&f.database).unwrap().conn(),
            &f.rows[2].id,
            messages::PAGE_LIMIT,
        )
        .unwrap(),
        vec![first, second]
    );
    assert_navigation(&f, cx);
}

#[gpui_kit::test]
fn active_duplicate_start_reports_controller_error_without_new_entries_message_or_reorder(
    cx: &mut TestAppContext,
) {
    let f = fixture(cx);
    let generation = begin(&f, cx);
    let mut row = commit_message(&f, "active-duplicate-start", 1);
    persist_delta(&f, &mut row, "The original active answer");
    assert!(matches!(
        ingress(
            &f,
            generation,
            vec![started(&row), delta(&row, &row.content)],
            None,
            cx,
        ),
        AgentBatchIngress::Running
    ));
    assert_live_message(&f, &row, cx);
    drag_after(&f, 2, 0, cx);
    assert_rows(&f, [0, 2, 1], cx);
    let dragged = snapshot(&f);
    let entry_count = f
        .background_stream
        .read_with(cx, |stream, _| stream.projected_entry_count());
    assert!(matches!(
        ingress(&f, generation, vec![started(&row)], None, cx),
        AgentBatchIngress::Running
    ));
    f.background_stream.read_with(cx, |stream, _| {
        assert_eq!(
            stream.controller_error_message(),
            Some("操作未保存，请重试")
        );
        assert!(stream.has_active_agent());
        assert_eq!(stream.projected_entry_count(), entry_count);
    });
    assert_projected_messages(&f, &[&row], cx);
    f.root.read_with(cx, |root, _| {
        assert!(
            root.agent_controller
                .matches(generation, &f.rows[2].id, &f.background_stream,)
        );
        assert_eq!(root.agent_controller.next_generation, generation);
    });
    let store = Store::open(&f.database).unwrap();
    assert_eq!(messages::next_seq(store.conn(), &f.rows[2].id).unwrap(), 2);
    assert_eq!(messages::find(store.conn(), &row.id).unwrap(), Some(row));
    assert_rows(&f, [0, 2, 1], cx);
    assert_eq!(snapshot(&f), dragged);
    assert_navigation(&f, cx);
}

#[gpui_kit::test]
fn finished_generation_replay_is_stale_without_reopening_or_reordering_message(
    cx: &mut TestAppContext,
) {
    let f = fixture(cx);
    let generation = begin(&f, cx);
    let mut row = commit_message(&f, "finished-generation", 1);
    persist_delta(&f, &mut row, "The finished answer");
    assert!(matches!(
        ingress(
            &f,
            generation,
            vec![started(&row), delta(&row, &row.content)],
            None,
            cx,
        ),
        AgentBatchIngress::Running
    ));
    finish(&f, generation, &mut row, cx);
    drag_after(&f, 2, 0, cx);
    assert_rows(&f, [0, 2, 1], cx);
    let dragged = snapshot(&f);
    let entry_count = f
        .background_stream
        .read_with(cx, |stream, _| stream.projected_entry_count());
    assert!(matches!(
        ingress(
            &f,
            generation,
            vec![
                started(&row),
                delta(&row, "Finished replay must not appear"),
                ConversationEvent::MessageFinished {
                    message_id: row.id.clone(),
                    stop_reason: ConversationStopReason::End,
                    execution_duration_ms: None,
                },
            ],
            Some(false),
            cx,
        ),
        AgentBatchIngress::Stale
    ));
    f.background_stream.read_with(cx, |stream, _| {
        assert!(!stream.has_active_agent());
        assert!(stream.controller_error_message().is_none());
        assert_eq!(stream.projected_entry_count(), entry_count);
    });
    assert_projected_messages(&f, &[&row], cx);
    f.root.read_with(cx, |root, _| {
        assert!(!root.agent_controller.active.contains_key(&f.rows[2].id));
    });
    let store = Store::open(&f.database).unwrap();
    assert_eq!(messages::next_seq(store.conn(), &f.rows[2].id).unwrap(), 2);
    assert_eq!(
        messages::recent(store.conn(), &f.rows[2].id, messages::PAGE_LIMIT).unwrap(),
        vec![row]
    );
    assert_rows(&f, [0, 2, 1], cx);
    assert_eq!(snapshot(&f), dragged);
    assert_navigation(&f, cx);
}

#[gpui_kit::test]
fn old_generation_replay_preserves_current_body_and_healthy_continuation_after_drag(
    cx: &mut TestAppContext,
) {
    let f = fixture(cx);
    let old_generation = begin(&f, cx);
    let mut old = commit_message(&f, "old-generation-answer", 1);
    persist_delta(&f, &mut old, "The older answer");
    assert!(matches!(
        ingress(
            &f,
            old_generation,
            vec![started(&old), delta(&old, &old.content)],
            None,
            cx,
        ),
        AgentBatchIngress::Running
    ));
    finish(&f, old_generation, &mut old, cx);

    let current_generation = begin(&f, cx);
    assert_ne!(current_generation, old_generation);
    let mut current = commit_message(&f, "current-generation-answer", 2);
    persist_delta(&f, &mut current, "The current answer");
    assert!(matches!(
        ingress(
            &f,
            current_generation,
            vec![started(&current), delta(&current, &current.content)],
            None,
            cx,
        ),
        AgentBatchIngress::Running
    ));
    assert_live_message(&f, &current, cx);
    drag_after(&f, 2, 0, cx);
    assert_rows(&f, [0, 2, 1], cx);
    let dragged = snapshot(&f);
    let entry_count = f
        .background_stream
        .read_with(cx, |stream, _| stream.projected_entry_count());
    assert!(matches!(
        ingress(
            &f,
            old_generation,
            vec![
                started(&old),
                delta(&old, "Stale content must not appear"),
                ConversationEvent::MessageFinished {
                    message_id: old.id.clone(),
                    stop_reason: ConversationStopReason::End,
                    execution_duration_ms: None,
                },
            ],
            Some(false),
            cx,
        ),
        AgentBatchIngress::Stale
    ));
    assert_live_message(&f, &current, cx);
    assert_projected_messages(&f, &[&old, &current], cx);
    f.background_stream.read_with(cx, |stream, _| {
        assert_eq!(stream.projected_entry_count(), entry_count);
    });
    f.root.read_with(cx, |root, _| {
        assert!(root.agent_controller.matches(
            current_generation,
            &f.rows[2].id,
            &f.background_stream,
        ));
    });
    assert_rows(&f, [0, 2, 1], cx);
    assert_eq!(snapshot(&f), dragged);
    assert_eq!(
        messages::next_seq(Store::open(&f.database).unwrap().conn(), &f.rows[2].id).unwrap(),
        3
    );
    assert_eq!(
        messages::find(Store::open(&f.database).unwrap().conn(), &old.id).unwrap(),
        Some(old.clone())
    );
    assert_navigation(&f, cx);

    persist_delta(&f, &mut current, "The current answer continues");
    assert!(matches!(
        ingress(
            &f,
            current_generation,
            vec![delta(&current, " continues")],
            None,
            cx,
        ),
        AgentBatchIngress::Running
    ));
    assert_live_message(&f, &current, cx);
    assert_projected_messages(&f, &[&old, &current], cx);
    finish(&f, current_generation, &mut current, cx);
    assert_projected_messages(&f, &[&old, &current], cx);
    assert_rows(&f, [0, 2, 1], cx);
    assert_eq!(snapshot(&f), dragged);
    assert_eq!(
        messages::recent(
            Store::open(&f.database).unwrap().conn(),
            &f.rows[2].id,
            messages::PAGE_LIMIT
        )
        .unwrap(),
        vec![old, current]
    );
    assert_navigation(&f, cx);
}

#[gpui_kit::test]
fn matching_pending_plan_approval_refreshes_background_rows_without_assistant_start(
    cx: &mut TestAppContext,
) {
    let f = fixture(cx);
    let store = Store::open(&f.database).unwrap();
    let plan_thread =
        vega_conversation::threads::set_thread_mode(&store, &f.rows[2].id, ThreadMode::Plan)
            .unwrap();
    f.background_stream
        .update(cx, |stream, cx| stream.apply_thread(plan_thread, cx));
    let plan_row = commit_message(&f, "background-plan-review", 1);
    messages::complete_plan(
        store.conn(),
        &f.rows[2].id,
        &plan_row.id,
        "Implement the approved background plan",
        2,
        None,
    )
    .unwrap();
    let plans = vega_conversation::plans::list_plans(&store, &f.rows[2].id).unwrap();
    assert_eq!(plans.len(), 1);
    let pending = plans.into_iter().next().unwrap();
    assert_eq!(pending.status, PlanStatus::Pending);
    f.background_stream
        .update(cx, |stream, cx| stream.apply_plan(pending.clone(), cx));
    let state = organization::snapshot(&store).unwrap();
    let state = organization::apply(
        &store,
        state.revision,
        SidebarOrganizationAction::ReorderThread {
            thread_id: f.rows[2].id.clone(),
            before_id: Some(f.rows[0].id.clone()),
        },
    )
    .unwrap()
    .snapshot;
    let committed = organization::apply(
        &store,
        state.revision,
        SidebarOrganizationAction::ReorderThread {
            thread_id: f.rows[1].id.clone(),
            before_id: Some(f.rows[0].id.clone()),
        },
    )
    .unwrap()
    .snapshot;
    assert_eq!(project_order(&f, &committed), expected_ids(&f, [2, 1, 0]));
    assert_rows(&f, [0, 1, 2], cx);
    assert_navigation(&f, cx);
    let entry_count = f.background_stream.read_with(cx, |stream, cx| {
        assert!(stream.has_pending_plan_review(cx));
        assert!(!stream.has_active_agent());
        assert!(!stream.has_pending_model_selection());
        assert!(!stream.reasoning_unavailable());
        assert!(stream.frozen_reasoning_for_submit().unwrap().is_none());
        assert!(!stream.pending_queue().busy);
        assert!(stream.projected_assistant_messages().is_empty());
        assert!(stream.controller_error_message().is_none());
        stream.projected_entry_count()
    });
    assert_eq!(entry_count, 1);
    let request = PlanReviewRequested {
        thread_id: f.rows[2].id.clone(),
        plan_id: pending.id.clone(),
        action: PlanReviewAction::Approve,
    };
    let generation = f.root.update(cx, |root, cx| {
        assert!(!root.owns_stream_request(&f.background_stream, &request.thread_id, cx));
        assert!(!crate::updater::installing());
        assert!(!root.trusted_actions.is_busy());
        assert!(root.reasoning_save_pending.is_none());
        assert!(root.agent_controller.preparation_stream.is_none());
        assert!(!vega_ui::sidebar::thread_is_running(&request.thread_id, cx));
        assert!(
            !root
                .agent_controller
                .active
                .contains_key(&request.thread_id)
        );
        assert!(
            !root
                .agent_controller
                .queue_operations
                .contains_key(&request.thread_id)
        );
        root.model_catalog_loading = true;
        root.agent_controller.pending_review.insert(
            request.thread_id.clone(),
            PendingPlanReview {
                stream: f.background_stream.clone(),
                request: request.clone(),
            },
        );
        let generation = root.agent_controller.next_generation;
        root.finish_owned_plan_review(f.background_stream.clone(), &request, cx);
        generation
    });

    let approved = vega_conversation::plans::list_plans(&store, &f.rows[2].id).unwrap();
    assert_eq!(approved.len(), 1);
    assert_eq!(approved[0].id, pending.id);
    assert_eq!(approved[0].content, pending.content);
    assert_eq!(approved[0].status, PlanStatus::Approved);
    assert!(approved[0].review_note.is_none());
    assert!(approved[0].reviewed_at.is_some());
    let reviewed = snapshot(&f);
    assert_eq!(
        reviewed
            .threads
            .iter()
            .find(|thread| thread.id == f.rows[2].id)
            .unwrap()
            .mode,
        ThreadMode::Execute
    );
    let rows = messages::recent(store.conn(), &f.rows[2].id, messages::PAGE_LIMIT).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, pending.id);
    assert_eq!(rows[0].seq, 1);
    assert_eq!(rows[0].role, "assistant");
    assert_eq!(rows[0].kind, "plan");
    assert_eq!(rows[0].status, "done");
    assert_eq!(rows[0].content, pending.content);
    assert_eq!(rows[0].plan_status.as_deref(), Some("approved"));
    let instruction = &rows[1];
    assert_ne!(instruction.id, pending.id);
    assert_eq!(instruction.thread_id, pending.thread_id);
    assert_eq!(instruction.seq, 2);
    assert_eq!(instruction.role, "user");
    assert_eq!(instruction.kind, "text");
    assert_eq!(instruction.status, "done");
    assert_eq!(
        instruction.content,
        "The plan was approved. Execute the approved plan from conversation history."
    );
    assert_eq!(Some(instruction.created_at), approved[0].reviewed_at);
    assert!(instruction.plan_status.is_none());
    assert!(instruction.plan_review_note.is_none());
    assert!(instruction.plan_reviewed_at.is_none());
    assert_eq!(messages::next_seq(store.conn(), &f.rows[2].id).unwrap(), 3);
    assert_eq!(
        vega_conversation::plans::recoverable_approved_instruction(&store, &f.rows[2].id).unwrap(),
        Some(instruction.id.clone())
    );
    f.root.read_with(cx, |root, cx| {
        assert!(root.model_catalog_loading);
        assert!(!vega_ui::sidebar::thread_is_running(&request.thread_id, cx));
        assert_eq!(root.agent_controller.next_generation, generation);
        assert!(
            !root
                .agent_controller
                .active
                .contains_key(&request.thread_id)
        );
        assert!(
            !root
                .agent_controller
                .pending_review
                .contains_key(&request.thread_id)
        );
        assert!(root.agent_controller.preparation_stream.is_none());
    });
    f.background_stream.read_with(cx, |stream, cx| {
        assert!(!stream.has_pending_plan_review(cx));
        assert!(!stream.has_active_agent());
        assert!(stream.projected_assistant_messages().is_empty());
        assert_eq!(stream.projected_entry_count(), entry_count);
        assert_eq!(
            stream.controller_error_message(),
            Some("执行未完成，可安全重试")
        );
    });
    assert_eq!(project_order(&f, &reviewed), expected_ids(&f, [2, 1, 0]));
    assert_navigation(&f, cx);
    assert_rows(&f, [2, 1, 0], cx);
    assert_eq!(snapshot(&f), reviewed);
    assert_navigation(&f, cx);
}

#[gpui_kit::test]
fn sidebar_sort_failure_keeps_normal_receipt_order_and_draft_until_next_new_message(
    cx: &mut TestAppContext,
) {
    let f = fixture(cx);
    let original = snapshot(&f);
    let store = Store::open(&f.database).unwrap();
    store
        .conn()
        .execute_batch(
            "CREATE TRIGGER reject_sidebar_update BEFORE UPDATE ON sidebar_organization
             BEGIN SELECT RAISE(ABORT, 'owned sidebar update failure'); END;",
        )
        .unwrap();
    let error = organization::apply(
        &store,
        original.revision,
        SidebarOrganizationAction::ReorderThread {
            thread_id: f.rows[2].id.clone(),
            before_id: Some(f.rows[0].id.clone()),
        },
    )
    .expect_err("the owned sidebar trigger must reject a real organization write");
    assert!(matches!(
        error,
        SidebarOrganizationError::Store(message)
            if message.contains("owned sidebar update failure")
    ));
    assert_eq!(snapshot(&f), original);
    let generation = begin(&f, cx);
    let mut row = commit_message(&f, "sort-failure-message", 1);
    assert_eq!(snapshot(&f), original);
    persist_delta(&f, &mut row, "The message still arrives");
    assert!(matches!(
        ingress(
            &f,
            generation,
            vec![started(&row), delta(&row, &row.content)],
            None,
            cx,
        ),
        AgentBatchIngress::Running
    ));
    assert_live_message(&f, &row, cx);
    assert_rows(&f, [0, 1, 2], cx);
    assert_eq!(snapshot(&f), original);
    assert_navigation(&f, cx);
    store
        .conn()
        .execute_batch("DROP TRIGGER reject_sidebar_update;")
        .unwrap();
    persist_delta(
        &f,
        &mut row,
        "The message still arrives after sorting recovers",
    );
    assert!(matches!(
        ingress(
            &f,
            generation,
            vec![delta(&row, " after sorting recovers")],
            None,
            cx,
        ),
        AgentBatchIngress::Running
    ));
    assert_live_message(&f, &row, cx);
    assert_rows(&f, [0, 1, 2], cx);
    assert_eq!(snapshot(&f), original);
    finish(&f, generation, &mut row, cx);
    assert_rows(&f, [0, 1, 2], cx);
    assert_eq!(
        snapshot(&f),
        expected_after_background_completion(&f, &original)
    );
    assert_projected_messages(&f, &[&row], cx);
    assert_navigation(&f, cx);

    let recovery_generation = begin(&f, cx);
    let mut recovery = commit_message(&f, "next-new-message", 2);
    let recovered = snapshot(&f);
    assert!(recovered.revision > original.revision);
    assert_eq!(project_order(&f, &recovered), expected_ids(&f, [2, 0, 1]));
    persist_delta(&f, &mut recovery, "The next new message promotes normally");
    assert!(matches!(
        ingress(
            &f,
            recovery_generation,
            vec![started(&recovery), delta(&recovery, &recovery.content)],
            None,
            cx,
        ),
        AgentBatchIngress::Running
    ));
    assert_rows(&f, [2, 0, 1], cx);
    assert_live_message(&f, &recovery, cx);
    assert_projected_messages(&f, &[&row, &recovery], cx);
    finish(&f, recovery_generation, &mut recovery, cx);
    assert_eq!(snapshot(&f), recovered);
    assert_rows(&f, [2, 0, 1], cx);
    assert_projected_messages(&f, &[&row, &recovery], cx);
    assert_eq!(
        messages::recent(
            Store::open(&f.database).unwrap().conn(),
            &f.rows[2].id,
            messages::PAGE_LIMIT,
        )
        .unwrap(),
        vec![row, recovery]
    );
    assert_navigation(&f, cx);
}
