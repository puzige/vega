use super::super::diagnostics::{DiagnosticsContext, DiagnosticsWriter};
use super::*;
use crate::database_writer::DatabaseCommand;
use vega_store::run_diagnostics::{DiagnosticFailureCode, DiagnosticMetrics};

const CANARY: &str = "private-body-and-credential-canary";

struct Fixture {
    store: Store,
    path: PathBuf,
    thread: String,
    owner: String,
    admission: crate::types::ThreadAdmission,
    diagnostics: DiagnosticsContext,
}

impl Fixture {
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../.diagnosis/regressions")
            .join(ulid::Ulid::generate().to_string());
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = directory.join("fixture.db");
        let store = Store::open(&path).unwrap();
        store.migrate().unwrap();
        let thread = ulid::Ulid::generate().to_string();
        let owner = ulid::Ulid::generate().to_string();
        store.conn().execute(
            "INSERT INTO threads(id,title,model,status,created_at,updated_at) VALUES(?1,'fixture','fixture-model','active',1,1)",
            [&thread],
        ).unwrap();
        let assistant = ulid::Ulid::generate().to_string();
        for (seq, role, id, content, status) in [
            (
                1,
                "user",
                ulid::Ulid::generate().to_string(),
                CANARY,
                "done",
            ),
            (2, "assistant", assistant.clone(), "prefix", "done"),
            (
                3,
                "user",
                ulid::Ulid::generate().to_string(),
                "next",
                "done",
            ),
            (4, "assistant", owner.clone(), "", "streaming"),
        ] {
            store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES(?1,?2,?3,?4,?5,?6,1)",
                (&id, &thread, seq, role, content, status)).unwrap();
        }
        for seq in 1..=35 {
            let call = ulid::Ulid::generate().to_string();
            store.conn().execute("INSERT INTO tool_calls(id,thread_id,message_id,seq,tool,input_json,output_text,status,created_at,finished_at,text_offset_bytes) VALUES(?1,?2,?3,?4,'read',?5,?6,'success',1,2,6)",
                (&call, &thread, &assistant, seq, format!("{{\"path\":\"{CANARY}\"}}"), CANARY.repeat(200))).unwrap();
        }
        let admission = crate::thread_access::acquire(&store, &thread).unwrap();
        let diagnostics = DiagnosticsContext {
            writer: Arc::new(DiagnosticsWriter::buffered()),
            thread_id: thread.clone(),
            run_id: owner.clone(),
            root_attempt_id: ulid::Ulid::generate().to_string(),
        };
        Self {
            store,
            path,
            thread,
            owner,
            admission,
            diagnostics,
        }
    }

    fn source(&self) -> vega_store::context_compaction::ContextSource {
        let tx = self.store.conn().unchecked_transaction().unwrap();
        let source = load_source_in_transaction(&tx, &self.thread).unwrap();
        tx.commit().unwrap();
        source
    }

    fn request(&self) -> ContextCompactionRequest {
        let source = self.source();
        let mut messages =
            super::super::pipeline::primary_history_from_context_source_with_checkpoint(
                &source,
                None,
                &self.owner,
            )
            .unwrap();
        vega_runtime::computer_use::redact_ephemeral_messages(&mut messages);
        let budget = ContextBudget::new(20_000, 1_000, true).unwrap();
        ContextCompactionRequest {
            trigger: ContextCompactionTrigger::Threshold,
            system_prompt: String::new(),
            estimate: estimate_wire_context(&messages, &[]).unwrap(),
            messages,
            tools: Vec::new(),
            budget,
            target_tokens: budget.target_tokens().unwrap(),
            source_version: source.source_version,
            source_fingerprint: Some(source.fingerprint),
            require_source_fence: false,
            source_owner_id: Some(self.owner.clone()),
        }
    }

    fn mutate_source(&self) {
        self.store.conn().execute("UPDATE messages SET content=content || ' changed' WHERE thread_id=?1 AND role='user' AND seq=1", [&self.thread]).unwrap();
    }

    async fn reasons(&self) -> Vec<String> {
        self.diagnostics
            .writer
            .flush_to(
                &self.store,
                self.admission.clone(),
                &self.thread,
                &self.owner,
            )
            .await
            .unwrap();
        let events =
            vega_store::run_diagnostics::read_by_run(self.store.conn(), &self.thread, &self.owner)
                .unwrap();
        let json = serde_json::to_string(&events).unwrap();
        assert!(!json.contains(CANARY));
        assert!(!json.contains(&self.path.to_string_lossy().to_string()));
        assert!(vega_store::run_diagnostics::incomplete_attempt_ids(&events).is_empty());
        events
            .into_iter()
            .filter(|row| row.event.state != vega_store::run_diagnostics::DiagnosticState::Started)
            .map(|row| {
                assert_eq!(
                    row.event.failure_code,
                    Some(DiagnosticFailureCode::SummarySourceChanged)
                );
                let reason = row.event.metrics.compaction_rejection.clone().unwrap();
                assert_eq!(
                    row.event.metrics,
                    DiagnosticMetrics {
                        compaction_rejection: Some(reason.clone()),
                        ..Default::default()
                    }
                );
                reason
            })
            .collect()
    }

    fn prepared(&self) -> vega_store::context_micro::PreparedMicro {
        let source = self.source();
        micro::prepare_micro(
            &self.store,
            &source,
            "fixture-model",
            None,
            &[source.tool_calls[0].id.clone()],
            Some(&self.diagnostics),
        )
        .unwrap()
    }
}

