use super::*;
use crate::cua_onboarding::controller::CardController;
use std::path::Path;

fn snapshot(stage: Stage) -> Snapshot {
    let controller = CardController::new(
        "task",
        Path::new("/Applications/Vega.app/Contents/MacOS/vega"),
    )
    .view();
    Snapshot {
        id: ProjectionId::new(),
        authority: Authority {
            stage,
            stage_identity: CallbackIdentity::default(),
            pane: PermissionPane::Accessibility,
            has_task: true,
            placed: true,
            controller: controller.clone(),
            preparation: controller,
            preparation_revision: 7,
            retained: false,
            local_fault: None,
            warning: None,
            path_fault: false,
        },
        icon: ImageSrc::svg_markup("<svg xmlns='http://www.w3.org/2000/svg'/>"),
        feedback: Vec::new(),
        colors: vega_theme::LIGHT,
        appearance: vega_theme::Appearance::Light,
        language: "en",
        selected: None,
        selection_revision: 0,
        reveal_revision: None,
        keyboard_active: false,
        recovery: false,
    }
}

fn measurement(page_y: f32, height: f32) -> Measurement {
    Measurement {
        frame: Bounds {
            origin: Point {
                x: Points(17.),
                y: Points(900.),
            },
            size: Size {
                width: Points(120.),
                height: Points(height),
            },
        },
        page: Point {
            x: Points(40.),
            y: Points(page_y),
        },
    }
}

#[test]
fn reveal_uses_scrolled_page_geometry_and_only_the_missing_viewport_distance() {
    let viewport = measurement(100., 200.);
    let scroll = ScrollEvent {
        offset: Point {
            x: Points(9.),
            y: Points(250.),
        },
        content_size: Size {
            width: Points(200.),
            height: Points(1000.),
        },
        size: viewport.frame.size,
        zoom_scale: 1.,
    };
    assert_eq!(
        reveal::destination(measurement(120., 40.), viewport, scroll),
        None
    );
    assert_eq!(
        reveal::destination(measurement(100., 200.), viewport, scroll),
        None
    );
    assert_eq!(
        reveal::destination(measurement(80., 40.), viewport, scroll),
        Some(Point {
            x: Points(9.),
            y: Points(230.)
        })
    );
    assert_eq!(
        reveal::destination(measurement(290., 40.), viewport, scroll),
        Some(Point {
            x: Points(9.),
            y: Points(280.)
        })
    );
    assert_eq!(
        reveal::destination(measurement(140., 240.), viewport, scroll),
        Some(Point {
            x: Points(9.),
            y: Points(290.)
        })
    );
    assert_eq!(
        reveal::destination(measurement(100., 240.), viewport, scroll),
        None
    );
    assert_eq!(
        reveal::destination(measurement(-400., 40.), viewport, scroll),
        Some(Point {
            x: Points(9.),
            y: Points(0.)
        })
    );
    assert_eq!(
        reveal::destination(measurement(1000., 40.), viewport, scroll),
        Some(Point {
            x: Points(9.),
            y: Points(800.)
        })
    );
    assert_eq!(
        reveal::destination(measurement(f32::NAN, 40.), viewport, scroll),
        None
    );
    assert_eq!(
        reveal::destination(measurement(290., 40.), measurement(100., 0.), scroll),
        None
    );
}

#[test]
fn reveal_requires_the_live_keyboard_revision_and_current_displayed_authority() {
    let action = Action::Controller(CardAction::Recheck);
    let mut shown = snapshot(Stage::Overview);
    shown.keyboard_active = true;
    shown.selected = Some(action);
    shown.selection_revision = 12;
    shown.reveal_revision = Some(12);
    assert!(shown.accepts_reveal(&shown, action));
    let mut current = shown.clone();
    current.selection_revision += 1;
    current.reveal_revision = Some(current.selection_revision);
    assert!(!current.accepts_reveal(&shown, action));
    current = shown.clone();
    current.selected = Some(Action::Controller(CardAction::Close));
    assert!(!current.accepts_reveal(&shown, action));
    current = shown.clone();
    current.keyboard_active = false;
    assert!(!current.accepts_reveal(&shown, action));
    current = shown.clone();
    current.reveal_revision = None;
    assert!(!current.accepts_reveal(&shown, action));
    current = shown.clone();
    current.authority.preparation.can_recheck = false;
    assert!(!current.accepts_reveal(&shown, action));
    current = shown.clone();
    current.id = ProjectionId::new();
    assert!(!current.accepts_reveal(&shown, action));
    current = shown.clone();
    current.authority.stage_identity = CallbackIdentity::default();
    assert!(!current.accepts_reveal(&shown, action));
    current = shown.clone();
    current.recovery = true;
    assert!(!current.accepts_reveal(&shown, action));
    current = shown.clone();
    current.authority.pane = PermissionPane::ScreenRecording;
    assert!(!current.accepts_reveal(&shown, action));
    current = shown.clone();
    current.authority.preparation_revision += 1;
    assert!(!current.accepts_reveal(&shown, action));
    shown.selected = Some(Action::Diagnostics);
    shown.authority.warning = Some("failure".into());
    assert!(shown.accepts_reveal(&shown, Action::Diagnostics));
    current = shown.clone();
    current.authority.warning = None;
    assert!(!current.accepts_reveal(&shown, Action::Diagnostics));
}

