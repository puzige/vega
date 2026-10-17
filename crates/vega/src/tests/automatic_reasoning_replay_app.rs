use super::*;

struct LocalReplayProvider {
    base_url: String,
    requests: Arc<Mutex<Vec<serde_json::Value>>>,
    authorization: Arc<Mutex<Vec<Option<String>>>>,
    responses_api: bool,
}

impl LocalReplayProvider {
    fn start(responses_api: bool) -> Self {
        Self {
            base_url: "https://replay-fixture.invalid/v1".into(),
            requests: Arc::new(Mutex::new(Vec::new())),
            authorization: Arc::new(Mutex::new(Vec::new())),
            responses_api,
        }
    }

    fn probe(&self) -> crate::app_agent::AgentWorkerStartProbe {
        let requests = self.requests.clone();
        let authorization = self.authorization.clone();
        let responses_api = self.responses_api;
        crate::app_agent::AgentWorkerStartProbe::with_sse_responder(Arc::new(move |request| {
            assert_eq!(
                request.url().path(),
                if responses_api {
                    "/v1/responses"
                } else {
                    "/v1/chat/completions"
                }
            );
            let body = serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).unwrap();
            let mut requests = requests.lock().unwrap();
            let index = requests.len();
            assert!(index < 4, "unexpected provider attempt");
            requests.push(body);
            authorization.lock().unwrap().push(
                request
                    .headers()
                    .get(reqwest::header::AUTHORIZATION)
                    .map(|header| header.to_str().unwrap().to_owned()),
            );
            local_replay_response(responses_api, index)
        }))
    }

    fn finish(self) -> Vec<serde_json::Value> {
        let authorization = self.authorization.lock().unwrap();
        assert_eq!(authorization.len(), 4);
        assert!(
            authorization
                .iter()
                .all(|header| header.as_deref() == Some("Bearer local-replay-fixture-credential"))
        );
        self.requests.lock().unwrap().clone()
    }
}

fn local_replay_response(responses_api: bool, request_index: usize) -> String {
    if responses_api {
        let output = match request_index {
            0 => serde_json::json!([
                {"type":"reasoning","id":"rs-tool-a","summary":[{"type":"summary_text","text":"private tool reasoning one"}]},
                {"type":"function_call","id":"fc-a","call_id":"read-a","name":"read","arguments":"{\"path\":\"replay-a.txt\"}","status":"completed","phase":"commentary"},
                {"type":"message","id":"msg-working-a","role":"assistant","phase":"commentary","status":"completed","content":[{"type":"output_text","text":"Reading the first file."}]}
            ]),
            1 => serde_json::json!([
                {"type":"reasoning","id":"rs-tool-b","summary":[{"type":"summary_text","text":"private tool reasoning two"}]},
                {"type":"function_call","id":"fc-b","call_id":"read-b","name":"read","arguments":"{\"path\":\"replay-b.txt\"}","status":"completed","phase":"commentary"},
                {"type":"message","id":"msg-working-b","role":"assistant","phase":"commentary","status":"completed","content":[{"type":"output_text","text":"Reading the second file."}]}
            ]),
            2 => serde_json::json!([
                {"type":"reasoning","id":"rs-final-1","summary":[{"type":"summary_text","text":"private final reasoning"}]},
                {"type":"message","id":"msg-final-1","role":"assistant","phase":"final","status":"completed","content":[{"type":"output_text","text":"first run complete"}]}
            ]),
            _ => serde_json::json!([
                {"type":"reasoning","id":"rs-final-2","summary":[{"type":"summary_text","text":"private second reasoning"}]},
                {"type":"message","id":"msg-final-2","role":"assistant","phase":"final","status":"completed","content":[{"type":"output_text","text":"second run complete"}]}
            ]),
        };
        let response = serde_json::json!({
            "type":"response.completed",
            "response": {
                "id": format!("resp-{request_index}"),
                "status":"completed",
                "output":output,
                "usage":{"input_tokens":100,"output_tokens":20,"input_tokens_details":{"cached_tokens":30}}
            }
        });
        format!("event: response.completed\ndata: {response}\n\n")
    } else {
        let delta = match request_index {
            0 => serde_json::json!({
                "reasoning_content":"private tool reasoning one",
                "tool_calls":[{"index":0,"id":"read-a","type":"function","function":{"name":"read","arguments":"{\"path\":\"replay-a.txt\"}"}}]
            }),
            1 => serde_json::json!({
                "reasoning_content":"private tool reasoning two",
                "tool_calls":[{"index":0,"id":"read-b","type":"function","function":{"name":"read","arguments":"{\"path\":\"replay-b.txt\"}"}}]
            }),
            2 => serde_json::json!({
                "reasoning_content":"private final reasoning",
                "content":"first run complete"
            }),
            _ => serde_json::json!({
                "reasoning_content":"private second reasoning",
                "content":"second run complete"
            }),
        };
        let finish_reason = if request_index < 2 {
            "tool_calls"
        } else {
            "stop"
        };
        let response = serde_json::json!({
            "choices":[{"delta":delta,"finish_reason":finish_reason}],
            "usage":{"prompt_tokens":100,"completion_tokens":20,"prompt_cache_hit_tokens":30,"prompt_cache_miss_tokens":70}
        });
        format!("data: {response}\n\ndata: [DONE]\n\n")
    }
}

