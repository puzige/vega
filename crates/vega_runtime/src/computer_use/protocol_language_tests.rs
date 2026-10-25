use super::diagnostics::{self, Class, Stage};
use super::feedback::{ExecutionState, ToolFailure};
use super::progress::{GuardDiagnostic, GuardTrigger, UnderlyingResult};
use super::{Capability, ComputerUseError, SUPERSEDED_CONTROL_OUTPUT, valid_error_output};

struct RestoreLanguage(&'static str);

impl Drop for RestoreLanguage {
    fn drop(&mut self) {
        vega_i18n::set_language(self.0);
    }
}

#[test]
fn generated_errors_keep_canonical_diagnostics_across_ui_languages() {
    let _restore = RestoreLanguage(vega_i18n::language());
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
        ComputerUseError::ObservationBudget { executed: false },
        ComputerUseError::ObservationBudget { executed: true },
        ComputerUseError::NoProgress,
        ComputerUseError::ScreenshotLimit,
    ];
    for field in [
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
    ] {
        errors.push(ComputerUseError::InvalidArguments(field));
    }
    for class in [
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
    ] {
        for capability in [Capability::Desktop, Capability::Browser] {
            errors.push(diagnostics::failure(capability, Stage::Action, class));
        }
    }
    let mut cases = Vec::new();
    for error in errors {
        let capability = match error {
            ComputerUseError::Executor(error) => error.capability(),
            _ => Capability::Desktop,
        };
        for state in [
            ExecutionState::NotDispatched,
            ExecutionState::Failed,
            ExecutionState::OutcomeUnknown,
        ] {
            if matches!(error, ComputerUseError::ObservationBudget { executed } if executed != (state != ExecutionState::NotDispatched))
            {
                continue;
            }
            let mut failure = ToolFailure::new(error, capability, Stage::Action, state);
            failure.guard = match error {
                ComputerUseError::NoProgress => Some(GuardDiagnostic {
                    trigger: GuardTrigger::RepeatedDirectory,
                    count: 3,
                    threshold: 3,
                    underlying_result: UnderlyingResult::Succeeded,
                    dispatch_started: true,
                }),
                ComputerUseError::ScreenshotLimit => Some(GuardDiagnostic {
                    trigger: GuardTrigger::ScreenshotCount,
                    count: crate::images::MAX_IMAGES + 1,
                    threshold: crate::images::MAX_IMAGES,
                    underlying_result: UnderlyingResult::Succeeded,
                    dispatch_started: true,
                }),
                _ => None,
            };
            cases.push(failure);
        }
    }
    vega_i18n::set_language("zh-CN");
    let chinese_display = ComputerUseError::Permissions.user_message();
    let chinese = cases
        .iter()
        .map(|failure| failure.feedback())
        .collect::<Vec<_>>();
    vega_i18n::set_language("en");
    assert_ne!(
        ComputerUseError::Permissions.user_message(),
        chinese_display
    );
    for (failure, historical) in cases.iter().zip(&chinese) {
        let name = match failure.capability {
            Capability::Desktop => "ComputerAct",
            Capability::Browser => "BrowserAct",
        };
        let english = failure.feedback();
        assert_eq!(&english, historical);
        assert!(english.len() <= 2048);
        assert!(valid_error_output(name, historical, false), "{historical}");
        if failure.execution_state == ExecutionState::NotDispatched {
            assert!(valid_error_output(name, historical, true));
        }
        let mut forged: serde_json::Value = serde_json::from_str(historical).unwrap();
        forged["diagnostic"] = "untrusted diagnostic".into();
        assert!(!valid_error_output(name, &forged.to_string(), false));
    }
    let run = super::ComputerUseRun::new(true, false);
    let screenshot_budget = run.error_feedback(
        "ComputerScreenshot",
        ComputerUseError::ObservationBudget { executed: true },
        true,
    );
    let expected: serde_json::Value = serde_json::from_str(r#"{"code":"observation_budget_exceeded","executed":true,"deferred":true,"retryable":true,"next_step":"use_current_results_then_request_remaining_windows","field":null,"capability":"desktop","stage":"screenshot","execution_state":"failed","diagnostic":"\u672c\u6279\u89c2\u5bdf\u5df2\u8fbe\u5230\u5bb9\u91cf\u9650\u5236\uff0c\u8bf7\u5148\u4f7f\u7528\u5df2\u6709\u7ed3\u679c\uff0c\u518d\u5206\u6279\u8bfb\u53d6\u5176\u4f59\u7a97\u53e3\u3002"}"#).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&screenshot_budget).unwrap(),
        expected
    );
    assert!(valid_error_output(
        "ComputerScreenshot",
        &screenshot_budget,
        false
    ));
    vega_i18n::set_language("zh-CN");
    assert_eq!(
        run.error_feedback(
            "ComputerScreenshot",
            ComputerUseError::ObservationBudget { executed: true },
            true,
        ),
        screenshot_budget
    );
    for (failure, historical) in cases.iter().zip(&chinese) {
        let name = match failure.capability {
            Capability::Desktop => "ComputerAct",
            Capability::Browser => "BrowserAct",
        };
        assert_eq!(&failure.feedback(), historical);
        assert!(valid_error_output(name, historical, false));
    }
    assert_eq!(
        ToolFailure::before_dispatch("ComputerAct", ComputerUseError::Superseded).feedback(),
        SUPERSEDED_CONTROL_OUTPUT
    );
}
