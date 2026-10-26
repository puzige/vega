use super::*;
use serde_json::{Value, json};
use vega_store::delegated_ownership::DelegatedOwnership;
use vega_store::delegated_stages as stages;

pub(super) struct StageHook {
    pub database: PathBuf,
    pub snapshot: SubagentSnapshot,
    pub owner: Arc<DelegatedOwnership>,
    pub admission: vega_store::thread_admission::ThreadAdmission,
    pub tools: vega_tools::Tools,
    pub config: vega_runtime::RuntimeToolConfig,
    pub permissions: PermissionQueue,
    pub registry: Registry,
}

impl vega_runtime::RuntimeSubagentHook for StageHook {
    fn call(
        &self,
        _: String,
        tool: String,
        mut input: Value,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<Value, VegaError>> {
        let database = self.database.clone();
        let snapshot = self.snapshot.clone();
        let owner = self.owner.clone();
        let admission = self.admission.clone();
        let tools = self.tools.clone();
        let config = self.config.clone();
        let permissions = self.permissions.clone();
        let registry = self.registry.clone();
        async move {
            let input_text = input.to_string();
            if tool != "checkpoint_stage"
                || !vega_runtime::valid_subagent_input(&tool, &input_text)
                || !owner.verifies(&database, &snapshot.agent_id)
            {
                return Err(failure("invalid stage declaration"));
            }
            let known = config
                .credential_reader()
                .map(|reader| reader())
                .transpose()
                .map_err(|_| VegaError::CredentialCheckUnavailable)?
                .unwrap_or_default();
            if vega_runtime::redact_sensitive_credential_text_with_policy(&input_text, &known, None)
                != input_text
            {
                return Err(VegaError::CredentialExposureBlocked);
            }
            let store = Store::open(&database)?;
            crate::thread_access::verify(&store, &snapshot.agent_id, &admission)
                .map_err(|_| failure("child admission identity changed"))?;
            let mut artifacts = Vec::new();
            for artifact in input["artifacts"]
                .as_array()
                .ok_or_else(|| failure("invalid artifacts"))?
            {
                let read = artifact["read_call_id"]
                    .as_str()
                    .ok_or_else(|| failure("invalid Read reference"))?;
                let path = artifact["path"]
                    .as_str()
                    .ok_or_else(|| failure("invalid artifact path"))?;
                let (canonical, hash) = stages::read_receipt(
                    store.conn(),
                    &snapshot.agent_id,
                    &snapshot.message_id,
                    read,
                )
                .map_err(|_| failure("complete Read evidence unavailable"))?;
                if tools
                    .file_path(path)
                    .map_err(|_| failure("artifact unavailable"))?
                    .to_string_lossy()
                    != canonical
                {
                    return Err(failure("artifact Read identity mismatch"));
                }
                let current = vega_runtime::read_identity_with_permission(
                    &tools,
                    &config,
                    &canonical,
                    &ArtifactReadPermissionAdapter {
                        shared: &permissions,
                    },
                    &cancel,
                )
                .await?;
                if current.sha256 != hash || current.canonical_path != canonical {
                    return Err(failure("artifact changed since Read"));
                }
                artifacts.push(json!({"path":canonical,"read_call_id":read,"sha256":hash}));
            }
            input["artifacts"] = Value::Array(artifacts);
            if cancel.is_cancelled() || !owner.verifies(&database, &snapshot.agent_id) {
                return Err(VegaError::Cancelled);
            }
            let tx = store.immediate_transaction()?;
            if stages::execution_authority(&tx, &snapshot.agent_id, &snapshot.message_id)?
                != stages::authority(&tx, &snapshot.agent_id)?
            {
                return Err(failure("stage authority changed"));
            }
            stages::evidence(&tx, &snapshot.agent_id, &input["evidence_call_ids"])?;
            let effects = stages::unknown_effects(&tx, &snapshot.agent_id, &snapshot.message_id)?;
            let checkpoint = stages::append(
                &tx,
                &snapshot.agent_id,
                &snapshot.message_id,
                "declared",
                input,
                effects,
                now_ms(),
            )?;
            if checkpoint.to_string().len() > 96 * 1024 {
                return Err(failure("checkpoint receipt too large"));
            }
            tx.commit()?;
            if let Ok(mut state) = registry.state.lock()
                && let Some(record) = state
                    .executions
                    .get_mut(&(snapshot.agent_id.clone(), snapshot.message_id.clone()))
            {
                record.snapshot.checkpoint = Some(checkpoint.clone());
            }
            Ok(json!({"checkpoint":checkpoint,"ready_for_review":false}))
        }
        .map(|result| {
            result.map_err(|error| match error {
                VegaError::Cancelled => VegaError::Cancelled,
                VegaError::Store(_) => failure("stage checkpoint persistence failed"),
                _ => failure("stage checkpoint verification failed"),
            })
        })
        .boxed()
    }
}

pub(super) fn tool_config(
    database: &std::path::Path,
    snapshot: &SubagentSnapshot,
    settings: &ChildSettings,
    project: String,
) -> vega_runtime::RuntimeToolConfig {
    let mut config = vega_runtime::RuntimeToolConfig::new(
        settings.authority.run_mode,
        settings.authority.permission_mode,
        project,
        snapshot.agent_id.clone(),
        database
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("checkpoints"),
        settings.authority.exact_rules.clone(),
    );
    if let Some(reader) = &settings.owner_credential_reader {
        config = config.with_credential_reader(reader.clone());
    }
    config
}

pub(super) fn verify_resume(
    store: &Store,
    snapshot: &SubagentSnapshot,
    previous: &ContinuationBoundary,
    input: &Value,
    authority: &DelegatedAuthority,
) -> Result<Value, VegaError> {
    let _permission_coordination = crate::permission_contracts::coordinate()
        .map_err(|_| failure("permission coordination unavailable"))?;
    let tx = store.immediate_transaction()?;
    let state = vega_store::permission_contracts::load_state(&tx)?;
    let parent = vega_store::permission_contracts::load_run_fence(
        &tx,
        &snapshot.parent_message_id,
        &snapshot.parent_thread_id,
    )?
    .ok_or_else(|| failure("parent permission snapshot unavailable"))?;
    let parent: crate::types::PermissionFence =
        serde_json::from_str(&parent).map_err(|_| failure("parent permission snapshot invalid"))?;
    let saved = vega_store::permission_contracts::interrupted_delegated_fence(
        &tx,
        &snapshot.agent_id,
        &previous.message_id,
        &snapshot.parent_thread_id,
    )?;
    let saved: crate::types::PermissionFence = serde_json::from_str(&saved)
        .map_err(|_| failure("interrupted permission snapshot invalid"))?;
    if state.reset_pending
        || !authority.matches_fence()
        || !parent.is_valid()
        || parent != authority.permission_fence
        || parent.authority_epoch != state.authority_epoch
        || parent.database_identity != store.database_identity().permission_fence_key()
        || parent.thread_id != snapshot.parent_thread_id
        || !saved.is_valid()
        || saved.thread_id != snapshot.agent_id
        || !saved.delegation.as_ref().is_some_and(|source| {
            source.child_message_id == previous.message_id
                && source.parent_fence.as_ref() == &parent
        })
    {
        return Err(failure("recovery permission source changed"));
    }
    let checkpoint = verify_resume_in_transaction(&tx, snapshot, previous, input)?;
    tx.commit()?;
    Ok(checkpoint)
}

pub(super) fn verify_resume_in_transaction(
    tx: &vega_store::Transaction<'_>,
    snapshot: &SubagentSnapshot,
    previous: &ContinuationBoundary,
    input: &Value,
) -> Result<Value, VegaError> {
    let source =
        vega_store::context_compaction::load_source_version_in_transaction(tx, &snapshot.agent_id)?;
    let checkpoint = stages::latest(tx, &snapshot.agent_id, &previous.message_id)?
        .ok_or_else(|| failure("checkpoint unavailable"))?;
    let saved = vega_store::permission_contracts::interrupted_delegated_fence(
        tx,
        &snapshot.agent_id,
        &previous.message_id,
        &snapshot.parent_thread_id,
    )?;
    let saved: crate::types::PermissionFence = serde_json::from_str(&saved)
        .map_err(|_| failure("interrupted permission snapshot invalid"))?;
    let checkpoint_fence: crate::types::PermissionFence =
        serde_json::from_value(checkpoint["permission_fence"].clone())
            .map_err(|_| failure("checkpoint permission snapshot unavailable"))?;
    if !saved.is_valid()
        || saved.thread_id != snapshot.agent_id
        || !saved.delegation.as_ref().is_some_and(|source| {
            source.child_message_id == previous.message_id
                && source.parent_fence.thread_id == snapshot.parent_thread_id
        })
        || checkpoint_fence != saved
    {
        return Err(failure("checkpoint permission snapshot changed"));
    }
    if input["checkpoint_id"] != checkpoint["id"]
        || input["source_version"] != checkpoint["source_version"]
        || input["source_fingerprint"] != checkpoint["source_fingerprint"]
        || checkpoint["source_version"].as_u64() != Some(source.source_version)
        || checkpoint["source_fingerprint"].as_str() != Some(source.fingerprint.as_str())
        || stages::execution_authority(tx, &snapshot.agent_id, &previous.message_id)?
            != stages::authority(tx, &snapshot.agent_id)?
        || !matches!(
            stages::release_state(tx, &snapshot.agent_id, &previous.message_id)?.as_str(),
            "released" | "recovered_abandoned"
        )
        || vega_store::pending_messages::count(tx, &snapshot.agent_id)? != 0
    {
        return Err(failure("stale recovery boundary"));
    }
    let effects = checkpoint["unknown_effects"]
        .as_array()
        .ok_or_else(|| failure("unknown effects unavailable"))?;
    let resolutions = input["reconciliations"]
        .as_array()
        .ok_or_else(|| failure("invalid reconciliations"))?;
    if effects.len() != resolutions.len() {
        return Err(failure("external effects require explicit reconciliation"));
    }
    let mut seen = std::collections::HashSet::new();
    for resolution in resolutions {
        let id = resolution["call_id"]
            .as_str()
            .ok_or_else(|| failure("invalid reconciliation"))?;
        if !seen.insert(id) || !effects.iter().any(|effect| effect["call_id"] == id) {
            return Err(failure("unmatched reconciliation"));
        }
        stages::evidence(tx, &snapshot.agent_id, &resolution["evidence_call_ids"])?;
    }
    Ok(checkpoint)
}

pub(super) async fn validate_artifacts(
    tools: &vega_tools::Tools,
    config: &vega_runtime::RuntimeToolConfig,
    permissions: &PermissionQueue,
    checkpoint: &Value,
    cancel: &CancellationToken,
) -> Result<(), VegaError> {
    for artifact in checkpoint["manifest"]["artifacts"]
        .as_array()
        .ok_or_else(|| failure("invalid artifacts"))?
    {
        let path = artifact["path"]
            .as_str()
            .ok_or_else(|| failure("invalid artifact path"))?;
        let actual = vega_runtime::read_identity_with_permission(
            tools,
            config,
            path,
            &ArtifactReadPermissionAdapter {
                shared: permissions,
            },
            cancel,
        )
        .await?;
        if artifact["sha256"] != actual.sha256 || artifact["path"] != actual.canonical_path {
            return Err(failure(
                "artifact changed; explicit reconciliation required",
            ));
        }
    }
    Ok(())
}

pub(super) fn refresh(store: &Store, snapshot: &mut SubagentSnapshot) -> Result<(), VegaError> {
    snapshot.checkpoint = stages::latest(store.conn(), &snapshot.agent_id, &snapshot.message_id)?;
    snapshot.release_state =
        stages::release_state(store.conn(), &snapshot.agent_id, &snapshot.message_id)?;
    snapshot.ready_for_review = snapshot.release_state == "released"
        && snapshot.state == SubagentState::Completed
        && stages::handoff(store.conn(), &snapshot.agent_id, &snapshot.message_id)?.is_some()
        && snapshot
            .checkpoint
            .as_ref()
            .is_some_and(|value| value["manifest"]["ready_for_review"] == true);
    Ok(())
}

pub(super) fn load(
    registry: &Registry,
    database: &std::path::Path,
    parent: &str,
) -> Result<(), VegaError> {
    let store = Store::open(database)?;
    let rows = vega_store::subagents::list(store.conn(), parent, None)?;
    for row in rows {
        let mut row = row;
        if (matches!(row.state.as_str(), "running" | "cancelling")
            || (matches!(row.state.as_str(), "interrupted" | "completed" | "failed")
                && stages::release_state(
                    store.conn(),
                    &row.child_thread_id,
                    &row.child_message_id,
                )? == "unknown"))
            && vega_store::recovery::recover_thread(store.conn(), &row.child_thread_id, now_ms())
                .is_ok()
        {
            row = vega_store::subagents::find(store.conn(), &row.child_thread_id)?
                .ok_or_else(|| failure("child unavailable"))?;
        }
        let mut snapshot = SubagentSnapshot {
            agent_id: row.child_thread_id,
            parent_thread_id: row.parent_thread_id,
            parent_message_id: row.parent_message_id,
            message_id: row.child_message_id,
            name: row.name,
            state: SubagentState::parse(&row.state)
                .ok_or_else(|| failure("invalid child state"))?,
            result: row.result,
            truncated: row.truncated,
            checkpoint: None,
            release_state: String::new(),
            ready_for_review: false,
        };
        refresh(&store, &mut snapshot)?;
        let occupied = !matches!(
            snapshot.release_state.as_str(),
            "released" | "recovered_abandoned"
        );
        let source = vega_store::context_compaction::load_source(store.conn(), &snapshot.agent_id)?;
        let thread = vega_store::threads::find(store.conn(), &snapshot.agent_id)?
            .ok_or_else(|| failure("child unavailable"))?;
        let boundary = (!occupied).then(|| ContinuationBoundary {
            message_id: snapshot.message_id.clone(),
            source_version: source.source_version,
            fingerprint: source.fingerprint,
            thread,
        });
        let mut state = registry
            .state
            .lock()
            .map_err(|_| failure("state unavailable"))?;
        if snapshot.release_state == "quarantined" {
            state.admission_failure = Some("Subagent ownership remains unknown".into());
        }
        state
            .latest
            .insert(snapshot.agent_id.clone(), snapshot.message_id.clone());
        state.executions.insert(
            (snapshot.agent_id.clone(), snapshot.message_id.clone()),
            AgentRecord {
                snapshot,
                call_id: row.spawn_call_id,
                cancel: CancellationToken::new(),
                receipt_persisted: CancellationToken::new(),
                writer: None,
                occupied,
                admitted: true,
                committed: true,
                boundary,
                activity: super::super::stage_activity::StageActivity::new(
                    registry.changed.clone(),
                ),
                delivered_attention: 0,
                bounded_observed: false,
                terminal_delivered: false,
            },
        );
    }
    Ok(())
}
