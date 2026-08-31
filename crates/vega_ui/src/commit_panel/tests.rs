use std::sync::{Arc, Mutex};

use super::*;
use gpui::{TestAppContext, WindowHandle};

struct Harness {
    panel: Entity<CommitPanel>,
}

impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.panel.clone())
    }
}

#[test]
fn commit_panel_open_close_is_first_wins_and_cancel_visible() {
    let mut model = CommitPanelModel::default();
    assert!(model.open());
    assert!(!model.open());
    assert_eq!(model.stage(), CommitPanelStage::Loading);
    assert_eq!(model.focus(), CommitPanelFocus::Cancel);
    assert!(model.close_visible());
    assert!(!model.close_visible());
    assert_eq!(model.stage(), CommitPanelStage::Closed);
}

#[test]
fn commit_panel_fixed_geometry_and_limit_are_exact() {
    assert_eq!(COMMIT_ROW_HEIGHT, 24.0);
    assert_eq!(COMMIT_PATH_LIMIT, 10_000);
    assert!(checklist_count_is_bounded(10_000, 0));
    assert!(checklist_count_is_bounded(4_000, 6_000));
    assert!(!checklist_count_is_bounded(10_000, 1));
    assert!(!checklist_count_is_bounded(usize::MAX, 1));
    assert_ne!(
        commit_row_key(7, 0, 1),
        commit_row_key(7, 1, 1),
        "mixed staged/optional rows require distinct stable GPUI ids"
    );
    assert_eq!(commit_row_key(7, 1, 1), "commit-row-7-optional-0");
    assert_eq!(commit_row_status(true, true), "Included · staged");
    assert_eq!(commit_row_status(false, true), "Selected · worktree");
    assert_eq!(commit_row_status(false, false), "Optional · worktree");
    assert!(!commit_row_is_focusable(true));
    assert!(commit_row_is_focusable(false));
    assert_ne!(
        commit_row_status(true, true),
        commit_row_status(false, true)
    );
}

#[test]
fn commit_panel_message_and_events_are_debug_redacted() {
    let thread_sentinel = "VEGA_COMMIT_UI_THREAD_SECRET";
    let project_sentinel = "VEGA_COMMIT_UI_PROJECT_SECRET";
    let request = CommitChecklistRequested {
        thread_id: thread_sentinel.into(),
        project_id: project_sentinel.into(),
    };
    let closed = CommitPanelClosed {
        thread_id: thread_sentinel.into(),
        project_id: project_sentinel.into(),
    };
    for rendered in [format!("{request:?}"), format!("{closed:?}")] {
        assert!(!rendered.contains(thread_sentinel));
        assert!(!rendered.contains(project_sentinel));
    }
}

#[test]
fn commit_panel_failures_restore_cancel_and_clear_exact_pending() {
    let operation = CommitOperationId(7);
    let mut model = CommitPanelModel {
        stage: CommitPanelStage::Preparing,
        pending: Some(operation),
        focus: CommitPanelFocus::Confirm,
        ..CommitPanelModel::default()
    };
    assert!(!model.fail_pending(CommitOperationId(8), CommitErrorCode::SpawnFailed));
    assert!(model.owns_pending(operation));
    assert!(model.fail_pending(operation, CommitErrorCode::SpawnFailed));
    assert_eq!(
        model.stage(),
        CommitPanelStage::Failed(CommitErrorCode::SpawnFailed)
    );
    assert_eq!(model.focus(), CommitPanelFocus::Cancel);
    assert!(!model.owns_pending(operation));
}

#[test]
fn commit_panel_checked_operation_overflow_fails_closed() {
    let mut model = CommitPanelModel {
        stage: CommitPanelStage::CommitReady,
        next_operation: u64::MAX,
        focus: CommitPanelFocus::Confirm,
        ..CommitPanelModel::default()
    };
    assert_eq!(model.next_operation(), None);
    assert_eq!(
        model.stage(),
        CommitPanelStage::Failed(CommitErrorCode::OutputTooLarge)
    );
    assert_eq!(model.focus(), CommitPanelFocus::Cancel);
}

#[test]
fn commit_panel_invalid_messages_are_typed_before_any_event() {
    for message in [String::new(), "\0".into(), "x".repeat(32 * 1024 + 1)] {
        let mut model = CommitPanelModel {
            stage: CommitPanelStage::CommitReady,
            focus: CommitPanelFocus::Confirm,
            ..CommitPanelModel::default()
        };
        assert_eq!(model.begin_commit(message), None);
        assert_eq!(
            model.stage(),
            CommitPanelStage::Failed(CommitErrorCode::InvalidMessage)
        );
        assert_eq!(model.focus(), CommitPanelFocus::Cancel);
        assert!(model.pending.is_none());
    }
}

