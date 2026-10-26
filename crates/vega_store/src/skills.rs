//! Private Skills consent, selection, audit and run-snapshot persistence.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, Row, params};
use sha2::{Digest, Sha256};

/// Store-local errors; no private path, body or reference bytes enter messages.
#[derive(Debug, thiserror::Error)]
pub enum SkillStoreError {
    #[error("skill store operation failed: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("invalid skill persistence input")]
    InvalidInput,
    #[error("skill authority or run binding changed")]
    Stale,
    #[error("stored skill snapshot digest mismatch")]
    DigestMismatch,
    #[error("skill snapshot exceeds the local limit")]
    TooLarge,
    #[error("skill row not found")]
    NotFound,
    #[error("skill generation counter exhausted")]
    GenerationOverflow,
}

/// Global Skills switches and monotonic authority epochs. Both switches
/// default off; consent/revocation generations fence stale run snapshots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SkillSettings {
    pub global_enabled: bool,
    pub automatic_enabled: bool,
    pub consent_generation: u64,
    pub revocation_generation: u64,
}

/// Per-project switches. Missing rows have the same off/off safe default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProjectSkillSettings {
    pub enabled: bool,
    pub automatic: bool,
}

/// Exact UI-authorized root association, not evidence that the path is still
/// safe now. The controller must re-bind it through `SkillSource` before use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillSourceRecord {
    pub id: String,
    pub scope: String,
    pub project_id: Option<String>,
    pub configured_root: String,
    pub canonical_root: String,
    pub root_dev: String,
    pub root_ino: String,
    pub import_order: i64,
    pub enabled: bool,
    pub automatic: bool,
    pub created_at: i64,
}

/// Root association created only after an exact native UI choice/preview.
pub struct NewSkillSource<'a> {
    pub id: &'a str,
    pub scope: &'a str,
    pub project_id: Option<&'a str>,
    pub configured_root: &'a str,
    pub canonical_root: &'a str,
    pub root_dev: &'a str,
    pub root_ino: &'a str,
    pub import_order: i64,
    pub enabled: bool,
    pub automatic: bool,
    pub created_at: i64,
}

/// Exact per-Skill reviewed content hash and UI preferences.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillApprovalRecord {
    pub source_id: String,
    pub name: String,
    pub approved_sha256: String,
    pub source_label: String,
    pub enabled: bool,
    pub automatic: bool,
    pub reviewed_at: i64,
}

/// Controller-created approval; only a UI-reviewed `SkillCandidate` may
/// supply its identity and content hash.
pub struct NewSkillApproval<'a> {
    pub source_id: &'a str,
    pub name: &'a str,
    pub approved_sha256: &'a str,
    pub source_label: &'a str,
    pub enabled: bool,
    pub automatic: bool,
    pub reviewed_at: i64,
}

