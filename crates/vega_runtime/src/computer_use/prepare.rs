#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComponentState {
    Available,
    Missing,
    Incompatible,
    UnsupportedPlatform,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionState {
    Granted,
    Denied,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectCaptureReason {
    Unavailable,
    TimedOut,
    ProbeFailed,
    BlockedByScreenRecording,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectCapture {
    Ready,
    NotReady(DirectCaptureReason),
    NotChecked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtensionState {
    NotEvaluated,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preparation {
    pub component: ComponentState,
    pub accessibility: PermissionState,
    pub screen_recording: PermissionState,
    pub direct_capture: DirectCapture,
    pub browser_extension: ExtensionState,
}

impl Preparation {
    pub fn desktop_ready(&self) -> bool {
        self.component == ComponentState::Available
            && self.accessibility == PermissionState::Granted
            && self.screen_recording == PermissionState::Granted
            && self.direct_capture == DirectCapture::Ready
    }

    pub fn needs_accessibility_settings(&self) -> bool {
        self.accessibility == PermissionState::Denied
    }

    pub fn needs_screen_recording_settings(&self) -> bool {
        match self.screen_recording {
            PermissionState::Denied => true,
            PermissionState::Granted => self.direct_capture != DirectCapture::Ready,
            PermissionState::Unknown => false,
        }
    }
}

pub fn unknown_preparation(component: ComponentState) -> Preparation {
    Preparation {
        component,
        accessibility: PermissionState::Unknown,
        screen_recording: PermissionState::Unknown,
        direct_capture: DirectCapture::NotChecked,
        browser_extension: ExtensionState::NotEvaluated,
    }
}

pub fn preparation_from_check_permissions(
    component: ComponentState,
    structured: Option<&serde_json::Value>,
) -> Preparation {
    let Some(structured) = structured else {
        return unknown_preparation(component);
    };
    Preparation {
        component,
        accessibility: permission_state(structured, "accessibility"),
        screen_recording: permission_state(structured, "screen_recording"),
        direct_capture: direct_capture_state(structured),
        browser_extension: ExtensionState::NotEvaluated,
    }
}

fn permission_state(structured: &serde_json::Value, key: &str) -> PermissionState {
    match structured.get(key).and_then(serde_json::Value::as_bool) {
        Some(true) => PermissionState::Granted,
        Some(false) => PermissionState::Denied,
        None => PermissionState::Unknown,
    }
}

fn direct_capture_state(structured: &serde_json::Value) -> DirectCapture {
    let capturable = matches!(
        structured.get("screen_recording_capturable"),
        Some(serde_json::Value::Bool(true))
    );
    let status = structured
        .get("direct_capture_status")
        .and_then(serde_json::Value::as_str);
    match status {
        Some("ready") if capturable => DirectCapture::Ready,
        Some("unavailable") => DirectCapture::NotReady(DirectCaptureReason::Unavailable),
        Some("timed_out") => DirectCapture::NotReady(DirectCaptureReason::TimedOut),
        Some("probe_failed") => DirectCapture::NotReady(DirectCaptureReason::ProbeFailed),
        Some("blocked_by_screen_recording") => {
            DirectCapture::NotReady(DirectCaptureReason::BlockedByScreenRecording)
        }
        _ => DirectCapture::NotChecked,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptMode {
    Inspect,
    UserInitiated,
}

impl PromptMode {
    pub fn prompt(self) -> bool {
        matches!(self, Self::UserInitiated)
    }
}
