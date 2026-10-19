use super::*;
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;
use vega_runtime::{
    ChatMessage, ChatRequest, ChatRole, EventStream, Provider, ReasoningChoice, RuntimeToolConfig,
    VegaError,
};

type BoxFuture<T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'static>>;

fn selected(
    endpoint: &str,
    model: &str,
    choice: &str,
    api: ProviderApi,
) -> ReasoningProfileProjection {
    let root = tempfile::tempdir().expect("owned preference fixture");
    let path = root.path().join(vega_store::reasoning::REASONING_FILE_NAME);
    std::fs::write(&path,format!("version = 1\n[[preferences]]\nprovider = 'fixture'\nmodel = '{model}'\npreference = '{choice}'\n")).expect("preference file");
    let stored = vega_store::reasoning::read_from(&path).expect("valid new preference grammar");
    let projection = ReasoningProfileProjection::from_store(
        stored.profile("fixture", model).expect("preference"),
    )
    .expect("stored projection");
    resolve_reasoning_profile("fixture", api, endpoint, model, Some(projection))
        .expect("resolved facts")
}

fn control(body: &Value) -> Value {
    let mut result = serde_json::Map::new();
    for key in [
        "thinking",
        "reasoning",
        "reasoning_effort",
        "enable_thinking",
        "chat_template_args",
    ] {
        if let Some(value) = body.get(key) {
            result.insert(key.into(), value.clone());
        }
    }
    Value::Object(result)
}

fn body(endpoint: &str, model: &str, choice: &str, api: ProviderApi) -> Value {
    let frozen = freeze_reasoning_profile(selected(endpoint, model, choice, api))
        .expect("valid frozen preference");
    vega_runtime::test_support::encode(
        endpoint,
        api == ProviderApi::Responses,
        &ChatRequest {
            model: model.into(),
            messages: vec![ChatMessage::new(ChatRole::User, "question")],
            tools: vec![],
            max_tokens: Some(256),
            reasoning: Some(frozen),
        },
    )
    .expect("real encoder")
}

#[test]
fn all_eight_catalog_dialects_encode_default_and_each_actual_control_from_new_preferences() {
    let cases = [
        (
            "https://api.groq.com/openai/v1",
            "openai/gpt-oss-120b",
            vec![
                ("provider_default", json!({})),
                ("low", json!({"reasoning_effort":"low"})),
                ("high", json!({"reasoning_effort":"high"})),
            ],
        ),
        (
            "https://api.deepseek.com",
            "deepseek-v4-pro",
            vec![
                ("provider_default", json!({})),
                ("disabled", json!({"thinking":{"type":"disabled"}})),
                (
                    "high",
                    json!({"thinking":{"type":"enabled"},"reasoning_effort":"high"}),
                ),
                (
                    "max",
                    json!({"thinking":{"type":"enabled"},"reasoning_effort":"max"}),
                ),
            ],
        ),
        (
            "https://api.z.ai/api/coding/paas/v4",
            "glm-5.3",
            vec![
                (
                    "provider_default",
                    json!({"thinking":{"clear_thinking":false}}),
                ),
                (
                    "high",
                    json!({"thinking":{"type":"enabled","clear_thinking":false},"reasoning_effort":"high"}),
                ),
                (
                    "max",
                    json!({"thinking":{"type":"enabled","clear_thinking":false},"reasoning_effort":"max"}),
                ),
            ],
        ),
        (
            "https://token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1",
            "qwen3.6-plus",
            vec![
                ("provider_default", json!({})),
                ("enabled", json!({"enable_thinking":true})),
                ("disabled", json!({"enable_thinking":false})),
            ],
        ),
        (
            "https://api.together.ai/v1",
            "Qwen/Qwen3.5-9B",
            vec![
                ("provider_default", json!({})),
                ("enabled", json!({"reasoning":{"enabled":true}})),
                ("disabled", json!({"reasoning":{"enabled":false}})),
            ],
        ),
        (
            "https://inference.baseten.co/v1",
            "moonshotai/Kimi-K2.6",
            vec![
                ("provider_default", json!({})),
                (
                    "enabled",
                    json!({"chat_template_args":{"enable_thinking":true}}),
                ),
                (
                    "disabled",
                    json!({"chat_template_args":{"enable_thinking":false}}),
                ),
            ],
        ),
        (
            "https://openrouter.ai/api/v1",
            "openai/gpt-oss-120b",
            vec![
                ("provider_default", json!({})),
                ("low", json!({"reasoning":{"effort":"low"}})),
                ("high", json!({"reasoning":{"effort":"high"}})),
            ],
        ),
        (
            "https://api.ant-ling.com/v1",
            "Ring-2.6-1T",
            vec![
                ("provider_default", json!({})),
                ("high", json!({"reasoning":{"effort":"high"}})),
                ("xhigh", json!({"reasoning":{"effort":"xhigh"}})),
            ],
        ),
    ];
    for (endpoint, model, choices) in cases {
        for (choice, expected) in choices {
            assert_eq!(
                control(&body(endpoint, model, choice, ProviderApi::ChatCompletions)),
                expected,
                "{endpoint}/{model}/{choice}"
            );
        }
    }
}

