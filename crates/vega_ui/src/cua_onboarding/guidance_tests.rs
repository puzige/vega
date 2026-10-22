use super::controller::{CardAction, CardController, CardEffect, CheckState};
use super::lifecycle::EntryLifecycle;
use super::retention::PrepareEntryKind;
use std::path::Path;
use vega_conversation::types::{
    ComponentState, ComputerUseError, DirectCapture, ExtensionState, PermissionState, Preparation,
};

fn preparation(
    component: ComponentState,
    ax: PermissionState,
    screen: PermissionState,
) -> Preparation {
    Preparation {
        component,
        accessibility: ax,
        screen_recording: screen,
        direct_capture: DirectCapture::NotChecked,
        browser_extension: ExtensionState::NotEvaluated,
    }
}

fn card() -> CardController {
    CardController::new(
        "task-a",
        Path::new("/Applications/Vega.app/Contents/MacOS/vega"),
    )
}

fn request(card: &mut CardController) -> super::controller::CheckTicket {
    let effects = card.act(CardAction::Recheck);
    assert_eq!(effects.len(), 1);
    let CardEffect::Inspect {
        ticket,
        prompt,
        probe_direct_capture,
    } = &effects[0]
    else {
        panic!("recheck must request read-only inspection");
    };
    assert!(!prompt);
    assert!(!probe_direct_capture);
    ticket.clone()
}

#[test]
fn observed_component_and_permissions_remain_separate() {
    let mut card = card();
    let ticket = request(&mut card);
    assert!(
        card.complete(
            ticket,
            Ok(preparation(
                ComponentState::Missing,
                PermissionState::Granted,
                PermissionState::Denied,
            )),
        )
        .is_empty()
    );
    let view = card.view();
    assert_eq!(view.component, Some(ComponentState::Missing));
    assert_eq!(view.accessibility, PermissionState::Granted);
    assert_eq!(view.screen_recording, PermissionState::Denied);
    assert_eq!(view.direct_capture, DirectCapture::NotChecked);
    assert!(view.can_recheck);
    assert!(view.open);
}

#[test]
fn granted_static_checks_never_complete_or_close_the_card() {
    let mut card = card();
    let ticket = request(&mut card);
    assert!(
        card.complete(
            ticket,
            Ok(preparation(
                ComponentState::Available,
                PermissionState::Granted,
                PermissionState::Granted,
            ))
        )
        .is_empty()
    );
    let view = card.view();
    assert_eq!(view.direct_capture, DirectCapture::NotChecked);
    assert!(!view.desktop_ready);
    assert!(view.open);
    assert!(view.can_return_to_task);
}

#[test]
fn busy_unknown_and_denied_are_distinct_and_retryable() {
    let mut card = card();
    let ticket = request(&mut card);
    assert!(
        card.complete(ticket, Err(ComputerUseError::Busy))
            .is_empty()
    );
    assert_eq!(card.view().check_state, CheckState::Busy);
    assert_eq!(card.view().accessibility, PermissionState::Unknown);
    assert!(card.view().can_recheck);
    let ticket = request(&mut card);
    assert!(
        card.complete(
            ticket,
            Ok(preparation(
                ComponentState::Available,
                PermissionState::Unknown,
                PermissionState::Unknown,
            ))
        )
        .is_empty()
    );
    assert_eq!(card.view().check_state, CheckState::Unknown);
    assert!(card.view().can_recheck);
    let ticket = request(&mut card);
    card.complete(
        ticket,
        Ok(preparation(
            ComponentState::Available,
            PermissionState::Denied,
            PermissionState::Denied,
        )),
    );
    assert_eq!(card.view().check_state, CheckState::Observed);
    assert_eq!(card.view().accessibility, PermissionState::Denied);
    assert_eq!(card.view().screen_recording, PermissionState::Denied);
    assert!(card.view().can_recheck);
}

#[test]
fn settings_actions_only_open_the_requested_privacy_route() {
    let mut card = card();
    assert_eq!(
        card.act(CardAction::AccessibilitySettings),
        vec![CardEffect::OpenUrl(
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility".into(),
        )]
    );
    assert_eq!(
        card.act(CardAction::ScreenRecordingSettings),
        vec![CardEffect::OpenUrl(
            "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture".into(),
        )]
    );
    assert!(card.view().open);
    assert_eq!(card.view().direct_capture, DirectCapture::NotChecked);
}

#[test]
fn drag_constructor_freezes_following_for_lifetime_but_not_recheck() {
    let mut card = card();
    assert!(card.view().following);
    assert_eq!(
        card.begin_drag(),
        Some(Path::new("/Applications/Vega.app").to_owned())
    );
    assert!(!card.view().following);
    card.drag_ended();
    assert!(!card.view().following);
    let ticket = request(&mut card);
    card.complete(
        ticket,
        Ok(preparation(
            ComponentState::Available,
            PermissionState::Granted,
            PermissionState::Granted,
        )),
    );
    assert!(!card.view().following);
    assert!(card.view().open);
    assert_eq!(card.view().direct_capture, DirectCapture::NotChecked);
    assert!(self::card().view().following);
}

#[test]
fn development_executable_has_no_drag_and_finder_reveals_real_executable() {
    let executable = Path::new("/workspace/vega/target/debug/vega");
    let mut card = CardController::new("task-a", executable);
    assert!(!card.view().can_drag);
    assert_eq!(card.begin_drag(), None);
    assert_eq!(
        card.act(CardAction::RevealInFinder),
        vec![CardEffect::RevealInFinder(executable.to_owned())]
    );
    assert!(card.view().following);
}

