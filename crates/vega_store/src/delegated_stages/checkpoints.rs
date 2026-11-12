use super::*;
use crate::context_compaction::{
    ContextSourceVersion,
    source_version::{
        PreparedSourceVersion, SourceFence, normalize_source, project_recovery_source,
    },
};

pub struct PreparedStageCheckpoint {
    source: PreparedSourceVersion,
    message: String,
    effects: Value,
}

impl PreparedStageCheckpoint {
    pub fn prepare(conn: &Connection, child: &str, message: &str) -> rusqlite::Result<Self> {
        let tx = conn.unchecked_transaction()?;
        let value = Self {
            source: PreparedSourceVersion::capture_in_transaction(&tx, child)?,
            message: message.to_owned(),
            effects: unknown_effects(&tx, child, message)?,
        };
        tx.commit()?;
        Ok(value)
    }

    pub fn append(
        self,
        tx: &rusqlite::Transaction<'_>,
        child: &str,
        message: &str,
        manifest: Value,
        now: i64,
    ) -> rusqlite::Result<Value> {
        if self.message != message || !self.source.validate(tx, child)? {
            return Err(invalid());
        }
        append_with_source(
            tx,
            child,
            message,
            "declared",
            StageCheckpointPayload {
                manifest,
                effects: self.effects,
            },
            now,
            self.source.source(),
        )
    }
}

struct ContinuationDecision {
    message: String,
    reason: String,
    unchanged: bool,
    user: String,
    raw: String,
    permission: Option<String>,
    unknown: bool,
}

pub struct PreparedTerminal {
    fence: SourceFence,
    message: String,
    source: ContextSourceVersion,
    effects: Value,
    continuations: Vec<ContinuationDecision>,
    now: i64,
    recover: bool,
}

impl PreparedTerminal {
    pub fn prepare(
        conn: &Connection,
        child: &str,
        message: &str,
        recover: bool,
        now: i64,
    ) -> rusqlite::Result<Self> {
        let tx = conn.unchecked_transaction()?;
        let value = Self::prepare_in_transaction(&tx, child, message, recover, now)?;
        tx.commit()?;
        Ok(value)
    }

