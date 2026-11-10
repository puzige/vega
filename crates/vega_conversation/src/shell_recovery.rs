use std::path::PathBuf;

use crate::types::Thread;
use vega_store::Store;

#[derive(Clone, Debug)]
pub struct ShellRecoveryRow {
    pub operation_id: String,
    pub thread_id: String,
    pub reason: String,
    pub command: String,
    pub exit_code: Option<i32>,
    pub output_incomplete: bool,
    pub can_retry: bool,
    pub can_resume: bool,
}

#[derive(Clone, Debug)]
pub struct ShellCommandProjection {
    pub operation_id: String,
    pub call_id: String,
    pub thread_id: String,
    pub command: String,
    pub state: String,
    pub exit_code: Option<i32>,
    pub duration_ms: Option<u64>,
    pub output_incomplete: bool,
}

#[derive(Clone, Debug)]
pub struct ShellRecoveryProjection {
    pub workspace: String,
    pub epoch: vega_store::shell_operations::ShellRecoveryVersion,
    pub rows: Vec<ShellRecoveryRow>,
    pub active: Vec<vega_runtime::RuntimeActiveShell>,
    pub commands: Vec<ShellCommandProjection>,
}

#[derive(Clone, Debug)]
pub struct ShellRecoveryConfirmation {
    pub source: Thread,
    pub projection: ShellRecoveryProjection,
    pub decision_id: String,
    authority_epoch: String,
    draft: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellRecoveryError {
    Unavailable,
    Changed,
    KnownProcessOrResource,
}

pub struct ShellRecoveryService {
    database_path: PathBuf,
    draft: bool,
}

impl ShellRecoveryService {
    pub fn new(database_path: PathBuf) -> Self {
        Self {
            database_path,
            draft: false,
        }
    }

    pub fn for_draft(database_path: PathBuf) -> Self {
        Self {
            database_path,
            draft: true,
        }
    }

    fn workspace(&self, store: &Store, source: &Thread) -> Result<String, ShellRecoveryError> {
        if !self.draft {
            return workspace(store, source);
        }
        if vega_store::threads::find(store.conn(), &source.id)
            .map_err(unavailable)?
            .is_some()
            || source.backend != crate::types::TaskBackend::Native
            || source.status != crate::types::ThreadStatus::Active
            || source.parent_thread_id.is_some()
            || source.forked_from_thread_id.is_some()
            || source.forked_from_message_id.is_some()
            || !(0..i64::MAX).contains(&source.permission_revision)
            || ulid::Ulid::from_string(&source.id).is_err()
        {
            return Err(ShellRecoveryError::Changed);
        }
        let authority =
            vega_store::permission_contracts::load_state(store.conn()).map_err(unavailable)?;
        if authority.reset_pending
            || source
                .draft_authority_epoch
                .as_ref()
                .is_some_and(|epoch| epoch != &authority.authority_epoch)
        {
            return Err(ShellRecoveryError::Changed);
        }
        project_workspace(store, &source.project_id)
    }