/// Read the singleton authority row. A missing row is a corrupt migration,
/// never an implicit opt-in.
pub fn read_settings(conn: &Connection) -> Result<SkillSettings, SkillStoreError> {
    let (global, auto, consent, revoked): (i64, i64, i64, i64) = conn.query_row(
        "SELECT global_enabled, automatic_enabled, consent_generation, \
         revocation_generation FROM skill_settings WHERE id = 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;
    Ok(SkillSettings {
        global_enabled: global != 0,
        automatic_enabled: auto != 0,
        consent_generation: u64::try_from(consent).map_err(|_| SkillStoreError::InvalidInput)?,
        revocation_generation: u64::try_from(revoked).map_err(|_| SkillStoreError::InvalidInput)?,
    })
}

/// UI-only update. Disabling either switch revokes currently active runs;
/// every actual mutation increments consent generation in one transaction.
pub fn set_global_settings(
    conn: &Connection,
    expected_consent_generation: u64,
    global_enabled: bool,
    automatic_enabled: bool,
) -> Result<SkillSettings, SkillStoreError> {
    let _permit =
        crate::write_coordination::acquire(conn, crate::write_coordination::WriteCategory::Skills)?;
    let tx = crate::write_coordination::WriteTransaction::new(
        conn,
        crate::write_coordination::WriteCategory::Skills,
    )?;
    ensure_generation(&tx, expected_consent_generation)?;
    let previous = read_settings(&tx)?;
    if previous.global_enabled != global_enabled || previous.automatic_enabled != automatic_enabled
    {
        tx.execute(
            "UPDATE skill_settings SET global_enabled = ?1, automatic_enabled = ?2 WHERE id = 1",
            params![global_enabled, automatic_enabled],
        )?;
        bump_generations(
            &tx,
            (previous.global_enabled && !global_enabled)
                || (previous.automatic_enabled && !automatic_enabled),
        )?;
    }
    let updated = read_settings(&tx)?;
    tx.commit()?;
    Ok(updated)
}

/// Read one project's settings; a missing settings row is safely off.
pub fn read_project_settings(
    conn: &Connection,
    project_id: &str,
) -> Result<ProjectSkillSettings, SkillStoreError> {
    if !valid_id(project_id) {
        return Err(SkillStoreError::InvalidInput);
    }
    let value: Option<(i64, i64)> = conn
        .query_row(
            "SELECT enabled, automatic FROM skill_project_settings WHERE project_id = ?1",
            [project_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let (enabled, automatic) = value.unwrap_or((0, 0));
    Ok(ProjectSkillSettings {
        enabled: enabled != 0,
        automatic: automatic != 0,
    })
}

/// Atomic UI-only project switch update. The project FK must exist.
pub fn set_project_settings(
    conn: &Connection,
    expected_consent_generation: u64,
    project_id: &str,
    enabled: bool,
    automatic: bool,
) -> Result<ProjectSkillSettings, SkillStoreError> {
    let _permit =
        crate::write_coordination::acquire(conn, crate::write_coordination::WriteCategory::Skills)?;
    if !valid_id(project_id) {
        return Err(SkillStoreError::InvalidInput);
    }
    let tx = crate::write_coordination::WriteTransaction::new(
        conn,
        crate::write_coordination::WriteCategory::Skills,
    )?;
    ensure_generation(&tx, expected_consent_generation)?;
    let previous = read_project_settings(&tx, project_id)?;
    if previous.enabled != enabled || previous.automatic != automatic {
        tx.execute(
            "INSERT INTO skill_project_settings (project_id, enabled, automatic) \
             VALUES (?1, ?2, ?3) ON CONFLICT(project_id) DO UPDATE SET \
             enabled = excluded.enabled, automatic = excluded.automatic",
            params![project_id, enabled, automatic],
        )?;
        bump_generations(
            &tx,
            (previous.enabled && !enabled) || (previous.automatic && !automatic),
        )?;
    }
    tx.commit()?;
    Ok(ProjectSkillSettings { enabled, automatic })
}

/// Add one reviewed root association. This does not scan or grant trust to a
/// file; the controller must have used an exact UI-selected `SkillSource`.
pub fn link_source(
    conn: &Connection,
    expected_consent_generation: u64,
    input: NewSkillSource<'_>,
) -> Result<(), SkillStoreError> {
    let _permit =
        crate::write_coordination::acquire(conn, crate::write_coordination::WriteCategory::Skills)?;
    validate_source(&input)?;
    let tx = crate::write_coordination::WriteTransaction::new(
        conn,
        crate::write_coordination::WriteCategory::Skills,
    )?;
    ensure_generation(&tx, expected_consent_generation)?;
    tx.execute(
        "INSERT INTO skill_sources \
         (id, scope, project_id, configured_root, canonical_root, root_dev, root_ino, \
          import_order, enabled, automatic, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            input.id,
            input.scope,
            input.project_id,
            input.configured_root,
            input.canonical_root,
            input.root_dev,
            input.root_ino,
            input.import_order,
            input.enabled,
            input.automatic,
            input.created_at,
        ],
    )?;
    bump_generations(&tx, false)?;
    tx.commit()?;
    Ok(())
}

/// Deterministic UI projection: project, Vega-owned global, imported UI order.
/// A run controller must additionally filter project rows to its own project.
pub fn list_sources(conn: &Connection) -> Result<Vec<SkillSourceRecord>, SkillStoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, scope, project_id, configured_root, canonical_root, root_dev, root_ino, \
         import_order, enabled, automatic, created_at FROM skill_sources ORDER BY \
         CASE scope WHEN 'project' THEN 0 WHEN 'vega_global' THEN 1 ELSE 2 END, \
         import_order, canonical_root COLLATE BINARY, id COLLATE BINARY",
    )?;
    Ok(stmt
        .query_map([], source_from_row)?
        .collect::<Result<Vec<_>, _>>()?)
}

/// Run-start projection for only the selected project's source plus the
/// global/imported roots. An unrelated project's Skills never enter catalog.
pub fn list_sources_for_project(
    conn: &Connection,
    project_id: Option<&str>,
) -> Result<Vec<SkillSourceRecord>, SkillStoreError> {
    if project_id.is_some_and(|id| !valid_id(id)) {
        return Err(SkillStoreError::InvalidInput);
    }
    let mut stmt = conn.prepare(
        "SELECT id, scope, project_id, configured_root, canonical_root, root_dev, root_ino, \
         import_order, enabled, automatic, created_at FROM skill_sources \
         WHERE scope != 'project' OR project_id = ?1 ORDER BY \
         CASE scope WHEN 'project' THEN 0 WHEN 'vega_global' THEN 1 ELSE 2 END, \
         import_order, canonical_root COLLATE BINARY, id COLLATE BINARY",
    )?;
    Ok(stmt
        .query_map([project_id], source_from_row)?
        .collect::<Result<Vec<_>, _>>()?)
}

