use super::{
    Action, AgentIdentity, Authority, Control, Geometry, Kind, Mode, PanelAuthority, Snapshot, Tab,
    reveal_delta,
};
use crate::window::workspace::{TabKey, Workspace};
use gpui_kit::{Bounds, point, px, size};
use vega_theme::{Layout, Theme};
use vega_ui::icons::Icon;
use vega_ui::nsh::ProjectionId;

fn snapshot() -> Snapshot {
    Snapshot {
        id: ProjectionId::new(),
        mount: ProjectionId::new(),
        kind: Kind::Header(0),
        mode: Mode::Header,
        authority: Authority {
            route: Some("parent".into()),
            project: Some("project".into()),
            tabs: vec![
                (TabKey::Terminal(7), false),
                (TabKey::File(2), false),
                (TabKey::Terminal(9), true),
            ],
            selected: [Some(TabKey::Terminal(7)), Some(TabKey::Terminal(9))],
            hidden: [false; 2],
            maximized: [false; 2],
            menu: false,
            menu_bottom: false,
            environment: (true, false),
            panel: None,
        },
        tabs: vec![Tab {
            open: Control::new(
                Action::Select(TabKey::Terminal(7)),
                "Shell",
                Some(Icon::Terminal),
            ),
            close: Control::icon(
                Action::Close(TabKey::Terminal(7)),
                "Close Shell",
                Icon::Close,
            ),
        }],
        controls: vec![
            Control::icon(Action::Hide, "Hide", Icon::ChevronRight),
            Control::icon(Action::Menu, "New tab", Icon::Plus),
        ],
        lines: Vec::new(),
        colors: Theme::light().colors,
        environment: ("en", vega_theme::Appearance::Light),
        selected: None,
        focused: false,
        accessible: None,
        recovery: false,
    }
}

#[test]
fn projection_rejects_recycled_mount_route_and_tab_identity() {
    let observed = snapshot();
    let mut current = observed.clone();
    assert!(current.accepts(&observed, &Action::Close(TabKey::Terminal(7))));
    assert!(!current.accepts(&observed, &Action::Close(TabKey::Terminal(8))));
    current.mount = ProjectionId::new();
    assert!(!current.same(&observed));
    current = observed.clone();
    current.authority.route = Some("other-parent".into());
    assert!(!current.same(&observed));
    current = observed.clone();
    current.authority.tabs[0] = (TabKey::Terminal(8), false);
    assert!(!current.same(&observed));
    current = observed.clone();
    current.authority.hidden[0] = true;
    assert!(!current.same(&observed));
}

#[test]
fn subagent_authority_rejects_recycled_messages_and_operation() {
    let mut observed = snapshot();
    observed.authority.panel = Some(PanelAuthority {
        parent: Some("parent".into()),
        agents: vec![AgentIdentity {
            child: "child".into(),
            parent: "parent".into(),
            parent_message: "parent-message".into(),
            message: "message".into(),
            generation: Some(4),
            operation: Some("operation".into()),
        }],
        ..Default::default()
    });
    let mut changed = observed.clone();
    if let Some(panel) = changed.authority.panel.as_mut() {
        panel.agents[0].message = "replacement".into();
    }
    assert!(!changed.same(&observed));
    changed = observed.clone();
    if let Some(panel) = changed.authority.panel.as_mut() {
        panel.agents[0].operation = Some("replacement".into());
    }
    assert!(!changed.same(&observed));
    changed = observed.clone();
    if let Some(panel) = changed.authority.panel.as_mut() {
        panel.history_epoch += 1;
    }
    assert!(!changed.same(&observed));
}

#[test]
fn header_traversal_includes_exact_close_target_and_recovery_only_reloads() {
    let mut state = snapshot();
    assert_eq!(
        state.enabled(),
        vec![
            Action::Hide,
            Action::Select(TabKey::Terminal(7)),
            Action::Close(TabKey::Terminal(7)),
            Action::Menu
        ]
    );
    state.recovery = true;
    assert_eq!(state.enabled(), vec![Action::Reload]);
    assert!(!state.accepts(&snapshot(), &Action::Hide));
}

#[test]
fn tab_close_preserves_sibling_order_and_other_pane_selection() {
    let mut workspace = Workspace {
        tabs: vec![
            (TabKey::Terminal(7), false),
            (TabKey::File(2), false),
            (TabKey::Terminal(9), true),
        ],
        selected: [Some(TabKey::Terminal(7)), Some(TabKey::Terminal(9))],
        ..Default::default()
    };
    workspace.close(&TabKey::Terminal(7));
    assert_eq!(
        workspace.tabs,
        vec![(TabKey::File(2), false), (TabKey::Terminal(9), true)]
    );
    assert_eq!(
        workspace.selected,
        [Some(TabKey::File(2)), Some(TabKey::Terminal(9))]
    );
    workspace.open(TabKey::Terminal(9));
    assert_eq!(workspace.tabs.len(), 2);
    assert_eq!(workspace.selected[1], Some(TabKey::Terminal(9)));
}