#[test]
fn old_surface_stage_and_pane_cannot_act_on_replacement() {
    let shown = snapshot(Stage::Guide);
    assert!(shown.accepts_action(&shown, Action::Back));
    assert!(shown.accepts_drag(&shown));
    let mut current = shown.clone();
    current.id = ProjectionId::new();
    assert!(!current.accepts_action(&shown, Action::Back));
    assert!(!current.accepts_drag(&shown));
    current = shown.clone();
    current.authority.stage_identity = CallbackIdentity::default();
    assert!(!current.accepts_action(&shown, Action::Back));
    assert!(!current.accepts_drag(&shown));
    current = shown.clone();
    current.authority.pane = PermissionPane::ScreenRecording;
    assert!(!current.accepts_action(&shown, Action::Controller(CardAction::RevealInFinder)));
    assert!(!current.accepts_drag(&shown));
    current = shown.clone();
    current.authority.controller.open = false;
    assert!(!current.accepts_action(&shown, Action::Controller(CardAction::Close)));
    assert!(!current.accepts_drag(&shown));
}

#[test]
fn completed_check_cycle_does_not_reauthorize_old_recheck() {
    let shown = snapshot(Stage::Overview);
    let recheck = Action::Controller(CardAction::Recheck);
    assert!(shown.accepts_action(&shown, recheck));
    let mut current = shown.clone();
    current.authority.preparation.can_recheck = false;
    assert!(!current.accepts_action(&shown, recheck));
    current.authority.preparation.can_recheck = true;
    current.authority.preparation_revision += 1;
    assert!(!current.accepts_action(&shown, recheck));
    assert!(current.accepts_action(&shown, Action::Controller(CardAction::Close)));
    assert!(current.accepts_action(&shown, Action::Controller(CardAction::ReturnToTask)));
}

#[test]
fn recovery_preserves_independent_escape_routes_without_drag_or_open_side_effects() {
    let mut current = snapshot(Stage::Guide);
    let shown = current.clone();
    current.recovery = true;
    current.authority.preparation.can_recheck = false;
    assert!(current.permits(Action::Controller(CardAction::Close)));
    assert!(current.permits(Action::Back));
    assert!(current.permits(Action::Diagnostics));
    assert!(current.permits(Action::Reload));
    assert!(current.permits(Action::Controller(CardAction::ReturnToTask)));
    assert!(!current.permits(Action::Controller(CardAction::Recheck)));
    assert!(!current.permits(Action::Controller(CardAction::AccessibilitySettings)));
    assert!(!current.permits(Action::Controller(CardAction::RevealInFinder)));
    assert!(!current.accepts_drag(&shown));
    assert!(!current.accepts_action(&shown, Action::Controller(CardAction::Close)));
    current.authority.has_task = false;
    assert!(!current.permits(Action::Controller(CardAction::ReturnToTask)));
}

#[test]
fn local_fault_disables_drag_without_blocking_reopen_back_and_diagnostics() {
    let shown = snapshot(Stage::Guide);
    let mut current = shown.clone();
    current.authority.local_fault = Some("window unavailable".into());
    current.authority.warning = current.authority.local_fault.clone();
    assert!(!current.accepts_drag(&shown));
    assert!(current.permits(Action::Controller(CardAction::AccessibilitySettings)));
    assert!(!current.permits(Action::Controller(CardAction::ScreenRecordingSettings)));
    assert!(current.permits(Action::Back));
    assert!(current.permits(Action::Diagnostics));
    assert!(current.permits(Action::Controller(CardAction::Close)));
    assert!(!current.permits(Action::Controller(CardAction::RevealInFinder)));
    current.authority.path_fault = true;
    assert!(current.permits(Action::Controller(CardAction::RevealInFinder)));
    current = shown.clone();
    current.authority.placed = false;
    assert!(!current.accepts_drag(&shown));
}

