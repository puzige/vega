use super::*;
use crate::agent::{PermissionQueue, PersistenceActorConfig};
use crate::types::{
    Approval, ApprovalAudit, ApprovalSource, AutomationCommand, AutomationKind, AutomationReason,
    AutomationStatus,
};

#[derive(Default)]
struct ExitGate {
    released: Mutex<bool>,
    changed: std::sync::Condvar,
    entered: tokio::sync::Notify,
    exited: AtomicBool,
}

impl ExitGate {
    fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.changed.notify_all();
    }
}

struct ReleaseProviders {
    background: BackgroundProvider,
    exit: Arc<ExitGate>,
}

impl Drop for ReleaseProviders {
    fn drop(&mut self) {
        self.background.0.proceed.notify_one();
        self.exit.release();
    }
}

struct EntryProvider {
    background: BackgroundProvider,
    exit: Arc<ExitGate>,
}

struct BackgroundWithExitGate {
    background: BackgroundProvider,
    exit: Arc<ExitGate>,
}

impl Provider for BackgroundWithExitGate {
    fn chat_stream(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        let exit = self.exit.clone();
        drop(tokio::task::spawn_blocking(move || {
            let released = exit.released.lock().unwrap();
            exit.entered.notify_one();
            let _released = exit
                .changed
                .wait_while(released, |released| !*released)
                .unwrap();
            exit.exited.store(true, Ordering::Release);
        }));
        self.background.chat_stream(request, cancel)
    }
}

impl Provider for EntryProvider {
    fn compaction_provider_factory(&self) -> Option<vega_runtime::CompactionProviderFactory> {
        let background = self.background.clone();
        let exit = self.exit.clone();
        Some(Box::new(move || {
            Ok(Arc::new(BackgroundWithExitGate { background, exit }))
        }))
    }

    fn chat_stream(
        &self,
        request: ChatRequest,
        _cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        assert!(!request.tools.is_empty());
        let budget = ContextBudget::new(532_000, 32_000, true).unwrap();
        let estimate =
            vega_runtime::estimate_wire_context(&request.messages, &request.tools).unwrap();
        assert!(estimate.input_tokens >= budget.precompute_tokens().unwrap());
        assert!(estimate.input_tokens < budget.trigger_tokens().unwrap());
        let background = self.background.clone();
        Box::pin(async move {
            tokio::time::timeout(Duration::from_secs(10), background.0.started.notified())
                .await
                .map_err(|_| VegaError::Cancelled)?;
            let stream: vega_runtime::EventStream = Box::pin(futures::stream::iter([
                Ok(ProviderEvent::Usage {
                    input: 7,
                    output: 3,
                    cache_read: None,
                    cache_write: None,
                }),
                Ok(ProviderEvent::TextDelta("Foreground complete.".into())),
                Ok(ProviderEvent::Done {
                    stop_reason: StopReason::End,
                }),
            ]));
            Ok(stream)
        })
    }
}

#[tokio::test]
async fn automation_entry_waits_for_background_physical_exit_before_sealing_budget() -> TestResult {
    verify_entry_sealing("success").await
}

#[tokio::test]
async fn automation_entry_keeps_unknown_background_usage_visible_after_sealing() -> TestResult {
    verify_entry_sealing("unknown").await
}

#[tokio::test]
async fn automation_entry_keeps_rolled_back_background_usage_visible_after_sealing() -> TestResult {
    verify_entry_sealing("rollback").await
}

