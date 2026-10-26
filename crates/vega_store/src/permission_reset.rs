use rusqlite::{Connection, OptionalExtension, Transaction, params};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResetConfigSource {
    pub configuration_id: String,
    pub mode: String,
    pub revision: i64,
    pub grant: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResetRecord {
    pub id: String,
    pub authority_epoch: String,
    pub source: ResetConfigSource,
    pub target_revision: i64,
    pub completed: bool,
}

impl ResetRecord {
    pub fn accepts_config(&self, current: &ResetConfigSource) -> bool {
        current == &self.source || self.config_is_reset(current)
    }

    pub fn config_is_reset(&self, current: &ResetConfigSource) -> bool {
        current.configuration_id == self.source.configuration_id
            && current.mode == "confirm"
            && current.revision == self.target_revision
            && current.grant.is_none()
    }
}

pub fn load(conn: &Connection) -> rusqlite::Result<Option<ResetRecord>> {
    let state = crate::permission_contracts::load_state(conn)?;
    let record = conn.query_row(
        "SELECT reset_id,authority_epoch,configuration_id,source_mode,source_revision,source_grant,target_revision,completed FROM permission_reset_journal WHERE singleton=1",
        [],
        |row| Ok(ResetRecord {
            id: row.get(0)?,
            authority_epoch: row.get(1)?,
            source: ResetConfigSource {
                configuration_id: row.get(2)?,
                mode: row.get(3)?,
                revision: row.get(4)?,
                grant: row.get(5)?,
            },
            target_revision: row.get(6)?,
            completed: row.get(7)?,
        }),
    ).optional()?;
    match &record {
        Some(record)
            if ulid::Ulid::from_string(&record.id).is_err()
                || record.authority_epoch != state.authority_epoch
                || !std::path::Path::new(&record.source.configuration_id).is_absolute()
                || record.source.revision < 0
                || record.source.revision.checked_add(1) != Some(record.target_revision)
                || record.completed == state.reset_pending =>
        {
            Err(rusqlite::Error::InvalidQuery)
        }
        None if state.reset_pending => Err(rusqlite::Error::InvalidQuery),
        _ => Ok(record),
    }
}

pub fn begin(
    transaction: &Transaction<'_>,
    expected_epoch: &str,
    source: &ResetConfigSource,
    now: i64,
) -> rusqlite::Result<ResetRecord> {
    let state = crate::permission_contracts::load_state(transaction)?;
    let previous = load(transaction)?;
    if state.authority_epoch != expected_epoch {
        return Err(rusqlite::Error::InvalidQuery);
    }
    if state.reset_pending {
        return previous
            .filter(|record| record.accepts_config(source))
            .ok_or(rusqlite::Error::InvalidQuery);
    }
    if source.revision < 0
        || !std::path::Path::new(&source.configuration_id).is_absolute()
        || transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM threads WHERE (permission_mode='full_access' OR full_access_grant IS NOT NULL) AND permission_revision=?1)",
            [i64::MAX],
            |row| row.get::<_, bool>(0),
        )?
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let record = ResetRecord {
        id: ulid::Ulid::generate().to_string(),
        authority_epoch: ulid::Ulid::generate().to_string(),
        source: source.clone(),
        target_revision: source
            .revision
            .checked_add(1)
            .ok_or(rusqlite::Error::InvalidQuery)?,
        completed: false,
    };
    let changed = transaction.execute(
        "UPDATE permission_contract_state SET authority_epoch=?1,reset_state='pending' WHERE singleton=1 AND authority_epoch=?2 AND reset_state='ready'",
        params![record.authority_epoch,expected_epoch],
    )?;
    if changed != 1 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    transaction.execute(
        "INSERT INTO permission_reset_journal(singleton,reset_id,authority_epoch,configuration_id,source_mode,source_revision,source_grant,target_revision,completed,created_at) VALUES(1,?1,?2,?3,?4,?5,?6,?7,0,?8) ON CONFLICT(singleton) DO UPDATE SET reset_id=excluded.reset_id,authority_epoch=excluded.authority_epoch,configuration_id=excluded.configuration_id,source_mode=excluded.source_mode,source_revision=excluded.source_revision,source_grant=excluded.source_grant,target_revision=excluded.target_revision,completed=0,created_at=excluded.created_at",
        params![record.id,record.authority_epoch,source.configuration_id,source.mode,source.revision,source.grant,record.target_revision,now],
    )?;
    transaction.execute(
        "UPDATE threads SET permission_mode=CASE WHEN permission_mode='full_access' THEN 'confirm' ELSE permission_mode END,permission_revision=permission_revision+1,full_access_grant=NULL,updated_at=?1 WHERE permission_mode='full_access' OR full_access_grant IS NOT NULL",
        [now],
    )?;
    transaction.execute(
        "UPDATE pending_messages SET permission_invalidated=1 WHERE state!='delivered'",
        [],
    )?;
    transaction.execute(
        "UPDATE credential_continuations SET permission_invalidated=1",
        [],
    )?;
    transaction.execute(
        "INSERT INTO permission_replay_cutoffs(thread_id,through_seq,reset_id) SELECT thread_id,MAX(seq),?1 FROM messages GROUP BY thread_id ON CONFLICT(thread_id) DO UPDATE SET through_seq=MAX(permission_replay_cutoffs.through_seq,excluded.through_seq),reset_id=excluded.reset_id",
        [&record.id],
    )?;
    transaction.execute(
        "UPDATE command_scope_audits SET state='cancelled',updated_at=?1 WHERE state IN ('pending','approved')",
        [now],
    )?;
    Ok(record)
}

