use super::*;
use std::fs;
use tempfile::tempdir;

fn fixture() -> (tempfile::TempDir, Store, SkillSettingsService, String) {
    let owned = tempdir().unwrap();
    let project_root = owned.path().join("project");
    let config_root = owned.path().join("config");
    fs::create_dir_all(&project_root).unwrap();
    fs::create_dir_all(&config_root).unwrap();
    let database = owned.path().join("vega.db");
    let store = Store::open(&database).unwrap();
    store.migrate().unwrap();
    let project = vega_store::projects::create(
        store.conn(),
        project_root.to_str().unwrap(),
        "project",
        None,
    )
    .unwrap();
    let service = SkillSettingsService::new(database, config_root, Some(project.id.clone()));
    (owned, store, service, project.id)
}

fn add_skill(root: &Path, body: &str) {
    let directory = root.join("reviewer");
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("SKILL.md"),
        format!("---\nname: reviewer\ndescription: Review changes.\n---\n{body}\n"),
    )
    .unwrap();
}

fn generation(store: &Store) -> u64 {
    skills::read_settings(store.conn())
        .unwrap()
        .consent_generation
}

#[test]
fn project_review_is_hash_bound_and_stale_approval_cannot_reenable() {
    let (owned, store, service, project_id) = fixture();
    let root = owned.path().join("project/.agents/skills");
    add_skill(&root, "BODY ONE");
    let root_preview = service.preview_project_root().unwrap();
    assert_eq!(root_preview.scope, SkillUiScope::Project);
    assert_eq!(
        root_preview.project_id.as_deref(),
        Some(project_id.as_str())
    );
    assert_eq!(root_preview.candidates.len(), 1);
    assert!(service.projection().unwrap().sources.is_empty());
    service
        .apply(
            generation(&store),
            SkillSettingsMutation::LinkRoot {
                preview_token: root_preview.token,
            },
        )
        .unwrap();
    let source = service.projection().unwrap().sources.remove(0);
    assert!(!source.enabled);
    let first_preview = service.preview_skill(&source.id, "reviewer").unwrap();
    assert!(first_preview.body.contains("BODY ONE"));
    assert!(first_preview.body.contains("name: reviewer"));
    add_skill(&root, "BODY TWO");
    assert_eq!(
        service.apply(
            generation(&store),
            SkillSettingsMutation::ApproveSkill {
                preview_token: first_preview.token,
            },
        ),
        Err(SkillSettingsError::Stale)
    );
    assert!(
        skills::list_approved_skills(store.conn())
            .unwrap()
            .is_empty()
    );
    let second_preview = service.preview_skill(&source.id, "reviewer").unwrap();
    assert!(second_preview.body.contains("BODY TWO"));
    service
        .apply(
            generation(&store),
            SkillSettingsMutation::ApproveSkill {
                preview_token: second_preview.token,
            },
        )
        .unwrap();
    service
        .apply(
            generation(&store),
            SkillSettingsMutation::SetProject {
                project_id,
                enabled: true,
                automatic: true,
            },
        )
        .unwrap();
    service
        .apply(
            generation(&store),
            SkillSettingsMutation::SetSource {
                source_id: source.id.clone(),
                enabled: true,
                automatic: true,
            },
        )
        .unwrap();
    service
        .apply(
            generation(&store),
            SkillSettingsMutation::SetSkill {
                source_id: source.id,
                name: "reviewer".into(),
                enabled: true,
                automatic: true,
            },
        )
        .unwrap();
    let projection = service.projection().unwrap();
    assert!(projection.sources[0].candidates[0].model_winner);
    add_skill(&root, "BODY THREE");
    let changed = service.projection().unwrap();
    assert_eq!(
        changed.sources[0].candidates[0].diagnostic.as_deref(),
        Some("changed_review_required")
    );
    assert!(!changed.sources[0].candidates[0].model_winner);
}

