use super::{Capability, ComputerUseError as Error};
use vega_mcp::{ManagedResult, McpError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stage {
    Initialize,
    Permissions,
    ListApps,
    ListWindows,
    TargetList,
    TargetParse,
    Observe,
    Screenshot,
    Action,
    ActionPreflight,
}

impl Stage {
    fn label(self) -> &'static str {
        match self {
            Self::Initialize => "initialize",
            Self::Permissions => "permissions",
            Self::ListApps => "list_apps",
            Self::ListWindows => "list_windows",
            Self::TargetList => "target_list",
            Self::TargetParse => "target_parse",
            Self::Observe => "observe",
            Self::Screenshot => "screenshot",
            Self::Action => "action",
            Self::ActionPreflight => "action_preflight",
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
    AttachFailed,
    CdpCommandFailed,
    TargetUnavailable,
    TargetCount,
    TargetFormat,
    TargetNotCurrent,
    AxUnavailable,
    AxTimeout,
    ObservationUnavailable,
    StaleObservation,
    BackgroundUnavailable,
    OffSpaceOrAxUnresolved,
    MinimizedOrHiddenWindow,
    SamePidKeyboardAmbiguity,
    InvalidCoordinates,
    InvalidImage,
}

impl Class {
    pub(super) fn code(self) -> &'static str {
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
            Self::AttachFailed => "attach_failed",
            Self::CdpCommandFailed => "cdp_command_failed",
            Self::TargetUnavailable => "target_unavailable",
            Self::TargetCount => "target_count",
            Self::TargetFormat => "target_format",
            Self::TargetNotCurrent => "target_not_current",
            Self::AxUnavailable => "accessibility_unavailable",
            Self::AxTimeout => "accessibility_timeout",
            Self::ObservationUnavailable => "observation_unavailable",
            Self::StaleObservation => "stale_observation",
            Self::BackgroundUnavailable => "background_unavailable",
            Self::OffSpaceOrAxUnresolved => "off_space_or_ax_unresolved",
            Self::MinimizedOrHiddenWindow => "minimized_or_hidden_window",
            Self::SamePidKeyboardAmbiguity => "same_pid_keyboard_ambiguity",
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
            Self::AttachFailed => {
                "浏览器未能附着到所选标签页，请检查目标是否可调试及是否已有其他调试连接。"
            }
            Self::CdpCommandFailed => {
                "浏览器调试协议命令失败，请检查扩展连接和所选标签页后重新连接。"
            }
            Self::PolicyDenied => "执行器范围授权拒绝了请求，请停止后重新选择并授权目标。",
            Self::TargetUnavailable => "执行器无法确认目标，请检查窗口或标签页后重新连接。",
            Self::TargetCount => {
                "执行器没有返回唯一标签页，请保持连接范围内只有所选目标后重新连接。"
            }
            Self::TargetFormat => "执行器目标列表格式无法验证，请检查组件版本后重新连接。",
            Self::TargetNotCurrent => "执行器未确认所选标签页为当前目标，请重新连接并选择目标。",
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
            Self::OffSpaceOrAxUnresolved => {
                "目标窗口的辅助功能归属无法确认，请检查目标所在空间并重新观察。"
            }
            Self::MinimizedOrHiddenWindow => {
                "目标可见性无法确认，可能隐藏、最小化或状态不可读；请检查目标并重新观察。"
            }
            Self::SamePidKeyboardAmbiguity => {
                "同一应用存在其他键盘目标，执行器无法保证输入落在所选窗口；请检查窗口后重新观察。"
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
            if self.capability == Capability::Desktop && self.class == Class::PolicyDenied {
                "执行器工具策略拒绝了请求，请检查组件安装与受管策略。"
            } else {
                self.class.guidance()
            }
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

pub(super) fn background_refusal(code: &str) -> Option<Class> {
    match code {
        "off_space_or_ax_unresolved" => Some(Class::OffSpaceOrAxUnresolved),
        "minimized_or_hidden_window" => Some(Class::MinimizedOrHiddenWindow),
        "same_pid_keyboard_ambiguity" => Some(Class::SamePidKeyboardAmbiguity),
        "window_not_found" | "owner_pid_mismatch" => Some(Class::TargetUnavailable),
        _ => None,
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
        if let Some(class) = code.and_then(background_refusal) {
            return class;
        }
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
            Some("background_unavailable") => return Class::BackgroundUnavailable,
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
    if capability == Capability::Browser {
        return classify_browser(result.text.first().map_or("", String::as_str));
    }
    for text in &result.text {
        let text = text.strip_prefix("### Error\n").unwrap_or(text);
        let text = text.strip_prefix("Error: ").unwrap_or(text);
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
    Class::Unknown
}

fn classify_browser(text: &str) -> Class {
    let text = text.strip_prefix("### Error\n").unwrap_or(text);
    let text = text.lines().next().unwrap_or_default();
    if text.starts_with("TimeoutError: ") {
        return Class::Timeout;
    }
    let mut text = text.strip_prefix("Error: ").unwrap_or(text);
    for _ in 0..3 {
        let inner = [
            "browserType.connectOverCDP: ",
            "browser.bind: ",
            "browserContext.newPage: ",
        ]
        .into_iter()
        .find_map(|prefix| text.strip_prefix(prefix));
        match inner {
            Some(inner) => text = inner,
            None => break,
        }
    }
    let protocol = text
        .strip_prefix("Protocol error (")
        .and_then(|inner| inner.split_once("): "))
        .filter(|(method, _)| valid_cdp_method(method));
    let text = protocol.map_or(text, |(_, message)| message);
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
    if text == "\"chrome\" executable not found. Make sure it is installed at a standard location."
    {
        return Class::BrowserMissing;
    }
    if text.starts_with("No attached tab to forward browser-level command: ")
        || text.starts_with("No tab found for sessionId: ")
    {
        return Class::TargetUnavailable;
    }
    let existing_debugger = text
        .strip_prefix("Another debugger is already attached to the tab with id: ")
        .and_then(|suffix| suffix.strip_suffix('.'))
        .is_some_and(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()));
    if existing_debugger
        || matches!(
            text,
            "Cannot attach to this target."
                | "Cannot access a chrome:// URL"
                | "Cannot access a chrome-extension:// URL of different extension"
        )
    {
        return Class::AttachFailed;
    }
    if protocol.is_some() {
        Class::CdpCommandFailed
    } else {
        Class::Unknown
    }
}

fn valid_cdp_method(method: &str) -> bool {
    method.len() <= 128
        && method.split_once('.').is_some_and(|(domain, name)| {
            [domain, name].into_iter().all(|part| {
                part.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
                    && part.bytes().all(|byte| byte.is_ascii_alphanumeric())
            })
        })
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
    fn issue75_browser_error_wrappers_preserve_finite_first_line_diagnostics() {
        for (text, expected) in [
            (
                "### Error\nError: browserType.connectOverCDP: Playwright Extension not found in \"/Users/SECRET_USER\". SECRET_TOKEN",
                "extension_missing",
            ),
            (
                "### Error\nError: browser.bind: Extension not connected\nCall log:\nSECRET_PAGE",
                "extension_connection",
            ),
            (
                "### Error\nError: browserType.connectOverCDP: Protocol error (Browser.getVersion): No attached tab to forward browser-level command: Browser.getVersion\nCall log:\nSECRET_TOKEN",
                "target_unavailable",
            ),
            (
                "### Error\nError: browserContext.newPage: Protocol error (Target.createTarget): Another debugger is already attached to the tab with id: 123456.\nCall log:\nSECRET_PAGE",
                "attach_failed",
            ),
            (
                "### Error\nError: browserContext.newPage: Protocol error (Target.createTarget): Cannot attach to this target.",
                "attach_failed",
            ),
            (
                "### Error\nError: Protocol error (Page.enable): No tab found for sessionId: pw-tab-123456",
                "target_unavailable",
            ),
            (
                "### Error\nError: Protocol error (Target.createTarget): Cannot access a chrome:// URL",
                "attach_failed",
            ),
            (
                "### Error\nError: Cannot access a chrome-extension:// URL of different extension",
                "attach_failed",
            ),
            (
                "### Error\nError: browserType.connectOverCDP: Protocol error (Browser.getVersion): SECRET_PAGE",
                "cdp_command_failed",
            ),
            (
                "### Error\nTimeoutError: browserType.connectOverCDP: SECRET_PAGE",
                "timeout",
            ),
        ] {
            let error = checked(
                Capability::Browser,
                Stage::TargetList,
                Ok(rejected(text, json!({}))),
            )
            .err()
            .unwrap();
            for output in [error.to_string(), format!("{error:?}")] {
                assert!(!output.contains("SECRET"));
                assert!(!output.contains("123456"));
                assert!(!output.contains("chrome://"));
                assert!(!output.contains("/Users/"));
            }
            assert!(
                error
                    .to_string()
                    .contains(&format!("error_class={expected}:")),
                "{text}: {error}"
            );
        }
    }

    #[test]
    fn issue75_browser_error_classification_ignores_logs_and_later_text_blocks() {
        for text in [
            "### Error\nError: SECRET_PAGE\nCall log:\nError: Cannot attach to this target.",
            "### Error\nError: SECRET_PAGE\n\nError: Extension not connected",
            "### Error\nError: SECRET_PAGE\n### Result\nError: No tab found for sessionId: pw-tab-123456",
            "### Error\nError: browser.unknownApi: Cannot attach to this target.",
            "### Error\nError: Protocol error (SECRET METHOD): Cannot attach to this target.",
            "### Error\nError: Another debugger is already attached to the tab with id: SECRET_TOKEN.",
        ] {
            let mut result = rejected(text, json!({}));
            result.text.push("Error: Extension not connected".into());
            let error = checked(Capability::Browser, Stage::TargetList, Ok(result))
                .err()
                .unwrap();
            assert!(
                error.to_string().contains("error_class=unknown:"),
                "{text}: {error}"
            );
            for output in [error.to_string(), format!("{error:?}")] {
                assert!(!output.contains("SECRET"));
                assert!(!output.contains("123456"));
            }
        }
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
    #[test]
    fn issue75_background_fixed_refusals_keep_distinct_safe_classes() {
        for code in [
            "off_space_or_ax_unresolved",
            "minimized_or_hidden_window",
            "same_pid_keyboard_ambiguity",
        ] {
            let error = checked(Capability::Desktop, Stage::Action, Ok(ManagedResult {
                text: vec!["SECRET_INPUT SECRET_PAGE".into()],
                images: Vec::new(),
                structured: Some(serde_json::json!({"status":"refused","refusal":{"code":code,"reason":"SECRET_REASON","escalation":"SECRET_PATH"}})),
                is_error: true,
            })).err().unwrap();
            let output = error.to_string();
            assert!(output.contains(&format!("error_class={code}")), "{output}");
            assert!(!output.contains("SECRET"));
            assert!(!format!("{error:?}").contains("SECRET"));
        }
    }
}
