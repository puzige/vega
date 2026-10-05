use super::*;
use crate::types::{SkillCardOutcome, ToolCardResultProjection};

#[tokio::test]
async fn issue87_s14_durable_metadata_survives_real_store_close_without_source_or_replay() {
    let (store, project, project_id) = setup();
    let skills_root = project.path().join(".agents/skills");
    add_skill(&skills_root, "reviewer", "PRIVATE GUIDANCE");
    let assets = skills_root.join("reviewer/assets");
    fs::create_dir(&assets).unwrap();
    fs::write(assets.join("pixel.png"), b"\0\xffPRIVATE ASSET BODY").unwrap();
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
    let authority = skills::read_settings(store.conn()).unwrap();
    let approvals = skills::list_approved_skills(store.conn()).unwrap();
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "s14-load".into(),
                name: "load_skill".into(),
                input_json: r#"{"name":"reviewer"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "s14-asset".into(),
                name: "read_skill_resource".into(),
                input_json: r#"{"name":"reviewer","path":"assets/pixel.png"}"#.into(),
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
        decision: PermissionDecision::Deny { note: None },
    };
    let run = run_thread_task_with_images_and_reasoning(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Inspect one asset",
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
    assert!(!run.failed && !run.interrupted);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(skills::read_settings(store.conn()).unwrap(), authority);
    assert_eq!(
        skills::list_approved_skills(store.conn()).unwrap(),
        approvals
    );
    let state = tool_calls::find_state(store.conn(), "s14-asset")
        .unwrap()
        .unwrap();
    assert_eq!(state.status, "success");
    assert!(!state.input_json.contains("assets/pixel.png"));
    assert!(state.input_json.contains("path_sha256"));
    let audit = crate::types::ApprovalAudit::from_json(state.approval.as_deref().unwrap()).unwrap();
    assert_eq!(audit.source, crate::types::ApprovalSource::ReadonlyTool);
    assert_eq!(audit.decision, crate::types::Approval::Once);
    let output = state.output_text.as_deref().unwrap();
    assert!(output.starts_with("[Lower-trust Skill asset metadata]\n"));
    assert!(!output.contains("PRIVATE ASSET BODY"));
    let snapshot = skills::load_recoverable_snapshot(store.conn(), &run.assistant_message_id)
        .unwrap()
        .unwrap();
    let payload: serde_json::Value = serde_json::from_slice(&snapshot.bytes).unwrap();
    assert_eq!(payload["version"], 2);
    assert_eq!(payload["assets"][0]["size_bytes"], 20);
    assert!(!String::from_utf8_lossy(&snapshot.bytes).contains("PRIVATE ASSET BODY"));
    let original = vega_store::messages::page_before(
        store.conn(),
        "thread-1",
        vega_store::messages::PageCursor::Head,
        20,
    )
    .unwrap()
    .rows;
    assert!(
        original
            .iter()
            .any(|row| row.role == "user" && row.content == "Inspect one asset")
    );
    let requests = provider.requests();
    assert_eq!(requests.len(), 3);
    assert!(
        requests.iter().all(|request| request
            .messages
            .iter()
            .all(|message| message.images.is_empty()
                && !message.content.contains("PRIVATE ASSET BODY")))
    );
    let database = store.database_path().unwrap().to_path_buf();
    fs::remove_dir_all(&skills_root).unwrap();
    drop(store);
    let reopened = Store::open(&database).unwrap();
    let page = crate::history::restart_history_page(&reopened, "thread-1", 20).unwrap();
    let entry = page.entries.iter().find(|entry| matches!(entry, crate::history::HistoryEntry::Tool { call_id, status:crate::types::ToolCallStatus::Success, result:Some(ToolCardResultProjection::Skill { outcome:SkillCardOutcome::AssetMetadata { size_bytes:20 }, .. }), .. } if call_id == "s14-asset")).unwrap();
    assert!(!format!("{entry:?}").contains("assets/pixel.png"));
    assert!(!format!("{entry:?}").contains("PRIVATE ASSET BODY"));
    let recovery =
        crate::agent::recover_skill_run(&reopened, &run.assistant_message_id, "thread-1")
            .unwrap()
            .unwrap();
    assert_eq!(recovery.activations.len(), 1);
    assert_eq!(
        vega_store::messages::page_before(
            reopened.conn(),
            "thread-1",
            vega_store::messages::PageCursor::Head,
            20
        )
        .unwrap()
        .rows,
        original
    );
    assert_eq!(
        skills::load_recoverable_snapshot(reopened.conn(), &run.assistant_message_id)
            .unwrap()
            .unwrap()
            .bytes,
        snapshot.bytes
    );
    assert_eq!(provider.requests().len(), 3);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn issue87_s14_store_revocation_keeps_metadata_unobserved_and_audits_content_free() {
    let (store, project, project_id) = setup();
    let root = project.path().join(".agents/skills");
    add_skill(&root, "reviewer", "PRIVATE GUIDANCE");
    let assets = root.join("reviewer/assets");
    fs::create_dir(&assets).unwrap();
    fs::write(assets.join("safe"), b"PRIVATE ASSET BODY").unwrap();
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
        &root,
        true,
    );
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "s14-load-revoke".into(),
                name: "load_skill".into(),
                input_json: r#"{"name":"reviewer"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "s14-never".into(),
                name: "read_skill_resource".into(),
                input_json: r#"{"name":"reviewer","path":"assets/safe"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
    ]);
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let hook = FixedPermissionHook {
        calls: calls.clone(),
        decision: PermissionDecision::Deny { note: None },
    };
    let database = store.database_path().unwrap().to_path_buf();
    let changed_project = project_id.clone();
    let run = run_thread_task_with_images_and_reasoning(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Inspect one asset",
        "System",
        CancellationToken::new(),
        &hook,
        move |event| {
            if matches!(event, ConversationEvent::SkillActivated { .. }) {
                let changed = Store::open(&database).unwrap();
                let settings = skills::read_settings(changed.conn()).unwrap();
                skills::set_project_settings(
                    changed.conn(),
                    settings.consent_generation,
                    &changed_project,
                    false,
                    false,
                )
                .unwrap();
            }
            Ok(())
        },
        PersistenceActorConfig::default(),
        None,
        None,
        None,
        Vec::new(),
    )
    .await
    .unwrap();
    assert!(run.interrupted && !run.failed);
    assert_eq!(provider.requests().len(), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(
        tool_calls::find_state(store.conn(), "s14-never")
            .unwrap()
            .is_none()
    );
    let audits = skills::list_activation_audits(store.conn(), "thread-1").unwrap();
    assert_eq!(audits.len(), 2);
    assert_eq!(audits[1].status, "revoked");
    let text = format!("{audits:?}");
    assert!(
        !text.contains("PRIVATE GUIDANCE")
            && !text.contains("PRIVATE ASSET BODY")
            && !text.contains("assets/safe")
            && !text.contains(project.path().to_str().unwrap())
    );
}