#[test]
fn known_native_missing_maps_null_keys_and_uncontrollable_models_have_truthful_choices() {
    for (endpoint, model, efforts, disabled) in [
        (
            "https://router.huggingface.co/v1",
            "MiniMaxAI/MiniMax-M2",
            vec!["minimal", "low", "medium", "high"],
            false,
        ),
        (
            "https://openrouter.ai/api/v1",
            "amazon/nova-2-lite-v1",
            vec!["minimal", "low", "medium", "high"],
            true,
        ),
        (
            "https://api.ant-ling.com/v1",
            "Ring-2.6-1T",
            vec!["high", "xhigh"],
            false,
        ),
        (
            "https://api.groq.com/openai/v1",
            "openai/gpt-oss-120b",
            vec!["low", "medium", "high"],
            false,
        ),
        (
            "https://integrate.api.nvidia.com/v1",
            "moonshotai/kimi-k2.6",
            vec![],
            false,
        ),
        (
            "https://proxy.example/v1",
            "step-5-preview",
            vec!["low", "medium", "high"],
            false,
        ),
    ] {
        let profile = selected(
            endpoint,
            model,
            "provider_default",
            ProviderApi::ChatCompletions,
        );
        assert_eq!(profile.efforts, efforts, "{endpoint}/{model}");
        assert_eq!(profile.supports_disabled, disabled, "{endpoint}/{model}");
        for choice in [
            "enabled", "disabled", "minimal", "low", "medium", "high", "xhigh", "max",
        ] {
            let selected = selected(endpoint, model, choice, ProviderApi::ChatCompletions);
            let valid = efforts.contains(&choice) || (disabled && choice == "disabled");
            assert_eq!(
                freeze_reasoning_profile(selected).is_ok(),
                valid,
                "{endpoint}/{model}/{choice}"
            );
        }
    }
    for (endpoint, model) in [
        ("https://api.together.ai/v1", "Qwen/Qwen3.5-9B"),
        ("https://inference.baseten.co/v1", "moonshotai/Kimi-K2.6"),
        (
            "https://token-plan.cn-beijing.maas.aliyuncs.com/compatible-mode/v1",
            "qwen3.6-plus",
        ),
    ] {
        let profile = selected(endpoint, model, "enabled", ProviderApi::ChatCompletions);
        assert!(profile.efforts.is_empty());
        assert!(freeze_reasoning_profile(profile).is_ok());
        assert!(
            freeze_reasoning_profile(selected(
                endpoint,
                model,
                "high",
                ProviderApi::ChatCompletions
            ))
            .is_err()
        );
    }
}

#[test]
fn service_identity_and_model_ambiguity_do_not_spread_vendor_controls() {
    assert_eq!(
        control(&body(
            "https://inference.baseten.co/v1",
            "zai-org/GLM-5.2",
            "high",
            ProviderApi::ChatCompletions
        )),
        json!({"chat_template_args":{"enable_thinking":true},"reasoning_effort":"high"})
    );
    assert_eq!(
        control(&body(
            "https://api.together.ai/v1",
            "deepseek-ai/DeepSeek-V4-Pro",
            "high",
            ProviderApi::ChatCompletions
        )),
        json!({"reasoning":{"enabled":true},"reasoning_effort":"high"})
    );
    assert_eq!(
        control(&body(
            "https://api.together.ai/v1",
            "openai/gpt-oss-120b",
            "high",
            ProviderApi::ChatCompletions
        )),
        json!({"reasoning_effort":"high"})
    );
    for endpoint in [
        "https://proxy.example/v1",
        "https://api.z.ai.evil/api/coding/paas/v4",
        "https://api.z.ai/api/coding/paas/v4?service=trusted",
        "https://cpa@api.z.ai/api/coding/paas/v4",
    ] {
        let profile = resolve_reasoning_profile(
            "zai",
            ProviderApi::ChatCompletions,
            endpoint,
            "glm-5.3",
            None,
        )
        .expect("proxy profile");
        assert_eq!(
            profile.efforts,
            ["minimal", "low", "medium", "high", "xhigh", "max"]
        );
        assert_eq!(
            control(&body(
                endpoint,
                "glm-5.3",
                "high",
                ProviderApi::ChatCompletions
            )),
            json!({"reasoning_effort":"high"})
        );
    }
    let response = body(
        "https://api.z.ai/api/coding/paas/v4",
        "glm-5.3",
        "high",
        ProviderApi::Responses,
    );
    assert_eq!(response["reasoning"]["effort"], "high");
    assert!(response.get("thinking").is_none());
    assert!(response.get("reasoning_effort").is_none());
}

