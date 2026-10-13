use super::*;

#[test]
fn issue75_takeover_waits_for_cleanup_and_keeps_single_lease() {
    let a = ComputerUseControl::new();
    let b = ComputerUseControl::new();
    let epoch = a.begin(Capability::Desktop).unwrap();
    let target = DesktopTarget {
        selection_id: "choice".into(),
        application: "Fixture".into(),
        bundle_id: "test.fixture".into(),
        pid: 4,
        window_id: 9,
        title: "Owned".into(),
    };
    a.set_windows(epoch, vec![target]).unwrap();
    a.select_window(epoch, "choice").unwrap();
    a.pause();
    assert_eq!(a.resume(a.status().epoch), Err(ComputerUseError::NotReady));
    assert_eq!(b.begin(Capability::Browser), Err(ComputerUseError::Busy));
    assert_eq!(
        a.select_window(epoch, "choice"),
        Err(ComputerUseError::NotReady)
    );
    a.finish_cleanup();
    a.resume(a.status().epoch).unwrap();
    assert!(a.check_epoch(epoch).is_err());
    assert_eq!(
        a.select_window(epoch, "choice"),
        Err(ComputerUseError::NotReady)
    );
    a.stop();
    assert_eq!(b.begin(Capability::Browser), Err(ComputerUseError::Busy));
    a.finish_cleanup();
    assert!(b.begin(Capability::Browser).is_ok());
    b.stop();
    b.finish_cleanup();
}

#[test]
fn issue75_audit_contains_no_typed_text_or_target() {
    let input = r#"{"action":"type_text","text":"owned-secret","target":"private-reference"}"#;
    let projected = ComputerUseAudit::project(input, "type_text");
    assert!(ComputerUseAudit::valid(&projected));
    assert!(!projected.contains("owned-secret"));
    assert!(!projected.contains("private-reference"));
}

pub(crate) fn fixture_observation(
    control: &ComputerUseControl,
    epoch: u64,
    index: usize,
) -> Observation {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(4, 3)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    Observation {
        id: format!("frame-{index}"),
        run_id: control.run_id().into(),
        call_id: format!("call-{index}"),
        epoch,
        capability: Capability::Desktop,
        source: ToolObservationSource::CuaWindow,
        model_image: true,
        read_only: false,
        target: Target::Desktop(DesktopTarget {
            selection_id: "fixture".into(),
            application: "Fixture".into(),
            bundle_id: "fixture".into(),
            pid: 1,
            window_id: 2,
            title: "private-title".into(),
        }),
        image: Some(crate::ImageAttachment::from_bytes(bytes.into_inner()).unwrap()),
        original_width: 4,
        original_height: 3,
        text: "owned-private-AX".into(),
        snapshot_id: Some(format!("snapshot-{index}")),
        capture_id: Some(format!("capture-{index}")),

        elements: serde_json::json!([]),
        background_input: background::BackgroundInput::default(),
    }
}

#[test]
fn issue75_observation_cache_trims_all_old_payloads_and_rejects_late_frames() {
    let run = ComputerUseRun::new(true, false);
    let epoch = run.control.begin(Capability::Desktop).unwrap();
    let first = fixture_observation(&run.control, epoch, 0);
    run.control
        .connected(epoch, Capability::Desktop, first.target.clone())
        .unwrap();
    let mut messages = Vec::new();
    for index in 0..20 {
        let observation = fixture_observation(&run.control, epoch, index);
        let mut message = crate::ChatMessage::tool_result(&observation.call_id, "durable summary");
        run.control.publish(observation.clone()).unwrap();
        run.apply_observation(&observation.call_id, &mut message);
        messages.push(message);
        run.trim_messages(&mut messages);
        assert_eq!(
            messages
                .iter()
                .filter(|message| !message.images.is_empty())
                .count(),
            1
        );
        assert_eq!(
            messages
                .iter()
                .filter(|message| message.content.contains("owned-private-AX"))
                .count(),
            1
        );
        assert!(run.control.snapshot().observations.len() <= 2);
    }
    assert!(
        run.control
            .observation(Capability::Desktop, "frame-0")
            .is_err()
    );
    run.invalidate_observations();
    run.trim_messages(&mut messages);
    assert!(
        messages
            .iter()
            .all(|message| message.images.is_empty()
                && !message.content.contains("owned-private-AX"))
    );
    run.control.pause();
    assert!(
        run.control
            .publish(fixture_observation(&run.control, epoch, 99))
            .is_err()
    );
    run.control.finish_cleanup();
    run.control.stop();
    run.control.finish_cleanup();
}

