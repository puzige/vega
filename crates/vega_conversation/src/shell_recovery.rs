use std::path::PathBuf;

use crate::types::Thread;
use vega_store::Store;

#[derive(Clone, Debug)]
pub struct ShellRecoveryRow {
    pub operation_id: String,
    pub thread_id: String,
    pub reason: String,
    pub exit_code: Option<i32>,
    pub output_incomplete: bool,
    pub can_retry: bool,
    pub can_resume: bool,
}

#[derive(Clone, Debug)]
pub struct ShellRecoveryProjection {
    pub workspace: String,
    pub epoch: i64,
    pub rows: Vec<ShellRecoveryRow>,
    pub active: Vec<vega_runtime::RuntimeActiveShell>,
}

#[derive(Clone, Debug)]
pub struct ShellRecoveryConfirmation {
    pub source: Thread,
    pub projection: ShellRecoveryProjection,
    pub decision_id: String,
    authority_epoch: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellRecoveryError {
    Unavailable,
    Changed,
    KnownProcessOrResource,
}

pub struct ShellRecoveryService {
    database_path: PathBuf,
}

impl ShellRecoveryService {
    pub fn new(database_path: PathBuf) -> Self {
        Self { database_path }
    }

    pub fn read(&self, source: &Thread) -> Result<ShellRecoveryProjection, ShellRecoveryError> {
        let store = Store::open_existing(&self.database_path).map_err(unavailable)?;
        let workspace = workspace(&store, source)?;
        reconcile_commits(&store, &workspace)?;
        vega_store::shell_operations::recover_generation(
            store.conn(),
            vega_runtime::shell_generation(),
            now(),
        )
        .map_err(unavailable)?;
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
                    ShellRecoveryRow {
                        operation_id: item.operation_id,
                        thread_id: item.thread_id,
                        reason: item.error.unwrap_or_else(|| "cleanup_unresolved".into()),
                        exit_code: item.exit_code,
                        output_incomplete: item.facts.output_incomplete,
                        can_retry: !prior_generation,
                        can_resume,
                    }
                })
                .collect(),
        })
    }

    pub async fn cancel(
        &self,
        source: &Thread,
        operation_id: &str,
    ) -> Result<ShellRecoveryProjection, ShellRecoveryError> {
        let store = Store::open_existing(&self.database_path).map_err(unavailable)?;
        let workspace = workspace(&store, source)?;
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
                vega_store::shell_operations::append_cleanup_observation(
                    store.conn(),
                    &item.operation_id,
                    &ulid::Ulid::generate().to_string(),
                    &facts,
                    now(),
                )
                .map_err(unavailable)?;
            }
        }
        self.read(source)
    }

    pub fn prepare(
        &self,
        source: &Thread,
        expected: &ShellRecoveryProjection,
    ) -> Result<ShellRecoveryConfirmation, ShellRecoveryError> {
        let _coordination = crate::permission_contracts::coordinate().map_err(unavailable)?;
        let projection = self.read(source)?;
        require_same(&projection, expected)?;
        if projection.rows.is_empty() || projection.rows.iter().any(|row| !row.can_resume) {
            return Err(ShellRecoveryError::KnownProcessOrResource);
        }
        let store = Store::open_existing(&self.database_path).map_err(unavailable)?;
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
        })
    }

    pub fn resume(
        &self,
        confirmation: &ShellRecoveryConfirmation,
    ) -> Result<ShellRecoveryProjection, ShellRecoveryError> {
        let _coordination = crate::permission_contracts::coordinate().map_err(unavailable)?;
        let store = Store::open_existing(&self.database_path).map_err(unavailable)?;
        if workspace(&store, &confirmation.source)? != confirmation.projection.workspace {
            return Err(ShellRecoveryError::Changed);
        }
        let authority =
            vega_store::permission_contracts::load_state(store.conn()).map_err(unavailable)?;
        if authority.reset_pending || authority.authority_epoch != confirmation.authority_epoch {
            return Err(ShellRecoveryError::Changed);
        }
        let operations: Vec<_> = confirmation
            .projection
            .rows
            .iter()
            .map(|row| row.operation_id.clone())
            .collect();
        vega_store::shell_operations::resume_writes(
            store.conn(),
            &confirmation.projection.workspace,
            confirmation.projection.epoch,
            &operations,
            &confirmation.decision_id,
            now(),
        )
        .map_err(|_| ShellRecoveryError::Changed)?;
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
            vega_store::shell_operations::append_cleanup_observation(
                store.conn(),
                &row.operation_id,
                &ulid::Ulid::generate().to_string(),
                &facts,
                now(),
            )
            .map_err(unavailable)?;
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
        if facts.cleanup_state == "best_effort_completed" {
            vega_tools::ReadState::invalidate_workspace(std::path::Path::new(&pending.workspace))
                .map_err(unavailable)?;
        }
        vega_runtime::confirm_shell_commit(
            &pending.operation_id,
            facts.cleanup_state == "best_effort_completed",
        )
        .map_err(unavailable)?;
        if facts.cleanup_state == "best_effort_completed" {
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
    let project = vega_store::projects::find(store.conn(), &current.project_id)
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
