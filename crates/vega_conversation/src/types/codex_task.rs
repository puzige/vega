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
pub struct CodexAdapterArgument {
    kind: CodexAdapterArgumentKind,
    tokens: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodexAdapterArgumentKind {
    Flag,
    Option,
    Positional,
    PathOption,
}

impl CodexAdapterArgument {
    pub fn new(value: impl Into<String>) -> Result<Self, ConversationError> {
        let value = value.into();
        if value.starts_with('-') {
            Self::flag(value)
        } else {
            Self::positional(value)
        }
    }

    pub fn flag(value: impl Into<String>) -> Result<Self, ConversationError> {
        let value = value.into();
        let short_flag = value.starts_with('-') && !value.starts_with("--");
        let name = value
            .strip_prefix("--")
            .or_else(|| value.strip_prefix('-'))
            .unwrap_or_default();
        if name.is_empty()
            || name.len() > 63
            || !name.as_bytes()[0].is_ascii_alphanumeric()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            || sensitive_flag_name(&name.to_ascii_lowercase(), short_flag)
        {
            return Err(ConversationError::InvalidTaskIdentity);
        }
        Ok(Self {
            kind: CodexAdapterArgumentKind::Flag,
            tokens: vec![value],
        })
    }

    pub fn option(
        flag: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Self, ConversationError> {
        let flag = Self::flag(flag)?;
        let value = value.into();
        if value.starts_with('-') || !safe_argument_value(&value) {
            return Err(ConversationError::InvalidTaskIdentity);
        }
        Ok(Self {
            kind: CodexAdapterArgumentKind::Option,
            tokens: vec![flag.tokens[0].clone(), value],
        })
    }

    pub fn path_option(
        flag: impl Into<String>,
        path: impl Into<String>,
    ) -> Result<Self, ConversationError> {
        let flag = Self::flag(flag)?;
        let path = path.into();
        let absolute_path = std::path::Path::new(&path).is_absolute();
        if !absolute_path
            || path.len() > 4096
            || path.chars().any(char::is_control)
            || path_contains_credential_material(&path)
            || has_credential_url_userinfo(&path)
        {
            return Err(ConversationError::InvalidTaskIdentity);
        }
        Ok(Self {
            kind: CodexAdapterArgumentKind::PathOption,
            tokens: vec![flag.tokens[0].clone(), path],
        })
    }

    pub fn positional(value: impl Into<String>) -> Result<Self, ConversationError> {
        let value = value.into();
        if !safe_argument_value(&value)
            || value.starts_with('-')
            || (!value.contains('/') && !value.contains('@'))
        {
            return Err(ConversationError::InvalidTaskIdentity);
        }
        Ok(Self {
            kind: CodexAdapterArgumentKind::Positional,
            tokens: vec![value],
        })
    }

    pub fn as_args(&self) -> &[String] {
        &self.tokens
    }

    pub(crate) fn from_persisted(tokens: Vec<String>) -> Result<Self, ConversationError> {
        match tokens.as_slice() {
            [kind, value] if kind == "flag" => Self::flag(value.clone()),
            [kind, value] if kind == "positional" => Self::positional(value.clone()),
            [kind, flag, value] if kind == "option" => Self::option(flag.clone(), value.clone()),
            [kind, flag, value] if kind == "path_option" => {
                Self::path_option(flag.clone(), value.clone())
            }
            _ => Err(ConversationError::InvalidTaskIdentity),
        }
    }

    pub(crate) fn persisted_tokens(&self) -> Vec<String> {
        let kind = match self.kind {
            CodexAdapterArgumentKind::Flag => "flag",
            CodexAdapterArgumentKind::Option => "option",
            CodexAdapterArgumentKind::Positional => "positional",
            CodexAdapterArgumentKind::PathOption => "path_option",
        };
        std::iter::once(kind.to_string())
            .chain(self.tokens.iter().cloned())
            .collect()
    }
}

fn safe_argument_value(value: &str) -> bool {
    let lowered = value.to_ascii_lowercase();
    let suspicious_prefixes = [
        "sk-",
        "sk_",
        "ghp_",
        "gho_",
        "ghu_",
        "ghs_",
        "github_pat_",
        "xoxb-",
        "xoxp-",
        "glpat-",
        "akia",
        "asia",
        "aiza",
        "eyj",
    ];
    !value.is_empty()
        && value.len() <= 512
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'_' | b'-' | b'.' | b'@' | b'/' | b'\\' | b':')
        })
        && !has_credential_url_userinfo(value)
        && !suspicious_prefixes
            .iter()
            .any(|prefix| lowered.starts_with(prefix))
        && !sensitive_argument_value(&lowered)
        && !(value.len() >= 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn has_credential_url_userinfo(value: &str) -> bool {
    value
        .split_once("://")
        .map(|(_, rest)| rest.split('/').next().unwrap_or_default())
        .is_some_and(|authority| authority.contains('@') && authority.contains(':'))
}

fn path_contains_credential_material(value: &str) -> bool {
    let lowered = value.to_ascii_lowercase();
    let suspicious_prefixes = [
        "sk-",
        "sk_",
        "ghp_",
        "gho_",
        "ghu_",
        "ghs_",
        "github_pat_",
        "xoxb-",
        "xoxp-",
        "glpat-",
        "akia",
        "asia",
        "aiza",
        "eyj",
    ];
    lowered
        .split(|character: char| !character.is_ascii_alphanumeric())
        .any(sensitive_argument_value)
        || suspicious_prefixes
            .iter()
            .any(|prefix| lowered.contains(prefix))
        || lowered
            .split(|character: char| !character.is_ascii_hexdigit())
            .any(|component| component.len() >= 32)
}

fn sensitive_flag_name(value: &str, short_flag: bool) -> bool {
    sensitive_argument_value(value)
        || value.contains("env")
        || (short_flag && matches!(value, "e" | "i" | "k" | "m" | "p" | "q" | "t"))
}

fn sensitive_argument_value(value: &str) -> bool {
    [
        "api-key",
        "apikey",
        "api_key",
        "key",
        "token",
        "secret",
        "password",
        "passwd",
        "passphrase",
        "credential",
        "auth",
        "environment",
        "env-file",
        "prompt",
        "instruction",
        "message",
        "input",
        "query",
        "text",
        "system",
        "bearer",
        "oauth",
        "private-key",
        "private_key",
        "ssh-key",
    ]
    .iter()
    .any(|part| value.contains(part))
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