#[test]
fn issue75_coordinate_projection_tracks_image_resize_and_rejects_out_of_bounds() {
    let control = ComputerUseControl::new();
    let mut observation = fixture_observation(&control, 1, 0);
    observation.original_width = 8;
    observation.original_height = 6;
    assert_eq!(observation.coordinates(2.0, 1.5).unwrap(), (4.0, 3.0));
    assert!(observation.coordinates(4.0, 0.0).is_err());
    assert!(observation.coordinates(f64::NAN, 0.0).is_err());
    assert!(!format!("{observation:?}").contains("owned-private-AX"));
    assert!(!format!("{:?}", observation.target).contains("private-title"));
}

#[test]
fn issue75_background_live_observation_invalidation_does_not_rewrite_original_success() {
    let run = ComputerUseRun::new(true, false);
    let epoch = run.control.begin(Capability::Desktop).unwrap();
    let mut first = fixture_observation(&run.control, epoch, 0);
    let mut report = serde_json::json!({
        "exact_window":{"status":"matched","pid":1,"window_id":2},
        "routes":[
            {"route":"accessibility","status":"available"},
            {"route":"window_pointer","status":"available"},
            {"route":"pid_keyboard","status":"refused","reason":"same_pid_keyboard_ambiguity"}
        ]
    });
    first.background_input = background::BackgroundInput::from_report(&report, 1, 2);
    run.control
        .connected(epoch, Capability::Desktop, first.target.clone())
        .unwrap();
    let mut message = crate::ChatMessage::tool_result(&first.call_id, "completed");
    run.control.publish(first.clone()).unwrap();
    run.apply_observation(&first.call_id, &mut message);
    assert!(message.content.contains("owned-private-AX"));
    assert!(!message.images.is_empty());
    assert!(
        message
            .content
            .contains("pid_keyboard=refused:same_pid_keyboard_ambiguity")
    );
    run.control.discard_observations();
    run.trim_messages(std::slice::from_mut(&mut message));
    assert!(message.images.is_empty());
    assert!(!message.content.contains("pid_keyboard"));
    assert!(!message.content.contains("same_pid_keyboard_ambiguity"));
    assert!(!message.content.contains("owned-private-AX"));
    assert!(message.content.contains("returned successfully"));
    assert!(message.content.contains("does not mean"));
    assert!(message.content.contains("fresh observation"));
    assert!(
        run.control
            .observation(Capability::Desktop, &first.id)
            .is_err()
    );
    let mut later = fixture_observation(&run.control, epoch, 1);
    later.text = "post-action-fixture".into();
    report["routes"][2] = serde_json::json!({"route":"pid_keyboard","status":"available"});
    later.background_input = background::BackgroundInput::from_report(&report, 1, 2);
    let mut later_message = crate::ChatMessage::tool_result(&later.call_id, "completed");
    run.control.publish(later.clone()).unwrap();
    run.apply_observation(&later.call_id, &mut later_message);
    assert!(later_message.content.contains("post-action-fixture"));
    assert!(!later_message.content.contains("owned-private-AX"));
    assert!(!later_message.images.is_empty());
    assert!(later_message.content.contains("pid_keyboard=available"));
    assert!(
        !later_message
            .content
            .contains("same_pid_keyboard_ambiguity")
    );
    let outcome = crate::agent::outcome(
        Vec::new(),
        vec![later_message],
        String::new(),
        3,
        1,
        false,
        false,
    );
    assert_eq!(outcome.messages[0].content, INVALIDATED_OBSERVATION);
    assert!(outcome.messages[0].images.is_empty());
    assert!(!outcome.messages[0].content.contains("pid_keyboard"));
    assert!(!outcome.messages[0].content.contains("post-action-fixture"));
    run.control.stop();
    run.control.finish_cleanup();
}

