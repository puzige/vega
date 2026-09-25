use super::*;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::Duration;
use vega_runtime::skills::{SkillSource, SourceScope};
use vega_store::skills::{self, NewSkillApproval, NewSkillSource, NewThreadSkillPin};

fn add_skill(root: &Path, name: &str, body: &str) {
    let directory = root.join(name);
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Review code changes.\n---\n{body}\n"),
    )
    .unwrap();
}

fn approve(
    store: &Store,
    source: &SkillSource,
    source_id: &str,
    project_id: Option<&str>,
    configured_root: &Path,
    automatic: bool,
) -> (String, String) {
    let candidate = source.discover().unwrap().candidates.remove(0);
    let identity = source.identity();
    let scope = match identity.scope() {
        SourceScope::Project => "project",
        SourceScope::VegaGlobal => "vega_global",
        SourceScope::Imported => "imported",
    };
    let root_dev = identity.device().to_string();
    let root_ino = identity.inode().to_string();
    let settings = skills::read_settings(store.conn()).unwrap();
    skills::link_source(
        store.conn(),
        settings.consent_generation,
        NewSkillSource {
            id: source_id,
            scope,
            project_id,
            configured_root: configured_root.to_str().unwrap(),
            canonical_root: identity.canonical_root().to_str().unwrap(),
            root_dev: &root_dev,
            root_ino: &root_ino,
            import_order: 0,
            enabled: true,
            automatic,
            created_at: 1,
        },
    )
    .unwrap();
    let settings = skills::read_settings(store.conn()).unwrap();
    skills::approve_skill(
        store.conn(),
        settings.consent_generation,
        NewSkillApproval {
            source_id,
            name: &candidate.name,
            approved_sha256: &candidate.sha256,
            source_label: source_id,
            enabled: true,
            automatic,
            reviewed_at: 2,
        },
    )
    .unwrap();
    (candidate.name, candidate.sha256)
}

fn load_provider() -> MockProvider {
    MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "load-reviewer".into(),
                name: "load_skill".into(),
                input_json: r#"{"name":"reviewer"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ])
}

