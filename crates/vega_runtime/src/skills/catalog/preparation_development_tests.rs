use super::*;
use std::path::PathBuf;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> (Self, SkillRun, SkillSelection) {
        let root = std::env::temp_dir().join(format!("vega-skill-{}", ulid::Ulid::generate()));
        std::fs::create_dir_all(root.join("synthetic")).unwrap();
        std::fs::write(
            root.join("synthetic/SKILL.md"),
            "---\nname: synthetic\ndescription: Inspect synthetic state\n---\nSynthetic body.\n",
        )
        .unwrap();
        let source = SkillSource::imported_approved(&root, 0).unwrap();
        let candidate = source.discover().unwrap().candidates.remove(0);
        let selection = SkillSelection::from_candidate(&candidate);
        let approval = SkillApproval::reviewed(&candidate, "synthetic", true, true).unwrap();
        let catalog = SkillCatalog::freeze(vec![candidate], &[approval], true).unwrap();
        (Self(root), SkillRun::new(catalog, true), selection)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn prepared_skill_still_uses_final_budget_and_cannot_activate_after_cancellation() {
    let (_fixture, mut run, selection) = Fixture::new();
    run.prepare_explicit_sources(std::slice::from_ref(&selection))
        .unwrap();
    let rejected = run.load_explicit(&selection, |_| false);
    assert_eq!(rejected.audit.status, "over_budget");
    assert!(run.active_summaries().is_empty());
    let loaded = run.load_explicit(&selection, |_| true);
    assert_eq!(loaded.audit.status, "loaded");
    assert_eq!(run.active_summaries().len(), 1);
    run.cancel();
    assert_eq!(
        run.load_explicit(&selection, |_| true).audit.status,
        "cancelled"
    );
}

#[test]
fn prepared_skill_rejects_in_place_edits_and_path_replacement() {
    for replace in [false, true] {
        let (fixture, mut run, selection) = Fixture::new();
        run.prepare_explicit_sources(std::slice::from_ref(&selection))
            .unwrap();
        if replace {
            std::fs::rename(fixture.0.join("synthetic"), fixture.0.join("previous")).unwrap();
            std::fs::create_dir(fixture.0.join("synthetic")).unwrap();
            std::fs::copy(
                fixture.0.join("previous/SKILL.md"),
                fixture.0.join("synthetic/SKILL.md"),
            )
            .unwrap();
        } else {
            std::fs::write(
                fixture.0.join("synthetic/SKILL.md"),
                "---\nname: synthetic\ndescription: Inspect synthetic state\n---\nChanged body.\n",
            )
            .unwrap();
        }
        assert!(run.validate_prepared_sources().is_err());
        assert_ne!(
            run.load_explicit(&selection, |_| true).audit.status,
            "loaded"
        );
        assert!(run.active_summaries().is_empty());
    }
}

#[test]
fn selected_skill_changed_or_removed_before_preparation_is_rejected() {
    for removed in [false, true] {
        let (fixture, mut run, selection) = Fixture::new();
        let path = fixture.0.join("synthetic/SKILL.md");
        if removed {
            std::fs::remove_file(path).unwrap();
        } else {
            std::fs::write(path, "changed source").unwrap();
        }
        assert!(
            run.prepare_explicit_sources(std::slice::from_ref(&selection))
                .is_err()
        );
        assert!(run.active_summaries().is_empty());
    }
}

#[test]
fn prepared_skill_rechecks_file_identity_after_successful_activation() {
    let (fixture, mut run, selection) = Fixture::new();
    run.prepare_explicit_sources(std::slice::from_ref(&selection))
        .unwrap();
    assert_eq!(
        run.load_explicit(&selection, |_| true).audit.status,
        "loaded"
    );
    std::fs::remove_file(fixture.0.join("synthetic/SKILL.md")).unwrap();
    assert!(run.validate_prepared_sources().is_err());
}
