use super::*;
use crate::skill_settings::SkillSettingsService;
use crate::types::{SkillMention, SkillSelectionIntent, SkillSettingsMutation};
use std::fs;
use std::os::unix::fs::symlink;

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Fixture {
    base: PathBuf,
    root: PathBuf,
    target: PathBuf,
    service: SkillSettingsService,
    source_id: String,
}

impl Fixture {
    fn new() -> Result<Self> {
        let base = std::env::temp_dir().join(format!("skill-toggles-{}", ulid::Ulid::generate()));
        let root = base.join("catalog");
        let target = base.join("target");
        fs::create_dir_all(&root)?;
        fs::create_dir_all(&target)?;
        fs::write(
            target.join("SKILL.md"),
            "---\nname: example\ndescription: Example workflow\n---\nOriginal body\n",
        )?;
        symlink(&target, root.join("example"))?;
        let store = vega_store::Store::open(base.join("store.sqlite"))?;
        store.migrate()?;
        vega_store::threads::create_standalone(
            store.conn(),
            vega_store::threads::NewThread {
                id: "thread",
                project_id: "",
                title: "Test",
                mode: "ask",
                permission_mode: "readonly",
                permission_revision: 0,
                full_access_grant: None,
                reasoning_preferences: None,
                model: "test",
                status: "active",
                pinned: false,
                unread: false,
                created_at: 1,
                updated_at: 1,
            },
        )?;
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

    fn toggle(&self, enabled: bool) -> Result {
        self.apply(SkillSettingsMutation::SetSkill {
            source_id: self.source_id.clone(),
            name: "example".into(),
            enabled,
            automatic: enabled,
        })
    }

    fn store(&self) -> Result<vega_store::Store> {
        Ok(vega_store::Store::open(self.base.join("store.sqlite"))?)
    }

    fn prepare(&self, selections: &[SkillSelection]) -> Result<PreparedSkills> {
        prepare_skill_run_with_config_dir(
            &self.store()?,
            "",
            "thread",
            "run",
            true,
            Some(&self.base.join("config")),
            selections,
        )?
        .ok_or_else(|| "missing run".into())
    }

    fn mention(&self) -> Result<SkillMention> {
        let projection = self.service.composer_projection("thread")?;
        let candidate = projection.candidates.first().ok_or("missing candidate")?;
        Ok(SkillMention {
            range: 0..8,
            text: "$example".into(),
            intent: SkillSelectionIntent {
                source_id: candidate.source_id.clone(),
                name: candidate.name.clone(),
                content_sha256: candidate.content_sha256.clone(),
                target_binding: candidate.target_binding.clone(),
                expected_consent_generation: projection.consent_generation,
            },
        })
    }

    fn validate(
        &self,
        mention: &SkillMention,
        frozen: &[SkillSelection],
    ) -> Result<Vec<SkillSelection>> {
        let store = self.store()?;
        let admission = crate::thread_access::acquire(&store, "thread")?;
        Ok(self.service.validate_mentions_with_owner(
            "thread",
            "$example",
            std::slice::from_ref(mention),
            &admission,
            frozen,
        )?)
    }

    fn retarget(&self) -> Result {
        let next = self.base.join("next-target");
        fs::create_dir_all(&next)?;
        fs::copy(self.target.join("SKILL.md"), next.join("SKILL.md"))?;
        symlink(next, self.root.join("replacement"))?;
        fs::rename(self.root.join("replacement"), self.root.join("example"))?;
        Ok(())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

#[test]
fn valid_linked_skill_is_available_and_activates_without_item_review() -> Result {
    let fixture = Fixture::new()?;
    let projection = fixture.service.projection()?;
    let candidate = &projection.sources[0].candidates[0];
    assert!(candidate.enabled);
    assert!(candidate.diagnostic.is_none());
    assert!(candidate.approved_sha256.is_none());
    assert!(skills::list_approved_skills(fixture.store()?.conn())?.is_empty());
    let mention = fixture.mention()?;
    assert!(mention.intent.target_binding.is_some());
    let selections = fixture.validate(&mention, &[])?;
    let mut prepared = fixture.prepare(&selections)?;
    assert!(!prepared.run.model_catalog().contains("example"));
    assert_eq!(
        prepared
            .run
            .load_explicit(&selections[0], |_| true)
            .audit
            .status,
        "loaded"
    );
    assert!(
        prepared
            .run
            .render_skill_envelope()
            .map_err(|error| format!("{error:?}"))?
            .contains("Original body")
    );
    assert!(skills::list_approved_skills(fixture.store()?.conn())?.is_empty());
    Ok(())
}

#[test]
fn disabling_default_skill_persists_revokes_and_reopening_restores_candidates() -> Result {
    let fixture = Fixture::new()?;
    let mention = fixture.mention()?;
    let before = fixture.service.projection()?;
    fixture.toggle(false)?;
    let after = fixture.service.projection()?;
    assert!(after.consent_generation > before.consent_generation);
    assert!(after.revocation_generation > before.revocation_generation);
    assert!(
        fixture
            .service
            .composer_projection("thread")?
            .candidates
            .is_empty()
    );
    assert!(fixture.validate(&mention, &[]).is_err());
    let reloaded = SkillSettingsService::new(
        fixture.base.join("store.sqlite"),
        fixture.base.join("config"),
        None,
    );
    assert!(!reloaded.projection()?.sources[0].candidates[0].enabled);
    assert!(fixture.prepare(&[])?.run.model_catalog().is_empty());
    assert!(
        fixture
            .service
            .apply(
                before.consent_generation,
                SkillSettingsMutation::SetSkill {
                    source_id: fixture.source_id.clone(),
                    name: "example".into(),
                    enabled: true,
                    automatic: true
                }
            )
            .is_err()
    );
    fixture.toggle(true)?;
    assert_eq!(
        fixture
            .service
            .composer_projection("thread")?
            .candidates
            .len(),
        1
    );
    Ok(())
}

#[test]
fn same_content_retarget_rejects_old_draft_and_preserves_active_frozen_body() -> Result {
    let fixture = Fixture::new()?;
    let mention = fixture.mention()?;
    let selections = fixture.validate(&mention, &[])?;
    let mut prepared = fixture.prepare(&selections)?;
    assert_eq!(
        prepared
            .run
            .load_explicit(&selections[0], |_| true)
            .audit
            .status,
        "loaded"
    );
    let frozen = prepared.run.active_selections();
    let envelope = prepared
        .run
        .render_skill_envelope()
        .map_err(|error| format!("{error:?}"))?;
    fixture.retarget()?;
    assert!(fixture.validate(&mention, &[]).is_err());
    let fresh = fixture.mention()?;
    assert_eq!(fresh.intent.content_sha256, mention.intent.content_sha256);
    assert_ne!(fresh.intent.target_binding, mention.intent.target_binding);
    assert_eq!(fixture.validate(&mention, &frozen)?, selections);
    assert_eq!(
        prepared
            .run
            .render_skill_envelope()
            .map_err(|error| format!("{error:?}"))?,
        envelope
    );
    let fresh_selections = fixture.validate(&fresh, &[])?;
    assert_eq!(
        fixture
            .prepare(&fresh_selections)?
            .run
            .load_explicit(&fresh_selections[0], |_| true)
            .audit
            .status,
        "loaded"
    );
    Ok(())
}

#[test]
fn exact_pin_without_review_is_available_until_its_target_changes() -> Result {
    let fixture = Fixture::new()?;
    let mention = fixture.mention()?;
    fixture
        .service
        .ensure_composer_pin("thread", &mention.intent)?;
    let projection = fixture.service.composer_projection("thread")?;
    assert!(projection.candidates[0].selected);
    assert!(projection.pins[0].available);
    let mut prepared = fixture.prepare(&[])?;
    assert_eq!(prepared.explicit.len(), 1);
    assert_eq!(
        prepared
            .run
            .load_explicit(&prepared.explicit[0], |_| true)
            .audit
            .status,
        "loaded"
    );
    fixture.retarget()?;
    let projection = fixture.service.composer_projection("thread")?;
    assert!(!projection.candidates[0].selected);
    assert!(!projection.pins[0].available);
    assert!(fixture.prepare(&[]).is_err());
    Ok(())
}

#[test]
fn automatic_activation_requires_both_source_and_global_gates() -> Result {
    let fixture = Fixture::new()?;
    fixture.apply(SkillSettingsMutation::SetGlobal {
        enabled: true,
        automatic: true,
    })?;
    assert!(
        !fixture
            .prepare(&[])?
            .run
            .model_catalog()
            .contains("example")
    );
    fixture.apply(SkillSettingsMutation::SetSource {
        source_id: fixture.source_id.clone(),
        enabled: true,
        automatic: true,
    })?;
    let mut prepared = fixture.prepare(&[])?;
    assert!(prepared.run.model_catalog().contains("example"));
    assert_eq!(
        prepared.run.load_model("example", |_| true).audit.status,
        "loaded"
    );
    fixture.toggle(false)?;
    assert!(
        !fixture
            .prepare(&[])?
            .run
            .model_catalog()
            .contains("example")
    );
    fixture.toggle(true)?;
    fixture.apply(SkillSettingsMutation::SetGlobal {
        enabled: false,
        automatic: true,
    })?;
    assert!(
        fixture
            .service
            .composer_projection("thread")?
            .candidates
            .is_empty()
    );
    assert!(
        !fixture
            .prepare(&[])?
            .run
            .model_catalog()
            .contains("example")
    );
    Ok(())
}

#[test]
fn format_errors_and_unsafe_internal_links_remain_unavailable() -> Result {
    let fixture = Fixture::new()?;
    fs::write(
        fixture.target.join("SKILL.md"),
        "---\nname: example\ndescription: Example\nmetadata:\n  nested:\n    value: true\n---\nBody\n",
    )?;
    let projection = fixture.service.projection()?;
    assert_eq!(
        projection.sources[0].candidates[0].diagnostic.as_deref(),
        Some("invalid_metadata")
    );
    assert!(
        fixture
            .service
            .composer_projection("thread")?
            .candidates
            .is_empty()
    );
    assert!(fixture.toggle(true).is_err());
    let outside = fixture.base.join("outside.md");
    fs::write(
        &outside,
        "---\nname: example\ndescription: Example\n---\nBody\n",
    )?;
    fs::remove_file(fixture.target.join("SKILL.md"))?;
    symlink(&outside, fixture.target.join("SKILL.md"))?;
    assert!(
        fixture
            .service
            .composer_projection("thread")?
            .candidates
            .is_empty()
    );
    fs::remove_file(fixture.target.join("SKILL.md"))?;
    fs::hard_link(&outside, fixture.target.join("SKILL.md"))?;
    assert!(
        fixture
            .service
            .composer_projection("thread")?
            .candidates
            .is_empty()
    );
    assert!(fixture.toggle(true).is_err());
    assert!(matches!(
        fixture.service.projection()?.sources[0].candidates[0]
            .diagnostic
            .as_deref(),
        Some("hardlink")
    ));
    Ok(())
}
