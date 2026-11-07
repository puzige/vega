use super::draft::Draft;
use super::nsh_view::{self, Snapshot};
use super::{CommitPanelFocus, CommitPanelStage};
use crate::nsh::ProjectionId;
use nsh_core::element::{Component, NoEvent, Root, Scope};
use nsh_core::prelude as ns;
use nsh_core::{ApproxTextEngine, Core, EventKind, EventPayload, MountItem, SubmitBehavior};
use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tokio::sync::mpsc;

fn snapshot(draft: Draft, page: ProjectionId) -> Snapshot {
    let value = draft.0.lock().expect("test draft lock");
    Snapshot {
        page,
        thread: "synthetic-thread".into(),
        project: "synthetic-project".into(),
        stage: CommitPanelStage::CommitReady,
        pending: None,
        checklist: None,
        generation: None,
        prepared: None,
        selected: HashSet::new(),
        disabled: false,
        focus: CommitPanelFocus::Confirm,
        focus_revision: 0,
        rows: Arc::from([]),
        draft: draft.clone(),
        draft_revision: value.revision,
        draft_text: value.text.clone(),
        draft_overflow: value.overflow,
        failure: false,
        colors: vega_theme::LIGHT,
    }
}

#[test]
fn retained_commands_require_their_immutable_displayed_revision() {
    let draft = Draft::default();
    let page = ProjectionId::new();
    let mount = Arc::new(());
    {
        let mut value = draft.0.lock().expect("test draft lock");
        value.page = Some(page.clone());
        value.mount = Arc::downgrade(&mount);
        value.visible.store(true, Ordering::Release);
        value.replace("displayed subject\n\nbody\n");
    }
    let displayed = snapshot(draft.clone(), page.clone());
    let retained = displayed.clone();
    let activate = |shown: &Snapshot, fresh: &Snapshot| {
        retained.with_current_draft(shown, fresh, |value| value.text.clone())
    };
    let full = "new subject\n\n中文 body\ntrailing newline\n";
    draft.0.lock().expect("test draft lock").replace(full);
    let fresh = snapshot(draft.clone(), page);
    assert!(activate(&displayed, &displayed).is_none());
    assert!(activate(&displayed, &fresh).is_none());
    assert!(activate(&fresh, &fresh).is_none());
    assert!(
        fresh
            .with_current_draft(&displayed, &fresh, |_| panic!(
                "unacknowledged command reached controller"
            ))
            .is_none()
    );
    let submitted = fresh.with_current_draft(&fresh, &fresh, |value| value.text.clone());
    assert_eq!(submitted.as_deref(), Some(full));
    let mut pending = fresh.clone();
    pending.stage = CommitPanelStage::Committing;
    pending.pending = Some(super::CommitOperationId(1));
    assert!(
        fresh
            .with_current_draft(&fresh, &pending, |_| panic!(
                "duplicate command reached controller"
            ))
            .is_none()
    );
    {
        let mut value = draft.0.lock().expect("test draft lock");
        value.composing = true;
    }
    assert!(
        fresh
            .with_current_draft(&fresh, &fresh, |_| panic!("composition reached controller"))
            .is_none()
    );
}

