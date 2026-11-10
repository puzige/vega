use super::*;
use rusqlite::{Transaction, types::Value};
use std::collections::BTreeMap;

const MAX_ROWS: usize = 400_000;
const MAX_BYTES: usize = 192 * 1024 * 1024;
const SCOPE: &str = "WITH privacy_cleanup_threads AS (SELECT ?1 AS id UNION SELECT id FROM threads WHERE parent_thread_id=?1 UNION SELECT parent_thread_id FROM delegated_executions WHERE parent_thread_id=?1 OR child_thread_id=?1 UNION SELECT child_thread_id FROM delegated_executions WHERE parent_thread_id=?1 OR child_thread_id=?1)";

type StoredRow = Vec<Value>;
type StoredRows = BTreeMap<i64, StoredRow>;

#[derive(Clone, Copy)]
enum CleanupTable {
    Messages,
    ToolCalls,
    PendingMessages,
    HistoricalMessages,
    HistoricalTools,
    DelegatedExecutions,
    DelegatedRuns,
    StageCheckpoints,
    ExecutionRecovery,
    AssistantResponses,
    GeneratedNotices,
    TimestampBoundaries,
    ResponseStarts,
    ContextCheckpoints,
    PrecomputeAccounting,
    PrecomputeReady,
    PrecomputeSlots,
    Continuations,
}

impl CleanupTable {
    fn predicate(self) -> &'static str {
        match self {
            Self::Messages
            | Self::ToolCalls
            | Self::PendingMessages
            | Self::HistoricalMessages
            | Self::HistoricalTools
            | Self::Continuations => "thread_id=?1",
            Self::DelegatedExecutions => "parent_thread_id=?1 OR child_thread_id=?1",
            Self::DelegatedRuns => {
                "child_thread_id IN (SELECT id FROM privacy_cleanup_threads) OR parent_thread_id IN (SELECT id FROM privacy_cleanup_threads)"
            }
            Self::StageCheckpoints | Self::ExecutionRecovery => {
                "child_thread_id=?1 OR child_thread_id IN (SELECT id FROM threads WHERE parent_thread_id=?1)"
            }
            Self::AssistantResponses
            | Self::GeneratedNotices
            | Self::TimestampBoundaries
            | Self::ResponseStarts => "message_id IN (SELECT id FROM messages WHERE thread_id=?1)",
            Self::ContextCheckpoints
            | Self::PrecomputeAccounting
            | Self::PrecomputeReady
            | Self::PrecomputeSlots => "thread_id IN (SELECT id FROM privacy_cleanup_threads)",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Messages => "messages",
            Self::ToolCalls => "tool_calls",
            Self::PendingMessages => "pending_messages",
            Self::HistoricalMessages => "historical_message_delivery",
            Self::HistoricalTools => "historical_tool_receipts",
            Self::DelegatedExecutions => "delegated_executions",
            Self::DelegatedRuns => "delegated_runs",
            Self::StageCheckpoints => "delegated_stage_checkpoints",
            Self::ExecutionRecovery => "delegated_execution_recovery",
            Self::AssistantResponses => "assistant_responses",
            Self::GeneratedNotices => "generated_notices",
            Self::TimestampBoundaries => "message_timestamp_boundaries",
            Self::ResponseStarts => "assistant_response_starts",
            Self::ContextCheckpoints => "context_checkpoints",
            Self::PrecomputeAccounting => "context_precompute_accounting",
            Self::PrecomputeReady => "context_precompute_ready",
            Self::PrecomputeSlots => "context_precompute_slots",
            Self::Continuations => "credential_continuations",
        }
    }
}

