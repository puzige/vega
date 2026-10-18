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
}