#[tokio::test]
async fn issue74_project_auto_load_persists_audit_snapshot_and_content_free_receipt() {
    let (store, project, project_id) = setup();
    let skills_root = project.path().join(".agents/skills");
    add_skill(&skills_root, "reviewer", "PRIVATE PROJECT RULE");
    let source = SkillSource::project_approved(project.path())
        .unwrap()
        .unwrap();
    let settings = skills::read_settings(store.conn()).unwrap();
    skills::set_project_settings(
        store.conn(),
        settings.consent_generation,
        &project_id,
        true,
        true,
    )
    .unwrap();
    approve(
        &store,
        &source,
        "source-project",
        Some(&project_id),
        &skills_root,
        true,
    );
    let provider = load_provider();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let run = run_thread_task_with_images_and_reasoning(
        &store,
        &provider,
        &tools,
        "thread-1",
        "review the change",
        "System",
        CancellationToken::new(),
        &FixedPermissionHook {
            calls: Arc::new(AtomicUsize::new(0)),
            decision: PermissionDecision::Deny { note: None },
        },
        |_| Ok(()),
        PersistenceActorConfig::default(),
        None,
        None,
        None,
        Vec::new(),
    )
    .await
    .unwrap();
    assert!(!run.failed);
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    assert!(
        requests[0].messages[0]
            .content
            .contains("Review code changes.")
    );
    assert!(
        !requests[0].messages[0]
            .content
            .contains("PRIVATE PROJECT RULE")
    );
    assert!(
        requests[1].messages[0]
            .content
            .contains("PRIVATE PROJECT RULE")
    );
    let tool = tool_calls::find_state(store.conn(), "load-reviewer")
        .unwrap()
        .unwrap();
    assert_eq!(tool.status, "success");
    assert!(!tool.output_text.unwrap().contains("PRIVATE PROJECT RULE"));
    let page = crate::history::latest_history_page(&store, "thread-1", 20).unwrap();
    let (input, result) = page
        .entries
        .iter()
        .find_map(|entry| match entry {
            crate::history::HistoryEntry::Tool {
                call_id,
                input,
                result,
                ..
            } if call_id == "load-reviewer" => Some((input, result)),
            _ => None,
        })
        .expect("durable load_skill tool card");
    assert!(!matches!(
        input,
        Some(crate::types::ToolCardInputProjection::Corrupt) | None
    ));
    assert!(!matches!(
        result,
        Some(crate::types::ToolCardResultProjection::Corrupt) | None
    ));
    let audits = skills::list_activation_audits(store.conn(), "thread-1").unwrap();
    assert_eq!(audits.len(), 1);
    assert_eq!(audits[0].status, "loaded");
    let snapshot = skills::load_recoverable_snapshot(store.conn(), &run.assistant_message_id)
        .unwrap()
        .unwrap();
    assert!(!snapshot.bytes.is_empty());
    let binding = vega_runtime::skills::RunBinding::from_trusted_parts(
        &snapshot.run_id,
        &snapshot.thread_id,
        snapshot.consent_generation,
        snapshot.revocation_generation,
        &snapshot.catalog_sha256,
    )
    .unwrap();
    let approved_sha = source.discover().unwrap().candidates.remove(0).sha256;
    add_skill(&skills_root, "reviewer", "CHANGED AFTER RUN");
    let reopened = Store::open(store.database_path().unwrap()).unwrap();
    let restarted_page = crate::history::restart_history_page(&reopened, "thread-1", 20).unwrap();
    assert!(restarted_page.entries.iter().any(|entry| matches!(
        entry,
        crate::history::HistoryEntry::Tool {
            call_id,
            status: crate::types::ToolCallStatus::Success,
            input: Some(crate::types::ToolCardInputProjection::Skill { .. }),
            result: Some(crate::types::ToolCardResultProjection::Skill { .. }),
            ..
        } if call_id == "load-reviewer"
    )));
    let report = super::super::recover_skill_run(&reopened, &run.assistant_message_id, "thread-1")
        .unwrap()
        .unwrap();
    assert_eq!(
        report.assistant_status,
        super::super::SkillAssistantStatus::Done
    );
    assert_eq!(report.activations.len(), 1);
    assert_eq!(report.activations[0].name, "reviewer");
    assert_eq!(report.activations[0].content_sha256, approved_sha);
    assert!(
        super::super::recover_skill_run(&reopened, &run.assistant_message_id, "another-thread",)
            .is_err()
    );
    reopened
        .conn()
        .execute(
            "UPDATE messages SET status = 'streaming' WHERE id = ?1",
            [&run.assistant_message_id],
        )
        .unwrap();
    vega_store::recovery::recover_thread(reopened.conn(), "thread-1", 101).unwrap();
    let interrupted =
        super::super::recover_skill_run(&reopened, &run.assistant_message_id, "thread-1")
            .unwrap()
            .unwrap();
    assert_eq!(
        interrupted.assistant_status,
        super::super::SkillAssistantStatus::Interrupted
    );
    let restored = vega_runtime::skills::SkillRun::restore_snapshot(
        &snapshot.bytes,
        &binding,
        &snapshot.snapshot_sha256,
    )
    .unwrap();
    assert!(
        restored
            .render_skill_envelope()
            .unwrap()
            .contains("PRIVATE PROJECT RULE"),
        "restart keeps frozen approved bytes, not changed disk content"
    );
    let wrong_run = vega_runtime::skills::RunBinding::from_trusted_parts(
        "other-run",
        &snapshot.thread_id,
        snapshot.consent_generation,
        snapshot.revocation_generation,
        &snapshot.catalog_sha256,
    )
    .unwrap();
    assert!(
        vega_runtime::skills::SkillRun::restore_snapshot(
            &snapshot.bytes,
            &wrong_run,
            &snapshot.snapshot_sha256,
        )
        .is_err()
    );
    let settings = skills::read_settings(store.conn()).unwrap();
    skills::set_project_settings(
        store.conn(),
        settings.consent_generation,
        &project_id,
        false,
        false,
    )
    .unwrap();
    assert!(matches!(
        skills::load_recoverable_snapshot(store.conn(), &run.assistant_message_id),
        Err(skills::SkillStoreError::Stale)
    ));
    assert!(
        super::super::recover_skill_run(&reopened, &run.assistant_message_id, "thread-1",).is_err()
    );
}

