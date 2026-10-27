use std::collections::BTreeSet;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tokio_util::sync::CancellationToken;
use vega_store::command_scope::{self as persistence, ScopeAuditRecord};
use vega_store::permission_contracts;
use vega_store::write_coordination::{WriteCategory, WriteTransaction};
use vega_store::{Connection, Store};
use vega_tools::PreparedBash;

use crate::types::{
    Approval, ApprovalAudit, ApprovalSource, CommandScopeBindingsReader, CommandScopeDecision,
    CommandScopeError, CommandScopeRequest, PermissionFence, RequestedCommandScope,
};

pub struct CommandScopeRun {
    inner: Arc<LiveScopeRun>,
}

struct LiveScopeRun {
    id: String,
    run: String,
    fence: PermissionFence,
    cancel: CancellationToken,
    bindings: CommandScopeBindingsReader,
}

struct ScopeState {
    owner: Weak<LiveScopeRun>,
    request: CommandScopeRequest,
    encoded: String,
    deadline: Instant,
}

pub struct PendingCommandScope {
    state: ScopeState,
}

pub struct CommandScopeToken {
    state: ScopeState,
}

pub struct ConsumedCommandScope {
    request: CommandScopeRequest,
}

impl PendingCommandScope {
    pub fn request(&self) -> &CommandScopeRequest {
        &self.state.request
    }
}

impl ConsumedCommandScope {
    pub fn request(&self) -> &CommandScopeRequest {
        &self.request
    }
}

