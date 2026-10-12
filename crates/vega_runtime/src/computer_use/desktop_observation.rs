use super::{
    Capability, ComputerUseError as Error, DesktopTarget, Observation,
    diagnostics::{self, Class, Stage},
    media::decode_image,
};
use serde_json::Value;

const AX_EMPTY: &str = "ax_tree_empty: the AX walk returned no actionable elements. The window may be a non-AX surface (canvas/WebGL/custom-drawn) or its accessibility tree was not ready (Chromium/Electron require an AX-enable + settle). Do not treat element data as authoritative — re-snapshot if the app just launched, otherwise switch to the visual path.";
const SCREENSHOT_ONLY: &str = "screenshot-only: AX is unavailable. This verified window image is read-only. No ComputerAct action is permitted from this observation. Obtain a new healthy ComputerObserve before any input.";

fn failure(class: Class) -> Error {
    diagnostics::failure(Capability::Desktop, Stage::Observe, class)
}

fn bounded_id(value: &Value) -> Option<&str> {
    value
        .as_str()
        .filter(|id| !id.is_empty() && id.len() <= 512)
}

fn numbered_template(text: &str, prefix: &str, suffix: &str) -> bool {
    text.strip_prefix(prefix)
        .and_then(|rest| rest.strip_suffix(suffix))
        .is_some_and(|digits| {
            !digits.is_empty()
                && digits.len() <= 10
                && digits.bytes().all(|byte| byte.is_ascii_digit())
                && digits.parse::<u32>().is_ok()
        })
}

fn degraded_class(text: &str, target: &DesktopTarget) -> Class {
    let launching = format!(
        "ax_app_launching: window_id {} exists and is owned by pid {}, but that app has not finished launching and did not answer accessibility within the ",
        target.window_id, target.pid
    );
    if numbered_template(
        text,
        &launching,
        " ms timeout_ms budget. The tree is returned EMPTY because the window's accessibility surface is not available yet.",
    ) {
        return Class::AxAppLaunching;
    }
    let unresolved = format!(
        "ax_window_unresolved: window_id {} exists and is owned by pid {}, but none of the ",
        target.window_id, target.pid
    );
    if numbered_template(
        text,
        &unresolved,
        " AXWindow element(s) under that pid reports this CGWindowID. The tree is returned EMPTY on purpose: the accessibility elements reachable under this pid belong to other surfaces (the menu bar, other windows), not to the requested window, so presenting them would misground the next action.",
    ) {
        return Class::AxWindowUnresolved;
    }
    Class::DegradedUnknown
}

fn error_metadata(structured: Option<&Value>, target: &DesktopTarget) -> Result<(), Error> {
    let Some(value) = structured else {
        return Ok(());
    };
    if !value.is_object() {
        return Err(failure(Class::Protocol));
    }
    if (value.get("pid").is_some() || value.get("window_id").is_some())
        && (value["pid"].as_u64() != Some(u64::from(target.pid))
            || value["window_id"].as_u64() != Some(u64::from(target.window_id)))
    {
        return Err(failure(Class::ObservationIdentityMismatch));
    }
    if value.get("screenshot_frame_valid").is_some() || value.get("screenshot_error").is_some() {
        match value["screenshot_frame_valid"].as_bool() {
            Some(true) if value.get("screenshot_error").is_none_or(Value::is_null) => {}
            Some(false) => match value["screenshot_error"]["code"].as_str() {
                Some("px_capture_unavailable") => {}
                Some("px_frame_mismatch") => return Err(failure(Class::CaptureFrameMismatch)),
                Some("px_window_not_found") => return Err(failure(Class::TargetUnavailable)),
                _ => return Err(failure(Class::ScreenshotUnverified)),
            },
            _ => return Err(failure(Class::ScreenshotUnverified)),
        }
    }
    match value.get("degraded") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => {}
        Some(Value::Bool(true)) => {
            let reason = value["degraded_reason"].as_str().unwrap_or_default();
            if reason == AX_EMPTY {
                if !value["elements"].as_array().is_some_and(Vec::is_empty) {
                    return Err(failure(Class::DegradedUnknown));
                }
            } else if degraded_class(reason, target) == Class::DegradedUnknown {
                return Err(failure(Class::DegradedUnknown));
            }
        }
        _ => return Err(failure(Class::DegradedUnknown)),
    }
    Ok(())
}

