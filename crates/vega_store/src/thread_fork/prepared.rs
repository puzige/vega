use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, Transaction, params, params_from_iter, types::Value};

use super::{ForkError, corrupt};
use crate::context_compaction::source_version::SourceFence;

macro_rules! fork_tables {
    ($( $variant:ident => ($table:literal, $columns:literal, $scope:literal) ),+ $(,)?) => {
        #[derive(Clone, Copy)]
        enum ForkTable { $( $variant ),+ }

        impl ForkTable {
            const ALL: &'static [Self] = &[$( Self::$variant ),+];
            fn table(self) -> &'static str { match self { $( Self::$variant => $table ),+ } }
            fn columns(self) -> &'static str { match self { $( Self::$variant => $columns ),+ } }
            fn scope(self) -> &'static str { match self { $( Self::$variant => $scope ),+ } }
        }
    };
}

fork_tables! {
    Prefix => ("thread_timestamp_fork_prefixes", "thread_id,prefix_seq", "thread_id=?1"),
    Message => ("messages", "id,thread_id,seq,role,kind,content,status,created_at,plan_status,plan_review_note,plan_reviewed_at", "thread_id=?1"),
    Call => ("tool_calls", "id,thread_id,message_id,seq,tool,input_json,output_text,output_full_path,status,approval,exit_code,duration_ms,created_at,finished_at,text_offset_bytes,description", "thread_id=?1"),
    Start => ("assistant_response_starts", "message_id,response_id,ordinal,text_start_bytes,started_at_ms", "message_id IN (SELECT id FROM messages WHERE thread_id=?1)"),
    Response => ("assistant_responses", "message_id,response_id,text_start_bytes,text_end_bytes,kind", "message_id IN (SELECT id FROM messages WHERE thread_id=?1)"),
    Round => ("tool_call_rounds", "message_id,call_id,response_id", "message_id IN (SELECT id FROM messages WHERE thread_id=?1)"),
    Notice => ("generated_notices", "message_id,text_start_bytes,text_end_bytes,kind,turn_limit,delivered_images", "message_id IN (SELECT id FROM messages WHERE thread_id=?1)"),
    Timestamp => ("message_timestamp_boundaries", "message_id,operation_key,text_start_bytes", "message_id IN (SELECT id FROM messages WHERE thread_id=?1)"),
    Image => ("image_attachments", "message_id,ordinal,encoded", "message_id IN (SELECT id FROM messages WHERE thread_id=?1)"),
    Duration => ("assistant_run_durations", "message_id,execution_duration_ms", "message_id IN (SELECT id FROM messages WHERE thread_id=?1)"),
    Builtin => ("builtin_skill_activations", "message_id,name,version,content_sha256", "message_id IN (SELECT id FROM messages WHERE thread_id=?1)"),
    Skill => ("skill_activation_audits", "run_id,thread_id,name,source_scope,content_sha256,origin,status,created_at", "thread_id=?1"),
    Delivery => ("historical_message_delivery", "message_id,thread_id,model_content,previous_assistant_id", "thread_id=?1"),
    Receipt => ("historical_tool_receipts", "call_id,thread_id,kind,source_thread_id,source_message_id,source_call_id,input_json,output_text,references_json", "thread_id=?1"),
    Screenshot => ("computer_screenshot_artifacts", "call_id,message_id,ordinal", "message_id IN (SELECT id FROM messages WHERE thread_id=?1)"),
    Origin => ("fork_mutation_origins", "call_id,project_id,source_thread_id,source_call_id", "call_id IN (SELECT id FROM tool_calls WHERE thread_id=?1)"),
}

struct ForkRows {
    table: ForkTable,
    rows: Vec<Vec<Value>>,
}