pub fn finish(
    transaction: &Transaction<'_>,
    expected: &ResetRecord,
    config: &ResetConfigSource,
) -> rusqlite::Result<()> {
    let current = load(transaction)?.ok_or(rusqlite::Error::InvalidQuery)?;
    let mut expected = expected.clone();
    expected.completed = current.completed;
    if current != expected || !current.config_is_reset(config) {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let unfinished: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM threads WHERE permission_mode='full_access' OR full_access_grant IS NOT NULL) OR EXISTS(SELECT 1 FROM pending_messages WHERE state!='delivered' AND permission_invalidated=0) OR EXISTS(SELECT 1 FROM credential_continuations WHERE permission_invalidated=0) OR EXISTS(SELECT 1 FROM command_scope_audits WHERE state IN ('pending','approved')) OR EXISTS(SELECT 1 FROM automation_tasks WHERE json_extract(data,'$.permission_authorization') IS NOT NULL OR json_extract(data,'$.status')='active' OR json_extract(data,'$.revision') IS NOT revision) OR EXISTS(SELECT 1 FROM automation_runs WHERE finished=0)",
        [],
        |row| row.get(0),
    )?;
    if unfinished {
        return Err(rusqlite::Error::InvalidQuery);
    }
    if current.completed {
        return Ok(());
    }
    let changed = transaction.execute(
        "UPDATE permission_contract_state SET reset_state='ready' WHERE singleton=1 AND reset_state='pending' AND authority_epoch=?1",
        [&current.authority_epoch],
    )?;
    if changed != 1 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let changed = transaction.execute(
        "UPDATE permission_reset_journal SET completed=1 WHERE singleton=1 AND reset_id=?1 AND completed=0",
        [&current.id],
    )?;
    if changed != 1 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(())
}

pub fn replay_allowed(conn: &Connection, thread: &str, seq: i64) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM permission_contract_state WHERE singleton=1 AND reset_state='ready') AND NOT EXISTS(SELECT 1 FROM permission_replay_cutoffs WHERE thread_id=?1 AND through_seq>=?2)",
        params![thread,seq],
        |row| row.get(0),
    )
}
