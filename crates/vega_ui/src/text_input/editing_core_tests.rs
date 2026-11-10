use std::sync::Arc;

use super::editing_core::*;
use super::nsh::EditorGate;

fn edit(text: &str, selection: (u32, u32), serial: u64, composing: bool) -> NativeEdit {
    NativeEdit {
        text: Arc::from(text),
        selection,
        serial,
        composing,
        mutation: None,
    }
}

fn mutated(mut edit: NativeEdit, range_utf16: (u32, u32), inserted: &str) -> NativeEdit {
    edit.mutation = Some(NativeMutation {
        range_utf16,
        inserted: inserted.into(),
    });
    edit
}

fn bridge() -> (ComposerNativeBridge, EditorOwner, u64) {
    let bridge = ComposerNativeBridge::new(edit("", (0, 0), 0, false), (0, 0), EditorGate::new());
    let owner = bridge.attach().expect("initial native mount");
    let focus = bridge.focus(&owner, true).expect("current native focus");
    (bridge, owner, focus)
}

fn primary() -> NativeModifiers {
    if cfg!(target_os = "macos") {
        NativeModifiers {
            meta: true,
            ..Default::default()
        }
    } else {
        NativeModifiers {
            control: true,
            ..Default::default()
        }
    }
}

#[test]
fn fast_local_edits_are_not_replaced_or_settled_by_earlier_receipts() {
    let (bridge, owner, _) = bridge();
    let first = mutated(edit("中", (1, 1), 1, false), (0, 0), "中");
    let second = mutated(edit("中文😀", (4, 2), 2, false), (1, 1), "文😀");
    assert!(bridge.observe(&owner, first.clone()));
    assert!(bridge.observe(&owner, second.clone()));
    let mut authority = edit("", (0, 0), 0, false);
    assert!(bridge.commit(&owner, &first, |before, revisions| {
        assert_eq!(before, &authority);
        assert_eq!(revisions, (0, 0));
        authority = first.clone();
        Some((1, 1))
    }));
    assert_eq!(bridge.local_draft(), second);
    assert!(!bridge.ready("中", (1, 1)));
    assert!(!bridge.ready("中文😀", (4, 2)));
    assert!(bridge.commit(&owner, &second, |before, revisions| {
        assert_eq!(before, &authority);
        assert_eq!(revisions, (1, 1));
        authority = second.clone();
        Some((2, 2))
    }));
    assert_eq!(authority, second);
    assert!(bridge.ready("中文😀", (4, 2)));
}

#[test]
fn request_receipts_only_synchronize_the_last_current_ticket() {
    let mut ledger = EditLedger::new();
    let first = ledger.issue().expect("first request");
    let last = ledger.issue().expect("last request");
    assert_eq!(ledger.accept(&first), ReceiptDecision::Advance);
    assert_eq!(ledger.accept(&last), ReceiptDecision::Synchronize);
    assert_eq!(ledger.accept(&last), ReceiptDecision::Reject);
    assert!(ledger.issue().is_none());
}

#[test]
fn out_of_order_receipt_freezes_instead_of_skipping_an_edit() {
    let mut ledger = EditLedger::new();
    let first = ledger.issue().expect("first request");
    let second = ledger.issue().expect("second request");
    assert_eq!(ledger.accept(&second), ReceiptDecision::Reject);
    assert_eq!(ledger.accept(&first), ReceiptDecision::Reject);
    assert!(ledger.issue().is_none());
}

#[test]
fn old_session_receipt_cannot_authorize_a_reloaded_session() {
    let mut old = EditLedger::new();
    let old_ticket = old.issue().expect("old request");
    let mut current = EditLedger::new();
    let new_ticket = current.issue().expect("new request");
    assert!(!old_ticket.matches(&new_ticket));
    assert_eq!(current.accept(&old_ticket), ReceiptDecision::Reject);
    assert_eq!(current.accept(&new_ticket), ReceiptDecision::Reject);
}

