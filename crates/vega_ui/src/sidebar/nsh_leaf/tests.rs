use super::{Item, Snapshot};
use crate::icons::Icon;
use crate::nsh::ProjectionId;
use crate::nsh::keyboard::{Selection, Step};

fn snapshot() -> Snapshot {
    let mut unavailable = Item::action("Fork unavailable", None);
    unavailable.enabled = false;
    Snapshot {
        id: ProjectionId::new(),
        text: None,
        items: vec![
            Item::action("Open", Some(Icon::Folder)),
            unavailable,
            Item::action("More", Some(Icon::More)),
        ],
        colors: vega_theme::LIGHT,
        selected: None,
        recovery: false,
        horizontal: true,
    }
}

#[test]
fn recycled_or_unmounted_rows_cannot_activate_retained_commands() {
    let displayed = snapshot();
    assert!(displayed.accepts(&displayed.id, 0, true, true, true, false));
    assert!(!displayed.accepts(&displayed.id, 0, false, true, true, false));
    let mut remounted = displayed.clone();
    remounted.id = ProjectionId::new();
    assert!(!remounted.accepts(&displayed.id, 0, true, true, true, false));
    assert!(remounted.accepts(&remounted.id, 0, true, true, true, false));
}

#[test]
fn route_draft_controller_and_drag_fences_are_independent() {
    let displayed = snapshot();
    assert!(!displayed.accepts(&displayed.id, 0, true, false, true, false));
    assert!(!displayed.accepts(&displayed.id, 0, true, true, false, false));
    assert!(!displayed.accepts(&displayed.id, 0, true, true, true, true));
    assert!(!displayed.accepts(&displayed.id, 1, true, true, true, false));
    assert!(!displayed.accepts(&displayed.id, usize::MAX, true, true, true, false));
}

#[test]
fn quiet_controls_remain_reachable_but_disabled_actions_are_skipped() {
    let mut displayed = snapshot();
    displayed.items[2].quiet = true;
    assert_eq!(displayed.actions(), vec![0, 2]);
    let Step::Selected(first) =
        Selection::advance(None, &displayed.id, &displayed.actions(), false)
    else {
        panic!("first enabled row action must be reachable");
    };
    let Step::Selected(last) =
        Selection::advance(Some(&first), &displayed.id, &displayed.actions(), false)
    else {
        panic!("quiet More action must be reachable");
    };
    assert_eq!(last.action(&displayed.id, &displayed.actions()), Some(2));
    assert!(matches!(
        Selection::advance(Some(&last), &displayed.id, &displayed.actions(), false),
        Step::Exit
    ));
    displayed.items[2].enabled = false;
    assert_eq!(last.action(&displayed.id, &displayed.actions()), None);
}

#[test]
fn recovery_exposes_only_reload_and_rejects_prior_business_commands() {
    let displayed = snapshot();
    let mut failed = displayed.clone();
    failed.id = ProjectionId::new();
    failed.recovery = true;
    assert_eq!(failed.actions(), vec![0]);
    assert!(!failed.accepts(&displayed.id, 0, true, true, true, false));
    assert!(!failed.accepts(&failed.id, 2, true, true, true, false));
    assert!(failed.accepts(&failed.id, 0, true, true, true, false));
    let mut rebuilt = failed.clone();
    rebuilt.id = ProjectionId::new();
    rebuilt.recovery = false;
    assert!(!rebuilt.accepts(&failed.id, 0, true, true, true, false));
}