    pub fn read(&self, source: &Thread) -> Result<ShellRecoveryProjection, ShellRecoveryError> {
        let store = Store::open_existing(&self.database_path).map_err(unavailable)?;
        let workspace = self.workspace(&store, source)?;
        reconcile_commits(&store, &workspace)?;
        commit(
            &store,
            RecoverGeneration {
                generation: vega_runtime::shell_generation().into(),
            },
        )?;
        let recovery = vega_store::shell_operations::recovery(store.conn(), &workspace)
            .map_err(unavailable)?;
        for item in &recovery.items {
            vega_runtime::RuntimeWorkspaceMutationGuard::set_persisted_block(
                std::path::Path::new(&workspace),
                &item.operation_id,
                true,
            )
            .map_err(unavailable)?;
        }
        let active =
            vega_runtime::active_shell_sessions(&source.id, std::path::Path::new(&workspace))
                .map_err(unavailable)?;
        let commands =
            vega_store::shell_operations::command_presentations(store.conn(), &workspace)
                .map_err(unavailable)?
                .into_iter()
                .map(|command| {
                    let input = crate::types::tool_card_input_projection(&crate::types::ToolCall {
                        id: command.call_id.clone(),
                        tool: "bash".into(),
                        input_json: command.input_json,
                        description: None,
                        codex_display: None,
                    });
                    let text = match input {
                        crate::types::ToolCardInputProjection::Bash { command } => command,
                        _ => String::new(),
                    };
                    ShellCommandProjection {
                        operation_id: command.operation_id,
                        call_id: command.call_id,
                        thread_id: command.thread_id,
                        command: text,
                        state: command.state,
                        exit_code: command.exit_code,
                        duration_ms: command.duration_ms,
                        output_incomplete: command.output_incomplete,
                    }
                })
                .collect::<Vec<_>>();
        Ok(ShellRecoveryProjection {
            active,
            workspace,
            epoch: recovery.epoch,
            rows: recovery
                .items
                .into_iter()
                .map(|item| {
                    let prior_generation = item.generation != vega_runtime::shell_generation();
                    let can_resume = item.facts.resources_closed
                        && (item.facts.process_resolved
                            || prior_generation
                                && item
                                    .facts
                                    .cleanup_reasons
                                    .iter()
                                    .any(|reason| reason == "control_continuity_lost"));
                    let command = commands
                        .iter()
                        .find(|command| command.operation_id == item.operation_id)
                        .map(|command| command.command.clone())
                        .unwrap_or_default();
                    let mut reasons = item.facts.cleanup_reasons.clone();
                    if let Some(error) = item.error {
                        reasons.push(error);
                    }
                    reasons.sort();
                    reasons.dedup();
                    ShellRecoveryRow {
                        operation_id: item.operation_id,
                        thread_id: item.thread_id,
                        reason: reasons.join(", "),
                        command,
                        exit_code: item.exit_code,
                        output_incomplete: item.facts.output_incomplete,
                        can_retry: !prior_generation,
                        can_resume,
                    }
                })
                .collect(),
            commands,
        })
    }

    pub async fn cancel(
        &self,
        source: &Thread,
        operation_id: &str,
    ) -> Result<ShellRecoveryProjection, ShellRecoveryError> {
        let store = Store::open_existing(&self.database_path).map_err(unavailable)?;
        let workspace = self.workspace(&store, source)?;
        vega_runtime::cancel_shell_session(
            &source.id,
            std::path::Path::new(&workspace),
            operation_id,
        )
        .await
        .map_err(unavailable)?;
        self.read(source)
    }

    pub fn check(
        &self,
        source: &Thread,
        expected: &ShellRecoveryProjection,
    ) -> Result<ShellRecoveryProjection, ShellRecoveryError> {
        let projection = self.read(source)?;
        require_same(&projection, expected)?;
        let store = Store::open_existing(&self.database_path).map_err(unavailable)?;
        for item in vega_store::shell_operations::recovery(store.conn(), &projection.workspace)
            .map_err(unavailable)?
            .items
        {
            if item.generation == vega_runtime::shell_generation()
                || item.facts.resources_closed
                || !item
                    .facts
                    .cleanup_reasons
                    .iter()
                    .any(|reason| reason == "control_continuity_lost")
            {
                continue;
            }
            let closed =
                match vega_store::shell_operations::resource(store.conn(), &item.operation_id)
                    .map_err(unavailable)?
                {
                    Some(identity) => {
                        serde_json::from_str::<vega_tools::BashSessionResource>(&identity)
                            .is_ok_and(|resource| resource.is_closed().unwrap_or(false))
                    }
                    None => true,
                };
            if closed {
                let mut facts = item.facts;
                facts.resources_closed = true;
                facts.cleanup_state = "unresolved".into();
                commit(
                    &store,
                    CleanupObservation {
                        operation: item.operation_id,
                        observation: ulid::Ulid::generate().to_string(),
                        facts,
                    },
                )?;
            }
        }
        self.read(source)
    }