#[test]
fn stale_generated_completion_preserves_newer_operation_and_edit_guard() {
    use super::draft::{Value, finish_generated};
    use super::{CommitErrorCode, CommitOperationId, CommitPanelModel};

    let a = CommitOperationId(1);
    let b = CommitOperationId(2);
    let mut model = CommitPanelModel {
        stage: CommitPanelStage::Drafting,
        pending: Some(b),
        ..Default::default()
    };
    let mut value = Value::default();
    value.replace("original\n\nbody\n");
    let mut token = Some((b, value.revision));
    let original_revision = value.revision;
    assert!(!finish_generated(
        &mut model,
        &mut token,
        &mut value,
        a,
        true,
        Ok("stale A".into())
    ));
    assert_eq!(token, Some((b, original_revision)));
    assert_eq!(model.pending, Some(b));
    assert_eq!(model.stage(), CommitPanelStage::Drafting);
    assert_eq!(value.text, "original\n\nbody\n");
    assert!(finish_generated(
        &mut model,
        &mut token,
        &mut value,
        b,
        true,
        Ok("generated B\n\n中文 body\n".into())
    ));
    assert_eq!(value.text, "generated B\n\n中文 body\n");
    assert_eq!(model.stage(), CommitPanelStage::CommitReady);
    assert_eq!(model.pending, None);
    assert_eq!(token, None);
    model.stage = CommitPanelStage::Drafting;
    model.pending = Some(b);
    token = Some((b, value.revision));
    value.replace("newer user edit\n");
    assert!(finish_generated(
        &mut model,
        &mut token,
        &mut value,
        b,
        true,
        Ok("must not overwrite".into())
    ));
    assert_eq!(value.text, "newer user edit\n");
    assert_eq!(
        model.stage(),
        CommitPanelStage::Failed(CommitErrorCode::ChangedDuringRead)
    );
    model.stage = CommitPanelStage::CommitReady;
    model.pending = Some(b);
    token = Some((b, value.revision));
    assert!(!finish_generated(
        &mut model,
        &mut token,
        &mut value,
        b,
        true,
        Ok("wrong stage".into())
    ));
    assert_eq!(token, Some((b, value.revision)));
    assert_eq!(model.pending, Some(b));
}

#[test]
fn suspension_retires_live_draft_authority_without_losing_text() {
    let mut state = super::host::State::default();
    let page = ProjectionId::new();
    let mount = Arc::new(());
    {
        let mut value = state.draft.0.lock().expect("test draft lock");
        value.page = Some(page.clone());
        value.mount = Arc::downgrade(&mount);
        value.visible.store(true, Ordering::Release);
        value.editable = true;
        value.replace("retained full draft\n\nbody\n");
        assert!(value.owns(&page));
    }
    let shown = snapshot(state.draft.clone(), page);
    state.unmount();
    let value = state.draft.0.lock().expect("test draft lock");
    assert!(!value.owns(&shown.page));
    assert!(!value.editable);
    assert!(value.mount.upgrade().is_none());
    assert_eq!(value.text, shown.draft_text);
    assert_eq!(value.revision, shown.draft_revision);
    drop(value);
    assert!(
        shown
            .with_current_draft(&shown, &shown, |_| panic!(
                "suspended command reached controller"
            ))
            .is_none()
    );
}

thread_local! {
    static DRAFT: RefCell<Option<Draft>> = const { RefCell::new(None) };
    static BUTTON: RefCell<Option<(Snapshot, mpsc::Receiver<nsh_view::Request>)>> = const { RefCell::new(None) };
}

struct RetainedButton;

impl Component for RetainedButton {
    type Props = ();
    type Event = NoEvent;

    fn view(_: (), _: Scope<NoEvent>) -> impl ns::IntoElement {
        let draft = Draft::default();
        let page = ProjectionId::new();
        let mount = ns::signal_with(Arc::new(()), ns::SignalEquals::Never);
        {
            let mut value = draft.0.lock().expect("test draft lock");
            value.page = Some(page.clone());
            value.mount = Arc::downgrade(&mount.get_untracked());
            value.visible.store(true, Ordering::Release);
            value.replace("displayed text\n");
        }
        let shown = snapshot(draft, page);
        let (sender, receiver) = mpsc::channel(64);
        BUTTON.with(|slot| *slot.borrow_mut() = Some((shown.clone(), receiver)));
        nsh_view::button(
            shown,
            crate::nsh::OrderedCommands::new(sender),
            "Commit",
            nsh_view::Action::Confirm,
            true,
            false,
        )
    }
}

