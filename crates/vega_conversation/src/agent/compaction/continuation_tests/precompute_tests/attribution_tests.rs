use super::*;
use crate::agent::compaction::precompute::PrecomputeUsageAttribution;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Default)]
struct UsageObserver(AtomicBool);

impl vega_runtime::RuntimeAutomationHook for UsageObserver {
    fn context(&self) -> serde_json::Value {
        serde_json::json!({})
    }
    fn call(
        &self,
        _: String,
        _: serde_json::Value,
        _: CancellationToken,
    ) -> futures::future::BoxFuture<'static, Result<serde_json::Value, VegaError>> {
        Box::pin(async { Err(VegaError::Cancelled) })
    }
    fn missing_usage(&self) {
        self.0.store(true, Ordering::Release);
    }
    fn provider_started(&self) {
        panic!("background accounting must not report foreground provider dispatch");
    }
    fn provider_usage(&self) {
        panic!("background accounting must not report foreground provider completion");
    }
}

#[tokio::test]
async fn background_usage_attributes_parent_and_goal_atomically_without_rebilling() -> TestResult {
    for mode in ["success", "unknown", "rollback"] {
        let (_directory, path, store) = source_fixture()?;
        let request = input(&store)?;
        threads::create_standalone(
            store.conn(),
            threads::NewThread {
                id: "parent",
                project_id: "",
                title: "Parent",
                mode: "execute",
                permission_mode: "readonly",
                permission_revision: 0,
                full_access_grant: None,
                model: "model",
                status: "active",
                pinned: false,
                unread: false,
                created_at: 1,
                updated_at: 1,
            },
        )?;
        messages::insert(
            store.conn(),
            &messages::MessageRow {
                id: "parent-run".into(),
                thread_id: "parent".into(),
                seq: 1,
                role: "assistant".into(),
                kind: "text".into(),
                content: String::new(),
                status: "streaming".into(),
                created_at: 1,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )?;
        let claim = crate::types::AutomationRunClaim {
            id: "goal-run".into(),
            thread_id: "parent".into(),
            source: None,
            started: true,
            tasks: Vec::new(),
            permission_fence: None,
            usage_ids: Vec::new(),
            tokens: 0,
            goal_report: None,
            wakeup: None,
        };
        vega_store::automation::insert_run(
            store.conn(),
            "parent",
            "goal-run",
            &serde_json::to_string(&claim)?,
        )?;
        if mode == "rollback" {
            store.conn().execute_batch("CREATE TRIGGER reject_goal_usage BEFORE UPDATE ON automation_runs BEGIN SELECT RAISE(FAIL, 'synthetic goal failure'); END;")?;
        }
        let provider = BackgroundProvider::new(
            if mode == "unknown" {
                "unknown"
            } else {
                "success"
            },
            false,
        );
        let observer = Arc::new(UsageObserver::default());
        let hook = ConversationCompactionHook::new(&provider, path, "thread", "model", None, None)
            .with_precompute_attribution(PrecomputeUsageAttribution {
                parent: Some(("parent".into(), "parent-run".into())),
                claim: Some(claim.clone()),
                observer: Some(observer.clone()),
            });
        let prepared = hook
            .prepare_precompute(request.clone(), CancellationToken::new())
            .await?
            .ok_or("prepared")?;
        let generation = prepared.generation().to_owned();
        let mut worker = prepared.launch()?;
        let result = tokio::time::timeout(Duration::from_secs(10), worker.wait()).await?;
        assert_eq!(result.is_ok(), mode != "rollback", "{mode}: {result:?}");
        assert_eq!(observer.0.load(Ordering::Acquire), mode != "success");
        let (finished, data) =
            vega_store::automation::run(store.conn(), "parent", "goal-run")?.ok_or("claim")?;
        assert!(!finished);
        let saved: crate::types::AutomationRunClaim = serde_json::from_str(&data)?;
        assert_eq!(saved.tokens, if mode == "success" { 110 } else { 0 });
        assert_eq!(saved.usage_ids.len(), usize::from(mode == "success"));
        assert_eq!(count(&store, "token_usage")?, i64::from(mode == "success"));
        assert_eq!(
            count(&store, "context_precompute_stages")?,
            i64::from(mode != "rollback")
        );
        if mode == "success" {
            let attributed: (String, String, bool) = store.conn().query_row("SELECT delegated_parent_thread_id,delegated_parent_message_id,delegated_cache_known FROM token_usage", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
            assert_eq!(attributed, ("parent".into(), "parent-run".into(), false));
            let payload: String = store.conn().query_row(
                "SELECT payload FROM context_precompute_stages",
                [],
                |row| row.get(0),
            )?;
            let reported: Option<accounting::ReportedUsage> = serde_json::from_str(&payload)?;
            let transaction = vega_store::write_coordination::WriteTransaction::new(
                store.conn(),
                vega_store::write_coordination::WriteCategory::Usage,
            )?;
            let id = accounting::record_stage_in_transaction(
                &transaction,
                &generation,
                "thread",
                "model",
                1,
                reported.as_ref(),
            )?
            .ok_or("usage")?;
            crate::automation::record_usage(&transaction, &claim, id, 100, 10)?;
            transaction.commit()?;
            let saved: crate::types::AutomationRunClaim = serde_json::from_str(
                &vega_store::automation::run(store.conn(), "parent", "goal-run")?
                    .ok_or("claim")?
                    .1,
            )?;
            assert_eq!(saved.tokens, 110);
            assert_eq!(saved.usage_ids, [id]);
        }
        if mode != "rollback" {
            let consumed = hook
                .compact_with_replay(
                    request.clone(),
                    Some(vega_runtime::ReplayFrame::new(
                        request.messages.len() + 1,
                        vega_runtime::ReasoningReplayPolicy::Unknown,
                    )),
                    CancellationToken::new(),
                )
                .await
                .map_err(|failure| *failure.error)?;
            assert!(consumed.usages.is_empty());
            assert!(consumed.settled_usage.is_some());
            assert_eq!(count(&store, "token_usage")?, i64::from(mode == "success"));
            assert_eq!(provider.0.requests.lock().unwrap().len(), 1);
        }
    }
    Ok(())
}