impl CommandScopeRun {
    pub fn new(
        store: &Store,
        fence: PermissionFence,
        run: String,
        cancel: CancellationToken,
        bindings: CommandScopeBindingsReader,
    ) -> Result<Self, CommandScopeError> {
        let _coordination = crate::permission_contracts::coordinate().map_err(unavailable)?;
        crate::permission_contracts::require_admission_open(store).map_err(unavailable)?;
        let inner = LiveScopeRun {
            id: ulid::Ulid::generate().to_string(),
            run,
            fence,
            cancel,
            bindings,
        };
        if inner.cancel.is_cancelled() {
            return Err(CommandScopeError::Cancelled);
        }
        inner.validate_live(store, store.conn())?;
        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    pub fn request(
        &self,
        store: &Store,
        call: &str,
        prepared: &PreparedBash,
        scope: RequestedCommandScope,
        timeout: Duration,
    ) -> Result<PendingCommandScope, CommandScopeError> {
        if !scope.write_roots().is_empty() {
            return Err(CommandScopeError::ScopeUnavailable);
        }
        self.request_with_directories(store, call, prepared, scope, Vec::new(), timeout)
    }

    pub(crate) fn request_prepared(
        &self,
        store: &Store,
        call: &str,
        prepared: &PreparedBash,
        scope: &vega_tools::PreparedBashScope,
        policy: vega_tools::BashLaunchPolicy,
        timeout: Duration,
    ) -> Result<PendingCommandScope, CommandScopeError> {
        let expected_policy = if self.inner.fence.run_mode != "execute"
            || self.inner.fence.permission_mode == "readonly"
        {
            vega_tools::BashLaunchPolicy::ReadOnly
        } else {
            vega_tools::BashLaunchPolicy::WorkspaceWrite
        };
        if policy != expected_policy || scope.bindings() != (self.inner.bindings)()?.as_slice() {
            return Err(CommandScopeError::StaleAuthority);
        }
        scope.validate(prepared, policy).map_err(unavailable)?;
        let requested = prepared
            .requested_scope()
            .ok_or(CommandScopeError::InvalidRequest)?;
        if !requested.write_roots().is_empty()
            && (self.inner.fence.run_mode != "execute"
                || !matches!(
                    self.inner.fence.permission_mode.as_str(),
                    "confirm" | "auto"
                ))
        {
            return Err(CommandScopeError::ScopeUnavailable);
        }
        self.request_with_directories(
            store,
            call,
            prepared,
            requested.into(),
            scope.directories().to_vec(),
            timeout,
        )
    }

    fn request_with_directories(
        &self,
        store: &Store,
        call: &str,
        prepared: &PreparedBash,
        scope: RequestedCommandScope,
        directories: Vec<vega_tools::BashScopeDirectory>,
        timeout: Duration,
    ) -> Result<PendingCommandScope, CommandScopeError> {
        if call.trim().is_empty()
            || timeout > crate::agent::PERMISSION_TIMEOUT
            || prepared
                .requested_scope()
                .map(RequestedCommandScope::from)
                .as_ref()
                != Some(&scope)
        {
            return Err(CommandScopeError::InvalidRequest);
        }
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or(CommandScopeError::InvalidRequest)?;
        self.inner.check_deadline(deadline)?;
        let bindings = (self.inner.bindings)()?;
        validate_bindings(&bindings)?;
        let project_root = prepared
            .project_root()
            .to_str()
            .ok_or(CommandScopeError::InvalidRequest)?;
        if project_root != self.inner.fence.workspace {
            return Err(CommandScopeError::StaleAuthority);
        }
        let _coordination = crate::permission_contracts::coordinate().map_err(unavailable)?;
        crate::permission_contracts::require_admission_open(store).map_err(unavailable)?;
        let tx =
            WriteTransaction::new(store.conn(), WriteCategory::Permissions).map_err(unavailable)?;
        self.inner.validate_live(store, &tx)?;
        let (input_json, ordinary_approval) =
            persistence::approved_bash(&tx, &self.inner.fence.thread_id, &self.inner.run, call)
                .map_err(unavailable)?
                .ok_or(CommandScopeError::UnapprovedCall)?;
        let request = CommandScopeRequest {
            schema_version: 1,
            request_id: ulid::Ulid::generate().to_string(),
            live_run_id: self.inner.id.clone(),
            run_id: self.inner.run.clone(),
            call_id: call.into(),
            command: prepared.command().into(),
            timeout_ms: prepared.timeout_ms(),
            tool_instance: prepared.tool_instance_id(),
            project_root: project_root.into(),
            fence: self.inner.fence.clone(),
            scope,
            directories,
            prepared_input_json: prepared.input_json(),
            bindings,
            input_json,
            ordinary_approval,
        };
        validate_ordinary(&request)?;
        let encoded = serde_json::to_string(&request).map_err(unavailable)?;
        let state = ScopeState {
            owner: Arc::downgrade(&self.inner),
            request,
            encoded,
            deadline,
        };
        self.inner.check_deadline(deadline)?;
        if !persistence::insert_pending(&tx, &state.record(), now()?).map_err(unavailable)? {
            return Err(CommandScopeError::Conflict);
        }
        tx.commit().map_err(unavailable)?;
        self.inner.check_deadline(deadline)?;
        Ok(PendingCommandScope { state })
    }

    pub fn decide(
        &self,
        store: &Store,
        pending: PendingCommandScope,
        decision: CommandScopeDecision,
    ) -> Result<Option<CommandScopeToken>, CommandScopeError> {
        let target = match decision {
            CommandScopeDecision::AllowOnce => "approved",
            CommandScopeDecision::Deny => "denied",
            CommandScopeDecision::Timeout => "timeout",
        };
        self.advance(store, &pending.state, "pending", target)?;
        Ok(
            (decision == CommandScopeDecision::AllowOnce).then_some(CommandScopeToken {
                state: pending.state,
            }),
        )
    }

    pub fn consume(
        &self,
        store: &Store,
        token: CommandScopeToken,
        current: &CommandScopeRequest,
        prepared: &PreparedBash,
    ) -> Result<ConsumedCommandScope, CommandScopeError> {
        if &token.state.request != current
            || prepared.command() != current.command
            || prepared.timeout_ms() != current.timeout_ms
            || prepared.tool_instance_id() != current.tool_instance
            || prepared.project_root().to_str() != Some(current.project_root.as_str())
            || prepared.input_json() != current.prepared_input_json
        {
            self.record_failure(
                store,
                &token.state,
                "approved",
                CommandScopeError::StaleAuthority,
            );
            return Err(CommandScopeError::StaleAuthority);
        }
        self.advance(store, &token.state, "approved", "consumed")?;
        Ok(ConsumedCommandScope {
            request: token.state.request,
        })
    }

    fn advance(
        &self,
        store: &Store,
        state: &ScopeState,
        from: &str,
        to: &str,
    ) -> Result<(), CommandScopeError> {
        let result = (|| {
            self.check_state(state)?;
            let bindings = (self.inner.bindings)()?;
            validate_bindings(&bindings)?;
            if bindings != state.request.bindings {
                return Err(CommandScopeError::StaleAuthority);
            }
            let _coordination = crate::permission_contracts::coordinate().map_err(unavailable)?;
            crate::permission_contracts::require_admission_open(store).map_err(unavailable)?;
            let tx = WriteTransaction::new(store.conn(), WriteCategory::Permissions)
                .map_err(unavailable)?;
            self.inner.validate_live(store, &tx)?;
            let call = persistence::approved_bash(
                &tx,
                &state.request.fence.thread_id,
                &state.request.run_id,
                &state.request.call_id,
            )
            .map_err(unavailable)?
            .ok_or(CommandScopeError::UnapprovedCall)?;
            if call.0 != state.request.input_json || call.1 != state.request.ordinary_approval {
                return Err(CommandScopeError::StaleAuthority);
            }
            validate_ordinary(&state.request)?;
            self.check_state(state)?;
            if !persistence::transition(&tx, &state.record(), from, to, now()?)
                .map_err(unavailable)?
            {
                return Err(CommandScopeError::Conflict);
            }
            self.check_state(state)?;
            tx.commit().map_err(unavailable)?;
            self.check_state(state)
        })();
        if let Err(error) = result {
            self.record_failure(store, state, from, error);
        }
        result
    }

    fn check_state(&self, state: &ScopeState) -> Result<(), CommandScopeError> {
        let owner = state.owner.upgrade().ok_or(CommandScopeError::OwnerGone)?;
        if !Arc::ptr_eq(&owner, &self.inner)
            || state.request.live_run_id != self.inner.id
            || state.request.fence != self.inner.fence
            || state.request.run_id != self.inner.run
        {
            return Err(CommandScopeError::StaleAuthority);
        }
        for directory in &state.request.directories {
            directory
                .validate()
                .map_err(|_| CommandScopeError::StaleAuthority)?;
        }
        self.inner.check_deadline(state.deadline)
    }

    fn record_failure(
        &self,
        store: &Store,
        state: &ScopeState,
        from: &str,
        error: CommandScopeError,
    ) {
        if store.database_identity().permission_fence_key() != state.request.fence.database_identity
        {
            return;
        }
        let target = match error {
            CommandScopeError::Cancelled | CommandScopeError::OwnerGone => "cancelled",
            CommandScopeError::Expired => "timeout",
            _ => "failed",
        };
        let _ = (|| -> Result<(), CommandScopeError> {
            let _coordination = crate::permission_contracts::coordinate().map_err(unavailable)?;
            let tx = WriteTransaction::new(store.conn(), WriteCategory::Permissions)
                .map_err(unavailable)?;
            persistence::transition(&tx, &state.record(), from, target, now()?)
                .map_err(unavailable)?;
            tx.commit().map_err(unavailable)
        })();
    }
}

impl ScopeState {
    fn record(&self) -> ScopeAuditRecord<'_> {
        ScopeAuditRecord {
            request_id: &self.request.request_id,
            thread_id: &self.request.fence.thread_id,
            run_id: &self.request.run_id,
            call_id: &self.request.call_id,
            request_json: &self.encoded,
        }
    }
}