#[test]
fn native_hit_region_cannot_cover_header_padding_or_nonfinite_geometry() {
    let target = DragBounds {
        x: Layout::CONTENT_PADDING
            + Layout::PREPARATION_GUIDE_CONTROL
            + Layout::PREPARATION_GUIDE_GAP,
        y: Layout::CONTENT_PADDING
            + Layout::PREPARATION_GUIDE_CONTROL
            + Layout::PREPARATION_GUIDE_GAP,
        width: Layout::PREPARATION_APP_TARGET_HEIGHT,
        height: Layout::PREPARATION_APP_TARGET_HEIGHT,
    };
    assert!(target.valid(Stage::Guide));
    assert!(!target.valid(Stage::Overview));
    for invalid in [
        DragBounds {
            x: f32::NAN,
            ..target
        },
        DragBounds {
            y: f32::INFINITY,
            ..target
        },
        DragBounds {
            y: Layout::CONTENT_PADDING,
            ..target
        },
        DragBounds { x: 0., ..target },
        DragBounds {
            width: -1.,
            ..target
        },
        DragBounds {
            width: Layout::PREPARATION_CARD_WIDTH,
            ..target
        },
        DragBounds {
            height: Layout::PREPARATION_CARD_HEIGHT,
            ..target
        },
    ] {
        assert!(!invalid.valid(Stage::Guide));
    }
}

fn displayed(snapshot: &Snapshot) -> Vec<Action> {
    snapshot
        .controls()
        .into_iter()
        .map(|control| control.action)
        .collect()
}

fn advance(
    selection: Option<&keyboard::Selection>,
    snapshot: &Snapshot,
    backwards: bool,
) -> keyboard::Selection {
    match keyboard::Selection::advance(selection, snapshot, backwards) {
        Some(selection) => selection,
        None => panic!("open card must have enabled controls"),
    }
}

#[test]
fn overview_keyboard_order_matches_displayed_controls_and_optional_routes() {
    let mut current = snapshot(Stage::Overview);
    assert_eq!(
        displayed(&current),
        vec![
            Action::Controller(CardAction::Close),
            Action::Controller(CardAction::AccessibilitySettings),
            Action::Controller(CardAction::ScreenRecordingSettings),
            Action::Controller(CardAction::Recheck),
            Action::Controller(CardAction::ReturnToTask),
        ]
    );
    current.authority.warning = Some("check unavailable".into());
    current.authority.has_task = false;
    assert_eq!(
        displayed(&current),
        vec![
            Action::Controller(CardAction::Close),
            Action::Controller(CardAction::AccessibilitySettings),
            Action::Controller(CardAction::ScreenRecordingSettings),
            Action::Controller(CardAction::Recheck),
            Action::Diagnostics,
        ]
    );
}

#[test]
fn guide_keyboard_order_tracks_bundle_fallback_and_local_fault_routes() {
    let mut current = snapshot(Stage::Guide);
    assert_eq!(
        displayed(&current),
        vec![
            Action::Back,
            Action::Controller(CardAction::Close),
            Action::Controller(CardAction::RevealInFinder)
        ]
    );
    current.authority.controller.can_drag = false;
    assert_eq!(
        displayed(&current),
        vec![
            Action::Back,
            Action::Controller(CardAction::Close),
            Action::Controller(CardAction::RevealInFinder)
        ]
    );
    current.authority.local_fault = Some("missing path".into());
    current.authority.warning = current.authority.local_fault.clone();
    current.authority.pane = PermissionPane::ScreenRecording;
    assert_eq!(
        displayed(&current),
        vec![
            Action::Back,
            Action::Controller(CardAction::Close),
            Action::Controller(CardAction::ScreenRecordingSettings),
            Action::Controller(CardAction::RevealInFinder),
            Action::Diagnostics
        ]
    );
    current.authority.controller.can_drag = true;
    assert_eq!(
        displayed(&current),
        vec![
            Action::Back,
            Action::Controller(CardAction::Close),
            Action::Controller(CardAction::ScreenRecordingSettings),
            Action::Diagnostics
        ]
    );
}

