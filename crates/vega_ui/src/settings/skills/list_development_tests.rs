use super::*;
use vega_conversation::types::SkillCandidateView;

fn source(id: &str, root: &str) -> SkillSourceView {
    SkillSourceView {
        id: id.into(),
        scope: SkillUiScope::Imported,
        project_id: None,
        configured_root: root.into(),
        canonical_root: root.into(),
        source_label: id.into(),
        enabled: true,
        automatic: true,
        diagnostic: None,
        candidates: Vec::new(),
    }
}

fn candidate(winner: bool) -> SkillCandidateView {
    SkillCandidateView {
        name: "example".into(),
        description: Some("Example workflow".into()),
        content_sha256: Some("a".repeat(64)),
        target_binding: None,
        approved_sha256: None,
        size_bytes: Some(128),
        enabled: true,
        automatic: true,
        model_winner: winner,
        diagnostic: None,
    }
}

fn projection(sources: Vec<SkillSourceView>) -> SkillSettingsProjection {
    SkillSettingsProjection {
        consent_generation: 0,
        revocation_generation: 0,
        global_enabled: true,
        automatic_enabled: true,
        selected_project_id: None,
        project_enabled: false,
        project_automatic: false,
        sources,
    }
}

#[test]
fn imported_sources_with_the_same_basename_have_distinct_display_names() {
    let first = source("one", "/work/first/skills");
    let second = source("two", "/work/second/skills");
    let projection = projection(vec![first.clone(), second.clone()]);
    assert_eq!(skill_source_name(&first, &projection), "first/skills");
    assert_eq!(skill_source_name(&second, &projection), "second/skills");
    assert_ne!(
        skill_source_name(&first, &projection),
        skill_source_name(&second, &projection)
    );
}

#[test]
fn default_on_and_content_updated_winners_have_shadow_feedback_without_approval() {
    let mut winner = source("winner", "/work/first/skills");
    winner.candidates.push(candidate(true));
    let shadowed = candidate(false);
    let mut projection = projection(vec![winner]);
    assert!(skill_shadow_label(&projection, &shadowed).is_some());
    projection.sources[0].candidates[0].approved_sha256 = Some("b".repeat(64));
    assert!(skill_shadow_label(&projection, &shadowed).is_some());
    projection.sources[0].candidates[0].model_winner = false;
    assert!(skill_shadow_label(&projection, &shadowed).is_none());
}