#[tokio::test]
async fn issue74_mid_run_disable_cancels_stream_before_later_tool_side_effects() {
    let (store, project, project_id) = setup();
    let skills_root = project.path().join(".agents/skills");
    add_skill(&skills_root, "reviewer", "PRIVATE PROJECT RULE");
    let source = SkillSource::project_approved(project.path())
        .unwrap()
        .unwrap();
    let settings = skills::read_settings(store.conn()).unwrap();
    skills::set_project_settings(
        store.conn(),
        settings.consent_generation,
        &project_id,
        true,
        true,
    )
    .unwrap();
    approve(
        &store,
        &source,
        "source-project",
        Some(&project_id),
        &skills_root,
        true,
    );
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "load-reviewer".into(),
                name: "load_skill".into(),
                input_json: r#"{"name":"reviewer"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![
            ScriptStep::delay(Duration::from_secs(30)),
            ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: "late-write".into(),
                    name: "write".into(),
                    input_json: r#"{"path":"late.txt","content":"SHOULD NOT WRITE"}"#.into(),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ]),
        ],
    ]);
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let permission_calls = Arc::new(AtomicUsize::new(0));
    let hook = FixedPermissionHook {
        calls: permission_calls.clone(),
        decision: PermissionDecision::Once,
    };
    let database = store.database_path().unwrap().to_path_buf();
    let run_future = run_thread_task_with_images_and_reasoning(
        &store,
        &provider,
        &tools,
        "thread-1",
        "review then wait",
        "System",
        CancellationToken::new(),
        &hook,
        |_| Ok(()),
        PersistenceActorConfig::default(),
        None,
        None,
        None,
        Vec::new(),
    );
    let revoke_future = async {
        for _ in 0..200 {
            if provider.requests().len() >= 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(
            provider.requests().len(),
            2,
            "run reached active Skill round"
        );
        let second_store = Store::open(&database).unwrap();
        let settings = skills::read_settings(second_store.conn()).unwrap();
        skills::set_project_settings(
            second_store.conn(),
            settings.consent_generation,
            &project_id,
            false,
            false,
        )
        .unwrap();
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(4), async {
        tokio::join!(run_future, revoke_future)
    })
    .await
    .expect("revocation cancels provider stream");
    let run = result.unwrap();
    assert!(run.interrupted);
    assert!(!run.failed);
    assert_eq!(permission_calls.load(Ordering::SeqCst), 0);
    assert!(!project.path().join("late.txt").exists());
    assert!(
        tool_calls::find_state(store.conn(), "late-write")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        vega_store::messages::find(store.conn(), &run.assistant_message_id)
            .unwrap()
            .unwrap()
            .status,
        "interrupted"
    );
    let audits = skills::list_activation_audits(store.conn(), "thread-1").unwrap();
    assert_eq!(audits.len(), 2);
    assert_eq!(audits[0].status, "loaded");
    assert_eq!(audits[1].status, "revoked");
    assert_eq!(audits[1].name, "reviewer");
    assert_eq!(audits[1].content_sha256, audits[0].content_sha256);
    assert!(!format!("{audits:?}").contains("PRIVATE PROJECT RULE"));
    assert!(!format!("{audits:?}").contains(project.path().to_str().unwrap()));
    let reopened = Store::open(&database).unwrap();
    let repeated = skills::append_revocation_audit_once(
        reopened.conn(),
        skills::NewSkillRevocationAudit {
            run_id: &run.assistant_message_id,
            thread_id: "thread-1",
            name: "reviewer",
            source_scope: "project",
            content_sha256: audits[0].content_sha256.as_deref().unwrap(),
            created_at: 101,
        },
    )
    .unwrap();
    assert!(!repeated);
    assert_eq!(
        skills::list_activation_audits(reopened.conn(), "thread-1")
            .unwrap()
            .len(),
        2
    );
    fs::remove_file(skills_root.join("reviewer/SKILL.md")).unwrap();
    fs::remove_dir(skills_root.join("reviewer")).unwrap();
    let page = crate::history::restart_history_page(&reopened, "thread-1", 20).unwrap();
    let provenance = page
        .entries
        .iter()
        .find_map(|entry| match entry {
            crate::history::HistoryEntry::SkillActivation { activation, .. } => Some(activation),
            _ => None,
        })
        .expect("revoked Skill remains visible after Store reopen without source file");
    assert_eq!(provenance.run_id, run.assistant_message_id);
    assert_eq!(provenance.name, "reviewer");
    assert_eq!(
        provenance.status,
        crate::history::SkillHistoryStatus::Revoked
    );
    assert_eq!(provenance.origin, crate::history::SkillHistoryOrigin::Model);
    assert_eq!(
        provenance.verification,
        crate::history::SkillHistoryVerification::Unavailable,
        "revoked consent cannot reuse a formerly valid frozen snapshot"
    );
    assert_eq!(
        provenance.content_sha256,
        audits[0].content_sha256.as_deref().unwrap()
    );
    assert!(page.entries.iter().any(|entry| matches!(
        entry,
        crate::history::HistoryEntry::AssistantText {
            message_id,
            status: crate::history::AssistantStatus::Interrupted,
            ..
        } if message_id == &run.assistant_message_id
    )));
    assert!(!format!("{page:?}").contains("PRIVATE PROJECT RULE"));
    assert!(!format!("{page:?}").contains(project.path().to_str().unwrap()));
}