fn assert_changed<T>(result: Result<T, VegaError>) {
    assert!(matches!(
        result,
        Err(VegaError::Context(ContextRuntimeError::SourceChanged))
    ));
}

struct NoNetwork;
impl Provider for NoNetwork {
    fn chat_stream(
        &self,
        _: ChatRequest,
        _: CancellationToken,
    ) -> futures::future::BoxFuture<'static, Result<vega_runtime::EventStream, VegaError>> {
        panic!("provider must not be invoked")
    }
}

#[tokio::test]
async fn canonical_history_and_micro_commit_remain_accepted_without_failure_diagnostics() {
    let f = Fixture::new();
    let request = f.request();
    assert!(
        validate_request_source_with_diagnostics(&f.source(), None, &request, Some(&f.diagnostics))
            .is_ok()
    );
    let hook = ConversationCompactionHook::new(
        &NoNetwork,
        f.path.clone(),
        &f.thread,
        "fixture-model",
        None,
        None,
    )
    .with_thread_admission(f.admission.clone())
    .with_diagnostics(f.diagnostics.clone());
    let result = hook
        .micro_compact_impl(
            vega_runtime::MicroCompactionRequest {
                context: request,
                replay_frame: None,
            },
            CancellationToken::new(),
        )
        .await
        .unwrap()
        .unwrap();
    assert!(!result.targets.is_empty());
    assert!(result.estimated_savings >= vega_runtime::MICRO_MIN_SAVINGS);
    assert!(
        f.source()
            .tool_calls
            .iter()
            .any(|call| call.micro_compacted)
    );
    assert!(f.reasons().await.is_empty());
}