#[test]
fn traversal_skips_disabled_displayed_controls_and_wraps_both_directions() {
    let mut current = snapshot(Stage::Overview);
    current.authority.preparation.can_recheck = false;
    let first = advance(None, &current, false);
    assert_eq!(first.action, Action::Controller(CardAction::Close));
    let second = advance(Some(&first), &current, false);
    assert_eq!(
        second.action,
        Action::Controller(CardAction::AccessibilitySettings)
    );
    let third = advance(Some(&second), &current, false);
    assert_eq!(
        third.action,
        Action::Controller(CardAction::ScreenRecordingSettings)
    );
    let fourth = advance(Some(&third), &current, false);
    assert_eq!(fourth.action, Action::Controller(CardAction::ReturnToTask));
    assert_eq!(advance(Some(&fourth), &current, false).action, first.action);
    assert_eq!(advance(Some(&first), &current, true).action, fourth.action);
    current.authority.controller.open = false;
    assert!(keyboard::Selection::advance(Some(&first), &current, false).is_none());
}

#[test]
fn recovery_order_and_initial_selection_do_not_reuse_failed_surface_selection() {
    let mut current = snapshot(Stage::Guide);
    let guide = keyboard::Selection::reconcile(None, &current);
    assert_eq!(
        guide.as_ref().map(|selection| selection.action),
        Some(Action::Back)
    );
    current.recovery = true;
    current.authority.preparation.can_recheck = false;
    assert_eq!(
        displayed(&current),
        vec![
            Action::Controller(CardAction::Close),
            Action::Controller(CardAction::Recheck),
            Action::Diagnostics,
            Action::Reload,
            Action::Back,
            Action::Controller(CardAction::ReturnToTask)
        ]
    );
    let recovery = keyboard::Selection::reconcile(guide.as_ref(), &current);
    assert_eq!(
        recovery.as_ref().map(|selection| selection.action),
        Some(Action::Controller(CardAction::Close))
    );
    assert_eq!(
        advance(recovery.as_ref(), &current, false).action,
        Action::Diagnostics
    );
    current.authority.has_task = false;
    assert_eq!(
        advance(recovery.as_ref(), &current, true).action,
        Action::Back
    );
}

#[test]
fn stale_focus_cannot_activate_new_stage_surface_recovery_or_completed_check() {
    let shown = snapshot(Stage::Overview);
    let selected = keyboard::Selection {
        observed: shown.clone(),
        action: Action::Controller(CardAction::Recheck),
    };
    assert!(selected.activatable(&shown));
    let mut current = shown.clone();
    current.authority.preparation.can_recheck = false;
    assert!(!selected.activatable(&current));
    current.authority.preparation.can_recheck = true;
    current.authority.preparation_revision += 1;
    assert!(!selected.activatable(&current));
    current = shown.clone();
    current.id = ProjectionId::new();
    assert!(!selected.activatable(&current));
    current = shown.clone();
    current.authority.stage_identity = CallbackIdentity::default();
    assert!(!selected.activatable(&current));
    current = shown.clone();
    current.recovery = true;
    assert!(!selected.activatable(&current));
    current = shown.clone();
    current.authority.stage = Stage::Guide;
    current.authority.stage_identity = CallbackIdentity::default();
    assert!(!selected.activatable(&current));
    assert_eq!(
        keyboard::Selection::reconcile(Some(&selected), &current).map(|selection| selection.action),
        Some(Action::Back)
    );
}

#[test]
fn removed_mouse_selection_cannot_activate_hidden_diagnostics_or_return() {
    let mut shown = snapshot(Stage::Overview);
    shown.authority.warning = Some("failure".into());
    let diagnostics = keyboard::Selection {
        observed: shown.clone(),
        action: Action::Diagnostics,
    };
    let task = keyboard::Selection {
        observed: shown.clone(),
        action: Action::Controller(CardAction::ReturnToTask),
    };
    assert!(diagnostics.activatable(&shown));
    assert!(task.activatable(&shown));
    shown.authority.warning = None;
    shown.authority.has_task = false;
    assert!(!diagnostics.activatable(&shown));
    assert!(!task.activatable(&shown));
    assert_eq!(
        keyboard::Selection::reconcile(Some(&diagnostics), &shown)
            .map(|selection| selection.action),
        Some(Action::Controller(CardAction::Close))
    );
}
