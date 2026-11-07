use super::*;
use vega_conversation::types::AutomationKind;

fn snapshot() -> Snapshot {
    let key = AutomationEditorKey {
        thread_id: "thread-a".into(),
        kind: AutomationKind::Goal,
        identity: "goal-a".into(),
    };
    Snapshot {
        id: ProjectionId::new(),
        authority: Authority {
            key: key.clone(),
            revision: 4,
            base: "original".into(),
            latest: Some((key, 4)),
            pending: None,
            request: 8,
        },
        draft: Draft::new("changed".into()),
        error: None,
        details: vec![],
        colors: vega_theme::Theme::light().colors,
        focus: 0,
    }
}

#[test]
fn field_replacement_does_not_replay_consumed_workspace_focus() {
    let requests = Rc::new(FocusRequests::default());
    assert!(!requests.consume(1, false));
    assert!(requests.consume(1, true));
    let replacement = requests.clone();
    assert!(!replacement.consume(1, true));
    assert!(replacement.consume(2, true));
    assert!(!requests.consume(2, true));
}

#[test]
fn save_claim_uses_utf8_bytes_and_preserves_untrimmed_instruction() {
    let draft = Draft::new("界".repeat(5461));
    assert_eq!(draft.text().len(), 16383);
    assert!(draft.claim(&draft.read(), "base").is_some());
    draft.release();
    let epoch = draft.read().epoch;
    assert!(draft.edit(epoch, Some(&"界".repeat(5462)), None));
    assert!(draft.claim(&draft.read(), "base").is_none());
    assert!(draft.edit(epoch, Some(" \n\t "), None));
    assert!(draft.claim(&draft.read(), "base").is_none());
    assert!(draft.edit(epoch, Some("  keep\n\n界  "), None));
    assert_eq!(
        draft.claim(&draft.read(), "base").as_deref(),
        Some("  keep\n\n界  ")
    );
}

#[test]
fn composition_stale_observations_and_duplicate_claims_cannot_submit() {
    let draft = Draft::new("draft".into());
    let stale = draft.read();
    assert!(draft.edit(stale.epoch, Some("new draft"), Some(true)));
    assert!(draft.claim(&stale, "base").is_none());
    assert!(draft.claim(&draft.read(), "base").is_none());
    assert!(draft.edit(stale.epoch, None, Some(false)));
    let observed = draft.read();
    assert_eq!(draft.claim(&observed, "base").as_deref(), Some("new draft"));
    assert!(draft.claim(&observed, "base").is_none());
    assert!(!draft.edit(stale.epoch, Some("late edit"), None));
    assert_eq!(draft.text(), "new draft");
}

#[test]
fn failed_save_release_retains_exact_draft_and_allows_explicit_retry() {
    let draft = Draft::new("  failed draft\n界  ".into());
    let original = draft.text();
    assert_eq!(draft.claim(&draft.read(), "base"), Some(original.clone()));
    draft.release();
    assert_eq!(draft.text(), original);
    assert_eq!(draft.claim(&draft.read(), "base"), Some(original));
}

#[test]
fn reset_rejects_discarded_field_events_and_old_save_or_revert_decisions() {
    let draft = Draft::new("local".into());
    let old = draft.read();
    draft.reset("latest".into());
    assert!(!draft.edit(old.epoch, Some("late old native event"), Some(true)));
    assert!(draft.claim(&old, "base").is_none());
    assert!(!draft.claim_revert(&old));
    assert_eq!(draft.text(), "latest");
    assert!(draft.edit(draft.read().epoch, Some("new local"), None));
    assert_eq!(draft.text(), "new local");
}

#[test]
fn refresh_only_rebases_clean_noncomposing_unclaimed_draft() {
    let draft = Draft::new("base".into());
    assert!(draft.rebase_if_clean("base", "latest"));
    let epoch = draft.read().epoch;
    assert!(draft.edit(epoch, Some("typing before controller notification"), None));
    assert!(!draft.rebase_if_clean("latest", "remote"));
    assert_eq!(draft.text(), "typing before controller notification");
    draft.reset("latest".into());
    assert!(draft.edit(draft.read().epoch, None, Some(true)));
    assert!(!draft.rebase_if_clean("latest", "remote"));
    assert!(draft.edit(draft.read().epoch, None, Some(false)));
    assert!(draft.claim(&draft.read(), "base").is_some());
    assert!(!draft.rebase_if_clean("latest", "remote"));
}

#[test]
fn decisions_are_fenced_by_surface_key_base_latest_and_request() {
    let observed = snapshot();
    assert!(accepts(&observed, &observed));
    let mut current = observed.clone();
    current.id = ProjectionId::new();
    assert!(!accepts(&current, &observed));
    current = observed.clone();
    current.authority.key.thread_id = "thread-b".into();
    assert!(!accepts(&current, &observed));
    current = observed.clone();
    current.authority.key.kind = AutomationKind::Loop;
    assert!(!accepts(&current, &observed));
    current = observed.clone();
    current.authority.key.identity = "replacement".into();
    assert!(!accepts(&current, &observed));
    current = observed.clone();
    current.authority.revision += 1;
    assert!(!accepts(&current, &observed));
    current = observed.clone();
    current.authority.latest.as_mut().unwrap().1 += 1;
    assert!(!accepts(&current, &observed));
    current = observed.clone();
    current.authority.request += 1;
    assert!(!accepts(&current, &observed));
    current = observed.clone();
    current.authority.pending = Some(AutomationEditSubmission {
        key: current.authority.key.clone(),
        revision: current.authority.revision,
        instruction: current.draft.text(),
        request: current.authority.request,
    });
    assert!(!accepts(&current, &observed));
}

#[test]
fn cleared_conflicted_and_pending_editors_cannot_save_but_latest_can_be_loaded() {
    let mut page = snapshot();
    assert!(allowed(&page, &page.draft.read(), Action::Save));
    page.authority.latest.as_mut().unwrap().1 += 1;
    assert!(!allowed(&page, &page.draft.read(), Action::Save));
    assert!(allowed(&page, &page.draft.read(), Action::Revert));
    page.authority.latest.as_mut().unwrap().0.identity = "replacement".into();
    assert!(!allowed(&page, &page.draft.read(), Action::Save));
    assert!(allowed(&page, &page.draft.read(), Action::Revert));
    page.authority.latest = None;
    assert!(!allowed(&page, &page.draft.read(), Action::Save));
    assert!(!allowed(&page, &page.draft.read(), Action::Revert));
    let mut page = snapshot();
    page.authority.pending = Some(AutomationEditSubmission {
        key: page.authority.key.clone(),
        revision: page.authority.revision,
        instruction: page.draft.text(),
        request: page.authority.request,
    });
    assert!(!allowed(&page, &page.draft.read(), Action::Save));
    assert!(!allowed(&page, &page.draft.read(), Action::Revert));
}
