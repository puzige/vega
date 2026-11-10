use std::path::PathBuf;
use std::sync::Mutex;

use vega_store::permission_contracts::{ContractState, ThreadPermissionRecord};
use vega_store::thread_admission::ThreadAdmission;
use vega_store::write_coordination::{WriteCategory, WriteTransaction};
use vega_store::{DatabaseIdentity, Store};

use crate::database_writer::{DatabaseCommand, Sealed};
use crate::types::*;

#[cfg(test)]
#[path = "codex_tasks/persistence_tests.rs"]
mod persistence_tests;

pub struct CodexPersistence {
    path: PathBuf,
    database: DatabaseIdentity,
    source: Mutex<CodexSource>,
    abandonment: Mutex<Option<crate::database_writer::NecessaryLease>>,
    external_started: std::sync::atomic::AtomicBool,
}

#[derive(Clone)]
pub(crate) struct CodexSource {
    pub(crate) task: CodexTaskIdentity,
    pub(crate) owner: ThreadAdmission,
    pub(crate) authority: ContractState,
    pub(crate) permission: ThreadPermissionRecord,
    pub(crate) message_id: Option<String>,
}

pub enum CodexSessionOutcome {
    Confirmed { session_id: String },
    Uncertain { code: CodexSessionUncertaintyCode },
    DefinitivelyFailed { code: CodexSessionFailureCode },
}

pub enum CodexMessageStatus {
    Done,
    Failed,
    Interrupted,
}

impl CodexMessageStatus {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }
}

pub struct CodexMessageReceipt {
    pub user_message_id: String,
    pub assistant_message_id: String,
    pub assistant_seq: i64,
    pub created_at: i64,
}

pub struct CodexToolProposal {
    pub call_id: String,
    pub message_id: String,
    pub identity: CodexAcpActivityIdentity,
    pub preview: CodexArgumentPreview,
    pub created_at: i64,
}

pub struct CodexToolSource {
    pub call_id: String,
    pub message_id: String,
    pub identity: CodexAcpActivityIdentity,
    pub preview: CodexArgumentPreview,
}

pub enum CodexToolChange {
    Start {
        selected_by_user: bool,
        observed_in_progress: bool,
    },
    Reject {
        source: ApprovalSource,
        reason: CodexTerminalReason,
    },
    Finish {
        status: ToolCallStatus,
        reason: CodexTerminalReason,
        selected_by_user: bool,
        rejected_by_user: bool,
    },
}