const MUTABLE_TABLES: &[CleanupTable] = &[
    CleanupTable::AssistantResponses,
    CleanupTable::GeneratedNotices,
    CleanupTable::ToolCalls,
    CleanupTable::TimestampBoundaries,
    CleanupTable::ResponseStarts,
    CleanupTable::Messages,
    CleanupTable::PendingMessages,
    CleanupTable::HistoricalMessages,
    CleanupTable::HistoricalTools,
    CleanupTable::StageCheckpoints,
    CleanupTable::ExecutionRecovery,
    CleanupTable::DelegatedExecutions,
    CleanupTable::DelegatedRuns,
    CleanupTable::ContextCheckpoints,
    CleanupTable::PrecomputeAccounting,
    CleanupTable::PrecomputeReady,
    CleanupTable::PrecomputeSlots,
    CleanupTable::Continuations,
];

struct PreparedRowChange {
    rowid: i64,
    original: Option<StoredRow>,
    projected: Option<StoredRow>,
}

struct PreparedTableChanges {
    table: CleanupTable,
    rows: Vec<PreparedRowChange>,
}

pub struct PreparedCleanup {
    fence: crate::context_compaction::source_version::SourceFence,
    revisions: Vec<(String, i64)>,
    permission: crate::permission_contracts::ContractState,
    epoch: Option<String>,
    exceptions: Vec<(String, String, String)>,
    changes: Vec<PreparedTableChanges>,
}

fn conflict() -> rusqlite::Error {
    rusqlite::Error::InvalidParameterName("conversation changed; review again".into())
}

fn revisions(conn: &Connection) -> rusqlite::Result<Vec<(String, i64)>> {
    conn.prepare("SELECT t.id,r.revision FROM threads t LEFT JOIN preparation_revisions r ON r.thread_id=t.id ORDER BY t.id")?
        .query_map([], |row| {
            let revision: i64 = row.get(1)?;
            if revision < 0 {
                return Err(rusqlite::Error::InvalidQuery);
            }
            Ok((row.get(0)?, revision))
        })?
        .collect()
}

fn epoch(conn: &Connection) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT (SELECT digest FROM credential_privacy_epoch WHERE singleton=1)",
        [],
        |row| row.get(0),
    )
}

fn all_exceptions(conn: &Connection) -> rusqlite::Result<Vec<(String, String, String)>> {
    conn.prepare("SELECT thread_id,rule_id,fingerprint FROM credential_privacy_exceptions ORDER BY thread_id,rule_id,fingerprint")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect()
}

fn require_idle(conn: &Connection, thread: &str) -> rusqlite::Result<()> {
    let running: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM messages WHERE thread_id=?1 AND status='streaming') OR EXISTS(SELECT 1 FROM delegated_executions WHERE (parent_thread_id=?1 OR child_thread_id=?1) AND state IN ('running','cancelling'))",
        [thread], |row| row.get(0),
    )?;
    if running {
        return Err(conflict());
    }
    Ok(())
}

fn read_rows(conn: &Connection, table: &str) -> rusqlite::Result<StoredRows> {
    let mut statement = conn.prepare(&format!("SELECT rowid,* FROM {table} ORDER BY rowid"))?;
    let columns = statement.column_count();
    statement
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                (1..columns)
                    .map(|index| row.get(index))
                    .collect::<rusqlite::Result<Vec<Value>>>()?,
            ))
        })?
        .collect()
}