#[tokio::test]
async fn issue74_explicit_pin_is_loaded_before_round_one_and_stale_pin_pauses() {
    let (store, project, project_id) = setup();
    let skills_root = project.path().join(".agents/skills");
    add_skill(&skills_root, "reviewer", "PRIVATE PIN RULE");
    let source = SkillSource::project_approved(project.path())
        .unwrap()
        .unwrap();
    let settings = skills::read_settings(store.conn()).unwrap();
    skills::set_project_settings(
        store.conn(),
        settings.consent_generation,
        &project_id,
        true,
        false,
    )
    .unwrap();
    let (name, hash) = approve(
        &store,
        &source,
        "source-project",
        Some(&project_id),
        &skills_root,
        false,
    );
    let identity = source.identity();
    let dev = identity.device().to_string();
    let ino = identity.inode().to_string();
    let settings = skills::read_settings(store.conn()).unwrap();
    skills::save_thread_pin(
        store.conn(),
        settings.consent_generation,
        NewThreadSkillPin {
            thread_id: "thread-1",
            scope: "project",
            canonical_root: identity.canonical_root().to_str().unwrap(),
            root_dev: &dev,
            root_ino: &ino,
            name: &name,
            approved_sha256: &hash,
            source_label: "source-project",
            pinned_at: 3,
        },
    )
    .unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![ProviderEvent::Done {
        stop_reason: StopReason::End,
    }])]);
    let hook = FixedPermissionHook {
        calls: Arc::new(AtomicUsize::new(0)),
        decision: PermissionDecision::Deny { note: None },
    };
    let run = run_thread_task_with_images_and_reasoning(
        &store,
        &provider,
        &tools,
        "thread-1",
        "do the review",
        "System",
        CancellationToken::new(),
        &hook,
        |_| Ok(()),
        PersistenceActorConfig::default(),
        None,
        None,
        None,
        Vec::new(),
    )
    .await
    .unwrap();
    assert!(!run.failed);
    assert!(run.events.iter().any(|event| matches!(
        event,
        crate::types::ConversationEvent::SkillActivated { skill, .. }
            if skill.name == "reviewer"
                && skill.source_label == "source-project"
                && skill.content_sha256 == hash
    )));
    assert!(
        provider.requests()[0].messages[0]
            .content
            .contains("PRIVATE PIN RULE")
    );
    add_skill(&skills_root, "reviewer", "CHANGED WITHOUT REVIEW");
    let provider = MockProvider::new(vec![ScriptStep::events(vec![ProviderEvent::Done {
        stop_reason: StopReason::End,
    }])]);
    let failed = run_thread_task_with_images_and_reasoning(
        &store,
        &provider,
        &tools,
        "thread-1",
        "do it again",
        "System",
        CancellationToken::new(),
        &hook,
        |_| Ok(()),
        PersistenceActorConfig::default(),
        None,
        None,
        None,
        Vec::new(),
    )
    .await
    .unwrap();
    assert!(failed.failed);
    assert!(provider.requests().is_empty());
    fs::remove_file(skills_root.join("reviewer/SKILL.md")).unwrap();
    fs::remove_dir(skills_root.join("reviewer")).unwrap();
    let reopened = Store::open(store.database_path().unwrap()).unwrap();
    let page = crate::history::restart_history_page(&reopened, "thread-1", 20).unwrap();
    let provenance = page
        .entries
        .iter()
        .find_map(|entry| match entry {
            crate::history::HistoryEntry::SkillActivation { activation, .. } => Some(activation),
            _ => None,
        })
        .expect("explicit Skill preload survives restart without source file");
    assert_eq!(provenance.run_id, run.assistant_message_id);
    assert_eq!(provenance.name, "reviewer");
    assert_eq!(
        provenance.origin,
        crate::history::SkillHistoryOrigin::ExplicitUser
    );
    assert_eq!(
        provenance.status,
        crate::history::SkillHistoryStatus::Loaded
    );
    assert_eq!(
        provenance.verification,
        crate::history::SkillHistoryVerification::Verified,
        "history verifies Store-owned binding and exact frozen activation"
    );
    assert_eq!(provenance.content_sha256, hash);
    let assistant_position = page
        .entries
        .iter()
        .rposition(|entry| {
            matches!(
                entry,
                crate::history::HistoryEntry::AssistantText { message_id, .. }
                    if message_id == &run.assistant_message_id
            )
        })
        .unwrap();
    let skill_position = page
        .entries
        .iter()
        .position(|entry| {
            matches!(
                entry,
                crate::history::HistoryEntry::SkillActivation { activation, .. }
                    if activation.run_id == run.assistant_message_id
            )
        })
        .unwrap();
    assert_eq!(skill_position, assistant_position + 1);
    assert!(
        !page.entries.iter().any(|entry| matches!(
            entry,
            crate::history::HistoryEntry::Tool { message_id, .. }
                if message_id == &run.assistant_message_id
        )),
        "explicit preload must not masquerade as a tool call"
    );
    assert!(!format!("{page:?}").contains("PRIVATE PIN RULE"));
    assert!(!format!("{page:?}").contains(project.path().to_str().unwrap()));
    // Even an internally consistent new outer digest cannot turn arbitrary
    // bytes into a validated frozen run; the audit remains only unverified
    // historical provenance, not restored authority.
    let tampered = b"tampered-skill-snapshot";
    let tampered_sha = format!("{:x}", Sha256::digest(tampered));
    reopened
        .conn()
        .execute(
            "UPDATE skill_run_snapshots SET bytes = ?2, snapshot_sha256 = ?3 WHERE run_id = ?1",
            (
                &run.assistant_message_id,
                tampered.as_slice(),
                tampered_sha.as_str(),
            ),
        )
        .unwrap();
    let page = crate::history::latest_history_page(&reopened, "thread-1", 20).unwrap();
    let provenance = page
        .entries
        .iter()
        .find_map(|entry| match entry {
            crate::history::HistoryEntry::SkillActivation { activation, .. } => Some(activation),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        provenance.verification,
        crate::history::SkillHistoryVerification::Unavailable
    );
}

