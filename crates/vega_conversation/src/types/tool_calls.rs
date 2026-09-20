use super::*;
use sha2::{Digest, Sha256};

/// Complete tool proposal emitted to UI/store consumers.
#[derive(Clone, PartialEq, Eq)]
pub struct ToolCall {
    /// Provider call id.
    pub id: CallId,
    /// Tool name.
    pub tool: String,
    /// Safe complete JSON input. Write/edit bodies are replaced by strict
    /// content-free audit projections before this boundary.
    pub input_json: String,
}

impl std::fmt::Debug for ToolCall {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ToolCall")
            .field("id_bytes", &self.id.len())
            .field("tool_bytes", &self.tool.len())
            .field("input_json_bytes", &self.input_json.len())
            .finish()
    }
}

/// Permission decision recorded for a tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Approval {
    /// Approved for this call only.
    Once,
    /// Persisted project-level rule (S5).
    Always,
    /// Denied.
    Deny,
}

impl Approval {
    /// Exact approval vocabulary used by the strict codec.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Once => "once",
            Self::Always => "always",
            Self::Deny => "deny",
        }
    }

    /// Parses only exact bare approval values.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "once" => Some(Self::Once),
            "always" => Some(Self::Always),
            "deny" => Some(Self::Deny),
            _ => None,
        }
    }
}

/// Persisted tool-call lifecycle (tech-spec §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCallStatus {
    /// Waiting for a permission decision.
    PendingApproval,
    /// Approved.
    Approved,
    /// Rejected.
    Rejected,
    /// Running.
    Running,
    /// Completed successfully.
    Success,
    /// Completed with a tool error.
    Failed,
    /// Cancelled while running.
    Cancelled,
}

impl ToolCallStatus {
    /// Parses the exact persisted `tool_calls.status` DDL vocabulary
    /// (S8-T45 hydration reads durable rows; unknown values fail closed).
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pending_approval" => Some(Self::PendingApproval),
            "approved" => Some(Self::Approved),
            "rejected" => Some(Self::Rejected),
            "running" => Some(Self::Running),
            "success" => Some(Self::Success),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

/// A display chunk from a tool.
#[derive(Clone, PartialEq, Eq)]
pub struct ToolOutputChunk(pub String);

impl std::fmt::Debug for ToolOutputChunk {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("ToolOutputChunk")
            .field(&format_args!("{} bytes", self.0.len()))
            .finish()
    }
}

/// Mutating tool identity for an invalid-input terminal projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidToolKind {
    /// Invalid `bash` provider input.
    Bash,
    /// Invalid `write` provider input.
    Write,
    /// Invalid `edit` provider input.
    Edit,
}

impl InvalidToolKind {
    /// Stable tool name safe for UI display.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::Write => "write",
            Self::Edit => "edit",
        }
    }
}

/// Closed validation-code vocabulary safe for an invalid tool card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidToolCode {
    InvalidInput,
    MalformedJson,
    InputNotObject,
    UnexpectedField,
    MissingPath,
    WrongPathType,
    MissingContent,
    WrongContentType,
    MissingOldString,
    WrongOldStringType,
    MissingNewString,
    WrongNewStringType,
    PathAbsolute,
    PathParent,
    PathRoot,
    PathSymlink,
    PathHardlink,
    PathGit,
    PathNotFile,
    ParentNotFound,
    TargetNotFound,
    CheckpointIdInvalid,
    CheckpointUnavailable,
    CheckpointSymlink,
    EditEmptyOldString,
    FilesystemError,
}

