use super::*;
use crate::{
    AgentOutcome, AgentRequest, ChatMessage, ChatRole, RuntimeEvent, RuntimeToolConfig,
    RuntimeToolStatus, ToolDefinition,
};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Mutex;
use tempfile::TempDir;

mod messages;
mod protocol;
mod replay;

struct Script {
    chunks: Vec<Value>,
    extra: Vec<ProviderEvent>,
    finalize: bool,
    raw_payload: Option<String>,
    cancel: bool,
}

impl Script {
    fn read(id: &str, state: Value) -> Self {
        let mut delta = state.as_object().cloned().expect("state object");
        delta.insert("role".into(), json!("assistant"));
        Self {
            chunks: vec![
                json!({"choices":[{"delta":delta}]}),
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":id,"type":"function","function":{"name":"read","arguments":"{\"path\":\"sample.txt\",\"offset\":null,\"limit\":null}"}}]}}]}),
                json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}),
            ],
            extra: Vec::new(),
            finalize: true,
            raw_payload: None,
            cancel: false,
        }
    }

    fn final_reply(state: Value) -> Self {
        let mut delta = state.as_object().cloned().expect("final state object");
        delta.insert("content".into(), json!("complete"));
        Self {
            chunks: vec![json!({"choices":[{"delta":delta,"finish_reason":"stop"}]})],
            extra: Vec::new(),
            finalize: true,
            raw_payload: None,
            cancel: false,
        }
    }
}

struct ScriptedProvider {
    transport: OpenAiProvider,
    scripts: Mutex<VecDeque<Script>>,
    requests: Arc<Mutex<Vec<ChatRequest>>>,
    bodies: Arc<Mutex<Vec<Value>>>,
}

impl ScriptedProvider {
    fn new(endpoint: &str, scripts: Vec<Script>) -> Self {
        Self {
            transport: OpenAiProvider::new(endpoint, "owned-fixture-credential")
                .expect("transport fixture"),
            scripts: Mutex::new(scripts.into()),
            requests: Default::default(),
            bodies: Default::default(),
        }
    }

    fn bodies(&self) -> Vec<Value> {
        self.bodies.lock().expect("body capture").clone()
    }

    fn requests(&self) -> Vec<ChatRequest> {
        self.requests.lock().expect("request capture").clone()
    }
}

impl Provider for ScriptedProvider {
    fn chat_stream(
        &self,
        mut request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<EventStream, VegaError>> {
        request.messages =
            crate::project_model_messages(&request.messages).expect("real provider projection");
        let encoded = self.transport.request_body(&request);
        self.requests
            .lock()
            .expect("capture requests")
            .push(request);
        if let Ok(body) = &encoded {
            self.bodies
                .lock()
                .expect("capture bodies")
                .push(body.clone());
        }
        let script = self.scripts.lock().expect("scripts").pop_front();
        Box::pin(async move {
            encoded?;
            let script = script.expect("unexpected request");
            let mut assembler = sse::SseAssembler::default();
            let mut events = Vec::new();
            let mut complete = true;
            let payloads = script
                .chunks
                .into_iter()
                .map(|chunk| chunk.to_string())
                .chain(script.raw_payload);
            for payload in payloads {
                match assembler.absorb(&payload) {
                    Ok(items) => events.extend(items.into_iter().map(Ok)),
                    Err(error) => {
                        events.push(Err(error));
                        complete = false;
                        break;
                    }
                }
            }
            if complete {
                events.extend(script.extra.into_iter().map(Ok));
                if script.finalize {
                    match assembler.finalize() {
                        Ok(items) => events.extend(items.into_iter().map(Ok)),
                        Err(error) => events.push(Err(error)),
                    }
                }
            }
            if script.cancel {
                cancel.cancel();
            }
            Ok(Box::pin(futures::stream::iter(events)) as EventStream)
        })
    }
}

fn fixture() -> (TempDir, vega_tools::Tools) {
    let root = tempfile::tempdir().expect("owned runtime fixture");
    std::fs::write(root.path().join("sample.txt"), "verified tool result\n").expect("fixture file");
    let tools = vega_tools::Tools::new(root.path()).expect("owned tools");
    (root, tools)
}

fn request(model: &str) -> AgentRequest {
    AgentRequest {
        model: model.into(),
        system_prompt: "Read the file and answer.".into(),
        history: vec![ChatMessage::new(ChatRole::User, "Read sample.txt")],
        max_tokens: Some(512),
        completed_tool_results: HashMap::new(),
        tool_config: RuntimeToolConfig::default().with_turn_limit(8),
        pricing_catalog: None,
        reasoning: None,
        replay_frame: None,
        context_budget: None,
        context_source_version: None,
        context_source_fingerprint: None,
        context_operation_id: None,
        context_compaction_hook: None,
    }
}

fn tool_messages(body: &Value) -> Vec<&Value> {
    body["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .filter(|message| message["role"] == "assistant" && message.get("tool_calls").is_some())
        .collect()
}

fn assert_failed_without_tools(outcome: &AgentOutcome) {
    assert!(outcome.failed);
    assert_eq!(outcome.executed_tool_call_count, 0);
    assert!(!outcome.events.iter().any(|event| matches!(
        event,
        RuntimeEvent::ToolCallApproved { .. }
            | RuntimeEvent::ToolCallRunning { .. }
            | RuntimeEvent::ToolCallOutput { .. }
            | RuntimeEvent::ToolCallFinished(_)
    )));
    assert!(
        outcome
            .messages
            .iter()
            .all(|message| message.tool_calls.is_empty())
    );
}

async fn run_scripts(
    endpoint: &str,
    model: &str,
    scripts: Vec<Script>,
) -> (AgentOutcome, ScriptedProvider) {
    let (_root, tools) = fixture();
    let provider = ScriptedProvider::new(endpoint, scripts);
    let outcome = crate::run_agent(&provider, &tools, request(model), CancellationToken::new())
        .await
        .expect("runtime result");
    (outcome, provider)
}
