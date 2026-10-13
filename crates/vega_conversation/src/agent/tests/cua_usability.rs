use super::*;
use vega_runtime::computer_use::{ComputerUseControl, test_support};

fn picture() -> crate::types::ImageAttachment {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(4, 3)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    crate::types::ImageAttachment::from_bytes(bytes.into_inner()).unwrap()
}

fn prepare(store: &Store) -> (ComputerUseControl, u64) {
    store.conn().execute_batch("INSERT INTO messages (id,thread_id,seq,role,kind,content,status,created_at) VALUES ('capture-owner','thread-1',1,'assistant','text','Screenshot attached.','streaming',1);").unwrap();
    tool_calls::insert(store.conn(), tool_calls::NewToolCall { id:"capture", thread_id:"thread-1", message_id:"capture-owner", seq:1, tool:"ComputerScreenshot", input_json:&serde_json::json!({"kind":"computer_use","version":1,"action":"screenshot","argument_bytes":2,"argument_digest":"a".repeat(64)}).to_string(), status:"running", created_at:1 }).unwrap();
    let control = test_support::connected_desktop().unwrap();
    let epoch = control.status().epoch;
    (control, epoch)
}

fn persist(store: &Store, project: &str, event: &RuntimeEvent) -> Result<(), VegaError> {
    persist_runtime_event(
        store,
        project,
        "thread-1",
        "capture-owner",
        "mock-model",
        false,
        "Screenshot attached.",
        &mut 2,
        event,
        None,
    )
}

