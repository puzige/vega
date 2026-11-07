use super::nsh_view::{Action, FileAction, Mode, Policy, Snapshot, horizontal_offset};
use super::{OPEN_TARGETS, preview_rows};
use crate::nsh::ProjectionId;
use crate::nsh::keyboard::{Selection, Step};

fn snapshot() -> Snapshot {
    Snapshot {
        id: ProjectionId::new(),
        mount: ProjectionId::new(),
        revision: ProjectionId::new(),
        mode: Mode::Group,
        message: "message".into(),
        route: ("thread".into(), "project".into()),
        files: Vec::new(),
        visible: 0,
        expanded: false,
        review_reason: None,
        errors: Vec::new(),
        colors: vega_theme::LIGHT,
        environment: ("en", vega_theme::Appearance::Light),
        selected: None,
        selection: ProjectionId::new(),
        focused: false,
        recovery: false,
        menu_surface: None,
    }
}

#[test]
fn preview_remains_available_in_read_only_mode_but_external_actions_do_not() {
    let policy = Policy {
        available: true,
        preview: true,
        reading: true,
        opening: false,
        menu: true,
    };
    assert!(policy.allows(FileAction::Preview, Mode::Compact));
    assert!(!policy.allows(FileAction::Menu, Mode::Compact));
    assert!(!policy.allows(FileAction::Clear, Mode::Row(1)));
    for (target, _) in OPEN_TARGETS {
        assert!(!policy.allows(FileAction::Open(target), Mode::Compact));
        assert!(!policy.allows(FileAction::Open(target), Mode::Row(1)));
    }
}

#[test]
fn opening_and_missing_file_reject_preview_and_all_external_targets() {
    for policy in [
        Policy {
            available: false,
            preview: true,
            reading: false,
            opening: false,
            menu: true,
        },
        Policy {
            available: true,
            preview: true,
            reading: false,
            opening: true,
            menu: true,
        },
    ] {
        assert!(!policy.allows(FileAction::Preview, Mode::Compact));
        assert!(!policy.allows(FileAction::Menu, Mode::Group));
        for (target, _) in OPEN_TARGETS {
            assert!(!policy.allows(FileAction::Open(target), Mode::Compact));
            assert!(!policy.allows(FileAction::Open(target), Mode::Row(1)));
        }
        assert!(policy.allows(FileAction::Clear, Mode::Row(1)));
        assert!(!policy.allows(FileAction::Clear, Mode::Compact));
    }
}

#[test]
fn compact_external_targets_require_the_displayed_menu_but_legacy_targets_do_not() {
    let mut policy = Policy {
        available: true,
        preview: false,
        reading: false,
        opening: false,
        menu: false,
    };
    assert!(!policy.allows(FileAction::Preview, Mode::Compact));
    assert!(policy.allows(FileAction::Menu, Mode::Compact));
    assert!(!policy.allows(FileAction::Menu, Mode::Row(1)));
    for (target, _) in OPEN_TARGETS {
        assert!(!policy.allows(FileAction::Open(target), Mode::Compact));
        assert!(policy.allows(FileAction::Open(target), Mode::Row(1)));
    }
    policy.menu = true;
    for (target, _) in OPEN_TARGETS {
        assert!(policy.allows(FileAction::Open(target), Mode::Compact));
    }
}

