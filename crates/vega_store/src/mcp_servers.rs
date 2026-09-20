//! Issue #73 MCP configuration metadata. Secret values are never stored here.
//!
//! Every mutation uses a configuration revision. Replacing a server disables
//! it, so a stale enabled row cannot acquire an edited endpoint or command.
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_ENABLED_SERVERS: i64 = 8;

/// SQLite representation, not a UI or runtime capability DTO.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerRow {
    pub id: String,
    pub display_name: String,
    pub transport: String,
    pub local_executable: Option<String>,
    pub local_args_json: Option<String>,
    pub local_working_directory: Option<String>,
    pub local_env_refs_json: Option<String>,
    pub remote_endpoint: Option<String>,
    pub remote_allow_loopback_http: bool,
    pub remote_auth_mode: Option<String>,
    pub remote_credential_ref: Option<String>,
    pub remote_oauth_issuer: Option<String>,
    pub remote_oauth_client_id: Option<String>,
    pub enabled: bool,
    pub deleting: bool,
    pub config_revision: u64,
    pub last_error_code: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A full settings form edit. The caller validates transport-specific JSON
/// and URLs before passing this content-free configuration to SQLite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerDraft {
    pub display_name: String,
    pub transport: String,
    pub local_executable: Option<String>,
    pub local_args_json: Option<String>,
    pub local_working_directory: Option<String>,
    pub local_env_refs_json: Option<String>,
    pub remote_endpoint: Option<String>,
    pub remote_allow_loopback_http: bool,
    pub remote_auth_mode: Option<String>,
    pub remote_credential_ref: Option<String>,
    pub remote_oauth_issuer: Option<String>,
    pub remote_oauth_client_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct McpEnvSlot {
    pub variable: String,
    pub credential_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpSecretCleanup {
    pub server_id: String,
    pub credential_ref: String,
}

#[derive(Debug, thiserror::Error)]
pub enum McpStoreError {
    #[error("MCP configuration is invalid")]
    Invalid,
    #[error("MCP configuration changed; reload before editing")]
    Conflict,
    #[error("MCP server not found")]
    NotFound,
    #[error("at most eight MCP servers may be enabled")]
    Capacity,
    #[error("MCP configuration storage failed")]
    Sql(#[from] rusqlite::Error),
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

fn validate(draft: &McpServerDraft) -> Result<(), McpStoreError> {
    if draft.display_name.trim().is_empty()
        || draft.display_name.len() > 128
        || draft.display_name.chars().any(char::is_control)
    {
        return Err(McpStoreError::Invalid);
    }
    let small = |value: &Option<String>, cap: usize| value.as_ref().is_none_or(|v| v.len() <= cap);
    if !small(&draft.local_executable, 4096)
        || !small(&draft.local_args_json, 64 * 1024)
        || !small(&draft.local_working_directory, 4096)
        || !small(&draft.local_env_refs_json, 16 * 1024)
        || !small(&draft.remote_endpoint, 4096)
        || !small(&draft.remote_credential_ref, 256)
        || !small(&draft.remote_oauth_issuer, 4096)
        || !small(&draft.remote_oauth_client_id, 512)
    {
        return Err(McpStoreError::Invalid);
    }
    match draft.transport.as_str() {
        "local"
            if draft
                .local_executable
                .as_ref()
                .is_some_and(|s| !s.is_empty())
                && draft.local_args_json.is_some()
                && draft.local_env_refs_json.is_some()
                && draft.remote_endpoint.is_none()
                && draft.remote_auth_mode.is_none()
                && draft.remote_credential_ref.is_none()
                && draft.remote_oauth_issuer.is_none()
                && draft.remote_oauth_client_id.is_none()
                && !draft.remote_allow_loopback_http => {}
        "remote"
            if draft
                .remote_endpoint
                .as_ref()
                .is_some_and(|s| !s.is_empty())
                && matches!(
                    draft.remote_auth_mode.as_deref(),
                    Some("none" | "bearer" | "oauth")
                )
                && draft.local_executable.is_none()
                && draft.local_args_json.is_none()
                && draft.local_working_directory.is_none()
                && draft.local_env_refs_json.is_none() => {}
        _ => return Err(McpStoreError::Invalid),
    }
    if draft.remote_credential_ref.is_some() {
        return Err(McpStoreError::Invalid);
    }
    if draft.transport == "local" {
        let _: Vec<String> = env_names(
            draft
                .local_env_refs_json
                .as_deref()
                .ok_or(McpStoreError::Invalid)?,
        )?;
    }
    Ok(())
}

fn env_names(json: &str) -> Result<Vec<String>, McpStoreError> {
    let names: Vec<String> = serde_json::from_str(json).map_err(|_| McpStoreError::Invalid)?;
    if names.len() > 32 {
        return Err(McpStoreError::Invalid);
    }
    let mut seen = HashSet::new();
    for name in &names {
        if name.is_empty()
            || name.len() > 128
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            || !seen.insert(name)
        {
            return Err(McpStoreError::Invalid);
        }
    }
    Ok(names)
}

fn local_slots(
    id: &str,
    revision: u64,
    draft: &McpServerDraft,
) -> Result<Option<String>, McpStoreError> {
    let Some(json) = &draft.local_env_refs_json else {
        return Ok(None);
    };
    let slots: Vec<_> = env_names(json)?
        .into_iter()
        .map(|variable| McpEnvSlot {
            credential_ref: format!("mcp-{id}-r{revision}-env-{variable}"),
            variable,
        })
        .collect();
    serde_json::to_string(&slots)
        .map(Some)
        .map_err(|_| McpStoreError::Invalid)
}

fn server_secret_refs(row: &McpServerRow) -> Result<HashSet<String>, McpStoreError> {
    let mut refs = HashSet::new();
    if let Some(reference) = &row.remote_credential_ref {
        let prefix = format!("mcp-{}-r", row.id);
        let suffix = match row.remote_auth_mode.as_deref() {
            Some("bearer") => "-bearer",
            Some("oauth") => "-oauth",
            _ => return Err(McpStoreError::Invalid),
        };
        let generation = reference
            .strip_prefix(&prefix)
            .and_then(|value| value.strip_suffix(suffix))
            .and_then(|value| value.parse::<u64>().ok());
        if generation.is_none_or(|value| value == 0 || value > row.config_revision) {
            return Err(McpStoreError::Invalid);
        }
        refs.insert(reference.clone());
    }
    if let Some(json) = &row.local_env_refs_json {
        let slots: Vec<McpEnvSlot> =
            serde_json::from_str(json).map_err(|_| McpStoreError::Invalid)?;
        for slot in slots {
            let prefix = format!("mcp-{}-r", row.id);
            let generation = slot
                .credential_ref
                .strip_prefix(&prefix)
                .and_then(|value| value.strip_suffix(&format!("-env-{}", slot.variable)))
                .and_then(|value| value.parse::<u64>().ok());
            if generation.is_none_or(|value| value == 0 || value > row.config_revision) {
                return Err(McpStoreError::Invalid);
            }
            refs.insert(slot.credential_ref);
        }
    }
    Ok(refs)
}

fn decode_row(row: &Row<'_>) -> rusqlite::Result<McpServerRow> {
    let revision: i64 = row.get(15)?;
    let config_revision = u64::try_from(revision)
        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(15, revision))?;
    Ok(McpServerRow {
        id: row.get(0)?,
        display_name: row.get(1)?,
        transport: row.get(2)?,
        local_executable: row.get(3)?,
        local_args_json: row.get(4)?,
        local_working_directory: row.get(5)?,
        local_env_refs_json: row.get(6)?,
        remote_endpoint: row.get(7)?,
        remote_allow_loopback_http: row.get::<_, i64>(8)? != 0,
        remote_auth_mode: row.get(9)?,
        remote_credential_ref: row.get(10)?,
        remote_oauth_issuer: row.get(11)?,
        remote_oauth_client_id: row.get(12)?,
        enabled: row.get::<_, i64>(13)? != 0,
        deleting: row.get::<_, i64>(14)? != 0,
        config_revision,
        last_error_code: row.get(16)?,
        created_at: row.get(17)?,
        updated_at: row.get(18)?,
    })
}

const SELECT: &str = "SELECT id, display_name, transport, local_executable, local_args_json, \
    local_working_directory, local_env_refs_json, remote_endpoint, remote_allow_loopback_http, \
    remote_auth_mode, remote_credential_ref, remote_oauth_issuer, remote_oauth_client_id, enabled, \
    deleting, config_revision, last_error_code, created_at, updated_at FROM mcp_servers";

/// Insert a disabled server. Merely saving a draft never launches a process.
pub fn create(conn: &Connection, draft: &McpServerDraft) -> Result<McpServerRow, McpStoreError> {
    validate(draft)?;
    let id = ulid::Ulid::generate().to_string();
    let credential_ref =
        if draft.transport == "remote" && draft.remote_auth_mode.as_deref() == Some("bearer") {
            Some(format!("mcp-{id}-r1-bearer"))
        } else {
            None
        };
    let environment_slots = local_slots(&id, 1, draft)?;
    let now = now_ms();
    conn.execute(
        "INSERT INTO mcp_servers (id, display_name, transport, local_executable, local_args_json, \
         local_working_directory, local_env_refs_json, remote_endpoint, remote_allow_loopback_http, \
         remote_auth_mode, remote_credential_ref, remote_oauth_issuer, remote_oauth_client_id, \
         enabled, config_revision, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 0, 1, ?14, ?14)",
        params![
            id, draft.display_name, draft.transport, draft.local_executable,
            draft.local_args_json, draft.local_working_directory, environment_slots,
            draft.remote_endpoint, draft.remote_allow_loopback_http, draft.remote_auth_mode,
            credential_ref, draft.remote_oauth_issuer, draft.remote_oauth_client_id,
            now,
        ],
    )?;
    find(conn, &id)?.ok_or(McpStoreError::NotFound)
}

pub fn find(conn: &Connection, id: &str) -> Result<Option<McpServerRow>, McpStoreError> {
    let sql = format!("{SELECT} WHERE id = ?1");
    Ok(conn.query_row(&sql, [id], decode_row).optional()?)
}

pub fn list(conn: &Connection) -> Result<Vec<McpServerRow>, McpStoreError> {
    let sql = format!("{SELECT} ORDER BY created_at, id");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    let mut servers = Vec::new();
    while let Some(row) = rows.next()? {
        servers.push(decode_row(row)?);
    }
    Ok(servers)
}

/// Replace a complete form after exact revision comparison. An edit always
/// disables the server; the user must re-enable after testing the new config.
pub fn replace(
    conn: &Connection,
    id: &str,
    expected_revision: u64,
    draft: &McpServerDraft,
) -> Result<McpServerRow, McpStoreError> {
    validate(draft)?;
    let expected = i64::try_from(expected_revision).map_err(|_| McpStoreError::Invalid)?;
    let next_revision = expected_revision
        .checked_add(1)
        .ok_or(McpStoreError::Invalid)?;
    let credential_ref =
        if draft.transport == "remote" && draft.remote_auth_mode.as_deref() == Some("bearer") {
            Some(format!("mcp-{id}-r{next_revision}-bearer"))
        } else {
            None
        };
    let environment_slots = local_slots(id, next_revision, draft)?;
    let tx = conn.unchecked_transaction()?;
    let old = find(&tx, id)?.ok_or(McpStoreError::NotFound)?;
    if old.config_revision != expected_revision || old.deleting {
        return Err(McpStoreError::Conflict);
    }
    let updated = tx.execute(
        "UPDATE mcp_servers SET display_name = ?1, transport = ?2, local_executable = ?3, \
         local_args_json = ?4, local_working_directory = ?5, local_env_refs_json = ?6, \
         remote_endpoint = ?7, remote_allow_loopback_http = ?8, remote_auth_mode = ?9, \
         remote_credential_ref = ?10, remote_oauth_issuer = ?11, remote_oauth_client_id = ?12, \
         enabled = 0, config_revision = config_revision + 1, last_error_code = NULL, \
         updated_at = ?13 WHERE id = ?14 AND config_revision = ?15",
        params![
            draft.display_name,
            draft.transport,
            draft.local_executable,
            draft.local_args_json,
            draft.local_working_directory,
            environment_slots,
            draft.remote_endpoint,
            draft.remote_allow_loopback_http,
            draft.remote_auth_mode,
            credential_ref,
            draft.remote_oauth_issuer,
            draft.remote_oauth_client_id,
            now_ms(),
            id,
            expected,
        ],
    )?;
    if updated == 0 {
        return Err(McpStoreError::Conflict);
    }
    let new = find(&tx, id)?.ok_or(McpStoreError::NotFound)?;
    let new_refs = server_secret_refs(&new)?;
    for reference in server_secret_refs(&old)?.difference(&new_refs) {
        enqueue_cleanup(&tx, id, reference)?;
    }
    tx.commit()?;
    Ok(new)
}

fn enqueue_cleanup(conn: &Connection, id: &str, reference: &str) -> Result<(), McpStoreError> {
    conn.execute(
        "INSERT OR IGNORE INTO mcp_secret_cleanup (credential_ref, server_id, created_at) \
         VALUES (?1, ?2, ?3)",
        params![reference, id, now_ms()],
    )?;
    Ok(())
}

/// Change authority only at the current revision. Enabling is bounded by the
/// documented eight-server ceiling inside one SQLite write transaction.
pub fn set_enabled(
    conn: &Connection,
    id: &str,
    expected_revision: u64,
    enabled: bool,
) -> Result<McpServerRow, McpStoreError> {
    let expected = i64::try_from(expected_revision).map_err(|_| McpStoreError::Invalid)?;
    let tx = conn.unchecked_transaction()?;
    let current = find(&tx, id)?.ok_or(McpStoreError::NotFound)?;
    if current.config_revision != expected_revision {
        return Err(McpStoreError::Conflict);
    }
    if current.deleting {
        return Err(McpStoreError::Conflict);
    }
    if enabled {
        let pending: i64 = tx.query_row(
            "SELECT COUNT(*) FROM mcp_secret_cleanup WHERE server_id = ?1",
            [id],
            |row| row.get(0),
        )?;
        if pending > 0 {
            return Err(McpStoreError::Conflict);
        }
    }
    if enabled && !current.enabled {
        let count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM mcp_servers WHERE enabled = 1",
            [],
            |row| row.get(0),
        )?;
        if count >= MAX_ENABLED_SERVERS {
            return Err(McpStoreError::Capacity);
        }
    }
    tx.execute(
        "UPDATE mcp_servers SET enabled = ?1, config_revision = config_revision + 1, \
         last_error_code = NULL, updated_at = ?2 WHERE id = ?3 AND config_revision = ?4",
        params![enabled, now_ms(), id, expected],
    )?;
    let changed = find(&tx, id)?.ok_or(McpStoreError::NotFound)?;
    tx.commit()?;
    Ok(changed)
}

/// Begin an OAuth credential install with an exact CAS. This commits a new
/// disabled revision and owner-only slot before Settings writes any token.
/// Old tokens are queued for recoverable deletion in the same transaction.
pub fn begin_oauth_install(
    conn: &Connection,
    id: &str,
    expected_revision: u64,
    issuer: &str,
    client_id: &str,
) -> Result<(McpServerRow, String), McpStoreError> {
    let expected = i64::try_from(expected_revision).map_err(|_| McpStoreError::Invalid)?;
    if issuer.is_empty()
        || issuer.len() > 4096
        || issuer.chars().any(char::is_control)
        || client_id.is_empty()
        || client_id.len() > 512
        || client_id.chars().any(char::is_control)
    {
        return Err(McpStoreError::Invalid);
    }
    let next_revision = expected_revision
        .checked_add(1)
        .ok_or(McpStoreError::Invalid)?;
    let reference = format!("mcp-{id}-r{next_revision}-oauth");
    let tx = conn.unchecked_transaction()?;
    let old = find(&tx, id)?.ok_or(McpStoreError::NotFound)?;
    if old.config_revision != expected_revision
        || old.deleting
        || old.transport != "remote"
        || old.remote_auth_mode.as_deref() != Some("oauth")
    {
        return Err(McpStoreError::Conflict);
    }
    tx.execute(
        "UPDATE mcp_servers SET enabled = 0, config_revision = config_revision + 1, \
         remote_credential_ref = ?1, remote_oauth_issuer = ?2, remote_oauth_client_id = ?3, \
         last_error_code = NULL, updated_at = ?4 WHERE id = ?5 AND config_revision = ?6",
        params![reference, issuer, client_id, now_ms(), id, expected],
    )?;
    let changed = find(&tx, id)?.ok_or(McpStoreError::NotFound)?;
    let new_refs = server_secret_refs(&changed)?;
    for old_ref in server_secret_refs(&old)?.difference(&new_refs) {
        enqueue_cleanup(&tx, id, old_ref)?;
    }
    tx.commit()?;
    Ok((changed, reference))
}

/// Revoke a remote OAuth binding without changing the server form. A failed
/// keystore deletion leaves a disabled row and durable cleanup outbox.
pub fn begin_oauth_disconnect(
    conn: &Connection,
    id: &str,
    expected_revision: u64,
) -> Result<McpServerRow, McpStoreError> {
    let expected = i64::try_from(expected_revision).map_err(|_| McpStoreError::Invalid)?;
    let tx = conn.unchecked_transaction()?;
    let old = find(&tx, id)?.ok_or(McpStoreError::NotFound)?;
    if old.config_revision != expected_revision
        || old.deleting
        || old.transport != "remote"
        || old.remote_auth_mode.as_deref() != Some("oauth")
    {
        return Err(McpStoreError::Conflict);
    }
    tx.execute(
        "UPDATE mcp_servers SET enabled = 0, config_revision = config_revision + 1, \
         remote_credential_ref = NULL, remote_oauth_issuer = NULL, last_error_code = NULL, \
         updated_at = ?1 WHERE id = ?2 AND config_revision = ?3",
        params![now_ms(), id, expected],
    )?;
    for old_ref in server_secret_refs(&old)? {
        enqueue_cleanup(&tx, id, &old_ref)?;
    }
    let changed = find(&tx, id)?.ok_or(McpStoreError::NotFound)?;
    tx.commit()?;
    Ok(changed)
}

/// Rotate one local variable using a server-owned slot. First revoke and
/// durably queue old secret deletion; the caller may install the new value
/// only after the queue is acknowledged. A crash cannot reuse the old value.
pub fn begin_local_secret_rotation(
    conn: &Connection,
    id: &str,
    expected_revision: u64,
    variable: &str,
) -> Result<(McpServerRow, String), McpStoreError> {
    let tx = conn.unchecked_transaction()?;
    let old = find(&tx, id)?.ok_or(McpStoreError::NotFound)?;
    if old.config_revision != expected_revision || old.deleting || old.transport != "local" {
        return Err(McpStoreError::Conflict);
    }
    let slots: Vec<McpEnvSlot> = serde_json::from_str(
        old.local_env_refs_json
            .as_deref()
            .ok_or(McpStoreError::Invalid)?,
    )
    .map_err(|_| McpStoreError::Invalid)?;
    let old_reference = slots
        .iter()
        .find(|slot| slot.variable == variable)
        .map(|slot| slot.credential_ref.clone())
        .ok_or(McpStoreError::Invalid)?;
    if !server_secret_refs(&old)?.contains(&old_reference) {
        return Err(McpStoreError::Invalid);
    }
    let next_revision = expected_revision
        .checked_add(1)
        .ok_or(McpStoreError::Invalid)?;
    let reference = format!("mcp-{id}-r{next_revision}-env-{variable}");
    let next_slots: Vec<_> = slots
        .into_iter()
        .map(|mut slot| {
            if slot.variable == variable {
                slot.credential_ref = reference.clone();
            }
            slot
        })
        .collect();
    let next_slots_json = serde_json::to_string(&next_slots).map_err(|_| McpStoreError::Invalid)?;
    tx.execute(
        "UPDATE mcp_servers SET enabled = 0, config_revision = config_revision + 1, \
         local_env_refs_json = ?1, updated_at = ?2 WHERE id = ?3",
        params![next_slots_json, now_ms(), id],
    )?;
    enqueue_cleanup(&tx, id, &old_reference)?;
    let changed = find(&tx, id)?.ok_or(McpStoreError::NotFound)?;
    tx.commit()?;
    Ok((changed, reference))
}

/// Set a content-free connection error code only if the observed revision is
/// still current. A late test cannot mark a newly edited server unhealthy.
pub fn set_last_error(
    conn: &Connection,
    id: &str,
    revision: u64,
    code: Option<&str>,
) -> Result<(), McpStoreError> {
    if code.is_some_and(|code| {
        code.is_empty()
            || code.len() > 64
            || !code
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
    }) {
        return Err(McpStoreError::Invalid);
    }
    let revision = i64::try_from(revision).map_err(|_| McpStoreError::Invalid)?;
    let updated = conn.execute(
        "UPDATE mcp_servers SET last_error_code = ?1 WHERE id = ?2 AND config_revision = ?3",
        params![code, id, revision],
    )?;
    if updated == 0 {
        return Err(McpStoreError::Conflict);
    }
    Ok(())
}

/// Start a recoverable removal. This commits a disabled tombstone and enqueues
/// every owned secret before the caller touches owner-only credential files.
pub fn begin_remove(
    conn: &Connection,
    id: &str,
    expected_revision: u64,
) -> Result<McpServerRow, McpStoreError> {
    let tx = conn.unchecked_transaction()?;
    let row = find(&tx, id)?.ok_or(McpStoreError::NotFound)?;
    if row.deleting {
        return Ok(row);
    }
    if row.config_revision != expected_revision {
        return Err(McpStoreError::Conflict);
    }
    tx.execute(
        "UPDATE mcp_servers SET enabled = 0, deleting = 1, \
         config_revision = config_revision + 1, updated_at = ?1 WHERE id = ?2",
        params![now_ms(), id],
    )?;
    for reference in server_secret_refs(&row)? {
        enqueue_cleanup(&tx, id, &reference)?;
    }
    let changed = find(&tx, id)?.ok_or(McpStoreError::NotFound)?;
    tx.commit()?;
    Ok(changed)
}

/// Pending cleanup survives process restart; list only references and IDs.
pub fn pending_cleanup(conn: &Connection) -> Result<Vec<McpSecretCleanup>, McpStoreError> {
    let mut stmt = conn.prepare(
        "SELECT server_id, credential_ref FROM mcp_secret_cleanup ORDER BY created_at, credential_ref",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(McpSecretCleanup {
            server_id: row.get(0)?,
            credential_ref: row.get(1)?,
        })
    })?;
    rows.collect::<Result<_, _>>().map_err(McpStoreError::from)
}