/// UI-only root toggle; disabling revokes active runs while retaining review
/// records for transparent re-enablement with the same exact content hash.
pub fn set_source_preferences(
    conn: &Connection,
    expected_consent_generation: u64,
    source_id: &str,
    enabled: bool,
    automatic: bool,
) -> Result<(), SkillStoreError> {
    let _permit =
        crate::write_coordination::acquire(conn, crate::write_coordination::WriteCategory::Skills)?;
    if !valid_id(source_id) {
        return Err(SkillStoreError::InvalidInput);
    }
    let tx = crate::write_coordination::WriteTransaction::new(
        conn,
        crate::write_coordination::WriteCategory::Skills,
    )?;
    ensure_generation(&tx, expected_consent_generation)?;
    let previous: Option<(i64, i64)> = tx
        .query_row(
            "SELECT enabled, automatic FROM skill_sources WHERE id = ?1",
            [source_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let (was_enabled, was_auto) = previous.ok_or(SkillStoreError::NotFound)?;
    if (was_enabled != 0) != enabled || (was_auto != 0) != automatic {
        tx.execute(
            "UPDATE skill_sources SET enabled = ?1, automatic = ?2 WHERE id = ?3",
            params![enabled, automatic, source_id],
        )?;
        bump_generations(
            &tx,
            (was_enabled != 0 && !enabled) || (was_auto != 0 && !automatic),
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// Remove Vega's association and cascading approvals, never source files.
pub fn unlink_source(
    conn: &Connection,
    expected_consent_generation: u64,
    source_id: &str,
) -> Result<bool, SkillStoreError> {
    let _permit =
        crate::write_coordination::acquire(conn, crate::write_coordination::WriteCategory::Skills)?;
    if !valid_id(source_id) {
        return Err(SkillStoreError::InvalidInput);
    }
    let tx = crate::write_coordination::WriteTransaction::new(
        conn,
        crate::write_coordination::WriteCategory::Skills,
    )?;
    ensure_generation(&tx, expected_consent_generation)?;
    let deleted = tx.execute("DELETE FROM skill_sources WHERE id = ?1", [source_id])? > 0;
    // `skill_source_delete_revokes` also catches FK cascades from project
    // removal, so do not double-increment the epoch here.
    tx.commit()?;
    Ok(deleted)
}

/// Record one exact UI-reviewed SHA. A changed hash revokes the prior content
/// even if the new review remains enabled.
pub fn approve_skill(
    conn: &Connection,
    expected_consent_generation: u64,
    input: NewSkillApproval<'_>,
) -> Result<(), SkillStoreError> {
    let _permit =
        crate::write_coordination::acquire(conn, crate::write_coordination::WriteCategory::Skills)?;
    if !valid_id(input.source_id)
        || !valid_name(input.name)
        || !valid_name(input.source_label)
        || !valid_hash(input.approved_sha256)
    {
        return Err(SkillStoreError::InvalidInput);
    }
    let tx = crate::write_coordination::WriteTransaction::new(
        conn,
        crate::write_coordination::WriteCategory::Skills,
    )?;
    ensure_generation(&tx, expected_consent_generation)?;
    let old: Option<(String, i64, i64, String)> = tx
        .query_row(
            "SELECT approved_sha256, enabled, automatic, source_label \
             FROM skill_approvals WHERE source_id = ?1 AND name = ?2",
            params![input.source_id, input.name],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let changed = old
        .as_ref()
        .is_none_or(|(hash, enabled, automatic, label)| {
            hash != input.approved_sha256
                || (*enabled != 0) != input.enabled
                || (*automatic != 0) != input.automatic
                || label != input.source_label
        });
    tx.execute(
        "INSERT INTO skill_approvals \
         (source_id, name, approved_sha256, source_label, enabled, automatic, reviewed_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
         ON CONFLICT(source_id, name) DO UPDATE SET \
         approved_sha256 = excluded.approved_sha256, source_label = excluded.source_label, \
         enabled = excluded.enabled, automatic = excluded.automatic, \
         reviewed_at = excluded.reviewed_at",
        params![
            input.source_id,
            input.name,
            input.approved_sha256,
            input.source_label,
            input.enabled,
            input.automatic,
            input.reviewed_at,
        ],
    )?;
    if changed {
        let revokes = old.as_ref().is_some_and(|(hash, enabled, automatic, _)| {
            hash != input.approved_sha256
                || (*enabled != 0 && !input.enabled)
                || (*automatic != 0 && !input.automatic)
        });
        bump_generations(&tx, revokes)?;
    }
    tx.commit()?;
    Ok(())
}

/// Toggle an existing exact approval without minting consent for new bytes.
/// A changed file still requires a separate UI review and `approve_skill`.
pub fn set_approval_preferences(
    conn: &Connection,
    expected_consent_generation: u64,
    source_id: &str,
    name: &str,
    enabled: bool,
    automatic: bool,
) -> Result<(), SkillStoreError> {
    let _permit =
        crate::write_coordination::acquire(conn, crate::write_coordination::WriteCategory::Skills)?;
    if !valid_id(source_id) || !valid_name(name) {
        return Err(SkillStoreError::InvalidInput);
    }
    let tx = crate::write_coordination::WriteTransaction::new(
        conn,
        crate::write_coordination::WriteCategory::Skills,
    )?;
    ensure_generation(&tx, expected_consent_generation)?;
    let old: Option<(i64, i64)> = tx
        .query_row(
            "SELECT enabled, automatic FROM skill_approvals \
             WHERE source_id = ?1 AND name = ?2",
            params![source_id, name],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let (was_enabled, was_auto) = old.ok_or(SkillStoreError::NotFound)?;
    if (was_enabled != 0) != enabled || (was_auto != 0) != automatic {
        tx.execute(
            "UPDATE skill_approvals SET enabled = ?1, automatic = ?2 \
             WHERE source_id = ?3 AND name = ?4",
            params![enabled, automatic, source_id, name],
        )?;
        bump_generations(
            &tx,
            (was_enabled != 0 && !enabled) || (was_auto != 0 && !automatic),
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// Exact approvals for Settings; callers must filter enabled roots and the
/// selected project before freezing a model catalog.
pub fn list_approved_skills(
    conn: &Connection,
) -> Result<Vec<SkillApprovalRecord>, SkillStoreError> {
    let mut stmt = conn.prepare(
        "SELECT source_id, name, approved_sha256, source_label, enabled, automatic, reviewed_at \
         FROM skill_approvals ORDER BY source_id COLLATE BINARY, name COLLATE BINARY",
    )?;
    Ok(stmt
        .query_map([], approval_from_row)?
        .collect::<Result<Vec<_>, _>>()?)
}

fn bump_generations(conn: &Connection, revokes: bool) -> Result<(), SkillStoreError> {
    let _permit =
        crate::write_coordination::acquire(conn, crate::write_coordination::WriteCategory::Skills)?;
    let updated = conn.execute(
        "UPDATE skill_settings SET consent_generation = consent_generation + 1, \
         revocation_generation = revocation_generation + ?1 \
         WHERE id = 1 AND consent_generation < 9223372036854775807 \
         AND revocation_generation <= 9223372036854775807 - ?1",
        [i64::from(revokes)],
    )?;
    if updated != 1 {
        return Err(SkillStoreError::GenerationOverflow);
    }
    Ok(())
}

fn ensure_generation(conn: &Connection, expected: u64) -> Result<(), SkillStoreError> {
    if read_settings(conn)?.consent_generation != expected {
        return Err(SkillStoreError::Stale);
    }
    Ok(())
}

fn source_from_row(row: &Row<'_>) -> Result<SkillSourceRecord, rusqlite::Error> {
    Ok(SkillSourceRecord {
        id: row.get(0)?,
        scope: row.get(1)?,
        project_id: row.get(2)?,
        configured_root: row.get(3)?,
        canonical_root: row.get(4)?,
        root_dev: row.get(5)?,
        root_ino: row.get(6)?,
        import_order: row.get(7)?,
        enabled: row.get::<_, i64>(8)? != 0,
        automatic: row.get::<_, i64>(9)? != 0,
        created_at: row.get(10)?,
    })
}

fn approval_from_row(row: &Row<'_>) -> Result<SkillApprovalRecord, rusqlite::Error> {
    Ok(SkillApprovalRecord {
        source_id: row.get(0)?,
        name: row.get(1)?,
        approved_sha256: row.get(2)?,
        source_label: row.get(3)?,
        enabled: row.get::<_, i64>(4)? != 0,
        automatic: row.get::<_, i64>(5)? != 0,
        reviewed_at: row.get(6)?,
    })
}

fn validate_source(input: &NewSkillSource<'_>) -> Result<(), SkillStoreError> {
    if !valid_id(input.id)
        || !matches!(input.scope, "project" | "vega_global" | "imported")
        || (input.scope == "project") != input.project_id.is_some()
        || input.project_id.is_some_and(|id| !valid_id(id))
        || !valid_path(input.configured_root)
        || !valid_path(input.canonical_root)
        || !valid_u64_text(input.root_dev)
        || !valid_u64_text(input.root_ino)
        || input.import_order < 0
    {
        return Err(SkillStoreError::InvalidInput);
    }
    Ok(())
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

fn valid_name(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0] != b'-'
        && bytes[bytes.len() - 1] != b'-'
        && !bytes.windows(2).any(|pair| pair == b"--")
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_path(value: &str) -> bool {
    value.len() <= 4096
        && !value.contains('\0')
        && Path::new(value).is_absolute()
        && !value
            .split('/')
            .any(|component| component == "." || component == "..")
}

fn valid_u64_text(value: &str) -> bool {
    value
        .parse::<u64>()
        .is_ok_and(|number| number.to_string() == value)
}

/// Frozen run state; bytes are private and never projected into an audit row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillSnapshotRecord {
    pub run_id: String,
    pub thread_id: String,
    pub consent_generation: u64,
    pub revocation_generation: u64,
    pub catalog_sha256: String,
    pub snapshot_sha256: String,
    pub bytes: Vec<u8>,
    pub updated_at: i64,
}

/// Runtime-exported snapshot and its independently held trusted binding.
pub struct NewSkillSnapshot<'a> {
    pub run_id: &'a str,
    pub thread_id: &'a str,
    pub consent_generation: u64,
    pub revocation_generation: u64,
    pub catalog_sha256: &'a str,
    pub snapshot_sha256: &'a str,
    pub bytes: &'a [u8],
    pub updated_at: i64,
}

struct SnapshotMetadata {
    run_id: String,
    thread_id: String,
    consent_generation: i64,
    revocation_generation: i64,
    catalog_sha256: String,
    snapshot_sha256: String,
    size: i64,
    updated_at: i64,
}

fn snapshot_metadata_from_row(row: &Row<'_>) -> Result<SnapshotMetadata, rusqlite::Error> {
    Ok(SnapshotMetadata {
        run_id: row.get(0)?,
        thread_id: row.get(1)?,
        consent_generation: row.get(2)?,
        revocation_generation: row.get(3)?,
        catalog_sha256: row.get(4)?,
        snapshot_sha256: row.get(5)?,
        size: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

/// Store a new or updated snapshot without changing its immutable run/thread/
/// catalog/epoch binding. A changed authority epoch rejects the write.
pub fn save_snapshot(
    conn: &Connection,
    input: NewSkillSnapshot<'_>,
) -> Result<(), SkillStoreError> {
    let _permit =
        crate::write_coordination::acquire(conn, crate::write_coordination::WriteCategory::Skills)?;
    if !valid_id(input.run_id)
        || !valid_id(input.thread_id)
        || !valid_hash(input.catalog_sha256)
        || !valid_hash(input.snapshot_sha256)
        || input.bytes.is_empty()
    {
        return Err(SkillStoreError::InvalidInput);
    }
    if input.bytes.len() > 1024 * 1024 {
        return Err(SkillStoreError::TooLarge);
    }
    if digest(input.bytes) != input.snapshot_sha256 {
        return Err(SkillStoreError::DigestMismatch);
    }
    let consent =
        i64::try_from(input.consent_generation).map_err(|_| SkillStoreError::InvalidInput)?;
    let revoked =
        i64::try_from(input.revocation_generation).map_err(|_| SkillStoreError::InvalidInput)?;
    let tx = crate::write_coordination::WriteTransaction::new(
        conn,
        crate::write_coordination::WriteCategory::Skills,
    )?;
    let settings = read_settings(&tx)?;
    if settings.consent_generation != input.consent_generation
        || settings.revocation_generation != input.revocation_generation
    {
        return Err(SkillStoreError::Stale);
    }
    let old: Option<(String, i64, i64, String)> = tx
        .query_row(
            "SELECT thread_id, consent_generation, revocation_generation, catalog_sha256 \
             FROM skill_run_snapshots WHERE run_id = ?1",
            [input.run_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    if old
        .as_ref()
        .is_some_and(|(thread, prior_consent, prior_revoked, catalog)| {
            thread != input.thread_id
                || *prior_consent != consent
                || *prior_revoked != revoked
                || catalog != input.catalog_sha256
        })
    {
        return Err(SkillStoreError::Stale);
    }
    tx.execute(
        "INSERT INTO skill_run_snapshots \
         (run_id, thread_id, consent_generation, revocation_generation, catalog_sha256, \
          snapshot_sha256, bytes, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
         ON CONFLICT(run_id) DO UPDATE SET snapshot_sha256 = excluded.snapshot_sha256, \
         bytes = excluded.bytes, updated_at = excluded.updated_at",
        params![
            input.run_id,
            input.thread_id,
            consent,
            revoked,
            input.catalog_sha256,
            input.snapshot_sha256,
            input.bytes,
            input.updated_at,
        ],
    )?;
    tx.commit()?;
    Ok(())
}

/// Load a recoverable snapshot only when its persisted digest and current
/// consent/revocation epochs still match. Runtime validates its inner format.
pub fn load_recoverable_snapshot(
    conn: &Connection,
    run_id: &str,
) -> Result<Option<SkillSnapshotRecord>, SkillStoreError> {
    if !valid_id(run_id) {
        return Err(SkillStoreError::InvalidInput);
    }
    // Inspect the SQL length before materializing private bytes. The read
    // transaction makes the metadata and subsequent BLOB read one snapshot.
    let tx = conn.unchecked_transaction()?;
    let metadata = tx
        .query_row(
            "SELECT run_id, thread_id, consent_generation, revocation_generation, \
             catalog_sha256, snapshot_sha256, length(bytes), updated_at \
             FROM skill_run_snapshots WHERE run_id = ?1",
            [run_id],
            snapshot_metadata_from_row,
        )
        .optional()?;
    let Some(metadata) = metadata else {
        return Ok(None);
    };
    if !(1..=1024 * 1024).contains(&metadata.size) {
        return Err(SkillStoreError::TooLarge);
    }
    let bytes: Vec<u8> = tx.query_row(
        "SELECT bytes FROM skill_run_snapshots WHERE run_id = ?1",
        [run_id],
        |row| row.get(0),
    )?;
    if bytes.len() != usize::try_from(metadata.size).map_err(|_| SkillStoreError::TooLarge)? {
        return Err(SkillStoreError::DigestMismatch);
    }
    if !valid_hash(&metadata.catalog_sha256)
        || !valid_hash(&metadata.snapshot_sha256)
        || digest(&bytes) != metadata.snapshot_sha256
    {
        return Err(SkillStoreError::DigestMismatch);
    }
    let consent_generation =
        u64::try_from(metadata.consent_generation).map_err(|_| SkillStoreError::InvalidInput)?;
    let revocation_generation =
        u64::try_from(metadata.revocation_generation).map_err(|_| SkillStoreError::InvalidInput)?;
    let settings = read_settings(&tx)?;
    if consent_generation != settings.consent_generation
        || revocation_generation != settings.revocation_generation
    {
        return Err(SkillStoreError::Stale);
    }
    tx.commit()?;
    let record = SkillSnapshotRecord {
        run_id: metadata.run_id,
        thread_id: metadata.thread_id,
        consent_generation,
        revocation_generation,
        catalog_sha256: metadata.catalog_sha256,
        snapshot_sha256: metadata.snapshot_sha256,
        bytes,
        updated_at: metadata.updated_at,
    };
    Ok(Some(record))
}

/// Full thread pin identity, retained even if its source association is later
/// unlinked so the UI can report unavailable rather than substitute a copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreadSkillPin {
    pub thread_id: String,
    pub scope: String,
    pub canonical_root: String,
    pub root_dev: String,
    pub root_ino: String,
    pub name: String,
    pub approved_sha256: String,
    pub source_label: String,
    pub pinned_at: i64,
}

/// Exact UI selection to pin; must still match an enabled approval now.
pub struct NewThreadSkillPin<'a> {
    pub thread_id: &'a str,
    pub scope: &'a str,
    pub canonical_root: &'a str,
    pub root_dev: &'a str,
    pub root_ino: &'a str,
    pub name: &'a str,
    pub approved_sha256: &'a str,
    pub source_label: &'a str,
    pub pinned_at: i64,
}

/// Pin only an exact currently enabled/root-reviewed Skill. A future source
/// change cannot silently retarget this full source/hash identity.
pub fn save_thread_pin(
    conn: &Connection,
    expected_consent_generation: u64,
    pin: NewThreadSkillPin<'_>,
) -> Result<(), SkillStoreError> {
    let _permit =
        crate::write_coordination::acquire(conn, crate::write_coordination::WriteCategory::Skills)?;
    if !valid_id(pin.thread_id)
        || !matches!(pin.scope, "project" | "vega_global" | "imported")
        || !valid_path(pin.canonical_root)
        || !valid_u64_text(pin.root_dev)
        || !valid_u64_text(pin.root_ino)
        || !valid_name(pin.name)
        || !valid_hash(pin.approved_sha256)
        || !valid_name(pin.source_label)
    {
        return Err(SkillStoreError::InvalidInput);
    }
    let tx = crate::write_coordination::WriteTransaction::new(
        conn,
        crate::write_coordination::WriteCategory::Skills,
    )?;
    ensure_generation(&tx, expected_consent_generation)?;
    let approved: Option<i64> = tx
        .query_row(
            "SELECT 1 FROM threads AS t JOIN skill_sources AS s \
             ON s.scope = ?2 AND s.canonical_root = ?3 AND s.root_dev = ?4 \
             AND s.root_ino = ?5 AND s.enabled = 1 \
             JOIN skill_approvals AS a ON a.source_id = s.id AND a.name = ?6 \
             AND a.approved_sha256 = ?7 AND a.source_label = ?8 AND a.enabled = 1 \
             JOIN skill_settings AS gs ON gs.id = 1 \
             LEFT JOIN skill_project_settings AS ps ON ps.project_id = s.project_id \
             WHERE t.id = ?1 AND \
             ((s.scope = 'project' AND s.project_id = t.project_id \
               AND COALESCE(ps.enabled, 0) = 1) OR \
              (s.scope != 'project' AND gs.global_enabled = 1)) LIMIT 1",
            params![
                pin.thread_id,
                pin.scope,
                pin.canonical_root,
                pin.root_dev,
                pin.root_ino,
                pin.name,
                pin.approved_sha256,
                pin.source_label,
            ],
            |row| row.get(0),
        )
        .optional()?;
    if approved.is_none() {
        return Err(SkillStoreError::Stale);
    }
    tx.execute(
        "INSERT INTO thread_skill_pins \
         (thread_id, scope, canonical_root, root_dev, root_ino, name, approved_sha256, \
          source_label, pinned_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
         ON CONFLICT(thread_id, name) DO UPDATE SET \
         scope = excluded.scope, canonical_root = excluded.canonical_root, \
         root_dev = excluded.root_dev, root_ino = excluded.root_ino, \
         approved_sha256 = excluded.approved_sha256, \
         source_label = excluded.source_label, pinned_at = excluded.pinned_at",
        params![
            pin.thread_id,
            pin.scope,
            pin.canonical_root,
            pin.root_dev,
            pin.root_ino,
            pin.name,
            pin.approved_sha256,
            pin.source_label,
            pin.pinned_at,
        ],
    )?;
    bump_generations(&tx, false)?;
    tx.commit()?;
    Ok(())
}

/// Remove one explicit pin. This revokes an active run that depended on it.
pub fn clear_thread_pin(
    conn: &Connection,
    expected_consent_generation: u64,
    thread_id: &str,
    name: &str,
) -> Result<bool, SkillStoreError> {
    let _permit =
        crate::write_coordination::acquire(conn, crate::write_coordination::WriteCategory::Skills)?;
    if !valid_id(thread_id) || !valid_name(name) {
        return Err(SkillStoreError::InvalidInput);
    }
    let tx = crate::write_coordination::WriteTransaction::new(
        conn,
        crate::write_coordination::WriteCategory::Skills,
    )?;
    ensure_generation(&tx, expected_consent_generation)?;
    let deleted = tx.execute(
        "DELETE FROM thread_skill_pins WHERE thread_id = ?1 AND name = ?2",
        params![thread_id, name],
    )? > 0;
    if deleted {
        bump_generations(&tx, true)?;
    }
    tx.commit()?;
    Ok(deleted)
}

/// List exact pins in stable name order, including now-unavailable sources.
pub fn list_thread_pins(
    conn: &Connection,
    thread_id: &str,
) -> Result<Vec<ThreadSkillPin>, SkillStoreError> {
    if !valid_id(thread_id) {
        return Err(SkillStoreError::InvalidInput);
    }
    let mut stmt = conn.prepare(
        "SELECT thread_id, scope, canonical_root, root_dev, root_ino, name, approved_sha256, \
         source_label, pinned_at FROM thread_skill_pins WHERE thread_id = ?1 \
         ORDER BY name COLLATE BINARY",
    )?;
    Ok(stmt
        .query_map([thread_id], pin_from_row)?
        .collect::<Result<Vec<_>, _>>()?)
}

fn pin_from_row(row: &Row<'_>) -> Result<ThreadSkillPin, rusqlite::Error> {
    Ok(ThreadSkillPin {
        thread_id: row.get(0)?,
        scope: row.get(1)?,
        canonical_root: row.get(2)?,
        root_dev: row.get(3)?,
        root_ino: row.get(4)?,
        name: row.get(5)?,
        approved_sha256: row.get(6)?,
        source_label: row.get(7)?,
        pinned_at: row.get(8)?,
    })
}

/// Content-free audit projection. Raw body, reference, absolute path and tool
/// arguments are intentionally absent from this type and its SQL table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillActivationAuditRecord {
    pub id: i64,
    pub run_id: String,
    pub thread_id: String,
    pub name: String,
    pub source_scope: Option<String>,
    pub content_sha256: Option<String>,
    pub origin: String,
    pub status: String,
    pub created_at: i64,
}

/// Trusted controller's bounded, content-free audit fields.
pub struct NewSkillActivationAudit<'a> {
    pub run_id: &'a str,
    pub thread_id: &'a str,
    pub name: &'a str,
    pub source_scope: Option<&'a str>,
    pub content_sha256: Option<&'a str>,
    pub origin: &'a str,
    pub status: &'a str,
    pub created_at: i64,
}

/// Append one lower-trust Skill activation outcome without content or paths.
pub fn append_activation_audit(
    conn: &Connection,
    input: NewSkillActivationAudit<'_>,
) -> Result<(), SkillStoreError> {
    let _permit =
        crate::write_coordination::acquire(conn, crate::write_coordination::WriteCategory::Skills)?;
    if !valid_id(input.run_id)
        || !valid_id(input.thread_id)
        || !valid_name(input.name)
        || input
            .source_scope
            .is_some_and(|scope| !matches!(scope, "project" | "vega_global" | "imported"))
        || input.content_sha256.is_some_and(|hash| !valid_hash(hash))
        || !matches!(input.origin, "model" | "explicit_user")
        || !valid_status(input.status)
    {
        return Err(SkillStoreError::InvalidInput);
    }
    conn.execute(
        "INSERT INTO skill_activation_audits \
         (run_id, thread_id, name, source_scope, content_sha256, origin, status, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            input.run_id,
            input.thread_id,
            input.name,
            input.source_scope,
            input.content_sha256,
            input.origin,
            input.status,
            input.created_at,
        ],
    )?;
    Ok(())
}

/// Idempotent terminal marker for one already-persisted activation. It copies
/// the original model/user origin and contains no body, private path or tool
/// input. A restarted observer may safely attempt the same marker again.
pub struct NewSkillRevocationAudit<'a> {
    pub run_id: &'a str,
    pub thread_id: &'a str,
    pub name: &'a str,
    pub source_scope: &'a str,
    pub content_sha256: &'a str,
    pub created_at: i64,
}

pub fn append_revocation_audit_once(
    conn: &Connection,
    input: NewSkillRevocationAudit<'_>,
) -> Result<bool, SkillStoreError> {
    let _permit =
        crate::write_coordination::acquire(conn, crate::write_coordination::WriteCategory::Skills)?;
    if !valid_id(input.run_id)
        || !valid_id(input.thread_id)
        || !valid_name(input.name)
        || !matches!(input.source_scope, "project" | "vega_global" | "imported")
        || !valid_hash(input.content_sha256)
    {
        return Err(SkillStoreError::InvalidInput);
    }
    let changed = conn.execute(
        "INSERT INTO skill_activation_audits \
         (run_id, thread_id, name, source_scope, content_sha256, origin, status, created_at) \
         SELECT loaded.run_id, loaded.thread_id, loaded.name, loaded.source_scope, \
                loaded.content_sha256, loaded.origin, 'revoked', ?1 \
         FROM skill_activation_audits AS loaded \
         WHERE loaded.run_id = ?2 AND loaded.thread_id = ?3 AND loaded.name = ?4 \
           AND loaded.source_scope = ?5 AND loaded.content_sha256 = ?6 \
           AND loaded.status = 'loaded' \
           AND NOT EXISTS ( \
             SELECT 1 FROM skill_activation_audits AS prior \
             WHERE prior.run_id = loaded.run_id AND prior.thread_id = loaded.thread_id \
               AND prior.name = loaded.name AND prior.source_scope = loaded.source_scope \
               AND prior.content_sha256 = loaded.content_sha256 AND prior.status = 'revoked' \
           ) \
         ORDER BY loaded.id DESC LIMIT 1",
        params![
            input.created_at,
            input.run_id,
            input.thread_id,
            input.name,
            input.source_scope,
            input.content_sha256,
        ],
    )?;
    Ok(changed == 1)
}

/// Bounded, content-free recent audit history for the visible indicator.
pub fn list_activation_audits(
    conn: &Connection,
    thread_id: &str,
) -> Result<Vec<SkillActivationAuditRecord>, SkillStoreError> {
    if !valid_id(thread_id) {
        return Err(SkillStoreError::InvalidInput);
    }
    let mut stmt = conn.prepare(
        "SELECT id, run_id, thread_id, name, source_scope, content_sha256, origin, status, \
         created_at FROM skill_activation_audits WHERE thread_id = ?1 \
         ORDER BY id DESC LIMIT 1000",
    )?;
    let mut audits = stmt
        .query_map([thread_id], audit_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    audits.reverse();
    Ok(audits)
}

/// One bounded read snapshot for a transcript page. Audit rows remain visible
/// after consent changes; only snapshots with a current authority generation,
/// valid external digest and an aggregate byte budget are offered for runtime
/// validation. The caller must never treat an audit alone as verified content.
pub struct SkillHistoryPageRecords {
    pub audits: Vec<SkillActivationAuditRecord>,
    pub recoverable_snapshots: Vec<SkillSnapshotRecord>,
}

pub fn load_history_page_records(
    conn: &Connection,
    thread_id: &str,
    oldest_seq: i64,
    newest_seq: i64,
) -> Result<SkillHistoryPageRecords, SkillStoreError> {
    const MAX_AUDITS: usize = 1_200;
    const MAX_SNAPSHOT_BYTES: i64 = 8 * 1024 * 1024;
    if !valid_id(thread_id) || oldest_seq <= 0 || newest_seq < oldest_seq {
        return Err(SkillStoreError::InvalidInput);
    }
    let tx = conn.unchecked_transaction()?;
    let settings = read_settings(&tx)?;
    let mut audit_stmt = tx.prepare(
        "SELECT a.id, a.run_id, a.thread_id, a.name, a.source_scope, \
         a.content_sha256, a.origin, a.status, a.created_at \
         FROM skill_activation_audits a JOIN messages m \
           ON m.id = a.run_id AND m.thread_id = a.thread_id \
         WHERE a.thread_id = ?1 AND m.role = 'assistant' AND m.kind IN ('text', 'plan') \
           AND m.seq BETWEEN ?2 AND ?3 AND a.status IN ('loaded', 'revoked') \
         ORDER BY m.seq, a.id LIMIT ?4",
    )?;
    let audits = audit_stmt
        .query_map(
            params![thread_id, oldest_seq, newest_seq, 1_201_i64],
            audit_from_row,
        )?
        .collect::<Result<Vec<_>, _>>()?;
    drop(audit_stmt);
    if audits.len() > MAX_AUDITS {
        return Err(SkillStoreError::TooLarge);
    }
    // Window over lengths only: private BLOBs are materialized for at most
    // 8 MiB of runs in one SQL statement, never one query per message.
    let mut snapshot_stmt = tx.prepare(
        "WITH eligible AS ( \
           SELECT s.run_id, m.seq, \
             SUM(length(s.bytes)) OVER (ORDER BY m.seq, s.run_id) AS total_bytes \
           FROM skill_run_snapshots s JOIN messages m \
             ON m.id = s.run_id AND m.thread_id = s.thread_id \
           WHERE s.thread_id = ?1 AND m.role = 'assistant' AND m.kind IN ('text', 'plan') \
             AND m.seq BETWEEN ?2 AND ?3 \
             AND length(s.bytes) BETWEEN 1 AND 1048576 \
             AND EXISTS (SELECT 1 FROM skill_activation_audits a \
                         WHERE a.run_id = s.run_id AND a.thread_id = s.thread_id \
                           AND a.status = 'loaded') \
         ) \
         SELECT s.run_id, s.thread_id, s.consent_generation, \
           s.revocation_generation, s.catalog_sha256, s.snapshot_sha256, \
           s.bytes, s.updated_at \
         FROM eligible e JOIN skill_run_snapshots s ON s.run_id = e.run_id \
         WHERE e.total_bytes <= ?4 ORDER BY e.seq, e.run_id",
    )?;
    let rows = snapshot_stmt
        .query_map(
            params![thread_id, oldest_seq, newest_seq, MAX_SNAPSHOT_BYTES],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, Vec<u8>>(6)?,
                    row.get::<_, i64>(7)?,
                ))
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;
    drop(snapshot_stmt);
    let mut recoverable_snapshots = Vec::with_capacity(rows.len());
    for (run_id, thread_id, consent, revoked, catalog, hash, bytes, updated_at) in rows {
        let (Ok(consent_generation), Ok(revocation_generation)) =
            (u64::try_from(consent), u64::try_from(revoked))
        else {
            continue;
        };
        if consent_generation != settings.consent_generation
            || revocation_generation != settings.revocation_generation
            || !valid_hash(&catalog)
            || !valid_hash(&hash)
            || digest(&bytes) != hash
        {
            continue;
        }
        recoverable_snapshots.push(SkillSnapshotRecord {
            run_id,
            thread_id,
            consent_generation,
            revocation_generation,
            catalog_sha256: catalog,
            snapshot_sha256: hash,
            bytes,
            updated_at,
        });
    }
    tx.commit()?;
    Ok(SkillHistoryPageRecords {
        audits,
        recoverable_snapshots,
    })
}

fn audit_from_row(row: &Row<'_>) -> Result<SkillActivationAuditRecord, rusqlite::Error> {
    Ok(SkillActivationAuditRecord {
        id: row.get(0)?,
        run_id: row.get(1)?,
        thread_id: row.get(2)?,
        name: row.get(3)?,
        source_scope: row.get(4)?,
        content_sha256: row.get(5)?,
        origin: row.get(6)?,
        status: row.get(7)?,
        created_at: row.get(8)?,
    })
}

fn valid_status(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