    fn prepare_in_transaction(
        tx: &rusqlite::Transaction<'_>,
        child: &str,
        message: &str,
        recover: bool,
        now: i64,
    ) -> rusqlite::Result<Self> {
        let fence = SourceFence::capture(tx, child)?;
        let effects = unknown_effects(tx, child, message)?;
        let source = if recover {
            project_recovery_source(tx, child, now)?
        } else {
            crate::context_compaction::load_source_version_in_transaction(tx, child)?
        };
        let mut continuations = Vec::new();
        if recover {
            let mut query = tx.prepare("SELECT c.continuation_message_id,c.reason,c.source_version,c.source_fingerprint,c.user_message_id,c.raw_user_fingerprint FROM credential_continuations c JOIN messages m ON m.id=c.continuation_message_id WHERE c.thread_id=?1 AND c.state='claimed' AND m.status='streaming' AND c.user_message_id=(SELECT id FROM messages WHERE thread_id=?1 AND role='user' ORDER BY seq DESC LIMIT 1)")?;
            let rows = query
                .query_map([child], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for (message, reason, version, fingerprint, user, raw) in rows {
                let current = crate::context_compaction::source_version_without_continuation(
                    tx, child, &message,
                )?;
                let current_raw = crate::credential_continuation::raw_user_fingerprint(tx, &user)?;
                let permission: Option<String> = tx.query_row("SELECT permission_fence FROM run_permission_fences WHERE message_id=?1 AND thread_id=?2", params![message,child], |row| row.get(0)).optional()?;
                let unchanged = u64::try_from(version).map_err(|_| invalid())?
                    == current.source_version
                    && current.fingerprint == fingerprint
                    && current_raw == raw;
                let unknown = permission.is_none()
                    || tx.query_row("SELECT EXISTS(SELECT 1 FROM tool_calls WHERE thread_id=?1 AND message_id IN (SELECT id FROM messages WHERE thread_id=?1 AND seq>(SELECT seq FROM messages WHERE id=?2)) AND status IN ('approved','running','cancelled'))", params![child,user], |row| row.get::<_,bool>(0))?;
                continuations.push(ContinuationDecision {
                    message,
                    reason,
                    unchanged,
                    user,
                    raw: current_raw,
                    permission,
                    unknown,
                });
            }
        }
        Ok(Self {
            fence,
            message: message.to_owned(),
            source,
            effects,
            continuations,
            now,
            recover,
        })
    }

    pub fn validate(
        &self,
        tx: &rusqlite::Transaction<'_>,
        child: &str,
        message: &str,
    ) -> rusqlite::Result<()> {
        if self.fence.thread() != child || self.message != message || !self.fence.validate(tx)? {
            return Err(invalid());
        }
        Ok(())
    }

    pub fn recover(
        &self,
        tx: &rusqlite::Transaction<'_>,
        child: &str,
        owner: Option<&crate::delegated_ownership::DelegatedOwnership>,
    ) -> rusqlite::Result<()> {
        if !self.recover || self.fence.thread() != child {
            return Err(invalid());
        }
        if crate::threads::find(tx, child)?.is_some_and(|row| row.parent_thread_id.is_some())
            && !owner.is_some_and(|owner| {
                crate::delegated_ownership::database_path(tx)
                    .is_ok_and(|path| owner.verifies(&path, child))
            })
        {
            return Err(invalid());
        }
        crate::subagents::recover(tx, child)?;
        crate::shell_operations::recover_thread(tx, child, self.now)?;
        normalize_source(tx, child, self.now)?;
        for value in &self.continuations {
            tx.execute("INSERT INTO credential_continuations(id,thread_id,user_message_id,source_version,source_fingerprint,state,reason,created_at,raw_user_fingerprint,permission_fence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) ON CONFLICT(id) DO NOTHING", params![value.message,child,value.user,self.source.source_version as i64,self.source.fingerprint,if value.unknown {"unknown"} else {"ready"},value.reason,self.now,value.raw,value.permission])?;
            if !value.unchanged {
                tx.execute("UPDATE credential_continuations SET state='unknown',reason='source_changed' WHERE id=?1", [&value.message])?;
            }
            crate::credential_continuation::complete(tx, &value.message)?;
        }
        Ok(())
    }

    pub fn final_checkpoint(
        &self,
        tx: &rusqlite::Transaction<'_>,
        child: &str,
        message: &str,
        interrupted: bool,
    ) -> rusqlite::Result<Value> {
        let previous = latest(tx, child, message)?;
        let manifest = previous.as_ref().map(|value| value["manifest"].clone()).unwrap_or_else(|| json!({"stage":null,"summary":null,"artifacts":[],"evidence_call_ids":[],"unresolved":["Interrupted without a stage declaration"],"next_step":null,"ready_for_review":false}));
        append_with_source(
            tx,
            child,
            message,
            if interrupted {
                "interrupted"
            } else {
                "declared"
            },
            StageCheckpointPayload {
                manifest,
                effects: self.effects.clone(),
            },
            self.now,
            &self.source,
        )
    }

    pub fn source(&self) -> &ContextSourceVersion {
        &self.source
    }
}

pub struct PreparedAbandonedRecovery {
    terminal: PreparedTerminal,
    execution: Option<crate::subagents::DelegationRow>,
}

impl PreparedAbandonedRecovery {
    pub fn prepare(conn: &Connection, child: &str, now: i64) -> rusqlite::Result<Self> {
        let tx = conn.unchecked_transaction()?;
        let execution = crate::subagents::find(&tx, child)?;
        let message = execution
            .as_ref()
            .map_or("", |row| row.child_message_id.as_str());
        let terminal = PreparedTerminal::prepare_in_transaction(&tx, child, message, true, now)?;
        tx.commit()?;
        Ok(Self {
            terminal,
            execution,
        })
    }

    pub fn install(
        &self,
        tx: &rusqlite::Transaction<'_>,
        child: &str,
        owner: Option<&crate::delegated_ownership::DelegatedOwnership>,
    ) -> rusqlite::Result<()> {
        let message = self
            .execution
            .as_ref()
            .map_or("", |row| row.child_message_id.as_str());
        self.terminal.validate(tx, child, message)?;
        if self.execution.is_some() && release_state(tx, child, message)? == "quarantined" {
            return Err(invalid());
        }
        let abandoned = self.execution.as_ref().filter(|row| {
            matches!(row.state.as_str(), "running" | "cancelling")
                || (matches!(row.state.as_str(), "interrupted" | "completed" | "failed")
                    && release_state(tx, child, message).is_ok_and(|state| state == "unknown"))
        });
        self.terminal.recover(tx, child, owner)?;
        if let Some(row) = abandoned {
            if release_state(tx, child, message)? == "unknown" {
                register(tx, child, message, None, &json!([]))?;
            }
            if !matches!(row.state.as_str(), "completed" | "failed") {
                self.terminal.final_checkpoint(tx, child, message, true)?;
            }
            set_release(tx, child, message, "recovered_abandoned")?;
        }
        Ok(())
    }
}
