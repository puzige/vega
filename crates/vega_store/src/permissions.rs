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
    if !matches!(tool, "bash" | "write" | "edit") {
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

#[cfg(test)]
mod tests {
    use super::{InsertExactRule, PermissionsError, insert_exact, list_exact, matches_exact};
    use crate::Store;

    fn store() -> Store {
        let store = Store::open(":memory:").unwrap();
        store.migrate().unwrap();
        store
    }

    fn insert<'a>(
        store: &Store,
        project_id: &'a str,
        tool: &'a str,
        pattern: &'a str,
        created_at: i64,
    ) -> super::InsertExactResult {
        insert_exact(
            store.conn(),
            InsertExactRule {
                project_id,
                tool,
                pattern,
                created_at,
            },
        )
        .unwrap()
    }

    #[test]
    fn insert_list_and_match_all_allowed_tools() {
        let store = store();
        for (tool, pattern) in [
            ("bash", "cargo test"),
            ("write", "src/lib.rs"),
            ("edit", "README.md"),
        ] {
            assert!(insert(&store, "project-a", tool, pattern, 10).inserted);
            assert!(matches_exact(store.conn(), "project-a", tool, pattern).unwrap());
        }
        assert_eq!(list_exact(store.conn(), "project-a").unwrap().len(), 3);
    }

    #[test]
    fn rejects_invalid_fields() {
        let store = store();
        for (project, tool, pattern, expected) in [
            ("", "bash", "echo ok", "empty-project"),
            ("p", "read", "README.md", "tool"),
            ("p", "bash", "", "empty-pattern"),
        ] {
            let error = insert_exact(
                store.conn(),
                InsertExactRule {
                    project_id: project,
                    tool,
                    pattern,
                    created_at: 1,
                },
            )
            .unwrap_err();
            assert!(
                matches!(
                    (expected, error),
                    ("empty-project", PermissionsError::EmptyProject)
                        | ("tool", PermissionsError::UnsupportedTool)
                        | ("empty-pattern", PermissionsError::EmptyPattern)
                ),
                "{expected}"
            );
        }
    }

    #[test]
    fn duplicate_insert_is_idempotent_and_keeps_original_timestamp() {
        let store = store();
        let first = insert(&store, "p", "bash", "cargo test", 20);
        let duplicate = insert(&store, "p", "bash", "cargo test", 99);
        assert!(first.inserted);
        assert!(!duplicate.inserted);
        assert_eq!(duplicate.rule.id, first.rule.id);
        assert_eq!(duplicate.rule.created_at, 20);
        assert_eq!(list_exact(store.conn(), "p").unwrap().len(), 1);
    }

    #[test]
    fn exact_matching_preserves_project_tool_case_whitespace_and_path_bytes() {
        let store = store();
        insert(&store, "project-a", "bash", "cargo  test", 1);
        insert(&store, "project-a", "write", "Src/lib.rs", 2);

        for (project, tool, pattern) in [
            ("project-b", "bash", "cargo  test"),
            ("project-a", "bash", "cargo test"),
            ("project-a", "bash", "Cargo  test"),
            ("project-a", "edit", "Src/lib.rs"),
            ("project-a", "write", "src/lib.rs"),
            ("project-a", "bash", "bash:cargo  test"),
        ] {
            assert!(!matches_exact(store.conn(), project, tool, pattern).unwrap());
        }
    }

    #[test]
    fn list_order_is_created_at_then_id() {
        let store = store();
        let second = insert(&store, "p", "edit", "b", 20).rule;
        let first = insert(&store, "p", "write", "a", 10).rule;
        let third = insert(&store, "p", "bash", "c", 20).rule;
        let listed = list_exact(store.conn(), "p").unwrap();
        assert_eq!(
            listed.iter().map(|rule| rule.id).collect::<Vec<_>>(),
            vec![first.id, second.id, third.id]
        );
    }

    #[test]
    fn schema_remains_six_tables_at_user_version_two() {
        let store = store();
        let user_version: i64 = store
            .conn()
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        let table_count: i64 = store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' \
                 AND name NOT LIKE 'sqlite_%'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        // S7-T38 appended migration 0003 (token_usage pricing columns): user_version
        // advances 2 → 3; the table set must stay exactly six either way.
        assert_eq!(user_version, 3);
        assert_eq!(table_count, 6);
    }
}
