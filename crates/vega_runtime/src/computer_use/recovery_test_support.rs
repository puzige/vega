use super::*;
use std::sync::{Mutex, MutexGuard};

#[derive(Default)]
pub(super) struct DriverState {
    pub(super) permissions: [bool; 2],
    pub(super) connections: usize,
    pub(super) calls: Vec<String>,
    pub(super) windows: usize,
    pub(super) unknown_action: bool,
    pub(super) pause_on_action: Option<ComputerUseControl>,
}

#[derive(Clone)]
pub struct Driver(pub(super) Arc<Mutex<DriverState>>);

impl Driver {
    pub fn new(permissions: [bool; 2], unknown_action: bool) -> Self {
        Self(Arc::new(Mutex::new(DriverState {
            permissions,
            unknown_action,
            ..DriverState::default()
        })))
    }

    fn state(&self) -> MutexGuard<'_, DriverState> {
        self.0.lock().unwrap_or_else(|error| error.into_inner())
    }

    pub fn calls(&self) -> Vec<String> {
        self.state().calls.clone()
    }

    pub(super) fn connected(&self) {
        self.state().connections += 1;
    }

    pub(super) fn windows(&self) -> Result<Vec<DesktopTarget>, ComputerUseError> {
        self.state().windows += 1;
        Ok(vec![DesktopTarget {
            selection_id: "driver-reference".into(),
            application: "Example".into(),
            bundle_id: "example.test".into(),
            pid: 7,
            window_id: 11,
            title: "Private window text".into(),
        }])
    }

    pub(super) fn call(
        &self,
        name: &str,
        _: Value,
    ) -> Result<vega_mcp::ManagedResult, vega_mcp::McpError> {
        let mut state = self.state();
        state.calls.push(name.into());
        if name == "check_permissions" {
            return Ok(vega_mcp::ManagedResult {
                text: Vec::new(),
                images: Vec::new(),
                is_error: false,
                structured: Some(
                    json!({"accessibility":state.permissions[0],"screen_recording":state.permissions[1]}),
                ),
            });
        }
        if state.unknown_action && name == "click" {
            if let Some(control) = &state.pause_on_action {
                control.pause();
            }
            return Err(vega_mcp::McpError::Timeout);
        }
        if name == "get_window_state" {
            let mut encoded = std::io::Cursor::new(Vec::new());
            image::DynamicImage::new_rgba8(1, 1)
                .write_to(&mut encoded, image::ImageFormat::Png)
                .map_err(|_| vega_mcp::McpError::UnsupportedResult)?;
            return Ok(vega_mcp::ManagedResult {
                text: Vec::new(),
                images: vec![vega_mcp::ManagedImage {
                    bytes: encoded.into_inner(),
                    mime_type: "image/png".into(),
                }],
                is_error: false,
                structured: Some(json!({
                    "pid":7,"window_id":11,"screenshot_frame_valid":true,
                    "snapshot_id":"snapshot","capture_id":"capture",
                    "elements":[{"role":"AXButton","element_token":"button"}],
                    "tree_markdown":"Visible window content"
                })),
            });
        }
        panic!("unexpected managed call: {name}");
    }
}

pub async fn configured_run(driver: &Driver) -> ComputerUseRun {
    let mut run = ComputerUseRun::new(true, false);
    run.control =
        ComputerUseControl::with_authority(Arc::new(ownership::OwnershipAuthority::default()));
    run.backend.lock().await.test_driver = Some(driver.clone());
    run
}

pub fn fill_observation_text_budget(run: &ComputerUseRun) {
    if let Ok(mut batch) = run.model_batch.lock() {
        batch.fill_text_budget(
            DesktopTarget {
                selection_id: "earlier-reference".into(),
                application: "Example".into(),
                bundle_id: "example.test".into(),
                pid: 7,
                window_id: 11,
                title: "Earlier window".into(),
            },
            run.control.status().epoch,
        );
    }
}

