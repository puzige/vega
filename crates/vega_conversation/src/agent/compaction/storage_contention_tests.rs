use super::*;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use vega_store::context_compaction::{self, ContextCheckpoint, ContextSource, ModelContextPolicy};
use vega_store::{messages, run_diagnostics, threads, tool_calls};

const THREAD: &str = "history-thread";
const STREAM_THREAD: &str = "stream-thread";
const ASSISTANT: &str = "history-assistant";
const LIVE: &str = "live-assistant";
const STREAM: &str = "stream-assistant";
const MODEL: &str = "storage-model";
const PROVIDER: &str = "storage-provider";
const SYSTEM: &str = "Continue the user's task.";
const SHELL_OUTPUT: &str = "workspace scan complete";
const WAIT: Duration = Duration::from_secs(10);

struct TemporaryDirectory(PathBuf);

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let result = std::fs::remove_dir_all(&self.0);
        if !std::thread::panicking() {
            result.expect("remove owned SQLite fixture directory");
        }
    }
}

struct Database {
    store: Store,
    directory: TemporaryDirectory,
}

impl Database {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "vega-storage-contention-{}",
            ulid::Ulid::generate()
        ));
        std::fs::create_dir(&path).unwrap();
        let directory = TemporaryDirectory(path);
        let store = Store::open(directory.0.join("history.sqlite")).unwrap();
        store.migrate().unwrap();
        store
            .conn()
            .busy_timeout(Duration::from_millis(25))
            .unwrap();
        let journal: String = store
            .conn()
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(journal, "wal");
        for id in [THREAD, STREAM_THREAD] {
            threads::create_standalone(
                store.conn(),
                threads::NewThread {
                    id,
                    project_id: "",
                    title: "Storage fixture",
                    mode: "ask",
                    permission_mode: "confirm",
                    model: MODEL,
                    status: "idle",
                    pinned: false,
                    unread: false,
                    created_at: 1,
                    updated_at: 1,
                },
            )
            .unwrap();
        }
        for row in [
            message(
                THREAD,
                "history-user",
                1,
                "user",
                "Inspect the workspace.",
                "done",
            ),
            message(
                THREAD,
                ASSISTANT,
                2,
                "assistant",
                "Inspected committed history.",
                "failed",
            ),
            message(
                THREAD,
                "latest-user",
                3,
                "user",
                "Continue the inspection.",
                "done",
            ),
            message(THREAD, LIVE, 4, "assistant", "", "streaming"),
            message(
                STREAM_THREAD,
                STREAM,
                1,
                "assistant",
                "initial stream",
                "streaming",
            ),
        ] {
            messages::insert(store.conn(), &row).unwrap();
        }
        tool_calls::insert(
            store.conn(),
            tool_calls::NewToolCall {
                id: "shell-call",
                thread_id: THREAD,
                message_id: ASSISTANT,
                seq: 1,
                tool: "bash",
                input_json: r#"{"command":"inspect workspace"}"#,
                status: "running",
                created_at: 1,
            },
        )
        .unwrap();
        tool_calls::finish(
            store.conn(),
            tool_calls::FinishToolCall {
                id: "shell-call",
                status: "success",
                output_text: SHELL_OUTPUT,
                exit_code: Some(0),
                duration_ms: Some(1),
                finished_at: 2,
            },
        )
        .unwrap();
        context_compaction::save_model_policy(store.conn(), &policy(100_000, 1_000, true, 7))
            .unwrap();
        add_failure(
            store.conn(),
            ASSISTANT,
            run_diagnostics::DiagnosticFailureCode::ContextOverLimit,
        );
        add_status(store.conn(), "known_unpriced", "no_compactable_prefix", 1);
        Self { store, directory }
    }

    fn path(&self) -> PathBuf {
        self.directory.0.join("history.sqlite")
    }

    fn connection(&self) -> Store {
        let store = Store::open(self.path()).unwrap();
        store
            .conn()
            .busy_timeout(Duration::from_millis(25))
            .unwrap();
        store
    }

    fn source(&self) -> ContextSource {
        context_compaction::load_source(self.store.conn(), THREAD).unwrap()
    }

    fn checkpoint(&self) -> Option<ContextCheckpoint> {
        context_compaction::latest_checkpoint(self.store.conn(), THREAD, MODEL).unwrap()
    }

    fn projection(&self) -> crate::types::ContextProjection {
        read_context_projection_with_provider(&self.store, THREAD, MODEL, Some(PROVIDER), SYSTEM)
            .unwrap()
    }

    fn stream(&self) -> messages::MessageRow {
        messages::find(self.store.conn(), STREAM).unwrap().unwrap()
    }
}

