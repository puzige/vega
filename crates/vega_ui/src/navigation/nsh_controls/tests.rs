use super::{Action, NavigationState, ProjectionId, Selection, Snapshot, Step, TaskMutationState};

fn snapshot() -> Snapshot {
    Snapshot {
        id: ProjectionId::new(),
        navigation: NavigationState {
            back: true,
            forward: true,
            error: None,
        },
        mutation: TaskMutationState::default(),
        task: Some("first-task".into()),
        project: Some("first-project".into()),
        sidebar: true,
        only_sidebar: false,
        recovery: false,
        settings_open: false,
        collapsed: false,
        selected: None,
        colors: vega_theme::Theme::light().colors,
    }
}

#[test]
fn navigation_rejects_changed_availability_mutation_and_mount() {
    let observed = snapshot();
    assert!(observed.accepts(&observed, Action::Back));
    let mut current = observed.clone();
    current.navigation.back = false;
    assert!(!current.accepts(&observed, Action::Back));
    current = observed.clone();
    current.mutation.epoch = 1;
    assert!(!current.accepts(&observed, Action::Back));
    current = observed.clone();
    current.task = Some("other-task".into());
    assert!(!current.accepts(&observed, Action::Back));
    current = observed.clone();
    current.project = Some("other-project".into());
    assert!(!current.accepts(&observed, Action::Back));
    current = observed.clone();
    current.id = ProjectionId::new();
    assert!(!current.accepts(&observed, Action::Search));
}

#[test]
fn hidden_and_relocated_controls_reject_queued_actions() {
    let observed = snapshot();
    let mut current = observed.clone();
    current.settings_open = true;
    assert!(!current.accepts(&observed, Action::Search));
    assert!(!current.accepts(&current, Action::Sidebar));
    current = observed.clone();
    current.collapsed = true;
    assert!(!current.accepts(&observed, Action::Sidebar));
    current = observed.clone();
    current.sidebar = false;
    assert!(!current.accepts(&observed, Action::Search));
}

#[test]
fn recovery_rebuild_cannot_replay_a_business_action() {
    let observed = snapshot();
    let mut recovery = observed.clone();
    recovery.id = ProjectionId::new();
    recovery.recovery = true;
    assert_eq!(recovery.actions().len(), 1);
    assert!(recovery.accepts(&recovery, Action::Reload));
    assert!(!recovery.accepts(&observed, Action::Back));
    assert!(!recovery.accepts(&recovery, Action::Search));
    let mut rebuilt = recovery.clone();
    rebuilt.id = ProjectionId::new();
    rebuilt.recovery = false;
    assert!(!rebuilt.accepts(&recovery, Action::Reload));
}

#[test]
fn sidebar_only_and_disabled_history_have_no_keyboard_targets() {
    let mut state = snapshot();
    state.navigation.back = false;
    state.navigation.forward = false;
    assert!(state.actions() == vec![Action::Sidebar, Action::Search]);
    let Step::Selected(first) = Selection::advance(None, &state.id, &state.actions(), false) else {
        panic!("missing first target")
    };
    assert!(first.action(&state.id, &state.actions()) == Some(Action::Sidebar));
    let Step::Selected(last) = Selection::advance(None, &state.id, &state.actions(), true) else {
        panic!("missing last target")
    };
    assert!(last.action(&state.id, &state.actions()) == Some(Action::Search));
    assert!(matches!(
        Selection::advance(Some(&last), &state.id, &state.actions(), false),
        Step::Exit
    ));
    state.only_sidebar = true;
    assert!(state.actions() == vec![Action::Sidebar]);
    assert!(!state.accepts(&state, Action::Search));
}