impl InvalidToolCode {
    /// Stable content-free wire label.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidInput => "invalid_input",
            Self::MalformedJson => "malformed_json",
            Self::InputNotObject => "input_not_object",
            Self::UnexpectedField => "unexpected_field",
            Self::MissingPath => "missing_path",
            Self::WrongPathType => "wrong_path_type",
            Self::MissingContent => "missing_content",
            Self::WrongContentType => "wrong_content_type",
            Self::MissingOldString => "missing_old_string",
            Self::WrongOldStringType => "wrong_old_string_type",
            Self::MissingNewString => "missing_new_string",
            Self::WrongNewStringType => "wrong_new_string_type",
            Self::PathAbsolute => "path_absolute",
            Self::PathParent => "path_parent",
            Self::PathRoot => "path_root",
            Self::PathSymlink => "path_symlink",
            Self::PathHardlink => "path_hardlink",
            Self::PathGit => "path_git",
            Self::PathNotFile => "path_not_file",
            Self::ParentNotFound => "parent_not_found",
            Self::TargetNotFound => "target_not_found",
            Self::CheckpointIdInvalid => "checkpoint_id_invalid",
            Self::CheckpointUnavailable => "checkpoint_unavailable",
            Self::CheckpointSymlink => "checkpoint_symlink",
            Self::EditEmptyOldString => "edit_empty_old_string",
            Self::FilesystemError => "filesystem_error",
        }
    }
}

/// Content-free identity attached only to atomic invalid write/edit terminals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidToolProjection {
    pub(crate) tool: InvalidToolKind,
    pub(crate) code: InvalidToolCode,
}

impl InvalidToolProjection {
    /// Constructs a projection from closed, content-free values only.
    pub const fn new(tool: InvalidToolKind, code: InvalidToolCode) -> Self {
        Self { tool, code }
    }

    /// Safe mutating tool identity.
    pub const fn tool(&self) -> InvalidToolKind {
        self.tool
    }

    /// Safe closed validation code.
    pub const fn code(&self) -> InvalidToolCode {
        self.code
    }
}

/// Terminal tool result delivered to the conversation.
#[derive(Clone, PartialEq, Eq)]
pub struct ToolResult {
    /// Terminal lifecycle status.
    pub status: ToolCallStatus,
    /// Truncated display output.
    pub output: String,
    /// True when a persisted result was reused by call id.
    pub reused: bool,
    /// Exact bash exit code when available.
    pub exit_code: Option<i32>,
    /// Exact bash duration when available.
    pub duration_ms: Option<u64>,
    /// Exact live truncation fact; absent on persisted recovery.
    pub truncated: Option<bool>,
    /// Typed content-free projection for an atomic invalid tool path.
    /// All ordinary terminal paths keep this absent.
    pub invalid: Option<InvalidToolProjection>,
}

impl std::fmt::Debug for ToolResult {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ToolResult")
            .field("status", &self.status)
            .field("output_bytes", &self.output.len())
            .field("reused", &self.reused)
            .field("exit_code", &self.exit_code)
            .field("duration_ms", &self.duration_ms)
            .field("truncated", &self.truncated)
            .field("invalid", &self.invalid)
            .finish()
    }
}

/// Strict, content-free tool input prepared for UI cards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadOnlyToolKind {
    Read,
    Glob,
    Grep,
}

/// Vega-owned, read-only Skill tools. This is display metadata only and does
/// not grant the tool or a Skill any permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillToolKind {
    Load,
    ReadResource,
}

impl SkillToolKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Load => "load_skill",
            Self::ReadResource => "read_skill_resource",
        }
    }
}

/// Content-free terminal detail for a Skill tool. Resource body/path never
/// cross into UI-owned card state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillCardOutcome {
    Loaded,
    AlreadyLoaded,
    ResourceRead { text_bytes: usize, sha256: String },
    Failed { code: String },
    Rejected,
    Cancelled,
}

impl ReadOnlyToolKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Glob => "glob",
            Self::Grep => "grep",
        }
    }
}

/// Strict, content-free tool input prepared for UI cards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolCardInputProjection {
    /// A read-only tool. Its raw JSON is intentionally not retained by UI.
    ReadOnly { tool: ReadOnlyToolKind },
    /// Full bash command, already strictly decoded by the tools boundary.
    Bash { command: String },
    /// Safe write audit summary; the body and fingerprint are discarded.
    Write { path: String, content_bytes: u64 },
    /// Safe edit audit summary; both strings and fingerprint are discarded.
    Edit {
        path: String,
        old_string_bytes: u64,
        new_string_bytes: u64,
    },
    /// Strictly parsed value-free identity for an external MCP call.
    Mcp {
        alias: String,
        identity: McpCallIdentity,
        permission_target: String,
    },
    /// Only the validated Skill name and a reference path's length/hash.
    /// `name=None` represents the exact `{}` invalid-input audit placeholder.
    Skill {
        kind: SkillToolKind,
        name: Option<String>,
        path_bytes: Option<u64>,
        path_sha256: Option<String>,
    },
    /// Fixed fail-closed projection for an invalid/unknown input shape.
    Corrupt,
}