#[test]
fn recovery_exposes_only_reload_and_rejects_replaced_authority() {
    let mut observed = snapshot();
    observed.recovery = true;
    assert_eq!(observed.enabled(), vec![Action::Reload]);
    assert!(observed.accepts(&observed, Action::Reload));
    assert!(!observed.accepts(&observed, Action::Review));
    assert!(!observed.accepts(&observed, Action::CloseMenu));
    assert!(!observed.accepts(&observed, Action::File(0, FileAction::Clear)));
    let mut replacements = Vec::new();
    let mut current = observed.clone();
    current.id = ProjectionId::new();
    replacements.push(current);
    let mut current = observed.clone();
    current.mount = ProjectionId::new();
    replacements.push(current);
    let mut current = observed.clone();
    current.revision = ProjectionId::new();
    replacements.push(current);
    let mut current = observed.clone();
    current.route.0 = "other-thread".into();
    replacements.push(current);
    let mut current = observed.clone();
    current.route.1 = "other-project".into();
    replacements.push(current);
    let mut current = observed.clone();
    current.message = "other-message".into();
    replacements.push(current);
    let mut current = observed.clone();
    current.mode = Mode::Compact;
    replacements.push(current);
    let mut current = observed.clone();
    current.visible = 1;
    replacements.push(current);
    let mut current = observed.clone();
    current.expanded = true;
    replacements.push(current);
    let mut current = observed.clone();
    current.review_reason = Some("ui.file_unavailable");
    replacements.push(current);
    let mut current = observed.clone();
    current.environment = ("zh", vega_theme::Appearance::Light);
    replacements.push(current);
    let mut current = observed.clone();
    current.environment.1 = vega_theme::Appearance::Dark;
    replacements.push(current);
    let mut current = observed.clone();
    current.recovery = false;
    replacements.push(current);
    for current in replacements {
        assert!(!current.accepts(&observed, Action::Reload));
    }
}

#[test]
fn accessibility_activation_requires_the_current_focused_selection() {
    let mut observed = snapshot();
    observed.recovery = true;
    observed.focused = true;
    observed.selected = Some(Action::Reload);
    assert!(observed.selection_matches(&observed, Action::Reload));
    let mut current = observed.clone();
    current.selection = ProjectionId::new();
    assert!(!current.selection_matches(&observed, Action::Reload));
    let mut current = observed.clone();
    current.focused = false;
    assert!(!current.selection_matches(&observed, Action::Reload));
    let mut current = observed.clone();
    current.selected = None;
    assert!(!current.selection_matches(&observed, Action::Reload));
}

#[test]
fn empty_group_is_zero_height_and_capture_errors_keep_a_visible_row() {
    let mut current = snapshot();
    assert_eq!(current.height(), 0.);
    assert!(!current.enabled().contains(&Action::Review));
    current
        .errors
        .push(super::GitWorkspaceErrorCode::ArtifactConflict);
    assert_eq!(
        current.height(),
        vega_theme::Layout::ARTIFACT_BODY_GAP
            + vega_theme::Layout::ARTIFACT_HEADER_HEIGHT
            + vega_theme::Layout::ARTIFACT_ROW_HEIGHT
            + 2. * vega_theme::Layout::ARTIFACT_BORDER
    );
    assert!(current.enabled().is_empty());
}

#[test]
fn horizontal_reveal_uses_absolute_content_offsets_in_both_directions() {
    assert_eq!(horizontal_offset(0., 240., 48., 12., 160.), Some(116.));
    assert_eq!(horizontal_offset(116., -12., 48., 12., 160.), Some(92.));
    assert_eq!(horizontal_offset(92., 28., 48., 12., 160.), Some(92.));
    assert_eq!(horizontal_offset(10., -20., 48., 12., 160.), Some(0.));
    assert_eq!(horizontal_offset(0., f32::NAN, 48., 12., 160.), None);
    assert_eq!(horizontal_offset(0., 12., 0., 12., 160.), None);
    assert_eq!(horizontal_offset(0., 12., 48., 12., 0.), None);
}

#[test]
fn traversal_preserves_displayed_order_and_exits_without_wrapping() {
    let session = ProjectionId::new();
    let actions = [Action::Review, Action::Toggle];
    let Step::Selected(last) = Selection::advance(None, &session, &actions, true) else {
        panic!("reverse entry must select last enabled action");
    };
    assert_eq!(last.action(&session, &actions), Some(Action::Toggle));
    assert!(matches!(
        Selection::advance(Some(&last), &session, &actions, false),
        Step::Exit
    ));
    let Step::Selected(first) = Selection::advance(Some(&last), &session, &actions, true) else {
        panic!("reverse traversal must select first action");
    };
    assert_eq!(first.action(&session, &actions), Some(Action::Review));
    assert!(matches!(
        Selection::advance(Some(&first), &session, &actions, true),
        Step::Exit
    ));
    assert!(Selection::reconcile(Some(&last), &session, &[]).is_none());
    assert_eq!(last.action(&ProjectionId::new(), &actions), None);
}