#[tokio::test]
async fn changed_history_fields_are_rejected_and_classified_without_values() {
    for (mutation, reason) in [
        (0, "compaction_history_result"),
        (1, "compaction_history_call"),
        (2, "compaction_history_offset"),
        (3, "compaction_history_group"),
        (4, "compaction_history_messages"),
    ] {
        let f = Fixture::new();
        let mut request = f.request();
        match mutation {
            0 => request
                .messages
                .iter_mut()
                .find(|m| m.role == ChatRole::Tool)
                .unwrap()
                .content
                .push_str(CANARY),
            1 => {
                request
                    .messages
                    .iter_mut()
                    .find(|m| !m.tool_calls.is_empty())
                    .unwrap()
                    .tool_calls[0]
                    .input_json = CANARY.into()
            }
            2 => request
                .messages
                .iter_mut()
                .find(|m| !m.tool_calls.is_empty())
                .unwrap()
                .content
                .push('x'),
            3 => {
                request.messages.retain(|m| m.role != ChatRole::Tool);
            }
            _ => request
                .messages
                .iter_mut()
                .find(|m| m.role == ChatRole::User)
                .unwrap()
                .content
                .push_str(CANARY),
        }
        let result = validate_request_source_with_diagnostics(
            &f.source(),
            None,
            &request,
            Some(&f.diagnostics),
        );
        assert_changed(result.map_err(|failure| *failure.error));
        assert_changed(
            validate_request_source_with_diagnostics(&f.source(), None, &request, None)
                .map_err(|failure| *failure.error),
        );
        assert_eq!(f.reasons().await, [reason]);
        assert!(
            !f.source()
                .tool_calls
                .iter()
                .any(|call| call.micro_compacted)
        );
    }
}

#[tokio::test]
async fn exact_source_fence_rejects_same_sequence_mutation_and_future_version() {
    for same_sequence in [true, false] {
        let f = Fixture::new();
        let mut request = f.request();
        if same_sequence {
            request.require_source_fence = true;
            f.mutate_source();
        } else {
            request.source_version += 1;
        }
        assert_changed(
            validate_request_source_with_diagnostics(
                &f.source(),
                None,
                &request,
                Some(&f.diagnostics),
            )
            .map_err(|failure| *failure.error),
        );
        assert_eq!(f.reasons().await, ["compaction_source_fence"]);
    }
}

#[tokio::test]
async fn live_owned_tool_difference_is_rejected_without_recording_values() {
    let f = Fixture::new();
    f.store
        .conn()
        .execute(
            "UPDATE tool_calls SET message_id=?1 WHERE thread_id=?2",
            (&f.owner, &f.thread),
        )
        .unwrap();
    f.store
        .conn()
        .execute(
            "UPDATE messages SET content='prefix' WHERE id=?1",
            [&f.owner],
        )
        .unwrap();
    let source = f.source();
    let mut owned_source = source.clone();
    owned_source.messages.retain(|row| row.id == f.owner);
    owned_source.messages[0].status = "done".into();
    let mut owned =
        super::super::pipeline::history_from_context_source(&owned_source, "unused-owner", None)
            .unwrap();
    let mut request = f.request();
    request.messages.append(&mut owned);
    assert!(
        validate_request_source_with_diagnostics(&source, None, &request, Some(&f.diagnostics))
            .is_ok()
    );
    request
        .messages
        .iter_mut()
        .find(|m| m.role == ChatRole::Tool)
        .unwrap()
        .content
        .push_str(CANARY);
    assert_changed(
        validate_request_source_with_diagnostics(&source, None, &request, Some(&f.diagnostics))
            .map_err(|failure| *failure.error),
    );
    assert_eq!(f.reasons().await, ["compaction_history_owned"]);
}

#[tokio::test]
async fn arbitrary_rejection_text_is_rejected_by_api_and_database_constraints() {
    let f = Fixture::new();
    Rejection::SourceFence.record(Some(&f.diagnostics));
    assert_eq!(f.reasons().await, ["compaction_source_fence"]);
    let start = vega_store::run_diagnostics::read_by_run(f.store.conn(), &f.thread, &f.owner)
        .unwrap()
        .remove(0)
        .event;
    let mut invalid = start.clone();
    invalid.attempt_id = ulid::Ulid::generate().to_string();
    invalid.metrics.compaction_rejection = Some(CANARY.into());
    assert!(vega_store::run_diagnostics::insert(f.store.conn(), &invalid).is_err());
    assert!(
        f.store
            .conn()
            .execute(
                "UPDATE run_diagnostic_events SET compaction_rejection=?1",
                [CANARY]
            )
            .is_err()
    );
    let events =
        vega_store::run_diagnostics::read_by_run(f.store.conn(), &f.thread, &f.owner).unwrap();
    assert!(!serde_json::to_string(&events).unwrap().contains(CANARY));
}

