use super::*;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

fn write_skill(root: &Path, name: &str, description: &str, body: &str) {
    let directory = root.join(name);
    fs::create_dir_all(&directory).expect("skill directory");
    fs::write(
        directory.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description}\n---\n{body}"),
    )
    .expect("skill file");
}

fn project_candidate(root: &Path, name: &str, description: &str, body: &str) -> SkillCandidate {
    write_skill(&root.join(".agents/skills"), name, description, body);
    SkillSource::project_approved(root)
        .unwrap()
        .unwrap()
        .discover()
        .unwrap()
        .candidates
        .into_iter()
        .find(|candidate| candidate.name == name)
        .unwrap()
}

#[test]
fn s07_s08_catalog_requires_reviewed_hash_and_exposes_no_body_or_path() {
    let root = tempdir().unwrap();
    let candidate = project_candidate(
        root.path(),
        "reviewer",
        "Review code changes.",
        "PRIVATE BODY",
    );
    let approved = SkillApproval::reviewed(&candidate, "project-a", true, true).unwrap();
    let missing = SkillCatalog::freeze(vec![candidate.clone()], &[], true).unwrap();
    assert!(missing.model_catalog().is_empty());

    let off = SkillCatalog::freeze(
        vec![candidate.clone()],
        std::slice::from_ref(&approved),
        false,
    )
    .unwrap();
    assert!(off.model_catalog().is_empty());
    let selection = SkillSelection::from_candidate(&candidate);
    let mut explicit = SkillRun::new(off, true);
    assert_eq!(
        explicit.load_explicit(&selection, |_| true).receipt.status,
        "loaded"
    );

    let stored = serde_json::to_string(&approved).unwrap();
    let enabled = SkillCatalog::freeze(vec![candidate], &[approved], true).unwrap();
    let catalog = enabled.model_catalog();
    assert!(catalog.contains("reviewer"));
    assert!(catalog.contains("Review code changes."));
    assert!(catalog.contains("project-a"));
    assert!(!catalog.contains("PRIVATE BODY"));
    assert!(!catalog.contains(&root.path().display().to_string()));
    let restored: SkillApproval = serde_json::from_str(&stored).unwrap();
    assert!(!stored.contains("PRIVATE BODY"));
    assert_eq!(serde_json::to_string(&restored).unwrap(), stored);
    let mut disabled = restored;
    disabled.set_ui_preferences(false, false);
    assert!(!disabled.enabled());
    assert!(!disabled.automatic());
    assert_eq!(disabled.selection().name(), "reviewer");
}

#[test]
fn s07_persisted_review_keeps_old_sha_after_file_change() {
    let root = tempdir().unwrap();
    let old = project_candidate(root.path(), "reviewer", "Review code changes.", "OLD BODY");
    let approved = SkillApproval::reviewed(&old, "project-a", true, true).unwrap();
    let old_sha = approved.selection().sha256().to_owned();
    let source = SkillSource::project_approved(root.path()).unwrap().unwrap();
    write_skill(
        &root.path().join(".agents/skills"),
        "reviewer",
        "Review code changes.",
        "NEW BODY",
    );
    let current = source.discover().unwrap().candidates.remove(0);
    assert_ne!(old_sha, current.sha256);

    let identity = source.identity();
    let stored = PersistedSkillApproval {
        scope: identity.scope(),
        canonical_root: identity.canonical_root(),
        root_dev: identity.device(),
        root_ino: identity.inode(),
        name: "reviewer",
        approved_sha256: &old_sha,
        source_label: "project-a",
        enabled: true,
        automatic: true,
    };
    let restored = SkillApproval::from_persisted_parts(&source, stored).unwrap();
    assert_eq!(restored.selection().sha256(), old_sha);
    let catalog = SkillCatalog::freeze(vec![current], &[restored], true).unwrap();
    assert!(catalog.model_catalog().is_empty());
    assert_eq!(catalog.exclusions()[0].error, SkillError::Stale);

    assert!(matches!(
        SkillApproval::from_persisted_parts(
            &source,
            PersistedSkillApproval {
                name: "../bad",
                ..stored
            }
        ),
        Err(SkillError::InvalidFormat)
    ));
    assert!(matches!(
        SkillApproval::from_persisted_parts(
            &source,
            PersistedSkillApproval {
                approved_sha256: "bad-sha",
                ..stored
            }
        ),
        Err(SkillError::InvalidFormat)
    ));
    assert!(matches!(
        SkillApproval::from_persisted_parts(
            &source,
            PersistedSkillApproval {
                root_ino: stored.root_ino + 1,
                ..stored
            }
        ),
        Err(SkillError::RootChanged)
    ));
}