#[tokio::test]
async fn issue74_imported_global_auto_respects_ui_switches_without_ambient_scan() {
    let (store, project, _) = setup();
    let external = tempdir().unwrap();
    add_skill(external.path(), "reviewer", "PRIVATE GLOBAL RULE");
    let source = SkillSource::imported_approved(external.path(), 0).unwrap();
    approve(
        &store,
        &source,
        "source-imported",
        None,
        external.path(),
        true,
    );
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let provider = MockProvider::new(vec![ScriptStep::events(vec![ProviderEvent::Done {
        stop_reason: StopReason::End,
    }])]);
    let hook = FixedPermissionHook {
        calls: Arc::new(AtomicUsize::new(0)),
        decision: PermissionDecision::Deny { note: None },
    };
    let first_run = run_thread_task_with_images_and_reasoning(
        &store,
        &provider,
        &tools,
        "thread-1",
        "review",
        "System",
        CancellationToken::new(),
        &hook,
        |_| Ok(()),
        PersistenceActorConfig::default(),
        None,
        None,
        None,
        Vec::new(),
    )
    .await
    .unwrap();
    let first_run_diagnostics = vega_store::run_diagnostics::read_by_run(
        store.conn(),
        "thread-1",
        &first_run.assistant_message_id,
    )
    .unwrap();
    assert!(first_run_diagnostics.iter().any(|event| {
        event.event.phase == vega_store::run_diagnostics::DiagnosticPhase::Run
            && event.event.state == vega_store::run_diagnostics::DiagnosticState::Succeeded
    }));
    assert!(
        !provider.requests()[0].messages[0]
            .content
            .contains("Review code changes.")
    );
    let settings = skills::read_settings(store.conn()).unwrap();
    skills::set_global_settings(store.conn(), settings.consent_generation, true, true).unwrap();
    let provider = load_provider();
    let run = run_thread_task_with_images_and_reasoning(
        &store,
        &provider,
        &tools,
        "thread-1",
        "review again",
        "System",
        CancellationToken::new(),
        &hook,
        |_| Ok(()),
        PersistenceActorConfig::default(),
        None,
        None,
        None,
        Vec::new(),
    )
    .await
    .unwrap();
    assert!(!run.failed);
    assert!(
        provider.requests()[0].messages[0]
            .content
            .contains("Review code changes.")
    );
    assert!(
        provider.requests()[1].messages[0]
            .content
            .contains("PRIVATE GLOBAL RULE")
    );
}

