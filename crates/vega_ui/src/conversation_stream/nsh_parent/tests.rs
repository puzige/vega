use super::{Action, ProjectionId, Snapshot};

fn snapshot() -> Snapshot {
    Snapshot {
        id: ProjectionId::new(),
        source: "child".into(),
        parent: "parent".into(),
        recovery: false,
        focused: false,
        colors: vega_theme::Theme::dark().colors,
    }
}

#[test]
fn navigation_rejects_replaced_routes_sessions_hidden_and_unmounted_controls() {
    let current = snapshot();
    assert!(current.accepts(&current, Action::Open, true, true));
    assert!(!current.accepts(&current, Action::Open, false, true));
    assert!(!current.accepts(&current, Action::Open, true, false));
    let mut stale = current.clone();
    stale.source = "previous child".into();
    assert!(!current.accepts(&stale, Action::Open, true, true));
    stale = current.clone();
    stale.parent = "previous parent".into();
    assert!(!current.accepts(&stale, Action::Open, true, true));
    stale = current.clone();
    stale.id = ProjectionId::new();
    assert!(!current.accepts(&stale, Action::Open, true, true));
}

#[test]
fn recovery_only_accepts_current_explicit_reload_and_never_navigation() {
    let ordinary = snapshot();
    let mut recovery = ordinary.clone();
    recovery.id = ProjectionId::new();
    recovery.recovery = true;
    assert!(!ordinary.accepts(&ordinary, Action::Reload, true, true));
    assert!(!recovery.accepts(&ordinary, Action::Open, true, true));
    assert!(!recovery.accepts(&recovery, Action::Open, true, true));
    assert!(recovery.accepts(&recovery, Action::Reload, true, true));
    assert!(!recovery.accepts(&recovery, Action::Reload, false, true));
    assert!(!recovery.accepts(&recovery, Action::Reload, true, false));
}