#[test]
fn s04_import_reordering_preserves_canonical_consent_identity() {
    let root = tempdir().unwrap();
    write_skill(root.path(), "imported", "Imported guide.", "BODY");
    let first = SkillSource::imported_approved(root.path(), 0)
        .unwrap()
        .discover()
        .unwrap()
        .candidates
        .remove(0);
    let approval = SkillApproval::reviewed(&first, "import-a", true, true).unwrap();
    let reordered = SkillSource::imported_approved(root.path(), 9)
        .unwrap()
        .discover()
        .unwrap()
        .candidates
        .remove(0);
    assert_eq!(first.source.identity(), reordered.source.identity());
    let catalog = SkillCatalog::freeze(vec![reordered], &[approval], true).unwrap();
    assert!(catalog.model_catalog().contains("Imported guide."));
}

#[test]
fn s08_catalog_limit_and_duplicate_consent_fail_closed() {
    let root = tempdir().unwrap();
    let root_skills = root.path().join(".agents/skills");
    for index in 0..128 {
        write_skill(
            &root_skills,
            &format!("skill-{index}"),
            &"x".repeat(1024),
            "BODY",
        );
    }
    let candidates = SkillSource::project_approved(root.path())
        .unwrap()
        .unwrap()
        .discover()
        .unwrap()
        .candidates;
    let approvals: Vec<_> = candidates
        .iter()
        .map(|candidate| SkillApproval::reviewed(candidate, "project-a", true, true).unwrap())
        .collect();
    assert!(matches!(
        SkillCatalog::freeze(candidates.clone(), &approvals, true),
        Err(SkillError::TooLarge)
    ));
    assert!(matches!(
        SkillCatalog::freeze(
            vec![candidates[0].clone()],
            &[approvals[0].clone(), approvals[0].clone()],
            true
        ),
        Err(SkillError::InvalidFormat)
    ));
    assert_eq!(
        SkillApproval::reviewed(&candidates[0], "bad\nlabel", true, true),
        Err(SkillError::InvalidName)
    );
}

#[test]
fn s04_s07_catalog_uses_only_approved_auto_winner_and_explicit_shadow() {
    let project = tempdir().unwrap();
    let config = tempdir().unwrap();
    let project_copy = project_candidate(project.path(), "same", "Project guide.", "PROJECT BODY");
    write_skill(
        &config.path().join("skills"),
        "same",
        "Global guide.",
        "GLOBAL BODY",
    );
    let global_copy = SkillSource::vega_global(config.path())
        .unwrap()
        .unwrap()
        .discover()
        .unwrap()
        .candidates
        .remove(0);
    let project_approval = SkillApproval::reviewed(&project_copy, "project-a", true, true).unwrap();
    let global_approval = SkillApproval::reviewed(&global_copy, "vega-global", true, true).unwrap();
    let catalog = SkillCatalog::freeze(
        vec![global_copy.clone(), project_copy],
        &[project_approval, global_approval],
        true,
    )
    .unwrap();
    assert!(catalog.model_catalog().contains("Project guide."));
    assert!(!catalog.model_catalog().contains("Global guide."));
    let selection = SkillSelection::from_candidate(&global_copy);
    let mut run = SkillRun::new(catalog, true);
    let outcome = run.load_explicit(&selection, |_| true);
    assert_eq!(outcome.receipt.status, "loaded");
    assert!(run.render_skill_envelope().unwrap().contains("GLOBAL BODY"));
    assert!(
        !run.render_skill_envelope()
            .unwrap()
            .contains("PROJECT BODY")
    );
}

