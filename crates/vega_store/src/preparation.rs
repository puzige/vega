use std::collections::HashMap;

use rusqlite::{Connection, Transaction, types::Value};

use crate::context_compaction::PreparedLegacyRedaction;
use crate::delegated_stages::PreparedPayloadRedaction;
use crate::fork_mutations::MutationOrigin;

const MAX_ROWS: usize = 400_000;
const MAX_BYTES: usize = 192 * 1024 * 1024;

const TABLES: &[(&str, &str)] = &[
    ("messages", "thread_id=?1"),
    ("tool_calls", "thread_id=?1"),
    (
        "image_attachments",
        "message_id IN (SELECT id FROM messages WHERE thread_id=?1)",
    ),
    ("pending_messages", "thread_id=?1"),
    (
        "permission_contract_state",
        "singleton=1 AND ?1 IS NOT NULL",
    ),
    ("historical_message_delivery", "thread_id=?1"),
    ("historical_tool_receipts", "thread_id=?1"),
    (
        "tool_call_rounds",
        "message_id IN (SELECT id FROM messages WHERE thread_id=?1)",
    ),
    ("context_micro_compactions", "thread_id=?1"),
    (
        "computer_screenshot_artifacts",
        "message_id IN (SELECT id FROM messages WHERE thread_id=?1)",
    ),
    ("context_checkpoints", "thread_id=?1"),
    (
        "assistant_responses",
        "message_id IN (SELECT id FROM messages WHERE thread_id=?1)",
    ),
    (
        "assistant_response_starts",
        "message_id IN (SELECT id FROM messages WHERE thread_id=?1)",
    ),
    (
        "message_timestamp_boundaries",
        "message_id IN (SELECT id FROM messages WHERE thread_id=?1)",
    ),
    (
        "generated_notices",
        "message_id IN (SELECT id FROM messages WHERE thread_id=?1)",
    ),
    ("delegated_executions", "child_thread_id=?1"),
    ("delegated_execution_messages", "child_thread_id=?1"),
    ("delegated_execution_recovery", "child_thread_id=?1"),
    ("delegated_stage_checkpoints", "child_thread_id=?1"),
];

pub struct DelegatedSnapshot<'a> {
    original: &'a Connection,
    database: Option<crate::DatabaseIdentity>,
    connection: Connection,
    thread: String,
    revision: i64,
    next_seq: i64,
    schema: i64,
    origins: HashMap<String, MutationOrigin>,
}

pub struct PreparedDelegatedInstall<'a> {
    snapshot: DelegatedSnapshot<'a>,
    legacy: PreparedLegacyRedaction,
    payloads: PreparedPayloadRedaction,
    assistant: crate::messages::MessageRow,
}

pub struct OwnedDelegatedInstall {
    database: crate::DatabaseIdentity,
    thread: String,
    revision: i64,
    next_seq: i64,
    schema: i64,
    origins: HashMap<String, MutationOrigin>,
    legacy: PreparedLegacyRedaction,
    payloads: PreparedPayloadRedaction,
    assistant: crate::messages::MessageRow,
}

fn revision(conn: &Connection, thread: &str) -> rusqlite::Result<i64> {
    let value: i64 = conn.query_row(
        "SELECT revision FROM preparation_revisions WHERE thread_id=?1 AND typeof(revision)='integer' AND revision>=0",
        [thread],
        |row| row.get(0),
    )?;
    Ok(value)
}

fn verify_schema(conn: &Connection) -> rusqlite::Result<()> {
    for section in include_str!("../migrations/0061_preparation_revisions.sql")
        .split("CREATE TRIGGER ")
        .skip(1)
    {
        let name = section
            .split_whitespace()
            .next()
            .ok_or(rusqlite::Error::InvalidQuery)?;
        let end = section.find("END;").ok_or(rusqlite::Error::InvalidQuery)?;
        let expected = format!("CREATE TRIGGER {}", &section[..end + 3]);
        let actual: String = conn.query_row(
            "SELECT sql FROM sqlite_schema WHERE type='trigger' AND name=?1",
            [name],
            |row| row.get(0),
        )?;
        if actual.trim_end_matches(';') != expected.trim_end_matches(';') {
            return Err(rusqlite::Error::InvalidQuery);
        }
    }
    Ok(())
}