impl CodexPersistence {
    pub(crate) fn from_startup(
        path: PathBuf,
        database: DatabaseIdentity,
        source: CodexSource,
        abandonment: crate::database_writer::NecessaryLease,
    ) -> Self {
        Self {
            path,
            database,
            source: Mutex::new(source),
            abandonment: Mutex::new(Some(abandonment)),
            external_started: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub fn settle_abandoned_startup(&self) -> Result<(), String> {
        let source = self
            .source
            .lock()
            .map_err(|_| "Codex source unavailable")?
            .clone();
        let CodexSessionCreationState::Intent { intent_id } = &source.task.session_creation else {
            return Ok(());
        };
        crate::database_writer::commit(
            &self.path,
            self.database,
            crate::codex_tasks::startup::AbandonedStartup {
                thread_id: source.task.thread_id.clone(),
                snapshot: source.task.snapshot.clone(),
                intent_id: intent_id.clone(),
                owner: source.owner.clone(),
                external_started: self
                    .external_started
                    .load(std::sync::atomic::Ordering::Acquire),
            },
        )??;
        self.abandonment
            .lock()
            .map_err(|_| "Codex abandonment lease unavailable")?
            .take();
        Ok(())
    }

    pub fn validate_external_start(&self) -> Result<(), String> {
        self.submit(CodexWrite::ValidateExternalStart).map(|_| ())
    }

    pub fn mark_external_started(&self) {
        self.external_started
            .store(true, std::sync::atomic::Ordering::Release);
    }

    fn submit(&self, operation: CodexWrite) -> Result<CodexReceipt, String> {
        let source = self
            .source
            .lock()
            .map_err(|_| "Codex source unavailable")?
            .clone();
        crate::database_writer::commit(
            &self.path,
            self.database,
            CodexCommand { source, operation },
        )?
    }

    pub fn session_outcome(&self, outcome: CodexSessionOutcome, now: i64) -> Result<(), String> {
        let CodexReceipt::Session(state) = self.submit(CodexWrite::Session { outcome, now })?
        else {
            return Err("Codex session receipt mismatch".into());
        };
        self.source
            .lock()
            .map_err(|_| "Codex source unavailable")?
            .task
            .session_creation = state;
        Ok(())
    }

    pub fn create_messages(
        &self,
        prompt: String,
        user_id: String,
        assistant_id: String,
        now: i64,
    ) -> Result<CodexMessageReceipt, String> {
        match self.submit(CodexWrite::Messages {
            prompt,
            user_id,
            assistant_id,
            now,
        })? {
            CodexReceipt::Messages(receipt) => {
                self.source
                    .lock()
                    .map_err(|_| "Codex source unavailable")?
                    .message_id = Some(receipt.assistant_message_id.clone());
                Ok(receipt)
            }
            _ => Err("Codex message receipt mismatch".into()),
        }
    }

    pub fn stream(&self, message_id: String, content: String) -> Result<(), String> {
        self.submit(CodexWrite::Stream {
            message_id,
            content,
        })
        .map(|_| ())
    }

    pub fn finish_message(
        &self,
        message_id: String,
        content: String,
        status: CodexMessageStatus,
        duration_ms: i64,
    ) -> Result<(), String> {
        self.submit(CodexWrite::FinishMessage {
            message_id,
            content,
            status,
            duration_ms,
        })
        .map(|_| ())
    }

    pub fn propose_tool(&self, proposal: CodexToolProposal) -> Result<(), String> {
        self.submit(CodexWrite::Propose(proposal)).map(|_| ())
    }

    pub fn change_tool(
        &self,
        source: CodexToolSource,
        change: CodexToolChange,
        now: i64,
    ) -> Result<(), String> {
        self.submit(CodexWrite::Tool {
            source,
            change,
            now,
        })
        .map(|_| ())
    }
}

impl Drop for CodexPersistence {
    fn drop(&mut self) {
        let source = self
            .source
            .get_mut()
            .unwrap_or_else(|poison| poison.into_inner());
        let CodexSessionCreationState::Intent { intent_id } = &source.task.session_creation else {
            return;
        };
        let Some(lease) = self
            .abandonment
            .get_mut()
            .unwrap_or_else(|poison| poison.into_inner())
            .take()
        else {
            return;
        };
        let command = crate::codex_tasks::startup::AbandonedStartup {
            thread_id: source.task.thread_id.clone(),
            snapshot: source.task.snapshot.clone(),
            intent_id: intent_id.clone(),
            owner: source.owner.clone(),
            external_started: self
                .external_started
                .load(std::sync::atomic::Ordering::Acquire),
        };
        if let Err(error) = lease.submit(self.database, command) {
            tracing::error!(error = %error, "Codex abandoned startup reconciliation not queued");
        }
    }
}

fn error(value: impl std::fmt::Display) -> String {
    value.to_string()
}

fn input_json(identity: &CodexAcpActivityIdentity) -> String {
    serde_json::json!({
        "kind": identity.kind,
        "arguments_bytes": identity.arguments_bytes,
        "arguments_sha256": identity.arguments_sha256,
        "argument_preview": identity.argument_preview,
    })
    .to_string()
}

enum CodexWrite {
    ValidateExternalStart,
    Session {
        outcome: CodexSessionOutcome,
        now: i64,
    },
    Messages {
        prompt: String,
        user_id: String,
        assistant_id: String,
        now: i64,
    },
    Stream {
        message_id: String,
        content: String,
    },
    FinishMessage {
        message_id: String,
        content: String,
        status: CodexMessageStatus,
        duration_ms: i64,
    },
    Propose(CodexToolProposal),
    Tool {
        source: CodexToolSource,
        change: CodexToolChange,
        now: i64,
    },
}

enum CodexReceipt {
    Applied,
    Session(CodexSessionCreationState),
    Messages(CodexMessageReceipt),
}

struct CodexCommand {
    source: CodexSource,
    operation: CodexWrite,
}

impl Sealed for CodexCommand {}

impl DatabaseCommand for CodexCommand {
    type Output = Result<CodexReceipt, String>;

    fn apply(self, store: &Store) -> Self::Output {
        let _permission = crate::permission_contracts::coordinate().map_err(error)?;
        crate::thread_access::verify(store, &self.source.task.thread_id, &self.source.owner)
            .map_err(error)?;
        let tx = WriteTransaction::new(store.conn(), WriteCategory::CodexTasks).map_err(error)?;
        let current = crate::codex_tasks::codex_task_identity(store, &self.source.task.thread_id)
            .map_err(error)?
            .ok_or("Codex task disappeared")?;
        if current != self.source.task {
            return Err("Codex task source changed before commit".into());
        }
        let sensitive = matches!(
            &self.operation,
            CodexWrite::ValidateExternalStart
                | CodexWrite::Messages { .. }
                | CodexWrite::Propose(_)
                | CodexWrite::Tool {
                    change: CodexToolChange::Start { .. },
                    ..
                }
                | CodexWrite::Session {
                    outcome: CodexSessionOutcome::Confirmed { .. },
                    ..
                }
        );
        if sensitive {
            crate::permission_contracts::require_admission_open(store).map_err(error)?;
            let authority = vega_store::permission_contracts::load_state(&tx).map_err(error)?;
            let permission = vega_store::permission_contracts::load_thread(&tx, &current.thread_id)
                .map_err(error)?;
            let thread = vega_store::threads::find(&tx, &current.thread_id)
                .map_err(error)?
                .ok_or("Codex thread disappeared")?;
            if authority != self.source.authority
                || authority.reset_pending
                || permission.as_ref() != Some(&self.source.permission)
                || thread.status != "active"
                || (!thread.project_id.is_empty()).then_some(thread.project_id.as_str())
                    != current.snapshot.workspace.project_id.as_deref()
                || (!thread.project_id.is_empty()
                    && !vega_store::projects::project_exists(&tx, &thread.project_id)
                        .map_err(error)?)
            {
                return Err("Codex permission source changed before commit".into());
            }
        }
        if matches!(&self.operation, CodexWrite::ValidateExternalStart)
            && !matches!(
                current.session_creation,
                CodexSessionCreationState::Intent { .. }
            )
        {
            return Err("Codex external startup intent changed".into());
        }
        if !matches!(
            &self.operation,
            CodexWrite::Session { .. } | CodexWrite::ValidateExternalStart
        ) && !matches!(
            current.session_creation,
            CodexSessionCreationState::Confirmed { .. }
        ) {
            return Err("Codex session is not confirmed".into());
        }
        match &self.operation {
            CodexWrite::Messages { .. } => {
                let installed: bool = tx
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM messages WHERE thread_id=?1)",
                        [&current.thread_id],
                        |row| row.get(0),
                    )
                    .map_err(error)?;
                if self.source.message_id.is_some() || installed {
                    return Err(
                        "Codex initial message pair already installed; reconcile existing messages"
                            .into(),
                    );
                }
            }
            CodexWrite::Stream { message_id, .. }
            | CodexWrite::FinishMessage { message_id, .. } => {
                if self.source.message_id.as_ref() != Some(message_id) {
                    return Err("Codex active message source changed".into());
                }
            }
            CodexWrite::Propose(proposal) => {
                if self.source.message_id.as_ref() != Some(&proposal.message_id) {
                    return Err("Codex proposal message source changed".into());
                }
            }
            CodexWrite::Tool { source, .. }
                if self.source.message_id.as_ref() != Some(&source.message_id) =>
            {
                return Err("Codex tool message source changed".into());
            }
            _ => {}
        }
        let result = execute_codex_write(&tx, &current, self.operation)?;
        tx.commit().map_err(|failure| {
            format!("Codex commit failed; outcome requires reconciliation: {failure}")
        })?;
        Ok(result)
    }
}

