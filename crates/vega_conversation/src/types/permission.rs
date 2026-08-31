use super::*;

/// Error surfaced by the vega_conversation orchestration layer.
///
/// Thread-management storage failures remain display strings, while the live
/// agent pipeline preserves the shared [`vega_runtime::VegaError`] kind and
/// fields for UI decisions. Send + Sync by construction (owned data only).
/// Error surfaced by the vega_conversation orchestration layer.
///
/// Thread-management storage failures remain display strings, while the live
/// agent pipeline preserves the shared [`vega_runtime::VegaError`] kind and
/// fields for UI decisions. Send + Sync by construction (owned data only).
#[derive(Debug, thiserror::Error)]
pub enum ConversationError {
    /// A store/IO failure, reported with the underlying error message.
    #[error("store error: {0}")]
    Store(String),
    /// The referenced thread does not exist.
    #[error("thread not found: {0}")]
    NotFound(String),
    /// No project row exists yet, so a thread cannot be created.
    #[error("no project exists; register a project first")]
    NoProject,
    /// A row carries a value outside the DDL vocabulary (e.g. `mode`).
    #[error("corrupt thread row: {0}")]
    CorruptRow(String),
    /// Execute selection was blocked until the current Plan is reviewed.
    #[error("pending plan must be reviewed before execute mode")]
    PendingPlan,
    /// Headless runtime/provider/persistence failure with its structured kind
    /// and fields preserved for callers.
    #[error("runtime error: {0}")]
    Runtime(Arc<vega_runtime::VegaError>),
}

/// Message identifier used by conversation events.
pub type MessageId = String;

/// Provider tool-call identifier used by conversation events and storage.
pub type CallId = String;

/// Permission mode selected by a thread (tech-spec §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionMode {
    /// Reject write-class tools.
    ReadOnly,
    /// Ask before mutations.
    Confirm,
    /// Auto-approve except hard-blocked dangerous commands.
    Auto,
}

impl PermissionMode {
    /// Exact DDL/config value for this mode.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "readonly",
            Self::Confirm => "confirm",
            Self::Auto => "auto",
        }
    }

    /// Parses the exact `readonly|confirm|auto` vocabulary.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "readonly" => Some(Self::ReadOnly),
            "confirm" => Some(Self::Confirm),
            "auto" => Some(Self::Auto),
            _ => None,
        }
    }
}

/// Content-free permission prompt shared with UI/store consumers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRequest {
    /// Provider call id.
    pub call_id: CallId,
    /// Exact mutating tool name.
    pub tool: String,
    /// Full bash command or normalized project-relative path.
    pub display_target: String,
    /// Stable danger rule id for a danger prompt.
    pub danger_rule_id: Option<String>,
    /// Stable danger reason for a danger prompt.
    pub danger_reason: Option<String>,
}

/// UI decision returned to the runtime permission hook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionDecision {
    /// Allow this call once.
    Once,
    /// Allow and remember the exact signature.
    Always,
    /// Reject, optionally with a note.
    Deny { note: Option<String> },
    /// Permission wait expired or disappeared.
    Timeout,
}

/// Source of a persisted approval audit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalSource {
    /// Explicit danger-card response.
    Danger,
    /// Read-only permission mode.
    ReadOnly,
    /// Ask/Plan capability gate.
    RunMode,
    /// Exact persisted rule.
    Rule,
    /// Auto permission mode.
    Auto,
    /// Ordinary permission-card response.
    User,
    /// Permission timeout.
    Timeout,
    /// Invalid write/edit input.
    Validation,
    /// Read-only built-in tool.
    ReadonlyTool,
    /// Startup recovery.
    Recovery,
    /// Exact S4 bare value read through the legacy branch.
    Legacy,
}

impl ApprovalSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Danger => "danger",
            Self::ReadOnly => "readonly",
            Self::RunMode => "run_mode",
            Self::Rule => "rule",
            Self::Auto => "auto",
            Self::User => "user",
            Self::Timeout => "timeout",
            Self::Validation => "validation",
            Self::ReadonlyTool => "readonly_tool",
            Self::Recovery => "recovery",
            Self::Legacy => "legacy",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "danger" => Some(Self::Danger),
            "readonly" => Some(Self::ReadOnly),
            "run_mode" => Some(Self::RunMode),
            "rule" => Some(Self::Rule),
            "auto" => Some(Self::Auto),
            "user" => Some(Self::User),
            "timeout" => Some(Self::Timeout),
            "validation" => Some(Self::Validation),
            "readonly_tool" => Some(Self::ReadonlyTool),
            "recovery" => Some(Self::Recovery),
            "legacy" => Some(Self::Legacy),
            _ => None,
        }
    }
}

