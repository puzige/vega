use super::*;
use crate::database_writer::{DatabaseCommand, Sealed};

pub(super) fn commit<C: DatabaseCommand<Output = Result<T, VegaError>>, T: Send + 'static>(
    database: &std::path::Path,
    command: C,
) -> Result<T, VegaError> {
    let store = Store::open_existing(database).map_err(VegaError::Store)?;
    crate::database_writer::commit(database, store.database_identity(), command)
        .map_err(|_| failure("database commit outcome unavailable"))?
}

pub(super) struct CancelChild {
    pub database: PathBuf,
    pub child: String,
    pub run: String,
    pub writer: Option<(
        Arc<DelegatedOwnership>,
        vega_store::thread_admission::ThreadAdmission,
    )>,
}

impl Sealed for CancelChild {}
impl DatabaseCommand for CancelChild {
    type Output = Result<Option<vega_store::subagents::ExecutionRow>, VegaError>;

    fn apply(self, store: &Store) -> Self::Output {
        let Self {
            database,
            child,
            run,
            writer,
        } = self;
        let (owner, admission) = writer.ok_or_else(|| failure("child writer unavailable"))?;
        if !owner.verifies(&database, &child) {
            return Err(failure("native writer identity changed"));
        }

        crate::thread_access::verify(store, &child, &admission)
            .map_err(|_| failure("child admission identity changed"))?;
        let tx = store
            .immediate_transaction()
            .map_err(|_| failure("storage unavailable"))?;
        vega_store::subagents::request_cancel(&tx, &child, &run)
            .map_err(|_| failure("storage unavailable"))?;
        let execution = vega_store::subagents::execution(&tx, &child, &run)
            .map_err(|_| failure("storage unavailable"))?;
        tx.commit().map_err(|_| failure("storage unavailable"))?;
        Ok::<_, VegaError>(execution)
    }
}

pub(super) struct FinishChild {
    pub database: PathBuf,
    pub snapshot: SubagentSnapshot,
    pub usage_complete: bool,
    pub prepared: vega_store::delegated_stages::PreparedTerminal,
    pub owner: Arc<DelegatedOwnership>,
    pub admission: vega_store::thread_admission::ThreadAdmission,
}

impl Sealed for FinishChild {}
impl DatabaseCommand for FinishChild {
    type Output = Result<Option<ContinuationBoundary>, VegaError>;

    fn apply(self, store: &Store) -> Self::Output {
        let Self {
            database,
            snapshot,
            usage_complete,
            prepared,
            owner,
            admission,
        } = self;
        if !owner.verifies(&database, &snapshot.agent_id) {
            return Err(failure("native writer identity changed"));
        }
        crate::thread_access::verify(store, &snapshot.agent_id, &admission)
            .map_err(|_| failure("child admission identity changed"))?;
        let tx = store
            .immediate_transaction()
            .map_err(|_| failure("storage unavailable"))?;
        prepared.validate(&tx, &snapshot.agent_id, &snapshot.message_id)?;
        if !vega_store::subagents::finish(
            &tx,
            &snapshot.agent_id,
            &snapshot.message_id,
            snapshot.state.as_str(),
            &snapshot.result,
            snapshot.truncated,
        )
        .map_err(|_| failure("storage unavailable"))?
        {
            return Err(failure("delegated generation changed"));
        }
        vega_store::subagents::mark_usage_complete(
            &tx,
            &snapshot.agent_id,
            &snapshot.message_id,
            usage_complete,
        )
        .map_err(|_| failure("storage unavailable"))?;
        if snapshot.state != SubagentState::Completed {
            prepared.recover(&tx, &snapshot.agent_id, Some(&owner))?;
        }
        if snapshot.state != SubagentState::Completed
            || vega_store::delegated_stages::latest(&tx, &snapshot.agent_id, &snapshot.message_id)?
                .is_some()
        {
            prepared.final_checkpoint(
                &tx,
                &snapshot.agent_id,
                &snapshot.message_id,
                snapshot.state != SubagentState::Completed,
            )?;
        }
        let boundary = if matches!(
            snapshot.state,
            SubagentState::Completed | SubagentState::Interrupted
        ) && vega_store::pending_messages::count(&tx, &snapshot.agent_id)
            .map_err(|_| failure("storage unavailable"))?
            == 0
        {
            let thread = vega_store::threads::find(&tx, &snapshot.agent_id)
                .map_err(|_| failure("storage unavailable"))?
                .ok_or_else(|| failure("unknown_agent"))?;
            let source = prepared.source();
            Some(ContinuationBoundary {
                message_id: snapshot.message_id,
                source_version: source.source_version,
                fingerprint: source.fingerprint.clone(),
                thread,
            })
        } else {
            None
        };
        tx.commit().map_err(|_| failure("storage unavailable"))?;
        Ok(boundary)
    }
}

