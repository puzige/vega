use super::*;

fn observations() -> Vec<ScriptStep> {
    batch(&[
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
    ])
}

fn screenshots() -> Vec<ScriptStep> {
    batch(&[
        (
            "shot-a",
            "ComputerScreenshot",
            json!({"target_id":"fixture-2"}),
        ),
        (
            "shot-b",
            "ComputerScreenshot",
            json!({"target_id":"fixture-3"}),
        ),
    ])
}

fn changed_frame(id: u32) -> vega_mcp::ManagedResult {
    let mut result = frame(id);
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(4, 3, image::Rgb([20, 40, 60])))
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    result.images[0].bytes = encoded.into_inner();
    result
}

fn screenshot_target(message: &ChatMessage) -> Value {
    serde_json::from_str(
        message
            .content
            .lines()
            .find_map(|line| line.strip_prefix("Screenshot target: "))
            .unwrap(),
    )
    .unwrap()
}

fn assert_private_outcome(outcome: &AgentOutcome) {
    for message in &outcome.messages {
        assert!(message.images.is_empty());
        assert!(!message.content.contains("BODY-"));
        assert!(!message.content.contains("Owned 2"));
        assert!(!message.content.contains("Owned 3"));
        for call in &message.tool_calls {
            if ComputerUseRun::tool_capability(&call.name).is_some() {
                assert!(crate::computer_use::ComputerUseAudit::valid(
                    &call.input_json
                ));
            }
        }
    }
}

