mod resume_refresh;
mod text_delivery;

use super::*;
use crate::computer_use::{ComputerUseRun, ControlPhase, DesktopTarget};
use serde_json::{Value, json};

fn window(id: u32) -> DesktopTarget {
    DesktopTarget {
        selection_id: format!("fixture-{id}"),
        application: "Fixture".into(),
        bundle_id: "test.fixture".into(),
        pid: 1,
        window_id: id,
        title: format!("Owned {id}"),
    }
}

fn batch(calls: &[(&str, &str, Value)]) -> Vec<ScriptStep> {
    let mut events = calls
        .iter()
        .map(|(id, name, args)| ProviderEvent::ToolUse {
            id: (*id).into(),
            name: (*name).into(),
            input_json: args.to_string(),
        })
        .collect::<Vec<_>>();
    events.push(ProviderEvent::Done {
        stop_reason: StopReason::ToolUse,
    });
    vec![ScriptStep::events(events)]
}

fn directory() -> Vec<ScriptStep> {
    batch(&[(
        "directory",
        "ComputerObserve",
        json!({"target_id":null,"cursor":null}),
    )])
}

fn finish() -> Vec<ScriptStep> {
    vec![
        ScriptStep::text("Read both windows."),
        ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }]),
    ]
}

fn result<'a>(request: &'a ChatRequest, id: &str) -> &'a ChatMessage {
    request
        .messages
        .iter()
        .find(|message| message.tool_call_id.as_deref() == Some(id))
        .unwrap()
}

struct ActionProvider {
    run: ComputerUseRun,
    old: Arc<Mutex<Option<String>>>,
    requests: Mutex<Vec<ChatRequest>>,
}

impl Provider for ActionProvider {
    fn chat_stream(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<crate::EventStream, VegaError>> {
        let mut requests = self.requests.lock().unwrap();
        let index = requests.len();
        requests.push(request.clone());
        drop(requests);
        let script = match index {
            0 => directory(),
            1 => batch(&[
                (
                    "a",
                    "ComputerObserve",
                    json!({"target_id":"fixture-2","cursor":null}),
                ),
                (
                    "b",
                    "ComputerObserve",
                    json!({"target_id":"fixture-3","cursor":null}),
                ),
            ]),
            2 => {
                let old = self.old.lock().unwrap().clone().unwrap();
                let b = result(&request, "b");
                let current = b
                    .content
                    .lines()
                    .find_map(|line| line.strip_prefix("observation_id: "))
                    .unwrap();
                assert!(
                    self.run
                        .control
                        .observation(crate::computer_use::Capability::Desktop, &old)
                        .is_err()
                );
                assert!(
                    self.run
                        .control
                        .observation(crate::computer_use::Capability::Desktop, current)
                        .is_ok()
                );
                batch(&[
                    (
                        "old-action",
                        "ComputerAct",
                        json!({"observation_id":old,"action":"type_text","target":"edit-2","text":"NEVER"}),
                    ),
                    (
                        "current-action",
                        "ComputerAct",
                        json!({"observation_id":current,"action":"type_text","target":"edit-3","text":"CHANGED"}),
                    ),
                ])
            }
            _ => finish(),
        };
        MockProvider::new(script).chat_stream(request, cancel)
    }
}

struct BatchCompaction {
    observed: Arc<Mutex<Vec<ChatMessage>>>,
}

impl ContextCompactionHook for BatchCompaction {
    fn compact<'a>(
        &'a self,
        request: ContextCompactionRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'a, Result<ContextCompactionResult, ContextCompactionFailure>> {
        *self.observed.lock().unwrap() = request.messages;
        async move {
            Ok(ContextCompactionResult {
                messages: vec![ChatMessage::new(
                    ChatRole::User,
                    "Continue the requested text task.",
                )],
                source_version: request.source_version + 1,
                source_fingerprint: request.source_fingerprint,
                usages: Vec::new(),
                usage_complete: false,
            })
        }
        .boxed()
    }
}