#[test]
fn unknown_outcome_preserves_local_text_and_never_replays_commit() {
    let (bridge, owner, _) = bridge();
    let local = mutated(
        edit("unsent\n中文😀", (11, 11), 1, false),
        (0, 0),
        "unsent\n中文😀",
    );
    assert!(bridge.observe(&owner, local.clone()));
    let mut attempts = 0;
    assert!(!bridge.commit(&owner, &local, |_, _| {
        attempts += 1;
        None
    }));
    assert!(!bridge.commit(&owner, &local, |_, _| {
        attempts += 1;
        Some((1, 1))
    }));
    assert_eq!(attempts, 1);
    assert_eq!(bridge.local_draft(), local);
    assert!(!bridge.ready("unsent\n中文😀", (11, 11)));
}

#[test]
fn native_composition_keys_pass_even_before_engine_composition_arrives() {
    let (bridge, owner, focus) = bridge();
    bridge.route(NativeEditorRoute {
        popup: true,
        prediction: true,
        ..Default::default()
    });
    for key in [
        NativeEditorKey::Return,
        NativeEditorKey::Escape,
        NativeEditorKey::Tab,
        NativeEditorKey::Up,
        NativeEditorKey::Down,
        NativeEditorKey::Right,
        NativeEditorKey::Undo,
        NativeEditorKey::Redo,
        NativeEditorKey::Other,
    ] {
        for modifiers in [NativeModifiers::default(), primary()] {
            assert!(matches!(
                bridge.key(&owner, focus, key, modifiers, true, false),
                NativeKeyDecision::Pass
            ));
        }
    }
}

#[test]
fn single_line_native_candidate_keys_precede_policy_and_old_focus_cannot_submit() {
    let (bridge, owner, focus) = bridge();
    bridge.route(NativeEditorRoute {
        policy: NativeEditorPolicy::SingleLine(gpui_kit::EntityId::from(1_u64)),
        ..Default::default()
    });
    for key in [NativeEditorKey::Return, NativeEditorKey::Escape] {
        assert!(matches!(
            bridge.key(&owner, focus, key, NativeModifiers::default(), true, false),
            NativeKeyDecision::Pass
        ));
    }
    let NativeKeyDecision::Command(token) = bridge.key(
        &owner,
        focus,
        NativeEditorKey::Return,
        NativeModifiers::default(),
        false,
        false,
    ) else {
        panic!("fresh single-line submit");
    };
    assert_eq!(token.command, NativeEditorCommand::Submit);
    bridge.focus(&owner, false).expect("blur");
    let next = bridge.focus(&owner, true).expect("refocus");
    assert!(!bridge.validates(&token));
    assert!(matches!(
        bridge.key(
            &owner,
            focus,
            NativeEditorKey::Return,
            NativeModifiers::default(),
            false,
            false
        ),
        NativeKeyDecision::Pass
    ));
    assert!(matches!(
        bridge.key(
            &owner,
            next,
            NativeEditorKey::Escape,
            NativeModifiers::default(),
            false,
            false
        ),
        NativeKeyDecision::Command(_)
    ));
}

#[test]
fn pending_submit_is_consumed_without_a_deferred_command() {
    let (bridge, owner, focus) = bridge();
    let local = mutated(edit("new", (3, 3), 1, false), (0, 0), "new");
    assert!(bridge.observe(&owner, local.clone()));
    assert!(matches!(
        bridge.key(
            &owner,
            focus,
            NativeEditorKey::Return,
            primary(),
            false,
            false
        ),
        NativeKeyDecision::Consume
    ));
    assert!(bridge.commit(&owner, &local, |_, _| Some((1, 1))));
    let NativeKeyDecision::Command(token) = bridge.key(
        &owner,
        focus,
        NativeEditorKey::Return,
        primary(),
        false,
        false,
    ) else {
        panic!("only a new settled key creates a command");
    };
    assert_eq!(token.command, NativeEditorCommand::Submit);
    assert!(token.claim());
    assert!(!token.clone().claim());
    assert!(matches!(
        bridge.key(
            &owner,
            focus,
            NativeEditorKey::Return,
            primary(),
            false,
            true
        ),
        NativeKeyDecision::Consume
    ));
}