/// The cleanup worker must not trust a writable DB row as authority to erase
/// an arbitrary owner-only credential. It may delete a current slot only for
/// a disabled removal tombstone; ordinary edits may delete stale slots only.
pub fn cleanup_target_is_stale(
    conn: &Connection,
    entry: &McpSecretCleanup,
) -> Result<bool, McpStoreError> {
    let Some(current) = find(conn, &entry.server_id)? else {
        return Ok(false);
    };
    let prefix = format!("mcp-{}-r", entry.server_id);
    let generation = entry
        .credential_ref
        .strip_prefix(&prefix)
        .and_then(|value| value.split_once('-'))
        .and_then(|(revision, _)| revision.parse::<u64>().ok());
    let Some(generation) = generation else {
        return Ok(false);
    };
    let current_refs = server_secret_refs(&current)?;
    Ok(generation > 0
        && generation <= current.config_revision
        && (current.deleting
            || (generation < current.config_revision
                && !current_refs.contains(&entry.credential_ref))))
}

pub fn acknowledge_cleanup(
    conn: &Connection,
    entry: &McpSecretCleanup,
) -> Result<(), McpStoreError> {
    conn.execute(
        "DELETE FROM mcp_secret_cleanup WHERE server_id = ?1 AND credential_ref = ?2",
        params![entry.server_id, entry.credential_ref],
    )?;
    Ok(())
}