fn copy_table(
    source: &Connection,
    destination: &Connection,
    table: &str,
    predicate: &str,
    thread: &str,
    row_count: &mut usize,
    byte_count: &mut usize,
) -> rusqlite::Result<()> {
    let definition: String = source.query_row(
        "SELECT sql FROM sqlite_schema WHERE type='table' AND name=?1",
        [table],
        |row| row.get(0),
    )?;
    destination.execute_batch(&definition)?;
    let mut select = source.prepare(&format!(
        "{SCOPE} SELECT rowid,* FROM {table} WHERE {predicate} ORDER BY rowid"
    ))?;
    let columns = select.column_count();
    let mut names = select.column_names();
    names[0] = "rowid";
    let mut insert = destination.prepare(&format!(
        "INSERT INTO {table} ({}) VALUES ({})",
        names.join(","),
        std::iter::repeat_n("?", columns)
            .collect::<Vec<_>>()
            .join(","),
    ))?;
    let mut rows = select.query([thread])?;
    while let Some(row) = rows.next()? {
        *row_count = row_count
            .checked_add(1)
            .ok_or(rusqlite::Error::InvalidQuery)?;
        let mut values = Vec::with_capacity(columns);
        for column in 0..columns {
            let value = row.get_ref(column)?;
            let bytes = match value {
                rusqlite::types::ValueRef::Text(value) | rusqlite::types::ValueRef::Blob(value) => {
                    value.len()
                }
                _ => 8,
            };
            *byte_count = byte_count
                .checked_add(bytes)
                .ok_or(rusqlite::Error::InvalidQuery)?;
            if *row_count > MAX_ROWS || *byte_count > MAX_BYTES {
                return Err(rusqlite::Error::InvalidQuery);
            }
            values.push(row.get::<_, Value>(column)?);
        }
        insert.execute(rusqlite::params_from_iter(values))?;
    }
    Ok(())
}

pub fn prepare_clean<F>(
    conn: &Connection,
    thread: &str,
    expected: &str,
    redact: F,
) -> rusqlite::Result<PreparedCleanup>
where
    F: FnMut(&str, &[usize]) -> (String, Vec<usize>),
{
    let source = conn.unchecked_transaction()?;
    let fence = crate::context_compaction::source_version::SourceFence::capture(&source, thread)?;
    let revisions = revisions(&source)?;
    let permission = crate::permission_contracts::load_state(&source)?;
    if permission.reset_pending {
        return Err(conflict());
    }
    let epoch = epoch(&source)?;
    let exceptions = all_exceptions(&source)?;
    require_idle(&source, thread)?;
    if source_fingerprint_in_transaction(&source, thread)? != expected {
        return Err(conflict());
    }
    let snapshot = Connection::open_in_memory()?;
    snapshot.pragma_update(None, "temp_store", "MEMORY")?;
    snapshot.pragma_update(None, "foreign_keys", false)?;
    let mut row_count = 0usize;
    let mut byte_count = 0usize;
    for &table in MUTABLE_TABLES {
        copy_table(
            &source,
            &snapshot,
            table.name(),
            table.predicate(),
            thread,
            &mut row_count,
            &mut byte_count,
        )?;
    }
    for (table, predicate) in [
        ("threads", "id IN (SELECT id FROM privacy_cleanup_threads)"),
        (
            "image_attachments",
            "message_id IN (SELECT id FROM messages WHERE thread_id=?1)",
        ),
        (
            "computer_screenshot_artifacts",
            "message_id IN (SELECT id FROM messages WHERE thread_id=?1)",
        ),
        (
            "tool_call_rounds",
            "message_id IN (SELECT id FROM messages WHERE thread_id=?1)",
        ),
        ("context_micro_compactions", "thread_id=?1"),
        ("permission_contract_state", "?1 IS NOT NULL"),
        ("credential_privacy_epoch", "?1 IS NOT NULL"),
        ("credential_privacy_exceptions", "thread_id=?1"),
    ] {
        copy_table(
            &source,
            &snapshot,
            table,
            predicate,
            thread,
            &mut row_count,
            &mut byte_count,
        )?;
    }
    let original = MUTABLE_TABLES
        .iter()
        .map(|table| read_rows(&snapshot, table.name()))
        .collect::<rusqlite::Result<Vec<_>>>()?;
    source.commit()?;
    clean_snapshot(&snapshot, thread, expected, redact)?;
    let mut changes = Vec::new();
    for (&table, mut original) in MUTABLE_TABLES.iter().zip(original) {
        let mut rows = Vec::new();
        for (rowid, projected) in read_rows(&snapshot, table.name())? {
            let previous = original.remove(&rowid);
            if previous.as_ref() != Some(&projected) {
                rows.push(PreparedRowChange {
                    rowid,
                    original: previous,
                    projected: Some(projected),
                });
            }
        }
        rows.extend(
            original
                .into_iter()
                .map(|(rowid, original)| PreparedRowChange {
                    rowid,
                    original: Some(original),
                    projected: None,
                }),
        );
        if !rows.is_empty() {
            changes.push(PreparedTableChanges { table, rows });
        }
    }
    Ok(PreparedCleanup {
        fence,
        revisions,
        permission,
        epoch,
        exceptions,
        changes,
    })
}

