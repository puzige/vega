use super::*;
use crate::{ChatMessage, ChatRequest, ChatRole, ContextBudget, RuntimeToolConfig};
use std::fs;
use std::path::PathBuf;

struct SkillFixture {
    root: PathBuf,
    source: SkillSource,
}

impl SkillFixture {
    fn new(names: &[&str]) -> Self {
        let root = std::env::temp_dir().join(format!("vega-skill-{}", ulid::Ulid::generate()));
        fs::create_dir_all(&root).unwrap();
        let source = SkillSource::imported_approved(&root, 0).unwrap();
        let fixture = Self { root, source };
        for name in names {
            fixture.write(name, &format!("original-{name}"));
        }
        fixture
    }

    fn write(&self, name: &str, body: &str) {
        let directory = self.root.join(name);
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: Reviewed development fixture\n---\n{body}\n"),
        )
        .unwrap();
    }

    fn catalog(&self) -> (SkillCatalog, Vec<SkillSelection>) {
        let discovery = self.source.discover().unwrap();
        assert!(discovery.diagnostics.is_empty());
        let selections = discovery
            .candidates
            .iter()
            .map(SkillSelection::from_candidate)
            .collect();
        let approvals = discovery
            .candidates
            .iter()
            .map(|candidate| {
                SkillApproval::reviewed(candidate, "reviewed-source", true, true).unwrap()
            })
            .collect::<Vec<_>>();
        (
            SkillCatalog::freeze(discovery.candidates, &approvals, true).unwrap(),
            selections,
        )
    }

    fn run(&self) -> (SkillRun, Vec<SkillSelection>) {
        let (catalog, selections) = self.catalog();
        let binding = RunBinding::for_catalog("run", "thread", 1, 0, &catalog).unwrap();
        (
            SkillRun::new_bound(catalog, true, binding).unwrap(),
            selections,
        )
    }
}

impl Drop for SkillFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn selection<'a>(selections: &'a [SkillSelection], name: &str) -> &'a SkillSelection {
    selections.iter().find(|item| item.name() == name).unwrap()
}

fn request() -> ChatRequest {
    ChatRequest {
        history_safety: None,
        model_snapshot: None,
        tool_choice: None,
        model: "development-model".into(),
        messages: vec![
            ChatMessage::new(ChatRole::System, "Follow the user request"),
            ChatMessage::new(ChatRole::User, "Use the selected skills"),
        ],
        tools: Vec::new(),
        max_tokens: Some(1),
        reasoning: None,
    }
}

fn prepare(
    run: &mut SkillRun,
    selections: &[SkillSelection],
) -> Result<Vec<ActivationAudit>, crate::VegaError> {
    RuntimeToolConfig::default().prepare_explicit_skills(&request(), None, run, selections, false)
}

fn snapshot(run: &SkillRun) -> Vec<u8> {
    run.export_snapshot().unwrap().bytes().to_vec()
}

#[test]
fn failed_later_activation_discards_the_whole_staged_batch() {
    let fixture = SkillFixture::new(&["alpha", "beta", "gamma"]);
    let (mut run, selections) = fixture.run();
    prepare(
        &mut run,
        std::slice::from_ref(selection(&selections, "alpha")),
    )
    .unwrap();
    let before = snapshot(&run);
    fixture.write("gamma", "changed-after-review");
    let error = prepare(
        &mut run,
        &[
            selection(&selections, "beta").clone(),
            selection(&selections, "gamma").clone(),
        ],
    )
    .unwrap_err();
    assert!(error.to_string().contains("stale"), "{error}");
    assert_eq!(snapshot(&run), before);
    assert_eq!(
        run.active_selections(),
        vec![selection(&selections, "alpha").clone()]
    );
    assert_eq!(run.historical_bodies()[0].1.trim(), "original-alpha");
    fixture.write("gamma", "original-gamma");
    let audits = prepare(
        &mut run,
        &[
            selection(&selections, "beta").clone(),
            selection(&selections, "gamma").clone(),
        ],
    )
    .unwrap();
    assert_eq!(audits.len(), 2);
    assert!(
        audits
            .iter()
            .all(|audit| audit.origin == ActivationOrigin::ExplicitUser)
    );
    assert_eq!(run.active_selections().len(), 3);
}