impl<'a> DelegatedSnapshot<'a> {
    pub fn capture(conn: &'a Connection, thread: &str, assistant: &str) -> rusqlite::Result<Self> {
        let database = conn
            .path()
            .filter(|path| !path.is_empty())
            .map(|_| crate::DatabaseIdentity::for_connection(conn))
            .transpose()?;
        let read = conn.unchecked_transaction()?;
        verify_schema(&read)?;
        let revision = revision(&read, thread)?;
        let next_seq = crate::messages::next_seq(&read, thread)?;
        let schema = read.pragma_query_value(None, "schema_version", |row| row.get(0))?;
        for sql in [
            "SELECT EXISTS(SELECT 1 FROM tool_calls c LEFT JOIN messages m ON m.id=c.message_id WHERE (c.thread_id=?1 OR m.thread_id=?1) AND (m.id IS NULL OR c.thread_id<>m.thread_id) AND ?2 IS NOT NULL)",
            "SELECT EXISTS(SELECT 1 FROM delegated_execution_messages e LEFT JOIN messages m ON m.id=e.message_id WHERE e.child_thread_id=?1 AND ((m.id IS NULL AND e.message_id<>?2) OR m.thread_id<>?1))",
            "SELECT EXISTS(SELECT 1 FROM delegated_execution_recovery r JOIN delegated_stage_checkpoints c ON c.id=r.resume_checkpoint_id OR c.id=r.handoff_checkpoint_id WHERE r.child_thread_id=?1 AND c.child_thread_id<>?1 AND ?2 IS NOT NULL)",
        ] {
            if read.query_row(sql, [thread, assistant], |row| row.get::<_, bool>(0))? {
                return Err(rusqlite::Error::InvalidQuery);
            }
        }
        let connection = Connection::open_in_memory()?;
        connection.pragma_update(None, "temp_store", "MEMORY")?;
        connection.pragma_update(None, "foreign_keys", false)?;
        let mut rows_copied = 0usize;
        let mut bytes_copied = 0usize;
        for &(table, predicate) in TABLES {
            let definition: String = read.query_row(
                "SELECT sql FROM sqlite_schema WHERE type='table' AND name=?1",
                [table],
                |row| row.get(0),
            )?;
            connection.execute_batch(&definition)?;
            let mut select = read.prepare(&format!(
                "SELECT rowid AS preparation_rowid,* FROM {table} WHERE {predicate} ORDER BY rowid"
            ))?;
            let columns = select.column_count();
            let placeholders = std::iter::repeat_n("?", columns)
                .collect::<Vec<_>>()
                .join(",");
            let mut names = select.column_names();
            names[0] = "rowid";
            let names = names.join(",");
            let mut insert = connection.prepare(&format!(
                "INSERT INTO {table} ({names}) VALUES({placeholders})"
            ))?;
            let mut rows = select.query([thread])?;
            while let Some(row) = rows.next()? {
                rows_copied = rows_copied
                    .checked_add(1)
                    .ok_or(rusqlite::Error::InvalidQuery)?;
                let mut values = Vec::with_capacity(columns);
                for index in 0..columns {
                    let value = row.get_ref(index)?;
                    let bytes = match value {
                        rusqlite::types::ValueRef::Text(text)
                        | rusqlite::types::ValueRef::Blob(text) => text.len(),
                        _ => 8,
                    };
                    bytes_copied = bytes_copied
                        .checked_add(bytes)
                        .ok_or(rusqlite::Error::InvalidQuery)?;
                    if rows_copied > MAX_ROWS || bytes_copied > MAX_BYTES {
                        return Err(rusqlite::Error::InvalidQuery);
                    }
                    values.push(row.get::<_, Value>(index)?);
                }
                insert.execute(rusqlite::params_from_iter(values))?;
            }
            let indexes = read.prepare("SELECT sql FROM sqlite_schema WHERE type='index' AND tbl_name=?1 AND sql IS NOT NULL")?
                .query_map([table], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for index in indexes {
                connection.execute_batch(&index)?;
            }
        }
        let ids = read.prepare("SELECT id FROM tool_calls WHERE thread_id=?1 AND tool IN ('write','Write','edit','Edit') AND status IN ('success','failed','rejected','cancelled')")?
            .query_map([thread], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut origins = HashMap::new();
        for id in ids {
            origins.insert(
                id.clone(),
                crate::fork_mutations::origin(&read, thread, &id)?,
            );
        }
        read.commit()?;
        Ok(Self {
            original: conn,
            database,
            connection,
            thread: thread.to_owned(),
            revision,
            next_seq,
            schema,
            origins,
        })
    }

    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    pub fn origins(&self) -> &HashMap<String, MutationOrigin> {
        &self.origins
    }

    pub fn prepare_install(
        self,
        legacy: PreparedLegacyRedaction,
        payloads: PreparedPayloadRedaction,
        assistant: crate::messages::MessageRow,
    ) -> rusqlite::Result<PreparedDelegatedInstall<'a>> {
        self.prepare_turn_install(legacy, payloads, assistant, None, &[], None)
    }

    pub fn prepare_turn_install(
        self,
        legacy: PreparedLegacyRedaction,
        payloads: PreparedPayloadRedaction,
        assistant: crate::messages::MessageRow,
        user: Option<crate::messages::MessageRow>,
        images: &[Vec<u8>],
        pending: Option<(&str, &str)>,
    ) -> rusqlite::Result<PreparedDelegatedInstall<'a>> {
        if !legacy.belongs_to(&self.thread)
            || !payloads.belongs_to(&self.thread)
            || assistant.thread_id != self.thread
            || assistant.role != "assistant"
            || assistant.status != "streaming"
        {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let prepared = self.prepare_redacted_install(legacy, payloads, assistant)?;
        prepared.project_turn(user, images, pending)?;
        Ok(prepared)
    }

    pub fn prepare_redacted_install(
        self,
        legacy: PreparedLegacyRedaction,
        payloads: PreparedPayloadRedaction,
        assistant: crate::messages::MessageRow,
    ) -> rusqlite::Result<PreparedDelegatedInstall<'a>> {
        if !legacy.belongs_to(&self.thread)
            || !payloads.belongs_to(&self.thread)
            || assistant.thread_id != self.thread
            || assistant.role != "assistant"
            || assistant.status != "streaming"
        {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let mut projection = self.connection.unchecked_transaction()?;
        payloads.install(&mut projection)?;
        legacy.clone().install(&mut projection)?;
        projection.commit()?;
        Ok(PreparedDelegatedInstall {
            snapshot: self,
            legacy,
            payloads,
            assistant,
        })
    }
}

impl PreparedDelegatedInstall<'_> {
    pub fn project_turn(
        &self,
        user: Option<crate::messages::MessageRow>,
        images: &[Vec<u8>],
        pending: Option<(&str, &str)>,
    ) -> rusqlite::Result<()> {
        let projection = self.snapshot.connection.unchecked_transaction()?;
        let assistant = &self.assistant;
        if let Some(user) = user {
            if user.thread_id != self.snapshot.thread
                || user.role != "user"
                || user.status != "done"
                || user.seq != crate::messages::next_seq(&projection, &self.snapshot.thread)?
            {
                return Err(rusqlite::Error::InvalidQuery);
            }
            crate::messages::insert(&projection, &user)?;
        }
        if !images.is_empty() {
            let user_id: String = projection.query_row(
                "SELECT id FROM messages WHERE thread_id=?1 AND role='user' AND seq<?2 ORDER BY seq DESC LIMIT 1",
                rusqlite::params![self.snapshot.thread, assistant.seq], |row| row.get(0),
            )?;
            for (ordinal, image) in images.iter().enumerate() {
                crate::image_attachments::insert(&projection, &user_id, ordinal, image)?;
            }
        }
        if let Some((id, content)) = pending {
            if !crate::pending_messages::mark_delivered(&projection, &self.snapshot.thread, id)? {
                return Err(rusqlite::Error::InvalidQuery);
            }
            crate::pending_messages::preserve_model_input(
                &projection,
                &self.snapshot.thread,
                id,
                content,
            )?;
        }
        if assistant.seq != crate::messages::next_seq(&projection, &self.snapshot.thread)? {
            return Err(rusqlite::Error::InvalidQuery);
        }
        projection.execute(
            "INSERT INTO messages(id,thread_id,seq,role,kind,content,status,created_at,plan_status,plan_review_note,plan_reviewed_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            rusqlite::params![assistant.id, assistant.thread_id, assistant.seq, assistant.role, assistant.kind, assistant.content, assistant.status, assistant.created_at, assistant.plan_status, assistant.plan_review_note, assistant.plan_reviewed_at],
        )?;
        projection.commit()?;
        Ok(())
    }