/// Nested danger decision retained when later policy also rejects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DangerAudit {
    /// Stable danger rule id.
    pub rule_id: String,
    /// Danger-card decision.
    pub decision: Approval,
    /// Optional denial note.
    pub note: Option<String>,
}

/// Strict four-field approval audit persisted in `tool_calls.approval`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalAudit {
    /// Final decision.
    pub decision: Approval,
    /// Optional denial note.
    pub note: Option<String>,
    /// Decision source.
    pub source: ApprovalSource,
    /// Nested danger decision, if a danger card was shown.
    pub danger: Option<DangerAudit>,
}

/// Strict approval codec failure. Callers must fail closed.
#[derive(Debug, thiserror::Error)]
pub enum ApprovalCodecError {
    /// JSON shape or scalar type is invalid.
    #[error("invalid approval audit shape")]
    InvalidShape,
    /// Decision/source vocabulary is unknown.
    #[error("invalid approval audit vocabulary")]
    InvalidVocabulary,
    /// Fields form an impossible permission audit.
    #[error("invalid approval audit semantics")]
    InvalidSemantics,
    /// Legacy audits are read-only and cannot be emitted by S5.
    #[error("legacy approval audit cannot be encoded")]
    LegacyWrite,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ApprovalWireRead {
    decision: String,
    note: RequiredNullableString,
    source: String,
    danger: RequiredNullableDanger,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DangerWireRead {
    rule_id: String,
    decision: String,
    note: RequiredNullableString,
}

pub(crate) struct RequiredNullableString(Option<String>);

impl<'de> Deserialize<'de> for RequiredNullableString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Option::<String>::deserialize(deserializer).map(Self)
    }
}

pub(crate) struct RequiredNullableDanger(Option<DangerWireRead>);

impl<'de> Deserialize<'de> for RequiredNullableDanger {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Option::<DangerWireRead>::deserialize(deserializer).map(Self)
    }
}

#[derive(Serialize)]
pub(crate) struct ApprovalWireWrite<'a> {
    decision: &'a str,
    note: &'a Option<String>,
    source: &'a str,
    danger: Option<DangerWireWrite<'a>>,
}

#[derive(Serialize)]
pub(crate) struct DangerWireWrite<'a> {
    rule_id: &'a str,
    decision: &'a str,
    note: &'a Option<String>,
}

impl ApprovalAudit {
    /// Decodes exact S5 JSON or the exact S4 bare `once|always|deny` values.
    pub fn from_json(raw: &str) -> Result<Self, ApprovalCodecError> {
        if let Some(decision) = Approval::parse(raw) {
            return Ok(Self {
                decision,
                note: None,
                source: ApprovalSource::Legacy,
                danger: None,
            });
        }
        let value: serde_json::Value =
            serde_json::from_str(raw).map_err(|_| ApprovalCodecError::InvalidShape)?;
        require_exact_keys(&value, &["decision", "note", "source", "danger"])?;
        if let Some(danger) = value.get("danger")
            && !danger.is_null()
        {
            require_exact_keys(danger, &["rule_id", "decision", "note"])?;
        }
        let wire: ApprovalWireRead =
            serde_json::from_str(raw).map_err(|_| ApprovalCodecError::InvalidShape)?;
        let note = wire.note.0;
        let decision =
            Approval::parse(&wire.decision).ok_or(ApprovalCodecError::InvalidVocabulary)?;
        let source =
            ApprovalSource::parse(&wire.source).ok_or(ApprovalCodecError::InvalidVocabulary)?;
        if source == ApprovalSource::Legacy {
            return Err(ApprovalCodecError::InvalidSemantics);
        }
        let danger = wire
            .danger
            .0
            .map(|danger| {
                Ok(DangerAudit {
                    rule_id: danger.rule_id,
                    decision: Approval::parse(&danger.decision)
                        .ok_or(ApprovalCodecError::InvalidVocabulary)?,
                    note: danger.note.0,
                })
            })
            .transpose()?;
        let audit = Self {
            decision,
            note,
            source,
            danger,
        };
        audit.validate(false)?;
        Ok(audit)
    }