impl LiveScopeRun {
    fn check_deadline(&self, deadline: Instant) -> Result<(), CommandScopeError> {
        if self.cancel.is_cancelled() {
            Err(CommandScopeError::Cancelled)
        } else if Instant::now() >= deadline {
            Err(CommandScopeError::Expired)
        } else {
            Ok(())
        }
    }

    fn validate_live(&self, store: &Store, conn: &Connection) -> Result<(), CommandScopeError> {
        if !self.fence.is_valid()
            || ulid::Ulid::from_string(&self.run).is_err()
            || store.database_identity().permission_fence_key() != self.fence.database_identity
            || (self.fence.run_mode == "execute" && self.fence.permission_mode == "full_access")
        {
            return Err(CommandScopeError::StaleAuthority);
        }
        let state = permission_contracts::load_state(conn).map_err(unavailable)?;
        if state.reset_pending || state.authority_epoch != self.fence.authority_epoch {
            return Err(CommandScopeError::StaleAuthority);
        }
        let saved = permission_contracts::load_run_fence(conn, &self.run, &self.fence.thread_id)
            .map_err(unavailable)?
            .ok_or(CommandScopeError::StaleAuthority)?;
        require_fence(&saved, &self.fence)?;
        if let Some(source) = &self.fence.delegation {
            let saved =
                permission_contracts::load_delegated_fence(conn, &self.fence.thread_id, &self.run)
                    .map_err(unavailable)?;
            require_fence(&saved, &self.fence)?;
            let parent = permission_contracts::load_run_fence(
                conn,
                &source.parent_message_id,
                &source.parent_fence.thread_id,
            )
            .map_err(unavailable)?
            .ok_or(CommandScopeError::StaleAuthority)?;
            require_fence(&parent, &source.parent_fence)?;
            let execution =
                vega_store::subagents::execution(conn, &self.fence.thread_id, &self.run)
                    .map_err(unavailable)?
                    .ok_or(CommandScopeError::StaleAuthority)?
                    .record;
            if execution.state != "running"
                || vega_store::delegated_stages::release_state(
                    conn,
                    &self.fence.thread_id,
                    &self.run,
                )
                .map_err(unavailable)?
                    != "held"
                || execution.parent_thread_id != source.parent_fence.thread_id
                || execution.parent_message_id != source.parent_message_id
                || source.child_message_id != self.run
            {
                return Err(CommandScopeError::StaleAuthority);
            }
        }
        Ok(())
    }
}