async fn verify_entry_sealing(mode: &'static str) -> TestResult {
    let (directory, _path, store) = source_fixture()?;
    let _request = input(&store)?;
    let approval = ApprovalAudit {
        decision: Approval::Once,
        note: None,
        source: ApprovalSource::ReadonlyTool,
        danger: None,
    }
    .to_json()?;
    for call in ["call-1", "call-2"] {
        assert_eq!(
            tool_calls::update(store.conn(), call, "success", Some(&approval), None, None)?,
            1
        );
    }
    let thread = ulid::Ulid::generate().to_string();
    threads::create_standalone(
        store.conn(),
        threads::NewThread {
            id: &thread,
            project_id: "",
            title: "Automation entry",
            mode: "execute",
            permission_mode: crate::types::PermissionMode::ReadOnly.as_str(),
            permission_revision: 0,
            full_access_grant: None,
            reasoning_preferences: None,
            model: "model",
            status: "active",
            pinned: false,
            unread: false,
            created_at: 1,
            updated_at: 1,
        },
    )?;
    let transaction = store.immediate_transaction()?;
    transaction.execute(
        "UPDATE messages SET thread_id=?1 WHERE thread_id='thread'",
        [&thread],
    )?;
    transaction.execute(
        "UPDATE tool_calls SET thread_id=?1 WHERE thread_id='thread'",
        [&thread],
    )?;
    transaction.execute("DELETE FROM threads WHERE id='thread'", [])?;
    transaction.commit()?;
    store.conn().execute(
        "UPDATE messages SET content=?1 WHERE id='owner'",
        ["Implementation detail. ".repeat(54_000)],
    )?;
    vega_store::context_compaction::save_model_policy(
        store.conn(),
        &vega_store::context_compaction::ModelContextPolicy {
            provider: "development".into(),
            model: "model".into(),
            input_limit: Some(500_000),
            output_reserve: Some(32_000),
            automatic_compaction: true,
            updated_at: 1,
        },
    )?;
    crate::automation::command(
        &store,
        &thread,
        &AutomationCommand::Create {
            kind: AutomationKind::Goal,
            instruction: "Continue the implementation.".into(),
            token_budget: Some(1_000),
            interval_seconds: None,
        },
        crate::automation::now_ms(),
    )?;
    if mode == "rollback" {
        store.conn().execute_batch(
            "CREATE TRIGGER reject_background_goal_usage BEFORE UPDATE ON automation_runs
             WHEN json_extract(NEW.data, '$.tokens') > json_extract(OLD.data, '$.tokens') + 10
             BEGIN SELECT RAISE(FAIL, 'synthetic background goal failure'); END;",
        )?;
    }
    let registry = crate::types::ContextPrecomputeRegistry::default();
    let admission = crate::thread_access::acquire(&store, &thread)?;
    let lease = registry
        .bind(store.database_identity(), &thread, &admission)?
        .ok_or("lease")?;
    let background = BackgroundProvider::new(
        if mode == "unknown" {
            "unknown"
        } else {
            "success"
        },
        true,
    );
    let exit = Arc::new(ExitGate::default());
    let release = ReleaseProviders {
        background: background.clone(),
        exit: exit.clone(),
    };
    let provider = EntryProvider {
        background: background.clone(),
        exit: exit.clone(),
    };
    let tools = vega_tools::Tools::new(directory.0.clone())?;
    let permissions = PermissionQueue::new();
    let terminal = tokio::sync::Notify::new();
    let run = crate::agent::run_thread_task_with_images_reasoning_and_mcp(
        &store,
        &provider,
        &tools,
        &thread,
        "Continue the implementation.",
        "system",
        CancellationToken::new(),
        &permissions,
        |event| {
            if matches!(
                event,
                crate::types::ConversationEvent::MessageFinished { .. }
            ) {
                terminal.notify_one();
            }
            Ok(())
        },
        PersistenceActorConfig {
            admission: Some(admission.clone()),
            precompute_lease: Some(lease.clone()),
            owner_credential_reader: Some(Arc::new(|| Ok(Vec::new()))),
            turn_limit: 1,
            ..Default::default()
        },
        None,
        None,
        Some(FrozenReasoning {
            compatibility: None,
            provider: "development".into(),
            model: "model".into(),
            protocol: vega_runtime::ReasoningProtocol::ZhipuChatCompletions,
            replay_policy: vega_runtime::ReasoningReplayPolicy::GlmInterleaved,
            choice: ReasoningChoice::ProviderDefault,
            supports_disabled: false,
            disabled_wire: None,
            declared_efforts: Vec::new(),
        }),
        Vec::new(),
        Vec::new(),
    );
    tokio::pin!(run);
    tokio::select! {
        result = &mut run => panic!("entry finished before background release: {result:?}"),
        result = tokio::time::timeout(Duration::from_secs(10), terminal.notified()) => result?,
    }
    tokio::time::timeout(Duration::from_secs(10), exit.entered.notified()).await?;
    let task = registry
        .pending(store.database_identity(), &thread, &admission)?
        .ok_or("background pending")?;
    let generation = task.generation().to_owned();
    let claim: crate::types::AutomationRunClaim = serde_json::from_str(
        &vega_store::automation::active_run(store.conn(), &thread)?.ok_or("active claim")?,
    )?;
    assert_eq!(claim.tokens, 10);
    assert_eq!(claim.usage_ids.len(), 1);
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut run)
            .await
            .is_err(),
        "{mode}: blocked provider must prevent sealing"
    );
    assert!(
        !vega_store::automation::run(store.conn(), &thread, &claim.id)?
            .ok_or("claim")?
            .0
    );
    assert_eq!(crate::automation::load(&store, &thread)?[0].used_tokens, 0);
    assert_eq!(count(&store, "context_precompute_stages")?, 0);
    background.0.proceed.notify_one();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let terminal =
                vega_store::run_diagnostics::read_by_run(store.conn(), &thread, &generation)?
                    .iter()
                    .any(|event| {
                        event.event.phase == vega_store::run_diagnostics::DiagnosticPhase::Run
                            && event.event.state
                                != vega_store::run_diagnostics::DiagnosticState::Started
                    });
            if terminal {
                break Ok::<(), Box<dyn std::error::Error>>(());
            }
            tokio::select! {
                result = &mut run => panic!("entry sealed before runtime exit: {result:?}"),
                _ = tokio::time::sleep(Duration::from_millis(10)) => {}
            }
        }
    })
    .await??;
    assert!(!exit.exited.load(Ordering::Acquire));
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut run)
            .await
            .is_err(),
        "{mode}: settled accounting is not physical exit"
    );
    let (finished, data) =
        vega_store::automation::run(store.conn(), &thread, &claim.id)?.ok_or("unsealed claim")?;
    assert!(!finished);
    let settled: crate::types::AutomationRunClaim = serde_json::from_str(&data)?;
    let expected_tokens = if mode == "success" { 120 } else { 10 };
    assert_eq!(settled.tokens, expected_tokens);
    assert_eq!(
        settled.usage_ids.len(),
        if mode == "success" { 2 } else { 1 }
    );
    assert_eq!(
        count(&store, "token_usage")?,
        if mode == "success" { 2 } else { 1 }
    );
    assert_eq!(
        count(&store, "context_precompute_stages")?,
        i64::from(mode != "rollback")
    );
    assert_eq!(crate::automation::load(&store, &thread)?[0].used_tokens, 0);
    exit.release();
    let outcome = tokio::time::timeout(Duration::from_secs(10), &mut run).await??;
    assert!(!outcome.failed);
    assert!(!outcome.interrupted);
    assert!(exit.exited.load(Ordering::Acquire));
    let background_result = tokio::time::timeout(Duration::from_secs(10), task.wait()).await?;
    assert_eq!(
        background_result.is_ok(),
        mode != "rollback",
        "{mode}: {background_result:?}"
    );
    assert!(
        registry
            .pending(store.database_identity(), &thread, &admission)?
            .is_none()
    );
    assert_eq!(count(&store, "context_checkpoints")?, 0);
    let sealed =
        vega_store::automation::run(store.conn(), &thread, &claim.id)?.ok_or("sealed claim")?;
    assert!(sealed.0);
    assert_eq!(sealed.1, data);
    let tasks = crate::automation::load(&store, &thread)?;
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].used_tokens, expected_tokens);
    assert_eq!(tasks[0].usage_complete, mode == "success");
    assert_eq!(
        tasks[0].status,
        if mode == "success" {
            AutomationStatus::Active
        } else {
            AutomationStatus::Paused
        }
    );
    assert_eq!(
        tasks[0].system_reason,
        if mode == "success" {
            None
        } else {
            Some(AutomationReason::UsageUnverified)
        }
    );
    let ready = ready::recover(store.conn(), &thread, now_ms())?;
    if mode == "rollback" {
        assert!(ready.is_none());
    } else {
        let ready = ready.ok_or("ready must survive sealing")?;
        assert_eq!(ready.generation, generation);
        assert_eq!(ready.usage_complete, mode == "success");
    }
    for _ in 0..2 {
        tokio::time::timeout(Duration::from_secs(10), lease.wait_for_pending_exit()).await??;
        crate::automation::finish(
            &store,
            &claim,
            false,
            false,
            9_999,
            crate::automation::now_ms(),
        )?;
        assert_eq!(crate::automation::load(&store, &thread)?, tasks);
        assert_eq!(
            vega_store::automation::run(store.conn(), &thread, &claim.id)?,
            Some(sealed.clone())
        );
        assert_eq!(
            count(&store, "token_usage")?,
            if mode == "success" { 2 } else { 1 }
        );
    }
    assert_eq!(background.0.requests.lock().unwrap().len(), 1);
    drop(release);
    Ok(())
}