#[test]
fn imported_root_requires_preview_and_unlink_never_deletes_source() {
    let (owned, store, service, _) = fixture();
    let external = owned.path().join("external-skills");
    add_skill(&external, "EXTERNAL BODY");
    let decoy = owned.path().join("unimported/.codex/skills");
    add_skill(&decoy, "DECOY BODY");
    assert!(service.projection().unwrap().sources.is_empty());
    let preview = service.preview_imported_root(&external).unwrap();
    assert_eq!(preview.scope, SkillUiScope::Imported);
    assert_eq!(preview.canonical_root, external.canonicalize().unwrap());
    assert_eq!(preview.candidates.len(), 1);
    let preview_label = preview.source_label.clone();
    service
        .apply(
            generation(&store),
            SkillSettingsMutation::LinkRoot {
                preview_token: preview.token,
            },
        )
        .unwrap();
    let linked = service.projection().unwrap();
    assert_eq!(linked.sources.len(), 1);
    assert_eq!(linked.sources[0].scope, SkillUiScope::Imported);
    assert_eq!(linked.sources[0].source_label, preview_label);
    let source_id = linked.sources[0].id.clone();
    let repeat = service.preview_imported_root(&external).unwrap();
    let stale_generation = generation(&store);
    service
        .apply(
            stale_generation,
            SkillSettingsMutation::SetGlobal {
                enabled: true,
                automatic: false,
            },
        )
        .unwrap();
    assert_eq!(
        service.apply(
            stale_generation,
            SkillSettingsMutation::LinkRoot {
                preview_token: repeat.token,
            },
        ),
        Err(SkillSettingsError::Stale)
    );
    let repeat = service.preview_imported_root(&external).unwrap();
    service
        .apply(
            generation(&store),
            SkillSettingsMutation::LinkRoot {
                preview_token: repeat.token,
            },
        )
        .unwrap();
    assert_eq!(service.projection().unwrap().sources.len(), 1);
    service
        .apply(
            generation(&store),
            SkillSettingsMutation::UnlinkSource { source_id },
        )
        .unwrap();
    assert!(service.projection().unwrap().sources.is_empty());
    assert!(external.join("reviewer/SKILL.md").is_file());
    assert!(decoy.join("reviewer/SKILL.md").is_file());
}

#[test]
fn stale_generation_and_cross_project_scope_cannot_mutate() {
    let (owned, store, service, project_id) = fixture();
    let root = owned.path().join("project/.agents/skills");
    add_skill(&root, "BODY");
    let preview = service.preview_project_root().unwrap();
    let stale_generation = generation(&store);
    service
        .apply(
            stale_generation,
            SkillSettingsMutation::SetProject {
                project_id: project_id.clone(),
                enabled: true,
                automatic: false,
            },
        )
        .unwrap();
    assert_eq!(
        service.apply(
            stale_generation,
            SkillSettingsMutation::LinkRoot {
                preview_token: preview.token,
            },
        ),
        Err(SkillSettingsError::Stale)
    );
    assert_eq!(
        service.apply(
            generation(&store),
            SkillSettingsMutation::SetProject {
                project_id: "another-project".into(),
                enabled: true,
                automatic: true,
            },
        ),
        Err(SkillSettingsError::Invalid)
    );
    assert!(service.projection().unwrap().sources.is_empty());
}

#[test]
fn vega_global_preview_uses_only_supplied_config_root() {
    let (owned, store, service, _) = fixture();
    let root = owned.path().join("config/skills");
    add_skill(&root, "VEGA BODY");
    let preview = service.preview_vega_global_root().unwrap();
    assert_eq!(preview.scope, SkillUiScope::VegaGlobal);
    assert_eq!(preview.configured_root, root);
    service
        .apply(
            generation(&store),
            SkillSettingsMutation::LinkRoot {
                preview_token: preview.token,
            },
        )
        .unwrap();
    assert_eq!(
        service.projection().unwrap().sources[0].scope,
        SkillUiScope::VegaGlobal
    );
}

