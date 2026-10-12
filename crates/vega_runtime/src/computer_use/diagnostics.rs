use super::{Capability, ComputerUseError as Error};
use vega_mcp::{ManagedResult, McpError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stage {
    Initialize,
    Permissions,
    ListApps,
    ListWindows,
    TargetList,
    Observe,
    Screenshot,
    Action,
}

impl Stage {
    fn label(self) -> &'static str {
        match self {
            Self::Initialize => "initialize",
            Self::Permissions => "permissions",
            Self::ListApps => "list_apps",
            Self::ListWindows => "list_windows",
            Self::TargetList => "target_list",
            Self::Observe => "observe",
            Self::Screenshot => "screenshot",
            Self::Action => "action",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Class {
    Unknown,
    Transport,
    Timeout,
    Cancelled,
    Protocol,
    LimitExceeded,
    RpcRejected,
    PermissionsMissing,
    PolicyDenied,
    ExtensionMissing,
    ExtensionConnection,
    BrowserMissing,
    TargetUnavailable,
    AxUnavailable,
    AxTimeout,
    ObservationUnavailable,
    StaleObservation,
    BackgroundUnavailable,
    InvalidCoordinates,
    InvalidImage,
}

impl Class {
    fn code(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Transport => "transport",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::Protocol => "protocol",
            Self::LimitExceeded => "limit_exceeded",
            Self::RpcRejected => "rpc_rejected",
            Self::PermissionsMissing => "permissions_missing",
            Self::PolicyDenied => "policy_denied",
            Self::ExtensionMissing => "extension_missing",
            Self::ExtensionConnection => "extension_connection",
            Self::BrowserMissing => "browser_missing",
            Self::TargetUnavailable => "target_unavailable",
            Self::AxUnavailable => "accessibility_unavailable",
            Self::AxTimeout => "accessibility_timeout",
            Self::ObservationUnavailable => "observation_unavailable",
            Self::StaleObservation => "stale_observation",
            Self::BackgroundUnavailable => "background_unavailable",
            Self::InvalidCoordinates => "invalid_coordinates",
            Self::InvalidImage => "invalid_image",
        }
    }

    fn guidance(self) -> &'static str {
        match self {
            Self::PermissionsMissing => {
                "系统权限预检未通过，请在系统设置中核对 Vega 辅助功能和屏幕录制权限。"
            }
            Self::ExtensionMissing => {
                "执行器未发现扩展，请在正在使用的 Chrome 配置中检查 Playwright 扩展安装。"
            }
            Self::ExtensionConnection => {
                "扩展连接未完成，请检查 Chrome 中的 Playwright 连接选择页。"
            }
            Self::BrowserMissing => "执行器未找到浏览器，请检查 Chrome 是否安装在标准位置。",
            Self::PolicyDenied => "执行器范围授权拒绝了请求，请停止后重新选择并授权目标。",
            Self::TargetUnavailable => "执行器无法确认目标，请检查窗口或标签页后重新连接。",
            Self::AxUnavailable | Self::AxTimeout => {
                "窗口辅助功能观察未完成，请检查目标应用是否响应后重新连接。"
            }
            Self::ObservationUnavailable | Self::InvalidImage => {
                "目标观察无法验证，请检查目标窗口或页面后重新连接。"
            }
            Self::StaleObservation | Self::InvalidCoordinates => {
                "执行器拒绝了过期或不匹配的观察，请检查目标后重新观察。"
            }
            Self::BackgroundUnavailable => {
                "目标不支持本次后台定向操作，请手动完成；Vega 不会改用前台或全屏输入。"
            }
            Self::Cancelled => "执行通道已取消，请检查目标；已派发动作不能自动重试。",
            Self::Protocol | Self::LimitExceeded => {
                "执行器响应不符合受管协议，请检查 Vega 安装和组件版本。"
            }
            Self::Transport | Self::Timeout | Self::RpcRejected | Self::Unknown => {
                "执行器请求未完成，请记录诊断代码并检查目标后重新连接；不要重试结果未知的动作。"
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExecutorFailure {
    capability: Capability,
    stage: Stage,
    class: Class,
}

impl std::fmt::Display for ExecutorFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let executor = match self.capability {
            Capability::Desktop => "cua",
            Capability::Browser => "playwright",
        };
        write!(
            f,
            "executor={executor} stage={} error_class={}: {}",
            self.stage.label(),
            self.class.code(),
            self.class.guidance()
        )
    }
}

pub(super) fn failure(capability: Capability, stage: Stage, class: Class) -> Error {
    Error::Executor(ExecutorFailure {
        capability,
        stage,
        class,
    })
}

pub(super) fn transport(capability: Capability, stage: Stage, error: McpError) -> Error {
    let class = match error {
        McpError::Cancelled => return Error::Cancelled,
        McpError::Transport => Class::Transport,
        McpError::Timeout => Class::Timeout,
        McpError::LimitExceeded => Class::LimitExceeded,
        McpError::InvalidConfig
        | McpError::IncompatibleVersion
        | McpError::UnsupportedTransport
        | McpError::InvalidMessage
        | McpError::UnsupportedResult => Class::Protocol,
        McpError::Rpc(_) => Class::RpcRejected,
        _ => Class::Unknown,
    };
    failure(capability, stage, class)
}

pub(super) fn context(capability: Capability, stage: Stage, error: Error) -> Error {
    let class = match error {
        Error::Executor(_) => return error,
        Error::Cancelled => Class::Cancelled,
        Error::TargetChanged => Class::TargetUnavailable,
        Error::StaleObservation => Class::StaleObservation,
        Error::Image => Class::InvalidImage,
        Error::Protocol => Class::Protocol,
        _ => Class::Unknown,
    };
    failure(capability, stage, class)
}

pub(super) fn checked(
    capability: Capability,
    stage: Stage,
    result: Result<ManagedResult, McpError>,
) -> Result<ManagedResult, Error> {
    let result = result.map_err(|error| transport(capability, stage, error))?;
    if result.is_error {
        Err(failure(capability, stage, classify(capability, &result)))
    } else {
        Ok(result)
    }
}

fn classify(capability: Capability, result: &ManagedResult) -> Class {
    if capability == Capability::Desktop
        && let Some(structured) = result.structured.as_ref()
    {
        let code = structured
            .get("code")
            .and_then(serde_json::Value::as_str)
            .or_else(|| {
                (structured["status"] == "refused")
                    .then(|| structured["refusal"]["code"].as_str())
                    .flatten()
            });
        match code {
            Some(
                "window_id_not_found"
                | "window_owner_pid_mismatch"
                | "window_not_found"
                | "owner_pid_mismatch"
                | "px_window_not_found"
                | "element_outside_target_window"
                | "conflicting_element_target",
            ) => return Class::TargetUnavailable,
            Some(
                "permission_denied"
                | "authorization_required"
                | "authorization_declined"
                | "authorization_canceled"
                | "authorization_revoked"
                | "authorization_suspended"
                | "authorization_expired"
                | "authorization_scope_mismatch"
                | "bounded_resource_outside_manifest"
                | "protected_resource_scope_invalid"
                | "protected_resource_scope_stale"
                | "authorization_host_failed",
            ) => return Class::PolicyDenied,
            Some(
                "background_unavailable"
                | "off_space_or_ax_unresolved"
                | "minimized_or_hidden_window"
                | "same_pid_keyboard_ambiguity",
            ) => return Class::BackgroundUnavailable,
            Some(
                "stale_element_token"
                | "screenshot_context_missing"
                | "zoom_context_missing"
                | "capture_not_found"
                | "capture_expired"
                | "capture_generation_mismatch"
                | "capture_target_mismatch"
                | "capture_frame_mismatch",
            ) => return Class::StaleObservation,
            Some(
                "px_capture_unavailable" | "capture_binding_failed" | "capture_publication_failed",
            ) => return Class::ObservationUnavailable,
            Some("capture_coordinate_invalid") => return Class::InvalidCoordinates,
            _ => {}
        }
    }
    for text in &result.text {
        let text = text.strip_prefix("### Error\n").unwrap_or(text);
        let text = text.strip_prefix("Error: ").unwrap_or(text);
        match capability {
            Capability::Browser => {
                if text.starts_with("Playwright Extension not found in \"") {
                    return Class::ExtensionMissing;
                }
                if (text.starts_with("Playwright extension did not connect within ")
                    && text.contains("s after opening the connect page."))
                    || text.starts_with("Extension disconnected before initialization: ")
                    || text == "Extension not connected"
                {
                    return Class::ExtensionConnection;
                }
                if text
                    == "\"chrome\" executable not found. Make sure it is installed at a standard location."
                {
                    return Class::BrowserMissing;
                }
            }
            Capability::Desktop => {
                if text.starts_with("AX tree walk for pid=")
                    && text.contains("did not return within")
                    && text.contains("timeout_ms budget")
                {
                    return Class::AxTimeout;
                }
                if text.starts_with("AX tree walk failed:") {
                    return Class::AxUnavailable;
                }
                if text == "No content produced (neither AX tree nor screenshot succeeded)" {
                    return Class::ObservationUnavailable;
                }
            }
        }
    }
    Class::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rejected(text: &str, structured: serde_json::Value) -> ManagedResult {
        ManagedResult {
            text: vec![text.into()],
            images: Vec::new(),
            structured: Some(structured),
            is_error: true,
        }
    }

    #[test]
    fn issue75_runtime_fixes_known_executor_errors_are_finite_and_content_free() {
        for (capability, stage, text, structured, expected) in [
            (
                Capability::Browser,
                Stage::TargetList,
                "### Error\nError: Playwright Extension not found in \"/Users/SECRET_USER/Profile 1\". SECRET_TOKEN",
                json!({}),
                "extension_missing",
            ),
            (
                Capability::Browser,
                Stage::TargetList,
                "Error: Playwright extension did not connect within 30s after opening the connect page. SECRET_TOKEN",
                json!({}),
                "extension_connection",
            ),
            (
                Capability::Browser,
                Stage::TargetList,
                "\"chrome\" executable not found. Make sure it is installed at a standard location.",
                json!({}),
                "browser_missing",
            ),
            (
                Capability::Desktop,
                Stage::Observe,
                "SECRET_PAGE",
                json!({"code":"window_owner_pid_mismatch","pid":123,"reason":"SECRET_INPUT"}),
                "target_unavailable",
            ),
            (
                Capability::Desktop,
                Stage::Observe,
                "SECRET_PAGE",
                json!({"status":"refused","refusal":{"code":"permission_denied","reason":"SECRET_INPUT"}}),
                "policy_denied",
            ),
            (
                Capability::Desktop,
                Stage::Observe,
                "AX tree walk for pid=123 did not return within 10 s: timeout_ms budget. SECRET_INPUT",
                json!({}),
                "accessibility_timeout",
            ),
            (
                Capability::Desktop,
                Stage::Action,
                "SECRET_PAGE",
                json!({"code":"background_unavailable"}),
                "background_unavailable",
            ),
            (
                Capability::Desktop,
                Stage::Action,
                "SECRET_PAGE",
                json!({"status":"refused","refusal":{"code":"stale_element_token"}}),
                "stale_observation",
            ),
            (
                Capability::Browser,
                Stage::Observe,
                "Protocol error: SECRET_TOKEN permission denied",
                json!({"code":"permission_denied"}),
                "unknown",
            ),
            (
                Capability::Desktop,
                Stage::Observe,
                "permission denied SECRET_TOKEN",
                json!({"reason":"permission_denied"}),
                "unknown",
            ),
        ] {
            let error = checked(capability, stage, Ok(rejected(text, structured)))
                .err()
                .unwrap();
            let output = error.to_string();
            assert!(
                output.contains(&format!("stage={}", stage.label())),
                "{output}"
            );
            assert!(
                output.contains(&format!("error_class={expected}:")),
                "{output}"
            );
            assert!(output.len() < 1024);
            for safe in [output, format!("{error:?}")] {
                assert!(!safe.contains("SECRET"), "{safe}");
                assert!(!safe.contains("/Users/"));
                assert!(!safe.contains("123"));
            }
        }
        let mut result = rejected("SECRET_PAGE", json!({"code":"background_unavailable"}));
        result.is_error = false;
        assert!(checked(Capability::Desktop, Stage::Observe, Ok(result)).is_ok());
    }

    #[test]
    fn issue75_runtime_fixes_transport_errors_do_not_claim_system_permissions() {
        for (error, class) in [
            (McpError::Transport, "transport"),
            (McpError::Timeout, "timeout"),
            (McpError::InvalidMessage, "protocol"),
            (McpError::Rpc(-32603), "rpc_rejected"),
        ] {
            let output = checked(Capability::Desktop, Stage::Observe, Err(error))
                .err()
                .unwrap()
                .to_string();
            assert!(output.contains("executor=cua stage=observe"));
            assert!(output.contains(&format!("error_class={class}:")));
            assert!(!output.contains("permissions_missing"));
        }
        assert_eq!(
            checked(
                Capability::Browser,
                Stage::TargetList,
                Err(McpError::Cancelled)
            )
            .err(),
            Some(Error::Cancelled)
        );
    }
}