#[test]
fn issue74_vega_owned_global_requires_exact_ui_link_and_uses_config_dir_root() {
    let (store, _project, project_id) = setup();
    let config_parent = tempdir().unwrap();
    let config_dir = config_parent.path().join("vega");
    let owned_root = config_dir.join("skills");
    let pi_root = config_parent.path().join(".pi/agent/skills");
    add_skill(&owned_root, "reviewer", "PRIVATE VEGA GLOBAL RULE");
    add_skill(&pi_root, "pi-reviewer", "PRIVATE PI RULE");
    let source = SkillSource::vega_global(&config_dir).unwrap().unwrap();
    let settings = skills::read_settings(store.conn()).unwrap();
    skills::set_global_settings(store.conn(), settings.consent_generation, true, true).unwrap();
    assert!(
        super::super::skills::prepare_skill_run_with_config_dir(
            &store,
            &project_id,
            "thread-1",
            "run-without-link",
            true,
            Some(&config_dir),
        )
        .unwrap()
        .is_none(),
        "an existing directory is not consent"
    );
    approve(
        &store,
        &source,
        "source-vega-global",
        None,
        &owned_root,
        true,
    );
    let mut prepared = super::super::skills::prepare_skill_run_with_config_dir(
        &store,
        &project_id,
        "thread-1",
        "run-with-link",
        true,
        Some(&config_dir),
    )
    .unwrap()
    .unwrap();
    assert!(prepared.run.model_catalog().contains("reviewer"));
    assert!(!prepared.run.model_catalog().contains("pi-reviewer"));
    assert_eq!(
        prepared.run.load_model("reviewer", |_| true).receipt.status,
        "loaded"
    );
    let envelope = prepared.run.render_skill_envelope().unwrap();
    assert!(envelope.contains("PRIVATE VEGA GLOBAL RULE"));
    assert!(!envelope.contains("PRIVATE PI RULE"));
}

