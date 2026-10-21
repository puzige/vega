use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::future::BoxFuture;
use futures::{StreamExt, stream};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use vega_runtime::model_compat::{
    ModelApi, ModelIdentity, ModelRoute, ResolvedModel, decode_model_catalog, resolve_model,
};
use vega_runtime::{
    ChatRequest, ChatRole, EventStream, Provider, ProviderEvent, ReasoningChoice, StopReason,
    VegaError,
};
use vega_token::{ModelPricingSpec, PricingCatalog, RateSpec};

use super::runner::{TrialRequest, run_trial as run_native_trial};

async fn run_trial(
    provider: &dyn Provider,
    public: TrialRequest,
    container: &Path,
    run_id: &str,
    pricing: Option<PricingCatalog>,
    cancel: CancellationToken,
) -> Result<Value, ()> {
    run_native_trial(
        provider,
        public,
        &container.join(run_id),
        run_id,
        pricing,
        cancel,
    )
    .await
    .map(|result| serde_json::to_value(result).unwrap())
    .map_err(|_| ())
}

struct OwnedDirectory(PathBuf);

impl OwnedDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("vega-harness-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&path).expect("create owned directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for OwnedDirectory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove owned directory");
    }
}

struct StreamDrop {
    cancel: CancellationToken,
    cleaned: Arc<AtomicBool>,
}

impl Drop for StreamDrop {
    fn drop(&mut self) {
        self.cleaned
            .store(self.cancel.is_cancelled(), Ordering::SeqCst);
    }
}

enum Step {
    Events {
        expected_result: Option<(&'static str, &'static str)>,
        events: Vec<Result<ProviderEvent, VegaError>>,
    },
    Waiting,
}

struct ScriptedProvider {
    steps: Mutex<VecDeque<Step>>,
    requests: Mutex<Vec<ChatRequest>>,
    calls: AtomicUsize,
    started: Arc<Notify>,
    cleaned: Arc<AtomicBool>,
    model: Option<ResolvedModel>,
}

