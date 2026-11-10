use rusqlite::{Connection, OptionalExtension, Transaction, params};

use super::{ContextSourceVersion, load_source_version_in_transaction};

#[derive(Clone)]
pub struct SourceFence {
    database: crate::DatabaseIdentity,
    thread: String,
    revision: i64,
    schema: i64,
}

impl SourceFence {
    pub fn capture(conn: &Connection, thread: &str) -> rusqlite::Result<Self> {
        verify_schema(conn)?;
        Ok(Self {
            database: crate::DatabaseIdentity::for_connection(conn)?,
            thread: thread.to_owned(),
            revision: revision(conn, thread)?,
            schema: conn.pragma_query_value(None, "schema_version", |row| row.get(0))?,
        })
    }

    pub fn validate(&self, tx: &Transaction<'_>) -> rusqlite::Result<bool> {
        Ok(
            crate::DatabaseIdentity::for_connection(tx)? == self.database
                && tx.pragma_query_value(None, "schema_version", |row| row.get::<_, i64>(0))?
                    == self.schema
                && revision(tx, &self.thread)? == self.revision
                && verify_schema(tx).is_ok(),
        )
    }

    pub fn thread(&self) -> &str {
        &self.thread
    }
}

fn revision(conn: &Connection, thread: &str) -> rusqlite::Result<i64> {
    conn.query_row("SELECT revision FROM preparation_revisions WHERE thread_id=?1 AND typeof(revision)='integer' AND revision>=0", [thread], |row| row.get(0))
}

fn verify_schema(conn: &Connection) -> rusqlite::Result<()> {
    for (sql, table) in [(
        include_str!("../../migrations/0061_preparation_revisions.sql"),
        "preparation_revisions",
    )] {
        let end = sql.find(';').ok_or(rusqlite::Error::InvalidQuery)?;
        let actual: String = conn.query_row(
            "SELECT sql FROM sqlite_schema WHERE type='table' AND name=?1",
            [table],
            |row| row.get(0),
        )?;
        if actual.trim_end_matches(';') != &sql[..end] {
            return Err(rusqlite::Error::InvalidQuery);
        }
    }
    for sql in [
        include_str!("../../migrations/0061_preparation_revisions.sql"),
        include_str!("../../migrations/0065_source_validation.sql"),
    ] {
        for section in sql.split("CREATE TRIGGER ").skip(1) {
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
    }
    Ok(())
}

#[derive(Clone)]
pub struct PreparedSourceVersion {
    fence: SourceFence,
    source: ContextSourceVersion,
}

impl PreparedSourceVersion {
    pub fn capture(conn: &Connection, thread: &str) -> rusqlite::Result<Self> {
        let tx = conn.unchecked_transaction()?;
        let value = Self::capture_in_transaction(&tx, thread)?;
        tx.commit()?;
        Ok(value)
    }

    pub fn capture_in_transaction(tx: &Transaction<'_>, thread: &str) -> rusqlite::Result<Self> {
        Ok(Self {
            fence: SourceFence::capture(tx, thread)?,
            source: load_source_version_in_transaction(tx, thread)?,
        })
    }

    pub fn validate(&self, tx: &Transaction<'_>, thread: &str) -> rusqlite::Result<bool> {
        Ok(self.fence.thread == thread && self.fence.validate(tx)?)
    }

    pub fn source(&self) -> &ContextSourceVersion {
        &self.source
    }
}

pub(crate) fn project_recovery_source(
    conn: &Connection,
    thread: &str,
    now: i64,
) -> rusqlite::Result<ContextSourceVersion> {
    let mut source = super::load_source_inner(conn, thread, false)?;
    for message in &mut source.messages {
        if message.status == "streaming" {
            message.status = "interrupted".into();
        }
    }
    for call in &mut source.tool_calls {
        match call.status.as_str() {
            "pending_approval" => {
                call.status = "rejected".into();
                call.approval = Some(crate::recovery::RECOVERY_DENIAL_APPROVAL_JSON.into());
                call.output_text = Some(crate::recovery::RECOVERY_REJECTED_OUTPUT.into());
                call.finished_at = Some(now);
                call.micro_compacted = false;
            }
            "approved" | "running" => {
                call.status = "cancelled".into();
                call.output_text = Some(crate::recovery::RECOVERY_CANCELLED_OUTPUT.into());
                call.finished_at = Some(now);
                call.micro_compacted = false;
            }
            _ => {}
        }
    }
    let (fingerprint, _) =
        super::fingerprint_source(conn, &source.messages, &source.tool_calls, thread, false)?;
    Ok(ContextSourceVersion {
        source_version: source.source_version,
        fingerprint,
    })
}

pub(crate) fn normalize_source(
    conn: &Connection,
    thread: &str,
    now: i64,
) -> rusqlite::Result<(usize, usize, usize)> {
    let messages = conn.execute(
        "UPDATE messages SET status='interrupted' WHERE thread_id=?1 AND status='streaming'",
        [thread],
    )?;
    let rejected = conn.execute("UPDATE tool_calls SET status='rejected',approval=?1,output_text=?2,finished_at=?3 WHERE thread_id=?4 AND status='pending_approval'", params![crate::recovery::RECOVERY_DENIAL_APPROVAL_JSON, crate::recovery::RECOVERY_REJECTED_OUTPUT, now, thread])?;
    let cancelled = conn.execute("UPDATE tool_calls SET status='cancelled',output_text=?1,finished_at=?2 WHERE thread_id=?3 AND status IN ('approved','running')", params![crate::recovery::RECOVERY_CANCELLED_OUTPUT, now, thread])?;
    Ok((messages, rejected, cancelled))
}

pub(crate) fn latest_checkpoint_id(
    conn: &Connection,
    thread: &str,
) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT id FROM context_checkpoints WHERE thread_id=?1 ORDER BY id DESC LIMIT 1",
        [thread],
        |row| row.get(0),
    )
    .optional()
}