#[tokio::test]
async fn issue74_reference_result_persists_lower_trust_bytes_without_path_in_audit() {
    let (store, project, project_id) = setup();
    let skills_root = project.path().join(".agents/skills");
    add_skill(&skills_root, "reviewer", "PRIVATE BODY");
    let references = skills_root.join("reviewer/references");
    fs::create_dir_all(&references).unwrap();
    let reference_body = format!("PRIVATE REFERENCE\n{}", "\u{0001}".repeat(8_000));
    fs::write(references.join("notes.md"), &reference_body).unwrap();
    let source = SkillSource::project_approved(project.path())
        .unwrap()
        .unwrap();
    let settings = skills::read_settings(store.conn()).unwrap();
    skills::set_project_settings(
        store.conn(),
        settings.consent_generation,
        &project_id,
        true,
        true,
    )
    .unwrap();
    approve(
        &store,
        &source,
        "source-project",
        Some(&project_id),
        &skills_root,
        true,
    );
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "load-reviewer".into(),
                name: "load_skill".into(),
                input_json: r#"{"name":"reviewer"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "ref-notes".into(),
                name: "read_skill_resource".into(),
                input_json: r#"{"name":"reviewer","path":"references/notes.md"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let hook = FixedPermissionHook {
        calls: Arc::new(AtomicUsize::new(0)),
        decision: PermissionDecision::Deny { note: None },
    };
    let run = run_thread_task_with_images_and_reasoning(
        &store,
        &provider,
        &tools,
        "thread-1",
        "read the note",
        "System",
        CancellationToken::new(),
        &hook,
        |_| Ok(()),
        PersistenceActorConfig::default(),
        None,
        None,
        None,
        Vec::new(),
    )
    .await
    .unwrap();
    assert!(
        !run.failed,
        "{:?}",
        run.events
            .iter()
            .filter_map(|event| match event {
                ConversationEvent::Error { error, .. } => Some(error.to_string()),
                _ => None,
            })
            .collect::<Vec<_>>()
    );
    let result = tool_calls::find_state(store.conn(), "ref-notes")
        .unwrap()
        .unwrap();
    assert_eq!(result.status, "success");
    assert!(result.input_json.contains("path_sha256"));
    assert!(!result.input_json.contains("references/notes.md"));
    let output = result.output_text.unwrap();
    assert!(output.contains("PRIVATE REFERENCE"));
    assert!(output.len() > 34 * 1024);
    let page = crate::history::latest_history_page(&store, "thread-1", 20).unwrap();
    let (input, result) = page
        .entries
        .iter()
        .find_map(|entry| match entry {
            crate::history::HistoryEntry::Tool {
                call_id,
                input,
                result,
                ..
            } if call_id == "ref-notes" => Some((input, result)),
            _ => None,
        })
        .expect("durable read_skill_resource tool card");
    assert!(!matches!(
        input,
        Some(crate::types::ToolCardInputProjection::Corrupt) | None
    ));
    assert!(!matches!(
        result,
        Some(crate::types::ToolCardResultProjection::Corrupt) | None
    ));
    assert!(!format!("{input:?} {result:?}").contains("PRIVATE REFERENCE"));
    assert!(!format!("{input:?} {result:?}").contains("references/notes.md"));
    assert!(
        provider.requests()[2].messages[0]
            .content
            .contains("PRIVATE BODY")
    );
    assert!(
        !provider.requests()[2].messages[0]
            .content
            .contains("PRIVATE REFERENCE")
    );
    let audits = skills::list_activation_audits(store.conn(), "thread-1").unwrap();
    assert_eq!(audits.len(), 1);
    assert!(!format!("{audits:?}").contains("PRIVATE REFERENCE"));
    let snapshot = skills::load_recoverable_snapshot(store.conn(), &run.assistant_message_id)
        .unwrap()
        .unwrap();
    assert!(String::from_utf8_lossy(&snapshot.bytes).contains("PRIVATE REFERENCE"));
    let reopened = Store::open(store.database_path().unwrap()).unwrap();
    let restarted_page = crate::history::restart_history_page(&reopened, "thread-1", 20).unwrap();
    let projection = restarted_page
        .entries
        .iter()
        .find(|entry| {
            matches!(entry,
                crate::history::HistoryEntry::Tool {
                    call_id,
                    status: crate::types::ToolCallStatus::Success,
                    input: Some(crate::types::ToolCardInputProjection::Skill { .. }),
                    result: Some(crate::types::ToolCardResultProjection::Skill { .. }),
                    ..
                } if call_id == "ref-notes"
            )
        })
        .expect("restarted reference card retains success state");
    assert!(!format!("{projection:?}").contains("PRIVATE REFERENCE"));
    assert!(!format!("{projection:?}").contains("references/notes.md"));
}

