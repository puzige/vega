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

fn add_limit_candidates(root: &Path, count: usize) {
    for index in 0..count {
        let name = format!("limit-skill-{index:03}");
        let directory = root.join(&name);
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("SKILL.md"),
            format!(
                "---\nname: {name}\ndescription: Review owned changes.\n---\nPRIVATE LIMIT BODY\n"
            ),
        )
        .unwrap();
    }
}

#[derive(Debug, PartialEq, Eq)]
struct ImportAuthority {
    settings: skills::SkillSettings,
    project: skills::ProjectSkillSettings,
    sources: Vec<SkillSourceRecord>,
    approvals: Vec<skills::SkillApprovalRecord>,
}

fn import_authority(store: &Store, project_id: &str) -> ImportAuthority {
    ImportAuthority {
        settings: skills::read_settings(store.conn()).unwrap(),
        project: skills::read_project_settings(store.conn(), project_id).unwrap(),
        sources: skills::list_sources(store.conn()).unwrap(),
        approvals: skills::list_approved_skills(store.conn()).unwrap(),
    }
}

#[test]
fn issue87_s22_import_rejects_129_candidates_without_mutating_authority() {
    let (owned, store, service, project_id) = fixture();
    let root = owned.path().join("private-limit-input");
    add_limit_candidates(&root, 129);
    let before = import_authority(&store, &project_id);
    assert!(before.sources.is_empty());
    assert!(before.approvals.is_empty());
    let error = service.preview_imported_root(&root).err().unwrap();
    assert_eq!(import_authority(&store, &project_id), before);
    assert!(service.projection().unwrap().sources.is_empty());
    let previews = service.previews.lock().unwrap();
    assert!(previews.root.is_none());
    assert!(previews.skill.is_none());
    assert_eq!(error.code(), "too_many_candidates");
    assert_eq!(error, SkillSettingsError::TooManyCandidates);
    assert_eq!(
        error.to_string(),
        "Skill source exceeds the candidate limit"
    );
}

#[test]
fn issue87_s22_import_previews_all_128_candidates_without_mutating_authority() {
    let (owned, store, service, project_id) = fixture();
    let root = owned.path().join("private-limit-input");
    add_limit_candidates(&root, 128);
    let before = import_authority(&store, &project_id);
    let preview = service.preview_imported_root(&root).unwrap();
    assert_eq!(preview.scope, SkillUiScope::Imported);
    assert_eq!(preview.canonical_root, root.canonicalize().unwrap());
    assert_eq!(preview.candidates.len(), 128);
    let names = preview
        .candidates
        .iter()
        .map(|candidate| candidate.name.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        names,
        (0..128)
            .map(|index| format!("limit-skill-{index:03}"))
            .collect()
    );
    for candidate in preview.candidates {
        assert!(candidate.content_sha256.is_some());
        assert!(candidate.approved_sha256.is_none());
        assert!(!candidate.enabled);
        assert!(!candidate.automatic);
        assert!(!candidate.model_winner);
        assert!(candidate.diagnostic.is_none());
    }
    assert_eq!(import_authority(&store, &project_id), before);
    assert!(service.projection().unwrap().sources.is_empty());
}

#[test]
fn issue87_s22_import_limit_retries_reduced_root_and_cancel_remains_unlinked_after_reopen() {
    let (owned, store, service, project_id) = fixture();
    let database = store.database_path().unwrap().to_path_buf();
    let root = owned.path().join("private-limit-input");
    add_limit_candidates(&root, 129);
    let before = import_authority(&store, &project_id);
    assert_eq!(
        service.preview_imported_root(&root).err(),
        Some(SkillSettingsError::TooManyCandidates)
    );
    for index in 1..129 {
        fs::remove_dir_all(root.join(format!("limit-skill-{index:03}"))).unwrap();
    }
    let preview = service.preview_imported_root(&root).unwrap();
    assert_eq!(preview.candidates.len(), 1);
    assert_eq!(preview.candidates[0].name, "limit-skill-000");
    assert_eq!(import_authority(&store, &project_id), before);
    service.clear_previews();
    assert_eq!(
        service.apply(
            before.settings.consent_generation,
            SkillSettingsMutation::LinkRoot {
                preview_token: preview.token,
            },
        ),
        Err(SkillSettingsError::PreviewRequired)
    );
    assert_eq!(import_authority(&store, &project_id), before);
    drop(service);
    drop(store);
    let reopened = Store::open(&database).unwrap();
    reopened.migrate().unwrap();
    assert_eq!(import_authority(&reopened, &project_id), before);
    let reopened_service =
        SkillSettingsService::new(database, owned.path().join("config"), Some(project_id));
    assert!(reopened_service.projection().unwrap().sources.is_empty());
    assert!(root.join("limit-skill-000/SKILL.md").is_file());
}