#[tokio::test]
async fn issue75_usability_delivery_is_idempotent_reloadable_and_not_resent_to_model() {
    let (store, dir, project) = setup();
    let (control, epoch) = prepare(&store);
    let image = picture();
    let event = RuntimeEvent::ScreenshotDelivered {
        call_id: "capture".into(),
        image: image.clone(),
        control: control.clone(),
        epoch,
        cancel: CancellationToken::new(),
    };
    persist(&store, &project, &event).unwrap();
    persist(&store, &project, &event).unwrap();
    assert_eq!(
        store
            .conn()
            .query_row(
                "SELECT count(*) FROM computer_screenshot_artifacts",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    assert!(
        vega_store::computer_use_delivery::insert_screenshot(
            store.conn(),
            "wrong-thread",
            "capture-owner",
            "capture",
            image.bytes()
        )
        .is_err()
    );
    assert!(
        vega_store::image_attachments::insert(store.conn(), "capture-owner", 1, image.bytes())
            .is_err()
    );
    store
        .conn()
        .execute_batch("UPDATE messages SET status='done' WHERE id='capture-owner';")
        .unwrap();
    tool_calls::update(
        store.conn(),
        "capture",
        "success",
        Some(r#"{"decision":"once","note":null,"source":"computer_use","danger":null}"#),
        Some("delivered"),
        Some(2),
    )
    .unwrap();
    control.stop();
    test_support::finish_cleanup(&control);
    let path = dir.path().join("vega.db");
    drop(store);
    let store = Store::open(&path).unwrap();
    store.migrate().unwrap();
    let page = crate::history::latest_history_page(&store, "thread-1", 50).unwrap();
    assert_eq!(page.entries.iter().filter(|entry| matches!(entry, crate::history::HistoryEntry::AssistantScreenshot { call_id, image:delivered,.. } if call_id == "capture" && delivered == &image)).count(), 1);
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let provider = MockProvider::new(vec![
        ScriptStep::text("next"),
        ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }]),
    ]);
    run_thread_task(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Next question",
        "",
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(
        provider.requests()[0]
            .messages
            .iter()
            .all(|message| message.images.is_empty())
    );
    store
        .conn()
        .execute("DELETE FROM messages WHERE id='capture-owner'", [])
        .unwrap();
    for table in ["computer_screenshot_artifacts", "image_attachments"] {
        assert_eq!(
            store
                .conn()
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}

#[test]
fn issue75_usability_delivery_failure_cancel_and_epoch_are_atomic() {
    for condition in ["db-failure", "cancel", "paused", "resumed"] {
        let (store, _dir, project) = setup();
        let (control, epoch) = prepare(&store);
        let cancel = CancellationToken::new();
        match condition {
            "db-failure" => store.conn().execute_batch("CREATE TRIGGER reject_artifact BEFORE INSERT ON computer_screenshot_artifacts BEGIN SELECT RAISE(ABORT,'owned fault'); END;").unwrap(),
            "cancel" => cancel.cancel(),
            "paused" => { control.pause(); },
            "resumed" => { control.pause(); test_support::finish_cleanup(&control); control.resume(control.status().epoch).unwrap(); },
            _ => unreachable!(),
        }
        let event = RuntimeEvent::ScreenshotDelivered {
            call_id: "capture".into(),
            image: picture(),
            control: control.clone(),
            epoch,
            cancel,
        };
        assert!(persist(&store, &project, &event).is_err(), "{condition}");
        for table in ["image_attachments", "computer_screenshot_artifacts"] {
            assert_eq!(
                store
                    .conn()
                    .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row
                        .get::<_, i64>(0))
                    .unwrap(),
                0,
                "{condition}"
            );
        }
        control.stop();
        test_support::finish_cleanup(&control);
    }
}

#[test]
fn issue75_usability_screenshot_limits_activation_audit_and_explicit_save() {
    let (store, dir, project) = setup();
    let (control, epoch) = prepare(&store);
    let activation = vega_runtime::computer_use::guide::BuiltinSkillActivation {
        name: "vega-cua".into(),
        version: 1,
        content_sha256: "a".repeat(64),
    };
    persist(
        &store,
        &project,
        &RuntimeEvent::BuiltinSkillActivated(activation.clone()),
    )
    .unwrap();
    persist(
        &store,
        &project,
        &RuntimeEvent::BuiltinSkillActivated(activation),
    )
    .unwrap();
    assert_eq!(
        store
            .conn()
            .query_row(
                "SELECT count(*) FROM builtin_skill_activations",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    let image = picture();
    for index in 0..5 {
        let id = format!("capture-{index}");
        tool_calls::insert(
            store.conn(),
            tool_calls::NewToolCall {
                id: &id,
                thread_id: "thread-1",
                message_id: "capture-owner",
                seq: index + 2,
                tool: "ComputerScreenshot",
                input_json: "{}",
                status: "running",
                created_at: 1,
            },
        )
        .unwrap();
        let event = RuntimeEvent::ScreenshotDelivered {
            call_id: id,
            image: image.clone(),
            control: control.clone(),
            epoch,
            cancel: CancellationToken::new(),
        };
        assert_eq!(persist(&store, &project, &event).is_ok(), index < 4);
    }
    assert_eq!(
        store
            .conn()
            .query_row("SELECT count(*) FROM image_attachments", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        4
    );
    let path = dir.path().join("saved.png");
    crate::attachments::save_screenshot(&image, &path).unwrap();
    assert_eq!(fs::read(&path).unwrap(), image.bytes());
    assert!(crate::attachments::save_screenshot(&image, &path).is_err());
    control.stop();
    test_support::finish_cleanup(&control);
}

#[tokio::test]
async fn issue75_readonly_permissions_screenshot_delivers_without_ax_through_persistence() {
    let (store, dir, _) = setup();
    let run = vega_runtime::computer_use::ComputerUseRun::new(true, false);
    let image = picture();
    let actions = test_support::install_desktop(
        &run,
        vec![vega_runtime::computer_use::DesktopTarget {
            selection_id: "fixture".into(),
            application: "Fixture".into(),
            bundle_id: "test.fixture".into(),
            pid: 1,
            window_id: 2,
            title: "Owned test".into(),
        }],
        vec![vega_mcp::ManagedResult {
            text: Vec::new(),
            images: vec![vega_mcp::ManagedImage { bytes: image.bytes().to_vec(), mime_type: "image/png".into() }],
            structured: Some(serde_json::json!({"pid":1,"window_id":2,"screenshot_frame_valid":true,"snapshot_id":"snapshot","capture_id":"capture","elements":[],"degraded":false})),
            is_error: false,
        }],
        vec![serde_json::json!({"accessibility":false,"screen_recording":true})],
    ).await;
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "directory".into(),
                name: "ComputerObserve".into(),
                input_json: r#"{"target_id":null,"cursor":null}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "capture-no-ax".into(),
                name: "ComputerScreenshot".into(),
                input_json: r#"{"target_id":"fixture"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![
            ScriptStep::text("Screenshot attached."),
            ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }]),
        ],
    ]);
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    run_thread_task_with_images_and_reasoning(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Screenshot Fixture",
        "",
        CancellationToken::new(),
        &RejectPermissionHook,
        |_| Ok(()),
        PersistenceActorConfig {
            computer_use: Some(run.clone()),
            ..Default::default()
        },
        None,
        None,
        None,
        Vec::new(),
    )
    .await
    .unwrap();
    assert_eq!(provider.requests().len(), 3);
    assert_eq!(actions.load(Ordering::SeqCst), 0);
    let calls = test_support::desktop_calls(&run).await;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "get_window_state");
    assert_eq!(calls[0].1["include_accessibility_tree"], false);
    assert_eq!(calls[0].1["pid"], 1);
    assert_eq!(calls[0].1["window_id"], 2);
    assert_eq!(
        store
            .conn()
            .query_row(
                "SELECT count(*) FROM computer_screenshot_artifacts",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    assert_eq!(
        store
            .conn()
            .query_row(
                "SELECT status FROM tool_calls WHERE id='capture-no-ax'",
                [],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
        "success"
    );
    assert!(
        provider.requests()[2]
            .messages
            .iter()
            .any(|message| message.role == vega_runtime::ChatRole::Tool
                && message.content.contains("conversation_image_attachment"))
    );
    let path = dir.path().join("vega.db");
    drop(store);
    let reopened = Store::open(path).unwrap();
    reopened.migrate().unwrap();
    let history = crate::history::latest_history_page(&reopened, "thread-1", 50).unwrap();
    assert_eq!(history.entries.iter().filter(|entry| matches!(entry, crate::history::HistoryEntry::AssistantScreenshot {call_id,image:found,..} if call_id=="capture-no-ax" && found==&image)).count(), 1);
}

#[tokio::test]
async fn issue75_readonly_permissions_denial_remains_rejected_in_store_and_history() {
    for (name, args, ax, sr, code) in [
        (
            "ComputerObserve",
            serde_json::json!({"target_id":null,"cursor":null}),
            true,
            false,
            "screen_recording_permission_missing",
        ),
        (
            "ComputerObserve",
            serde_json::json!({"target_id":"fixture","cursor":null}),
            false,
            true,
            "accessibility_permission_missing",
        ),
        (
            "ComputerObserve",
            serde_json::json!({"target_id":"fixture","cursor":null}),
            false,
            false,
            "permissions_missing",
        ),
    ] {
        let (store, dir, _) = setup();
        let run = vega_runtime::computer_use::ComputerUseRun::new(true, false);
        test_support::install_desktop(
            &run,
            Vec::new(),
            Vec::new(),
            vec![serde_json::json!({"accessibility":ax,"screen_recording":sr})],
        )
        .await;
        let provider = MockProvider::new_rounds(vec![vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "permission-denied".into(),
                name: name.into(),
                input_json: args.to_string(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])]]);
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        run_thread_task_with_images_and_reasoning(
            &store,
            &provider,
            &tools,
            "thread-1",
            "Screenshot Fixture",
            "",
            CancellationToken::new(),
            &RejectPermissionHook,
            |_| Ok(()),
            PersistenceActorConfig {
                computer_use: Some(run),
                ..Default::default()
            },
            None,
            None,
            None,
            Vec::new(),
        )
        .await
        .unwrap();
        let (status, approval, output): (String, String, String) = store
            .conn()
            .query_row(
                "SELECT status,approval,output_text FROM tool_calls WHERE id='permission-denied'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(status, "rejected");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&approval).unwrap()["source"],
            "computer_use"
        );
        assert!(output.contains(code), "{output}");
        assert_eq!(
            store
                .conn()
                .query_row(
                    "SELECT count(*) FROM computer_screenshot_artifacts",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        let path = dir.path().join("vega.db");
        drop(store);
        let reopened = Store::open(path).unwrap();
        reopened.migrate().unwrap();
        let page = crate::history::latest_history_page(&reopened, "thread-1", 50).unwrap();
        assert!(page.entries.iter().any(|entry|matches!(entry,crate::history::HistoryEntry::Tool{status:ToolCallStatus::Rejected,result:Some(crate::types::ToolCardResultProjection::ComputerUse{status:ToolCallStatus::Rejected,output,..}),..} if output.contains(code))));
    }
}

#[test]
fn issue75_readonly_permissions_cua_denial_codec_rejects_unrelated_authority() {
    for (decision, note, danger, valid) in [
        (
            "deny",
            serde_json::Value::Null,
            serde_json::Value::Null,
            true,
        ),
        (
            "always",
            serde_json::Value::Null,
            serde_json::Value::Null,
            false,
        ),
        (
            "deny",
            serde_json::json!("invented"),
            serde_json::Value::Null,
            false,
        ),
        (
            "deny",
            serde_json::Value::Null,
            serde_json::json!({"rule_id":"invented","decision":"deny","note":null}),
            false,
        ),
    ] {
        let raw=serde_json::json!({"decision":decision,"note":note,"source":"computer_use","danger":danger}).to_string();
        assert_eq!(
            crate::types::ApprovalAudit::from_json(&raw).is_ok(),
            valid,
            "{raw}"
        );
    }
    assert!(!approval_source_matches(
        "read",
        RuntimeToolStatus::Rejected,
        crate::types::ApprovalSource::ComputerUse,
        false
    ));
    assert!(!approval_source_matches(
        "write",
        RuntimeToolStatus::Rejected,
        crate::types::ApprovalSource::ComputerUse,
        false
    ));
}

#[tokio::test]
async fn issue75_delivery_progress_batched_reuse_preserves_private_model_inputs_and_safe_store() {
    let (store, dir, _) = setup();
    let run = vega_runtime::computer_use::ComputerUseRun::new(true, false);
    let image = picture();
    let windows = (2..4)
        .map(|id| vega_runtime::computer_use::DesktopTarget {
            selection_id: format!("private-target-{id}"),
            application: "Private Fixture".into(),
            bundle_id: "test.private.fixture".into(),
            pid: 1,
            window_id: id,
            title: format!("Private Owned {id}"),
        })
        .collect();
    let frames = [2, 3, 2, 3]
        .into_iter()
        .enumerate()
        .map(|(index, id)| vega_mcp::ManagedResult {
            text: Vec::new(),
            images: vec![vega_mcp::ManagedImage {
                bytes: image.bytes().to_vec(),
                mime_type: "image/png".into(),
            }],
            structured: Some(
                serde_json::json!({"pid":1,"window_id":id,"screenshot_frame_valid":true,
            "snapshot_id":format!("snapshot-{index}"),"capture_id":format!("capture-{index}"),
            "elements":[],"degraded":false}),
            ),
            is_error: false,
        })
        .collect();
    let actions = test_support::install_desktop(
        &run,
        windows,
        frames,
        vec![serde_json::json!({"accessibility":false,"screen_recording":true})],
    )
    .await;
    let batch = |number| {
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: format!("shot-a-{number}"),
                name: "ComputerScreenshot".into(),
                input_json: r#"{"target_id":"private-target-2"}"#.into(),
            },
            ProviderEvent::ToolUse {
                id: format!("shot-b-{number}"),
                name: "ComputerScreenshot".into(),
                input_json: r#"{"target_id":"private-target-3"}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])]
    };
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "private-directory".into(),
                name: "ComputerObserve".into(),
                input_json: r#"{"target_id":null,"cursor":null}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        batch(1),
        batch(2),
        vec![
            ScriptStep::text("Attached."),
            ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }]),
        ],
    ]);
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    run_thread_task_with_images_and_reasoning(
        &store,
        &provider,
        &tools,
        "thread-1",
        "Screenshot the fixtures",
        "",
        CancellationToken::new(),
        &RejectPermissionHook,
        |_| Ok(()),
        PersistenceActorConfig {
            computer_use: Some(run.clone()),
            ..Default::default()
        },
        None,
        None,
        None,
        Vec::new(),
    )
    .await
    .unwrap();
    assert_eq!(actions.load(Ordering::SeqCst), 0);
    assert_eq!(provider.requests().len(), 4);
    assert_eq!(test_support::desktop_calls(&run).await.len(), 4);
    for table in ["image_attachments", "computer_screenshot_artifacts"] {
        assert_eq!(
            store
                .conn()
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            2
        );
    }
    let rows = store
        .conn()
        .prepare("SELECT input_json,output_text,status FROM tool_calls ORDER BY seq")
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(rows.len(), 5);
    for (input, output, status) in rows {
        assert!(vega_runtime::computer_use::ComputerUseAudit::valid(&input));
        assert_eq!(status, "success");
        for secret in [
            "private-target",
            "Private Owned",
            "Private Fixture",
            "window_id",
            "bundle_id",
        ] {
            assert!(!input.contains(secret));
            assert!(!output.contains(secret), "{output}");
        }
    }
    for (id, target, artifact) in [
        ("shot-a-2", "private-target-2", "shot-a-1"),
        ("shot-b-2", "private-target-3", "shot-b-1"),
    ] {
        let requests = provider.requests();
        let request = requests.last().unwrap();
        let call = request
            .messages
            .iter()
            .flat_map(|message| &message.tool_calls)
            .find(|call| call.id == id)
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&call.input_json).unwrap(),
            serde_json::json!({"target_id":target})
        );
        let message = request
            .messages
            .iter()
            .find(|message| message.tool_call_id.as_deref() == Some(id))
            .unwrap();
        let receipt: serde_json::Value =
            serde_json::from_str(message.content.lines().next().unwrap()).unwrap();
        assert_eq!(receipt["artifact_id"], format!("screenshot:{artifact}"));
        assert_eq!(receipt["unchanged"], true);
    }
    let path = dir.path().join("vega.db");
    drop(store);
    let store = Store::open(path).unwrap();
    store.migrate().unwrap();
    let page = crate::history::latest_history_page(&store, "thread-1", 50).unwrap();
    let images = page
        .entries
        .iter()
        .filter_map(|entry| match entry {
            crate::history::HistoryEntry::AssistantScreenshot { call_id, image, .. } => {
                Some((call_id.as_str(), image))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(images.len(), 2);
    assert_eq!(images[0], ("shot-a-1", &image));
    assert_eq!(images[1], ("shot-b-1", &image));
}

#[tokio::test]
async fn issue75_resume_refresh_approved_superseded_call_persists_and_reloads_neutrally() {
    for boundary in ["proposed", "approved", "running"] {
        let (store, dir, _) = setup();
        let run = vega_runtime::computer_use::ComputerUseRun::new(true, false);
        test_support::install_desktop(
            &run,
            vec![vega_runtime::computer_use::DesktopTarget {
                selection_id: "private-target".into(),
                application: "Fixture".into(),
                bundle_id: "test.fixture".into(),
                pid: 1,
                window_id: 2,
                title: "Owned".into(),
            }],
            Vec::new(),
            vec![serde_json::json!({"accessibility":true,"screen_recording":true})],
        )
        .await;
        let call = |id: &str, name: &str, args: &str| {
            vec![ScriptStep::events(vec![
                ProviderEvent::ToolUse {
                    id: id.into(),
                    name: name.into(),
                    input_json: args.into(),
                },
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ])]
        };
        let provider = MockProvider::new_rounds(vec![
            call(
                "directory",
                "ComputerObserve",
                r#"{"target_id":null,"cursor":null}"#,
            ),
            call(
                "superseded",
                "ComputerScreenshot",
                r#"{"target_id":"private-target"}"#,
            ),
            vec![
                ScriptStep::text("The pending capture was withdrawn after takeover."),
                ScriptStep::events(vec![ProviderEvent::Done {
                    stop_reason: StopReason::End,
                }]),
            ],
        ]);
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let control = run.control.clone();
        run_thread_task_with_images_and_reasoning(
            &store,
            &provider,
            &tools,
            "thread-1",
            "Capture the owned fixture",
            "",
            CancellationToken::new(),
            &RejectPermissionHook,
            move |event| {
                let pause = match &event {
                    ConversationEvent::ToolCallProposed { call } => {
                        boundary == "proposed" && call.id == "superseded"
                    }
                    ConversationEvent::ToolCallApproved { call_id, .. } => {
                        boundary == "approved" && call_id == "superseded"
                    }
                    ConversationEvent::ToolCallRunning { call_id } => {
                        boundary == "running" && call_id == "superseded"
                    }
                    _ => false,
                };
                if pause {
                    control.pause();
                    test_support::finish_cleanup(&control);
                    control.resume(control.status().epoch).unwrap();
                }
                Ok(())
            },
            PersistenceActorConfig {
                computer_use: Some(run.clone()),
                ..Default::default()
            },
            None,
            None,
            None,
            Vec::new(),
        )
        .await
        .unwrap();
        assert!(test_support::desktop_calls(&run).await.is_empty());
        let state = vega_store::tool_calls::find_state(store.conn(), "superseded")
            .unwrap()
            .unwrap();
        assert_eq!(state.status, "cancelled");
        assert!(!state.input_json.contains("private-target"));
        assert_eq!(
            store
                .conn()
                .query_row(
                    "SELECT output_text FROM tool_calls WHERE id='superseded'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            vega_runtime::computer_use::SUPERSEDED_CONTROL_OUTPUT
        );
        assert_eq!(store.conn().query_row("SELECT status FROM messages WHERE id=(SELECT message_id FROM tool_calls WHERE id='superseded')",[],|row|row.get::<_,String>(0)).unwrap(),"done");
        assert_eq!(
            store
                .conn()
                .query_row(
                    "SELECT count(*) FROM computer_screenshot_artifacts",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        let requests = provider.requests();
        assert_eq!(requests.len(), 3);
        let result = requests[2]
            .messages
            .iter()
            .find(|message| message.tool_call_id.as_deref() == Some("superseded"))
            .unwrap();
        assert_eq!(
            result.content,
            vega_runtime::computer_use::SUPERSEDED_CONTROL_OUTPUT
        );
        assert!(result.images.is_empty());
        let path = dir.path().join("vega.db");
        drop(store);
        let store = Store::open(path).unwrap();
        store.migrate().unwrap();
        let page = crate::history::latest_history_page(&store, "thread-1", 50).unwrap();
        assert!(page.entries.iter().any(
            |entry| matches!(entry,crate::history::HistoryEntry::Tool {
        call_id,status:ToolCallStatus::Cancelled,approval:Some(crate::types::Approval::Once),
        result:Some(crate::types::ToolCardResultProjection::ComputerUse {output,..}),..
    } if call_id=="superseded" && output==vega_runtime::computer_use::SUPERSEDED_CONTROL_OUTPUT)
        ));
    }
}

#[test]
fn issue75_resume_refresh_receipt_requires_exact_cancelled_cua_approval() {
    use crate::types::{
        Approval, ApprovalAudit, ApprovalSource, ToolCall, ToolCardResultProjection, ToolResult,
        tool_card_input_projection, tool_card_result_projection,
    };
    let input=serde_json::json!({"kind":"computer_use","version":1,"action":"screenshot","argument_bytes":2,"argument_digest":"a".repeat(64)}).to_string();
    let approval = ApprovalAudit {
        decision: Approval::Once,
        note: None,
        source: ApprovalSource::ComputerUse,
        danger: None,
    };
    let receipt = vega_runtime::computer_use::SUPERSEDED_CONTROL_OUTPUT;
    let call = ToolCall {
        id: "capture".into(),
        tool: "ComputerScreenshot".into(),
        input_json: input.clone(),
    };
    let projected = tool_card_input_projection(&call);
    for (status, output, valid) in [
        (RuntimeToolStatus::Cancelled, receipt.to_owned(), true),
        (RuntimeToolStatus::Success, receipt.to_owned(), false),
        (RuntimeToolStatus::Failed, receipt.to_owned(), false),
        (RuntimeToolStatus::Rejected, receipt.to_owned(), false),
        (
            RuntimeToolStatus::Cancelled,
            receipt.replace("\"executed\":false", "\"executed\":true"),
            false,
        ),
        (
            RuntimeToolStatus::Cancelled,
            receipt.replace("\"preserved\"", "\"deleted\""),
            false,
        ),
    ] {
        assert_eq!(
            super::super::pipeline::validate_recovered_projection(
                "p",
                "t",
                "capture",
                "ComputerScreenshot",
                &input,
                &output,
                status,
                &approval,
                None,
                None
            )
            .is_ok(),
            valid
        );
        let ui_status = match status {
            RuntimeToolStatus::Cancelled => ToolCallStatus::Cancelled,
            RuntimeToolStatus::Success => ToolCallStatus::Success,
            RuntimeToolStatus::Failed => ToolCallStatus::Failed,
            RuntimeToolStatus::Rejected => ToolCallStatus::Rejected,
        };
        let result = ToolResult {
            status: ui_status,
            output,
            reused: false,
            exit_code: None,
            duration_ms: None,
            truncated: (status == RuntimeToolStatus::Success).then_some(false),
            invalid: None,
        };
        assert_eq!(
            !matches!(
                tool_card_result_projection(Some(&projected), &result),
                ToolCardResultProjection::Corrupt
            ),
            valid
        );
    }
    for invalid in [
        ApprovalAudit {
            source: ApprovalSource::User,
            ..approval.clone()
        },
        ApprovalAudit {
            note: Some("not part of the receipt".into()),
            ..approval.clone()
        },
    ] {
        assert!(
            super::super::pipeline::validate_recovered_projection(
                "p",
                "t",
                "capture",
                "ComputerScreenshot",
                &input,
                receipt,
                RuntimeToolStatus::Cancelled,
                &invalid,
                None,
                None
            )
            .is_err()
        );
    }
    let denied = ApprovalAudit {
        decision: Approval::Deny,
        ..approval
    };
    assert!(
        super::super::pipeline::validate_recovered_projection(
            "p",
            "t",
            "capture",
            "ComputerScreenshot",
            &input,
            receipt,
            RuntimeToolStatus::Cancelled,
            &denied,
            None,
            None
        )
        .is_err()
    );
    let (store, _dir, project) = setup();
    let (control, _) = prepare(&store);
    let approved = ApprovalAudit {
        decision: Approval::Once,
        note: None,
        source: ApprovalSource::ComputerUse,
        danger: None,
    };
    store
        .conn()
        .execute(
            "UPDATE tool_calls SET approval=?1 WHERE id='capture'",
            [approved.to_json().unwrap()],
        )
        .unwrap();
    let mut terminal = vega_runtime::RuntimeToolResult {
        call_id: "capture".into(),
        output: receipt.into(),
        status: RuntimeToolStatus::Cancelled,
        reused: false,
        exit_code: None,
        duration_ms: None,
        truncated: Some(false),
        approval: Some(vega_runtime::RuntimeApprovalAudit {
            decision: vega_runtime::RuntimeApprovalDecision::Once,
            note: None,
            source: vega_runtime::RuntimeApprovalSource::ComputerUse,
            danger: None,
        }),
        remember_rule: None,
    };
    assert!(
        persist(
            &store,
            &project,
            &RuntimeEvent::ToolCallFinished(terminal.clone())
        )
        .is_err()
    );
    assert_eq!(
        tool_calls::find_state(store.conn(), "capture")
            .unwrap()
            .unwrap()
            .status,
        "running"
    );
    terminal.truncated = None;
    persist(&store, &project, &RuntimeEvent::ToolCallFinished(terminal)).unwrap();
    assert_eq!(
        tool_calls::find_state(store.conn(), "capture")
            .unwrap()
            .unwrap()
            .status,
        "cancelled"
    );
    control.stop();
    test_support::finish_cleanup(&control);
}