#[test]
fn newline_popup_priority_and_focus_retirement_are_distinct() {
    let (bridge, owner, focus) = bridge();
    assert!(matches!(
        bridge.key(
            &owner,
            focus,
            NativeEditorKey::Return,
            NativeModifiers::default(),
            false,
            false
        ),
        NativeKeyDecision::Pass
    ));
    bridge.route(NativeEditorRoute {
        popup: true,
        prediction: true,
        ..Default::default()
    });
    let NativeKeyDecision::Command(token) = bridge.key(
        &owner,
        focus,
        NativeEditorKey::Return,
        NativeModifiers::default(),
        false,
        false,
    ) else {
        panic!("popup Return accepts");
    };
    assert_eq!(token.command, NativeEditorCommand::Accept);
    assert!(bridge.validates(&token));
    bridge.focus(&owner, false).expect("blur");
    bridge.focus(&owner, true).expect("refocus");
    assert!(!bridge.validates(&token));
    assert!(matches!(
        bridge.key(
            &owner,
            focus,
            NativeEditorKey::Escape,
            NativeModifiers::default(),
            false,
            false
        ),
        NativeKeyDecision::Pass
    ));
    bridge.retire();
    assert!(!bridge.validates(&token));
    assert!(bridge.attach().is_none());
}

#[test]
fn changed_menu_identity_rejects_previously_created_navigation() {
    let (bridge, owner, focus) = bridge();
    bridge.route(NativeEditorRoute {
        popup: true,
        action_generation: 1,
        ..Default::default()
    });
    let NativeKeyDecision::Command(token) = bridge.key(
        &owner,
        focus,
        NativeEditorKey::Down,
        NativeModifiers::default(),
        false,
        false,
    ) else {
        panic!("current popup navigation");
    };
    bridge.route(NativeEditorRoute {
        popup: true,
        action_generation: 2,
        ..Default::default()
    });
    assert!(!bridge.validates(&token));
}

#[test]
fn selection_preserves_direction_and_rejects_split_surrogate_or_outside_text() {
    assert!(valid_selection("中😀文", (4, 1)));
    assert!(valid_selection("中😀文", (1, 3)));
    assert!(!valid_selection("中😀文", (2, 2)));
    assert!(!valid_selection("中😀文", (0, 5)));
    let (bridge, owner, _) = bridge();
    let invalid = mutated(edit("😀", (1, 1), 1, false), (0, 0), "😀");
    assert!(!bridge.observe(&owner, invalid.clone()));
    assert_eq!(bridge.local_draft(), invalid);
    assert!(!bridge.ready("😀", (1, 1)));
}

#[test]
fn composition_end_settles_only_after_its_own_authoritative_commit() {
    let (bridge, owner, _) = bridge();
    let marked = mutated(edit("候选", (2, 2), 1, true), (0, 0), "候选");
    let committed = edit("候选", (2, 2), 2, false);
    assert!(bridge.observe(&owner, marked.clone()));
    assert!(bridge.commit(&owner, &marked, |_, _| Some((1, 1))));
    assert!(!bridge.ready("候选", (2, 2)));
    assert!(bridge.observe(&owner, committed.clone()));
    assert!(!bridge.ready("候选", (2, 2)));
    assert!(bridge.commit(&owner, &committed, |_, _| Some((1, 2))));
    assert!(bridge.ready("候选", (2, 2)));
}

