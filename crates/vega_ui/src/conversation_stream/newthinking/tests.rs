use super::{Authority, OwnerBinding, Snapshot, transition_visibility};
use crate::conversation_stream::model::MessageCopy;
use crate::nsh::ProjectionId;

fn snapshot() -> Snapshot {
    Snapshot {
        session: ProjectionId::new(),
        owner_session: None,
        authority: Authority {
            source: 4,
            revision: 9,
            start: 1024,
            expanded: false,
            available: true,
            actions: true,
            selection: 3,
            visible: true,
            title: "Reviewing the implementation".into(),
        },
        colors: vega_theme::LIGHT,
        focused: false,
        epoch: None,
        omission: "Earlier thinking omitted".into(),
        label: "Thinking disclosure".into(),
    }
}

#[test]
fn disclosure_rejects_source_window_selection_permission_and_expansion_changes() {
    let observed = snapshot();
    assert!(observed.accepts(&observed.session, &observed.authority, true));
    let changes: &[fn(&mut Authority)] = &[
        |value| value.source += 1,
        |value| value.revision += 1,
        |value| value.start += 1,
        |value| value.expanded = true,
        |value| value.available = false,
        |value| value.actions = false,
        |value| value.selection += 1,
        |value| value.visible = false,
        |value| value.title = "A newer title".into(),
    ];
    for change in changes {
        let mut current = observed.authority.clone();
        change(&mut current);
        assert!(!observed.accepts(&observed.session, &current, true));
    }
}

#[test]
fn hidden_or_recycled_disclosure_cannot_reactivate_with_the_same_source() {
    let observed = snapshot();
    assert!(!observed.accepts(&observed.session, &observed.authority, false));
    assert!(!observed.accepts(&ProjectionId::new(), &observed.authority, true));
    let mut hidden = observed.authority.clone();
    hidden.visible = false;
    assert!(!observed.accepts(&observed.session, &hidden, true));
    assert!(observed.accepts(&observed.session, &observed.authority, true));
}

#[test]
fn trusted_resume_restores_current_owner_without_replaying_old_requests_or_selection() {
    let source = MessageCopy::new("retained original source");
    let before_suspend = ProjectionId::new();
    let mut binding = OwnerBinding::new(before_suspend.clone());
    let old_session = binding.session.clone();
    let selection = source.selection_generation();
    source.set_selected_text(selection, "original".into());
    assert!(binding.accepts(&before_suspend, &old_session));
    assert!(source.has_selected_text());

    let after_suspend = ProjectionId::new();
    assert!(!binding.current(&after_suspend));
    assert!(!binding.accepts(&after_suspend, &old_session));
    assert!(binding.rebind(&after_suspend, &source));
    assert!(binding.current(&after_suspend));
    assert!(!binding.accepts(&after_suspend, &old_session));
    assert!(!binding.accepts(&before_suspend, &old_session));
    assert!(binding.accepts(&after_suspend, &binding.session));
    assert!(!source.has_selected_text());
    assert!(source.selection_generation() > selection);
    source.set_selected_text(selection, "stale selection".into());
    assert!(!source.has_selected_text());
    assert_eq!(source.original_text(), "retained original source");
    assert!(source.has_text());

    let resumed_session = binding.session.clone();
    let resumed_selection = source.selection_generation();
    source.set_selected_text(resumed_selection, "original".into());
    assert!(!binding.rebind(&after_suspend, &source));
    assert!(binding.accepts(&after_suspend, &resumed_session));
    assert_eq!(source.selection_generation(), resumed_selection);
    assert!(source.has_selected_text());
    assert_eq!(source.original_text(), "retained original source");

    let suspended_again = ProjectionId::new();
    assert!(!binding.accepts(&suspended_again, &resumed_session));
    assert!(binding.rebind(&suspended_again, &source));
    assert!(!binding.accepts(&suspended_again, &resumed_session));
    assert!(binding.accepts(&suspended_again, &binding.session));
    assert!(!source.has_selected_text());
    assert_eq!(source.original_text(), "retained original source");
}

#[test]
fn hiding_thinking_synchronously_revokes_selected_source() {
    let source = MessageCopy::new("retained source");
    let generation = source.selection_generation();
    source.set_selected_text(generation, "source".into());
    assert!(source.has_selected_text());
    let mut visible = true;
    assert!(transition_visibility(&mut visible, false, &source));
    assert!(!visible);
    assert!(!source.has_selected_text());
    assert!(source.selection_generation() > generation);
    let hidden_generation = source.selection_generation();
    assert!(!transition_visibility(&mut visible, false, &source));
    assert_eq!(source.selection_generation(), hidden_generation);
    assert!(transition_visibility(&mut visible, true, &source));
    assert!(!source.has_selected_text());
    assert_eq!(source.original_text(), "retained source");
    assert!(source.has_text());
}
