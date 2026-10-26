//! Exact project-scoped permission rules over the existing `permissions` table.

use rusqlite::{Connection, OptionalExtension, params};

/// One persisted exact permission rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRule {
    /// SQLite row id.
    pub id: i64,
    /// Owning project id.
    pub project_id: String,
    /// Mutating tool name (`bash|write|edit`).
    pub tool: String,
    /// Byte-exact command or normalized relative path.
    pub pattern: String,
    /// Creation timestamp in unix milliseconds.
    pub created_at: i64,
}

/// Values accepted by [`insert_exact`].
pub struct InsertExactRule<'a> {
    /// Owning project id.
    pub project_id: &'a str,
    /// Mutating tool name (`bash|write|edit`).
    pub tool: &'a str,
    /// Byte-exact command or normalized relative path.
    pub pattern: &'a str,
    /// Creation timestamp in unix milliseconds.
    pub created_at: i64,
}

/// Result of an idempotent exact-rule insertion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InsertExactResult {
    /// The inserted or pre-existing exact rule.
    pub rule: PermissionRule,
    /// Whether this call inserted a new row.
    pub inserted: bool,
}

/// Permission-rule validation or persistence failure.
#[derive(Debug, thiserror::Error)]
pub enum PermissionsError {
    /// Empty project ids are invalid.
    #[error("permission project id must not be empty")]
    EmptyProject,
    /// Only mutating tools can be remembered.
    #[error("unsupported permission tool")]
    UnsupportedTool,
    /// Empty exact signatures are invalid.
    #[error("permission pattern must not be empty")]
    EmptyPattern,
    /// SQLite operation failed.
    #[error("permission store error: {0}")]
    Store(#[from] rusqlite::Error),
    /// An idempotent insert could not reload its row.
    #[error("inserted permission rule could not be reloaded")]
    MissingInsertedRule,
}

/// Lists a project's exact rules in stable creation order.
pub fn list_exact(
    conn: &Connection,
    project_id: &str,
) -> Result<Vec<PermissionRule>, PermissionsError> {
    if project_id.is_empty() {
        return Err(PermissionsError::EmptyProject);
    }
    let mut statement = conn.prepare(
        "SELECT id, project_id, tool, pattern, created_at FROM permissions \
         WHERE project_id = ?1 COLLATE BINARY ORDER BY created_at ASC, id ASC",
    )?;
    let rows = statement.query_map([project_id], permission_rule_from_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Returns whether a byte-exact project/tool/pattern rule exists.
pub fn matches_exact(
    conn: &Connection,
    project_id: &str,
    tool: &str,
    pattern: &str,
) -> Result<bool, PermissionsError> {
    validate(project_id, tool, pattern)?;
    let found = conn
        .query_row(
            "SELECT 1 FROM permissions WHERE project_id = ?1 COLLATE BINARY \
             AND tool = ?2 COLLATE BINARY AND pattern = ?3 COLLATE BINARY",
            params![project_id, tool, pattern],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    Ok(found.is_some())
}

/// Idempotently inserts one byte-exact permission rule.
pub fn insert_exact(
    conn: &Connection,
    input: InsertExactRule<'_>,
) -> Result<InsertExactResult, PermissionsError> {
    let _permit = crate::write_coordination::acquire(
        conn,
        crate::write_coordination::WriteCategory::Permissions,
    )?;
    validate(input.project_id, input.tool, input.pattern)?;
    let inserted = conn.execute(
        "INSERT INTO permissions (project_id, tool, pattern, created_at) \
         VALUES (?1, ?2, ?3, ?4) \
         ON CONFLICT(project_id, tool, pattern) DO NOTHING",
        params![
            input.project_id,
            input.tool,
            input.pattern,
            input.created_at
        ],
    )? == 1;
    let rule = find_exact(conn, input.project_id, input.tool, input.pattern)?
        .ok_or(PermissionsError::MissingInsertedRule)?;
    Ok(InsertExactResult { rule, inserted })
}

fn validate(project_id: &str, tool: &str, pattern: &str) -> Result<(), PermissionsError> {
    if project_id.is_empty() {
        return Err(PermissionsError::EmptyProject);
    }
    if !matches!(tool, "read" | "bash" | "write" | "edit") {
        return Err(PermissionsError::UnsupportedTool);
    }
    if pattern.is_empty() {
        return Err(PermissionsError::EmptyPattern);
    }
    Ok(())
}

fn find_exact(
    conn: &Connection,
    project_id: &str,
    tool: &str,
    pattern: &str,
) -> Result<Option<PermissionRule>, rusqlite::Error> {
    conn.query_row(
        "SELECT id, project_id, tool, pattern, created_at FROM permissions \
         WHERE project_id = ?1 COLLATE BINARY AND tool = ?2 COLLATE BINARY \
         AND pattern = ?3 COLLATE BINARY",
        params![project_id, tool, pattern],
        permission_rule_from_row,
    )
    .optional()
}

fn permission_rule_from_row(row: &rusqlite::Row<'_>) -> Result<PermissionRule, rusqlite::Error> {
    Ok(PermissionRule {
        id: row.get(0)?,
        project_id: row.get(1)?,
        tool: row.get(2)?,
        pattern: row.get(3)?,
        created_at: row.get(4)?,
    })
}