#[test]
fn s04_stale_approved_winner_does_not_promote_shadow_without_selection() {
    let project = tempdir().unwrap();
    let config = tempdir().unwrap();
    let project_copy = project_candidate(project.path(), "same", "Project guide.", "PROJECT BODY");
    write_skill(
        &config.path().join("skills"),
        "same",
        "Global guide.",
        "GLOBAL BODY",
    );
    let global_copy = SkillSource::vega_global(config.path())
        .unwrap()
        .unwrap()
        .discover()
        .unwrap()
        .candidates
        .remove(0);
    let approvals = [
        SkillApproval::reviewed(&project_copy, "project-a", true, true).unwrap(),
        SkillApproval::reviewed(&global_copy, "vega-global", true, true).unwrap(),
    ];
    fs::write(
        project.path().join(".agents/skills/same/SKILL.md"),
        "---\nname: same\ndescription: Project guide.\n---\nTAMPERED",
    )
    .unwrap();
    let catalog = SkillCatalog::freeze(vec![global_copy, project_copy], &approvals, true).unwrap();
    assert!(catalog.model_catalog().is_empty());
    assert_eq!(catalog.exclusions().len(), 1);
    assert_eq!(catalog.exclusions()[0].error, SkillError::Stale);
}

#[test]
fn s09_s10_model_activation_freezes_body_and_receipt_is_content_free() {
    let root = tempdir().unwrap();
    let candidate = project_candidate(
        root.path(),
        "reviewer",
        "Review code changes.",
        "FIRST BODY",
    );
    let approval = SkillApproval::reviewed(&candidate, "project-a", true, true).unwrap();
    let catalog = SkillCatalog::freeze(vec![candidate], &[approval], true).unwrap();
    let mut run = SkillRun::new(catalog, true);
    let first = run.load_model("reviewer", |next| next.contains("FIRST BODY"));
    assert_eq!(first.receipt.status, "loaded");
    assert_eq!(first.audit.origin, ActivationOrigin::Model);
    assert_eq!(first.audit.content_sha256.as_ref().unwrap().len(), 64);
    assert!(!first.receipt.to_json().unwrap().contains("FIRST BODY"));
    assert!(
        !first
            .receipt
            .to_json()
            .unwrap()
            .contains(&root.path().display().to_string())
    );
    let initial = run.render_skill_envelope().unwrap();
    assert_eq!(initial.matches("FIRST BODY").count(), 1);

    fs::write(
        root.path().join(".agents/skills/reviewer/SKILL.md"),
        "---\nname: reviewer\ndescription: Review code changes.\n---\nCHANGED BODY",
    )
    .unwrap();
    assert_eq!(run.render_skill_envelope().unwrap(), initial);
    assert_eq!(
        run.load_model("reviewer", |_| panic!("duplicate budget check"))
            .receipt
            .status,
        "already_loaded"
    );
    assert_eq!(
        run.render_skill_envelope()
            .unwrap()
            .matches("FIRST BODY")
            .count(),
        1
    );
}

#[test]
fn s07_s10_changed_approval_and_preload_tamper_do_not_fallback() {
    let root = tempdir().unwrap();
    let candidate = project_candidate(
        root.path(),
        "reviewer",
        "Review code changes.",
        "FIRST BODY",
    );
    let approval = SkillApproval::reviewed(&candidate, "project-a", true, true).unwrap();
    let catalog = SkillCatalog::freeze(vec![candidate], &[approval], true).unwrap();
    let skill_file = root.path().join(".agents/skills/reviewer/SKILL.md");
    fs::write(
        &skill_file,
        "---\nname: reviewer\ndescription: Review code changes.\n---\nCHANGED BODY",
    )
    .unwrap();
    let mut run = SkillRun::new(catalog, true);
    let rejected = run.load_model("reviewer", |_| panic!("stale must not reach budget"));
    assert_eq!(rejected.receipt.status, "stale");
    assert!(run.render_skill_envelope().unwrap().is_empty());
    let refreshed = SkillSource::project_approved(root.path())
        .unwrap()
        .unwrap()
        .discover()
        .unwrap()
        .candidates
        .remove(0);
    let old_approval = SkillApproval::reviewed(&refreshed, "project-a", true, true).unwrap();
    let stale_hash = old_approval.with_approved_hash("0".repeat(64));
    let catalog = SkillCatalog::freeze(vec![refreshed], &[stale_hash], true).unwrap();
    assert!(catalog.model_catalog().is_empty());
    assert_eq!(catalog.exclusions()[0].error, SkillError::Stale);
}