#[tokio::test]
async fn issue74_mixed_batch_rejects_persisted_write_without_permission_prompt() {
    let (store, project, project_id) = setup();
    let skills_root = project.path().join(".agents/skills");
    add_skill(&skills_root, "reviewer", "PRIVATE BODY");
    let source = SkillSource::project_approved(project.path())
        .unwrap()
        .unwrap();
    let settings = skills::read_settings(store.conn()).unwrap();
    skills::set_project_settings(
        store.conn(),
        settings.consent_generation,
        &project_id,
        true,
        true,
    )
    .unwrap();
    approve(
        &store,
        &source,
        "source-project",
        Some(&project_id),
        &skills_root,
        true,
    );
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "mixed-write".into(),
                name: "write".into(),
                input_json: r#"{"path":"bad.txt","content":"BAD"}"#.into(),
            },
            ProviderEvent::ToolUse {
                id: "mixed-load".into(),
                name: "load_skill".into(),
                input_json: r#"{"name":"reviewer"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ]);
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let hook = FixedPermissionHook {
        calls: calls.clone(),
        decision: PermissionDecision::Once,
    };
    let run = run_thread_task_with_images_and_reasoning(
        &store,
        &provider,
        &tools,
        "thread-1",
        "review and write",
        "System",
        CancellationToken::new(),
        &hook,
        |_| Ok(()),
        PersistenceActorConfig::default(),
        None,
        None,
        None,
        Vec::new(),
    )
    .await
    .unwrap();
    assert!(
        !run.failed,
        "{:?}",
        run.events
            .iter()
            .filter_map(|event| match event {
                ConversationEvent::Error { error, .. } => Some(error.to_string()),
                _ => None,
            })
            .collect::<Vec<_>>()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(!project.path().join("bad.txt").exists());
    let write = tool_calls::find_state(store.conn(), "mixed-write")
        .unwrap()
        .unwrap();
    assert_eq!(write.status, "rejected");
    assert_eq!(
        tool_calls::find_state(store.conn(), "mixed-load")
            .unwrap()
            .unwrap()
            .status,
        "success"
    );
    assert!(
        provider.requests()[1].messages[0]
            .content
            .contains("PRIVATE BODY")
    );
}