#[test]
fn bounded_preview_rows_preserve_empty_lines_and_final_line_identity() {
    assert!(preview_rows("").is_empty());
    assert_eq!(preview_rows("a\n\nb"), vec!["a", "", "b"]);
    assert_eq!(preview_rows("a\n"), vec!["a"]);
    assert_eq!(preview_rows("\n"), vec![""]);
}

#[test]
fn six_menu_rows_fit_their_own_viewport_without_expanding_compact_or_group_rows() {
    use super::host::{Geometry, measured_activation};
    use super::nsh_view::{menu_anchor_top, menu_height};
    use gpui_kit::{Bounds, point, px, size};
    use vega_theme::Layout;

    let mut normal = snapshot();
    normal.mode = Mode::Compact;
    assert_eq!(normal.height(), 36.);
    normal.mode = Mode::Group;
    normal.visible = 1;
    assert_eq!(normal.height(), 98.);
    assert_eq!(menu_anchor_top(Mode::Compact, 0), 0.);
    assert_eq!(menu_anchor_top(Mode::Group, 0), 61.);
    assert_eq!(menu_anchor_top(Mode::Group, 1), 105.);
    assert_eq!(OPEN_TARGETS.len(), 6);
    assert_eq!(menu_height(), 226.);

    let mut observed = snapshot();
    observed.recovery = true;
    observed.focused = true;
    observed.selected = Some(Action::Reload);
    let mut geometry = Geometry::default();
    let bounds = Bounds {
        origin: point(px(10.), px(20.)),
        size: size(px(Layout::TASK_MENU_WIDTH), px(menu_height())),
    };
    geometry.bounds = Some(bounds);
    geometry.clip = Some(bounds);
    for row in 0..6 {
        let rect = [1., 5. + row as f32 * 36., 238., 36.];
        let measured = (observed.clone(), Action::Reload, rect, 0);
        assert!(measured_activation(
            &observed,
            &observed,
            Action::Reload,
            Some(&measured),
            &geometry,
            true,
        ));
    }
    let last = (observed.clone(), Action::Reload, [1., 185., 238., 36.], 0);
    geometry.bounds = Some(Bounds {
        size: size(px(240.), px(36.)),
        ..bounds
    });
    assert!(!measured_activation(
        &observed,
        &observed,
        Action::Reload,
        Some(&last),
        &geometry,
        true,
    ));
}

#[test]
fn accessible_click_rejects_unmeasured_stale_clipped_or_unselected_controls_but_pointer_still_works()
 {
    use super::host::{Geometry, measured_activation};
    use gpui_kit::{Bounds, point, px, size};

    let mut observed = snapshot();
    observed.recovery = true;
    observed.focused = true;
    observed.selected = Some(Action::Reload);
    let mut geometry = Geometry::default();
    let bounds = Bounds {
        origin: point(px(0.), px(0.)),
        size: size(px(240.), px(36.)),
    };
    geometry.bounds = Some(bounds);
    geometry.clip = Some(bounds);
    geometry.revision = 7;
    let displayed = (observed.clone(), Action::Reload, [8., 0., 80., 36.], 7);
    assert!(measured_activation(
        &observed,
        &observed,
        Action::Reload,
        Some(&displayed),
        &geometry,
        true
    ));
    assert!(!measured_activation(
        &observed,
        &observed,
        Action::Reload,
        None,
        &geometry,
        true
    ));
    assert!(!measured_activation(
        &observed,
        &observed,
        Action::Reload,
        Some(&displayed),
        &geometry,
        false
    ));
    for current in [
        Snapshot {
            selected: None,
            ..observed.clone()
        },
        Snapshot {
            focused: false,
            ..observed.clone()
        },
        Snapshot {
            selection: ProjectionId::new(),
            ..observed.clone()
        },
        Snapshot {
            mount: ProjectionId::new(),
            ..observed.clone()
        },
    ] {
        assert!(!measured_activation(
            &current,
            &observed,
            Action::Reload,
            Some(&displayed),
            &geometry,
            true
        ));
    }
    geometry.revision = 8;
    assert!(!measured_activation(
        &observed,
        &observed,
        Action::Reload,
        Some(&displayed),
        &geometry,
        true
    ));
    geometry.revision = 7;
    geometry.clip = Some(Bounds {
        size: size(px(60.), px(36.)),
        ..bounds
    });
    assert!(!measured_activation(
        &observed,
        &observed,
        Action::Reload,
        Some(&displayed),
        &geometry,
        true
    ));
    geometry.clip = None;
    assert!(!measured_activation(
        &observed,
        &observed,
        Action::Reload,
        Some(&displayed),
        &geometry,
        true
    ));
    let pointer = Snapshot {
        selected: None,
        focused: false,
        ..observed.clone()
    };
    assert!(pointer.accepts(&observed, Action::Reload));
    assert!(!measured_activation(
        &pointer,
        &observed,
        Action::Reload,
        Some(&displayed),
        &geometry,
        true
    ));
}