impl ToolCardInputProjection {
    /// Stable known tool name, absent for corrupt input.
    pub fn tool(&self) -> Option<&str> {
        match self {
            Self::ReadOnly { tool } => Some(tool.as_str()),
            Self::Bash { .. } => Some("bash"),
            Self::Write { .. } => Some("write"),
            Self::Edit { .. } => Some("edit"),
            Self::Mcp { alias, .. } => Some(alias),
            Self::Skill { kind, .. } => Some(kind.as_str()),
            Self::Corrupt => None,
        }
    }

    /// Exact permission target for mutating cards.
    pub fn permission_target(&self) -> Option<&str> {
        match self {
            Self::Bash { command } => Some(command),
            Self::Write { path, .. } | Self::Edit { path, .. } => Some(path),
            Self::Mcp {
                permission_target, ..
            } => Some(permission_target),
            Self::ReadOnly { .. } | Self::Skill { .. } | Self::Corrupt => None,
        }
    }
}

/// Strict terminal projection safe for a tool card to retain and render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolCardResultProjection {
    /// Bounded bash output plus structured execution metadata.
    Bash {
        status: ToolCallStatus,
        output: String,
        exit_code: Option<i32>,
        duration_ms: Option<u64>,
        truncated: Option<bool>,
        reused: bool,
    },
    /// Bounded generic read/glob/grep output.
    ReadOnly {
        status: ToolCallStatus,
        output: String,
        reused: bool,
    },
    /// Bounded lower-trust result from a third-party MCP server.
    Mcp {
        status: ToolCallStatus,
        output: String,
        reused: bool,
    },
    /// Strictly decoded content-free result from Vega's Skill tool pair.
    Skill {
        status: ToolCallStatus,
        outcome: SkillCardOutcome,
        reused: bool,
    },
    /// Strict write success with the opaque checkpoint reference discarded.
    WriteSuccess {
        path: String,
        bytes_written: u64,
        reused: bool,
    },
    /// Strict edit success with the opaque checkpoint reference discarded.
    EditSuccess {
        path: String,
        bytes_written: u64,
        replacements: u64,
        reused: bool,
    },
    /// Fixed content-free terminal state for a valid write/edit failure.
    MutationTerminal {
        tool: InvalidToolKind,
        status: ToolCallStatus,
        reused: bool,
    },
    /// Atomic invalid-input rejection with no proposal and no raw identity.
    InvalidRejected {
        tool: InvalidToolKind,
        code: InvalidToolCode,
        reused: bool,
    },
    /// Stable fail-closed card; no untrusted fields are retained.
    Corrupt,
}

/// Strictly reduces a shared safe proposal to the fields T27 may retain.
pub fn tool_card_input_projection(call: &ToolCall) -> ToolCardInputProjection {
    if matches!(call.tool.as_str(), "load_skill" | "read_skill_resource") {
        return skill_card_input_projection(&call.tool, &call.input_json);
    }
    if call.tool.starts_with("mcp_") {
        return McpCallIdentity::from_tool_call(call).map_or(
            ToolCardInputProjection::Corrupt,
            |identity| ToolCardInputProjection::Mcp {
                alias: call.tool.clone(),
                permission_target: identity.permission_target(),
                identity,
            },
        );
    }
    match call.tool.as_str() {
        "read" | "glob" | "grep" => {
            if serde_json::from_str::<serde_json::Value>(&call.input_json)
                .ok()
                .and_then(|value| value.as_object().map(|_| ()))
                .is_some()
            {
                let tool = match call.tool.as_str() {
                    "read" => ReadOnlyToolKind::Read,
                    "glob" => ReadOnlyToolKind::Glob,
                    "grep" => ReadOnlyToolKind::Grep,
                    _ => return ToolCardInputProjection::Corrupt,
                };
                ToolCardInputProjection::ReadOnly { tool }
            } else {
                ToolCardInputProjection::Corrupt
            }
        }
        "bash" => vega_tools::bash_permission_signature(&call.input_json)
            .map_or(ToolCardInputProjection::Corrupt, |command| {
                ToolCardInputProjection::Bash { command }
            }),
        "write" | "edit" => match vega_tools::WriteEditAudit::from_json(&call.input_json) {
            Ok(vega_tools::WriteEditAudit::Write {
                path,
                content_bytes,
                ..
            }) if call.tool == "write" => ToolCardInputProjection::Write {
                path,
                content_bytes,
            },
            Ok(vega_tools::WriteEditAudit::Edit {
                path,
                old_string_bytes,
                new_string_bytes,
                ..
            }) if call.tool == "edit" => ToolCardInputProjection::Edit {
                path,
                old_string_bytes,
                new_string_bytes,
            },
            _ => ToolCardInputProjection::Corrupt,
        },
        _ => ToolCardInputProjection::Corrupt,
    }
}