fn message(
    thread: &str,
    id: &str,
    seq: i64,
    role: &str,
    content: &str,
    status: &str,
) -> messages::MessageRow {
    messages::MessageRow {
        id: id.into(),
        thread_id: thread.into(),
        seq,
        role: role.into(),
        kind: "text".into(),
        content: content.into(),
        status: status.into(),
        created_at: seq,
        plan_status: None,
        plan_review_note: None,
        plan_reviewed_at: None,
    }
}

fn policy(input: u64, output: u64, automatic: bool, updated_at: i64) -> ModelContextPolicy {
    ModelContextPolicy {
        provider: PROVIDER.into(),
        model: MODEL.into(),
        input_limit: Some(input),
        output_reserve: Some(output),
        automatic_compaction: automatic,
        updated_at,
    }
}

fn add_failure(
    conn: &vega_store::Connection,
    owner: &str,
    code: run_diagnostics::DiagnosticFailureCode,
) {
    let mut event = run_diagnostics::NewDiagnosticEvent {
        thread_id: THREAD.into(),
        run_id: owner.into(),
        attempt_id: format!("{owner}-attempt"),
        parent_attempt_id: None,
        tool_call_id: None,
        phase: run_diagnostics::DiagnosticPhase::Run,
        state: run_diagnostics::DiagnosticState::Started,
        failure_code: None,
        occurred_at: 1,
        duration_ms: None,
        metrics: Default::default(),
    };
    run_diagnostics::insert(conn, &event).unwrap();
    event.state = run_diagnostics::DiagnosticState::Failed;
    event.failure_code = Some(code);
    event.occurred_at = 2;
    event.duration_ms = Some(1);
    run_diagnostics::insert(conn, &event).unwrap();
}

fn add_status(conn: &vega_store::Connection, usage: &str, failure: &str, generation: u64) {
    context_compaction::insert_status(
        conn,
        NewContextCompactionStatus {
            thread_id: THREAD,
            model: MODEL,
            operation_key: &format!("{ASSISTANT}:context-generation-{generation}:attempt-status"),
            generation,
            phase: "failed",
            usage_state: usage,
            failure: Some(failure),
            source_version: 4,
            estimated_tokens: 500,
            input_budget: 100_000,
            target_tokens: 60_000,
            created_at: generation as i64,
        },
    )
    .unwrap();
}

fn install_checkpoint(database: &Database) -> ContextCheckpoint {
    let source = database.source();
    let result = context_compaction::install_checkpoint(
        database.store.conn(),
        &NewContextCheckpoint {
            thread_id: THREAD.into(),
            model: MODEL.into(),
            source_version: source.source_version,
            covered_through_seq: 1,
            covered_assistant_id: None,
            covered_assistant_text_bytes: 0,
            covered_assistant_tool_seq: 0,
            source_fingerprint: source.fingerprint,
            summary: "Previously inspected the workspace.".into(),
            estimator_version: CONTEXT_ESTIMATOR_VERSION.into(),
            expected_previous_id: None,
            created_at: 1,
        },
    )
    .unwrap();
    match result {
        ContextCheckpointInstall::Applied(checkpoint) => checkpoint,
        other => panic!("expected a real installed checkpoint, got {other:?}"),
    }
}

