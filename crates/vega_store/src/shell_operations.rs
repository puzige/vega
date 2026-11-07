use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;

#[cfg(test)]
#[path = "shell_operations_tests.rs"]
mod tests;

pub struct ShellOperationRecord<'a> {
    pub operation_id: &'a str,
    pub thread_id: &'a str,
    pub run_id: &'a str,
    pub call_id: &'a str,
    pub generation: &'a str,
    pub workspace: &'a str,
    pub state: &'a str,
    pub exit_code: Option<i32>,
    pub error: Option<&'a str>,
    pub duration_ms: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ShellFacts {
    pub command_result: Option<String>,
    pub exit_signal: Option<i32>,
    pub cleanup_state: String,
    pub output_incomplete: bool,
    pub resources_closed: bool,
    pub process_resolved: bool,
    pub cleanup_reasons: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct ShellRecoveryItem {
    pub operation_id: String,
    pub thread_id: String,
    pub workspace: String,
    pub generation: String,
    pub state: String,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
    pub facts: ShellFacts,
}

#[derive(Clone, Debug)]
pub struct ShellWorkspaceRecovery {
    pub epoch: i64,
    pub items: Vec<ShellRecoveryItem>,
}

fn terminal(state: &str) -> bool {
    matches!(
        state,
        "succeeded" | "failed" | "timed_out" | "cancelled" | "outcome_unknown"
    )
}

fn overlap(a: &str, b: &str) -> bool {
    Path::new(a).starts_with(b) || Path::new(b).starts_with(a)
}

fn epoch(conn: &Connection, workspace: &str) -> rusqlite::Result<i64> {
    Ok(conn
        .query_row(
            "SELECT epoch FROM shell_workspace_epochs WHERE workspace=?1",
            [workspace],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(0))
}

fn advance(conn: &Connection, workspace: &str) -> rusqlite::Result<i64> {
    conn.execute("INSERT INTO shell_workspace_epochs(workspace,epoch) VALUES(?1,1) ON CONFLICT(workspace) DO UPDATE SET epoch=epoch+1", [workspace])?;
    epoch(conn, workspace)
}

fn json(value: &impl serde::Serialize) -> rusqlite::Result<String> {
    serde_json::to_string(value).map_err(|_| rusqlite::Error::InvalidQuery)
}

fn record_content(
    record: &ShellOperationRecord<'_>,
    facts: &ShellFacts,
) -> rusqlite::Result<String> {
    json(
        &serde_json::json!({"operation_id":record.operation_id,"thread_id":record.thread_id,"run_id":record.run_id,"call_id":record.call_id,"generation":record.generation,"workspace":record.workspace,"state":record.state,"exit_code":record.exit_code,"error":record.error,"duration_ms":record.duration_ms,"facts":facts}),
    )
}

pub fn record(
    conn: &Connection,
    record: &ShellOperationRecord<'_>,
    now: i64,
) -> rusqlite::Result<()> {
    let completed = terminal(record.state) && record.state != "outcome_unknown";
    let facts = ShellFacts {
        command_result: completed.then(|| {
            if matches!(record.state, "cancelled" | "timed_out") {
                record.state.into()
            } else {
                "exited".into()
            }
        }),
        cleanup_state: if completed {
            "best_effort_completed"
        } else if record.state == "outcome_unknown" {
            "unresolved"
        } else {
            "pending"
        }
        .into(),
        resources_closed: completed,
        process_resolved: completed,
        ..ShellFacts::default()
    };
    record_with_facts(conn, record, &facts, now)
}

pub fn record_with_facts(
    conn: &Connection,
    record: &ShellOperationRecord<'_>,
    facts: &ShellFacts,
    now: i64,
) -> rusqlite::Result<()> {
    if !matches!(
        facts.cleanup_state.as_str(),
        "pending" | "best_effort_completed" | "unresolved"
    ) || facts.cleanup_reasons.len() > 256
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    if facts.cleanup_state == "best_effort_completed"
        && (!facts.resources_closed || !facts.process_resolved || facts.output_incomplete)
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let content = record_content(record, facts)?;
    let disposition = format!("{}:terminal", record.operation_id);
    let tx = crate::write_coordination::WriteTransaction::new(
        conn,
        crate::write_coordination::WriteCategory::ToolCalls,
    )?;
    let prior: Option<(String,String,String,String,String,String)> = tx.query_row("SELECT thread_id,run_id,call_id,generation,workspace,state FROM shell_operations WHERE operation_id=?1",[record.operation_id],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?))).optional()?;
    if let Some((thread, run, call, generation, workspace, state)) = prior {
        if (
            thread.as_str(),
            run.as_str(),
            call.as_str(),
            generation.as_str(),
            workspace.as_str(),
        ) != (
            record.thread_id,
            record.run_id,
            record.call_id,
            record.generation,
            record.workspace,
        ) {
            return Err(rusqlite::Error::InvalidQuery);
        }
        if terminal(&state) {
            let saved: Option<String> = tx
                .query_row(
                    "SELECT content FROM shell_dispositions WHERE disposition_id=?1",
                    [&disposition],
                    |row| row.get(0),
                )
                .optional()?;
            if saved.as_deref() != Some(content.as_str()) {
                return Err(rusqlite::Error::InvalidQuery);
            }
            return tx.commit();
        }
        let valid = match state.as_str() {
            "prepared" => matches!(
                record.state,
                "prepared" | "running" | "failed" | "cancelled" | "outcome_unknown"
            ),
            "running" => {
                record.state == "running"
                    || matches!(record.state, "finishing" | "stopping")
                    || terminal(record.state)
            }
            "finishing" | "stopping" => state == record.state || terminal(record.state),
            _ => false,
        };
        if !valid {
            return Err(rusqlite::Error::InvalidQuery);
        }
    } else {
        if record.state != "prepared"
            || record.exit_code.is_some()
            || record.error.is_some()
            || record.duration_ms.is_some()
        {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let inserted = tx.execute("INSERT INTO shell_operations(operation_id,thread_id,run_id,call_id,generation,workspace,state,created_at,updated_at) SELECT ?1,?2,?3,?4,?5,?6,'prepared',?7,?7 WHERE EXISTS(SELECT 1 FROM messages WHERE id=?3 AND thread_id=?2 AND role='assistant' AND status='streaming') AND EXISTS(SELECT 1 FROM tool_calls WHERE id=?4 AND thread_id=?2 AND message_id=?3 AND tool='bash' AND status='running')",params![record.operation_id,record.thread_id,record.run_id,record.call_id,record.generation,record.workspace,now])?;
        if inserted != 1 {
            return Err(rusqlite::Error::InvalidQuery);
        }
    }
    let duration = record
        .duration_ms
        .map(i64::try_from)
        .transpose()
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    tx.execute("UPDATE shell_operations SET state=?2,exit_code=?3,error=?4,duration_ms=?5,command_result=?6,exit_signal=?7,cleanup_state=?8,output_incomplete=?9,resources_closed=?10,process_resolved=?11,cleanup_reasons=?12,updated_at=?13 WHERE operation_id=?1",params![record.operation_id,record.state,record.exit_code,record.error,duration,facts.command_result,facts.exit_signal,facts.cleanup_state,facts.output_incomplete,facts.resources_closed,facts.process_resolved,json(&facts.cleanup_reasons)?,now])?;
    if terminal(record.state) {
        let before = epoch(&tx, record.workspace)?;
        let after = advance(&tx, record.workspace)?;
        if facts.cleanup_state == "best_effort_completed" {
            tx.execute(
                "DELETE FROM shell_blockers WHERE operation_id=?1",
                [record.operation_id],
            )?;
        } else {
            tx.execute("INSERT INTO shell_blockers(operation_id,workspace,epoch,reason,created_at) VALUES(?1,?2,?3,?4,?5)",params![record.operation_id,record.workspace,after,record.error.unwrap_or("cleanup_unresolved"),now])?;
        }
        tx.execute("INSERT INTO shell_dispositions(disposition_id,workspace,operation_ids,expected_epoch,committed_epoch,kind,content,created_at) VALUES(?1,?2,?3,?4,?5,'automatic',?6,?7)",params![disposition,record.workspace,json(&[record.operation_id])?,before,after,content,now])?;
    }
    tx.commit()
}

pub fn verify_record(
    conn: &Connection,
    record: &ShellOperationRecord<'_>,
    facts: &ShellFacts,
) -> rusqlite::Result<bool> {
    let content = record_content(record, facts)?;
    let saved: Option<String> = conn
        .query_row(
            "SELECT content FROM shell_dispositions WHERE disposition_id=?1",
            [format!("{}:terminal", record.operation_id)],
            |row| row.get(0),
        )
        .optional()?;
    Ok(saved.as_deref() == Some(content.as_str()))
}

pub fn list_blockers(conn: &Connection) -> rusqlite::Result<Vec<ShellRecoveryItem>> {
    let mut stmt = conn.prepare("SELECT o.operation_id,o.thread_id,o.workspace,o.generation,o.state,o.exit_code,o.error,o.command_result,o.exit_signal,o.cleanup_state,o.output_incomplete,o.resources_closed,o.process_resolved,o.cleanup_reasons FROM shell_operations o JOIN shell_blockers b ON b.operation_id=o.operation_id ORDER BY o.created_at,o.operation_id")?;
    let rows = stmt.query_map([], |row| {
        let reasons: String = row.get(13)?;
        let cleanup_reasons =
            serde_json::from_str(&reasons).map_err(|_| rusqlite::Error::InvalidQuery)?;
        Ok(ShellRecoveryItem {
            operation_id: row.get(0)?,
            thread_id: row.get(1)?,
            workspace: row.get(2)?,
            generation: row.get(3)?,
            state: row.get(4)?,
            exit_code: row.get(5)?,
            error: row.get(6)?,
            facts: ShellFacts {
                command_result: row.get(7)?,
                exit_signal: row.get(8)?,
                cleanup_state: row.get(9)?,
                output_incomplete: row.get(10)?,
                resources_closed: row.get(11)?,
                process_resolved: row.get(12)?,
                cleanup_reasons,
            },
        })
    })?;
    let mut items = Vec::new();
    for item in rows {
        let mut item = item?;
        {
            let latest: Option<String> = conn.query_row("SELECT facts FROM shell_cleanup_observations WHERE operation_id=?1 ORDER BY epoch DESC LIMIT 1",[&item.operation_id],|row| row.get(0)).optional()?;
            if let Some(latest) = latest {
                let mut facts: ShellFacts =
                    serde_json::from_str(&latest).map_err(|_| rusqlite::Error::InvalidQuery)?;
                facts.output_incomplete |= item.facts.output_incomplete;
                item.facts = facts;
            }
            items.push(item);
        }
    }
    Ok(items)
}

pub fn recovery(conn: &Connection, workspace: &str) -> rusqlite::Result<ShellWorkspaceRecovery> {
    let items = list_blockers(conn)?
        .into_iter()
        .filter(|item| overlap(&item.workspace, workspace))
        .collect();
    Ok(ShellWorkspaceRecovery {
        epoch: epoch(conn, workspace)?,
        items,
    })
}

pub fn has_unknown_workspace(conn: &Connection, workspace: &str) -> rusqlite::Result<bool> {
    Ok(!recovery(conn, workspace)?.items.is_empty())
}

fn recover_where(
    tx: &Connection,
    predicate: &str,
    value: &str,
    now: i64,
) -> rusqlite::Result<usize> {
    let pending: Vec<(String, String)> = {
        let mut stmt = tx.prepare(&format!("SELECT operation_id,workspace FROM shell_operations WHERE {predicate} AND state IN ('prepared','running','finishing','stopping')"))?;
        stmt.query_map([value], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    for (operation, workspace) in &pending {
        let next = advance(tx, workspace)?;
        tx.execute("UPDATE shell_operations SET state='outcome_unknown',command_result='outcome_unknown',cleanup_state='unresolved',cleanup_reasons='[\"control_continuity_lost\"]',error='session_expired',updated_at=?2 WHERE operation_id=?1",params![operation,now])?;
        tx.execute("INSERT INTO shell_blockers(operation_id,workspace,epoch,reason,created_at) VALUES(?1,?2,?3,'session_expired',?4)",params![operation,workspace,next,now])?;
    }
    Ok(pending.len())
}

pub fn recover_thread(conn: &Connection, thread: &str, now: i64) -> rusqlite::Result<usize> {
    recover_where(conn, "thread_id=?1", thread, now)
}

pub fn recover_generation(conn: &Connection, current: &str, now: i64) -> rusqlite::Result<usize> {
    let tx = crate::write_coordination::WriteTransaction::new(
        conn,
        crate::write_coordination::WriteCategory::ToolCalls,
    )?;
    let changed = recover_where(&tx, "generation<>?1", current, now)?;
    tx.commit()?;
    Ok(changed)
}

pub fn register_resource(
    conn: &Connection,
    operation: &str,
    identity: &str,
) -> rusqlite::Result<()> {
    let tx = crate::write_coordination::WriteTransaction::new(
        conn,
        crate::write_coordination::WriteCategory::ToolCalls,
    )?;
    let prior: Option<String> = tx
        .query_row(
            "SELECT identity FROM shell_resources WHERE operation_id=?1",
            [operation],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(prior) = prior {
        return if prior == identity {
            tx.commit()
        } else {
            Err(rusqlite::Error::InvalidQuery)
        };
    }
    if identity.len() > 4096 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let changed = tx.execute("INSERT INTO shell_resources(operation_id,identity) SELECT operation_id,?2 FROM shell_operations WHERE operation_id=?1 AND state='prepared'", params![operation,identity])?;
    if changed != 1 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    tx.commit()
}

pub fn resource(conn: &Connection, operation: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT identity FROM shell_resources WHERE operation_id=?1",
        [operation],
        |row| row.get(0),
    )
    .optional()
}

pub fn append_cleanup_observation(
    conn: &Connection,
    operation: &str,
    observation: &str,
    facts: &ShellFacts,
    now: i64,
) -> rusqlite::Result<()> {
    let tx = crate::write_coordination::WriteTransaction::new(
        conn,
        crate::write_coordination::WriteCategory::ToolCalls,
    )?;
    let content = json(facts)?;
    let prior: Option<(String, String)> = tx
        .query_row(
            "SELECT operation_id,facts FROM shell_cleanup_observations WHERE observation_id=?1",
            [observation],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    if let Some((prior_operation, prior)) = prior {
        return if prior_operation == operation && prior == content {
            tx.commit()
        } else {
            Err(rusqlite::Error::InvalidQuery)
        };
    }
    let workspace: String = tx.query_row(
        "SELECT workspace FROM shell_blockers WHERE operation_id=?1",
        [operation],
        |row| row.get(0),
    )?;
    let next = advance(&tx, &workspace)?;
    tx.execute("INSERT INTO shell_cleanup_observations(observation_id,operation_id,workspace,epoch,facts,created_at) VALUES(?1,?2,?3,?4,?5,?6)",params![observation,operation,workspace,next,content,now])?;
    if facts.cleanup_state == "best_effort_completed"
        && facts.resources_closed
        && facts.process_resolved
        && !facts.output_incomplete
    {
        tx.execute(
            "DELETE FROM shell_blockers WHERE operation_id=?1",
            [operation],
        )?;
    }
    tx.commit()
}

pub fn verify_resume_decision(
    conn: &Connection,
    workspace: &str,
    expected_epoch: i64,
    operations: &[String],
    decision: &str,
) -> rusqlite::Result<bool> {
    let mut selected = operations.to_vec();
    selected.sort();
    selected.dedup();
    if selected.is_empty() || selected.len() != operations.len() || decision.is_empty() {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let expected = json(
        &serde_json::json!({"workspace":workspace,"epoch":expected_epoch,"operations":selected,"kind":"user_risk"}),
    )?;
    let prior: Option<String> = conn
        .query_row(
            "SELECT content FROM shell_dispositions WHERE disposition_id=?1 AND kind='user_risk'",
            [decision],
            |row| row.get(0),
        )
        .optional()?;
    Ok(prior.as_deref() == Some(expected.as_str()))
}

pub fn resume_writes(
    conn: &Connection,
    workspace: &str,
    expected_epoch: i64,
    operations: &[String],
    decision: &str,
    now: i64,
) -> rusqlite::Result<()> {
    let mut selected = operations.to_vec();
    selected.sort();
    selected.dedup();
    if selected.is_empty() || selected.len() != operations.len() || decision.is_empty() {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let content = json(
        &serde_json::json!({"workspace":workspace,"epoch":expected_epoch,"operations":selected,"kind":"user_risk"}),
    )?;
    let tx = crate::write_coordination::WriteTransaction::new(
        conn,
        crate::write_coordination::WriteCategory::ToolCalls,
    )?;
    let prior: Option<String> = tx
        .query_row(
            "SELECT content FROM shell_dispositions WHERE disposition_id=?1",
            [decision],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(prior) = prior {
        return if prior == content {
            tx.commit()
        } else {
            Err(rusqlite::Error::InvalidQuery)
        };
    }
    if epoch(&tx, workspace)? != expected_epoch {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let current = recovery(&tx, workspace)?;
    for operation in &selected {
        let item = current
            .items
            .iter()
            .find(|item| &item.operation_id == operation && item.workspace == workspace)
            .ok_or(rusqlite::Error::InvalidQuery)?;
        if !item.facts.resources_closed
            || (!item.facts.process_resolved
                && !item
                    .facts
                    .cleanup_reasons
                    .iter()
                    .any(|reason| reason == "control_continuity_lost"))
        {
            return Err(rusqlite::Error::InvalidQuery);
        }
    }
    let next = advance(&tx, workspace)?;
    for operation in &selected {
        tx.execute(
            "DELETE FROM shell_blockers WHERE operation_id=?1 AND workspace=?2",
            params![operation, workspace],
        )?;
    }
    tx.execute("INSERT INTO shell_dispositions(disposition_id,workspace,operation_ids,expected_epoch,committed_epoch,kind,content,created_at) VALUES(?1,?2,?3,?4,?5,'user_risk',?6,?7)",params![decision,workspace,json(&selected)?,expected_epoch,next,content,now])?;
    tx.commit()
}