fn skill_card_input_projection(tool: &str, input_json: &str) -> ToolCardInputProjection {
    let kind = match tool {
        "load_skill" => SkillToolKind::Load,
        "read_skill_resource" => SkillToolKind::ReadResource,
        _ => return ToolCardInputProjection::Corrupt,
    };
    if input_json == "{}" {
        return ToolCardInputProjection::Skill {
            kind,
            name: None,
            path_bytes: None,
            path_sha256: None,
        };
    }
    let Some(object) = serde_json::from_str::<serde_json::Value>(input_json)
        .ok()
        .and_then(|value| value.as_object().cloned())
    else {
        return ToolCardInputProjection::Corrupt;
    };
    let Some(name) = object.get("name").and_then(serde_json::Value::as_str) else {
        return ToolCardInputProjection::Corrupt;
    };
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return ToolCardInputProjection::Corrupt;
    }
    let (path_bytes, path_sha256) = match kind {
        SkillToolKind::Load if object.len() == 1 => (None, None),
        SkillToolKind::ReadResource if object.len() == 3 => {
            let Some(path_bytes) = object
                .get("path_bytes")
                .and_then(serde_json::Value::as_u64)
                .filter(|bytes| (1..=1024).contains(bytes))
            else {
                return ToolCardInputProjection::Corrupt;
            };
            let Some(hash) = object
                .get("path_sha256")
                .and_then(serde_json::Value::as_str)
                .filter(|hash| valid_skill_sha256(hash))
            else {
                return ToolCardInputProjection::Corrupt;
            };
            (Some(path_bytes), Some(hash.to_string()))
        }
        _ => return ToolCardInputProjection::Corrupt,
    };
    ToolCardInputProjection::Skill {
        kind,
        name: Some(name.to_string()),
        path_bytes,
        path_sha256,
    }
}

