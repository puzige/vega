use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Directory(PathBuf);

impl Directory {
    fn new() -> Result<Self, std::io::Error> {
        let path =
            std::env::temp_dir().join(format!("vega-context-files-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Deny(AtomicUsize);

impl RuntimePermissionHook for Deny {
    fn request(
        &self,
        _: RuntimePermissionPrompt,
        _: CancellationToken,
    ) -> BoxFuture<'static, Result<RuntimeUserDecision, VegaError>> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Box::pin(async { Ok(RuntimeUserDecision::Deny { note: None }) })
    }
}

fn reference(path: &str) -> Vec<ChatMessage> {
    let mut summary = ChatMessage::new(ChatRole::User, "Historical summary");
    summary.attach_context_file_references(vec![("source-read".into(), path.into())]);
    vec![ChatMessage::new(ChatRole::System, "system"), summary]
}

#[tokio::test]
async fn current_file_content_is_filtered_without_changing_durable_messages() -> TestResult {
    let directory = Directory::new()?;
    let path = directory.0.join("notes.txt");
    let tools = vega_tools::Tools::new(&directory.0)?;
    std::fs::write(&path, "old file")?;
    tools.read("notes.txt", None, None)?;
    std::fs::write(&path, "current file with synthetic-private-value")?;
    let config = RuntimeToolConfig::default()
        .with_credential_reader(Arc::new(|| Ok(vec!["synthetic-private-value".into()])));
    let deny = Deny(AtomicUsize::new(0));
    let mut messages = reference("notes.txt");
    let durable = messages[1].clone();
    restore_context_files(
        &mut messages,
        &tools,
        &config,
        &deny,
        &CancellationToken::new(),
    )
    .await?;
    assert_eq!(messages[1].without_runtime_metadata(), durable);
    assert_eq!(messages[1].content, "Historical summary");
    let projected = crate::project_model_messages(&messages)?;
    assert!(projected[1].content.contains("current file with"));
    assert!(!projected[1].content.contains("synthetic-private-value"));
    assert!(!projected[1].content.contains("old file"));
    assert_eq!(crate::project_model_messages(&projected)?, projected);
    assert_eq!(deny.0.load(Ordering::Relaxed), 0);
    let full = crate::estimate_wire_context(&messages, &[])?.input_tokens;
    let base = crate::estimate_wire_context(&reference("notes.txt"), &[])?.input_tokens;
    assert!(full > base);
    fit_context_files(&mut messages, &[], base)?;
    assert_eq!(
        crate::estimate_wire_context(&messages, &[])?.input_tokens,
        base
    );
    assert_eq!(messages[1].without_runtime_metadata(), durable);
    Ok(())
}

struct OnceThenDeny(AtomicUsize);

impl RuntimePermissionHook for OnceThenDeny {
    fn request(
        &self,
        _: RuntimePermissionPrompt,
        _: CancellationToken,
    ) -> BoxFuture<'static, Result<RuntimeUserDecision, VegaError>> {
        let decision = if self.0.fetch_add(1, Ordering::Relaxed) == 0 {
            RuntimeUserDecision::Once
        } else {
            RuntimeUserDecision::Deny { note: None }
        };
        Box::pin(async move { Ok(decision) })
    }
}

#[tokio::test]
async fn warm_candidates_recheck_current_files_and_once_permissions() -> TestResult {
    for mode in ["updated", "deleted", "denied"] {
        let directory = Directory::new()?;
        let external = Directory::new()?;
        let path = if mode == "denied" {
            external.0.join("notes.txt")
        } else {
            directory.0.join("notes.txt")
        };
        std::fs::write(&path, "old cached file detail")?;
        let tools = vega_tools::Tools::new(&directory.0)?;
        let provider = CapturingProvider(std::sync::Mutex::new(Vec::new()));
        let hook = OnceThenDeny(AtomicUsize::new(0));
        let route = [7; 32];
        let build_request =
            |history: Vec<ChatMessage>, replay_frame: crate::ReplayFrame| AgentRequest {
                model: "synthetic".into(),
                system_prompt: "system".into(),
                history,
                max_tokens: None,
                completed_tool_results: Default::default(),
                tool_config: RuntimeToolConfig::default(),
                pricing_catalog: None,
                reasoning: None,
                model_snapshot: None,
                replay_frame: Some(replay_frame),
                context_budget: Some(crate::ContextBudget::new(100000, 10000, true).unwrap()),
                context_source_version: Some(1),
                context_source_fingerprint: Some("source".into()),
                context_operation_id: Some("owner".into()),
                context_compaction_hook: None,
            };
        let mut frame = crate::ReplayFrame::new(2, crate::ReasoningReplayPolicy::Unknown);
        frame.attach_route_fingerprint(route);
        let history = reference(path.to_str().ok_or("path")?)
            .into_iter()
            .skip(1)
            .collect();
        let first = run_agent_with_permission_sink_and_context(
            &provider,
            &tools,
            build_request(history, frame),
            CancellationToken::new(),
            &hook,
            None,
            |_| async { Ok(()) },
        )
        .await?;
        assert!(!first.failed);
        let environment = first.messages[0]
            .replay_frame()
            .and_then(crate::ReplayFrame::environment_fingerprint)
            .ok_or("environment")?;
        let mut warm = first.messages;
        warm.push(ChatMessage::new(ChatRole::Assistant, "warm-only-marker"));
        if mode == "updated" {
            std::fs::write(&path, "new current file detail")?;
        } else if mode == "deleted" {
            std::fs::remove_file(&path)?;
        }
        let mut frame = crate::ReplayFrame::new(2, crate::ReasoningReplayPolicy::Unknown);
        frame.attach_route_fingerprint(route);
        frame.attach_candidate(warm, environment);
        let mut history = reference(path.to_str().ok_or("path")?)
            .into_iter()
            .skip(1)
            .collect::<Vec<_>>();
        history.push(ChatMessage::new(ChatRole::User, "Continue now"));
        let second = run_agent_with_permission_sink_and_context(
            &provider,
            &tools,
            build_request(history, frame),
            CancellationToken::new(),
            &hook,
            None,
            |_| async { Ok(()) },
        )
        .await?;
        assert!(!second.failed);
        let requests = provider.0.lock().unwrap();
        assert_eq!(requests.len(), 2);
        let first_wire = requests[0]
            .messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let wire = requests[1]
            .messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(first_wire.contains("old cached file detail"));
        assert!(wire.contains("warm-only-marker"));
        assert!(!wire.contains("old cached file detail"));
        assert!(wire.contains("source-read"));
        if mode == "updated" {
            assert!(wire.contains("new current file detail"));
            assert!(wire.contains("restored_file"));
        } else {
            assert!(wire.contains("compact_file_reference"));
            assert!(!wire.contains("restored_file"));
        }
        assert_eq!(
            hook.0.load(Ordering::Relaxed),
            if mode == "denied" { 2 } else { 0 }
        );
    }
    Ok(())
}

struct CapturingProvider(std::sync::Mutex<Vec<ChatRequest>>);

impl Provider for CapturingProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        _: CancellationToken,
    ) -> BoxFuture<'static, Result<crate::EventStream, VegaError>> {
        self.0.lock().unwrap().push(request);
        Box::pin(async {
            Ok(Box::pin(futures::stream::iter([
                Ok(ProviderEvent::TextDelta("Continued".into())),
                Ok(ProviderEvent::Done {
                    stop_reason: StopReason::End,
                }),
            ])) as crate::EventStream)
        })
    }
}

