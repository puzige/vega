use super::*;
use crate::computer_use::{ComputerUseRun, DesktopTarget};
use serde_json::{Value, json};

#[derive(Clone, Copy)]
enum Step {
    Directory,
    Page,
    Observe(u32),
    Type,
    Screenshot(u32),
    Finish,
}

struct ReadProvider {
    run: ComputerUseRun,
    windows: Vec<DesktopTarget>,
    steps: Vec<Step>,
    requests: Arc<Mutex<Vec<ChatRequest>>>,
    targets: Mutex<std::collections::HashMap<u32, String>>,
}

fn values(message: &ChatMessage) -> impl Iterator<Item = Value> + '_ {
    message.content.lines().filter_map(|line| {
        serde_json::from_str(line.strip_prefix("AX elements: ").unwrap_or(line)).ok()
    })
}

impl crate::Provider for ReadProvider {
    fn chat_stream(
        &self,
        req: ChatRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<crate::EventStream, VegaError>> {
        let mut requests = self.requests.lock().unwrap();
        let index = requests.len();
        requests.push(req.clone());
        drop(requests);
        let last = req
            .messages
            .iter()
            .rev()
            .find(|message| message.role == ChatRole::Tool);
        let mut targets = self.targets.lock().unwrap();
        if let Some(last) = last {
            for value in values(last) {
                if let Some(windows) = value["windows"].as_array() {
                    for window in windows {
                        targets.insert(
                            window["window_id"].as_u64().unwrap() as u32,
                            window["target_id"].as_str().unwrap().into(),
                        );
                    }
                }
            }
        }
        let step = self.steps.get(index).copied().unwrap_or(Step::Finish);
        let script = if matches!(step, Step::Finish) {
            vec![
                ScriptStep::text("Already delivered."),
                ScriptStep::events(vec![ProviderEvent::Done {
                    stop_reason: StopReason::End,
                }]),
            ]
        } else {
            let (name, arguments) = match step {
                Step::Directory => ("ComputerObserve", json!({"target_id":null,"cursor":null})),
                Step::Page => {
                    let cursor = last
                        .and_then(|message| {
                            values(message)
                                .find_map(|value| value["next_cursor"].as_str().map(str::to_owned))
                        })
                        .unwrap();
                    ("ComputerObserve", json!({"target_id":null,"cursor":cursor}))
                }
                Step::Observe(window) => (
                    "ComputerObserve",
                    json!({"target_id":targets[&window],"cursor":null}),
                ),
                Step::Screenshot(window) => {
                    let current = last.and_then(|message| {
                        message.content.lines().find_map(|line| {
                            line.strip_prefix("Screenshot target: ")
                                .and_then(|value| serde_json::from_str::<Value>(value).ok())
                                .and_then(|value| value["target_id"].as_str().map(str::to_owned))
                        })
                    });
                    (
                        "ComputerScreenshot",
                        json!({"target_id":current.filter(|id|targets.get(&window)==Some(id)).unwrap_or_else(||targets[&window].clone())}),
                    )
                }
                Step::Type => {
                    let last = last.unwrap();
                    let id = last
                        .content
                        .lines()
                        .find_map(|line| line.strip_prefix("observation_id: "))
                        .unwrap();
                    let token = values(last)
                        .find_map(|value| value["element_token"].as_str().map(str::to_owned))
                        .unwrap();
                    (
                        "ComputerAct",
                        json!({"observation_id":id,"action":"type_text","target":token,"text":" APPENDED"}),
                    )
                }
                Step::Finish => unreachable!(),
            };
            vec![ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: format!("read-{index}"),
                    name: name.into(),
                    input_json: arguments.to_string(),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ])]
        };
        let run = self.run.clone();
        let windows = self
            .windows
            .iter()
            .cloned()
            .map(|mut window| {
                window.selection_id = format!("target-{index}-{}", window.window_id);
                window
            })
            .collect();
        Box::pin(async move {
            if matches!(step, Step::Directory) {
                run.set_desktop_targets_fixture(windows).await;
            }
            MockProvider::new(script).chat_stream(req, cancel).await
        })
    }
}

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

fn frame(window: u32, token: usize, value: &str, caret: Option<u32>) -> vega_mcp::ManagedResult {
    let mut pixels = image::RgbImage::from_pixel(32, 20, image::Rgb([255, 255, 255]));
    if let Some(column) = caret {
        for y in 4..16 {
            pixels.put_pixel(column, y, image::Rgb([0, 0, 0]));
        }
    }
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(pixels)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    vega_mcp::ManagedResult {
        text: Vec::new(),
        images: vec![vega_mcp::ManagedImage {
            bytes: bytes.into_inner(),
            mime_type: "image/png".into(),
        }],
        structured: Some(
            json!({"pid":1,"window_id":window,"screenshot_frame_valid":true,"snapshot_id":format!("snapshot-{token}"),"capture_id":format!("capture-{token}"),"degraded":false,"elements":[{"element_token":format!("edit-{token}"),"role":"AXTextArea","label":"Editor","value":value}],"tree_markdown":value}),
        ),
        is_error: false,
    }
}

fn provider(run: ComputerUseRun, steps: Vec<Step>) -> ReadProvider {
    ReadProvider {
        run,
        windows: vec![window(2)],
        steps,
        requests: Arc::new(Mutex::new(Vec::new())),
        targets: Mutex::new(std::collections::HashMap::new()),
    }
}

fn configured_request(run: ComputerUseRun, root: &std::path::Path) -> AgentRequest {
    let mut req = request(vec![ChatMessage::new(
        ChatRole::User,
        "Append text in the owned window and show its screenshot.",
    )]);
    req.tool_config = tool_config(
        RuntimeRunMode::Execute,
        RuntimePermissionMode::FullAccess,
        root.to_owned(),
    )
    .with_computer_use(Some(run));
    req
}

struct DeliveryCompaction {
    observed: Arc<Mutex<Vec<ChatMessage>>>,
}

impl ContextCompactionHook for DeliveryCompaction {
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
                    "Screenshots already delivered.",
                )],
                source_version: request.source_version + 1,
                source_fingerprint: request.source_fingerprint,
                replay_cold_prefix_messages: None,
                replay_interleaved_user_boundary: None,
                usages: Vec::new(),
                usage_complete: false,
            })
        }
        .boxed()
    }
}
