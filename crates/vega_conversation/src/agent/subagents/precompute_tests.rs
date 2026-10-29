use super::*;
use crate::agent::ConversationCompactionHook;
use std::sync::atomic::{AtomicBool, Ordering};
use vega_runtime::{
    ChatMessage, ChatRequest, ChatRole, ContextBudget, ContextCompactionRequest,
    ContextCompactionTrigger, ProviderEvent, StopReason,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct TestDatabase(PathBuf);
impl Drop for TestDatabase {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct ResourceGate(Arc<(Mutex<bool>, std::sync::Condvar)>);
impl ResourceGate {
    fn release(&self) {
        let (lock, changed) = &*self.0;
        *lock.lock().unwrap() = true;
        changed.notify_all();
    }
}
impl Drop for ResourceGate {
    fn drop(&mut self) {
        self.release();
    }
}

struct ProviderState {
    blocked: Notify,
    stream_polled: Notify,
    gate: Arc<(Mutex<bool>, std::sync::Condvar)>,
    released: AtomicBool,
    wait: bool,
}
struct ChildSummaryProvider(Arc<ProviderState>);
impl Provider for ChildSummaryProvider {
    fn compaction_provider_factory(&self) -> Option<vega_runtime::CompactionProviderFactory> {
        let state = self.0.clone();
        Some(Box::new(move || Ok(Arc::new(Self(state)))))
    }
    fn chat_stream(
        &self,
        request: ChatRequest,
        _: CancellationToken,
    ) -> BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        assert!(request.tools.is_empty());
        let state = self.0.clone();
        Box::pin(async move {
            if state.wait {
                let resource = state.clone();
                drop(tokio::task::spawn_blocking(move || {
                    let (lock, changed) = &*resource.gate;
                    let released = lock.lock().unwrap();
                    resource.blocked.notify_one();
                    let _released = changed.wait_while(released, |released| !*released).unwrap();
                    resource.released.store(true, Ordering::Release);
                }));
            }
            let usage = Ok(ProviderEvent::Usage {
                input: 100,
                output: 10,
                cache_read: Some(0),
                cache_write: Some(0),
            });
            let stream: vega_runtime::EventStream = if state.wait {
                Box::pin(
                    futures::stream::iter([usage]).chain(futures::stream::poll_fn(move |_| {
                        state.stream_polled.notify_one();
                        std::task::Poll::Pending
                    })),
                )
            } else {
                Box::pin(futures::stream::iter([
                    usage,
                    Ok(ProviderEvent::TextDelta("Continue the child task.".into())),
                    Ok(ProviderEvent::Done {
                        stop_reason: StopReason::End,
                    }),
                ]))
            };
            Ok(stream)
        })
    }
}

fn provider(wait: bool) -> (ChildSummaryProvider, ResourceGate) {
    let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    (
        ChildSummaryProvider(Arc::new(ProviderState {
            blocked: Notify::new(),
            stream_polled: Notify::new(),
            gate: gate.clone(),
            released: AtomicBool::new(false),
            wait,
        })),
        ResourceGate(gate),
    )
}

fn seed(store: &Store, thread: &str) -> TestResult {
    vega_store::threads::create_standalone(
        store.conn(),
        vega_store::threads::NewThread {
            id: thread,
            project_id: "",
            title: "Child",
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
    for (seq, role, content) in [
        (1, "user", "Continue child task.".into()),
        (2, "assistant", "a".repeat(40_000)),
        (3, "user", "Continue.".into()),
        (4, "assistant", "Kept tail.".into()),
    ] {
        messages::insert(
            store.conn(),
            &messages::MessageRow {
                id: format!("{thread}-{seq}"),
                thread_id: thread.into(),
                seq,
                role: role.into(),
                kind: "text".into(),
                content,
                status: "done".into(),
                created_at: seq,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )?;
    }
    Ok(())
}

async fn prepare(
    store: &Store,
    path: &std::path::Path,
    thread: &str,
    provider: &dyn Provider,
    admission: &vega_store::thread_admission::ThreadAdmission,
) -> Result<crate::types::PreparedContextPrecompute, Box<dyn std::error::Error>> {
    let source = vega_store::context_compaction::load_source(store.conn(), thread)?;
    let messages = crate::agent::pipeline::history_from_context_source(&source, "__none__", None)?;
    let budget = ContextBudget::new(200_000, 32_000, true)?;
    let request = ContextCompactionRequest {
        trigger: ContextCompactionTrigger::Threshold,
        system_prompt: "system".into(),
        estimate: vega_runtime::estimate_chat_context("system", &messages, &[])?,
        messages,
        tools: Vec::new(),
        budget,
        target_tokens: budget.target_tokens()?,
        source_version: source.source_version,
        source_fingerprint: Some(source.fingerprint),
        require_source_fence: true,
        source_owner_id: None,
    };
    Ok(
        ConversationCompactionHook::new(provider, path.to_path_buf(), thread, "model", None, None)
            .with_thread_admission(admission.clone())
            .prepare_precompute(request, CancellationToken::new())
            .await?
            .ok_or("prepared")?,
    )
}

#[tokio::test]
async fn child_exit_waits_for_real_runtime_exit_and_retires_only_its_candidate() -> TestResult {
    for cancel_execution in [false, true] {
        let directory = TestDatabase(
            std::env::temp_dir().join(format!("vega-child-compaction-{}", ulid::Ulid::generate())),
        );
        std::fs::create_dir(&directory.0)?;
        let path = directory.0.join("test.sqlite");
        let store = Store::open(&path)?;
        store.migrate()?;
        seed(&store, "child")?;
        seed(&store, "other")?;
        let admission = crate::thread_access::acquire(&store, "child")?;
        let other_admission = crate::thread_access::acquire(&store, "other")?;
        let cancel = CancellationToken::new();
        let (child, lease) =
            ChildPrecompute::begin(&store, path.clone(), "child", &admission, &cancel)?;
        let lease = lease.ok_or("lease")?;
        let (active_provider, release) = provider(true);
        let task = lease
            .try_launch(prepare(&store, &path, "child", &active_provider, &admission).await?)?
            .ok_or("task")?;
        tokio::time::timeout(
            Duration::from_secs(10),
            active_provider.0.blocked.notified(),
        )
        .await?;
        tokio::time::timeout(
            Duration::from_secs(10),
            active_provider.0.stream_polled.notified(),
        )
        .await?;
        let (ready_provider, _unused) = provider(false);
        let mut other = prepare(&store, &path, "other", &ready_provider, &other_admission)
            .await?
            .launch()?;
        tokio::time::timeout(Duration::from_secs(10), other.wait()).await??;
        let other_generation = other.generation().to_owned();
        if cancel_execution {
            cancel.cancel();
            assert!(!lease.is_current()?);
            tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    let sealed: bool = store.conn().query_row(
                        "SELECT EXISTS(SELECT 1 FROM context_precompute_accounting WHERE generation=?1 AND stage_count=1 AND usage_complete=1)",
                        [task.generation()],
                        |row| row.get(0),
                    )?;
                    let diagnostics: i64 = store.conn().query_row(
                        "SELECT COUNT(*) FROM run_diagnostic_events WHERE thread_id='child'",
                        [],
                        |row| row.get(0),
                    )?;
                    if sealed && diagnostics == 4 {
                        break Ok::<(), rusqlite::Error>(());
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }).await??;
            assert!(!active_provider.0.released.load(Ordering::Acquire));
            assert!(
                tokio::time::timeout(Duration::from_millis(100), task.wait_for_exit())
                    .await
                    .is_err()
            );
            let ready: bool = store.conn().query_row(
                "SELECT EXISTS(SELECT 1 FROM context_precompute_slots WHERE thread_id='child')",
                [],
                |row| row.get(0),
            )?;
            assert!(!ready);
        }
        let finish = child.finish(&admission);
        tokio::pin!(finish);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), &mut finish)
                .await
                .is_err()
        );
        assert!(!active_provider.0.released.load(Ordering::Acquire));
        assert!(!lease.is_current()?);
        assert_eq!(cancel.is_cancelled(), cancel_execution);
        release.release();
        tokio::time::timeout(Duration::from_secs(10), finish).await??;
        assert!(matches!(
            task.wait().await.as_ref().map_err(|error| error.as_ref()),
            Err(VegaError::Cancelled)
        ));
        assert!(active_provider.0.released.load(Ordering::Acquire));
        assert!(
            vega_store::context_precompute::ready::recover(
                store.conn(),
                "child",
                crate::threads::now_ms()
            )?
            .is_none()
        );
        assert_eq!(
            vega_store::context_precompute::ready::recover(
                store.conn(),
                "other",
                crate::threads::now_ms()
            )?
            .ok_or("other ready")?
            .generation,
            other_generation
        );
        let bills: i64 = store
            .conn()
            .query_row("SELECT COUNT(*) FROM token_usage", [], |row| row.get(0))?;
        assert_eq!(bills, 2);
        let diagnostics: i64 = store.conn().query_row(
            "SELECT COUNT(*) FROM run_diagnostic_events WHERE thread_id='child'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(diagnostics, 4);
        let (next, lease) = ChildPrecompute::begin(
            &store,
            path.clone(),
            "child",
            &admission,
            &CancellationToken::new(),
        )?;
        let next_task = lease
            .ok_or("next lease")?
            .try_launch(prepare(&store, &path, "child", &ready_provider, &admission).await?)?
            .ok_or("next task")?;
        assert!(
            tokio::time::timeout(Duration::from_secs(10), next_task.wait())
                .await?
                .is_ok()
        );
        assert!(
            vega_store::context_precompute::ready::recover(
                store.conn(),
                "child",
                crate::threads::now_ms()
            )?
            .is_some()
        );
        next.finish(&admission).await?;
        assert!(
            vega_store::context_precompute::ready::recover(
                store.conn(),
                "child",
                crate::threads::now_ms()
            )?
            .is_none()
        );
    }
    Ok(())
}
