use super::*;
use std::fs;
use std::os::unix::fs::symlink;
use vega_runtime::skills::{
    PersistedSkillApproval, SkillApproval, SkillCatalog, SkillRun, SkillSelection,
};

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct TestError(String);

impl From<SkillError> for TestError {
    fn from(error: SkillError) -> Self {
        Self(format!("{error:?}"))
    }
}
impl From<std::io::Error> for TestError {
    fn from(error: std::io::Error) -> Self {
        Self(format!("{error}"))
    }
}
impl From<serde_json::Error> for TestError {
    fn from(error: serde_json::Error) -> Self {
        Self(format!("{error}"))
    }
}
impl From<std::num::ParseIntError> for TestError {
    fn from(error: std::num::ParseIntError) -> Self {
        Self(format!("{error}"))
    }
}
impl From<&str> for TestError {
    fn from(error: &str) -> Self {
        Self(format!("{error}"))
    }
}
impl From<SkillSettingsError> for TestError {
    fn from(error: SkillSettingsError) -> Self {
        Self(format!("{error}"))
    }
}
impl From<SkillStoreError> for TestError {
    fn from(error: SkillStoreError) -> Self {
        Self(format!("{error}"))
    }
}

type Result<T = ()> = std::result::Result<T, TestError>;

struct Fixture {
    base: PathBuf,
    root: PathBuf,
    target: PathBuf,
    service: SkillSettingsService,
    source_id: String,
}

impl Fixture {
    fn new() -> Result<Self> {
        let base = std::env::temp_dir().join(format!("linked-skills-{}", ulid::Ulid::generate()));
        let root = base.join("catalog");
        let target = base.join("target");
        fs::create_dir_all(&root)?;
        fs::create_dir_all(&target)?;
        fs::write(
            target.join("SKILL.md"),
            "---\nname: example\ndescription: Example workflow\n---\nReviewed body\n",
        )?;
        symlink(&target, root.join("example"))?;
        let service =
            SkillSettingsService::new(base.join("store.sqlite"), base.join("config"), None);
        let preview = service.preview_imported_root(&root)?;
        service.apply(
            service.projection()?.consent_generation,
            SkillSettingsMutation::LinkRoot {
                preview_token: preview.token,
            },
        )?;
        let source_id = service.projection()?.sources[0].id.clone();
        let fixture = Self {
            base,
            root,
            target,
            service,
            source_id,
        };
        fixture.apply(SkillSettingsMutation::SetGlobal {
            enabled: true,
            automatic: false,
        })?;
        fixture.apply(SkillSettingsMutation::SetSource {
            source_id: fixture.source_id.clone(),
            enabled: true,
            automatic: false,
        })?;
        Ok(fixture)
    }

    fn apply(&self, mutation: SkillSettingsMutation) -> Result {
        self.service
            .apply(self.service.projection()?.consent_generation, mutation)?;
        Ok(())
    }