fn valid_skill_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Strictly reduces a terminal result. `input=None` is legal only for the
/// atomic invalid write/edit/bash terminal projection.
pub fn tool_card_result_projection(
    input: Option<&ToolCardInputProjection>,
    result: &ToolResult,
) -> ToolCardResultProjection {
    if let Some(invalid) = result.invalid {
        let expected = match (invalid.tool(), invalid.code()) {
            (InvalidToolKind::Bash, InvalidToolCode::InvalidInput) => {
                vega_runtime::BASH_INVALID_INPUT_OUTPUT.to_string()
            }
            (InvalidToolKind::Bash, _) | (_, InvalidToolCode::InvalidInput) => {
                return ToolCardResultProjection::Corrupt;
            }
            (tool, code) => format!(
                "Tool error: invalid {} input ({})",
                tool.as_str(),
                code.as_str()
            ),
        };
        if input.is_none()
            && result.status == ToolCallStatus::Rejected
            && result.exit_code.is_none()
            && result.duration_ms.is_none()
            && result.truncated.is_none()
            && result.output == expected
        {
            return ToolCardResultProjection::InvalidRejected {
                tool: invalid.tool(),
                code: invalid.code(),
                reused: result.reused,
            };
        }
        return ToolCardResultProjection::Corrupt;
    }

    let Some(input) = input else {
        return ToolCardResultProjection::Corrupt;
    };
    match input {
        ToolCardInputProjection::Bash { .. } => {
            let metadata_valid = match result.status {
                ToolCallStatus::Success => {
                    result.exit_code.is_some()
                        && result.duration_ms.is_some()
                        && success_truncation_valid(result)
                }
                ToolCallStatus::Failed | ToolCallStatus::Rejected | ToolCallStatus::Cancelled => {
                    result.exit_code.is_none()
                        && result.duration_ms.is_none()
                        && result.truncated.is_none()
                }
                ToolCallStatus::PendingApproval
                | ToolCallStatus::Approved
                | ToolCallStatus::Running => false,
            };
            if !metadata_valid {
                return ToolCardResultProjection::Corrupt;
            }
            ToolCardResultProjection::Bash {
                status: result.status,
                output: result.output.clone(),
                exit_code: result.exit_code,
                duration_ms: result.duration_ms,
                truncated: result.truncated,
                reused: result.reused,
            }
        }
        ToolCardInputProjection::ReadOnly { .. } => {
            if !is_terminal(result.status)
                || result.exit_code.is_some()
                || result.duration_ms.is_some()
                || match result.status {
                    ToolCallStatus::Success => !success_truncation_valid(result),
                    _ => result.truncated.is_some(),
                }
            {
                return ToolCardResultProjection::Corrupt;
            }
            ToolCardResultProjection::ReadOnly {
                status: result.status,
                output: result.output.clone(),
                reused: result.reused,
            }
        }
        ToolCardInputProjection::Mcp { .. } => {
            if !is_terminal(result.status)
                || result.exit_code.is_some()
                || result.duration_ms.is_some()
                || result.output.len() > 256 * 1024 + 64
                || match result.status {
                    ToolCallStatus::Success => !success_truncation_valid(result),
                    _ => result.truncated.is_some(),
                }
            {
                return ToolCardResultProjection::Corrupt;
            }
            ToolCardResultProjection::Mcp {
                status: result.status,
                output: result.output.clone(),
                reused: result.reused,
            }
        }
        ToolCardInputProjection::Skill {
            kind,
            name,
            path_bytes,
            path_sha256,
        } => skill_card_result_projection(
            *kind,
            name.as_deref(),
            *path_bytes,
            path_sha256.as_deref(),
            result,
        ),
        ToolCardInputProjection::Write {
            path,
            content_bytes,
        } => {
            if result.status == ToolCallStatus::Success {
                if result.exit_code.is_some()
                    || result.duration_ms.is_some()
                    || !mutation_success_truncation_valid(result)
                {
                    return ToolCardResultProjection::Corrupt;
                }
                let Ok(success) = vega_tools::WriteSuccessOutput::from_json(&result.output) else {
                    return ToolCardResultProjection::Corrupt;
                };
                if success.path != *path || success.bytes_written != *content_bytes {
                    return ToolCardResultProjection::Corrupt;
                }
                ToolCardResultProjection::WriteSuccess {
                    path: success.path,
                    bytes_written: success.bytes_written,
                    reused: result.reused,
                }
            } else {
                mutation_terminal(InvalidToolKind::Write, path, Some(*content_bytes), result)
            }
        }
        ToolCardInputProjection::Edit { path, .. } => {
            if result.status == ToolCallStatus::Success {
                if result.exit_code.is_some()
                    || result.duration_ms.is_some()
                    || !mutation_success_truncation_valid(result)
                {
                    return ToolCardResultProjection::Corrupt;
                }
                let Ok(success) = vega_tools::EditSuccessOutput::from_json(&result.output) else {
                    return ToolCardResultProjection::Corrupt;
                };
                if success.path != *path || success.replacements != 1 {
                    return ToolCardResultProjection::Corrupt;
                }
                ToolCardResultProjection::EditSuccess {
                    path: success.path,
                    bytes_written: success.bytes_written,
                    replacements: success.replacements,
                    reused: result.reused,
                }
            } else {
                mutation_terminal(InvalidToolKind::Edit, path, None, result)
            }
        }
        ToolCardInputProjection::Corrupt => ToolCardResultProjection::Corrupt,
    }
}