fn run_local_replay_case(cx: &mut gpui_kit::TestAppContext, responses_api: bool) {
    let server = LocalReplayProvider::start(responses_api);
    let config_root = tempfile::tempdir().expect("local replay config root");
    let config_path = config_root.path().join("config.toml");
    let model = if responses_api {
        "gpt-5.6-luna"
    } else {
        "deepseek-v4-pro"
    };
    let api = if responses_api {
        "responses"
    } else {
        "chat_completions"
    };
    fs::write(
        &config_path,
        format!(
            "[[providers]]\napi = \"{api}\"\nname = \"local\"\nbase_url = \"{}\"\nmodels = [\"{model}\"]\nkey_ref = \"local\"\n\n[defaults]\nmodel = \"{model}\"\npermission_mode = \"readonly\"\n\n[ui]\ntheme = \"dark\"\n",
            server.base_url
        ),
    )
    .expect("local replay provider config");
    vega_store::keystore::set_key(
        config_root.path(),
        "local",
        "local-replay-fixture-credential",
    )
    .expect("local replay owner credential");
    let reasoning = vega_store::reasoning::ReasoningConfig {
        version: vega_store::reasoning::REASONING_CONFIG_VERSION,
        profiles: vec![vega_store::reasoning::ReasoningProfile {
            provider: "local".into(),
            model: model.into(),
            protocol: if responses_api {
                "responses_api".into()
            } else {
                "deepseek_chat_completions".into()
            },
            support: if responses_api {
                "unknown".into()
            } else {
                "optional".into()
            },
            efforts: if responses_api {
                Vec::new()
            } else {
                vec!["low".into(), "high".into(), "max".into()]
            },
            supports_disabled: !responses_api,
            disabled_wire: (!responses_api).then(|| "thinking_type_disabled".into()),
            replay_profile: if responses_api {
                "responses_items".into()
            } else {
                "deepseek_tools".into()
            },
            preference: if responses_api {
                "provider_default".into()
            } else {
                "high".into()
            },
        }],
    };
    fs::write(
        config_root.path().join("reasoning.toml"),
        vega_store::reasoning::encode(&reasoning).expect("encode local replay profile"),
    )
    .expect("local replay reasoning config");

    let data_root = tempfile::tempdir().expect("local replay data root");
    let _git = vega_conversation::register_git_test_executor(
        data_root.path(),
        Arc::new(|command, _, _| {
            let args = command
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            assert!(args.ends_with(&["rev-parse".into(), "--show-toplevel".into()]));
            Err(vega_conversation::types::GitWorkspaceError::for_test(
                vega_conversation::types::GitWorkspaceErrorCode::NotRepository,
            ))
        }),
    )
    .expect("local replay Git fixture");
    fs::write(data_root.path().join("replay-a.txt"), "first file content")
        .expect("first local replay file");
    fs::write(data_root.path().join("replay-b.txt"), "second file content")
        .expect("second local replay file");
    let database_path = data_root.path().join("vega.db");
    let store = Store::open(&database_path).expect("local replay store");
    store.migrate().expect("local replay migrations");
    let project = vega_store::projects::create(
        store.conn(),
        data_root.path().to_str().expect("UTF-8 local replay path"),
        "automatic-replay",
        None,
    )
    .expect("local replay project");
    let created = vega_conversation::threads::create_thread(
        &store,
        &project.id,
        model,
        PermissionMode::ReadOnly.as_str(),
    )
    .expect("local replay thread");
    let thread =
        vega_conversation::threads::rename_thread(&store, &created.id, "Automatic replay fixture")
            .expect("pre-title local replay thread");
    let registry = vega_conversation::agent::ReplaySessionRegistry::default();
    cx.update(|cx| {
        install_diff_window_globals(
            Store::open(&database_path).expect("local replay global store"),
            thread.clone(),
            cx,
        );
        cx.set_global(crate::app_agent::AppReplaySessions(registry.clone()));
    });
    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, cx| {
        root.model_selection_config_override = Some(config_path.clone());
        root.stream_view = Some((thread.id.clone(), stream.clone()));
        root.start_model_catalog_load(cx);
    });
    let window_root = root.clone();
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), move |_, _| window_root)
            .expect("local replay app window")
    });
    root.update(cx, |root, _| {
        root.agent_worker_start_probe = Arc::new(server.probe())
    });
    let worker_probe = root.read_with(cx, |root, _| root.agent_worker_start_probe.clone());
    pump_test_app(cx, |cx| {
        root.read_with(cx, |root, _| {
            !root.model_catalog_loading
                && root.configured_reasoning_authority.is_some()
                && root.reasoning_profile_for_model(model).is_some()
        }) && stream.read_with(cx, |stream, _| {
            stream.reasoning_profile().is_some() && !stream.reasoning_unavailable()
        })
    });

    root.update(cx, |root, cx| {
        root.start_agent_run(
            stream.clone(),
            &thread.id,
            PendingAgentRun::UserMessage(
                "Read replay-a.txt and replay-b.txt, then summarize both.".into(),
            ),
            cx,
        );
    });
    let observer = Store::open(&database_path).expect("local replay observer");
    pump_test_app_with_diagnostics(
        cx,
        |cx| {
            root.read_with(cx, |root, _| root.agent_controller.active.is_empty())
                && server
                    .requests
                    .lock()
                    .is_ok_and(|requests| requests.len() == 3)
                && observer.conn().query_row(
                    "SELECT COUNT(*) FROM messages WHERE thread_id=?1 AND role='assistant' AND status='done'",
                    [&thread.id],
                    |row| row.get::<_, i64>(0),
                ).is_ok_and(|count| count == 1)
        },
        |_| "first local replay worker did not finish".to_string(),
    );
    let durable_tools = vega_store::tool_calls::terminal_results(observer.conn(), &thread.id)
        .expect("first replay tool audit");
    assert_eq!(durable_tools.len(), 2);
    assert_eq!(
        durable_tools.get("read-a").map(|call| call.output.as_str()),
        Some("1 | first file content")
    );
    assert_eq!(
        durable_tools.get("read-b").map(|call| call.output.as_str()),
        Some("1 | second file content")
    );
    let tool_starts = worker_probe.tool_start_counts();
    assert_eq!(tool_starts.len(), 2);
    assert_eq!(tool_starts.get("read-a"), Some(&1_usize));
    assert_eq!(tool_starts.get("read-b"), Some(&1_usize));

    root.update(cx, |root, cx| {
        root.start_agent_run(
            stream.clone(),
            &thread.id,
            PendingAgentRun::UserMessage("Continue with the two file results.".into()),
            cx,
        );
    });
    pump_test_app_with_diagnostics(
        cx,
        |cx| {
            root.read_with(cx, |root, _| root.agent_controller.active.is_empty())
                && server
                    .requests
                    .lock()
                    .is_ok_and(|requests| requests.len() == 4)
                && observer.conn().query_row(
                    "SELECT COUNT(*) FROM messages WHERE thread_id=?1 AND role='assistant' AND status='done'",
                    [&thread.id],
                    |row| row.get::<_, i64>(0),
                ).is_ok_and(|count| count == 2)
        },
        |_| "second local replay worker did not finish".to_string(),
    );
    let requests = server.finish();
    assert_eq!(requests.len(), 4);
    let tool_starts = worker_probe.tool_start_counts();
    assert_eq!(tool_starts.len(), 2);
    assert_eq!(tool_starts.get("read-a"), Some(&1_usize));
    assert_eq!(tool_starts.get("read-b"), Some(&1_usize));
    if responses_api {
        assert_eq!(requests[0]["model"], model);
        for (request, expected_ids) in [
            (&requests[1], &["rs-tool-a", "fc-a", "msg-working-a"][..]),
            (
                &requests[2],
                &[
                    "rs-tool-a",
                    "fc-a",
                    "msg-working-a",
                    "rs-tool-b",
                    "fc-b",
                    "msg-working-b",
                ][..],
            ),
        ] {
            let input = request["input"]
                .as_array()
                .expect("Responses continuation input");
            let positions = expected_ids
                .iter()
                .map(|id| {
                    input
                        .iter()
                        .position(|item| item["id"] == *id)
                        .expect("completed response item remains in continuation")
                })
                .collect::<Vec<_>>();
            assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
            assert!(input.iter().any(|item| {
                item["type"] == "reasoning"
                    && item["summary"][0]["text"] == "private tool reasoning one"
            }));
            if request["input"]
                .as_array()
                .is_some_and(|items| items.iter().any(|item| item["id"] == "msg-working-b"))
            {
                assert!(input.iter().any(|item| {
                    item["type"] == "reasoning"
                        && item["summary"][0]["text"] == "private tool reasoning two"
                }));
            }
        }
        let input = requests[3]["input"].as_array().expect("Responses input");
        let item_positions = [
            "rs-tool-a",
            "fc-a",
            "msg-working-a",
            "rs-tool-b",
            "fc-b",
            "msg-working-b",
            "rs-final-1",
            "msg-final-1",
        ]
        .map(|id| {
            input
                .iter()
                .position(|item| item["id"] == id)
                .expect("replayed raw item")
        });
        assert!(item_positions.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(input[item_positions[2]]["phase"], "commentary");
        assert_eq!(input[item_positions[1]]["phase"], "commentary");
        assert_eq!(input[item_positions[5]]["phase"], "commentary");
        assert_eq!(input[item_positions[4]]["phase"], "commentary");
        assert_eq!(input[item_positions[7]]["phase"], "final");
        assert!(
            input
                .iter()
                .all(|item| { item["id"] != "rs-final-2" && item["id"] != "msg-final-2" })
        );
        for (item_position, text) in [
            (item_positions[0], "private tool reasoning one"),
            (item_positions[3], "private tool reasoning two"),
            (item_positions[6], "private final reasoning"),
        ] {
            assert_eq!(input[item_position]["summary"][0]["text"], text);
        }
        assert!(input.iter().any(|item| {
            item["type"] == "function_call_output"
                && item["call_id"] == "read-a"
                && item["output"]
                    .as_str()
                    .is_some_and(|value| value.contains("first file content"))
        }));
        assert!(input.iter().any(|item| {
            item["type"] == "function_call_output"
                && item["call_id"] == "read-b"
                && item["output"]
                    .as_str()
                    .is_some_and(|value| value.contains("second file content"))
        }));
        let current_user = input
            .last()
            .expect("current user follows the replayed prefix");
        assert_eq!(current_user["role"], "user");
        assert_eq!(
            current_user["content"],
            serde_json::json!([{
                "type":"input_text", "text":"Continue with the two file results."
            }])
        );
    } else {
        assert_eq!(requests[0]["model"], model);
        let first_continuation = requests[1]["messages"]
            .as_array()
            .expect("first Chat continuation");
        assert!(first_continuation.iter().any(|message| {
            message["reasoning_content"] == "private tool reasoning one"
                && message["tool_calls"][0]["id"] == "read-a"
        }));
        let second_continuation = requests[2]["messages"]
            .as_array()
            .expect("second Chat continuation");
        for (reasoning, call_id) in [
            ("private tool reasoning one", "read-a"),
            ("private tool reasoning two", "read-b"),
        ] {
            assert!(second_continuation.iter().any(|message| {
                message["reasoning_content"] == reasoning
                    && message["tool_calls"][0]["id"] == call_id
            }));
        }
        let messages = requests[3]["messages"].as_array().expect("Chat messages");
        let tool_assistant_one = messages
            .iter()
            .find(|message| message["reasoning_content"] == "private tool reasoning one")
            .expect("first raw tool-call reasoning replay");
        assert_eq!(tool_assistant_one["tool_calls"][0]["id"], "read-a");
        let tool_assistant_two = messages
            .iter()
            .find(|message| message["reasoning_content"] == "private tool reasoning two")
            .expect("second raw tool-call reasoning replay");
        assert_eq!(tool_assistant_two["tool_calls"][0]["id"], "read-b");
        assert!(messages.iter().any(|message| {
            message["role"] == "tool"
                && message["tool_call_id"] == "read-a"
                && message["content"]
                    .as_str()
                    .is_some_and(|value| value.contains("first file content"))
        }));
        assert!(messages.iter().any(|message| {
            message["role"] == "tool"
                && message["tool_call_id"] == "read-b"
                && message["content"]
                    .as_str()
                    .is_some_and(|value| value.contains("second file content"))
        }));
        assert!(
            messages
                .iter()
                .any(|message| message["reasoning_content"] == "private final reasoning")
        );
        assert!(
            messages
                .iter()
                .all(|message| message["reasoning_content"] != "private second reasoning")
        );
        assert!(messages.iter().any(|message| {
            message["role"] == "user" && message["content"] == "Continue with the two file results."
        }));
    }
    let latest_assistant =
        vega_store::messages::last_terminal_assistant(observer.conn(), &thread.id)
            .expect("latest replay assistant row")
            .expect("latest replay assistant exists");
    assert!(latest_assistant.content.contains("second run complete"));
    let assistant_ids = {
        let mut statement = observer
            .conn()
            .prepare(
                "SELECT id FROM messages WHERE thread_id=?1 AND role='assistant' AND status='done' ORDER BY seq",
            )
            .expect("replay assistant list statement");
        statement
            .query_map([&thread.id], |row| row.get::<_, String>(0))
            .expect("replay assistant list")
            .collect::<Result<Vec<_>, _>>()
            .expect("replay assistant ids")
    };
    assert_eq!(assistant_ids.len(), 2);
    let first_usage = vega_store::token_usage::aggregate_by_message(
        observer.conn(),
        &thread.id,
        &assistant_ids[0],
    )
    .expect("first durable replay usage");
    assert_eq!(first_usage.row_count, 3);
    assert_eq!(first_usage.input_tokens, 300);
    assert_eq!(first_usage.output_tokens, 60);
    assert_eq!(first_usage.cache_read_tokens, 90);
    let usage = vega_store::token_usage::aggregate_by_message(
        observer.conn(),
        &thread.id,
        &latest_assistant.id,
    )
    .expect("durable replay usage");
    assert_eq!(usage.row_count, 1);
    assert_eq!(usage.input_tokens, 100);
    assert_eq!(usage.output_tokens, 20);
    assert_eq!(usage.cache_read_tokens, 30);
    let first_summary = vega_conversation::summary::task_cost_summary(
        &observer,
        &thread.id,
        &assistant_ids[0],
        None,
    )
    .expect("first durable replay task summary");
    assert_eq!(first_summary.usage.map(|usage| usage.input), Some(300));
    assert_eq!(first_summary.usage.map(|usage| usage.output), Some(60));
    assert_eq!(
        first_summary.usage.map(|usage| usage.cache_read),
        Some(Some(90))
    );
    assert_eq!(
        first_summary.usage.map(|usage| usage.cache_write),
        Some(None)
    );
    let summary = vega_conversation::summary::task_cost_summary(
        &observer,
        &thread.id,
        &latest_assistant.id,
        None,
    )
    .expect("durable replay task summary");
    assert_eq!(summary.usage.map(|usage| usage.cache_read), Some(Some(30)));
    assert_eq!(summary.usage.map(|usage| usage.cache_write), Some(None));
    if responses_api {
        assert_eq!(
            summary.cost,
            vega_conversation::types::SummaryCost::Unavailable
        );
    } else {
        assert!(matches!(
            summary.cost,
            vega_conversation::types::SummaryCost::Priced(_)
        ));
    }
    window
        .update(cx, |_, window, _| window.remove_window())
        .expect("close local replay window");
}

#[gpui_kit::test]
async fn automatic_replay_app_workers_capture_chat_and_responses_wire_bodies(
    cx: &mut gpui_kit::TestAppContext,
) {
    run_local_replay_case(cx, false);
    run_local_replay_case(cx, true);
}