struct Network {
    endpoint: String,
    scripts: Mutex<VecDeque<Vec<Value>>>,
    bodies: Arc<Mutex<Vec<Value>>>,
    reject: bool,
}

impl Provider for Network {
    fn chat_stream(
        &self,
        mut request: ChatRequest,
        _: CancellationToken,
    ) -> BoxFuture<Result<EventStream, VegaError>> {
        request.messages =
            vega_runtime::project_model_messages(&request.messages).expect("projection");
        let body = vega_runtime::test_support::encode(&self.endpoint, false, &request)
            .expect("real encoder");
        self.bodies.lock().expect("bodies").push(body);
        let reject = self.reject;
        let chunks = self.scripts.lock().expect("scripts").pop_front();
        Box::pin(async move {
            if reject {
                return Err(VegaError::ProviderDiagnostic {
                    kind: vega_runtime::ProviderFailureKind::Http,
                    status: Some(400),
                    message: "selected effort unsupported".into(),
                    retryable: false,
                    retry_count: None,
                    request_id: None,
                });
            }
            Ok(vega_runtime::test_support::stream(&chunks.expect("script")))
        })
    }
}

fn agent_request(model: &str, reasoning: FrozenReasoning) -> vega_runtime::AgentRequest {
    vega_runtime::AgentRequest {
        model: model.into(),
        system_prompt: "Read file then answer".into(),
        history: vec![ChatMessage::new(ChatRole::User, "read sample.txt")],
        max_tokens: Some(512),
        completed_tool_results: HashMap::new(),
        tool_config: RuntimeToolConfig::default().with_turn_limit(4),
        pricing_catalog: None,
        reasoning: Some(reasoning),
        replay_frame: None,
        context_budget: None,
        context_source_version: None,
        context_source_fingerprint: None,
        context_operation_id: None,
        context_compaction_hook: None,
    }
}

fn read_script() -> Vec<Value> {
    vec![
        json!({"choices":[{"delta":{"reasoning_content":"tool raw state","tool_calls":[{"id":"owned-read","index":0,"function":{"name":"read","arguments":"{\"path\":\"sample.txt\",\"offset\":null,\"limit\":null}"}}]},"finish_reason":"tool_calls"}]}),
    ]
}

#[tokio::test]
async fn every_catalog_dialect_and_step_complete_a_real_tool_cycle_without_templates() {
    for (endpoint, model) in [
        ("https://api.groq.com/openai/v1", "openai/gpt-oss-120b"),
        ("https://api.deepseek.com", "deepseek-v4-pro"),
        ("https://api.z.ai/api/coding/paas/v4", "glm-5.3"),
        (
            "https://token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1",
            "qwen3.6-plus",
        ),
        ("https://api.together.ai/v1", "Qwen/Qwen3.5-9B"),
        ("https://inference.baseten.co/v1", "moonshotai/Kimi-K2.6"),
        ("https://openrouter.ai/api/v1", "openai/gpt-oss-120b"),
        ("https://api.ant-ling.com/v1", "Ring-2.6-1T"),
        ("https://proxy.example/v1", "step-5-preview"),
    ] {
        let root = tempfile::tempdir().expect("owned tool fixture");
        std::fs::write(
            root.path().join("sample.txt"),
            "verified actual tool result",
        )
        .expect("file");
        let tools = vega_tools::Tools::new(root.path()).expect("tools");
        let network = Network {endpoint:endpoint.into(),scripts:Mutex::new(vec![read_script(),vec![json!({"choices":[{"delta":{"content":"complete","reasoning_content":"final raw state"},"finish_reason":"stop"}]})]].into()),bodies:Default::default(),reject:false};
        let frozen = freeze_reasoning_profile(selected(
            endpoint,
            model,
            "provider_default",
            ProviderApi::ChatCompletions,
        ))
        .expect("default facts");
        let outcome = vega_runtime::run_agent(
            &network,
            &tools,
            agent_request(model, frozen),
            CancellationToken::new(),
        )
        .await
        .expect("runtime");
        assert!(!outcome.failed, "{endpoint}/{model}");
        assert_eq!(outcome.executed_tool_call_count, 1);
        assert_eq!(outcome.final_text, "complete");
        let bodies = network.bodies.lock().expect("bodies");
        let messages = bodies[1]["messages"].as_array().expect("followup messages");
        let assistant = messages
            .iter()
            .find(|message| message.get("tool_calls").is_some())
            .expect("replayed call");
        assert_eq!(assistant["reasoning_content"], "tool raw state");
        assert_eq!(assistant["tool_calls"][0]["id"], "owned-read");
        let result = messages
            .iter()
            .find(|message| message["role"] == "tool")
            .expect("real result");
        assert_eq!(result["tool_call_id"], "owned-read");
        assert!(
            result["content"]
                .as_str()
                .expect("result text")
                .contains("verified actual tool result")
        );
    }
}