#[test]
fn issue75_desktop_access_explicit_discovery_schema() {
    let run = ComputerUseRun::new(true, false);
    let definition = run
        .definitions()
        .into_iter()
        .find(|tool| tool.name == "ComputerObserve")
        .unwrap();
    assert_eq!(
        definition.input_schema["properties"]["target_id"]["type"],
        json!(["string", "null"])
    );
    assert_eq!(
        definition.input_schema["required"],
        json!(["target_id", "cursor"])
    );
    assert!(!definition.description.contains("user selects and grants"));
    assert!(!definition.description.contains("any action dispatch"));
    assert!(
        definition
            .description
            .contains("original 60-second lifetime across action dispatch")
    );
    assert!(
        definition
            .description
            .contains("actions consume only actionable observations")
    );
    for invalidation in [
        "Pause/takeover",
        "stop",
        "compaction",
        "explicit invalidation",
    ] {
        assert!(definition.description.contains(invalidation));
    }
}

#[test]
fn issue75_desktop_access_discovery_does_not_require_approval() {
    let control = ComputerUseControl::new();
    let epoch = control.begin(Capability::Desktop).unwrap();
    let Target::Desktop(target) = fixture_observation(&control, epoch, 0).target else {
        panic!("fixture target")
    };
    control.set_windows(epoch, vec![target]).unwrap();
    assert_eq!(control.status().phase, ControlPhase::Running);
    control.stop();
    control.finish_cleanup();
}