#[test]
fn leaving_page_or_closing_settings_revokes_all_preview_receipts() {
    let (owned, store, service, _) = fixture();
    let root = owned.path().join("project/.agents/skills");
    add_skill(&root, "REVIEW BODY");
    let preview = service.preview_project_root().unwrap();
    service.clear_previews();
    assert_eq!(
        service.apply(
            generation(&store),
            SkillSettingsMutation::LinkRoot {
                preview_token: preview.token,
            },
        ),
        Err(SkillSettingsError::PreviewRequired)
    );
    let preview = service.preview_project_root().unwrap();
    service
        .apply(
            generation(&store),
            SkillSettingsMutation::LinkRoot {
                preview_token: preview.token,
            },
        )
        .unwrap();
    let source_id = service.projection().unwrap().sources[0].id.clone();
    let preview = service.preview_skill(&source_id, "reviewer").unwrap();
    let worker_clone = service.clone();
    service.close_session();
    assert_eq!(
        worker_clone.apply(
            generation(&store),
            SkillSettingsMutation::ApproveSkill {
                preview_token: preview.token,
            },
        ),
        Err(SkillSettingsError::Stale)
    );
    assert!(matches!(
        worker_clone.preview_skill(&source_id, "reviewer"),
        Err(SkillSettingsError::Stale)
    ));
    assert_eq!(
        worker_clone.apply(
            generation(&store),
            SkillSettingsMutation::SetGlobal {
                enabled: true,
                automatic: true,
            },
        ),
        Err(SkillSettingsError::Stale)
    );
    assert!(
        skills::list_approved_skills(store.conn())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn slow_preview_cannot_reinstall_receipt_after_page_leave() {
    let (owned, store, service, _) = fixture();
    let project = owned.path().join("project");
    let root = project.join(".agents/skills");
    add_skill(&root, "REVIEW BODY");
    let source = SkillSource::project_approved(&project).unwrap().unwrap();
    let previous_epoch = service.preview_epoch().unwrap();
    service.clear_previews();
    assert!(matches!(
        service.preview_root(&store, SkillUiScope::Project, root, source, previous_epoch,),
        Err(SkillSettingsError::Stale)
    ));
    let previews = service.previews.lock().unwrap();
    assert!(previews.root.is_none());
    assert!(previews.skill.is_none());
}

#[test]
fn composer_explicit_pin_is_exact_persistent_and_never_adopts_changed_bytes() {
    let (owned, store, service, project_id) = fixture();
    let root = owned.path().join("project/.agents/skills");
    add_skill(&root, "REVIEW BODY");
    let thread = crate::threads::create_thread(&store, &project_id, "mock", "confirm").unwrap();
    let preview = service.preview_project_root().unwrap();
    service
        .apply(
            generation(&store),
            SkillSettingsMutation::LinkRoot {
                preview_token: preview.token,
            },
        )
        .unwrap();
    let source_id = service.projection().unwrap().sources[0].id.clone();
    let preview = service.preview_skill(&source_id, "reviewer").unwrap();
    let approved_sha = preview.content_sha256.clone();
    service
        .apply(
            generation(&store),
            SkillSettingsMutation::ApproveSkill {
                preview_token: preview.token,
            },
        )
        .unwrap();
    service
        .apply(
            generation(&store),
            SkillSettingsMutation::SetProject {
                project_id: project_id.clone(),
                enabled: true,
                automatic: false,
            },
        )
        .unwrap();
    service
        .apply(
            generation(&store),
            SkillSettingsMutation::SetSource {
                source_id: source_id.clone(),
                enabled: true,
                automatic: false,
            },
        )
        .unwrap();
    let before = service.composer_projection(&thread.id).unwrap();
    assert_eq!(before.candidates.len(), 1);
    assert!(before.pins.is_empty());
    service
        .apply_composer(
            &thread.id,
            before.consent_generation,
            crate::types::SkillComposerMutation::Pin {
                source_id: source_id.clone(),
                name: "reviewer".into(),
                content_sha256: approved_sha.clone(),
            },
        )
        .unwrap();
    let reopened = SkillSettingsService::new(
        store.database_path().unwrap().to_path_buf(),
        owned.path().join("config"),
        Some(project_id),
    );
    let pinned = reopened.composer_projection(&thread.id).unwrap();
    assert_eq!(pinned.pins.len(), 1);
    assert!(pinned.pins[0].available);
    assert_eq!(pinned.pins[0].content_sha256, approved_sha);
    let pinned_generation = generation(&store);
    reopened
        .ensure_composer_pin(
            &thread.id,
            &crate::types::SkillSelectionIntent {
                source_id: source_id.clone(),
                name: "reviewer".into(),
                content_sha256: approved_sha.clone(),
                expected_consent_generation: before.consent_generation,
            },
        )
        .unwrap();
    assert_eq!(
        generation(&store),
        pinned_generation,
        "retry must not re-pin or bump CAS"
    );
    add_skill(&root, "CHANGED BODY");
    let changed = reopened.composer_projection(&thread.id).unwrap();
    assert!(changed.candidates.is_empty());
    assert_eq!(changed.pins.len(), 1);
    assert!(!changed.pins[0].available);
    assert_eq!(changed.pins[0].content_sha256, approved_sha);
    assert_eq!(
        reopened.ensure_composer_pin(
            &thread.id,
            &crate::types::SkillSelectionIntent {
                source_id: source_id.clone(),
                name: "reviewer".into(),
                content_sha256: approved_sha.clone(),
                expected_consent_generation: before.consent_generation,
            },
        ),
        Err(SkillSettingsError::Stale)
    );
    assert_eq!(
        reopened.apply_composer(
            &thread.id,
            changed.consent_generation,
            crate::types::SkillComposerMutation::Pin {
                source_id,
                name: "reviewer".into(),
                content_sha256: approved_sha,
            },
        ),
        Err(SkillSettingsError::Stale)
    );
}
