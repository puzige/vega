use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskBackend {
    Native,
    Codex,
}

impl TaskBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Codex => "codex",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "native" => Some(Self::Native),
            "codex" => Some(Self::Codex),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexAdapterKind {
    CodexAcp,
}

impl CodexAdapterKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CodexAcp => "codex_acp",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "codex_acp" => Some(Self::CodexAcp),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexProfileReference {
    pub id: String,
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexAdapterArgument(String);

impl CodexAdapterArgument {
    pub fn new(value: impl Into<String>) -> Result<Self, ConversationError> {
        let value = value.into();
        let option = value.strip_prefix("--").unwrap_or_default();
        let sensitive = [
            "api-key",
            "apikey",
            "token",
            "secret",
            "password",
            "credential",
            "auth",
            "environment",
            "prompt",
        ];
        if option.is_empty()
            || option.len() > 63
            || !option.as_bytes()[0].is_ascii_lowercase()
            || !option
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            || sensitive.iter().any(|part| option.contains(part))
        {
            return Err(ConversationError::InvalidTaskIdentity);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexReasoningEffort {
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
}

impl CodexReasoningEffort {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "minimal" => Some(Self::Minimal),
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            "xhigh" => Some(Self::Xhigh),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexSandboxMode {
    ReadOnly,
    WorkspaceWrite,
}

impl CodexSandboxMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::WorkspaceWrite => "workspace_write",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "read_only" => Some(Self::ReadOnly),
            "workspace_write" => Some(Self::WorkspaceWrite),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexApprovalPolicy {
    OnRequest,
    Never,
}

impl CodexApprovalPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OnRequest => "on_request",
            Self::Never => "never",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "on_request" => Some(Self::OnRequest),
            "never" => Some(Self::Never),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexRunSettings {
    pub model: Option<String>,
    pub model_provider: Option<String>,
    pub reasoning_effort: Option<CodexReasoningEffort>,
    pub sandbox_mode: CodexSandboxMode,
    pub approval_policy: CodexApprovalPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexWorkspaceSnapshot {
    pub project_id: Option<String>,
    pub worktree_id: Option<String>,
    pub canonical_working_directory: String,
    pub additional_directories: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexExecutionSnapshot {
    pub profile: CodexProfileReference,
    pub adapter: CodexAdapterKind,
    pub executable: String,
    pub arguments: Vec<CodexAdapterArgument>,
    pub adapter_version: String,
    pub codex_version: String,
    pub settings: CodexRunSettings,
    pub workspace: CodexWorkspaceSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodexSessionFailureCode {
    ProcessStartFailed,
    AuthenticationRequired,
    AdapterRejected,
    InvalidConfiguration,
}

impl CodexSessionFailureCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ProcessStartFailed => "process_start_failed",
            Self::AuthenticationRequired => "authentication_required",
            Self::AdapterRejected => "adapter_rejected",
            Self::InvalidConfiguration => "invalid_configuration",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "process_start_failed" => Some(Self::ProcessStartFailed),
            "authentication_required" => Some(Self::AuthenticationRequired),
            "adapter_rejected" => Some(Self::AdapterRejected),
            "invalid_configuration" => Some(Self::InvalidConfiguration),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodexSessionUncertaintyCode {
    TransportClosed,
    ProtocolFailure,
    OutcomeUnknown,
}

impl CodexSessionUncertaintyCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::TransportClosed => "transport_closed",
            Self::ProtocolFailure => "protocol_failure",
            Self::OutcomeUnknown => "outcome_unknown",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "transport_closed" => Some(Self::TransportClosed),
            "protocol_failure" => Some(Self::ProtocolFailure),
            "outcome_unknown" => Some(Self::OutcomeUnknown),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexSessionIntentId(String);

impl CodexSessionIntentId {
    pub(crate) fn generate() -> Self {
        Self(ulid::Ulid::generate().to_string())
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        if value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        {
            return None;
        }
        Some(Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodexSessionCreationState {
    Absent,
    Intent {
        intent_id: CodexSessionIntentId,
    },
    Confirmed {
        intent_id: CodexSessionIntentId,
        session_id: String,
    },
    Uncertain {
        intent_id: CodexSessionIntentId,
        code: CodexSessionUncertaintyCode,
    },
    DefinitivelyFailed {
        intent_id: CodexSessionIntentId,
        code: CodexSessionFailureCode,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexTaskIdentity {
    pub thread_id: String,
    pub backend: TaskBackend,
    pub snapshot: CodexExecutionSnapshot,
    pub session_creation: CodexSessionCreationState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexPromptBinding {
    pub thread_id: String,
    pub session_id: String,
    pub snapshot: CodexExecutionSnapshot,
}