    pub fn prepare(
        &self,
        source: &Thread,
        expected: &ShellRecoveryProjection,
    ) -> Result<ShellRecoveryConfirmation, ShellRecoveryError> {
        let projection = self.read(source)?;
        require_same(&projection, expected)?;
        if projection.rows.is_empty() || projection.rows.iter().any(|row| !row.can_resume) {
            return Err(ShellRecoveryError::KnownProcessOrResource);
        }
        let store = Store::open_existing(&self.database_path).map_err(unavailable)?;
        let _coordination = crate::permission_contracts::coordinate().map_err(unavailable)?;
        let authority =
            vega_store::permission_contracts::load_state(store.conn()).map_err(unavailable)?;
        if authority.reset_pending {
            return Err(ShellRecoveryError::Changed);
        }
        Ok(ShellRecoveryConfirmation {
            source: source.clone(),
            projection,
            decision_id: ulid::Ulid::generate().to_string(),
            authority_epoch: authority.authority_epoch,
            draft: self.draft,
        })
    }

    pub fn resume(
        &self,
        confirmation: &ShellRecoveryConfirmation,
    ) -> Result<ShellRecoveryProjection, ShellRecoveryError> {
        let store = Store::open_existing(&self.database_path).map_err(unavailable)?;
        let project = vega_store::projects::find(store.conn(), &confirmation.source.project_id)
            .map_err(unavailable)?
            .ok_or(ShellRecoveryError::Changed)?;
        if self.draft != confirmation.draft
            || self.workspace(&store, &confirmation.source)? != confirmation.projection.workspace
        {
            return Err(ShellRecoveryError::Changed);
        }
        let operations: Vec<_> = confirmation
            .projection
            .rows
            .iter()
            .map(|row| row.operation_id.clone())
            .collect();
        let result = commit(
            &store,
            ResumeWrites {
                source: confirmation.source.clone(),
                draft: self.draft,
                authority_epoch: confirmation.authority_epoch.clone(),
                workspace: confirmation.projection.workspace.clone(),
                project_path: project.path,
                epoch: confirmation.projection.epoch.clone(),
                operations: operations.clone(),
                decision: confirmation.decision_id.clone(),
            },
        );
        if let Err(error) = result {
            if error == ShellRecoveryError::Changed {
                return Err(error);
            }
            store
                .verify_database_identity(store.database_identity())
                .map_err(unavailable)?;
            if !vega_store::shell_operations::verify_resume_decision(
                store.conn(),
                &confirmation.projection.workspace,
                &confirmation.projection.epoch,
                &operations,
                &confirmation.decision_id,
            )
            .map_err(unavailable)?
            {
                let current = self.read(&confirmation.source)?;
                require_same(&current, &confirmation.projection)?;
                return Err(error);
            }
        }
        vega_tools::ReadState::invalidate_workspace(std::path::Path::new(
            &confirmation.projection.workspace,
        ))
        .map_err(unavailable)?;
        for operation in &operations {
            vega_runtime::release_shell_recovery(operation).map_err(unavailable)?;
            vega_runtime::RuntimeWorkspaceMutationGuard::set_persisted_block(
                std::path::Path::new(&confirmation.projection.workspace),
                operation,
                false,
            )
            .map_err(unavailable)?;
        }
        self.read(&confirmation.source)
    }