#[derive(Default)]
struct SummaryProvider {
    requests: Mutex<Vec<ChatRequest>>,
    unavailable: bool,
}

impl Provider for SummaryProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        _: CancellationToken,
    ) -> futures::future::BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        self.requests.lock().unwrap().push(request);
        let unavailable = self.unavailable;
        Box::pin(async move {
            if unavailable {
                return Err(VegaError::Provider {
                    status: Some(503),
                    message: "summary boundary unavailable".into(),
                    retryable: false,
                });
            }
            let events: vega_runtime::EventStream = Box::pin(futures::stream::iter([
                Ok(ProviderEvent::TextDelta(
                    "Continue the workspace inspection.".into(),
                )),
                Ok(ProviderEvent::Usage {
                    input: 100,
                    output: 10,
                    cache_read: None,
                    cache_write: None,
                }),
                Ok(ProviderEvent::Done {
                    stop_reason: StopReason::End,
                }),
            ]));
            Ok(events)
        })
    }
}

fn hook<'a>(database: &Database, provider: &'a SummaryProvider) -> ConversationCompactionHook<'a> {
    ConversationCompactionHook::new(provider, database.path(), THREAD, MODEL, None, None)
}

fn automatic_request(database: &Database) -> ContextCompactionRequest {
    let (source, checkpoint) =
        context_compaction::load_source_and_latest_checkpoint(database.store.conn(), THREAD, MODEL)
            .unwrap();
    let messages = super::super::pipeline::primary_history_from_context_source_with_checkpoint(
        &source,
        checkpoint.as_ref(),
        LIVE,
    )
    .unwrap();
    let budget = ContextBudget::new(101_000, 1_000, true).unwrap();
    ContextCompactionRequest {
        estimate: estimate_chat_context(SYSTEM, &messages, &[]).unwrap(),
        system_prompt: SYSTEM.into(),
        messages,
        tools: Vec::new(),
        budget,
        target_tokens: budget.target_tokens().unwrap(),
        source_version: source.source_version,
        source_fingerprint: Some(source.fingerprint),
        require_source_fence: false,
        source_owner_id: Some(LIVE.into()),
    }
}

async fn manual(
    database: &Database,
    provider: &SummaryProvider,
    reader: vega_runtime::CredentialReader,
) -> Result<ContextCompactionResult, ContextCompactionFailure> {
    tokio::time::timeout(
        WAIT,
        compact_thread_manually_with_credential_reader(
            &database.store,
            provider,
            THREAD,
            MODEL,
            SYSTEM,
            Vec::new(),
            ContextBudget::new(101_000, 1_000, true).unwrap(),
            CancellationToken::new(),
            None,
            None,
            Some(reader),
        ),
    )
    .await
    .expect("bounded manual compaction")
}

fn guard(source: &ContextSource, predecessor: Option<i64>) -> SummarySourceGuard {
    SummarySourceGuard {
        version: source.source_version,
        fingerprint: source.fingerprint.clone(),
        predecessor,
    }
}

fn assert_source_changed(error: &VegaError) {
    assert!(
        matches!(
            error,
            VegaError::Context(ContextRuntimeError::SourceChanged)
        ),
        "{error:?}"
    );
}

fn assert_provider_unavailable(result: Result<ContextCompactionResult, ContextCompactionFailure>) {
    let failure = result.expect_err("summary provider must return its typed failure");
    assert!(
        matches!(
            failure.error.as_ref(),
            VegaError::Provider {
                status: Some(503),
                retryable: false,
                ..
            }
        ),
        "{failure:?}"
    );
    assert!(failure.usages.is_empty());
}