#[test]
fn issue87_s22_import_limit_does_not_change_invalid_or_unsafe_path_rejection() {
    let (owned, store, service, project_id) = fixture();
    let before = import_authority(&store, &project_id);
    let file_root = owned.path().join("private-file-root");
    fs::write(&file_root, "PRIVATE INVALID ROOT").unwrap();
    assert_eq!(
        service.preview_imported_root(&file_root).err(),
        Some(SkillSettingsError::Invalid)
    );
    let root = owned.path().join("private-unsafe-input");
    let outside = owned.path().join("outside-input");
    fs::create_dir_all(&root).unwrap();
    add_skill(&outside, "PRIVATE OUTSIDE BODY");
    std::os::unix::fs::symlink(outside.join("reviewer"), root.join("reviewer")).unwrap();
    let preview = service.preview_imported_root(&root).unwrap();
    assert_eq!(preview.candidates.len(), 1);
    let candidate = &preview.candidates[0];
    assert_eq!(candidate.name, "reviewer");
    assert_eq!(candidate.diagnostic.as_deref(), Some("unsafe_path"));
    assert!(candidate.description.is_none());
    assert!(candidate.content_sha256.is_none());
    assert!(candidate.size_bytes.is_none());
    assert!(candidate.approved_sha256.is_none());
    assert!(!candidate.enabled);
    assert!(!candidate.automatic);
    assert!(!candidate.model_winner);
    assert_eq!(import_authority(&store, &project_id), before);
    assert!(service.projection().unwrap().sources.is_empty());
    assert_eq!(
        fs::read_to_string(outside.join("reviewer/SKILL.md")).unwrap(),
        "---\nname: reviewer\ndescription: Review changes.\n---\nPRIVATE OUTSIDE BODY\n"
    );
}

#[test]
fn issue87_s22_root_preview_limit_mapping_applies_to_project_and_vega_global() {
    let (owned, store, service, project_id) = fixture();
    let before = import_authority(&store, &project_id);
    add_limit_candidates(&owned.path().join("project/.agents/skills"), 129);
    add_limit_candidates(&owned.path().join("config/skills"), 129);
    assert_eq!(
        service.preview_project_root().err(),
        Some(SkillSettingsError::TooManyCandidates)
    );
    assert_eq!(
        service.preview_vega_global_root().err(),
        Some(SkillSettingsError::TooManyCandidates)
    );
    assert_eq!(import_authority(&store, &project_id), before);
    assert!(service.projection().unwrap().sources.is_empty());
}

#[test]
fn issue87_s22_import_limit_preserves_existing_authority_and_switches() {
    let (owned, store, service, project_id) = fixture();
    let approved_root = owned.path().join("approved-input");
    add_skill(&approved_root, "EXISTING APPROVED BODY");
    let root_preview = service.preview_imported_root(&approved_root).unwrap();
    service
        .apply(
            generation(&store),
            SkillSettingsMutation::LinkRoot {
                preview_token: root_preview.token,
            },
        )
        .unwrap();
    let source_id = service.projection().unwrap().sources[0].id.clone();
    let body_preview = service.preview_skill(&source_id, "reviewer").unwrap();
    service
        .apply(
            generation(&store),
            SkillSettingsMutation::ApproveSkill {
                preview_token: body_preview.token,
            },
        )
        .unwrap();
    for mutation in [
        SkillSettingsMutation::SetGlobal {
            enabled: true,
            automatic: true,
        },
        SkillSettingsMutation::SetProject {
            project_id: project_id.clone(),
            enabled: true,
            automatic: true,
        },
        SkillSettingsMutation::SetSource {
            source_id,
            enabled: true,
            automatic: true,
        },
    ] {
        service.apply(generation(&store), mutation).unwrap();
    }
    service.clear_previews();
    let before = import_authority(&store, &project_id);
    assert_eq!(before.sources.len(), 1);
    assert_eq!(before.approvals.len(), 1);
    assert!(before.settings.global_enabled);
    assert!(before.settings.automatic_enabled);
    assert!(before.project.enabled);
    assert!(before.project.automatic);
    let root = owned.path().join("private-limit-input");
    add_limit_candidates(&root, 129);
    assert_eq!(
        service.preview_imported_root(&root).err(),
        Some(SkillSettingsError::TooManyCandidates)
    );
    assert_eq!(import_authority(&store, &project_id), before);
    let previews = service.previews.lock().unwrap();
    assert!(previews.root.is_none());
    assert!(previews.skill.is_none());
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