pub fn finite_error_outputs() -> Vec<(String, String, bool)> {
    use diagnostics::{Class, Stage};
    let mut errors = vec![
        ComputerUseError::Unavailable,
        ComputerUseError::MissingComponents,
        ComputerUseError::IncompatibleComponents,
        ComputerUseError::UnsupportedPlatform,
        ComputerUseError::Busy,
        ComputerUseError::NotReady,
        ComputerUseError::Cancelled,
        ComputerUseError::TargetChanged,
        ComputerUseError::StaleObservation,
        ComputerUseError::GrantExpired,
        ComputerUseError::Permissions,
        ComputerUseError::BrowserExtension,
        ComputerUseError::Protocol,
        ComputerUseError::Rejected,
        ComputerUseError::Image,
        ComputerUseError::InvalidAction,
        ComputerUseError::CoordinateElementConflict,
        ComputerUseError::ReadOnlyObservation,
        ComputerUseError::TargetExpired,
        ComputerUseError::ScreenshotLimit,
        ComputerUseError::ObservationBudget { executed: false },
        ComputerUseError::ObservationBudget { executed: true },
        ComputerUseError::NoProgress,
    ];
    errors.extend(
        [
            "target_id",
            "cursor",
            "observation_id",
            "action",
            "target",
            "text",
            "key",
            "url",
            "x",
            "y",
            "direction",
            "amount",
        ]
        .map(ComputerUseError::InvalidArguments),
    );
    let mut outputs = Vec::new();
    for (name, capability) in [
        ("ComputerObserve", Capability::Desktop),
        ("BrowserObserve", Capability::Browser),
    ] {
        let mut variants = errors.clone();
        variants.extend(
            [
                Class::Unknown,
                Class::Transport,
                Class::Timeout,
                Class::Cancelled,
                Class::Protocol,
                Class::LimitExceeded,
                Class::RpcRejected,
                Class::PermissionsMissing,
                Class::AccessibilityPermissionMissing,
                Class::ScreenRecordingPermissionMissing,
                Class::PolicyDenied,
                Class::ExtensionMissing,
                Class::ExtensionConnection,
                Class::BrowserMissing,
                Class::AttachFailed,
                Class::CdpCommandFailed,
                Class::TargetUnavailable,
                Class::TargetCount,
                Class::TargetFormat,
                Class::TargetNotCurrent,
                Class::AxUnavailable,
                Class::AxTimeout,
                Class::ObservationIdentityMismatch,
                Class::ScreenshotUnverified,
                Class::CaptureUnavailable,
                Class::CaptureFrameMismatch,
                Class::CaptureBindingFailed,
                Class::CapturePublicationFailed,
                Class::AxAppLaunching,
                Class::AxWindowUnresolved,
                Class::DegradedUnknown,
                Class::NoObservationContent,
                Class::StaleObservation,
                Class::BackgroundUnavailable,
                Class::OffSpaceOrAxUnresolved,
                Class::MinimizedOrHiddenWindow,
                Class::SamePidKeyboardAmbiguity,
                Class::InvalidCoordinates,
                Class::InvalidImage,
            ]
            .map(|class| diagnostics::failure(capability, Stage::Observe, class)),
        );
        for error in variants {
            for rejected in [false, true] {
                if matches!(error, ComputerUseError::ObservationBudget { executed } if executed == rejected)
                {
                    continue;
                }
                let mut failure = ToolFailure::new(
                    error,
                    capability,
                    Stage::Observe,
                    if rejected {
                        ExecutionState::NotDispatched
                    } else {
                        ExecutionState::Failed
                    },
                );
                if matches!(
                    error,
                    ComputerUseError::NoProgress | ComputerUseError::ScreenshotLimit
                ) {
                    let screenshot = error == ComputerUseError::ScreenshotLimit;
                    failure.guard = Some(progress::GuardDiagnostic {
                        trigger: if screenshot {
                            progress::GuardTrigger::ScreenshotCount
                        } else {
                            progress::GuardTrigger::RepeatedWindow
                        },
                        count: if screenshot { 5 } else { 3 },
                        threshold: if screenshot { 4 } else { 3 },
                        underlying_result: progress::UnderlyingResult::Succeeded,
                        dispatch_started: true,
                    });
                }
                outputs.push((name.into(), failure.feedback(), rejected));
            }
        }
    }
    outputs
}