#[test]
fn commit_panel_focus_boundaries_escape_without_wrapping() {
    let mut model = CommitPanelModel {
        stage: CommitPanelStage::Checklist,
        ..CommitPanelModel::default()
    };
    assert!(!model.move_focus(true), "Shift+Tab at Cancel must escape");
    assert!(model.move_focus(false));
    assert_eq!(model.focus(), CommitPanelFocus::Confirm);
    assert!(!model.move_focus(false), "Tab at Confirm must escape");
    assert!(model.move_focus(true));
    assert_eq!(model.focus(), CommitPanelFocus::Cancel);

    model.stage = CommitPanelStage::CommitReady;
    assert!(model.move_focus(false));
    assert_eq!(model.focus(), CommitPanelFocus::Draft);
    assert!(model.move_focus(false));
    assert_eq!(model.focus(), CommitPanelFocus::Generate);
    assert!(model.move_focus(false));
    assert_eq!(model.focus(), CommitPanelFocus::Confirm);
    assert!(!model.move_focus(false));
    assert!(model.move_focus(true));
    assert_eq!(model.focus(), CommitPanelFocus::Generate);
}

#[gpui::test]
async fn commit_panel_draft_revision_overflow_never_accepts_equal_revision(
    cx: &mut TestAppContext,
) {
    let operation = CommitOperationId(9);
    // The entity-level fence must reject an apparently equal revision
    // after checked revision arithmetic has overflowed.
    let entity = cx.new(|cx| CommitPanel::new("thread".into(), "project".into(), cx));
    entity.update(cx, |panel, _| {
        panel.editor_revision = u64::MAX;
        panel.editor_revision_overflow = true;
        panel.draft_revision = Some((operation, u64::MAX));
        assert!(!panel.draft_revision_is_current(operation));
    });
}

#[gpui::test]
async fn commit_panel_scoped_keys_focus_cancel_and_escape_first_wins(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        crate::init(cx);
    });
    let panel = cx.new(|cx| CommitPanel::new("thread".into(), "project".into(), cx));
    let captured = Arc::new(Mutex::new(Vec::new()));
    let events = captured.clone();
    let root = panel.clone();
    let window: WindowHandle<Harness> = cx.update(|cx| {
        cx.open_window(Default::default(), move |_, cx| {
            cx.new(|cx| {
                cx.subscribe(&root, move |_, _, event: &CommitPanelClosed, _| {
                    events.lock().expect("events").push(event.clone());
                })
                .detach();
                Harness { panel: root }
            })
        })
        .expect("commit panel window")
    });
    window
        .update(cx, |_, window, cx| {
            assert!(panel.update(cx, |panel, cx| panel.request_open(cx)));
            let focus = panel.read(cx).focus_handle(cx);
            window.focus(&focus, cx);
            assert!(focus.is_focused(window));
        })
        .expect("focus commit panel");
    cx.simulate_keystrokes(window.into(), "enter space cmd-enter");
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.stage()),
        CommitPanelStage::Loading
    );
    cx.simulate_keystrokes(window.into(), "escape escape");
    assert_eq!(captured.lock().expect("events").len(), 1);
    assert!(!panel.read_with(cx, |panel, _| panel.is_open()));
}

#[gpui::test]
async fn commit_panel_ready_tab_chain_reaches_editor_generate_and_confirm(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        crate::init(cx);
    });
    let panel = cx.new(|cx| CommitPanel::new("thread".into(), "project".into(), cx));
    let root = panel.clone();
    let window: WindowHandle<Harness> = cx
        .update(|cx| {
            cx.open_window(Default::default(), move |_, cx| {
                cx.new(|_| Harness { panel: root })
            })
        })
        .expect("commit ready focus window");
    window
        .update(cx, |_, window, cx| {
            panel.update(cx, |panel, _| {
                panel.model.stage = CommitPanelStage::CommitReady;
                panel.model.focus = CommitPanelFocus::Cancel;
            });
            let focus = panel.read(cx).cancel_focus.clone();
            focus.focus(window, cx);
        })
        .expect("focus cancel");
    cx.simulate_keystrokes(window.into(), "tab");
    assert!(
        window
            .update(cx, |_, window, cx| {
                panel
                    .read(cx)
                    .message
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(window)
            })
            .expect("editor focus")
    );
    cx.simulate_keystrokes(window.into(), "tab");
    assert!(
        window
            .update(cx, |_, window, cx| {
                let focus = panel.read(cx).draft_focus.clone();
                focus.is_focused(window)
            })
            .expect("generate focus")
    );
    cx.simulate_keystrokes(window.into(), "tab");
    assert!(
        window
            .update(cx, |_, window, cx| {
                let focus = panel.read(cx).confirm_focus.clone();
                focus.is_focused(window)
            })
            .expect("confirm focus")
    );
}