#[tokio::test]
async fn changed_source_before_prepare_is_rejected_without_installation() {
    let f = Fixture::new();
    let source = f.source();
    f.mutate_source();
    assert_changed(micro::prepare_micro(
        &f.store,
        &source,
        "fixture-model",
        None,
        &[source.tool_calls[0].id.clone()],
        Some(&f.diagnostics),
    ));
    assert_eq!(f.reasons().await, ["compaction_micro_prepare"]);
    assert!(
        !f.source()
            .tool_calls
            .iter()
            .any(|call| call.micro_compacted)
    );
}

#[tokio::test]
async fn changed_source_after_prepare_is_rejected_by_real_shared_writer() {
    let f = Fixture::new();
    let prepared = f.prepared();
    f.mutate_source();
    assert_changed(writer::commit_micro(
        &f.path,
        Some(&f.diagnostics),
        writer::Micro {
            prepared,
            cancel: CancellationToken::new(),
            admission: f.admission.clone(),
            diagnostics: Some(f.diagnostics.clone()),
        },
    ));
    assert_eq!(f.reasons().await, ["compaction_micro_commit"]);
    assert!(
        !f.source()
            .tool_calls
            .iter()
            .any(|call| call.micro_compacted)
    );
}

#[tokio::test]
async fn foreign_thread_admission_is_rejected_at_hook_and_install() {
    for at_install in [false, true] {
        let f = Fixture::new();
        let wrong =
            crate::thread_access::acquire(&f.store, &ulid::Ulid::generate().to_string()).unwrap();
        if at_install {
            assert_changed(
                writer::Micro {
                    prepared: f.prepared(),
                    cancel: CancellationToken::new(),
                    admission: wrong,
                    diagnostics: Some(f.diagnostics.clone()),
                }
                .apply(&f.store),
            );
        } else {
            let hook = ConversationCompactionHook::new(
                &NoNetwork,
                f.path.clone(),
                &f.thread,
                "fixture-model",
                None,
                None,
            )
            .with_thread_admission(wrong)
            .with_diagnostics(f.diagnostics.clone());
            assert_changed(
                hook.micro_compact_impl(
                    vega_runtime::MicroCompactionRequest {
                        context: f.request(),
                        replay_frame: None,
                    },
                    CancellationToken::new(),
                )
                .await,
            );
        }
        assert_eq!(f.reasons().await, ["compaction_ownership"]);
        assert!(
            !f.source()
                .tool_calls
                .iter()
                .any(|call| call.micro_compacted)
        );
    }
}

#[tokio::test]
async fn database_identity_conflict_is_rejected_before_command_execution() {
    let f = Fixture::new();
    let other = Fixture::new();
    let raw = crate::database_writer::commit(
        &f.path,
        other.store.database_identity(),
        writer::Micro {
            prepared: f.prepared(),
            cancel: CancellationToken::new(),
            admission: f.admission.clone(),
            diagnostics: Some(f.diagnostics.clone()),
        },
    );
    assert!(raw.is_err());
    assert_changed(writer::micro_writer_result(raw, Some(&f.diagnostics)));
    assert_eq!(f.reasons().await, ["compaction_writer_admission"]);
    assert!(
        !f.source()
            .tool_calls
            .iter()
            .any(|call| call.micro_compacted)
    );
}

#[tokio::test]
async fn cancelled_install_stays_cancelled_without_source_changed_diagnostic() {
    let f = Fixture::new();
    let cancel = CancellationToken::new();
    cancel.cancel();
    let result = writer::Micro {
        prepared: f.prepared(),
        cancel,
        admission: f.admission.clone(),
        diagnostics: Some(f.diagnostics.clone()),
    }
    .apply(&f.store);
    assert!(matches!(result, Err(VegaError::Cancelled)));
    assert!(f.reasons().await.is_empty());
    assert!(
        !f.source()
            .tool_calls
            .iter()
            .any(|call| call.micro_compacted)
    );
}