fn require_fence(encoded: &str, fence: &PermissionFence) -> Result<(), CommandScopeError> {
    let saved: PermissionFence = serde_json::from_str(encoded).map_err(unavailable)?;
    if &saved != fence {
        return Err(CommandScopeError::StaleAuthority);
    }
    Ok(())
}

fn validate_bindings(
    bindings: &[vega_credentials::CredentialBinding],
) -> Result<(), CommandScopeError> {
    let mut aliases = BTreeSet::new();
    for binding in bindings {
        binding.validate().map_err(unavailable)?;
        if !aliases.insert(&binding.env) {
            return Err(CommandScopeError::InvalidRequest);
        }
    }
    Ok(())
}

fn validate_ordinary(request: &CommandScopeRequest) -> Result<(), CommandScopeError> {
    let effective = &request.prepared_input_json;
    let approval = ApprovalAudit::from_json(&request.ordinary_approval).map_err(unavailable)?;
    if !vega_tools::bash_inputs_semantically_equal(&request.input_json, effective)
        || !matches!(approval.decision, Approval::Once | Approval::Always)
        || !crate::agent::bash_danger_audit_matches(&request.input_json, &approval)
    {
        return Err(CommandScopeError::UnapprovedCall);
    }
    let readonly =
        request.fence.run_mode != "execute" || request.fence.permission_mode == "readonly";
    let allowed = if readonly {
        approval.source == ApprovalSource::SandboxReadOnly && approval.danger.is_none()
    } else {
        match approval.source {
            ApprovalSource::Danger => approval.danger.is_some(),
            ApprovalSource::User => true,
            ApprovalSource::Auto => request.fence.permission_mode == "auto",
            ApprovalSource::Rule => request
                .fence
                .exact_rules
                .iter()
                .any(|(tool, signature)| tool == "bash" && signature == &request.command),
            _ => false,
        }
    };
    if !allowed {
        return Err(CommandScopeError::UnapprovedCall);
    }
    Ok(())
}

fn now() -> Result<i64, CommandScopeError> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(unavailable)?;
    i64::try_from(elapsed.as_millis()).map_err(unavailable)
}

fn unavailable(_: impl std::fmt::Display) -> CommandScopeError {
    CommandScopeError::Unavailable
}

#[cfg(test)]
pub(crate) mod tests;