#[test]
fn projection_reads_committed_state_while_a_writer_is_active_and_refreshes_after_commit() {
    let database = Database::new();
    let before = database.projection();
    let source = database.source();
    assert_eq!(before.source_version, 4);
    assert_eq!(before.source_fingerprint, source.fingerprint);
    assert_eq!(before.run_mode, crate::types::ThreadMode::Ask);
    assert_eq!(
        before.settings,
        Some(crate::types::ContextSettings {
            thread_id: THREAD.into(),
            model: MODEL.into(),
            context_limit: Some(101_000),
            output_reserve: 1_000,
            automatic_compaction: true,
            updated_at: 7,
        })
    );
    assert_eq!(before.model_input_limit, Some(100_000));
    assert!(before.estimated_tokens.is_some_and(|tokens| tokens > 0));
    assert!(before.compactable);
    assert!(!before.unknown_usage);
    assert_eq!(before.last_status_message_id.as_deref(), Some(ASSISTANT));
    assert_eq!(
        before.last_status.as_ref().unwrap().failure,
        Some(crate::types::ContextCompactionFailureCode::NoCompactablePrefix)
    );
    assert_eq!(
        before.run_failures,
        vec![(
            ASSISTANT.into(),
            crate::types::RunFailureKind::ContextOverLimit
        )]
    );

    let writer = database.connection();
    let transaction = writer.immediate_transaction().unwrap();
    messages::finish(
        &transaction,
        ASSISTANT,
        &"new committed inspection detail ".repeat(512),
        "failed",
    )
    .unwrap();
    messages::insert(
        &transaction,
        &message(
            THREAD,
            "new-assistant",
            5,
            "assistant",
            "new failed response",
            "failed",
        ),
    )
    .unwrap();
    context_compaction::save_model_policy(&transaction, &policy(200_000, 2_000, false, 9)).unwrap();
    add_status(&transaction, "unknown", "source_changed", 2);
    add_failure(
        &transaction,
        "new-assistant",
        run_diagnostics::DiagnosticFailureCode::ReasoningLimit,
    );
    let during = read_context_projection_with_provider(
        &database.store,
        THREAD,
        MODEL,
        Some(PROVIDER),
        SYSTEM,
    );
    transaction.commit().unwrap();
    let after = database.projection();
    assert_eq!(
        during.expect("projection must not reserve the SQLite writer"),
        before
    );
    assert_eq!(after.source_version, 5);
    assert_eq!(after.source_fingerprint, database.source().fingerprint);
    assert_ne!(after.source_fingerprint, before.source_fingerprint);
    assert!(after.estimated_tokens > before.estimated_tokens);
    assert_eq!(after.model_input_limit, Some(200_000));
    let settings = after.settings.unwrap();
    assert_eq!(settings.context_limit, Some(202_000));
    assert_eq!(settings.output_reserve, 2_000);
    assert!(!settings.automatic_compaction);
    assert_eq!(settings.updated_at, 9);
    assert!(after.unknown_usage);
    assert_eq!(
        after.last_status.unwrap().failure,
        Some(crate::types::ContextCompactionFailureCode::SourceChanged)
    );
    assert_eq!(
        after.run_failures,
        vec![
            (
                ASSISTANT.into(),
                crate::types::RunFailureKind::ContextOverLimit
            ),
            (
                "new-assistant".into(),
                crate::types::RunFailureKind::ReasoningLimit(None)
            ),
        ]
    );
}

#[test]
fn projection_on_a_read_only_store_matches_the_committed_projection_without_writes() {
    let database = Database::new();
    let expected = database.projection();
    let source = database.source();
    let read_only = Store::open_read_only(database.path()).unwrap();
    let total_changes = read_only.conn().total_changes();
    let actual =
        read_context_projection_with_provider(&read_only, THREAD, MODEL, Some(PROVIDER), SYSTEM);
    assert_eq!(
        actual.expect("read-only projection must not request write authority"),
        expected
    );
    assert_eq!(read_only.conn().total_changes(), total_changes);
    assert_eq!(database.source(), source);
    assert!(database.checkpoint().is_none());
}