    pub fn into_owned(self) -> rusqlite::Result<OwnedDelegatedInstall> {
        Ok(OwnedDelegatedInstall {
            database: self
                .snapshot
                .database
                .ok_or(rusqlite::Error::InvalidQuery)?,
            thread: self.snapshot.thread,
            revision: self.snapshot.revision,
            next_seq: self.snapshot.next_seq,
            schema: self.snapshot.schema,
            origins: self.snapshot.origins,
            legacy: self.legacy,
            payloads: self.payloads,
            assistant: self.assistant,
        })
    }

    pub fn connection(&self) -> &Connection {
        &self.snapshot.connection
    }

    pub fn origins(&self) -> &HashMap<String, MutationOrigin> {
        &self.snapshot.origins
    }

    pub fn validate(&self, transaction: &Transaction<'_>) -> rusqlite::Result<bool> {
        Ok(std::ptr::eq(&**transaction, self.snapshot.original)
            && transaction
                .pragma_query_value(None, "schema_version", |row| row.get::<_, i64>(0))?
                == self.snapshot.schema
            && revision(transaction, &self.snapshot.thread)? == self.snapshot.revision
            && crate::messages::next_seq(transaction, &self.snapshot.thread)?
                == self.snapshot.next_seq)
    }

    pub fn install(&self, transaction: &mut Transaction<'_>) -> rusqlite::Result<bool> {
        if !self.validate(transaction)? {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let savepoint = transaction.savepoint()?;
        for (id, expected) in &self.snapshot.origins {
            if crate::fork_mutations::origin(&savepoint, &self.snapshot.thread, id)? != *expected {
                return Err(rusqlite::Error::InvalidQuery);
            }
        }
        self.payloads.install_preparation_updates(&savepoint)?;
        let changed = self.legacy.install_preparation_updates(&savepoint)?;
        savepoint.commit()?;
        Ok(changed)
    }

    pub fn assistant(&self) -> &crate::messages::MessageRow {
        &self.assistant
    }
}

impl OwnedDelegatedInstall {
    pub fn validate(&self, transaction: &Transaction<'_>) -> rusqlite::Result<bool> {
        Ok(
            crate::DatabaseIdentity::for_connection(transaction)? == self.database
                && transaction
                    .pragma_query_value(None, "schema_version", |row| row.get::<_, i64>(0))?
                    == self.schema
                && revision(transaction, &self.thread)? == self.revision
                && crate::messages::next_seq(transaction, &self.thread)? == self.next_seq,
        )
    }

    pub fn install(&self, transaction: &mut Transaction<'_>) -> rusqlite::Result<bool> {
        if !self.validate(transaction)? {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let savepoint = transaction.savepoint()?;
        for (id, expected) in &self.origins {
            if crate::fork_mutations::origin(&savepoint, &self.thread, id)? != *expected {
                return Err(rusqlite::Error::InvalidQuery);
            }
        }
        self.payloads.install_preparation_updates(&savepoint)?;
        let changed = self.legacy.install_preparation_updates(&savepoint)?;
        savepoint.commit()?;
        Ok(changed)
    }

    pub fn assistant(&self) -> &crate::messages::MessageRow {
        &self.assistant
    }
}