#[test]
fn issue75_desktop_access_pages_all_windows_and_invalidates_cross_window_frames() {
    let run = ComputerUseRun::new(true, false);
    let epoch = run.control.begin(Capability::Desktop).unwrap();
    let windows = (0..450)
        .map(|index| DesktopTarget {
            selection_id: format!("target-{index}"),
            application: format!("App-{index}"),
            bundle_id: format!("fixture.{index}"),
            pid: index + 1,
            window_id: index + 2,
            title: format!("Title-{index}"),
        })
        .collect();
    run.control.set_windows(epoch, windows).unwrap();
    let first = run.control.catalog_page(epoch, None).unwrap();
    assert_eq!(first["windows"].as_array().unwrap().len(), 200);
    let cursor = first["next_cursor"].as_str().unwrap().to_owned();
    let second = run.control.catalog_page(epoch, Some(&cursor)).unwrap();
    assert_eq!(second["windows"][0]["target_id"], "target-200");
    assert!(run.control.catalog_page(epoch, Some(&cursor)).is_err());
    let third = run
        .control
        .catalog_page(epoch, second["next_cursor"].as_str())
        .unwrap();
    assert_eq!(third["windows"].as_array().unwrap().len(), 50);
    assert!(third["next_cursor"].is_null());
    run.control.select_window(epoch, "target-0").unwrap();
    let mut old = fixture_observation(&run.control, epoch, 0);
    old.target = run.control.target(Capability::Desktop).unwrap().unwrap();
    run.control.publish(old.clone()).unwrap();
    run.control.select_window(epoch, "target-449").unwrap();
    assert!(
        run.control
            .observation(Capability::Desktop, &old.id)
            .is_err()
    );
    assert!(run.control.publish(old).is_err());
    let current = run.control.target(Capability::Desktop).unwrap().unwrap();
    assert!(
        matches!(current, Target::Desktop(window) if window.pid == 450 && window.window_id == 451)
    );
    run.control.pause();
    assert!(run.control.set_windows(epoch, Vec::new()).is_err());
    assert!(run.control.select_window(epoch, "target-449").is_err());
    assert!(run.control.resume(run.control.status().epoch).is_err());
    run.control.finish_cleanup();
    run.control.resume(run.control.status().epoch).unwrap();
    let later = run.control.begin(Capability::Desktop).unwrap();
    run.control.set_windows(later, Vec::new()).unwrap();
    assert_eq!(run.control.status().phase, ControlPhase::Running);
    assert!(run.control.select_window(later, "target-449").is_err());
    assert!(run.control.catalog_page(later, Some(&cursor)).is_err());
    assert!(
        run.control.catalog_page(later, None).unwrap()["windows"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    run.control.stop();
    assert!(run.control.catalog_page(later, None).is_err());
    run.control.finish_cleanup();
}

#[test]
fn issue75_desktop_access_directory_is_ephemeral_and_not_an_action_frame() {
    let run = ComputerUseRun::new(true, false);
    let epoch = run.control.begin(Capability::Desktop).unwrap();
    let mut observation = fixture_observation(&run.control, epoch, 1);
    observation.target = Target::DesktopCatalog;
    observation.source = ToolObservationSource::CuaWindowCatalog;
    observation.image = None;
    observation.model_image = false;
    observation.text = "private-window-directory".into();
    run.control.set_windows(epoch, Vec::new()).unwrap();
    run.control.publish(observation.clone()).unwrap();
    let mut message =
        crate::ChatMessage::tool_result(&observation.call_id, "durable directory summary");
    run.apply_observation(&observation.call_id, &mut message);
    assert!(message.content.contains("private-window-directory"));
    assert!(message.images.is_empty());
    run.invalidate_observations();
    run.trim_messages(std::slice::from_mut(&mut message));
    assert!(!message.content.contains("private-window-directory"));
    assert!(run.control.catalog_page(epoch, None).is_err());
    run.control.stop();
    run.control.finish_cleanup();
}

#[tokio::test]
async fn issue75_observation_recoverable_failure_clears_binding_and_keeps_run_ready() {
    let run = ComputerUseRun::new(true, false);
    let epoch = run.control.begin(Capability::Desktop).unwrap();
    let observation = fixture_observation(&run.control, epoch, 0);
    let Target::Desktop(target) = observation.target.clone() else {
        panic!()
    };
    run.control
        .set_windows(epoch, vec![target.clone()])
        .unwrap();
    run.control
        .select_window(epoch, &target.selection_id)
        .unwrap();
    run.control.publish(observation.clone()).unwrap();
    let error = diagnostics::failure(
        Capability::Desktop,
        diagnostics::Stage::Observe,
        diagnostics::Class::AxUnavailable,
    );
    assert!(!run.record_failure("ComputerObserve", Capability::Desktop, epoch, error));
    assert_eq!(run.control.status().phase, ControlPhase::Running);
    assert_eq!(
        run.control.target(Capability::Desktop).unwrap(),
        Some(Target::DesktopCatalog)
    );
    assert!(
        run.control
            .observation(Capability::Desktop, &observation.id)
            .is_err()
    );
    assert!(
        run.control
            .select_window(epoch, &target.selection_id)
            .is_err()
    );
    assert!(run.wait_ready(&CancellationToken::new()).await.is_ok());
    run.control
        .set_windows(epoch, vec![target.clone()])
        .unwrap();
    run.control
        .select_window(epoch, &target.selection_id)
        .unwrap();
    run.control
        .publish(fixture_observation(&run.control, epoch, 1))
        .unwrap();
    run.control.stop();
    run.control.finish_cleanup();
}

#[tokio::test]
async fn issue75_observation_recovery_never_revives_revoked_or_fatal_runs() {
    use diagnostics::{Class, Stage, failure};
    for (name, capability, stage, class) in [
        (
            "ComputerAct",
            Capability::Desktop,
            Stage::Observe,
            Class::CaptureUnavailable,
        ),
        (
            "BrowserObserve",
            Capability::Browser,
            Stage::Observe,
            Class::CaptureUnavailable,
        ),
        (
            "ComputerObserve",
            Capability::Desktop,
            Stage::Observe,
            Class::CaptureFrameMismatch,
        ),
        (
            "ComputerObserve",
            Capability::Desktop,
            Stage::Observe,
            Class::ObservationIdentityMismatch,
        ),
        (
            "ComputerObserve",
            Capability::Desktop,
            Stage::Observe,
            Class::DegradedUnknown,
        ),
        (
            "ComputerObserve",
            Capability::Desktop,
            Stage::Observe,
            Class::Protocol,
        ),
        (
            "ComputerObserve",
            Capability::Desktop,
            Stage::Permissions,
            Class::PermissionsMissing,
        ),
        (
            "ComputerObserve",
            Capability::Desktop,
            Stage::Observe,
            Class::PolicyDenied,
        ),
    ] {
        let run = ComputerUseRun::new(true, true);
        let epoch = run.control.begin(Capability::Desktop).unwrap();
        run.control
            .connected(epoch, Capability::Desktop, Target::DesktopCatalog)
            .unwrap();
        assert!(run.record_failure(name, capability, epoch, failure(capability, stage, class)));
        assert_eq!(run.control.status().phase, ControlPhase::Failed);
        assert_eq!(
            run.wait_ready(&CancellationToken::new()).await,
            Err(ComputerUseError::Cancelled)
        );
        run.control.stop();
        run.control.finish_cleanup();
    }
    for stop in [false, true] {
        let run = ComputerUseRun::new(true, false);
        let epoch = run.control.begin(Capability::Desktop).unwrap();
        run.control
            .connected(epoch, Capability::Desktop, Target::DesktopCatalog)
            .unwrap();
        if stop {
            run.control.stop();
        } else {
            run.control.pause();
        }
        let before = run.control.status();
        run.record_failure(
            "ComputerObserve",
            Capability::Desktop,
            epoch,
            failure(
                Capability::Desktop,
                Stage::Observe,
                Class::CaptureUnavailable,
            ),
        );
        let after = run.control.status();
        assert_eq!(after.phase, before.phase);
        assert_eq!(after.epoch, before.epoch);
        assert!(run.control.snapshot().observations.is_empty());
        run.control.stop();
        run.control.finish_cleanup();
    }
}

#[test]
fn issue75_usability_delivery_commit_and_pause_have_one_ordered_boundary() {
    let control = ComputerUseControl::new();
    let epoch = control.begin(Capability::Desktop).unwrap();
    control
        .connected(epoch, Capability::Desktop, Target::DesktopCatalog)
        .unwrap();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let delivery = control.clone();
    let committing = std::thread::spawn(move || {
        delivery.commit_delivery(epoch, || {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            1
        })
    });
    started_rx.recv().unwrap();
    let paused = control.clone();
    let pausing = std::thread::spawn(move || paused.pause());
    release_tx.send(()).unwrap();
    assert_eq!(committing.join().unwrap().unwrap(), 1);
    pausing.join().unwrap();
    assert!(
        control
            .commit_delivery(epoch, || panic!("late artifact must not commit"))
            .is_err()
    );
    control.finish_cleanup();
    control.resume(control.status().epoch).unwrap();
    assert!(
        control
            .commit_delivery(epoch, || panic!("old epoch cannot publish after resume"))
            .is_err()
    );
    control.stop();
    control.finish_cleanup();
}

#[test]
fn issue75_successful_reads_only_durable_ack_can_survive_capture_invalidation() {
    for acknowledged in [false, true] {
        let run = ComputerUseRun::new(true, false);
        let epoch = run.control.begin(Capability::Desktop).unwrap();
        let observation = fixture_observation(&run.control, epoch, 0);
        run.control
            .connected(epoch, Capability::Desktop, observation.target.clone())
            .unwrap();
        let image = observation.image.clone().unwrap();
        run.retain_screenshot(&observation).unwrap();
        assert!(
            run.acknowledge_screenshot(&observation.call_id, epoch + 1)
                .is_err()
        );
        if acknowledged {
            run.acknowledge_screenshot(&observation.call_id, epoch)
                .unwrap();
        }
        let mut message = crate::ChatMessage::projected_tool_result(
            &observation.call_id,
            "ComputerScreenshot",
            &screenshot_receipt(&observation.call_id),
        );
        message.images = vec![image];
        run.control.publish(observation.clone()).unwrap();
        run.control.pause();
        run.control.finish_cleanup();
        run.trim_messages(std::slice::from_mut(&mut message));
        assert!(message.images.is_empty());
        assert_eq!(
            message.content.contains("conversation_image_attachment"),
            acknowledged
        );
        if acknowledged {
            assert_eq!(
                message.content,
                run.model_receipt(&observation.call_id).unwrap()
            );
        } else {
            assert_eq!(message.content, INVALIDATED_OBSERVATION);
        }
        run.control.stop();
        run.control.finish_cleanup();
    }
}

#[test]
fn issue75_delivery_progress_reuse_requires_ack_and_current_capture_epoch() {
    let run = ComputerUseRun::new(true, false);
    let epoch = run.control.begin(Capability::Desktop).unwrap();
    let first = fixture_observation(&run.control, epoch, 0);
    run.control
        .connected(epoch, Capability::Desktop, first.target.clone())
        .unwrap();
    run.retain_screenshot(&first).unwrap();
    assert!(run.model_receipt(&first.call_id).is_none());
    let mut pending = first.clone();
    pending.call_id = "pending-second".into();
    let receipt: Value = serde_json::from_str(&run.retain_screenshot(&pending).unwrap()).unwrap();
    assert_eq!(receipt["unchanged"], false);
    assert!(run.screenshot(&pending.call_id).is_some());
    assert!(run.model_receipt(&pending.call_id).is_none());
    run.acknowledge_screenshot(&first.call_id, epoch).unwrap();
    let mut reused = first.clone();
    reused.call_id = "reused-after-ack".into();
    let receipt: Value = serde_json::from_str(&run.retain_screenshot(&reused).unwrap()).unwrap();
    assert_eq!(receipt["unchanged"], true);
    assert_eq!(
        receipt["artifact_id"],
        format!("screenshot:{}", first.call_id)
    );
    assert!(run.screenshot(&reused.call_id).is_none());
    assert!(run.model_receipt(&reused.call_id).is_some());
    run.control.pause();
    let mut late = first.clone();
    late.call_id = "late-after-pause".into();
    assert!(run.retain_screenshot(&late).is_err());
    assert!(run.model_receipt(&late.call_id).is_none());
    run.control.finish_cleanup();
    run.control.resume(run.control.status().epoch).unwrap();
    assert!(run.retain_screenshot(&late).is_err());
    assert!(run.model_receipt(&late.call_id).is_none());
    run.control.stop();
    run.control.finish_cleanup();
}

#[test]
fn issue75_delivery_progress_unacknowledged_external_body_cannot_escape_outcome() {
    let control = ComputerUseControl::new();
    let mut observation = fixture_observation(&control, 1, 0);
    observation.text = "owned-private-screenshot-body".into();
    let mut message =
        crate::ChatMessage::tool_result(&observation.call_id, observation.model_text());
    message.images = observation.image.into_iter().collect();
    let outcome = crate::agent::outcome(
        Vec::new(),
        vec![
            crate::ChatMessage::assistant_with_tools(
                "",
                vec![crate::ChatToolCall {
                    id: observation.call_id,
                    name: "ComputerScreenshot".into(),
                    input_json: json!({"target_id":"owned-private-target"}).to_string(),
                }],
            ),
            message,
        ],
        String::new(),
        1,
        1,
        true,
        false,
    );
    assert_eq!(outcome.messages[1].content, INVALIDATED_OBSERVATION);
    assert!(outcome.messages[1].images.is_empty());
    assert!(ComputerUseAudit::valid(
        &outcome.messages[0].tool_calls[0].input_json
    ));
}