fn skill_card_result_projection(
    kind: SkillToolKind,
    name: Option<&str>,
    path_bytes: Option<u64>,
    path_sha256: Option<&str>,
    result: &ToolResult,
) -> ToolCardResultProjection {
    if !is_terminal(result.status)
        || result.exit_code.is_some()
        || result.duration_ms.is_some()
        || result.output.len() > 256 * 1024 + 64
        || match result.status {
            ToolCallStatus::Success => !success_truncation_valid(result),
            _ => result.truncated.is_some(),
        }
    {
        return ToolCardResultProjection::Corrupt;
    }
    let outcome = match result.status {
        ToolCallStatus::Rejected => {
            let allowed = match result.output.as_str() {
                "Tool error: invalid Skill input" => name.is_none(),
                "Tool error: mixed Skill load batch rejected other tools"
                | "Tool error: permission denied" => true,
                other => other == vega_store::recovery::RECOVERY_REJECTED_OUTPUT,
            };
            if !allowed {
                return ToolCardResultProjection::Corrupt;
            }
            SkillCardOutcome::Rejected
        }
        ToolCallStatus::Cancelled => {
            if !matches!(
                result.output.as_str(),
                vega_runtime::CANCELLED_BEFORE_EXECUTION_OUTPUT
                    | vega_store::recovery::RECOVERY_CANCELLED_OUTPUT
            ) {
                return ToolCardResultProjection::Corrupt;
            }
            SkillCardOutcome::Cancelled
        }
        ToolCallStatus::Success if kind == SkillToolKind::ReadResource => {
            let Some(outcome) =
                skill_reference_outcome(name, path_bytes, path_sha256, &result.output)
            else {
                return ToolCardResultProjection::Corrupt;
            };
            outcome
        }
        ToolCallStatus::Success | ToolCallStatus::Failed => {
            let Some(outcome) = skill_receipt_outcome(name, &result.output, result.status) else {
                return ToolCardResultProjection::Corrupt;
            };
            outcome
        }
        _ => return ToolCardResultProjection::Corrupt,
    };
    ToolCardResultProjection::Skill {
        status: result.status,
        outcome,
        reused: result.reused,
    }
}