#[tokio::test]
async fn manual_credential_callback_can_save_a_competing_streaming_snapshot() {
    let database = Database::new();
    let provider = SummaryProvider::default();
    let source = database.source();
    let writer = Mutex::new(database.connection());
    let snapshot = Arc::new(Mutex::new(None));
    let callback_snapshot = snapshot.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let callback_calls = calls.clone();
    let reader = Arc::new(move || {
        if callback_calls.fetch_add(1, Ordering::SeqCst) == 0 {
            let writer = writer.lock().map_err(|_| ())?;
            let result = messages::update_streaming_content(
                writer.conn(),
                STREAM,
                "saved during credential read",
            );
            *callback_snapshot.lock().map_err(|_| ())? = Some(match &result {
                Ok(updated) => Ok(*updated),
                Err(error) => Err(error.sqlite_error().map(|error| error.extended_code)),
            });
            let updated = result.map_err(|_| ())?;
            if updated != 1
                || messages::find(writer.conn(), STREAM)
                    .map_err(|_| ())?
                    .is_none_or(|row| {
                        row.content != "saved during credential read" || row.status != "streaming"
                    })
            {
                return Err(());
            }
        }
        Ok(Vec::new())
    });
    let result = manual(&database, &provider, reader).await;
    assert!(calls.load(Ordering::SeqCst) > 0);
    assert_eq!(
        *snapshot.lock().unwrap(),
        Some(Ok(1)),
        "snapshot write result inside the credential callback: {result:?}"
    );
    assert_eq!(database.stream().content, "saved during credential read");
    let result = result.expect("credential reads must not block another conversation's snapshot");
    assert_eq!(database.source(), source);
    let checkpoint = database
        .checkpoint()
        .expect("successful manual compaction installs a checkpoint");
    assert_eq!(checkpoint.source_fingerprint, source.fingerprint);
    assert_eq!(checkpoint.covered_through_seq, 2);
    assert!(
        result
            .messages
            .iter()
            .any(|message| message.content.contains(&checkpoint.summary))
    );
    assert!(
        result
            .messages
            .iter()
            .any(|message| message.role == ChatRole::User
                && message.content == "Continue the inspection.")
    );
    let requests = provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].tools.is_empty());
    assert!(
        requests[0]
            .messages
            .iter()
            .any(|message| message.content.contains(SHELL_OUTPUT))
    );
}