    pub async fn retry(
        &self,
        source: &Thread,
        expected: &ShellRecoveryProjection,
    ) -> Result<ShellRecoveryProjection, ShellRecoveryError> {
        let projection = self.read(source)?;
        require_same(&projection, expected)?;
        for row in &projection.rows {
            if !row.can_retry {
                continue;
            }
            let snapshot = vega_runtime::retry_shell_cleanup(&row.operation_id)
                .await
                .map_err(unavailable)?;
            let store = Store::open_existing(&self.database_path).map_err(unavailable)?;
            let facts = vega_store::shell_operations::ShellFacts {
                command_result: snapshot.command_result,
                exit_signal: snapshot.exit_signal,
                cleanup_state: if snapshot.cleanup_safe {
                    "best_effort_completed"
                } else {
                    "unresolved"
                }
                .into(),
                output_incomplete: snapshot.output_incomplete,
                resources_closed: snapshot.resources_closed,
                process_resolved: snapshot.process_resolved,
                cleanup_reasons: snapshot.cleanup_reasons,
            };
            commit(
                &store,
                CleanupObservation {
                    operation: row.operation_id.clone(),
                    observation: ulid::Ulid::generate().to_string(),
                    facts,
                },
            )?;
            if snapshot.cleanup_safe {
                vega_tools::ReadState::invalidate_workspace(std::path::Path::new(
                    &projection.workspace,
                ))
                .map_err(unavailable)?;
                vega_runtime::release_shell_recovery(&row.operation_id).map_err(unavailable)?;
                vega_runtime::RuntimeWorkspaceMutationGuard::set_persisted_block(
                    std::path::Path::new(&projection.workspace),
                    &row.operation_id,
                    false,
                )
                .map_err(unavailable)?;
            }
        }
        self.read(source)
    }
}

fn reconcile_commits(store: &Store, workspace: &str) -> Result<(), ShellRecoveryError> {
    for pending in
        vega_runtime::pending_shell_commits(std::path::Path::new(workspace)).map_err(unavailable)?
    {
        let complete = pending.cleanup_state == "best_effort_completed";
        commit(store, ShellRecord(pending.clone()))?;
        if complete {
            vega_tools::ReadState::invalidate_workspace(std::path::Path::new(&pending.workspace))
                .map_err(unavailable)?;
        }
        vega_runtime::confirm_shell_commit(&pending.operation_id, complete).map_err(unavailable)?;
        if complete {
            vega_runtime::RuntimeWorkspaceMutationGuard::set_persisted_block(
                std::path::Path::new(&pending.workspace),
                &pending.operation_id,
                false,
            )
            .map_err(unavailable)?;
        }
    }
    Ok(())
}

fn require_same(
    current: &ShellRecoveryProjection,
    expected: &ShellRecoveryProjection,
) -> Result<(), ShellRecoveryError> {
    if current.workspace != expected.workspace
        || current.epoch != expected.epoch
        || current
            .rows
            .iter()
            .map(|row| (&row.operation_id, row.can_resume, row.can_retry))
            .collect::<Vec<_>>()
            != expected
                .rows
                .iter()
                .map(|row| (&row.operation_id, row.can_resume, row.can_retry))
                .collect::<Vec<_>>()
    {
        Err(ShellRecoveryError::Changed)
    } else {
        Ok(())
    }
}

fn workspace(store: &Store, source: &Thread) -> Result<String, ShellRecoveryError> {
    let current = crate::threads::get_thread(store, &source.id).map_err(unavailable)?;
    if current.project_id != source.project_id
        || current.permission_revision != source.permission_revision
        || current.mode != source.mode
        || current.permission_mode != source.permission_mode
    {
        return Err(ShellRecoveryError::Changed);
    }
    project_workspace(store, &current.project_id)
}

fn project_workspace(store: &Store, project_id: &str) -> Result<String, ShellRecoveryError> {
    let project = vega_store::projects::find(store.conn(), project_id)
        .map_err(unavailable)?
        .ok_or(ShellRecoveryError::Changed)?;
    let path = std::fs::canonicalize(&project.path).map_err(unavailable)?;
    Ok(path.to_string_lossy().into_owned())
}

fn unavailable(_: impl std::fmt::Debug) -> ShellRecoveryError {
    ShellRecoveryError::Unavailable
}
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

fn commit<C: crate::database_writer::DatabaseCommand<Output = Result<(), ShellRecoveryError>>>(
    store: &Store,
    command: C,
) -> Result<(), ShellRecoveryError> {
    let path = store
        .database_path()
        .ok_or(ShellRecoveryError::Unavailable)?;
    crate::database_writer::commit(path, store.database_identity(), command).map_err(unavailable)?
}

struct ResumeWrites {
    source: Thread,
    draft: bool,
    authority_epoch: String,
    workspace: String,
    project_path: String,
    epoch: vega_store::shell_operations::ShellRecoveryVersion,
    operations: Vec<String>,
    decision: String,
}

impl crate::database_writer::Sealed for ResumeWrites {}
impl crate::database_writer::DatabaseCommand for ResumeWrites {
    type Output = Result<(), ShellRecoveryError>;