    /// Encodes the canonical strict four-field S5 JSON shape.
    pub fn to_json(&self) -> Result<String, ApprovalCodecError> {
        if self.source == ApprovalSource::Legacy {
            return Err(ApprovalCodecError::LegacyWrite);
        }
        self.validate(true)?;
        serde_json::to_string(&ApprovalWireWrite {
            decision: self.decision.as_str(),
            note: &self.note,
            source: self.source.as_str(),
            danger: self.danger.as_ref().map(|danger| DangerWireWrite {
                rule_id: &danger.rule_id,
                decision: danger.decision.as_str(),
                note: &danger.note,
            }),
        })
        .map_err(|_| ApprovalCodecError::InvalidShape)
    }

    fn validate(&self, encoding: bool) -> Result<(), ApprovalCodecError> {
        if self.source == ApprovalSource::Legacy {
            return if encoding {
                Err(ApprovalCodecError::LegacyWrite)
            } else if self.note.is_none() && self.danger.is_none() {
                Ok(())
            } else {
                Err(ApprovalCodecError::InvalidSemantics)
            };
        }
        if self.note.is_some() && self.decision != Approval::Deny {
            return Err(ApprovalCodecError::InvalidSemantics);
        }
        if let Some(danger) = &self.danger
            && (danger.rule_id.is_empty()
                || (danger.note.is_some() && danger.decision != Approval::Deny))
        {
            return Err(ApprovalCodecError::InvalidSemantics);
        }
        let valid = match self.source {
            ApprovalSource::Danger => self
                .danger
                .as_ref()
                .is_some_and(|danger| self.decision == danger.decision && self.note == danger.note),
            ApprovalSource::ReadOnly => {
                self.decision == Approval::Deny
                    && self.note.is_none()
                    && self.danger.as_ref().is_none_or(|danger| {
                        matches!(danger.decision, Approval::Once | Approval::Always)
                            && danger.note.is_none()
                    })
            }
            ApprovalSource::RunMode | ApprovalSource::Validation | ApprovalSource::Recovery => {
                self.decision == Approval::Deny && self.note.is_none() && self.danger.is_none()
            }
            ApprovalSource::Rule => {
                self.decision == Approval::Always && self.note.is_none() && self.danger.is_none()
            }
            ApprovalSource::Auto => {
                self.decision == Approval::Once && self.note.is_none() && self.danger.is_none()
            }
            ApprovalSource::User => self.danger.is_none(),
            ApprovalSource::Timeout => {
                self.decision == Approval::Deny
                    && self.note.is_none()
                    && self.danger.as_ref().is_none_or(|danger| {
                        danger.decision == Approval::Deny && danger.note.is_none()
                    })
            }
            ApprovalSource::ReadonlyTool => {
                self.decision == Approval::Once && self.note.is_none() && self.danger.is_none()
            }
            ApprovalSource::Legacy => false,
        };
        valid
            .then_some(())
            .ok_or(ApprovalCodecError::InvalidSemantics)
    }
}

pub(crate) fn require_exact_keys(
    value: &serde_json::Value,
    expected: &[&str],
) -> Result<(), ApprovalCodecError> {
    let object = value.as_object().ok_or(ApprovalCodecError::InvalidShape)?;
    if object.len() != expected.len() || expected.iter().any(|key| !object.contains_key(*key)) {
        return Err(ApprovalCodecError::InvalidShape);
    }
    Ok(())
}

/// Maps a runtime prompt into the shared content-free UI projection.
pub fn permission_request_from_runtime(
    prompt: &vega_runtime::RuntimePermissionPrompt,
) -> PermissionRequest {
    PermissionRequest {
        call_id: prompt.target.call_id.clone(),
        tool: prompt.target.tool.as_str().to_string(),
        display_target: prompt.target.display_target.clone(),
        danger_rule_id: prompt.danger.as_ref().map(|danger| danger.rule_id.clone()),
        danger_reason: prompt.danger.as_ref().map(|danger| danger.reason.clone()),
    }
}