impl ForkRows {
    fn capture(conn: &Connection, table: ForkTable, thread: &str) -> rusqlite::Result<Self> {
        let mut statement = conn.prepare(&format!(
            "SELECT {} FROM {} WHERE {} ORDER BY rowid",
            table.columns(),
            table.table(),
            table.scope()
        ))?;
        let columns = statement.column_count();
        let rows = statement
            .query_map([thread], |row| {
                (0..columns).map(|index| row.get(index)).collect()
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(Self { table, rows })
    }

    fn matches(&self, conn: &Connection, thread: &str) -> rusqlite::Result<bool> {
        let mut statement = conn.prepare(&format!(
            "SELECT {} FROM {} WHERE {} ORDER BY rowid",
            self.table.columns(),
            self.table.table(),
            self.table.scope()
        ))?;
        let mut rows = statement.query([thread])?;
        for expected in &self.rows {
            let Some(row) = rows.next()? else {
                return Ok(false);
            };
            for (index, value) in expected.iter().enumerate() {
                if row.get_ref(index)? != rusqlite::types::ValueRef::from(value) {
                    return Ok(false);
                }
            }
        }
        Ok(rows.next()?.is_none())
    }

    fn install(&self, conn: &Connection) -> rusqlite::Result<()> {
        let placeholders = std::iter::repeat_n("?", self.table.columns().split(',').count())
            .collect::<Vec<_>>()
            .join(",");
        let mut insert = conn.prepare(&format!(
            "INSERT INTO {}({}) VALUES({placeholders})",
            self.table.table(),
            self.table.columns()
        ))?;
        for row in &self.rows {
            insert.execute(params_from_iter(row))?;
        }
        Ok(())
    }
}

pub struct ForkProjection {
    connection: Connection,
    fences: Vec<SourceFence>,
    source: crate::threads::ThreadRow,
    cutoff: String,
    origins: BTreeMap<String, crate::fork_mutations::MutationOrigin>,
    metadata: Vec<ForkRows>,
}

pub struct PreparedForkInstall {
    fences: Vec<SourceFence>,
    source: crate::threads::ThreadRow,
    row: crate::threads::ThreadRow,
    rows: Vec<ForkRows>,
    metadata: Vec<ForkRows>,
}

#[derive(Default)]
struct CopyBudget {
    rows: usize,
    bytes: usize,
}

impl CopyBudget {
    fn include(&mut self, values: &[Value]) -> Result<(), ForkError> {
        self.rows = self.rows.saturating_add(1);
        for value in values {
            self.bytes = self.bytes.saturating_add(match value {
                Value::Text(value) => value.len(),
                Value::Blob(value) => value.len(),
                _ => 8,
            });
        }
        if self.rows > 400_000 || self.bytes > 192 * 1024 * 1024 {
            return Err(corrupt("fork projection exceeds bounded read"));
        }
        Ok(())
    }
}

fn copy_table(
    source: &Connection,
    destination: &Connection,
    table: &str,
    predicate: &str,
    thread: &str,
    budget: &mut CopyBudget,
) -> Result<(), ForkError> {
    let definition: String = source.query_row(
        "SELECT sql FROM sqlite_schema WHERE type='table' AND name=?1",
        [table],
        |row| row.get(0),
    )?;
    destination.execute_batch(&definition)?;
    let mut select = source.prepare(&format!(
        "SELECT * FROM {table} WHERE {predicate} ORDER BY rowid"
    ))?;
    let placeholders = std::iter::repeat_n("?", select.column_count())
        .collect::<Vec<_>>()
        .join(",");
    let mut insert = destination.prepare(&format!("INSERT INTO {table} VALUES({placeholders})"))?;
    let columns = select.column_count();
    let mut rows = select.query([thread])?;
    while let Some(row) = rows.next()? {
        let values = (0..columns)
            .map(|index| row.get::<_, Value>(index))
            .collect::<rusqlite::Result<Vec<_>>>()?;
        budget.include(&values)?;
        insert.execute(params_from_iter(values))?;
    }
    let indexes = source
        .prepare(
            "SELECT sql FROM sqlite_schema WHERE type='index' AND tbl_name=?1 AND sql IS NOT NULL",
        )?
        .query_map([table], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for index in indexes {
        destination.execute_batch(&index)?;
    }
    Ok(())
}

impl ForkProjection {
    pub fn capture(
        conn: &Connection,
        thread: &str,
        cutoff: Option<&str>,
    ) -> Result<Self, ForkError> {
        let read = conn.unchecked_transaction()?;
        let source = crate::threads::find(&read, thread)?.ok_or(ForkError::MissingSource)?;
        if source.backend != "native"
            || source.parent_thread_id.is_some()
            || source.workspace_owner_thread_id.is_some()
            || source.project_id.is_empty()
            || !crate::projects::project_exists(&read, &source.project_id)?
        {
            return Err(ForkError::UnsupportedSource);
        }
        let cutoff = match cutoff {
            Some(id) => id.to_owned(),
            None => super::latest_fork_cutoff(&read, thread)?.ok_or(ForkError::InvalidCutoff)?,
        };
        crate::context_compaction::load_source_version_in_transaction(&read, thread)?;
        super::prefix_images(&read, thread, &cutoff)?;
        let orphan: bool = read.query_row(
            "SELECT EXISTS(SELECT 1 FROM tool_calls c LEFT JOIN messages m ON m.id=c.message_id WHERE (c.thread_id=?1 OR m.thread_id=?1) AND (m.id IS NULL OR c.thread_id<>m.thread_id))",
            [thread], |row| row.get(0),
        )?;
        if orphan {
            return Err(corrupt("invalid tool owner"));
        }
        let mut fences = Vec::new();
        let mut ancestor = Some(thread.to_owned());
        let mut visited = BTreeSet::new();
        while let Some(id) = ancestor {
            if !visited.insert(id.clone()) {
                return Err(corrupt("cyclic fork ancestry"));
            }
            fences.push(SourceFence::capture(&read, &id)?);
            ancestor = crate::threads::find(&read, &id)?
                .ok_or(ForkError::MissingSource)?
                .forked_from_thread_id;
        }
        let ids = read.prepare("SELECT id FROM tool_calls WHERE thread_id=?1 AND tool IN ('write','Write','edit','Edit')")?
            .query_map([thread], |row| row.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let mut origins = BTreeMap::new();
        for id in ids {
            origins.insert(
                id.clone(),
                crate::fork_mutations::origin(&read, thread, &id)?,
            );
        }
        let connection = Connection::open_in_memory()?;
        connection.pragma_update(None, "temp_store", "MEMORY")?;
        connection.pragma_update(None, "foreign_keys", false)?;
        let mut budget = CopyBudget::default();
        copy_table(
            &read,
            &connection,
            "projects",
            "id IN (SELECT project_id FROM threads WHERE id=?1)",
            thread,
            &mut budget,
        )?;
        copy_table(&read, &connection, "threads", "id=?1", thread, &mut budget)?;
        for table in ForkTable::ALL {
            copy_table(
                &read,
                &connection,
                table.table(),
                table.scope(),
                thread,
                &mut budget,
            )?;
        }
        copy_table(
            &read,
            &connection,
            "pending_messages",
            "thread_id=?1 AND state='delivered'",
            thread,
            &mut budget,
        )?;
        copy_table(
            &read,
            &connection,
            "context_micro_compactions",
            "thread_id=?1",
            thread,
            &mut budget,
        )?;
        let metadata = [ForkTable::Duration, ForkTable::Builtin, ForkTable::Skill]
            .into_iter()
            .map(|table| ForkRows::capture(&read, table, thread))
            .collect::<rusqlite::Result<_>>()?;
        read.commit()?;
        Ok(Self {
            connection,
            fences,
            source,
            cutoff,
            origins,
            metadata,
        })
    }

    pub fn connection(&self) -> &Connection {
        &self.connection
    }
    pub fn cutoff(&self) -> &str {
        &self.cutoff
    }

    pub fn project(self, now: i64) -> Result<(Self, crate::threads::ThreadRow), ForkError> {
        let tx = self.connection.unchecked_transaction()?;
        let row = super::clone_prefix_with_origins(
            &tx,
            &self.source.id,
            &self.cutoff,
            now,
            Some(&self.origins),
        )?;
        tx.commit()?;
        Ok((self, row))
    }

    pub fn prepare_install(
        self,
        row: crate::threads::ThreadRow,
    ) -> Result<PreparedForkInstall, ForkError> {
        if row.forked_from_thread_id.as_deref() != Some(self.source.id.as_str())
            || row.forked_from_message_id.as_deref() != Some(self.cutoff.as_str())
        {
            return Err(corrupt("invalid prepared destination"));
        }
        let rows = ForkTable::ALL
            .iter()
            .map(|table| ForkRows::capture(&self.connection, *table, &row.id))
            .collect::<rusqlite::Result<_>>()?;
        Ok(PreparedForkInstall {
            fences: self.fences,
            source: self.source,
            row,
            rows,
            metadata: self.metadata,
        })
    }
}

impl PreparedForkInstall {
    pub fn install(mut self, tx: &Transaction<'_>) -> Result<crate::threads::ThreadRow, ForkError> {
        for fence in &self.fences {
            if !fence.validate(tx)? {
                return Err(corrupt("fork source changed during preparation"));
            }
        }
        if crate::threads::find(tx, &self.source.id)?.as_ref() != Some(&self.source)
            || !crate::projects::project_exists(tx, &self.source.project_id)?
        {
            return Err(corrupt("fork source changed during preparation"));
        }
        for metadata in &self.metadata {
            if !metadata.matches(tx, &self.source.id)? {
                return Err(corrupt("fork metadata changed during preparation"));
            }
        }
        self.row.title = super::next_fork_title(tx, &self.source)?;
        tx.execute("INSERT INTO threads(id,project_id,title,mode,permission_mode,model,backend,status,pinned,unread,created_at,updated_at,forked_from_thread_id,forked_from_message_id,reasoning_preferences) VALUES(?1,?2,?3,?4,'confirm',?5,'native','active',0,0,?6,?6,?7,?8,?9)", params![self.row.id,self.row.project_id,self.row.title,self.row.mode,self.row.model,self.row.created_at,self.row.forked_from_thread_id,self.row.forked_from_message_id,self.row.reasoning_preferences])?;
        for rows in self.rows {
            rows.install(tx)?;
        }
        Ok(self.row)
    }
}