fn skill_receipt_outcome(
    name: Option<&str>,
    output: &str,
    status: ToolCallStatus,
) -> Option<SkillCardOutcome> {
    let name = name?;
    if output.len() > 512 {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(output).ok()?;
    let object = value.as_object()?;
    let code = object.get("status")?.as_str()?;
    if object.len() != 2
        || object.get("name")?.as_str()? != name
        || code.is_empty()
        || code.len() > 64
        || !code
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
    {
        return None;
    }
    match (status, code) {
        (ToolCallStatus::Success, "loaded") => Some(SkillCardOutcome::Loaded),
        (ToolCallStatus::Success, "already_loaded") => Some(SkillCardOutcome::AlreadyLoaded),
        (ToolCallStatus::Failed, "loaded" | "already_loaded") => None,
        (ToolCallStatus::Failed, other) => Some(SkillCardOutcome::Failed {
            code: other.to_string(),
        }),
        _ => None,
    }
}

fn skill_reference_outcome(
    name: Option<&str>,
    path_bytes: Option<u64>,
    path_sha256: Option<&str>,
    output: &str,
) -> Option<SkillCardOutcome> {
    let json = output.strip_prefix("[Lower-trust Skill reference]\n")?;
    if json.len() > 256 * 1024 {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let reference = value.as_object()?;
    let path = reference.get("path")?.as_str()?;
    let text = reference.get("text")?.as_str()?;
    let text_sha = format!("{:x}", Sha256::digest(text.as_bytes()));
    let actual_path_sha = format!("{:x}", Sha256::digest(path.as_bytes()));
    if reference.len() != 5
        || reference.get("name")?.as_str()? != name?
        || reference.get("lower_trust")?.as_bool() != Some(true)
        || reference.get("content_sha256")?.as_str()? != text_sha
        || path_bytes != Some(path.len() as u64)
        || path_sha256 != Some(actual_path_sha.as_str())
        || !path.starts_with("references/")
        || path.split('/').any(|part| matches!(part, "" | "." | ".."))
        || text.len() > 32 * 1024
    {
        return None;
    }
    Some(SkillCardOutcome::ResourceRead {
        text_bytes: text.len(),
        sha256: text_sha,
    })
}

pub(crate) fn mutation_terminal(
    tool: InvalidToolKind,
    path: &str,
    expected_write_bytes: Option<u64>,
    result: &ToolResult,
) -> ToolCardResultProjection {
    let metadata_valid = result.exit_code.is_none()
        && result.duration_ms.is_none()
        && match result.status {
            ToolCallStatus::Cancelled
                if mutation_cancelled_success_matches(tool, path, expected_write_bytes, result) =>
            {
                mutation_success_truncation_valid(result)
            }
            _ => result.truncated.is_none(),
        };
    if metadata_valid
        && matches!(
            result.status,
            ToolCallStatus::Failed | ToolCallStatus::Rejected | ToolCallStatus::Cancelled
        )
        && mutation_failure_output_allowed(tool, path, expected_write_bytes, result)
    {
        ToolCardResultProjection::MutationTerminal {
            tool,
            status: result.status,
            reused: result.reused,
        }
    } else {
        ToolCardResultProjection::Corrupt
    }
}

pub(crate) fn success_truncation_valid(result: &ToolResult) -> bool {
    if result.reused {
        result.truncated.is_none()
    } else {
        result.truncated.is_some()
    }
}

pub(crate) fn mutation_success_truncation_valid(result: &ToolResult) -> bool {
    if result.reused {
        result.truncated.is_none()
    } else {
        result.truncated == Some(false)
    }
}

pub(crate) fn mutation_cancelled_success_matches(
    tool: InvalidToolKind,
    path: &str,
    expected_write_bytes: Option<u64>,
    result: &ToolResult,
) -> bool {
    match tool {
        InvalidToolKind::Bash => false,
        InvalidToolKind::Write => vega_tools::WriteSuccessOutput::from_json(&result.output)
            .is_ok_and(|success| {
                success.path == path && Some(success.bytes_written) == expected_write_bytes
            }),
        InvalidToolKind::Edit => vega_tools::EditSuccessOutput::from_json(&result.output)
            .is_ok_and(|success| success.path == path && success.replacements == 1),
    }
}

pub(crate) fn mutation_failure_output_allowed(
    tool: InvalidToolKind,
    path: &str,
    expected_write_bytes: Option<u64>,
    result: &ToolResult,
) -> bool {
    let tool_name = tool.as_str();
    match result.status {
        ToolCallStatus::Rejected => {
            matches!(
                result.output.as_str(),
                "Tool error: permission denied" | "Tool error: denied by run mode"
            ) || result.output == vega_store::recovery::RECOVERY_REJECTED_OUTPUT
                || result.output
                    == format!(
                        "Tool error: denied: tool '{tool_name}' is unavailable until the S5 permission gate"
                    )
        }
        ToolCallStatus::Failed => {
            result.output == format!("Tool error: {tool_name} failed")
                || result.output == "Tool error: tool worker failed"
                || result.output == "Tool error: invalid mutation result"
        }
        ToolCallStatus::Cancelled => {
            mutation_cancelled_success_matches(tool, path, expected_write_bytes, result)
                || result.output == format!("Tool error: {tool_name} failed")
                || result.output == "Tool error: tool worker failed"
                || result.output == vega_runtime::CANCELLED_BEFORE_EXECUTION_OUTPUT
                || result.output == vega_store::recovery::RECOVERY_CANCELLED_OUTPUT
        }
        ToolCallStatus::PendingApproval
        | ToolCallStatus::Approved
        | ToolCallStatus::Running
        | ToolCallStatus::Success => false,
    }
}

pub(crate) fn is_terminal(status: ToolCallStatus) -> bool {
    matches!(
        status,
        ToolCallStatus::Success
            | ToolCallStatus::Failed
            | ToolCallStatus::Rejected
            | ToolCallStatus::Cancelled
    )
}