/// Maps runtime-local audit facts into the shared strict audit type.
pub fn approval_audit_from_runtime(audit: &vega_runtime::RuntimeApprovalAudit) -> ApprovalAudit {
    ApprovalAudit {
        decision: approval_from_runtime(audit.decision),
        note: audit.note.clone(),
        source: match audit.source {
            vega_runtime::RuntimeApprovalSource::Danger => ApprovalSource::Danger,
            vega_runtime::RuntimeApprovalSource::ReadOnly => ApprovalSource::ReadOnly,
            vega_runtime::RuntimeApprovalSource::RunMode => ApprovalSource::RunMode,
            vega_runtime::RuntimeApprovalSource::Rule => ApprovalSource::Rule,
            vega_runtime::RuntimeApprovalSource::Auto => ApprovalSource::Auto,
            vega_runtime::RuntimeApprovalSource::User => ApprovalSource::User,
            vega_runtime::RuntimeApprovalSource::Timeout => ApprovalSource::Timeout,
            vega_runtime::RuntimeApprovalSource::Validation => ApprovalSource::Validation,
            vega_runtime::RuntimeApprovalSource::ReadonlyTool => ApprovalSource::ReadonlyTool,
            vega_runtime::RuntimeApprovalSource::Recovery => ApprovalSource::Recovery,
            vega_runtime::RuntimeApprovalSource::Legacy => ApprovalSource::Legacy,
        },
        danger: audit.danger.as_ref().map(|danger| DangerAudit {
            rule_id: danger.rule_id.clone(),
            decision: approval_from_runtime(danger.decision),
            note: danger.note.clone(),
        }),
    }
}

/// Maps a strict persisted/shared audit back into the runtime-local recovery
/// representation without weakening its source or nested danger facts.
pub fn approval_audit_to_runtime(audit: &ApprovalAudit) -> vega_runtime::RuntimeApprovalAudit {
    vega_runtime::RuntimeApprovalAudit {
        decision: approval_to_runtime(audit.decision),
        note: audit.note.clone(),
        source: match audit.source {
            ApprovalSource::Danger => vega_runtime::RuntimeApprovalSource::Danger,
            ApprovalSource::ReadOnly => vega_runtime::RuntimeApprovalSource::ReadOnly,
            ApprovalSource::RunMode => vega_runtime::RuntimeApprovalSource::RunMode,
            ApprovalSource::Rule => vega_runtime::RuntimeApprovalSource::Rule,
            ApprovalSource::Auto => vega_runtime::RuntimeApprovalSource::Auto,
            ApprovalSource::User => vega_runtime::RuntimeApprovalSource::User,
            ApprovalSource::Timeout => vega_runtime::RuntimeApprovalSource::Timeout,
            ApprovalSource::Validation => vega_runtime::RuntimeApprovalSource::Validation,
            ApprovalSource::ReadonlyTool => vega_runtime::RuntimeApprovalSource::ReadonlyTool,
            ApprovalSource::Recovery => vega_runtime::RuntimeApprovalSource::Recovery,
            ApprovalSource::Legacy => vega_runtime::RuntimeApprovalSource::Legacy,
        },
        danger: audit
            .danger
            .as_ref()
            .map(|danger| vega_runtime::RuntimeDangerAudit {
                rule_id: danger.rule_id.clone(),
                decision: approval_to_runtime(danger.decision),
                note: danger.note.clone(),
            }),
    }
}

pub(crate) fn approval_to_runtime(decision: Approval) -> vega_runtime::RuntimeApprovalDecision {
    match decision {
        Approval::Once => vega_runtime::RuntimeApprovalDecision::Once,
        Approval::Always => vega_runtime::RuntimeApprovalDecision::Always,
        Approval::Deny => vega_runtime::RuntimeApprovalDecision::Deny,
    }
}

pub(crate) fn approval_from_runtime(decision: vega_runtime::RuntimeApprovalDecision) -> Approval {
    match decision {
        vega_runtime::RuntimeApprovalDecision::Once => Approval::Once,
        vega_runtime::RuntimeApprovalDecision::Always => Approval::Always,
        vega_runtime::RuntimeApprovalDecision::Deny => Approval::Deny,
    }
}

/// Maps a shared UI decision into the runtime-local decision vocabulary.
pub fn permission_decision_to_runtime(
    decision: PermissionDecision,
) -> vega_runtime::RuntimeUserDecision {
    match decision {
        PermissionDecision::Once => vega_runtime::RuntimeUserDecision::Once,
        PermissionDecision::Always => vega_runtime::RuntimeUserDecision::Always,
        PermissionDecision::Deny { note } => vega_runtime::RuntimeUserDecision::Deny { note },
        PermissionDecision::Timeout => vega_runtime::RuntimeUserDecision::Timeout,
    }
}

/// `RunMode` name used by the tech spec; the persisted implementation was
/// introduced as [`ThreadMode`] in S2.
pub type RunMode = ThreadMode;