#[test]
fn frozen_duplicate_keeps_exact_bytes_after_disk_change_and_allows_new_skill() {
    let fixture = SkillFixture::new(&["alpha", "beta"]);
    let (mut run, selections) = fixture.run();
    let alpha = selection(&selections, "alpha").clone();
    prepare(&mut run, std::slice::from_ref(&alpha)).unwrap();
    let before = snapshot(&run);
    let original = run.historical_bodies();
    fixture.write("alpha", "unreviewed-replacement");
    assert!(
        prepare(&mut run, &[alpha.clone(), alpha.clone()])
            .unwrap()
            .is_empty()
    );
    assert_eq!(snapshot(&run), before);
    let audits = prepare(&mut run, &[alpha, selection(&selections, "beta").clone()]).unwrap();
    assert_eq!(audits.len(), 1);
    assert_eq!(audits[0].name, "beta");
    assert_eq!(run.historical_bodies()[0], original[0]);
    assert_eq!(run.historical_bodies()[1].1.trim(), "original-beta");
}

#[test]
fn new_hash_is_not_substituted_for_the_reviewed_selection() {
    let fixture = SkillFixture::new(&["alpha"]);
    let (mut run, _) = fixture.run();
    let before = snapshot(&run);
    fixture.write("alpha", "new-reviewed-version");
    let (_, changed) = fixture.catalog();
    let error = prepare(&mut run, &changed).unwrap_err();
    assert!(error.to_string().contains("unavailable"), "{error}");
    assert_eq!(snapshot(&run), before);
}

#[test]
fn same_name_from_another_source_cannot_replace_frozen_activation() {
    let first = SkillFixture::new(&["alpha"]);
    let second = SkillFixture::new(&["alpha"]);
    let mut candidates = first.source.discover().unwrap().candidates;
    candidates.extend(second.source.discover().unwrap().candidates);
    let selections = candidates
        .iter()
        .map(SkillSelection::from_candidate)
        .collect::<Vec<_>>();
    let approvals = candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| {
            SkillApproval::reviewed(candidate, &format!("source-{index}"), true, true).unwrap()
        })
        .collect::<Vec<_>>();
    let catalog = SkillCatalog::freeze(candidates, &approvals, true).unwrap();
    let binding = RunBinding::for_catalog("run", "thread", 1, 0, &catalog).unwrap();
    let mut run = SkillRun::new_bound(catalog, true, binding).unwrap();
    prepare(&mut run, std::slice::from_ref(&selections[0])).unwrap();
    let before = snapshot(&run);
    let error = prepare(&mut run, std::slice::from_ref(&selections[1])).unwrap_err();
    assert!(error.to_string().contains("name_conflict"), "{error}");
    assert_eq!(snapshot(&run), before);
}

#[test]
fn unknown_source_with_same_name_and_bytes_is_rejected() {
    let first = SkillFixture::new(&["alpha"]);
    let second = SkillFixture::new(&["alpha"]);
    let (mut run, original) = first.run();
    let (_, foreign) = second.catalog();
    assert_eq!(original[0].sha256(), foreign[0].sha256());
    assert_ne!(original[0].source(), foreign[0].source());
    let before = snapshot(&run);
    let error = prepare(&mut run, &foreign).unwrap_err();
    assert!(error.to_string().contains("unavailable"), "{error}");
    assert_eq!(snapshot(&run), before);
}

#[test]
fn activation_limit_failure_rolls_back_earlier_additions_in_the_batch() {
    let fixture = SkillFixture::new(&["alpha", "beta", "gamma", "delta"]);
    let (mut run, selections) = fixture.run();
    prepare(
        &mut run,
        std::slice::from_ref(selection(&selections, "alpha")),
    )
    .unwrap();
    let before = snapshot(&run);
    let error = prepare(
        &mut run,
        &[
            selection(&selections, "beta").clone(),
            selection(&selections, "gamma").clone(),
            selection(&selections, "delta").clone(),
        ],
    )
    .unwrap_err();
    assert!(error.to_string().contains("activation_limit"), "{error}");
    assert_eq!(snapshot(&run), before);
}

#[test]
fn context_budget_rejection_preserves_the_original_snapshot() {
    let fixture = SkillFixture::new(&["alpha", "beta"]);
    let (mut run, selections) = fixture.run();
    prepare(
        &mut run,
        std::slice::from_ref(selection(&selections, "alpha")),
    )
    .unwrap();
    let before = snapshot(&run);
    let error = RuntimeToolConfig::default()
        .prepare_explicit_skills(
            &request(),
            Some(ContextBudget::new(2, 1, false).unwrap()),
            &mut run,
            std::slice::from_ref(selection(&selections, "beta")),
            false,
        )
        .unwrap_err();
    assert!(error.to_string().contains("over_budget"), "{error}");
    assert_eq!(snapshot(&run), before);
}
