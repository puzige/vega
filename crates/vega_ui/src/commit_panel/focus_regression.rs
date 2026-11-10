use super::{CommitPanelFocus, CommitPanelStage, draft::Draft, nsh_view::Snapshot};
use crate::nsh::ProjectionId;
use std::{collections::HashSet, sync::Arc};

fn snapshot(draft: Draft, page: ProjectionId) -> Snapshot {
    let value = draft.0.lock().expect("test draft lock");
    Snapshot {
        page,
        thread: "synthetic-thread".into(),
        project: "synthetic-project".into(),
        stage: CommitPanelStage::CommitReady,
        pending: None,
        checklist: None,
        generation: None,
        prepared: None,
        selected: HashSet::new(),
        disabled: false,
        focus: CommitPanelFocus::Draft,
        focus_revision: 0,
        rows: Arc::from([]),
        draft: draft.clone(),
        draft_revision: value.revision,
        draft_text: value.text.clone(),
        draft_overflow: value.overflow,
        failure: false,
        colors: vega_theme::LIGHT,
    }
}

#[test]
fn tab_traversal_preserves_focus_authority_across_only_text_revisions() {
    let shown = snapshot(Draft::default(), ProjectionId::new());
    let mut current = shown.clone();
    current.draft_revision += 1;
    current.draft_text.push_str("pasted");
    assert!(shown.same_focus_authority(&current));
    assert!(!shown.same_authority(&current));
    current.focus_revision += 1;
    assert!(!shown.same_focus_authority(&current));
    current.focus_revision = shown.focus_revision;
    current.draft_overflow = true;
    assert!(!shown.same_focus_authority(&current));
    current.draft_overflow = false;
    current.page = ProjectionId::new();
    assert!(!shown.same_focus_authority(&current));
}