#[test]
fn overflow_preserves_the_latest_local_draft_but_freezes_delivery() {
    let (bridge, owner, _) = bridge();
    for serial in 1..=64 {
        assert!(bridge.observe(
            &owner,
            mutated(
                edit("x", (1, 1), serial, false),
                (0, if serial == 1 { 0 } else { 1 }),
                "x"
            )
        ));
    }
    let latest = mutated(
        edit("not discarded", (13, 13), 65, false),
        (0, 1),
        "not discarded",
    );
    assert!(!bridge.observe(&owner, latest.clone()));
    assert_eq!(bridge.local_draft(), latest);
    let mut applied = false;
    assert!(!bridge.commit(
        &owner,
        &mutated(edit("x", (1, 1), 1, false), (0, 0), "x"),
        |_, _| {
            applied = true;
            Some((1, 1))
        }
    ));
    assert!(!applied);
}

#[test]
fn authoritative_projection_cannot_overwrite_pending_native_text() {
    let (bridge, owner, _) = bridge();
    let local = mutated(edit("local", (5, 5), 1, false), (0, 0), "local");
    assert!(bridge.observe(&owner, local.clone()));
    let mut projected = false;
    assert!(
        !bridge.project(&owner, 1, Arc::from("old"), (3, 3), (1, 1), |_| {
            projected = true;
            true
        })
    );
    assert!(!projected);
    assert_eq!(bridge.local_draft(), local);
    assert!(bridge.commit(&owner, &local, |_, _| Some((1, 1))));
    assert!(
        bridge.project(&owner, 1, Arc::from("undo"), (4, 0), (2, 1), |native| {
            assert_eq!(native.text.as_ref(), "undo");
            assert_eq!(native.selection, (4, 0));
            assert_eq!(native.serial, 2);
            true
        })
    );
    assert!(bridge.ready("undo", (4, 0)));
    assert!(!bridge.project(&owner, 1, Arc::from("stale"), (5, 5), (3, 1), |_| true));
}

#[test]
fn stale_native_notification_cannot_replace_a_newer_local_draft() {
    let (bridge, owner, _) = bridge();
    let first = mutated(edit("first", (5, 5), 1, false), (0, 0), "first");
    let newer = mutated(
        edit("newer draft", (11, 11), 2, false),
        (0, 5),
        "newer draft",
    );
    assert!(bridge.observe(&owner, first.clone()));
    assert!(bridge.observe(&owner, newer.clone()));
    assert!(!bridge.observe(&owner, first));
    assert_eq!(bridge.local_draft(), newer);
    assert!(!bridge.ready("newer draft", (11, 11)));
}

#[test]
fn explicit_restore_cannot_cross_pending_paste_or_changed_authority() {
    let (bridge, owner, _) = bridge();
    let token = bridge.restore_token().expect("settled restore");
    let before = bridge.local_draft();
    assert!(bridge.begin_paste(&owner, &before));
    assert!(bridge.restore_token().is_none());
    assert!(!bridge.restore_authority(&token, (0, 0), "", (0, 0), || panic!("pending paste")));
    let pasted = mutated(edit("paste", (5, 5), 1, false), (0, 0), "paste");
    assert!(bridge.observe_paste(&owner, &before, pasted.clone()));
    assert!(!bridge.restore_authority(&token, (0, 0), "", (0, 0), || panic!("pending edit")));
    assert!(bridge.commit(&owner, &pasted, |_, _| Some((1, 1))));
    assert!(!bridge.restore_authority(&token, (1, 1), "paste", (5, 5), || panic!("stale token")));
    let current = bridge.restore_token().expect("settled paste");
    assert!(!bridge.restore_authority(&current, (2, 1), "paste", (5, 5), || panic!("conflict")));
    assert!(bridge.ready("paste", (5, 5)));
    assert!(bridge.restore_authority(&current, (1, 1), "paste", (5, 5), || true));
    assert!(!bridge.ready("paste", (5, 5)));
    assert!(!bridge.restore_authority(&current, (1, 1), "paste", (5, 5), || panic!("replay")));
}