#[test]
fn packaged_finder_action_reveals_the_app_not_a_guessed_installation() {
    let mut card = CardController::new(
        "task-a",
        Path::new("/Users/test/Downloads/Vega.app/Contents/MacOS/vega"),
    );
    assert_eq!(
        card.act(CardAction::RevealInFinder),
        vec![CardEffect::RevealInFinder(
            Path::new("/Users/test/Downloads/Vega.app").to_owned()
        )]
    );
}

#[test]
fn close_invalidates_callbacks_and_all_later_actions() {
    let mut card = card();
    let ticket = request(&mut card);
    assert_eq!(
        card.act(CardAction::Close),
        vec![CardEffect::CancelPending, CardEffect::CloseWindow]
    );
    let closed = card.view();
    assert!(!closed.open);
    assert!(!closed.following);
    assert!(!closed.can_recheck);
    assert!(
        card.complete(
            ticket,
            Ok(preparation(
                ComponentState::Available,
                PermissionState::Granted,
                PermissionState::Granted,
            ))
        )
        .is_empty()
    );
    for action in [
        CardAction::Recheck,
        CardAction::AccessibilitySettings,
        CardAction::ScreenRecordingSettings,
        CardAction::RevealInFinder,
        CardAction::ReturnToTask,
        CardAction::Close,
    ] {
        assert!(card.act(action).is_empty());
    }
    assert_eq!(card.begin_drag(), None);
    card.drag_ended();
    assert_eq!(card.view(), closed);
}

#[test]
fn older_check_generation_cannot_overwrite_newer_observation() {
    let mut card = card();
    let older = request(&mut card);
    let newer = request(&mut card);
    assert_ne!(older, newer);
    card.complete(
        newer,
        Ok(preparation(
            ComponentState::Available,
            PermissionState::Granted,
            PermissionState::Denied,
        )),
    );
    let current = card.view();
    assert!(card.complete(older, Err(ComputerUseError::Busy)).is_empty());
    assert_eq!(card.view(), current);
}

#[test]
fn callbacks_from_another_task_or_closed_card_lifetime_are_rejected() {
    let mut old = card();
    let old_ticket = request(&mut old);
    old.act(CardAction::Close);
    for task in ["task-a", "task-b"] {
        let mut next = CardController::new(
            task,
            Path::new("/Applications/Vega.app/Contents/MacOS/vega"),
        );
        let _current = request(&mut next);
        let before = next.view();
        assert!(
            next.complete(old_ticket.clone(), Err(ComputerUseError::Permissions))
                .is_empty()
        );
        assert_eq!(next.view(), before);
    }
}

#[test]
fn return_only_navigates_to_original_task_and_never_resumes_authorized_actions() {
    let mut card = card();
    let pending = request(&mut card);
    assert_eq!(
        card.act(CardAction::ReturnToTask),
        vec![
            CardEffect::CancelPending,
            CardEffect::CloseWindow,
            CardEffect::ReturnToTask("task-a".into())
        ]
    );
    assert!(!card.view().open);
    assert!(
        card.complete(pending, Err(ComputerUseError::Permissions))
            .is_empty()
    );
    assert!(card.act(CardAction::ReturnToTask).is_empty());
}

#[test]
fn detach_captures_outgoing_permission_error_and_repeated_detach_preserves_entry() {
    let mut entry = EntryLifecycle::default();
    let context = entry.begin_task("task-a");
    entry.detach(&context, Some(ComputerUseError::Permissions));
    assert_eq!(entry.visible(), Some(PrepareEntryKind::Permissions));
    entry.detach(&context, None);
    entry.detach(&context, None);
    assert_eq!(entry.visible(), Some(PrepareEntryKind::Permissions));
}

#[test]
fn explicit_dismiss_survives_repeated_status_and_detach_for_same_task() {
    let mut entry = EntryLifecycle::default();
    let context = entry.begin_task("task-a");
    entry.observe(&context, ComputerUseError::Permissions);
    entry.dismiss(&context);
    entry.observe(&context, ComputerUseError::Permissions);
    entry.detach(&context, Some(ComputerUseError::Permissions));
    entry.detach(&context, None);
    assert_eq!(entry.visible(), None);
}

#[test]
fn nonpermission_failures_do_not_create_a_permission_entry() {
    for error in [
        ComputerUseError::Protocol,
        ComputerUseError::Unavailable,
        ComputerUseError::Busy,
        ComputerUseError::Rejected,
    ] {
        let mut entry = EntryLifecycle::default();
        let context = entry.begin_task("task-a");
        entry.observe(&context, error);
        entry.detach(&context, Some(error));
        assert_eq!(entry.visible(), None);
    }
}

#[test]
fn new_task_generation_rejects_old_observe_detach_and_dismiss_callbacks() {
    let mut entry = EntryLifecycle::default();
    let old = entry.begin_task("task-a");
    entry.observe(&old, ComputerUseError::Permissions);
    let current = entry.begin_task("task-b");
    assert_eq!(entry.visible(), None);
    entry.observe(&old, ComputerUseError::Permissions);
    entry.detach(&old, Some(ComputerUseError::Permissions));
    assert_eq!(entry.visible(), None);
    entry.observe(&current, ComputerUseError::Permissions);
    entry.dismiss(&old);
    assert_eq!(entry.visible(), Some(PrepareEntryKind::Permissions));
    entry.dismiss(&current);
    let restarted = entry.begin_task("task-b");
    assert_ne!(current, restarted);
    entry.observe(&current, ComputerUseError::Permissions);
    assert_eq!(entry.visible(), None);
    entry.observe(&restarted, ComputerUseError::Permissions);
    assert_eq!(entry.visible(), Some(PrepareEntryKind::Permissions));
}