fn require_message(tx: &vega_store::Transaction<'_>, thread: &str, id: &str) -> Result<(), String> {
    let message = vega_store::messages::find(tx, id)
        .map_err(error)?
        .ok_or("Codex message missing")?;
    if message.thread_id != thread || message.role != "assistant" || message.status != "streaming" {
        return Err("Codex message source changed".into());
    }
    Ok(())
}

fn execute_codex_write(
    tx: &vega_store::Transaction<'_>,
    task: &CodexTaskIdentity,
    operation: CodexWrite,
) -> Result<CodexReceipt, String> {
    let thread = task.thread_id.as_str();
    match operation {
        CodexWrite::ValidateExternalStart => Ok(CodexReceipt::Applied),
        CodexWrite::Session { outcome, now } => {
            let CodexSessionCreationState::Intent { intent_id } = &task.session_creation else {
                return Err("Codex session intent changed".into());
            };
            let state = match outcome {
                CodexSessionOutcome::Confirmed { session_id } => {
                    if session_id.is_empty()
                        || session_id.len() > 512
                        || session_id.chars().any(char::is_control)
                    {
                        return Err("Codex session identity invalid".into());
                    }
                    if !vega_store::codex_tasks::confirm_session_creation(
                        tx,
                        thread,
                        intent_id.as_str(),
                        &session_id,
                        now,
                    )
                    .map_err(error)?
                    {
                        return Err("Codex session confirmation not applied".into());
                    }
                    CodexSessionCreationState::Confirmed {
                        intent_id: intent_id.clone(),
                        session_id,
                    }
                }
                CodexSessionOutcome::Uncertain { code } => {
                    if !vega_store::codex_tasks::mark_session_uncertain(
                        tx,
                        thread,
                        intent_id.as_str(),
                        code.as_str(),
                        now,
                    )
                    .map_err(error)?
                    {
                        return Err("Codex uncertain session not applied".into());
                    }
                    CodexSessionCreationState::Uncertain {
                        intent_id: intent_id.clone(),
                        code,
                    }
                }
                CodexSessionOutcome::DefinitivelyFailed { code } => {
                    if !vega_store::codex_tasks::mark_session_definitively_failed(
                        tx,
                        thread,
                        intent_id.as_str(),
                        code.as_str(),
                        now,
                    )
                    .map_err(error)?
                    {
                        return Err("Codex failed session not applied".into());
                    }
                    CodexSessionCreationState::DefinitivelyFailed {
                        intent_id: intent_id.clone(),
                        code,
                    }
                }
            };
            Ok(CodexReceipt::Session(state))
        }
        CodexWrite::Messages {
            prompt,
            user_id,
            assistant_id,
            now,
        } => {
            let user_seq = vega_store::messages::next_seq(tx, thread).map_err(error)?;
            for (id, seq, role, content, status) in [
                (user_id.clone(), user_seq, "user", prompt, "done"),
                (
                    assistant_id.clone(),
                    user_seq
                        .checked_add(1)
                        .ok_or("Codex message sequence overflow")?,
                    "assistant",
                    String::new(),
                    "streaming",
                ),
            ] {
                vega_store::messages::insert(
                    tx,
                    &vega_store::messages::MessageRow {
                        id,
                        thread_id: thread.into(),
                        seq,
                        role: role.into(),
                        kind: "text".into(),
                        content,
                        status: status.into(),
                        created_at: now,
                        plan_status: None,
                        plan_review_note: None,
                        plan_reviewed_at: None,
                    },
                )
                .map_err(error)?;
            }
            Ok(CodexReceipt::Messages(CodexMessageReceipt {
                user_message_id: user_id,
                assistant_message_id: assistant_id,
                assistant_seq: user_seq + 1,
                created_at: now,
            }))
        }
        CodexWrite::Stream {
            message_id,
            content,
        } => {
            require_message(tx, thread, &message_id)?;
            if vega_store::messages::update_streaming_content(tx, &message_id, &content)
                .map_err(error)?
                != 1
            {
                return Err("Codex streaming update not applied".into());
            }
            Ok(CodexReceipt::Applied)
        }
        CodexWrite::FinishMessage {
            message_id,
            content,
            status,
            duration_ms,
        } => {
            require_message(tx, thread, &message_id)?;
            if duration_ms < 0
                || vega_store::messages::finish_streaming_in_transaction(
                    tx,
                    &message_id,
                    &content,
                    status.as_str(),
                    Some(duration_ms),
                )
                .map_err(error)?
                    != 1
            {
                return Err("Codex message finish not applied".into());
            }
            Ok(CodexReceipt::Applied)
        }
        CodexWrite::Propose(proposal) => {
            require_message(tx, thread, &proposal.message_id)?;
            if !proposal.identity.is_valid()
                || (CodexDisplayMetadata {
                    preview: proposal.preview,
                    reason: None,
                })
                .validated_preview(&proposal.identity)
                .is_none()
            {
                return Err("Codex proposal metadata mismatch".into());
            }
            let input = input_json(&proposal.identity);
            let seq = vega_store::tool_calls::next_seq(tx, thread).map_err(error)?;
            let (kind, count) = proposal.preview.storage_parts();
            vega_store::tool_calls::insert_codex_pending_in_transaction(
                tx,
                vega_store::tool_calls::NewToolCall {
                    id: &proposal.call_id,
                    thread_id: thread,
                    message_id: &proposal.message_id,
                    seq,
                    tool: "codex_acp",
                    input_json: &input,
                    description: None,
                    status: "pending_approval",
                    created_at: proposal.created_at,
                },
                &vega_store::tool_calls::StoredCodexDisplayMetadata {
                    preview_kind: kind.into(),
                    preview_count: count,
                    terminal_reason: None,
                },
            )
            .map_err(error)?;
            Ok(CodexReceipt::Applied)
        }
        CodexWrite::Tool {
            source,
            change,
            now,
        } => {
            require_message(tx, thread, &source.message_id)?;
            let identity = vega_store::tool_calls::find_identity(tx, &source.call_id)
                .map_err(error)?
                .ok_or("Codex tool missing")?;
            let state = vega_store::tool_calls::find_state(tx, &source.call_id)
                .map_err(error)?
                .ok_or("Codex tool state missing")?;
            if identity.thread_id != thread
                || identity.message_id != source.message_id
                || identity.tool != "codex_acp"
                || identity.input_json != input_json(&source.identity)
                || (CodexDisplayMetadata {
                    preview: source.preview,
                    reason: None,
                })
                .validated_preview(&source.identity)
                .is_none()
            {
                return Err("Codex tool source changed".into());
            }
            let (preview_kind, preview_count, terminal_reason): (String, Option<i64>, Option<String>) = tx.query_row(
                "SELECT preview_kind, preview_count, terminal_reason FROM codex_display_metadata WHERE tool_call_id=?1",
                [&source.call_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            ).map_err(error)?;
            let expected_preview = source.preview.storage_parts();
            if preview_kind != expected_preview.0
                || preview_count != expected_preview.1
                || terminal_reason.is_some()
            {
                return Err("Codex display metadata source changed".into());
            }
            match change {
                CodexToolChange::Start {
                    selected_by_user,
                    observed_in_progress,
                } => {
                    if !observed_in_progress {
                        return Err("Codex running fact missing".into());
                    }
                    let audit = ApprovalAudit {
                        decision: Approval::Once,
                        note: None,
                        source: if selected_by_user {
                            ApprovalSource::User
                        } else {
                            ApprovalSource::Auto
                        },
                        danger: None,
                    };
                    vega_store::tool_calls::approve_in_transaction(
                        tx,
                        &source.call_id,
                        &audit.to_json().map_err(error)?,
                        None,
                        now,
                    )
                    .map_err(error)?;
                    vega_store::tool_calls::mark_running(tx, &source.call_id).map_err(error)?;
                }
                CodexToolChange::Reject {
                    source: approval_source,
                    reason,
                } => {
                    if !matches!(
                        reason,
                        CodexTerminalReason::PermissionRejected
                            | CodexTerminalReason::PermissionInvalid
                            | CodexTerminalReason::PermissionEnded
                            | CodexTerminalReason::PermissionIncomplete
                    ) {
                        return Err("Codex rejection reason mismatch".into());
                    }
                    let audit = ApprovalAudit {
                        decision: Approval::Deny,
                        note: None,
                        source: approval_source,
                        danger: None,
                    };
                    vega_store::tool_calls::reject_codex_in_transaction(
                        tx,
                        &source.call_id,
                        &audit.to_json().map_err(error)?,
                        reason.canonical_output(),
                        now,
                        reason.as_str(),
                    )
                    .map_err(error)?;
                }
                CodexToolChange::Finish {
                    status,
                    reason,
                    selected_by_user,
                    rejected_by_user,
                } => {
                    if selected_by_user && rejected_by_user {
                        return Err("Codex conflicting user approval facts".into());
                    }
                    let status = match (status, reason) {
                        (ToolCallStatus::Success, CodexTerminalReason::ToolCompleted) => "success",
                        (
                            ToolCallStatus::Failed,
                            CodexTerminalReason::ToolFailed | CodexTerminalReason::ToolIncomplete,
                        ) => "failed",
                        (
                            ToolCallStatus::Cancelled,
                            CodexTerminalReason::ToolCancelled
                            | CodexTerminalReason::ToolIncomplete,
                        ) => "cancelled",
                        _ => return Err("Codex terminal reason mismatch".into()),
                    };
                    if state.status == "pending_approval" && rejected_by_user {
                        let audit = ApprovalAudit {
                            decision: Approval::Deny,
                            note: None,
                            source: ApprovalSource::User,
                            danger: None,
                        };
                        vega_store::tool_calls::finish_codex_observed_after_denial_in_transaction(
                            tx,
                            vega_store::tool_calls::FinishToolCall {
                                id: &source.call_id,
                                status,
                                output_text: reason.canonical_output(),
                                exit_code: None,
                                duration_ms: None,
                                finished_at: now,
                            },
                            &audit.to_json().map_err(error)?,
                            reason.as_str(),
                        )
                        .map_err(error)?;
                        return Ok(CodexReceipt::Applied);
                    }
                    if state.status == "pending_approval" {
                        if reason == CodexTerminalReason::ToolIncomplete {
                            return Err("Codex terminal execution fact missing".into());
                        }
                        let audit = ApprovalAudit {
                            decision: Approval::Once,
                            note: None,
                            source: if selected_by_user {
                                ApprovalSource::User
                            } else {
                                ApprovalSource::Auto
                            },
                            danger: None,
                        };
                        vega_store::tool_calls::approve_in_transaction(
                            tx,
                            &source.call_id,
                            &audit.to_json().map_err(error)?,
                            None,
                            now,
                        )
                        .map_err(error)?;
                        vega_store::tool_calls::mark_running(tx, &source.call_id).map_err(error)?;
                    }
                    vega_store::tool_calls::finish_codex_in_transaction(
                        tx,
                        vega_store::tool_calls::FinishToolCall {
                            id: &source.call_id,
                            status,
                            output_text: reason.canonical_output(),
                            exit_code: None,
                            duration_ms: None,
                            finished_at: now,
                        },
                        reason.as_str(),
                    )
                    .map_err(error)?;
                }
            }
            Ok(CodexReceipt::Applied)
        }
    }
}