#[tokio::test]
async fn restarted_history_restores_current_file_in_actual_primary_request() -> TestResult {
    let directory = Directory::new()?;
    std::fs::write(directory.0.join("notes.txt"), "current restart detail")?;
    let tools = vega_tools::Tools::new(&directory.0)?;
    let provider = CapturingProvider(std::sync::Mutex::new(Vec::new()));
    let request = AgentRequest {
        model: "synthetic".into(),
        system_prompt: "system".into(),
        history: reference("notes.txt").into_iter().skip(1).collect(),
        max_tokens: None,
        completed_tool_results: Default::default(),
        tool_config: RuntimeToolConfig::default(),
        pricing_catalog: None,
        reasoning: None,
        model_snapshot: None,
        replay_frame: None,
        context_budget: Some(crate::ContextBudget::new(100000, 10000, true)?),
        context_source_version: Some(1),
        context_source_fingerprint: Some("source".into()),
        context_operation_id: Some("owner".into()),
        context_compaction_hook: None,
    };
    let outcome = run_agent_with_permission_sink_and_context(
        &provider,
        &tools,
        request,
        CancellationToken::new(),
        &Deny(AtomicUsize::new(0)),
        None,
        |_| async { Ok(()) },
    )
    .await?;
    assert!(!outcome.failed);
    let requests = provider.0.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0]
            .messages
            .iter()
            .any(|message| message.content.contains("current restart detail")
                && message.content.contains("restored_file"))
    );
    Ok(())
}

#[tokio::test]
async fn denied_missing_and_oversized_files_never_claim_full_restoration() -> TestResult {
    let root = Directory::new()?;
    let external = Directory::new()?;
    let external_path = external.0.join("external.txt");
    std::fs::write(&external_path, "must not be exposed")?;
    std::fs::write(root.0.join("large.txt"), "x".repeat(20001))?;
    std::fs::write(root.0.join("huge.txt"), "x".repeat(1024 * 1024 + 1))?;
    let tools = vega_tools::Tools::new(&root.0)?;
    let config = RuntimeToolConfig::default();
    let deny = Deny(AtomicUsize::new(0));
    for path in [
        external_path.to_str().ok_or("path")?,
        "missing.txt",
        "large.txt",
        "huge.txt",
    ] {
        let mut messages = reference(path);
        restore_context_files(
            &mut messages,
            &tools,
            &config,
            &deny,
            &CancellationToken::new(),
        )
        .await?;
        let wire = crate::project_model_messages(&messages)?;
        assert!(!wire[1].content.contains("restored_file"));
        assert!(!wire[1].content.contains("must not be exposed"));
        assert!(wire[1].content.contains("compact_file_reference"));
        assert!(wire[1].content.contains("source-read"));
        restore_context_files(
            &mut messages,
            &tools,
            &config,
            &deny,
            &CancellationToken::new(),
        )
        .await?;
    }
    assert_eq!(deny.0.load(Ordering::Relaxed), 1);
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(matches!(
        restore_context_files(&mut reference("large.txt"), &tools, &config, &deny, &cancel).await,
        Err(VegaError::Cancelled)
    ));
    std::fs::write(root.0.join("safe.txt"), "safe")?;
    let unavailable = config.with_credential_reader(Arc::new(|| Err(())));
    assert!(matches!(
        restore_context_files(
            &mut reference("safe.txt"),
            &tools,
            &unavailable,
            &deny,
            &CancellationToken::new()
        )
        .await,
        Err(VegaError::CredentialCheckUnavailable)
    ));
    Ok(())
}
