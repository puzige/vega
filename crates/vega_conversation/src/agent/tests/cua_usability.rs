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