    fn apply(self, store: &Store) -> Self::Output {
        let _coordination = crate::permission_contracts::coordinate().map_err(unavailable)?;
        let tx = store.immediate_transaction().map_err(unavailable)?;
        let authority = vega_store::permission_contracts::load_state(&tx).map_err(unavailable)?;
        if authority.reset_pending || authority.authority_epoch != self.authority_epoch {
            return Err(ShellRecoveryError::Changed);
        }
        let current = vega_store::threads::find(&tx, &self.source.id).map_err(unavailable)?;
        if self.draft {
            if current.is_some()
                || self.source.backend != crate::types::TaskBackend::Native
                || self.source.status != crate::types::ThreadStatus::Active
                || self.source.parent_thread_id.is_some()
                || self.source.forked_from_thread_id.is_some()
                || self.source.forked_from_message_id.is_some()
                || !(0..i64::MAX).contains(&self.source.permission_revision)
                || ulid::Ulid::from_string(&self.source.id).is_err()
                || self
                    .source
                    .draft_authority_epoch
                    .as_ref()
                    .is_some_and(|epoch| epoch != &authority.authority_epoch)
            {
                return Err(ShellRecoveryError::Changed);
            }
        } else {
            let current =
                crate::threads::thread_from_row(&current.ok_or(ShellRecoveryError::Changed)?)
                    .map_err(unavailable)?;
            if current.project_id != self.source.project_id
                || current.permission_revision != self.source.permission_revision
                || current.mode != self.source.mode
                || current.permission_mode != self.source.permission_mode
            {
                return Err(ShellRecoveryError::Changed);
            }
        }
        let project = vega_store::projects::find(&tx, &self.source.project_id)
            .map_err(unavailable)?
            .ok_or(ShellRecoveryError::Changed)?;
        if project.path != self.project_path {
            return Err(ShellRecoveryError::Changed);
        }
        vega_store::shell_operations::resume_writes_in_transaction(
            &tx,
            &self.workspace,
            &self.epoch,
            &self.operations,
            &self.decision,
            now(),
        )
        .map_err(unavailable)?;
        tx.commit().map_err(unavailable)
    }
}

struct RecoverGeneration {
    generation: String,
}
impl crate::database_writer::Sealed for RecoverGeneration {}
impl crate::database_writer::DatabaseCommand for RecoverGeneration {
    type Output = Result<(), ShellRecoveryError>;
    fn apply(self, store: &Store) -> Self::Output {
        vega_store::shell_operations::recover_generation(store.conn(), &self.generation, now())
            .map(|_| ())
            .map_err(unavailable)
    }
}

struct CleanupObservation {
    operation: String,
    observation: String,
    facts: vega_store::shell_operations::ShellFacts,
}
impl crate::database_writer::Sealed for CleanupObservation {}
impl crate::database_writer::DatabaseCommand for CleanupObservation {
    type Output = Result<(), ShellRecoveryError>;
    fn apply(self, store: &Store) -> Self::Output {
        vega_store::shell_operations::append_cleanup_observation(
            store.conn(),
            &self.operation,
            &self.observation,
            &self.facts,
            now(),
        )
        .map_err(unavailable)
    }
}

struct ShellRecord(vega_runtime::RuntimeShellOperationRecord);
impl crate::database_writer::Sealed for ShellRecord {}
impl crate::database_writer::DatabaseCommand for ShellRecord {
    type Output = Result<(), ShellRecoveryError>;
    fn apply(self, store: &Store) -> Self::Output {
        let pending = self.0;
        let facts = vega_store::shell_operations::ShellFacts {
            command_result: pending.command_result.clone(),
            exit_signal: pending.exit_signal,
            cleanup_state: pending.cleanup_state.clone(),
            output_incomplete: pending.output_incomplete,
            resources_closed: pending.resources_closed,
            process_resolved: pending.process_resolved,
            cleanup_reasons: pending.cleanup_reasons.clone(),
        };
        let record = vega_store::shell_operations::ShellOperationRecord {
            operation_id: &pending.operation_id,
            thread_id: &pending.thread_id,
            run_id: &pending.run_id,
            call_id: &pending.call_id,
            generation: &pending.generation,
            workspace: &pending.workspace,
            state: &pending.state,
            exit_code: pending.exit_code,
            error: pending.error.as_deref(),
            duration_ms: pending.duration_ms,
        };
        let result =
            vega_store::shell_operations::record_with_facts(store.conn(), &record, &facts, now());
        if !vega_store::shell_operations::verify_record(store.conn(), &record, &facts)
            .map_err(unavailable)?
        {
            result.map_err(unavailable)?;
            return Err(ShellRecoveryError::Unavailable);
        }
        Ok(())
    }
}