#[test]
fn trigger_edges_and_clipping_follow_the_current_compact_or_group_row() {
    use super::host::Geometry;
    use gpui_kit::{Bounds, point, px, size};

    for (mode, index, left, top, right) in [
        (Mode::Compact, 0, 384., 206.5, 408.),
        (Mode::Group, 0, 383., 267.5, 407.),
        (Mode::Group, 2, 383., 355.5, 407.),
    ] {
        let bounds = Bounds {
            origin: point(px(100.), px(200.)),
            size: size(px(320.), px(if mode == Mode::Compact { 36. } else { 220. })),
        };
        let mut geometry = Geometry::default();
        assert!(geometry.menu_trigger(mode, index).is_none());
        geometry.bounds = Some(bounds);
        geometry.clip = Some(bounds);
        let trigger = geometry.menu_trigger(mode, index).unwrap();
        assert!(trigger.contains(&point(px(left + 1.), px(top + 12.))));
        assert!(!trigger.contains(&point(px(right + 1.), px(top + 12.))));
        assert!(!trigger.contains(&point(px(left + 12.), px(top - 1.))));
        assert!(!trigger.contains(&point(px(left + 12.), px(top + 25.))));

        geometry.clip = Some(Bounds {
            origin: point(px(100.), px(top)),
            size: size(px(right - 100.), px(24.)),
        });
        assert!(geometry.menu_trigger(mode, index).is_some());
        geometry.clip.as_mut().unwrap().size.width -= px(1.);
        assert!(geometry.menu_trigger(mode, index).is_none());
        geometry.clip = Some(Bounds {
            origin: point(px(left + 1.), px(200.)),
            size: size(px(320.), px(220.)),
        });
        assert!(geometry.menu_trigger(mode, index).is_none());

        geometry.bounds.as_mut().unwrap().size.width = px(400.);
        geometry.clip = geometry.bounds;
        let resized = geometry.menu_trigger(mode, index).unwrap();
        assert!(!resized.contains(&point(px(left + 1.), px(top + 12.))));
        assert!(resized.contains(&point(px(left + 81.), px(top + 12.))));
        geometry.clip = None;
        assert!(geometry.menu_trigger(mode, index).is_none());
    }
}

#[test]
fn replaced_or_closed_menu_identity_invalidates_queued_activation_evidence() {
    use super::host::{Geometry, measured_activation};
    use gpui_kit::{Bounds, point, px, size};

    let mut observed = snapshot();
    observed.recovery = true;
    observed.focused = true;
    observed.selected = Some(Action::Reload);
    observed.menu_surface = Some(ProjectionId::new());
    let displayed = (observed.clone(), Action::Reload, [0., 0., 80., 36.], 0);
    let mut geometry = Geometry::default();
    let bounds = Bounds {
        origin: point(px(0.), px(0.)),
        size: size(px(240.), px(226.)),
    };
    geometry.bounds = Some(bounds);
    geometry.clip = Some(bounds);
    assert!(measured_activation(
        &observed,
        &observed,
        Action::Reload,
        Some(&displayed),
        &geometry,
        true
    ));
    for identity in [None, Some(ProjectionId::new())] {
        let current = Snapshot {
            menu_surface: identity,
            ..observed.clone()
        };
        assert!(!current.accepts(&observed, Action::Reload));
        assert!(!measured_activation(
            &current,
            &observed,
            Action::Reload,
            Some(&displayed),
            &geometry,
            true
        ));
    }
}