pub(super) fn apply(
    observation: &mut Observation,
    target: &DesktopTarget,
    result: vega_mcp::ManagedResult,
) -> Result<(), Error> {
    if result.is_error {
        error_metadata(result.structured.as_ref(), target)?;
        return diagnostics::checked(Capability::Desktop, Stage::Observe, Ok(result)).map(|_| ());
    }
    let structured = result.structured.ok_or_else(|| failure(Class::Protocol))?;
    if structured["pid"].as_u64() != Some(u64::from(target.pid))
        || structured["window_id"].as_u64() != Some(u64::from(target.window_id))
    {
        return Err(failure(Class::ObservationIdentityMismatch));
    }
    let capture_unavailable = if structured["screenshot_frame_valid"] != true {
        let class = if structured["screenshot_frame_valid"] == false {
            match structured["screenshot_error"]["code"].as_str() {
                Some("px_capture_unavailable") => Class::CaptureUnavailable,
                Some("px_frame_mismatch") => Class::CaptureFrameMismatch,
                Some("px_window_not_found") => Class::TargetUnavailable,
                _ => Class::ScreenshotUnverified,
            }
        } else {
            Class::ScreenshotUnverified
        };
        if class != Class::CaptureUnavailable {
            return Err(failure(class));
        }
        true
    } else {
        if structured
            .get("screenshot_error")
            .is_some_and(|error| !error.is_null())
        {
            return Err(failure(Class::ScreenshotUnverified));
        }
        false
    };
    let read_only = match structured.get("degraded") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::Bool(true)) => {
            let reason = structured["degraded_reason"].as_str().unwrap_or_default();
            if reason != AX_EMPTY {
                return Err(failure(degraded_class(reason, target)));
            }
            if !structured["elements"].as_array().is_some_and(Vec::is_empty) {
                return Err(failure(Class::DegradedUnknown));
            }
            true
        }
        _ => return Err(failure(Class::DegradedUnknown)),
    };
    if capture_unavailable {
        return Err(failure(Class::CaptureUnavailable));
    }
    let snapshot_id = bounded_id(&structured["snapshot_id"])
        .ok_or_else(|| failure(Class::CaptureBindingFailed))?;
    let capture_id = bounded_id(&structured["capture_id"])
        .ok_or_else(|| failure(Class::CaptureBindingFailed))?;
    let image = result
        .images
        .into_iter()
        .next()
        .ok_or_else(|| failure(Class::InvalidImage))?;
    let (image, width, height) = decode_image(image).map_err(|_| failure(Class::InvalidImage))?;
    observation.snapshot_id = Some(snapshot_id.into());
    observation.capture_id = Some(capture_id.into());
    observation.read_only = read_only;
    observation.elements = if read_only {
        Value::Null
    } else {
        structured["elements"].clone()
    };
    observation.background_input = if read_only {
        super::background::BackgroundInput::default()
    } else {
        super::background::BackgroundInput::from_report(
            &structured["background_input"],
            target.pid,
            target.window_id,
        )
    };
    observation.text = if read_only {
        SCREENSHOT_ONLY.into()
    } else {
        structured["tree_markdown"]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(65536)
            .collect()
    };
    observation.image = Some(image);
    observation.original_width = width;
    observation.original_height = height;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const AX_EMPTY: &str = "ax_tree_empty: the AX walk returned no actionable elements. The window may be a non-AX surface (canvas/WebGL/custom-drawn) or its accessibility tree was not ready (Chromium/Electron require an AX-enable + settle). Do not treat element data as authoritative — re-snapshot if the app just launched, otherwise switch to the visual path.";

    pub(super) fn result() -> vega_mcp::ManagedResult {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(4, 3)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        vega_mcp::ManagedResult {
            text: vec!["PRIVATE_UPSTREAM_TEXT".into()],
            images: vec![vega_mcp::ManagedImage {
                bytes: bytes.into_inner(),
                mime_type: "image/png".into(),
            }],
            structured: Some(json!({"pid":1,"window_id":2,"screenshot_frame_valid":true,
                "snapshot_id":"snapshot","capture_id":"capture","elements":[],
                "tree_markdown":"PRIVATE_AX","degraded":true,"degraded_reason":AX_EMPTY})),
            is_error: false,
        }
    }

    #[test]
    fn issue75_observation_verified_ax_empty_is_read_only_then_healthy() {
        let run = super::super::ComputerUseRun::new(true, false);
        let epoch = run.control.begin(Capability::Desktop).unwrap();
        let mut observation = super::super::tests::fixture_observation(&run.control, epoch, 0);
        let super::super::Target::Desktop(target) = observation.target.clone() else {
            panic!()
        };
        run.control
            .connected(epoch, Capability::Desktop, observation.target.clone())
            .unwrap();
        apply(&mut observation, &target, result()).unwrap();
        assert!(observation.read_only);
        assert!(observation.image.is_some());
        assert!(observation.model_text().contains("screenshot-only"));
        assert!(!observation.model_text().contains("PRIVATE"));
        run.control.publish(observation.clone()).unwrap();
        let mut message = crate::ChatMessage::tool_result(&observation.call_id, "completed");
        run.apply_observation(&observation.call_id, &mut message);
        assert!(!message.images.is_empty());
        assert!(message.content.contains("screenshot-only"));
        let mut healthy = result();
        healthy.structured.as_mut().unwrap()["degraded"] = json!(false);
        observation.id = "healthy".into();
        apply(&mut observation, &target, healthy).unwrap();
        assert!(!observation.read_only);
        assert_eq!(observation.text, "PRIVATE_AX");
        run.control.publish(observation).unwrap();
        assert!(
            run.control
                .observation(Capability::Desktop, "frame-0")
                .is_err()
        );
        run.control.stop();
        run.control.finish_cleanup();
    }

    #[test]
    fn issue75_observation_guard_rejects_unverified_or_spoofed_screenshot() {
        let control = super::super::ComputerUseControl::new();
        for (key, value) in [
            ("pid", json!(3)),
            ("window_id", json!(4)),
            ("screenshot_frame_valid", json!(false)),
            ("screenshot_frame_valid", Value::Null),
            ("snapshot_id", json!("")),
            ("capture_id", Value::Null),
            ("elements", json!([{"element_token":"spoof"}])),
            ("degraded", json!("true")),
            ("degraded_reason", json!("ax_tree_empty: PRIVATE_REASON")),
            (
                "degraded_reason",
                json!(format!("{AX_EMPTY} PRIVATE_REASON")),
            ),
        ] {
            let mut response = result();
            response.structured.as_mut().unwrap()[key] = value;
            let mut observation = super::super::tests::fixture_observation(&control, 1, 0);
            let super::super::Target::Desktop(target) = observation.target.clone() else {
                panic!()
            };
            assert!(apply(&mut observation, &target, response).is_err(), "{key}");
        }
        let mut response = result();
        response.images[0].bytes = vec![1, 2, 3];
        let mut observation = super::super::tests::fixture_observation(&control, 1, 0);
        let super::super::Target::Desktop(target) = observation.target.clone() else {
            panic!()
        };
        assert!(apply(&mut observation, &target, response).is_err());
    }
    #[test]
    fn issue75_observation_explicit_frame_failure_dominates_recoverable_ax() {
        let control = super::super::ComputerUseControl::new();
        let mut observation = super::super::tests::fixture_observation(&control, 1, 0);
        let super::super::Target::Desktop(target) = observation.target.clone() else {
            panic!()
        };
        let mut response = result();
        let structured = response.structured.as_mut().unwrap();
        structured["degraded_reason"] = json!(
            "ax_window_unresolved: window_id 2 exists and is owned by pid 1, but none of the 2 AXWindow element(s) under that pid reports this CGWindowID. The tree is returned EMPTY on purpose: the accessibility elements reachable under this pid belong to other surfaces (the menu bar, other windows), not to the requested window, so presenting them would misground the next action."
        );
        structured["screenshot_frame_valid"] = json!(false);
        structured["screenshot_error"] =
            json!({"code":"px_frame_mismatch","message":"PRIVATE_REASON"});
        assert_eq!(
            apply(&mut observation, &target, response),
            Err(failure(Class::CaptureFrameMismatch))
        );
    }

    #[tokio::test]
    async fn issue75_observation_conflicting_capture_metadata_never_publishes_or_dispatches() {
        for is_error in [false, true] {
            for degraded in [false, true] {
                for screenshot_error in [
                    json!({"code":"px_frame_mismatch","message":"PRIVATE_REASON"}),
                    json!({"code":"px_capture_unavailable","message":"PRIVATE_REASON"}),
                    json!({"code":"PRIVATE_UNKNOWN"}),
                    json!({}),
                    json!(""),
                    json!(false),
                ] {
                    let run = super::super::ComputerUseRun::new(true, false);
                    let epoch = run.control.begin(Capability::Desktop).unwrap();
                    let observation =
                        super::super::tests::fixture_observation(&run.control, epoch, 0);
                    let super::super::Target::Desktop(target) = observation.target else {
                        panic!()
                    };
                    run.control
                        .set_windows(epoch, vec![target.clone()])
                        .unwrap();
                    let mut response = result();
                    response.is_error = is_error;
                    let structured = response.structured.as_mut().unwrap();
                    structured["code"] = json!("px_capture_unavailable");
                    structured["degraded"] = json!(degraded);
                    structured["screenshot_error"] = screenshot_error.clone();
                    let actions = run
                        .install_desktop_fixture(vec![target], vec![response])
                        .await;
                    let observed = run
                        .execute(
                            "ComputerObserve",
                            "conflicting-frame",
                            json!({"target_id":"fixture","cursor":null}),
                            &tokio_util::sync::CancellationToken::new(),
                        )
                        .await;
                    assert_eq!(
                        observed,
                        Err(failure(Class::ScreenshotUnverified)),
                        "is_error={is_error} degraded={degraded} error={screenshot_error}"
                    );
                    assert_eq!(
                        run.control.status().phase,
                        super::super::ControlPhase::Failed
                    );
                    assert!(run.control.snapshot().observations.is_empty());
                    let mut message =
                        crate::ChatMessage::tool_result("conflicting-frame", "failed");
                    run.apply_observation("conflicting-frame", &mut message);
                    assert!(message.images.is_empty());
                    assert!(!message.content.contains("PRIVATE"));
                    assert!(
                        run.execute(
                            "ComputerAct",
                            "after-conflicting-frame",
                            json!({"action":"click","x":1,"y":1,"observation_id":"frame-0"}),
                            &tokio_util::sync::CancellationToken::new(),
                        )
                        .await
                        .is_err()
                    );
                    assert_eq!(actions.load(std::sync::atomic::Ordering::SeqCst), 0);
                    run.control.stop();
                    run.control.finish_cleanup();
                }
            }
        }
    }

    #[test]
    fn issue75_observation_null_screenshot_error_keeps_verified_content() {
        let control = super::super::ComputerUseControl::new();
        for degraded in [false, true] {
            let mut observation = super::super::tests::fixture_observation(&control, 1, 0);
            let super::super::Target::Desktop(target) = observation.target.clone() else {
                panic!()
            };
            let mut response = result();
            let structured = response.structured.as_mut().unwrap();
            structured["degraded"] = json!(degraded);
            structured["screenshot_error"] = Value::Null;
            apply(&mut observation, &target, response).unwrap();
            assert!(observation.image.is_some());
            assert_eq!(observation.read_only, degraded);
            assert_eq!(observation.snapshot_id.as_deref(), Some("snapshot"));
            assert_eq!(observation.capture_id.as_deref(), Some("capture"));
        }
    }

    #[test]
    fn issue75_observation_metadata_classes_are_fixed_and_content_free() {
        let control = super::super::ComputerUseControl::new();
        let launching = "ax_app_launching: window_id 2 exists and is owned by pid 1, but that app has not finished launching and did not answer accessibility within the 1500 ms timeout_ms budget. The tree is returned EMPTY because the window's accessibility surface is not available yet.";
        let unresolved = "ax_window_unresolved: window_id 2 exists and is owned by pid 1, but none of the 2 AXWindow element(s) under that pid reports this CGWindowID. The tree is returned EMPTY on purpose: the accessibility elements reachable under this pid belong to other surfaces (the menu bar, other windows), not to the requested window, so presenting them would misground the next action.";
        for (patch, class, recoverable) in [
            (json!({"pid":4}), Class::ObservationIdentityMismatch, false),
            (
                json!({"screenshot_frame_valid":null}),
                Class::ScreenshotUnverified,
                false,
            ),
            (
                json!({"screenshot_frame_valid":false,"screenshot_error":{"code":"px_capture_unavailable","reason":"PRIVATE_RAW"}}),
                Class::CaptureUnavailable,
                true,
            ),
            (
                json!({"screenshot_frame_valid":false,"screenshot_error":{"code":"px_frame_mismatch","reason":"PRIVATE_RAW"}}),
                Class::CaptureFrameMismatch,
                false,
            ),
            (
                json!({"screenshot_frame_valid":false,"screenshot_error":{"code":"px_capture_unavailable"},"degraded_reason":"PRIVATE_UNKNOWN"}),
                Class::DegradedUnknown,
                false,
            ),
            (json!({"capture_id":""}), Class::CaptureBindingFailed, false),
            (
                json!({"snapshot_id":"x".repeat(513)}),
                Class::CaptureBindingFailed,
                false,
            ),
            (
                json!({"degraded_reason":launching,"snapshot_id":null,"capture_id":null}),
                Class::AxAppLaunching,
                true,
            ),
            (
                json!({"degraded_reason":unresolved,"snapshot_id":null,"capture_id":null}),
                Class::AxWindowUnresolved,
                true,
            ),
            (
                json!({"degraded_reason":format!("{unresolved} PRIVATE_RAW")}),
                Class::DegradedUnknown,
                false,
            ),
            (
                json!({"degraded_reason":launching.replace("pid 1,","pid 3,")}),
                Class::DegradedUnknown,
                false,
            ),
        ] {
            let mut response = result();
            for (key, value) in patch.as_object().unwrap() {
                response.structured.as_mut().unwrap()[key] = value.clone();
            }
            let mut observation = super::super::tests::fixture_observation(&control, 1, 0);
            let super::super::Target::Desktop(target) = observation.target.clone() else {
                panic!()
            };
            let error = apply(&mut observation, &target, response).unwrap_err();
            assert_eq!(error, failure(class));
            assert_eq!(diagnostics::recoverable_observe(error), recoverable);
            assert!(!error.to_string().contains("PRIVATE"));
        }
    }
}
