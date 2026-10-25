use super::{Capability, ComputerUseError, ComputerUseRun, diagnostics, progress::GuardDiagnostic};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ExecutionState {
    NotDispatched,
    Failed,
    OutcomeUnknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ToolFailure {
    pub(super) error: ComputerUseError,
    pub(super) capability: Capability,
    pub(super) stage: diagnostics::Stage,
    pub(super) execution_state: ExecutionState,
    pub(super) guard: Option<GuardDiagnostic>,
}

impl ToolFailure {
    pub(super) fn new(
        error: ComputerUseError,
        capability: Capability,
        stage: diagnostics::Stage,
        execution_state: ExecutionState,
    ) -> Self {
        Self {
            error,
            capability,
            stage,
            execution_state,
            guard: None,
        }
    }

    pub(super) fn before_dispatch(name: &str, error: ComputerUseError) -> Self {
        Self::new(
            error,
            ComputerUseRun::tool_capability(name).unwrap_or(Capability::Desktop),
            match error {
                ComputerUseError::MissingComponents
                | ComputerUseError::IncompatibleComponents
                | ComputerUseError::UnsupportedPlatform
                | ComputerUseError::BrowserExtension => diagnostics::Stage::Initialize,
                ComputerUseError::Permissions => diagnostics::Stage::Permissions,
                _ => diagnostics::Stage::for_tool(name),
            },
            ExecutionState::NotDispatched,
        )
    }

    pub(crate) fn superseded(self) -> bool {
        self.error == ComputerUseError::Superseded
    }

    pub(crate) fn feedback(self) -> String {
        if self.superseded() {
            return super::SUPERSEDED_CONTROL_OUTPUT.into();
        }
        let mut value = self.error.feedback_value();
        let (capability, stage) = match self.error {
            ComputerUseError::Executor(error) => (error.capability(), error.stage()),
            ComputerUseError::NoProgress | ComputerUseError::ScreenshotLimit => {
                (self.capability, diagnostics::Stage::Progress)
            }
            _ => (self.capability, self.stage),
        };
        value["capability"] = serde_json::json!(capability);
        value["stage"] = serde_json::json!(stage);
        value["execution_state"] = serde_json::json!(self.execution_state);
        if value.get("diagnostic").is_none() {
            value["diagnostic"] = Value::String(self.error.protocol_message());
        }
        if let Some(guard) = self.guard {
            value["guard"] = serde_json::json!(guard);
        }
        if self.execution_state == ExecutionState::OutcomeUnknown {
            value["retryable"] = Value::Bool(false);
            value["next_step"] = Value::String("ask_user_to_verify_without_replay".into());
        }
        value.to_string()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredFailure {
    code: String,
    capability: Capability,
    stage: diagnostics::Stage,
    execution_state: ExecutionState,
    retryable: bool,
    next_step: String,
    field: Option<String>,
    diagnostic: String,
    guard: Option<GuardDiagnostic>,
    executed: Option<bool>,
    deferred: Option<bool>,
}

pub fn valid_error_output(name: &str, output: &str, rejected: bool) -> bool {
    if output.len() > 2048 {
        return false;
    }
    let Ok(stored) = serde_json::from_str::<StoredFailure>(output) else {
        return false;
    };
    if ComputerUseRun::tool_capability(name).is_none()
        || (stored.execution_state != ExecutionState::NotDispatched
            && ComputerUseRun::tool_capability(name) != Some(stored.capability))
        || (rejected && stored.execution_state != ExecutionState::NotDispatched)
        || (stored.execution_state == ExecutionState::OutcomeUnknown && !name.ends_with("Act"))
        || (matches!(stored.code.as_str(), "no_progress" | "screenshot_limit")
            != stored.guard.is_some())
        || stored.guard.is_some_and(|guard| {
            !guard.valid()
                || !matches!(stored.code.as_str(), "no_progress" | "screenshot_limit")
                || (matches!(
                    guard.trigger,
                    super::progress::GuardTrigger::ScreenshotCount
                        | super::progress::GuardTrigger::ScreenshotBytes
                ) != (stored.code == "screenshot_limit"))
                || (stored.execution_state != ExecutionState::NotDispatched
                    && !guard.dispatch_started)
        })
    {
        return false;
    }
    let error = diagnostics::Class::from_code(&stored.code)
        .map(|class| diagnostics::failure(stored.capability, stored.stage, class));
    let candidates = [
        ComputerUseError::InvalidAction,
        ComputerUseError::InvalidArguments("target_id"),
        ComputerUseError::InvalidArguments("cursor"),
        ComputerUseError::InvalidArguments("observation_id"),
        ComputerUseError::InvalidArguments("action"),
        ComputerUseError::InvalidArguments("target"),
        ComputerUseError::InvalidArguments("text"),
        ComputerUseError::InvalidArguments("key"),
        ComputerUseError::InvalidArguments("url"),
        ComputerUseError::InvalidArguments("x"),
        ComputerUseError::InvalidArguments("y"),
        ComputerUseError::InvalidArguments("direction"),
        ComputerUseError::InvalidArguments("amount"),
        ComputerUseError::CoordinateElementConflict,
        ComputerUseError::ReadOnlyObservation,
        ComputerUseError::StaleObservation,
        ComputerUseError::TargetExpired,
        ComputerUseError::TargetChanged,
        ComputerUseError::ObservationBudget { executed: false },
        ComputerUseError::ObservationBudget { executed: true },
        ComputerUseError::NoProgress,
        ComputerUseError::ScreenshotLimit,
        ComputerUseError::Permissions,
        ComputerUseError::Unavailable,
        ComputerUseError::MissingComponents,
        ComputerUseError::IncompatibleComponents,
        ComputerUseError::UnsupportedPlatform,
        ComputerUseError::Busy,
        ComputerUseError::NotReady,
        ComputerUseError::Cancelled,
        ComputerUseError::GrantExpired,
        ComputerUseError::BrowserExtension,
        ComputerUseError::Protocol,
        ComputerUseError::Rejected,
        ComputerUseError::Image,
    ];
    let Ok(actual) = serde_json::from_str::<Value>(output) else {
        return false;
    };
    if !actual.as_object().is_some_and(|object| {
        [
            "code",
            "capability",
            "stage",
            "execution_state",
            "retryable",
            "next_step",
            "field",
            "diagnostic",
        ]
        .iter()
        .all(|key| object.contains_key(*key))
    }) {
        return false;
    }
    error.into_iter().chain(candidates).any(|error| {
        let failure = ToolFailure {
            error,
            capability: stored.capability,
            stage: stored.stage,
            execution_state: stored.execution_state,
            guard: stored.guard,
        };
        let Ok(value) = serde_json::from_str::<Value>(&failure.feedback()) else {
            return false;
        };
        value == actual
            && value["retryable"] == stored.retryable
            && value["next_step"] == stored.next_step
            && value["field"] == serde_json::json!(stored.field)
            && value["diagnostic"] == stored.diagnostic
            && stored.executed.is_none_or(|executed| {
                stored.code == "observation_budget_exceeded"
                    && executed == (stored.execution_state != ExecutionState::NotDispatched)
            })
            && stored
                .deferred
                .is_none_or(|deferred| deferred && stored.code == "observation_budget_exceeded")
    })
}
