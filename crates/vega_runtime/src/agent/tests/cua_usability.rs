use super::*;

fn call_round(id: &str, name: &str, args: serde_json::Value) -> Vec<ScriptStep> {
    vec![ScriptStep::events(vec![
        ProviderEvent::ToolUse {
            id: id.into(),
            name: name.into(),
            input_json: args.to_string(),
        },
        ProviderEvent::Done {
            stop_reason: StopReason::ToolUse,
        },
    ])]
}

struct RepeatedClickProvider {
    run: crate::computer_use::ComputerUseRun,
    rounds: AtomicUsize,
}

impl crate::Provider for RepeatedClickProvider {
    fn chat_stream(
        &self,
        req: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<crate::EventStream, VegaError>> {
        let index = self.rounds.fetch_add(1, Ordering::SeqCst);
        let (name, args) = match index {
            0 => (
                "ComputerObserve",
                serde_json::json!({"target_id":null,"cursor":null}),
            ),
            1 => (
                "ComputerObserve",
                serde_json::json!({"target_id":"fixture","cursor":null}),
            ),
            _ => {
                let snapshot = self.run.control.snapshot();
                let id = &snapshot.observations.last().unwrap().id;
                (
                    "ComputerAct",
                    serde_json::json!({"observation_id":id,"action":"click","target":null,"text":format!("unused-{index}"),"key":null,"url":null,"x":1,"y":1,"direction":null,"amount":null}),
                )
            }
        };
        MockProvider::new(call_round(&format!("call-{index}"), name, args)).chat_stream(req, cancel)
    }
}
