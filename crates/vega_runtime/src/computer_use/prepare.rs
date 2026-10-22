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
        false
    }

    pub fn needs_accessibility_settings(&self) -> bool {
        false
    }

    pub fn needs_screen_recording_settings(&self) -> bool {
        false
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
    let _ = structured;
    unknown_preparation(component)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptMode {
    Inspect,
    UserInitiated,
}

impl PromptMode {
    pub fn prompt(self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn available(structured: serde_json::Value) -> Preparation {
        preparation_from_check_permissions(ComponentState::Available, Some(&structured))
    }

    #[test]
    fn both_granted_and_capturable_ready() {
        let preparation = available(json!({
            "accessibility": true,
            "screen_recording": true,
            "screen_recording_capturable": true,
            "direct_capture_status": "ready"
        }));

        assert_eq!(preparation.component, ComponentState::Available);
        assert_eq!(preparation.accessibility, PermissionState::Granted);
        assert_eq!(preparation.screen_recording, PermissionState::Granted);
        assert_eq!(preparation.direct_capture, DirectCapture::Ready);
        assert_eq!(preparation.browser_extension, ExtensionState::NotEvaluated);
        assert!(preparation.desktop_ready());
        assert!(!preparation.needs_accessibility_settings());
        assert!(!preparation.needs_screen_recording_settings());
    }

    #[test]
    fn accessibility_denied_screen_recording_granted() {
        let preparation = available(json!({
            "accessibility": false,
            "screen_recording": true,
            "screen_recording_capturable": true,
            "direct_capture_status": "ready"
        }));

        assert_eq!(preparation.accessibility, PermissionState::Denied);
        assert_eq!(preparation.screen_recording, PermissionState::Granted);
        assert_eq!(preparation.direct_capture, DirectCapture::Ready);
        assert!(!preparation.desktop_ready());
        assert!(preparation.needs_accessibility_settings());
        assert!(!preparation.needs_screen_recording_settings());
    }

    #[test]
    fn both_denied() {
        let preparation = available(json!({
            "accessibility": false,
            "screen_recording": false,
            "screen_recording_capturable": null,
            "direct_capture_status": "blocked_by_screen_recording"
        }));

        assert_eq!(preparation.accessibility, PermissionState::Denied);
        assert_eq!(preparation.screen_recording, PermissionState::Denied);
        assert_eq!(
            preparation.direct_capture,
            DirectCapture::NotReady(DirectCaptureReason::BlockedByScreenRecording)
        );
        assert!(!preparation.desktop_ready());
        assert!(preparation.needs_accessibility_settings());
        assert!(preparation.needs_screen_recording_settings());
    }

    #[test]
    fn granted_but_capture_not_ready() {
        let preparation = available(json!({
            "accessibility": true,
            "screen_recording": true,
            "screen_recording_capturable": false,
            "direct_capture_status": "probe_failed"
        }));

        assert_eq!(preparation.accessibility, PermissionState::Granted);
        assert_eq!(preparation.screen_recording, PermissionState::Granted);
        assert_eq!(
            preparation.direct_capture,
            DirectCapture::NotReady(DirectCaptureReason::ProbeFailed)
        );
        assert!(!preparation.desktop_ready());
    }

    #[test]
    fn capturable_true_but_status_not_ready() {
        let preparation = available(json!({
            "accessibility": true,
            "screen_recording": true,
            "screen_recording_capturable": true,
            "direct_capture_status": "timed_out"
        }));

        assert_eq!(preparation.screen_recording, PermissionState::Granted);
        assert_eq!(
            preparation.direct_capture,
            DirectCapture::NotReady(DirectCaptureReason::TimedOut)
        );
        assert!(!preparation.desktop_ready());
    }

    #[test]
    fn status_ready_but_capturable_false_is_not_ready() {
        let preparation = available(json!({
            "accessibility": true,
            "screen_recording": true,
            "screen_recording_capturable": false,
            "direct_capture_status": "ready"
        }));

        assert_ne!(preparation.direct_capture, DirectCapture::Ready);
        assert!(!preparation.desktop_ready());
    }

    #[test]
    fn status_unavailable() {
        let preparation = available(json!({
            "accessibility": true,
            "screen_recording": true,
            "screen_recording_capturable": false,
            "direct_capture_status": "unavailable"
        }));

        assert_eq!(
            preparation.direct_capture,
            DirectCapture::NotReady(DirectCaptureReason::Unavailable)
        );
        assert!(!preparation.desktop_ready());
    }

    #[test]
    fn not_checked() {
        let preparation = available(json!({
            "accessibility": true,
            "screen_recording": true,
            "screen_recording_capturable": null,
            "direct_capture_status": "not_checked"
        }));

        assert_eq!(preparation.direct_capture, DirectCapture::NotChecked);
        assert!(!preparation.desktop_ready());
    }

    #[test]
    fn unknown_status_string_is_not_checked() {
        let preparation = available(json!({
            "accessibility": true,
            "screen_recording": true,
            "screen_recording_capturable": true,
            "direct_capture_status": "something_new"
        }));

        assert_eq!(preparation.direct_capture, DirectCapture::NotChecked);
        assert!(!preparation.desktop_ready());
    }

    #[test]
    fn missing_fields_are_unknown_not_denied() {
        let preparation = available(json!({}));

        assert_eq!(preparation.accessibility, PermissionState::Unknown);
        assert_ne!(preparation.accessibility, PermissionState::Denied);
        assert_eq!(preparation.screen_recording, PermissionState::Unknown);
        assert_ne!(preparation.screen_recording, PermissionState::Denied);
        assert_eq!(preparation.direct_capture, DirectCapture::NotChecked);
        assert_eq!(preparation.browser_extension, ExtensionState::NotEvaluated);
        assert!(!preparation.desktop_ready());
    }

    #[test]
    fn null_structured_is_probe_failure() {
        let preparation = preparation_from_check_permissions(ComponentState::Available, None);

        assert_eq!(preparation.accessibility, PermissionState::Unknown);
        assert_ne!(preparation.accessibility, PermissionState::Denied);
        assert_eq!(preparation.screen_recording, PermissionState::Unknown);
        assert_ne!(preparation.screen_recording, PermissionState::Denied);
        assert_eq!(preparation.direct_capture, DirectCapture::NotChecked);
        assert_eq!(preparation.browser_extension, ExtensionState::NotEvaluated);
        assert!(!preparation.desktop_ready());
    }

    #[test]
    fn non_bool_permission_is_unknown() {
        let preparation = available(json!({
            "accessibility": "true",
            "screen_recording": true,
            "screen_recording_capturable": true,
            "direct_capture_status": "ready"
        }));

        assert_eq!(preparation.accessibility, PermissionState::Unknown);
        assert_eq!(preparation.screen_recording, PermissionState::Granted);
        assert!(!preparation.desktop_ready());
    }

    #[test]
    fn unknown_preparation_keeps_component() {
        let preparation = unknown_preparation(ComponentState::Missing);

        assert_eq!(preparation.component, ComponentState::Missing);
        assert_eq!(preparation.accessibility, PermissionState::Unknown);
        assert_eq!(preparation.screen_recording, PermissionState::Unknown);
        assert_eq!(preparation.direct_capture, DirectCapture::NotChecked);
        assert_eq!(preparation.browser_extension, ExtensionState::NotEvaluated);
    }

    #[test]
    fn component_unavailable_blocks_ready() {
        let preparation = preparation_from_check_permissions(
            ComponentState::Missing,
            Some(&json!({
                "accessibility": true,
                "screen_recording": true,
                "screen_recording_capturable": true,
                "direct_capture_status": "ready"
            })),
        );

        assert_eq!(preparation.component, ComponentState::Missing);
        assert_eq!(preparation.accessibility, PermissionState::Granted);
        assert_eq!(preparation.screen_recording, PermissionState::Granted);
        assert_eq!(preparation.direct_capture, DirectCapture::Ready);
        assert!(!preparation.desktop_ready());
    }

    #[test]
    fn prompt_mode_mapping() {
        assert!(!PromptMode::Inspect.prompt());
        assert!(PromptMode::UserInitiated.prompt());
    }
}