    fn replace_link(&self, target: &Path) -> Result {
        let next = self.root.join("replacement");
        symlink(target, &next)?;
        fs::rename(next, self.root.join("example"))?;
        Ok(())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

#[test]
fn linked_skill_requires_explicit_review_before_composer_and_runtime_activation() -> Result {
    let fixture = Fixture::new()?;
    let projection = fixture.service.projection()?;
    let candidate = &projection.sources[0].candidates[0];
    assert_eq!(candidate.diagnostic.as_deref(), Some("review_required"));
    assert!(candidate.content_sha256.is_some());
    assert!(
        fixture
            .service
            .composer_draft_projection("draft")?
            .candidates
            .is_empty()
    );
    let preview = fixture
        .service
        .preview_skill(&fixture.source_id, "example")?;
    assert_eq!(
        preview.linked_paths,
        Some((
            fixture.root.canonicalize()?.join("example"),
            fixture.target.canonicalize()?
        ))
    );
    assert!(preview.body.contains("Reviewed body"));
    assert!(
        fixture
            .service
            .composer_draft_projection("draft")?
            .candidates
            .is_empty()
    );
    fixture.apply(SkillSettingsMutation::ApproveSkill {
        preview_token: preview.token,
    })?;
    let projection = fixture.service.composer_draft_projection("draft")?;
    assert_eq!(projection.candidates.len(), 1);
    let store = fixture.service.store()?;
    let approvals = skills::list_approved_skills(store.conn())?;
    assert_eq!(approvals.len(), 1);
    let stored = &approvals[0];
    assert!(stored.enabled);
    assert!(!stored.automatic);
    assert!(stored.target_binding.is_some());
    let row = fixture
        .service
        .find_scoped_source(&store, &fixture.source_id)?;
    let source = fixture.service.rebind(&store, &row)?;
    let approval = SkillApproval::from_persisted_parts(
        &source,
        PersistedSkillApproval {
            scope: source.identity().scope(),
            canonical_root: Path::new(&row.canonical_root),
            root_dev: row.root_dev.parse()?,
            root_ino: row.root_ino.parse()?,
            name: &stored.name,
            approved_sha256: &stored.approved_sha256,
            target_binding: stored.target_binding.as_deref(),
            source_label: &stored.source_label,
            enabled: stored.enabled,
            automatic: stored.automatic,
        },
    )?;
    let candidate = source.discover()?.candidates.remove(0);
    assert_eq!(
        approval.selection(),
        SkillSelection::from_candidate(&candidate)
    );
    let catalog = SkillCatalog::freeze(vec![candidate], std::slice::from_ref(&approval), false)?;
    let mut run = SkillRun::new(catalog, true);
    assert_eq!(
        run.load_explicit(&approval.selection(), |_| true)
            .receipt
            .status,
        "loaded"
    );
    assert!(run.render_skill_envelope()?.contains("Reviewed body"));
    Ok(())
}

#[test]
fn preview_cannot_approve_replaced_link_even_with_identical_body() -> Result {
    let fixture = Fixture::new()?;
    let preview = fixture
        .service
        .preview_skill(&fixture.source_id, "example")?;
    fixture.replace_link(&fixture.target)?;
    let generation = fixture.service.projection()?.consent_generation;
    assert_eq!(
        fixture.service.apply(
            generation,
            SkillSettingsMutation::ApproveSkill {
                preview_token: preview.token
            }
        ),
        Err(SkillSettingsError::Stale)
    );
    assert!(skills::list_approved_skills(fixture.service.store()?.conn())?.is_empty());
    Ok(())
}

#[test]
fn same_content_retarget_requires_review_and_reapproval_revokes_prior_generation() -> Result {
    let fixture = Fixture::new()?;
    let preview = fixture
        .service
        .preview_skill(&fixture.source_id, "example")?;
    fixture.apply(SkillSettingsMutation::ApproveSkill {
        preview_token: preview.token,
    })?;
    let before = fixture.service.projection()?;
    let other = fixture.base.join("other");
    fs::create_dir(&other)?;
    fs::copy(fixture.target.join("SKILL.md"), other.join("SKILL.md"))?;
    fixture.replace_link(&other)?;
    let changed = fixture.service.projection()?;
    assert_eq!(
        changed.sources[0].candidates[0].diagnostic.as_deref(),
        Some("changed_review_required")
    );
    assert!(
        fixture
            .service
            .composer_draft_projection("draft")?
            .candidates
            .is_empty()
    );
    let preview = fixture
        .service
        .preview_skill(&fixture.source_id, "example")?;
    assert_eq!(
        preview.linked_paths.as_ref().map(|paths| &paths.1),
        Some(&other.canonicalize()?)
    );
    fixture.apply(SkillSettingsMutation::ApproveSkill {
        preview_token: preview.token,
    })?;
    let after = fixture.service.projection()?;
    assert!(after.consent_generation > before.consent_generation);
    assert!(after.revocation_generation > before.revocation_generation);
    assert_eq!(
        fixture
            .service
            .composer_draft_projection("draft")?
            .candidates
            .len(),
        1
    );
    Ok(())
}

#[test]
fn changed_body_or_revoked_consent_cannot_reuse_preview() -> Result {
    let fixture = Fixture::new()?;
    let preview = fixture
        .service
        .preview_skill(&fixture.source_id, "example")?;
    fs::write(
        fixture.target.join("SKILL.md"),
        "---\nname: example\ndescription: Changed\n---\nChanged body\n",
    )?;
    let generation = fixture.service.projection()?.consent_generation;
    assert_eq!(
        fixture.service.apply(
            generation,
            SkillSettingsMutation::ApproveSkill {
                preview_token: preview.token
            }
        ),
        Err(SkillSettingsError::Stale)
    );
    let preview = fixture
        .service
        .preview_skill(&fixture.source_id, "example")?;
    fixture.apply(SkillSettingsMutation::SetGlobal {
        enabled: false,
        automatic: false,
    })?;
    let generation = fixture.service.projection()?.consent_generation;
    assert_eq!(
        fixture.service.apply(
            generation,
            SkillSettingsMutation::ApproveSkill {
                preview_token: preview.token
            }
        ),
        Err(SkillSettingsError::Stale)
    );
    assert!(skills::list_approved_skills(fixture.service.store()?.conn())?.is_empty());
    Ok(())
}