#[test]
fn unknown_gpt_and_future_models_expose_all_six_efforts_and_send_the_exact_saved_choice() {
    for model in [
        "gpt-6-luna",
        "gpt-6.1-sol",
        "gpt-6-astra",
        "future-model-not-in-any-catalog",
    ] {
        for api in [ProviderApi::ChatCompletions, ProviderApi::Responses] {
            let profile = selected("https://proxy.example/v1", model, "provider_default", api);
            assert_eq!(
                profile.efforts,
                ["minimal", "low", "medium", "high", "xhigh", "max"]
            );
            assert!(!profile.supports_disabled);
            let default = body("https://proxy.example/v1", model, "provider_default", api);
            assert_eq!(
                control(&default),
                if api == ProviderApi::Responses {
                    json!({"reasoning":{"summary":"auto"}})
                } else {
                    json!({})
                }
            );
            for effort in ["minimal", "low", "medium", "high", "xhigh", "max"] {
                let body = body("https://proxy.example/v1", model, effort, api);
                assert_eq!(
                    if api == ProviderApi::Responses {
                        body["reasoning"]["effort"].clone()
                    } else {
                        body["reasoning_effort"].clone()
                    },
                    effort,
                    "{model}/{api:?}/{effort}"
                );
            }
            for unsupported in ["enabled", "disabled"] {
                assert!(
                    freeze_reasoning_profile(selected(
                        "https://proxy.example/v1",
                        model,
                        unsupported,
                        api
                    ))
                    .is_err()
                );
            }
        }
    }
}

#[test]
fn known_services_unknown_model_use_the_current_service_effort_dialect_without_swallowing_choice() {
    let model = "new-model-with-unknown-capabilities";
    for (endpoint, expected) in [
        (
            "https://openrouter.ai/api/v1",
            json!({"reasoning":{"effort":"xhigh"}}),
        ),
        (
            "https://api.ant-ling.com/v1",
            json!({"reasoning":{"effort":"xhigh"}}),
        ),
        (
            "https://api.deepseek.com",
            json!({"thinking":{"type":"enabled"},"reasoning_effort":"xhigh"}),
        ),
        (
            "https://api.z.ai/api/coding/paas/v4",
            json!({"thinking":{"type":"enabled","clear_thinking":false},"reasoning_effort":"xhigh"}),
        ),
        (
            "https://token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1",
            json!({"enable_thinking":true,"reasoning_effort":"xhigh"}),
        ),
        (
            "https://api.together.ai/v1",
            json!({"reasoning":{"enabled":true},"reasoning_effort":"xhigh"}),
        ),
        (
            "https://inference.baseten.co/v1",
            json!({"reasoning_effort":"xhigh"}),
        ),
    ] {
        assert_eq!(
            control(&body(
                endpoint,
                model,
                "xhigh",
                ProviderApi::ChatCompletions
            )),
            expected,
            "{endpoint}"
        );
    }
}