#[tokio::test]
async fn issue75_text_delivery_screenshot_pair_has_named_readable_images_in_three_rounds() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = ComputerUseRun::new(true, false);
    run.install_desktop_fixture(vec![window(2), window(3)], vec![frame(2), frame(3)])
        .await;
    let provider = MockProvider::new_rounds(vec![directory(), screenshots(), finish()]);
    let outcome = run_agent(
        &provider,
        &tools,
        configured(run.clone(), dir.path()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let requests = provider.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(outcome.executed_tool_call_count, 3);
    assert_eq!(
        outcome
            .events
            .iter()
            .filter(|event| matches!(event, RuntimeEvent::ScreenshotDelivered { .. }))
            .count(),
        2
    );
    for (call, title, other) in [
        ("shot-a", "Owned 2", "Owned 3"),
        ("shot-b", "Owned 3", "Owned 2"),
    ] {
        let message = result(&requests[2], call);
        assert!(
            message.content.contains(title),
            "image has no window attribution: {}",
            message.content
        );
        assert!(!message.content.contains(other));
        assert!(message.content.contains("read visible text"));
        assert!(message.content.contains("conversation_image_attachment"));
        assert_eq!(message.images.len(), 1);
        assert!(!message.content.contains("element_token"));
    }
    assert!(
        run.fixture_calls()
            .await
            .iter()
            .all(|(name, args)| name != "get_window_state"
                || args["include_accessibility_tree"] == false)
    );
    let definition = requests[0]
        .tools
        .iter()
        .find(|tool| tool.name == "ComputerScreenshot")
        .unwrap();
    assert!(definition.description.contains("visible text"));
    for request in requests {
        assert_provider_contract(&request);
    }
    assert_private_outcome(&outcome);
}

#[tokio::test]
async fn issue75_text_delivery_ax_reads_bridge_screenshot_pair_without_stale_pixels_or_authority() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = ComputerUseRun::new(true, false);
    run.install_desktop_fixture(
        vec![window(2), window(3)],
        vec![frame(2), frame(3), changed_frame(2), changed_frame(3)],
    )
    .await;
    let provider =
        MockProvider::new_rounds(vec![directory(), observations(), screenshots(), finish()]);
    let outcome = run_agent(
        &provider,
        &tools,
        configured(run, dir.path()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let requests = provider.requests();
    assert_eq!(requests.len(), 4);
    assert_eq!(outcome.executed_tool_call_count, 5);
    for (call, shot, id) in [("a", "shot-a", 2), ("b", "shot-b", 3)] {
        assert_eq!(
            screenshot_target(result(&requests[2], call))["target_id"],
            format!("fixture-{id}")
        );
        let old = result(&requests[3], call);
        assert!(
            old.content.contains(&format!("BODY-{id}")),
            "AX evidence vanished before final: {}",
            old.content
        );
        assert!(old.content.contains(&format!("Owned {id}")));
        assert!(old.content.contains("Prior read-only evidence"));
        assert!(old.content.contains("observed_at_unix_ms"));
        assert!(old.content.contains("may differ from later screenshots"));
        assert!(!old.content.contains("observation_id:"));
        assert!(!old.content.contains("element_token"));
        assert!(!old.content.contains("edit-"));
        assert!(!old.content.contains("snapshot-"));
        assert!(!old.content.contains("capture-"));
        assert!(old.images.is_empty());
        let current = result(&requests[3], shot);
        assert_eq!(current.images.len(), 1);
        assert_ne!(
            current.images[0].bytes(),
            result(&requests[2], call).images[0].bytes()
        );
        assert!(!current.content.contains(&format!("BODY-{id}")));
    }
    assert_eq!(
        requests[3]
            .messages
            .iter()
            .map(|message| message.images.len())
            .sum::<usize>(),
        2
    );
    for request in requests {
        assert_provider_contract(&request);
    }
    assert_private_outcome(&outcome);
}

#[tokio::test]
async fn issue75_text_delivery_bridge_is_revoked_before_actual_send() {
    for mode in ["pause", "invalidate", "cancel", "error"] {
        let dir = tempdir().unwrap();
        let tools = vega_tools::Tools::new(dir.path()).unwrap();
        let run = ComputerUseRun::new(true, false);
        run.install_desktop_fixture(
            vec![window(2), window(3)],
            vec![frame(2), frame(3), frame(2), frame(3)],
        )
        .await;
        let provider =
            MockProvider::new_rounds(vec![directory(), observations(), screenshots(), finish()]);
        let revoked = run.clone();
        let cancel = CancellationToken::new();
        let stopped = cancel.clone();
        let mut count = 0;
        let outcome = run_agent_with_sink(
            &provider, &tools, configured(run.clone(), dir.path()), cancel, move |event| {
                let mut fail = false;
                if matches!(event,RuntimeEvent::DiagnosticAttempt(ref value) if value.phase==RuntimeDiagnosticPhase::PrimaryModel && value.state==RuntimeDiagnosticState::Started) {
                    count += 1;
                    if count == 4 {
                        match mode {
                            "invalidate" => revoked.invalidate_observations(),
                            "cancel" => stopped.cancel(),
                            "error" => fail = true,
                            _ => {
                                revoked.control.pause();
                                revoked.control.finish_cleanup();
                                revoked.control.resume(revoked.control.status().epoch).unwrap();
                            }
                        }
                    }
                }
                async move { if fail { Err(VegaError::Cancelled) } else { Ok(()) } }
            },
        ).await;
        let requests = provider.requests();
        if mode == "error" {
            assert!(outcome.is_err());
            assert_eq!(requests.len(), 3);
        } else {
            let outcome = outcome.unwrap();
            assert_eq!(outcome.interrupted, mode == "cancel");
            assert_private_outcome(&outcome);
            if mode == "cancel" {
                assert_eq!(requests.len(), 3);
            } else {
                assert_eq!(requests.len(), 4);
                for id in ["a", "b"] {
                    assert!(!result(&requests[3], id).content.contains("BODY-"));
                    assert!(result(&requests[3], id).images.is_empty());
                }
                for id in ["shot-a", "shot-b"] {
                    let delivered = result(&requests[3], id);
                    assert!(delivered.content.contains("conversation_image_attachment"));
                    assert!(delivered.images.is_empty());
                }
            }
        }
        let mut retained = requests[2].messages.clone();
        run.trim_messages(&mut retained);
        assert!(
            retained
                .iter()
                .all(|message| !message.content.contains("BODY-") && message.images.is_empty())
        );
        assert_eq!(run.control.status().phase, ControlPhase::Stopped);
    }
}

#[tokio::test]
async fn issue75_text_delivery_bridge_keeps_latest_read_per_window() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = ComputerUseRun::new(true, false);
    let mut latest = frame(2);
    latest.structured.as_mut().unwrap()["tree_markdown"] = json!("LATEST-BODY");
    run.install_desktop_fixture(vec![window(2)], vec![frame(2), latest, changed_frame(2)])
        .await;
    let provider = MockProvider::new_rounds(vec![
        directory(),
        batch(&[
            (
                "a",
                "ComputerObserve",
                json!({"target_id":"fixture-2","cursor":null}),
            ),
            (
                "latest",
                "ComputerObserve",
                json!({"target_id":"fixture-2","cursor":null}),
            ),
        ]),
        batch(&[(
            "shot",
            "ComputerScreenshot",
            json!({"target_id":"fixture-2"}),
        )]),
        finish(),
    ]);
    let outcome = run_agent(
        &provider,
        &tools,
        configured(run, dir.path()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let requests = provider.requests();
    assert!(!result(&requests[3], "a").content.contains("BODY-2"));
    assert!(
        result(&requests[3], "latest")
            .content
            .contains("LATEST-BODY")
    );
    assert!(result(&requests[3], "latest").images.is_empty());
    assert!(
        !outcome
            .messages
            .iter()
            .any(|message| message.content.contains("LATEST-BODY"))
    );
    for request in requests {
        assert_provider_contract(&request);
    }
}

#[tokio::test]
async fn issue75_text_delivery_new_read_batch_cannot_accumulate_prior_window_evidence() {
    let dir = tempdir().unwrap();
    let tools = vega_tools::Tools::new(dir.path()).unwrap();
    let run = ComputerUseRun::new(true, false);
    run.install_desktop_fixture(
        vec![window(2), window(3), window(4)],
        vec![frame(2), frame(3), frame(2), frame(4), frame(4)],
    )
    .await;
    let provider = MockProvider::new_rounds(vec![
        directory(),
        observations(),
        batch(&[(
            "shot-a",
            "ComputerScreenshot",
            json!({"target_id":"fixture-2"}),
        )]),
        batch(&[(
            "c",
            "ComputerObserve",
            json!({"target_id":"fixture-4","cursor":null}),
        )]),
        batch(&[(
            "shot-c",
            "ComputerScreenshot",
            json!({"target_id":"fixture-4"}),
        )]),
        finish(),
    ]);
    let outcome = run_agent(
        &provider,
        &tools,
        configured(run, dir.path()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let requests = provider.requests();
    assert!(result(&requests[3], "a").content.contains("BODY-2"));
    assert!(result(&requests[3], "b").content.contains("BODY-3"));
    for id in ["a", "b"] {
        assert!(!result(&requests[5], id).content.contains("BODY-"));
        assert!(result(&requests[5], id).images.is_empty());
    }
    assert!(result(&requests[5], "c").content.contains("BODY-4"));
    assert!(
        result(&requests[5], "c")
            .content
            .contains("Prior read-only evidence")
    );
    assert!(result(&requests[5], "c").images.is_empty());
    for request in requests {
        assert_provider_contract(&request);
    }
    assert_private_outcome(&outcome);
}