impl PreparedCleanup {
    pub fn validate(&self, transaction: &Transaction<'_>) -> rusqlite::Result<()> {
        if !self.fence.validate(transaction)?
            || revisions(transaction)? != self.revisions
            || crate::permission_contracts::load_state(transaction)? != self.permission
            || epoch(transaction)? != self.epoch
            || all_exceptions(transaction)? != self.exceptions
        {
            return Err(conflict());
        }
        require_idle(transaction, self.fence.thread())
    }

    pub fn install(self, conn: &Connection) -> rusqlite::Result<()> {
        let mut transaction = crate::write_coordination::WriteTransaction::new(
            conn,
            crate::write_coordination::WriteCategory::Credentials,
        )?;
        self.install_in_transaction(&mut transaction)?;
        transaction.commit()
    }

    pub fn install_in_transaction(self, transaction: &mut Transaction<'_>) -> rusqlite::Result<()> {
        self.validate(transaction)?;
        let savepoint = transaction.savepoint()?;
        for table in self.changes {
            install_table(&savepoint, table)?;
        }
        savepoint.commit()
    }
}

fn install_table(transaction: &Connection, changes: PreparedTableChanges) -> rusqlite::Result<()> {
    let table = changes.table.name();
    let select = transaction.prepare(&format!("SELECT * FROM {table} LIMIT 0"))?;
    let columns = select
        .column_names()
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    drop(select);
    let predicate = format!(
        "rowid IS ? AND {}",
        columns
            .iter()
            .map(|name| format!("{name} IS ?"))
            .collect::<Vec<_>>()
            .join(" AND ")
    );
    let replace = matches!(changes.table, CleanupTable::GeneratedNotices);
    for row in &changes.rows {
        let Some(original) = &row.original else {
            continue;
        };
        if original.len() != columns.len() {
            return Err(rusqlite::Error::InvalidQuery);
        }
        if replace || row.projected.is_none() {
            let values = std::iter::once(Value::Integer(row.rowid)).chain(original.iter().cloned());
            if transaction.execute(
                &format!("DELETE FROM {table} WHERE {predicate}"),
                rusqlite::params_from_iter(values),
            )? != 1
            {
                return Err(conflict());
            }
        }
    }
    for row in changes.rows {
        let Some(projected) = row.projected else {
            continue;
        };
        if projected.len() != columns.len() {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let count = if !replace && let Some(original) = row.original {
            let changed = projected
                .iter()
                .zip(&original)
                .enumerate()
                .filter(|(_, (after, before))| after != before)
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            if changed.is_empty() {
                continue;
            }
            let assignments = changed
                .iter()
                .map(|index| format!("{}=?", columns[*index]))
                .collect::<Vec<_>>()
                .join(",");
            let values = changed
                .iter()
                .map(|index| projected[*index].clone())
                .chain(std::iter::once(Value::Integer(row.rowid)))
                .chain(original);
            transaction.execute(
                &format!("UPDATE {table} SET {assignments} WHERE {predicate}"),
                rusqlite::params_from_iter(values),
            )?
        } else {
            let values = std::iter::once(Value::Integer(row.rowid)).chain(projected);
            transaction.execute(
                &format!(
                    "INSERT INTO {table} (rowid,{}) VALUES ({})",
                    columns.join(","),
                    std::iter::repeat_n("?", columns.len() + 1)
                        .collect::<Vec<_>>()
                        .join(",")
                ),
                rusqlite::params_from_iter(values),
            )?
        };
        if count != 1 {
            return Err(conflict());
        }
    }
    Ok(())
}