pub(super) struct ReleaseChild {
    pub database: PathBuf,
    pub snapshot: SubagentSnapshot,
    pub owner: Arc<DelegatedOwnership>,
    pub admission: vega_store::thread_admission::ThreadAdmission,
}

impl Sealed for ReleaseChild {}
impl DatabaseCommand for ReleaseChild {
    type Output = Result<(), VegaError>;

    fn apply(self, store: &Store) -> Self::Output {
        if !self.owner.verifies(&self.database, &self.snapshot.agent_id) {
            return Err(failure("native writer identity changed"));
        }
        crate::thread_access::verify(store, &self.snapshot.agent_id, &self.admission)
            .map_err(|_| failure("child admission identity changed"))?;
        vega_store::delegated_stages::set_release(
            store.conn(),
            &self.snapshot.agent_id,
            &self.snapshot.message_id,
            "released",
        )?;
        Ok(())
    }
}

pub(super) struct CreateChild {
    pub database: PathBuf,
    pub snapshot: SubagentSnapshot,
    pub task: String,
    pub call_id: String,
    pub model: String,
    pub authority: DelegatedAuthority,
    pub automation_claim: Option<crate::types::AutomationRunClaim>,
    pub cancel: CancellationToken,
    pub previous: Option<ContinuationBoundary>,
    pub source: Option<vega_store::context_compaction::source_version::PreparedSourceVersion>,
    pub resume: Option<serde_json::Value>,
    pub owner: Arc<DelegatedOwnership>,
    pub admission: vega_store::thread_admission::ThreadAdmission,
    pub prepared_workspace: PathBuf,
}
impl Sealed for CreateChild {}
impl DatabaseCommand for CreateChild {
    type Output = Result<(crate::types::Thread, String), VegaError>;
    fn apply(self, store: &Store) -> Self::Output {
        let Self {
            database,
            snapshot,
            task,
            call_id,
            model,
            authority,
            automation_claim,
            cancel,
            previous,
            source,
            resume,
            owner,
            admission,
            prepared_workspace,
        } = self;
        if cancel.is_cancelled() {
            return Err(VegaError::Cancelled);
        }
        if !owner.verifies(&database, &snapshot.agent_id) {
            return Err(failure("native writer identity changed"));
        }
        let _permission_coordination = crate::permission_contracts::coordinate()
            .map_err(|_| failure("permission coordination unavailable"))?;
        crate::permission_contracts::require_admission_open(store)
            .map_err(|_| failure("permission admission unavailable"))?;
        if !authority.matches_fence() {
            return Err(failure("delegated permission source changed"));
        }
        crate::thread_access::verify(store, &snapshot.agent_id, &admission)
            .map_err(|_| failure("child admission identity changed"))?;
        let tx = store
            .immediate_transaction()
            .map_err(|_| failure("storage unavailable"))?;
        if cancel.is_cancelled() {
            return Err(VegaError::Cancelled);
        }
        if let Some(claim) = &automation_claim {
            crate::automation::check_subagent_budget(&tx, claim)
                .map_err(|message| failure(&message))?;
        }
        let parent = vega_store::threads::find(&tx, &snapshot.parent_thread_id)
            .map_err(|_| failure("parent unavailable"))?
            .ok_or_else(|| failure("parent unavailable"))?;
        let parent =
            crate::threads::thread_from_row(&parent).map_err(|_| failure("parent unavailable"))?;
        if parent.backend != TaskBackend::Native || parent.parent_thread_id.is_some() {
            return Err(failure("nested delegation unavailable"));
        }
        let permission_mode = match authority.permission_mode {
            RuntimePermissionMode::ReadOnly => "readonly",
            RuntimePermissionMode::Confirm => "confirm",
            RuntimePermissionMode::Auto => "auto",
            RuntimePermissionMode::FullAccess => "full_access",
        };
        if let Some(previous) = &previous {
            let row = vega_store::threads::find(&tx, &snapshot.agent_id)
                .map_err(|_| failure("storage unavailable"))?
                .ok_or_else(|| failure("unknown_agent"))?;
            let prepared = source
                .as_ref()
                .ok_or_else(|| failure("source preparation unavailable"))?;
            if !prepared.validate(&tx, &snapshot.agent_id)? {
                return Err(failure("externally_continued"));
            }
            let source = prepared.source();
            if !same_context(&row, &previous.thread)
                || row.backend != "native"
                || row.mode != "execute"
                || row.permission_mode != permission_mode
                || row.model != model
                || row.parent_thread_id.as_deref() != Some(&snapshot.parent_thread_id)
                || source.source_version != previous.source_version
                || source.fingerprint != previous.fingerprint
                || vega_store::pending_messages::count(&tx, &snapshot.agent_id)
                    .map_err(|_| failure("storage unavailable"))?
                    != 0
            {
                return Err(failure("externally_continued"));
            }
            let latest = vega_store::subagents::find(&tx, &snapshot.agent_id)
                .map_err(|_| failure("storage unavailable"))?
                .ok_or_else(|| failure("unknown_agent"))?;
            if latest.child_message_id != previous.message_id {
                return Err(failure("stale_execution"));
            }
            if let Some(input) = &resume {
                stage_recovery::verify_resume_in_transaction(
                    &tx, &snapshot, previous, input, source,
                )?;
            }
            if latest.state
                != if resume.is_some() {
                    "interrupted"
                } else {
                    "completed"
                }
            {
                return Err(failure("agent state changed"));
            }
        } else {
            let mut draft =
                crate::threads::draft_thread(parent.project_binding(), &model, permission_mode);
            draft.reasoning_preferences = parent.reasoning_preferences.clone();
            draft.id = snapshot.agent_id.clone();
            crate::threads::materialize_draft_in_transaction(&tx, &draft)
                .map_err(|_| failure("admission_failed"))?;
            if vega_store::threads::rename(&tx, &snapshot.agent_id, &snapshot.name, now_ms())
                .map_err(|_| failure("admission_failed"))?
                != 1
            {
                return Err(failure("admission_failed"));
            }
        }
        let user_id = ulid::Ulid::generate().to_string();
        let seq = vega_store::messages::next_seq(&tx, &snapshot.agent_id)
            .map_err(|_| failure("admission_failed"))?;
        vega_store::messages::insert(
            &tx,
            &vega_store::messages::MessageRow {
                id: user_id.clone(),
                thread_id: snapshot.agent_id.clone(),
                seq,
                role: "user".into(),
                kind: "text".into(),
                content: task.clone(),
                status: "done".into(),
                created_at: now_ms(),
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .map_err(|_| failure("admission_failed"))?;
        let record = vega_store::subagents::DelegationRow {
            child_thread_id: snapshot.agent_id.clone(),
            parent_thread_id: parent.id,
            parent_message_id: snapshot.parent_message_id,
            spawn_call_id: call_id,
            child_message_id: snapshot.message_id,
            name: snapshot.name,
            task,
            state: "running".into(),
            result: String::new(),
            truncated: false,
            created_at: now_ms(),
        };
        if let Some(previous) = previous {
            if resume.is_some() {
                vega_store::subagents::append_resume(&tx, &record, &previous.message_id, &user_id)
            } else {
                vega_store::subagents::append(&tx, &record, &previous.message_id, &user_id)
            }
            .map_err(|_| failure("admission_failed"))?;
        } else {
            vega_store::subagents::insert(&tx, &record).map_err(|_| failure("admission_failed"))?;
        }
        vega_store::delegated_stages::register(
            &tx,
            &record.child_thread_id,
            &record.child_message_id,
            resume
                .as_ref()
                .and_then(|input| input["checkpoint_id"].as_str()),
            &resume
                .as_ref()
                .map(|input| input["reconciliations"].clone())
                .unwrap_or_else(|| serde_json::json!([])),
        )?;
        let row = vega_store::threads::find(&tx, &snapshot.agent_id)
            .map_err(|_| failure("admission_failed"))?
            .ok_or_else(|| failure("admission_failed"))?;
        let permission_fence = crate::permission_contracts::capture_delegated_fence_for_commit(
            store,
            &row,
            &record.parent_thread_id,
            &record.parent_message_id,
            &record.child_message_id,
            &authority.permission_fence,
        )
        .map_err(|_| failure("delegated permission source changed"))?;
        if prepared_workspace.to_str() != Some(permission_fence.workspace.as_str()) {
            return Err(failure("delegated workspace changed"));
        }
        let encoded = serde_json::to_string(&permission_fence)
            .map_err(|_| failure("delegated permission snapshot unavailable"))?;
        vega_store::permission_contracts::record_delegated_fence(
            &tx,
            &record.child_thread_id,
            &record.child_message_id,
            &encoded,
        )?;
        let child =
            crate::threads::thread_from_row(&row).map_err(|_| failure("admission_failed"))?;
        if cancel.is_cancelled() {
            return Err(VegaError::Cancelled);
        }
        tx.commit().map_err(|_| failure("admission_failed"))?;
        Ok((child, user_id))
    }
}
