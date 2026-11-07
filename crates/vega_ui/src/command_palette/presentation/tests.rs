use super::{
    Operation, PaletteAction, PaletteScope, PaletteTarget, ProjectionId, Request, SharedDraft,
    next_scope, register_keys,
};
use nsh_core::prelude::{self as ns, Interactive as _};
use nsh_core::{Core, EventKind, EventPayload, MountBatch, MountItem, ROOT_ID};
use std::sync::Arc;

thread_local! {
    static KEY_COMMANDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[ns::component]
fn PaletteKeys() -> impl ns::IntoElement {
    let composing = ns::signal(false);
    let focused = ns::signal(true);
    register_keys(composing, focused, false, |_| {
        KEY_COMMANDS.with(|count| count.set(count.get() + 1));
    });
    ns::view()
        .on_press(move || composing.update(|value| *value = !*value))
        .on_focus(move || focused.set(true))
        .on_blur(move || focused.set(false))
}

fn published_keymap(batch: &MountBatch) -> Arc<nsh_core::keymap::Keymap> {
    batch
        .items
        .iter()
        .rev()
        .find_map(|item| match item {
            MountItem::Keymap(table) => Some(table.clone()),
            _ => None,
        })
        .expect("published key table")
}

#[test]
fn composition_and_blur_remove_bindings_and_reregistration_never_duplicates() {
    use nsh_core::keymap::{KeyMatch, Keystroke};
    KEY_COMMANDS.with(|count| count.set(0));
    let mut core = Core::new(
        nsh_core::ApproxTextEngine,
        ns::size(ns::px(400.), ns::px(300.)),
    );
    core.mount_root(&nsh_core::element::Root::of::<PaletteKeys>());
    let batch = core.commit().expect("initial mount");
    let host = batch
        .items
        .iter()
        .find_map(|item| match item {
            MountItem::Insert {
                parent: ROOT_ID,
                child,
                ..
            } => Some(*child),
            _ => None,
        })
        .expect("root host");
    let keys = ["down", "up", "enter", "escape", "tab", "shift+tab"];
    let mut table = published_keymap(&batch);
    for _ in 0..3 {
        assert_eq!(table.bindings().len(), keys.len());
        for key in keys {
            let stroke = Keystroke::parse_with(key, true).expect("palette key");
            assert!(matches!(table.find(stroke, true, false), KeyMatch::Fire(_)));
        }
        let retired: Vec<_> = table.bindings().iter().map(|binding| binding.id).collect();
        let previous = KEY_COMMANDS.with(|count| count.get());
        assert!(core.component_event(
            ROOT_ID,
            EventKind::KeyCommand,
            &EventPayload::Value(nsh_core::PayloadValue::Number(retired[0] as f64)),
        ));
        assert_eq!(KEY_COMMANDS.with(|count| count.get()), previous + 1);
        assert!(core.component_event(host, EventKind::Press, &EventPayload::None));
        table = published_keymap(&core.commit().expect("composition start"));
        assert!(table.bindings().is_empty());
        for key in keys {
            let stroke = Keystroke::parse_with(key, true).expect("IME key");
            assert_eq!(table.find(stroke, true, false), KeyMatch::None);
            assert_eq!(table.find(stroke, true, true), KeyMatch::None);
        }
        for id in retired {
            core.component_event(
                ROOT_ID,
                EventKind::KeyCommand,
                &EventPayload::Value(nsh_core::PayloadValue::Number(id as f64)),
            );
        }
        assert_eq!(KEY_COMMANDS.with(|count| count.get()), previous + 1);
        assert!(core.component_event(host, EventKind::Press, &EventPayload::None));
        table = published_keymap(&core.commit().expect("composition end"));
    }
    assert_eq!(table.bindings().len(), keys.len());
    assert!(core.component_event(host, EventKind::Blur, &EventPayload::None));
    assert!(
        published_keymap(&core.commit().expect("blur"))
            .bindings()
            .is_empty()
    );
    assert!(core.component_event(host, EventKind::Focus, &EventPayload::None));
    assert_eq!(
        published_keymap(&core.commit().expect("focus"))
            .bindings()
            .len(),
        keys.len()
    );
    core.apply(nsh_core::Op::Remove {
        parent: ROOT_ID,
        child: host,
    })
    .expect("remove host");
    core.apply(nsh_core::Op::Release { id: host })
        .expect("release owner");
    assert!(
        published_keymap(&core.commit().expect("dispose"))
            .bindings()
            .is_empty()
    );
}

#[test]
fn recovery_dismissal_requires_current_mount_session_draft_projection_and_no_ime() {
    let draft = SharedDraft::new();
    let observed = draft.read().expect("draft");
    let projection = ProjectionId::new();
    let request = Request {
        session: observed.session.clone(),
        projection: projection.clone(),
        observed: observed.clone(),
        operation: Operation::Dismiss,
    };
    assert!(request.authorized(&draft, &observed.session, &projection, true, false, true));
    assert!(!request.authorized(&draft, &observed.session, &projection, false, false, true));
    assert!(!request.authorized(&draft, &ProjectionId::new(), &projection, true, false, true));
    assert!(!request.authorized(
        &draft,
        &observed.session,
        &ProjectionId::new(),
        true,
        false,
        true
    ));
    assert!(!request.authorized(&draft, &observed.session, &projection, true, true, true));
    draft
        .edit(&observed.session, None, Some(true))
        .expect("IME start");
    assert!(!request.authorized(&draft, &observed.session, &projection, true, false, true));
    draft
        .edit(&observed.session, None, Some(false))
        .expect("IME end");
    assert!(!request.authorized(&draft, &observed.session, &projection, true, false, true));
    let current = draft.read().expect("current revision");
    let current_request = Request {
        observed: current.clone(),
        ..request
    };
    assert!(current_request.authorized(&draft, &current.session, &projection, true, false, true));
    let renewed = draft.renew().expect("replacement session");
    assert!(!current_request.authorized(&draft, &renewed.session, &projection, true, false, true));
}

#[test]
fn failure_allows_only_explicit_current_close_or_ui_rebuild() {
    let draft = SharedDraft::new();
    let observed = draft.read().expect("draft");
    let projection = ProjectionId::new();
    for operation in [
        Operation::Commit,
        Operation::Next,
        Operation::Previous,
        Operation::Accept,
        Operation::Activate(0, PaletteTarget::Action(PaletteAction::NewTask)),
        Operation::Scope(PaletteScope::Files),
        Operation::CycleScope(false),
        Operation::CycleScope(true),
    ] {
        let request = Request {
            session: observed.session.clone(),
            projection: projection.clone(),
            observed: observed.clone(),
            operation,
        };
        assert!(!request.authorized(&draft, &observed.session, &projection, true, false, true));
        assert!(request.authorized(&draft, &observed.session, &projection, true, false, false));
    }
    for operation in [Operation::Dismiss, Operation::Rebuild] {
        let request = Request {
            session: observed.session.clone(),
            projection: projection.clone(),
            observed: observed.clone(),
            operation,
        };
        assert!(request.authorized(&draft, &observed.session, &projection, true, false, true));
        assert!(!request.authorized(&draft, &observed.session, &projection, false, false, true));
        assert!(!request.authorized(
            &draft,
            &observed.session,
            &ProjectionId::new(),
            true,
            false,
            true
        ));
    }
}

#[test]
fn committed_query_waits_for_composition_end_and_rejects_old_activation() {
    let draft = SharedDraft::new();
    let initial = draft.read().expect("initial draft");
    let composing = draft
        .edit(&initial.session, None, Some(true))
        .expect("composition start");
    assert!(!draft.accepts(&initial));
    assert!(!draft.accepts(&composing));
    let changed = draft
        .edit(&initial.session, Some("日本語"), None)
        .expect("committed text before composition end");
    assert!(
        !draft
            .read()
            .expect("current draft")
            .committed("日本語", changed.revision)
    );
    let committed = draft
        .edit(&initial.session, None, Some(false))
        .expect("composition end");
    assert!(
        draft
            .read()
            .expect("current draft")
            .committed("日本語", committed.revision)
    );
    assert!(draft.accepts(&committed));
    assert!(!draft.accepts(&changed));
}

#[test]
fn full_paste_is_bounded_by_unicode_scalars_not_bytes() {
    let draft = SharedDraft::new();
    let initial = draft.read().expect("initial draft");
    let pasted = draft
        .edit(&initial.session, Some(&"界".repeat(300)), None)
        .expect("paste");
    assert_eq!(pasted.text, "界".repeat(256));
    assert_eq!(pasted.text.chars().count(), 256);
    assert!(
        draft
            .read()
            .expect("current draft")
            .committed(&pasted.text, pasted.revision)
    );
    assert!(!draft.accepts(&initial));
}

#[test]
fn returning_to_same_text_does_not_restore_old_result_authority() {
    let draft = SharedDraft::new();
    let initial = draft.read().expect("initial draft");
    let first = draft
        .edit(&initial.session, Some("first"), None)
        .expect("first edit");
    let second = draft
        .edit(&initial.session, Some("second"), None)
        .expect("second edit");
    let returned = draft
        .edit(&initial.session, Some("first"), None)
        .expect("return edit");
    assert!(!draft.accepts(&first));
    assert!(!draft.accepts(&second));
    assert!(
        !draft
            .read()
            .expect("current draft")
            .committed("first", first.revision)
    );
    assert!(
        draft
            .read()
            .expect("current draft")
            .committed("first", returned.revision)
    );
    assert!(draft.accepts(&returned));
}

#[test]
fn rebuilding_preserves_draft_without_accepting_previous_session() {
    let draft = SharedDraft::new();
    let initial = draft.read().expect("initial draft");
    let edited = draft
        .edit(&initial.session, Some("keep me"), None)
        .expect("edit");
    let rebuilt = draft.renew().expect("new session");
    assert_eq!(rebuilt.text, "keep me");
    assert_eq!(rebuilt.revision, edited.revision);
    assert!(
        draft
            .read()
            .expect("current draft")
            .committed("keep me", edited.revision)
    );
    assert!(!draft.accepts(&edited));
    assert!(draft.accepts(&rebuilt));
    assert!(
        draft
            .edit(&edited.session, Some("late stale edit"), None)
            .is_none()
    );
    assert_eq!(draft.read().expect("retained draft").text, "keep me");
}

#[test]
fn scope_navigation_cycles_in_both_directions() {
    let mut forward = PaletteScope::All;
    for expected in [
        PaletteScope::Actions,
        PaletteScope::Tasks,
        PaletteScope::Files,
        PaletteScope::All,
    ] {
        forward = next_scope(forward, false);
        assert_eq!(forward, expected);
    }
    let mut backward = PaletteScope::All;
    for expected in [
        PaletteScope::Files,
        PaletteScope::Tasks,
        PaletteScope::Actions,
        PaletteScope::All,
    ] {
        backward = next_scope(backward, true);
        assert_eq!(backward, expected);
    }
}