#[tokio::test]
async fn manual_credential_reader_failure_blocks_the_provider_and_checkpoint() {
    let database = Database::new();
    let provider = SummaryProvider::default();
    let source = database.source();
    let calls = Arc::new(AtomicUsize::new(0));
    let callback_calls = calls.clone();
    let failure = manual(
        &database,
        &provider,
        Arc::new(move || {
            callback_calls.fetch_add(1, Ordering::SeqCst);
            Err(())
        }),
    )
    .await
    .expect_err("unavailable credentials must fail closed");
    assert!(matches!(
        failure.error.as_ref(),
        VegaError::CredentialCheckUnavailable
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(provider.requests.lock().unwrap().is_empty());
    assert!(database.checkpoint().is_none());
    assert_eq!(database.source(), source);
}

#[tokio::test]
async fn manual_shell_credential_exposure_blocks_the_provider_and_checkpoint() {
    let database = Database::new();
    let provider = SummaryProvider::default();
    let source = database.source();
    let calls = Arc::new(AtomicUsize::new(0));
    let callback_calls = calls.clone();
    let failure = manual(
        &database,
        &provider,
        Arc::new(move || {
            callback_calls.fetch_add(1, Ordering::SeqCst);
            Ok(vec![SHELL_OUTPUT.into()])
        }),
    )
    .await
    .expect_err("known credentials in persisted shell output must fail closed");
    assert!(matches!(
        failure.error.as_ref(),
        VegaError::CredentialExposureBlocked
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(provider.requests.lock().unwrap().is_empty());
    assert!(database.checkpoint().is_none());
    assert_eq!(database.source(), source);
}

#[tokio::test]
async fn automatic_capture_and_failure_validation_preserve_the_committed_provider_result_under_a_writer()
 {
    let database = Database::new();
    let checkpoint = install_checkpoint(&database);
    let provider = SummaryProvider {
        unavailable: true,
        ..Default::default()
    };
    let hook = hook(&database, &provider);
    let request = automatic_request(&database);
    assert_provider_unavailable(
        tokio::time::timeout(
            WAIT,
            hook.compact(request.clone(), CancellationToken::new()),
        )
        .await
        .unwrap(),
    );
    let writer = database.connection();
    let transaction = writer.immediate_transaction().unwrap();
    messages::finish(
        &transaction,
        ASSISTANT,
        "Inspected committed history. Uncommitted inspection.",
        "failed",
    )
    .unwrap();
    threads::set_model(&transaction, THREAD, "uncommitted-model", 2).unwrap();
    let result = tokio::time::timeout(WAIT, hook.compact(request, CancellationToken::new())).await;
    transaction.rollback().unwrap();
    assert_provider_unavailable(result.expect("bounded automatic source capture"));
    assert_eq!(database.checkpoint(), Some(checkpoint.clone()));
    let requests = provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0], requests[1]);
    assert_eq!(requests[1].model, MODEL);
    assert!(
        requests[1]
            .messages
            .iter()
            .any(|message| message.content.contains(&checkpoint.summary))
    );
    assert!(
        requests[1]
            .messages
            .iter()
            .any(|message| message.content.contains(SHELL_OUTPUT))
    );
}

#[derive(Clone, Copy, Debug)]
enum SourceMutation {
    Model,
    Fingerprint,
    Version,
}

fn mutate_source(database: &Database, mutation: SourceMutation) {
    match mutation {
        SourceMutation::Model => {
            threads::set_model(database.store.conn(), THREAD, "changed-model", 2).unwrap();
        }
        SourceMutation::Fingerprint => {
            messages::finish(
                database.store.conn(),
                ASSISTANT,
                "Inspected committed history. Changed historical content.",
                "failed",
            )
            .unwrap();
        }
        SourceMutation::Version => {
            messages::insert(
                database.store.conn(),
                &message(
                    THREAD,
                    "unrelated-user",
                    5,
                    "user",
                    "unrelated request",
                    "done",
                ),
            )
            .unwrap();
        }
    }
}

#[tokio::test]
async fn automatic_source_capture_rejects_model_and_unowned_history_changes_before_provider_activity()
 {
    for mutation in [
        SourceMutation::Model,
        SourceMutation::Fingerprint,
        SourceMutation::Version,
    ] {
        let database = Database::new();
        let provider = SummaryProvider::default();
        let request = automatic_request(&database);
        mutate_source(&database, mutation);
        let source = database.source();
        let failure = tokio::time::timeout(
            WAIT,
            hook(&database, &provider).compact(request.clone(), CancellationToken::new()),
        )
        .await
        .unwrap()
        .expect_err("stale automatic authority must fail closed");
        assert_source_changed(&failure.error);
        let writer = database.connection();
        let transaction = writer.immediate_transaction().unwrap();
        messages::update_streaming_content(&transaction, STREAM, "uncommitted unrelated snapshot")
            .unwrap();
        let result = tokio::time::timeout(
            WAIT,
            hook(&database, &provider).compact(request, CancellationToken::new()),
        )
        .await;
        transaction.rollback().unwrap();
        let failure = result
            .expect("bounded stale-source capture under a writer")
            .expect_err("contention must not bypass stale automatic authority");
        assert_source_changed(&failure.error);
        assert!(provider.requests.lock().unwrap().is_empty(), "{mutation:?}");
        assert!(database.checkpoint().is_none());
        assert_eq!(database.source(), source);
    }
}

#[tokio::test]
async fn failed_source_validation_reads_the_committed_source_model_and_predecessor_under_a_writer()
{
    let database = Database::new();
    let checkpoint = install_checkpoint(&database);
    let source = database.source();
    let provider = SummaryProvider::default();
    let hook = hook(&database, &provider);
    tokio::time::timeout(
        WAIT,
        hook.check_failed_source(guard(&source, Some(checkpoint.id))),
    )
    .await
    .unwrap()
    .unwrap();
    let writer = database.connection();
    let transaction = writer.immediate_transaction().unwrap();
    messages::finish(
        &transaction,
        ASSISTANT,
        "Inspected committed history. Uncommitted source.",
        "failed",
    )
    .unwrap();
    threads::set_model(&transaction, THREAD, "uncommitted-model", 2).unwrap();
    transaction
        .execute(
            "DELETE FROM context_checkpoints WHERE id=?1",
            [checkpoint.id],
        )
        .unwrap();
    let result = tokio::time::timeout(
        WAIT,
        hook.check_failed_source(guard(&source, Some(checkpoint.id))),
    )
    .await;
    transaction.rollback().unwrap();
    result
        .expect("bounded failed-source validation")
        .expect("read-only failure validation must not acquire the writer");
    assert_eq!(database.source(), source);
    assert_eq!(database.checkpoint(), Some(checkpoint));
    assert!(provider.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn failed_source_validation_rejects_model_fingerprint_and_predecessor_changes() {
    for mutation in [
        Some(SourceMutation::Model),
        Some(SourceMutation::Fingerprint),
        None,
    ] {
        let database = Database::new();
        let source = database.source();
        let provider = SummaryProvider::default();
        let hook = hook(&database, &provider);
        tokio::time::timeout(WAIT, hook.check_failed_source(guard(&source, None)))
            .await
            .unwrap()
            .unwrap();
        if let Some(mutation) = mutation {
            mutate_source(&database, mutation);
            if matches!(mutation, SourceMutation::Fingerprint) {
                assert_eq!(database.source().source_version, source.source_version);
                assert_ne!(database.source().fingerprint, source.fingerprint);
            }
        } else {
            install_checkpoint(&database);
            assert_eq!(database.source(), source);
        }
        let error = tokio::time::timeout(WAIT, hook.check_failed_source(guard(&source, None)))
            .await
            .unwrap()
            .expect_err("changed source authority must be rejected");
        assert_source_changed(&error);
        let writer = database.connection();
        let transaction = writer.immediate_transaction().unwrap();
        messages::update_streaming_content(&transaction, STREAM, "uncommitted unrelated snapshot")
            .unwrap();
        let result =
            tokio::time::timeout(WAIT, hook.check_failed_source(guard(&source, None))).await;
        transaction.rollback().unwrap();
        let error = result
            .expect("bounded stale-source validation under a writer")
            .expect_err("contention must not bypass a stale source guard");
        assert_source_changed(&error);
        assert!(provider.requests.lock().unwrap().is_empty());
    }
}

#[test]
fn a_genuinely_contended_streaming_snapshot_remains_busy_and_safely_classified() {
    let database = Database::new();
    let writer = database.connection();
    let transaction = writer.immediate_transaction().unwrap();
    threads::rename(&transaction, THREAD, "uncommitted title", 2).unwrap();
    let error =
        messages::update_streaming_content(database.store.conn(), STREAM, "blocked snapshot")
            .expect_err("a real competing writer must still block snapshot persistence");
    let sqlite = error.sqlite_error().expect("SQLite contention error");
    assert_eq!(sqlite.extended_code & 255, 5);
    assert_eq!(sqlite.extended_code, 5);
    let runtime =
        VegaError::Store(error).at_runtime_stage(vega_runtime::RuntimeFailureStage::Snapshot);
    let kind = crate::types::RunFailureKind::from_runtime(&runtime);
    assert_eq!(kind, crate::types::RunFailureKind::RuntimeStorageBusy);
    assert_eq!(
        kind.message(),
        "Conversation storage is busy. Wait for other tasks to finish, then retry."
    );
    assert_eq!(database.stream().content, "initial stream");
    transaction.rollback().unwrap();
    assert_eq!(
        messages::update_streaming_content(database.store.conn(), STREAM, "saved after release")
            .unwrap(),
        1
    );
    assert_eq!(database.stream().content, "saved after release");
}