#[test]
fn production_pointer_callback_cannot_borrow_an_undisplayed_revision() {
    let mut core = Core::new(ApproxTextEngine, ns::size(ns::px(480.), ns::px(600.)));
    core.mount_root(&Root::of::<RetainedButton>());
    let batch = core.commit().expect("button mount");
    let button = batch
        .items
        .iter()
        .find_map(|item| match item {
            MountItem::Events { id, events } if events.contains(EventKind::Press) => Some(*id),
            _ => None,
        })
        .expect("production button press target");
    let (shown, mut receiver) =
        BUTTON.with(|slot| slot.borrow_mut().take().expect("button context"));
    shown
        .draft
        .0
        .lock()
        .expect("test draft lock")
        .replace("undisplayed newer text\n\nbody\n");
    assert!(core.component_event(button, EventKind::Press, &EventPayload::None));
    core.run_component_tasks();
    let request = receiver.try_recv().expect("production pointer command");
    let nsh_view::Request::Action(command, nsh_view::Action::Confirm, None) = request else {
        panic!("unexpected button request");
    };
    assert_eq!(command.draft_revision, shown.draft_revision);
    let fresh = snapshot(shown.draft.clone(), shown.page.clone());
    assert!(
        command
            .with_current_draft(&shown, &fresh, |_| panic!(
                "stale pointer reached controller"
            ))
            .is_none()
    );
}

struct MessagePanel;

impl Component for MessagePanel {
    type Props = ();
    type Event = NoEvent;

    fn view(_: (), _: Scope<NoEvent>) -> impl ns::IntoElement {
        let draft = Draft::default();
        let page = ProjectionId::new();
        let mount = ns::signal_with(Arc::new(()), ns::SignalEquals::Never);
        {
            let mut value = draft.0.lock().expect("test draft lock");
            value.page = Some(page.clone());
            value.mount = Arc::downgrade(&mount.get_untracked());
            value.visible.store(true, Ordering::Release);
            value.editable = true;
        }
        DRAFT.with(|slot| *slot.borrow_mut() = Some(draft.clone()));
        let (sender, receiver) = mpsc::channel(64);
        let _receiver = ns::signal_with(receiver, ns::SignalEquals::Never);
        nsh_view::panel(
            ns::signal_with(
                Snapshot {
                    page,
                    thread: "synthetic-thread".into(),
                    project: "synthetic-project".into(),
                    stage: CommitPanelStage::CommitReady,
                    pending: None,
                    checklist: None,
                    generation: None,
                    prepared: None,
                    selected: HashSet::new(),
                    disabled: false,
                    focus: CommitPanelFocus::Cancel,
                    focus_revision: 0,
                    rows: Arc::from([]),
                    draft,
                    draft_revision: 0,
                    draft_text: String::new(),
                    draft_overflow: false,
                    failure: false,
                    colors: vega_theme::LIGHT,
                },
                ns::SignalEquals::Never,
            ),
            sender,
        )
    }
}

#[test]
fn production_multiline_preserves_full_text_and_rejects_edits_after_freeze() {
    let mut core = Core::new(ApproxTextEngine, ns::size(ns::px(480.), ns::px(600.)));
    core.mount_root(&Root::of::<MessagePanel>());
    let batch = core.commit().expect("panel mount");
    let input = batch
        .items
        .iter()
        .find_map(|item| match item {
            MountItem::Input { id, config } => {
                assert!(config.multiline);
                assert_eq!(config.submit_behavior, Some(SubmitBehavior::Newline));
                Some(*id)
            }
            _ => None,
        })
        .expect("production multiline input");
    let full = "subject\n\n中文 body\ntrailing newline\n";
    assert!(core.component_event(
        input,
        EventKind::ChangeText,
        &EventPayload::Text {
            text: full.into(),
            count: 1
        }
    ));
    DRAFT.with(|slot| {
        let draft = slot.borrow();
        let mut value = draft
            .as_ref()
            .expect("shared draft")
            .0
            .lock()
            .expect("test lock");
        assert_eq!(value.text, full);
        value.editable = false;
    });
    assert!(core.component_event(
        input,
        EventKind::ChangeText,
        &EventPayload::Text {
            text: "late event".into(),
            count: 2
        }
    ));
    DRAFT.with(|slot| {
        assert_eq!(
            slot.borrow()
                .as_ref()
                .expect("shared draft")
                .0
                .lock()
                .expect("test lock")
                .text,
            full
        );
    });
}