#[test]
fn s10_s18_model_scope_caps_duplicates_and_rejects_over_budget() {
    let root = tempdir().unwrap();
    let candidates: Vec<_> = ["one", "two", "three", "four"]
        .into_iter()
        .map(|name| project_candidate(root.path(), name, "Useful guide.", "BODY"))
        .collect();
    let approvals: Vec<_> = candidates
        .iter()
        .map(|candidate| SkillApproval::reviewed(candidate, "project-a", true, true).unwrap())
        .collect();
    let catalog = SkillCatalog::freeze(candidates, &approvals, true).unwrap();
    let mut indirect = SkillRun::new(catalog.clone(), false);
    assert_eq!(
        indirect.load_model("one", |_| true).receipt.status,
        "not_direct_user"
    );
    let mut run = SkillRun::new(catalog, true);
    assert_eq!(
        run.load_model("unknown", |_| panic!("unknown read"))
            .receipt
            .status,
        "unavailable"
    );
    assert_eq!(
        run.load_model("one", |_| false).receipt.status,
        "over_budget"
    );
    assert!(run.render_skill_envelope().unwrap().is_empty());
    for name in ["one", "two", "three"] {
        assert_eq!(run.load_model(name, |_| true).receipt.status, "loaded");
    }
    assert_eq!(
        run.load_model("four", |_| panic!("over cap read"))
            .receipt
            .status,
        "activation_limit"
    );
    assert_eq!(
        run.render_skill_envelope().unwrap().matches("BODY").count(),
        3
    );
}

#[test]
fn s11_mixed_batch_is_rejected_before_operational_dispatch() {
    assert_eq!(
        classify_tool_batch(&["bash", "mcp.server.tool"]),
        BatchPolicy::Ordinary
    );
    assert_eq!(classify_tool_batch(&["load_skill"]), BatchPolicy::SkillOnly);
    assert_eq!(
        classify_tool_batch(&["load_skill", "load_skill"]),
        BatchPolicy::SkillOnly
    );
    assert_eq!(
        classify_tool_batch(&["load_skill", "bash"]),
        BatchPolicy::RejectOtherTools
    );
    assert_eq!(
        classify_tool_batch(&["mcp.server.tool", "load_skill"]),
        BatchPolicy::RejectOtherTools
    );
}

#[test]
fn s14_s15_reference_is_lower_trust_and_frozen_after_first_read() {
    let root = tempdir().unwrap();
    let candidate = project_candidate(
        root.path(),
        "reviewer",
        "Review code changes.",
        "Read references/note.md",
    );
    let approval = SkillApproval::reviewed(&candidate, "project-a", true, true).unwrap();
    let references = root.path().join(".agents/skills/reviewer/references");
    fs::create_dir_all(&references).unwrap();
    fs::write(references.join("note.md"), "FIRST REFERENCE").unwrap();
    let mut run = SkillRun::new(
        SkillCatalog::freeze(vec![candidate], &[approval], true).unwrap(),
        true,
    );
    assert_eq!(
        run.load_model("reviewer", |_| true).receipt.status,
        "loaded"
    );
    assert_eq!(
        run.read_reference("reviewer", "../outside.md", |_| true),
        Err(SkillError::UnsafePath)
    );
    assert_eq!(
        run.read_reference("reviewer", &"x".repeat(1025), |_| panic!(
            "oversize path read"
        )),
        Err(SkillError::UnsafePath)
    );
    let first = run
        .read_reference("reviewer", "references/note.md", |text| {
            text.contains("FIRST REFERENCE")
        })
        .unwrap();
    assert!(first.lower_trust);
    assert_eq!(first.content_sha256.len(), 64);
    fs::write(references.join("note.md"), "SECOND REFERENCE").unwrap();
    let again = run
        .read_reference("reviewer", "references/note.md", |result| {
            result.contains("FIRST REFERENCE")
        })
        .unwrap();
    assert_eq!(again, first);
    assert_eq!(
        run.read_reference("unloaded", "references/note.md", |_| true),
        Err(SkillError::NotActivated)
    );
}