#[test]
fn geometry_rejects_clipped_invalid_or_zero_size_activation() {
    let geometry = Geometry {
        bounds: Some(Bounds {
            origin: point(px(100.), px(50.)),
            size: size(px(200.), px(40.)),
        }),
        clip: Some(Bounds {
            origin: point(px(120.), px(50.)),
            size: size(px(180.), px(40.)),
        }),
        revision: 0,
    };
    assert!(geometry.visible([20., 0., 32., 32.]));
    assert!(!geometry.visible([0., 0., 32., 32.]));
    assert!(!geometry.visible([20., 0., 0., 32.]));
    assert!(!geometry.visible([f32::NAN, 0., 32., 32.]));
}

#[test]
fn detail_body_reserves_stream_at_minimum_default_and_actual_window_heights() {
    use super::detail_layout::detail_heights;
    let header = Layout::WORKSPACE_HEADER_HEIGHT;
    assert_eq!(detail_heights(150. - header, true), (55., 55.));
    assert_eq!(
        detail_heights(Layout::BOTTOM_WORKSPACE_HEIGHT - header, true),
        (116., 116.)
    );
    assert_eq!(detail_heights(600. - header, true), (220., 340.));
    assert_eq!(detail_heights(800. - header, true), (220., 540.));
    assert_eq!(detail_heights(110., false), (110., 0.));
    for body in [0., 40., 110., 232., 439., 440., 560., 760.] {
        let (chrome, stream) = detail_heights(body, true);
        assert_eq!(chrome + stream, body);
        assert!(stream >= body * 0.5);
        assert!(chrome <= 220.);
    }
    assert_eq!(detail_heights(f32::NAN, true), (0., 0.));
}

fn visible_geometry() -> Geometry {
    Geometry {
        bounds: Some(Bounds {
            origin: point(px(0.), px(0.)),
            size: size(px(200.), px(40.)),
        }),
        clip: Some(Bounds {
            origin: point(px(0.), px(0.)),
            size: size(px(200.), px(40.)),
        }),
        revision: 4,
    }
}

#[test]
fn ax_browse_nonselected_button_activates_after_explicit_selection_without_tab_focus() {
    let observed = snapshot();
    assert!(!observed.focused);
    assert!(observed.selected.is_none());
    let action = Action::Menu;
    let mut current = observed.clone();
    let request = current.accessible_request(&observed, &action);
    assert!(request.is_some());
    current.accessible = request;
    current.selected = Some(action.clone());
    assert!(!current.focused);
    let measured = current.clone();
    let geometry = visible_geometry();
    assert_eq!(
        current.measured_revision(
            &measured,
            &action,
            current.selected.as_ref(),
            &geometry,
            [80., 0., 32., 32.]
        ),
        Some(4)
    );
    assert_eq!(
        current.measured_revision(&measured, &action, None, &geometry, [80., 0., 32., 32.]),
        None
    );
    assert!(current.accessible_request(&observed, &action).is_none());
}

#[test]
fn ax_measurement_rejects_recycled_request_controller_mount_and_scrolled_geometry() {
    let mut observed = snapshot();
    observed.accessible = Some(ProjectionId::new());
    observed.selected = Some(Action::Menu);
    let rect = [80., 0., 32., 32.];
    let mut geometry = visible_geometry();
    let revision = observed.measured_revision(
        &observed,
        &Action::Menu,
        Some(&Action::Menu),
        &geometry,
        rect,
    );
    assert_eq!(revision, Some(4));
    geometry.revision += 1;
    assert_ne!(
        observed.measured_revision(
            &observed,
            &Action::Menu,
            Some(&Action::Menu),
            &geometry,
            rect
        ),
        revision
    );
    geometry.clip = None;
    assert_eq!(
        observed.measured_revision(
            &observed,
            &Action::Menu,
            Some(&Action::Menu),
            &geometry,
            rect
        ),
        None
    );
    geometry = visible_geometry();
    for changed in [
        Snapshot {
            accessible: Some(ProjectionId::new()),
            ..observed.clone()
        },
        Snapshot {
            mount: ProjectionId::new(),
            ..observed.clone()
        },
        Snapshot {
            accessible: None,
            ..observed.clone()
        },
    ] {
        assert_eq!(
            changed.measured_revision(
                &observed,
                &Action::Menu,
                Some(&Action::Menu),
                &geometry,
                rect
            ),
            None
        );
    }
    let mut changed = observed.clone();
    changed.authority.route = Some("replacement".into());
    assert_eq!(
        changed.measured_revision(
            &observed,
            &Action::Menu,
            Some(&Action::Menu),
            &geometry,
            rect
        ),
        None
    );
    assert_eq!(
        observed.measured_revision(
            &observed,
            &Action::Menu,
            Some(&Action::Menu),
            &geometry,
            [190., 0., 32., 32.]
        ),
        None
    );
    let mut hidden = observed.clone();
    hidden.authority.hidden[0] = true;
    assert!(
        hidden
            .accessible_request(&observed, &Action::Menu)
            .is_none()
    );
    assert!(
        observed
            .accessible_request(&observed, &Action::Stop("child".into(), 2))
            .is_none()
    );
}

#[test]
fn reveal_scrolls_only_enough_and_rejects_unrevealable_controls() {
    assert_eq!(reveal_delta(120., 24., 100., 100.), Some(0.));
    assert_eq!(reveal_delta(80., 24., 100., 100.), Some(-20.));
    assert_eq!(reveal_delta(190., 24., 100., 100.), Some(14.));
    assert_eq!(reveal_delta(100., 120., 100., 100.), None);
    assert_eq!(reveal_delta(f32::NAN, 24., 100., 100.), None);
}