#[tokio::test]
async fn saved_unknown_effort_reaches_request_and_service_rejection_is_terminal_without_tools_or_downgrade()
 {
    for model in [
        "gpt-6-luna",
        "gpt-6.1-sol",
        "gpt-6-astra",
        "future-model-not-in-any-catalog",
    ] {
        let root = tempfile::tempdir().expect("owned rejection fixture");
        let path = root.path().join(vega_store::reasoning::REASONING_FILE_NAME);
        let config = vega_store::reasoning::ReasoningConfig {
            version: 1,
            profiles: vec![vega_store::reasoning::ReasoningProfile {
                provider: "cpa".into(),
                model: model.into(),
                preference: "xhigh".into(),
            }],
        };
        std::fs::write(
            &path,
            vega_store::reasoning::encode(&config).expect("three field encode"),
        )
        .expect("save preference");
        let frozen = crate::app_agent::reasoning_for_provider_model(
            "cpa",
            ProviderApi::ChatCompletions,
            "https://proxy.example/v1",
            model,
            Some(&path),
        )
        .expect("real loader and freeze");
        assert_eq!(frozen.choice, ReasoningChoice::Effort("xhigh".into()));
        let tools = vega_tools::Tools::new(root.path()).expect("tools");
        let network = Network {
            endpoint: "https://proxy.example/v1".into(),
            scripts: Mutex::new(VecDeque::new()),
            bodies: Default::default(),
            reject: true,
        };
        let outcome = vega_runtime::run_agent(
            &network,
            &tools,
            agent_request(model, frozen),
            CancellationToken::new(),
        )
        .await
        .expect("rejected runtime outcome");
        assert!(outcome.failed);
        assert_eq!(outcome.executed_tool_call_count, 0);
        let bodies = network.bodies.lock().expect("bodies");
        assert_eq!(bodies.len(), 1);
        assert_eq!(bodies[0]["reasoning_effort"], "xhigh");
    }
}

#[tokio::test]
async fn warm_replay_uses_effective_profile_facts_even_when_other_environment_inputs_are_unchanged()
{
    let qwen = "https://token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1";
    let together = "https://api.together.ai/v1";
    let model = "future-shared-model";
    let root = tempfile::tempdir().expect("profile binding fixture");
    std::fs::write(
        root.path().join("sample.txt"),
        "verified profile tool result",
    )
    .expect("file");
    let tools = vega_tools::Tools::new(root.path()).expect("tools");
    let initial_frozen =
        freeze_reasoning_profile(selected(qwen, model, "high", ProviderApi::ChatCompletions))
            .expect("initial facts");
    let mut request = agent_request(model, initial_frozen.clone());
    let mut frame = vega_runtime::ReplayFrame::new(2, initial_frozen.replay_policy);
    frame.attach_route_fingerprint([6; 32]);
    request.replay_frame = Some(frame);
    let initial=Network {endpoint:qwen.into(),scripts:Mutex::new(vec![read_script(),vec![json!({"choices":[{"delta":{"content":"complete","reasoning_text":"effective-profile raw sentinel"},"finish_reason":"stop"}]})]].into()),bodies:Default::default(),reject:false};
    let first = vega_runtime::run_agent(&initial, &tools, request, CancellationToken::new())
        .await
        .expect("initial run");
    assert!(!first.failed);
    let environment = first.messages[0]
        .replay_frame()
        .expect("frame")
        .environment_fingerprint()
        .expect("environment");
    for (endpoint, warm) in [(qwen, true), (together, false)] {
        let frozen = freeze_reasoning_profile(selected(
            endpoint,
            model,
            "high",
            ProviderApi::ChatCompletions,
        ))
        .expect("next facts");
        let mut request = agent_request(model, frozen.clone());
        request.history = first.messages[1..]
            .iter()
            .map(ChatMessage::without_runtime_metadata)
            .collect();
        request
            .history
            .push(ChatMessage::new(ChatRole::User, "continue"));
        let mut frame =
            vega_runtime::ReplayFrame::new(request.history.len() + 1, frozen.replay_policy);
        frame.attach_route_fingerprint([6; 32]);
        frame.attach_candidate(first.messages.clone(), environment);
        request.replay_frame = Some(frame);
        let provider = Network {
            endpoint: endpoint.into(),
            scripts: Mutex::new(
                vec![vec![
                    json!({"choices":[{"delta":{"content":"continued"},"finish_reason":"stop"}]}),
                ]]
                .into(),
            ),
            bodies: Default::default(),
            reject: false,
        };
        let outcome = vega_runtime::run_agent(&provider, &tools, request, CancellationToken::new())
            .await
            .expect("continued run");
        assert!(!outcome.failed);
        assert_eq!(outcome.executed_tool_call_count, 0);
        assert_eq!(
            provider.bodies.lock().expect("bodies")[0]
                .to_string()
                .contains("effective-profile raw sentinel"),
            warm
        );
    }
}