#[test]
fn mutation_mismatch_or_missing_provenance_freezes_without_applying_or_replaying() {
    for mutation in [
        None,
        Some(NativeMutation {
            range_utf16: (0, 0),
            inserted: "wrong".into(),
        }),
        Some(NativeMutation {
            range_utf16: (1, 0),
            inserted: "new".into(),
        }),
        Some(NativeMutation {
            range_utf16: (0, 1),
            inserted: "new".into(),
        }),
    ] {
        let (bridge, owner, _) = bridge();
        let mut local = edit("new", (3, 3), 1, false);
        local.mutation = mutation;
        assert!(!bridge.observe(&owner, local.clone()));
        assert_eq!(bridge.local_draft(), local);
        assert!(bridge.failed());
        assert!(!bridge.ready("new", (3, 3)));
        assert!(!bridge.commit(&owner, &local, |_, _| panic!("invalid mutation applied")));
        assert!(!bridge.observe(&owner, mutated(edit("new!", (4, 4), 2, false), (3, 3), "!")));
    }
}

#[test]
fn mutation_range_rejects_split_surrogates_and_untouched_text_changes() {
    for mutation in [
        NativeMutation {
            range_utf16: (2, 3),
            inserted: "x".into(),
        },
        NativeMutation {
            range_utf16: (1, 3),
            inserted: "y".into(),
        },
        NativeMutation {
            range_utf16: (1, 3),
            inserted: "x".into(),
        },
    ] {
        let initial = edit("中😀文", (3, 1), 0, false);
        let bridge = ComposerNativeBridge::new(initial, (0, 0), EditorGate::new());
        let owner = bridge.attach().expect("native mount");
        let mut local = edit("中x末", (2, 2), 1, false);
        local.mutation = Some(mutation);
        assert!(!bridge.observe(&owner, local.clone()));
        assert_eq!(bridge.local_draft(), local);
        assert!(!bridge.commit(&owner, &local, |_, _| panic!("invalid range applied")));
    }
}

#[test]
fn ordered_receipt_cannot_substitute_an_equivalent_text_mutation() {
    let initial = edit("@foo", (4, 0), 0, false);
    let bridge = ComposerNativeBridge::new(initial, (0, 0), EditorGate::new());
    let owner = bridge.attach().expect("native mount");
    let original = mutated(edit("@foobar", (7, 7), 1, false), (0, 4), "@foobar");
    assert!(bridge.observe(&owner, original.clone()));
    let inferred = mutated(edit("@foobar", (7, 7), 1, false), (4, 4), "bar");
    assert!(!bridge.commit(&owner, &inferred, |_, _| panic!(
        "substituted mutation applied"
    )));
    assert!(!bridge.commit(&owner, &original, |_, _| panic!("unknown receipt replayed")));
    assert_eq!(bridge.local_draft(), original);
    assert!(!bridge.ready("@foobar", (7, 7)));
}

#[test]
fn restore_rejects_old_owner_composition_and_pending_command() {
    let (old, _, _) = bridge();
    let old_token = old.restore_token().expect("old token");
    let (current, owner, epoch) = bridge();
    assert!(!current.restore_authority(&old_token, (0, 0), "", (0, 0), || panic!("old owner")));
    let command = current.key(
        &owner,
        epoch,
        NativeEditorKey::Return,
        primary(),
        false,
        false,
    );
    let NativeKeyDecision::Command(command) = command else {
        panic!("submit token")
    };
    assert!(current.restore_token().is_none());
    current.finish_command(&command);
    let composing = mutated(edit("中", (1, 1), 1, true), (0, 0), "中");
    assert!(current.observe(&owner, composing.clone()));
    assert!(current.commit(&owner, &composing, |_, _| Some((1, 1))));
    assert!(current.restore_token().is_none());
}