/// Erase a tombstone only after every queued secret has been removed. No
/// history or tool-call audit row is touched.
pub fn finish_remove(conn: &Connection, id: &str) -> Result<bool, McpStoreError> {
    let count = conn.execute(
        "DELETE FROM mcp_servers WHERE id = ?1 AND deleting = 1 \
         AND NOT EXISTS (SELECT 1 FROM mcp_secret_cleanup WHERE server_id = ?1)",
        [id],
    )?;
    Ok(count == 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;

    fn local(name: &str) -> McpServerDraft {
        McpServerDraft {
            display_name: name.into(),
            transport: "local".into(),
            local_executable: Some("/bin/echo".into()),
            local_args_json: Some("[]".into()),
            local_working_directory: None,
            local_env_refs_json: Some("[]".into()),
            remote_endpoint: None,
            remote_allow_loopback_http: false,
            remote_auth_mode: None,
            remote_credential_ref: None,
            remote_oauth_issuer: None,
            remote_oauth_client_id: None,
        }
    }

    fn bearer(name: &str) -> McpServerDraft {
        McpServerDraft {
            display_name: name.into(),
            transport: "remote".into(),
            local_executable: None,
            local_args_json: None,
            local_working_directory: None,
            local_env_refs_json: None,
            remote_endpoint: Some("https://example.com/mcp".into()),
            remote_allow_loopback_http: false,
            remote_auth_mode: Some("bearer".into()),
            remote_credential_ref: None,
            remote_oauth_issuer: None,
            remote_oauth_client_id: None,
        }
    }

    fn oauth(name: &str) -> McpServerDraft {
        let mut draft = bearer(name);
        draft.remote_auth_mode = Some("oauth".into());
        draft.remote_oauth_client_id = Some("pre-registered-client".into());
        draft
    }

    #[test]
    fn issue73_oauth_install_disconnect_cas_and_outbox_never_target_new_revision() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("vega.db");
        let (id, first_ref) = {
            let store = Store::open(&path).unwrap();
            store.migrate().unwrap();
            let created = create(store.conn(), &oauth("owned")).unwrap();
            assert!(created.remote_credential_ref.is_none());
            let (installed, first_ref) = begin_oauth_install(
                store.conn(),
                &created.id,
                created.config_revision,
                "https://issuer.example",
                "pre-registered-client",
            )
            .unwrap();
            assert_eq!(installed.config_revision, 2);
            assert!(!installed.enabled);
            assert_eq!(
                installed.remote_credential_ref.as_deref(),
                Some(first_ref.as_str())
            );
            assert!(matches!(
                begin_oauth_install(
                    store.conn(),
                    &created.id,
                    1,
                    "https://issuer.example",
                    "client"
                ),
                Err(McpStoreError::Conflict)
            ));
            (created.id, first_ref)
        };
        let store = Store::open(&path).unwrap();
        store.migrate().unwrap();
        let (rotated, second_ref) =
            begin_oauth_install(store.conn(), &id, 2, "https://issuer.example", "client-2")
                .unwrap();
        assert_ne!(first_ref, second_ref);
        let queued = pending_cleanup(store.conn()).unwrap();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].credential_ref, first_ref);
        assert!(cleanup_target_is_stale(store.conn(), &queued[0]).unwrap());
        assert_ne!(
            queued[0].credential_ref,
            rotated.remote_credential_ref.unwrap()
        );
        acknowledge_cleanup(store.conn(), &queued[0]).unwrap();
        let disconnected = begin_oauth_disconnect(store.conn(), &id, 3).unwrap();
        assert_eq!(disconnected.config_revision, 4);
        assert!(!disconnected.enabled);
        assert!(disconnected.remote_credential_ref.is_none());
        let queued = pending_cleanup(store.conn()).unwrap();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].credential_ref, second_ref);
        assert!(cleanup_target_is_stale(store.conn(), &queued[0]).unwrap());
    }

    #[test]
    fn issue73_config_starts_disabled_survives_restart_and_uses_revision_cas() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("vega.db");
        let id = {
            let store = Store::open(&path).unwrap();
            store.migrate().unwrap();
            let saved = create(store.conn(), &local("owned")).unwrap();
            assert!(!saved.enabled);
            assert_eq!(saved.config_revision, 1);
            saved.id
        };
        let store = Store::open(&path).unwrap();
        store.migrate().unwrap();
        let enabled = set_enabled(store.conn(), &id, 1, true).unwrap();
        assert_eq!(enabled.config_revision, 2);
        assert!(enabled.enabled);
        assert!(matches!(
            set_enabled(store.conn(), &id, 1, false),
            Err(McpStoreError::Conflict)
        ));
        let edited = replace(store.conn(), &id, 2, &local("edited")).unwrap();
        assert_eq!(edited.config_revision, 3);
        assert!(!edited.enabled);
        assert_eq!(edited.display_name, "edited");
        assert!(matches!(
            begin_remove(store.conn(), &id, 2),
            Err(McpStoreError::Conflict)
        ));
        let tombstone = begin_remove(store.conn(), &id, 3).unwrap();
        assert!(tombstone.deleting);
        assert!(!tombstone.enabled);
        assert!(finish_remove(store.conn(), &id).unwrap());
        assert!(list(store.conn()).unwrap().is_empty());
    }

    #[test]
    fn issue73_enabled_capacity_and_late_health_result_are_closed() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path().join("vega.db")).unwrap();
        store.migrate().unwrap();
        let ids: Vec<_> = (0..9)
            .map(|index| {
                create(store.conn(), &local(&format!("server-{index}")))
                    .unwrap()
                    .id
            })
            .collect();
        for id in ids.iter().take(8) {
            set_enabled(store.conn(), id, 1, true).unwrap();
        }
        assert!(matches!(
            set_enabled(store.conn(), &ids[8], 1, true),
            Err(McpStoreError::Capacity)
        ));
        let ninth = find(store.conn(), &ids[8]).unwrap().unwrap();
        assert!(!ninth.enabled);
        set_last_error(store.conn(), &ids[0], 2, Some("connection_failed")).unwrap();
        set_enabled(store.conn(), &ids[0], 2, false).unwrap();
        assert!(matches!(
            set_last_error(store.conn(), &ids[0], 2, Some("stale_error")),
            Err(McpStoreError::Conflict)
        ));
        set_enabled(store.conn(), &ids[8], 1, true).unwrap();
    }

    #[test]
    fn issue73_cross_server_or_provider_secret_ref_is_rejected_before_persistence() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path().join("vega.db")).unwrap();
        store.migrate().unwrap();
        let mut remote = bearer("remote");
        remote.remote_credential_ref = Some("provider-api-key".into());
        assert!(matches!(
            create(store.conn(), &remote),
            Err(McpStoreError::Invalid)
        ));
        let mut local = local("local");
        local.local_env_refs_json =
            Some(r#"[{"variable":"TOKEN","credential_ref":"mcp-other-env-TOKEN"}]"#.into());
        assert!(matches!(
            create(store.conn(), &local),
            Err(McpStoreError::Invalid)
        ));
        assert!(list(store.conn()).unwrap().is_empty());
    }

    #[test]
    fn issue73_replace_outbox_survives_restart_and_never_targets_new_bearer() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("vega.db");
        let (id, old_ref, new_ref) = {
            let store = Store::open(&path).unwrap();
            store.migrate().unwrap();
            let created = create(store.conn(), &bearer("old")).unwrap();
            let old_ref = created.remote_credential_ref.clone().unwrap();
            let enabled = set_enabled(store.conn(), &created.id, 1, true).unwrap();
            let changed = replace(
                store.conn(),
                &created.id,
                enabled.config_revision,
                &bearer("new"),
            )
            .unwrap();
            let new_ref = changed.remote_credential_ref.clone().unwrap();
            assert_ne!(old_ref, new_ref);
            assert!(!changed.enabled);
            assert!(matches!(
                set_enabled(store.conn(), &created.id, changed.config_revision, true),
                Err(McpStoreError::Conflict)
            ));
            (created.id, old_ref, new_ref)
        };
        let store = Store::open(&path).unwrap();
        store.migrate().unwrap();
        let queued = pending_cleanup(store.conn()).unwrap();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].credential_ref, old_ref);
        assert_ne!(queued[0].credential_ref, new_ref);
        acknowledge_cleanup(store.conn(), &queued[0]).unwrap();
        let current = find(store.conn(), &id).unwrap().unwrap();
        assert_eq!(
            current.remote_credential_ref.as_deref(),
            Some(new_ref.as_str())
        );
        assert!(set_enabled(store.conn(), &id, current.config_revision, true).is_ok());
    }

    #[test]
    fn issue73_remove_tombstone_blocks_reenable_until_cleanup_is_acknowledged() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("vega.db");
        let id = {
            let store = Store::open(&path).unwrap();
            store.migrate().unwrap();
            let created = create(store.conn(), &bearer("remove")).unwrap();
            let enabled = set_enabled(store.conn(), &created.id, 1, true).unwrap();
            let tombstone =
                begin_remove(store.conn(), &created.id, enabled.config_revision).unwrap();
            assert!(tombstone.deleting);
            assert!(!tombstone.enabled);
            assert!(matches!(
                set_enabled(store.conn(), &created.id, tombstone.config_revision, true),
                Err(McpStoreError::Conflict)
            ));
            assert!(!finish_remove(store.conn(), &created.id).unwrap());
            created.id
        };
        let store = Store::open(&path).unwrap();
        store.migrate().unwrap();
        assert!(find(store.conn(), &id).unwrap().unwrap().deleting);
        let queued = pending_cleanup(store.conn()).unwrap();
        assert_eq!(queued.len(), 1);
        acknowledge_cleanup(store.conn(), &queued[0]).unwrap();
        assert!(finish_remove(store.conn(), &id).unwrap());
        assert!(find(store.conn(), &id).unwrap().is_none());
    }
}