impl ScriptedProvider {
    fn new(steps: Vec<Step>) -> Self {
        Self {
            steps: Mutex::new(steps.into()),
            requests: Mutex::new(Vec::new()),
            calls: AtomicUsize::new(0),
            started: Arc::new(Notify::new()),
            cleaned: Arc::new(AtomicBool::new(false)),
            model: Some(approved_model()),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Provider for ScriptedProvider {
    fn model_snapshot(&self) -> Option<ResolvedModel> {
        self.model.clone()
    }

    fn chat_stream(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(request.model, "gpt-6.1-sol");
        assert_eq!(
            request.reasoning.as_ref().map(|value| &value.choice),
            Some(&ReasoningChoice::Effort("max".into()))
        );
        self.requests.lock().unwrap().push(request.clone());
        let step = self.steps.lock().unwrap().pop_front().expect("script step");
        let started = self.started.clone();
        let cleaned = self.cleaned.clone();
        Box::pin(async move {
            match step {
                Step::Events {
                    expected_result,
                    events,
                } => {
                    if let Some((call_id, expected_text)) = expected_result {
                        let result = request
                            .messages
                            .iter()
                            .find(|message| {
                                message.role == ChatRole::Tool
                                    && message.tool_call_id.as_deref() == Some(call_id)
                            })
                            .expect("native tool result reached provider");
                        assert!(result.content.contains(expected_text));
                    }
                    Ok(Box::pin(stream::iter(events)) as EventStream)
                }
                Step::Waiting => {
                    let cleanup = StreamDrop {
                        cancel: cancel.clone(),
                        cleaned,
                    };
                    let first = stream::once(async move {
                        started.notify_one();
                        Ok(ProviderEvent::TextDelta("partial answer".into()))
                    });
                    let pending = stream::once(async move {
                        let _cleanup = cleanup;
                        cancel.cancelled().await;
                        Err(VegaError::Cancelled)
                    });
                    Ok(Box::pin(first.chain(pending)) as EventStream)
                }
            }
        })
    }
}

fn approved_model() -> ResolvedModel {
    let catalog = decode_model_catalog(
        &serde_json::to_vec(&json!({
            "version": 1,
            "models": [{
                "provider": "pilot-test",
                "id": "gpt-6.1-sol",
                "api": "openai-completions",
                "baseUrl": "https://example.invalid/v1",
                "reasoning": true,
                "thinkingLevelMap": {"max": "max"},
                "input": ["text"],
                "contextWindow": 32768,
                "maxTokens": 4096,
                "compat": {
                    "thinkingFormat": "openai",
                    "supportsReasoningEffort": true
                }
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    let definition = catalog
        .lookup(
            &ModelRoute::new(ModelApi::ChatCompletions, "https://example.invalid/v1").unwrap(),
            &ModelIdentity {
                provider: "pilot-test".into(),
                model: "gpt-6.1-sol".into(),
            },
        )
        .unwrap();
    resolve_model(&definition, None).unwrap()
}

fn request_value() -> Value {
    json!({
        "schema_version": 1,
        "task_id": "fixture-task",
        "task_prompt": "Read answer.txt and replace before with after.",
        "seed_files": {"answer.txt": "before\n", "source.txt": "immutable\n"},
        "writable_paths": ["answer.txt", "new.txt"],
        "fixed_policy": "Only the declared output files may change.",
        "harness": {"strategy": "Read the input and make the requested edit.", "skills": {}},
        "trusted_skill_approvals": {},
        "model": {"id": "gpt-6.1-sol", "reasoning_effort": "max"},
        "limits": {
            "timeout_ms": 10000,
            "turn_limit": 8,
            "max_output_tokens": 4096,
            "max_total_tokens": 20000
        }
    })
}

fn request(value: Value) -> TrialRequest {
    serde_json::from_value(value).expect("valid public request")
}

fn events(values: Vec<ProviderEvent>) -> Step {
    Step::Events {
        expected_result: None,
        events: values.into_iter().map(Ok).collect(),
    }
}

fn tool(id: &str, name: &str, input: Value) -> ProviderEvent {
    ProviderEvent::ToolUse {
        id: id.into(),
        name: name.into(),
        input_json: input.to_string(),
    }
}

fn done(reason: StopReason) -> ProviderEvent {
    ProviderEvent::Done {
        stop_reason: reason,
    }
}

fn read_tool(id: &str, path: &str) -> ProviderEvent {
    tool(
        id,
        "read",
        json!({"path": path, "offset": null, "limit": null}),
    )
}

fn edit_script() -> Vec<Step> {
    vec![
        events(vec![
            read_tool("read-input", "answer.txt"),
            done(StopReason::ToolUse),
        ]),
        Step::Events {
            expected_result: Some(("read-input", "before")),
            events: vec![
                Ok(tool(
                    "edit-output",
                    "edit",
                    json!({"path": "answer.txt", "edits": [{"oldText": "before", "newText": "after"}]}),
                )),
                Ok(done(StopReason::ToolUse)),
            ],
        },
        Step::Events {
            expected_result: Some(("edit-output", "replacements")),
            events: vec![
                Ok(ProviderEvent::TextDelta(
                    "Edited the requested file.".into(),
                )),
                Ok(done(StopReason::End)),
            ],
        },
    ]
}

fn pricing() -> PricingCatalog {
    PricingCatalog::from_specs(vec![ModelPricingSpec {
        model: "gpt-6.1-sol".into(),
        rates: RateSpec {
            input_usd_per_million: "1".into(),
            output_usd_per_million: "2".into(),
            cache_read_usd_per_million: "0".into(),
            cache_write_usd_per_million: "0".into(),
        },
        max_standard_input_tokens: None,
        schedule: None,
    }])
    .unwrap()
}

fn usage() -> ProviderEvent {
    ProviderEvent::Usage {
        input: 10,
        output: 5,
        cache_read: Some(0),
        cache_write: Some(0),
    }
}

#[tokio::test]
async fn native_edits_start_fresh_and_identical_retries_never_reexecute() {
    let root = OwnedDirectory::new();
    let public = request_value();
    let mut script = edit_script();
    script.extend(edit_script());
    let provider = ScriptedProvider::new(script);
    let first = run_trial(
        &provider,
        request(public.clone()),
        root.path(),
        "sample-a",
        None,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let second = run_trial(
        &provider,
        request(public.clone()),
        root.path(),
        "sample-b",
        None,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let first = serde_json::to_value(first).unwrap();
    let second = serde_json::to_value(second).unwrap();
    assert_eq!(first["backend"], "vega_native_runtime");
    assert_eq!(first["terminal"], "end");
    assert_eq!(first["artifacts"]["answer.txt"], "after\n");
    assert_eq!(second["artifacts"]["answer.txt"], "after\n");
    assert_eq!(first["hashes"]["seed"], second["hashes"]["seed"]);
    assert_eq!(public["seed_files"]["answer.txt"], "before\n");
    assert!(first.get("success").is_none());
    assert_eq!(first["metrics"]["primary_model_requests"], 3);
    assert_eq!(first["metrics"]["tool_executions"], 2);
    assert_eq!(provider.calls(), 6);
    let cached = run_trial(
        &provider,
        request(public.clone()),
        root.path(),
        "sample-a",
        None,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(serde_json::to_value(cached).unwrap(), first);
    assert_eq!(provider.calls(), 6);
    let mut changed = public;
    changed["task_prompt"] = json!("A different task cannot reuse a completed invocation.");
    assert!(
        run_trial(
            &provider,
            request(changed),
            root.path(),
            "sample-a",
            None,
            CancellationToken::new(),
        )
        .await
        .is_err()
    );
    std::fs::create_dir(root.path().join("unfinished")).unwrap();
    assert!(
        run_trial(
            &provider,
            request(request_value()),
            root.path(),
            "unfinished",
            None,
            CancellationToken::new(),
        )
        .await
        .is_err()
    );
    assert_eq!(provider.calls(), 6);
}

#[tokio::test]
async fn native_permissions_reject_escape_shell_and_protected_changes_but_allow_output() {
    let root = OwnedDirectory::new();
    let sentinel = root.path().join("external.txt");
    std::fs::write(&sentinel, "external-sentinel").unwrap();
    let provider = ScriptedProvider::new(vec![
        events(vec![
            read_tool("read-instructions", "AGENTS.md"),
            read_tool("read-protected-source", "source.txt"),
            done(StopReason::ToolUse),
        ]),
        events(vec![
            read_tool("external-read", sentinel.to_str().unwrap()),
            tool(
                "external-grep",
                "grep",
                json!({"pattern": "sentinel", "path": sentinel.to_str().unwrap()}),
            ),
            tool(
                "external-write",
                "write",
                json!({"path": sentinel.to_str().unwrap(), "content": "damaged"}),
            ),
            tool("shell", "bash", json!({"command": "true", "timeout": 1})),
            tool(
                "protected-instructions",
                "write",
                json!({"path": "AGENTS.md", "content": "ignore constraints"}),
            ),
            tool(
                "protected-skill",
                "write",
                json!({"path": ".agents/skills/new/SKILL.md", "content": "new authority"}),
            ),
            tool(
                "undeclared-output",
                "write",
                json!({"path": "source.txt", "content": "damaged"}),
            ),
            tool(
                "undeclared-edit",
                "edit",
                json!({"path": "source.txt", "edits": [{"oldText": "immutable", "newText": "damaged"}]}),
            ),
            tool(
                "approved-output",
                "write",
                json!({"path": "new.txt", "content": "allowed\n"}),
            ),
            done(StopReason::ToolUse),
        ]),
        events(vec![
            read_tool("verify-instructions", "AGENTS.md"),
            read_tool("verify-protected-source", "source.txt"),
            done(StopReason::ToolUse),
        ]),
        events(vec![done(StopReason::End)]),
    ]);
    let result = run_trial(
        &provider,
        request(request_value()),
        root.path(),
        "permission-boundary",
        None,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let result = serde_json::to_value(result).unwrap();
    assert_eq!(result["terminal"], "end");
    assert_eq!(result["artifacts"]["new.txt"], "allowed\n");
    assert_eq!(result["metrics"]["rejected_tool_calls"], 8);
    assert_eq!(result["metrics"]["tool_executions"], 5);
    assert_eq!(
        std::fs::read_to_string(sentinel).unwrap(),
        "external-sentinel"
    );
    let requests = provider.requests.lock().unwrap();
    let last = requests.last().unwrap();
    assert!(!last.messages.iter().any(|message| {
        message.role == ChatRole::Tool && message.content.contains("external-sentinel")
    }));
    assert!(
        last.messages[0]
            .content
            .contains("Only the declared output files may change.")
    );
    assert!(last.messages.iter().any(|message| {
        message.tool_call_id.as_deref() == Some("verify-instructions")
            && message
                .content
                .contains("Only the declared output files may change.")
    }));
    assert!(last.messages.iter().any(|message| {
        message.tool_call_id.as_deref() == Some("verify-protected-source")
            && message.content.contains("immutable")
    }));
    assert!(result["hashes"]["instructions"].as_str().unwrap().len() == 64);
}

#[tokio::test]
async fn terminal_status_and_observed_token_budget_never_imply_task_success() {
    let root = OwnedDirectory::new();
    let cases = [
        ("natural", StopReason::End, "end"),
        ("truncated", StopReason::Length, "length"),
        ("turns", StopReason::ToolUse, "turn_limit"),
    ];
    for (run_id, reason, expected) in cases {
        let mut public = request_value();
        public["limits"]["turn_limit"] = json!(1);
        let proposed = if reason == StopReason::ToolUse {
            vec![read_tool("read-once", "source.txt"), done(reason)]
        } else {
            vec![
                ProviderEvent::TextDelta("wrong answer".into()),
                done(reason),
            ]
        };
        let provider = ScriptedProvider::new(vec![events(proposed)]);
        let result = run_trial(
            &provider,
            request(public),
            root.path(),
            run_id,
            None,
            CancellationToken::new(),
        )
        .await
        .unwrap();
        let result = serde_json::to_value(result).unwrap();
        assert_eq!(result["terminal"], expected);
        assert!(result.get("success").is_none());
        assert_eq!(result["artifacts"]["answer.txt"], "before\n");
    }
    let mut public = request_value();
    public["limits"]["max_total_tokens"] = json!(15);
    let provider = ScriptedProvider::new(vec![events(vec![
        usage(),
        tool(
            "budget-write",
            "write",
            json!({"path": "new.txt", "content": "unexpected"}),
        ),
        done(StopReason::ToolUse),
    ])]);
    let result = run_trial(
        &provider,
        request(public),
        root.path(),
        "token-budget",
        None,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let result = serde_json::to_value(result).unwrap();
    assert_eq!(result["terminal"], "budget_exhausted");
    assert_eq!(result["metrics"]["input_tokens"], 10);
    assert_eq!(result["metrics"]["output_tokens"], 5);
    assert!(result["artifacts"].get("new.txt").is_none());
    let provider = ScriptedProvider::new(vec![Step::Events {
        expected_result: None,
        events: vec![Err(VegaError::Provider {
            status: Some(503),
            message: "private-provider-diagnostic".into(),
            retryable: false,
        })],
    }]);
    let result = run_trial(
        &provider,
        request(request_value()),
        root.path(),
        "provider-failure",
        None,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let result = serde_json::to_value(result).unwrap();
    assert_eq!(result["terminal"], "failed");
    assert!(result["failure_category"].is_string());
    assert!(!result.to_string().contains("private-provider-diagnostic"));
}

#[tokio::test]
async fn cancellation_and_deadline_preserve_partial_trace_after_native_cleanup() {
    let root = OwnedDirectory::new();
    for timed_out in [false, true] {
        let provider = ScriptedProvider::new(vec![Step::Waiting]);
        let cancel = CancellationToken::new();
        let mut public = request_value();
        let cancellation = if timed_out {
            public["limits"]["timeout_ms"] = json!(40);
            None
        } else {
            let started = provider.started.clone();
            let cancel = cancel.clone();
            Some(tokio::spawn(async move {
                started.notified().await;
                tokio::time::sleep(Duration::from_millis(1)).await;
                cancel.cancel();
            }))
        };
        let result = run_trial(
            &provider,
            request(public),
            root.path(),
            if timed_out { "deadline" } else { "cancelled" },
            None,
            cancel,
        )
        .await
        .unwrap();
        if let Some(cancellation) = cancellation {
            cancellation.await.unwrap();
        }
        let result = serde_json::to_value(result).unwrap();
        assert_eq!(
            result["terminal"],
            if timed_out { "timeout" } else { "cancelled" }
        );
        assert_eq!(result["final_text"], "partial answer");
        assert!(!result["trace"].as_array().unwrap().is_empty());
        assert_eq!(result["metrics"]["primary_model_requests"], 1);
        assert!(provider.cleaned.load(Ordering::SeqCst));
    }
}

#[tokio::test]
async fn missing_or_partial_usage_and_pricing_remain_unknown() {
    let root = OwnedDirectory::new();
    for (run_id, catalogue, emits_usage, known) in [
        ("priced", Some(pricing()), true, true),
        ("unpriced", None, true, false),
        ("missing-usage", Some(pricing()), false, false),
    ] {
        let mut output = vec![done(StopReason::End)];
        if emits_usage {
            output.insert(0, usage());
        }
        let provider = ScriptedProvider::new(vec![events(output)]);
        let result = run_trial(
            &provider,
            request(request_value()),
            root.path(),
            run_id,
            catalogue,
            CancellationToken::new(),
        )
        .await
        .unwrap();
        let result = serde_json::to_value(result).unwrap();
        assert_eq!(result["metrics"]["pricing_known"], known);
        assert_eq!(result["metrics"]["usage_complete"], emits_usage);
        if known {
            assert_eq!(result["metrics"]["cost_microcents"], 2000);
        } else {
            assert!(result["metrics"]["cost_microcents"].is_null());
        }
        if emits_usage {
            assert_eq!(result["metrics"]["input_tokens"], 10);
        } else {
            assert!(result["metrics"]["input_tokens"].is_null());
        }
    }
    let provider = ScriptedProvider::new(vec![
        events(vec![
            read_tool("first-read", "source.txt"),
            usage(),
            done(StopReason::ToolUse),
        ]),
        events(vec![done(StopReason::End)]),
    ]);
    let result = run_trial(
        &provider,
        request(request_value()),
        root.path(),
        "partially-priced",
        Some(pricing()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let result = serde_json::to_value(result).unwrap();
    assert_eq!(result["metrics"]["usage_complete"], false);
    assert_eq!(result["metrics"]["pricing_known"], false);
    assert!(result["metrics"]["cost_microcents"].is_null());
}

#[tokio::test]
async fn trusted_skill_hashes_gate_real_native_loading_and_generated_guidance() {
    let root = OwnedDirectory::new();
    let body = "---\nname: fixture-skill\ndescription: Transform fixture output.\n---\nUse the literal text skill-result in new.txt.\n";
    let mut public = request_value();
    public["harness"]["skills"] = json!({"fixture-skill": body});
    public["trusted_skill_approvals"] =
        json!({"fixture-skill": format!("{:x}", Sha256::digest(body.as_bytes()))});
    let provider = ScriptedProvider::new(vec![
        events(vec![
            tool(
                "load-guidance",
                "load_skill",
                json!({"name": "fixture-skill"}),
            ),
            done(StopReason::ToolUse),
        ]),
        Step::Events {
            expected_result: Some(("load-guidance", "loaded")),
            events: vec![
                Ok(tool(
                    "skill-output",
                    "write",
                    json!({"path": "new.txt", "content": "skill-result"}),
                )),
                Ok(done(StopReason::ToolUse)),
            ],
        },
        events(vec![done(StopReason::End)]),
    ]);
    let result = run_trial(
        &provider,
        request(public.clone()),
        root.path(),
        "approved-skill",
        None,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let result = serde_json::to_value(result).unwrap();
    assert_eq!(result["artifacts"]["new.txt"], "skill-result");
    assert!(result["trace"].as_array().unwrap().iter().any(|entry| {
        entry["event"] == "skill_activation"
            && entry["name"] == "fixture-skill"
            && entry["status"] == "loaded"
    }));
    assert!(
        provider.requests.lock().unwrap()[1].messages[0]
            .content
            .contains("skill-result")
    );
    let uncalled = ScriptedProvider::new(Vec::new());
    public["harness"]["skills"]["fixture-skill"] = json!(format!("{body}\nChanged body."));
    assert!(
        run_trial(
            &uncalled,
            request(public.clone()),
            root.path(),
            "stale-skill",
            None,
            CancellationToken::new(),
        )
        .await
        .is_err()
    );
    public["trusted_skill_approvals"] = json!({});
    assert!(
        run_trial(
            &uncalled,
            request(public),
            root.path(),
            "unapproved-skill",
            None,
            CancellationToken::new(),
        )
        .await
        .is_err()
    );
    assert_eq!(uncalled.calls(), 0);
}

#[tokio::test]
async fn unsafe_request_fields_paths_and_unknown_model_profiles_fail_before_work() {
    let root = OwnedDirectory::new();
    let provider = ScriptedProvider::new(Vec::new());
    for (index, change) in [
        json!({"writable_paths": ["../outside"]}),
        json!({"writable_paths": ["AGENTS.md"]}),
        json!({"writable_paths": [".agents/skills/fixture-skill/SKILL.md"]}),
        json!({"seed_files": {"../outside": "escape"}}),
        json!({"seed_files": {"AGENTS.md": "replace policy"}}),
        json!({"harness": {"strategy": "text", "skills": {"../escape": "text"}}}),
        json!({"limits": {"timeout_ms": 0, "turn_limit": 1, "max_output_tokens": 1, "max_total_tokens": 1}}),
        json!({"limits": {"timeout_ms": 1, "turn_limit": 0, "max_output_tokens": 1, "max_total_tokens": 1}}),
        json!({"limits": {"timeout_ms": 1, "turn_limit": 1, "max_output_tokens": 0, "max_total_tokens": 1}}),
        json!({"limits": {"timeout_ms": 1, "turn_limit": 1, "max_output_tokens": 1, "max_total_tokens": 0}}),
        json!({"model": {"id": "other-model", "reasoning_effort": "max"}}),
        json!({"model": {"id": "gpt-6.1-sol", "reasoning_effort": "low"}}),
    ]
    .into_iter()
    .enumerate()
    {
        let mut public = request_value();
        for (key, value) in change.as_object().unwrap() {
            public[key] = value.clone();
        }
        assert!(
            run_trial(
                &provider,
                request(public),
                root.path(),
                &format!("invalid-{index}"),
                None,
                CancellationToken::new(),
            )
            .await
            .is_err()
        );
    }
    for change in [
        json!({"oracle": "private answer"}),
        json!({"expected": "private answer"}),
        json!({"harness": {"strategy": "text", "skills": {}, "scripts": {"run.py": "pass"}}}),
        json!({"model": {"id": "gpt-6.1-sol", "reasoning_effort": "max", "api_key": "secret"}}),
    ] {
        let mut public = request_value();
        for (key, value) in change.as_object().unwrap() {
            public[key] = value.clone();
        }
        assert!(serde_json::from_value::<TrialRequest>(public).is_err());
    }
    let mut unknown = ScriptedProvider::new(Vec::new());
    unknown.model = None;
    assert!(
        run_trial(
            &unknown,
            request(request_value()),
            root.path(),
            "unknown-profile",
            None,
            CancellationToken::new(),
        )
        .await
        .is_err()
    );
    let mut unsupported = ScriptedProvider::new(Vec::new());
    let mut definition = approved_model().definition;
    definition.thinking_level_map.insert("max".into(), None);
    unsupported.model = Some(resolve_model(&definition, None).unwrap());
    assert!(
        run_trial(
            &unsupported,
            request(request_value()),
            root.path(),
            "unsupported-effort",
            None,
            CancellationToken::new(),
        )
        .await
        .is_err()
    );
    assert_eq!(provider.calls(), 0);
    assert_eq!(unknown.calls(), 0);
    assert_eq!(unsupported.calls(), 0);
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[tokio::test]
async fn repeated_proposals_use_json_identity_and_reset_after_known_mutation() {
    let root = OwnedDirectory::new();
    let provider = ScriptedProvider::new(vec![
        events(vec![
            read_tool("read-first", "answer.txt"),
            done(StopReason::ToolUse),
        ]),
        events(vec![
            ProviderEvent::ToolUse {
                id: "read-reordered".into(),
                name: "read".into(),
                input_json: "{\"limit\":null,\"offset\":null,\"path\":\"answer.txt\"}".into(),
            },
            done(StopReason::ToolUse),
        ]),
        events(vec![
            tool(
                "change-file",
                "write",
                json!({"path": "answer.txt", "content": "changed\n"}),
            ),
            done(StopReason::ToolUse),
        ]),
        events(vec![
            read_tool("read-after-change", "answer.txt"),
            done(StopReason::ToolUse),
        ]),
        Step::Events {
            expected_result: Some(("read-after-change", "changed")),
            events: vec![Ok(done(StopReason::End))],
        },
    ]);
    let result = run_trial(
        &provider,
        request(request_value()),
        root.path(),
        "repeat-metric",
        None,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let result = serde_json::to_value(result).unwrap();
    assert_eq!(result["metrics"]["repeated_tool_proposals"], 1);
    assert_eq!(result["metrics"]["tool_executions"], 4);
    assert_eq!(result["artifacts"]["answer.txt"], "changed\n");
}
