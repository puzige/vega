//! Project registration CRUD (A1-03) against the `projects` table.
//!
//! Functions take a [`Connection`] (the single connection owned by
//! [`Store`](crate::Store)) rather than `&Store`, so the module stays free of
//! store-level concerns. IDs are ULIDs (tech-spec §2: `projects.id = ulid`);
//! timestamps are unix milliseconds.
//!
//! The `path` column is `UNIQUE`: registering the same folder twice returns
//! [`ProjectsError::PathAlreadyRegistered`] so callers (the projects UI) can
//! render the inline danger bar instead of a generic failure.

use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension};

/// One registered project row (`projects` table, tech-spec §2).
#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    /// ULID primary key.
    pub id: String,
    /// Absolute path of the project folder (unique).
    pub path: String,
    /// Display name (UI: the folder's file name).
    pub name: String,
    /// Default branch detected at registration time; `None` for non-git
    /// directories and detached checkouts.
    pub git_default_branch: Option<String>,
    /// Creation time (unix ms).
    pub created_at: i64,
    /// Last time the project was opened/selected (unix ms).
    pub last_opened_at: i64,
}

/// Sort order for [`list`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectSort {
    /// Case-insensitive lexicographic order by `name`.
    Name,
    /// `last_opened_at` descending: most recently opened first.
    RecentlyOpened,
}

/// Errors raised by the project CRUD functions.
#[derive(Debug, thiserror::Error)]
pub enum ProjectsError {
    /// The folder is already registered (`UNIQUE` violation on `path`).
    #[error("path already registered: {0}")]
    PathAlreadyRegistered(String),
    /// Underlying SQLite failure.
    #[error("projects store error: {0}")]
    Sql(#[from] rusqlite::Error),
}

/// Inserts a project row and returns it.
///
/// `id` is a freshly generated ULID; `created_at` and `last_opened_at` both
/// start at "now". A non-git directory registers with
/// `git_default_branch = None`.
pub fn create(
    conn: &Connection,
    path: &str,
    name: &str,
    git_default_branch: Option<&str>,
) -> Result<Project, ProjectsError> {
    let now = now_ms();
    let id = ulid::Ulid::generate().to_string();
    let inserted = conn.execute(
        "INSERT INTO projects (id, path, name, git_default_branch, created_at, last_opened_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        rusqlite::params![id, path, name, git_default_branch, now],
    );
    if let Err(error) = inserted {
        if is_unique_violation(&error) {
            return Err(ProjectsError::PathAlreadyRegistered(path.to_string()));
        }
        return Err(error.into());
    }
    Ok(Project {
        id,
        path: path.to_string(),
        name: name.to_string(),
        git_default_branch: git_default_branch.map(str::to_string),
        created_at: now,
        last_opened_at: now,
    })
}

/// Lists all registered projects in the requested order.
///
/// Tiebreakers (`path` / `name`) keep the order stable across reloads.
pub fn list(conn: &Connection, sort: ProjectSort) -> Result<Vec<Project>, ProjectsError> {
    let order = match sort {
        ProjectSort::Name => "name COLLATE NOCASE ASC, path ASC",
        ProjectSort::RecentlyOpened => "last_opened_at DESC, name COLLATE NOCASE ASC",
    };
    let sql = format!(
        "SELECT id, path, name, git_default_branch, created_at, last_opened_at \
         FROM projects ORDER BY {order}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok(Project {
            id: row.get(0)?,
            path: row.get(1)?,
            name: row.get(2)?,
            git_default_branch: row.get(3)?,
            created_at: row.get(4)?,
            last_opened_at: row.get(5)?,
        })
    })?;
    let mut projects = Vec::new();
    for row in rows {
        projects.push(row?);
    }
    Ok(projects)
}

/// Removes the project row with `id` and returns whether a row was deleted.
///
/// Only the database row is removed; files on disk are never touched (S2
/// ruling: no confirmation layer, no filesystem writes).
pub fn remove(conn: &Connection, id: &str) -> Result<bool, ProjectsError> {
    let deleted = conn.execute("DELETE FROM projects WHERE id = ?1", rusqlite::params![id])?;
    Ok(deleted > 0)
}

/// Updates `last_opened_at` to "now" for the project with `id`.
pub fn touch_last_opened(conn: &Connection, id: &str) -> Result<(), ProjectsError> {
    conn.execute(
        "UPDATE projects SET last_opened_at = ?1 WHERE id = ?2",
        rusqlite::params![now_ms(), id],
    )?;
    Ok(())
}

/// Whether a `projects` row with this id exists.
///
/// Create-thread guard for `vega_conversation` so a missing project surfaces
/// as a typed error instead of a bare foreign-key failure. T11 temporarily
/// parked these two project-domain helpers next to the thread SQL; T12 moved
/// them back into the projects module (architect ruling).
pub fn project_exists(conn: &Connection, project_id: &str) -> Result<bool, rusqlite::Error> {
    let found: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM projects WHERE id = ?1",
            [project_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(found.is_some())
}

/// The most recently opened project, if any.
///
/// Backs the sidebar's initial selected project (latest_project semantics);
/// the UI caches the selection and rewrites it on row click.
pub fn latest_project(conn: &Connection) -> Result<Option<ProjectRef>, rusqlite::Error> {
    conn.query_row(
        "SELECT id, name FROM projects ORDER BY last_opened_at DESC, created_at ASC LIMIT 1",
        [],
        |row| {
            Ok(ProjectRef {
                id: row.get(0)?,
                name: row.get(1)?,
            })
        },
    )
    .optional()
}

/// Minimal `(id, name)` projection of a `projects` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRef {
    /// Project id (`projects.id`).
    pub id: String,
    /// Display name.
    pub name: String,
}

/// Current time as unix milliseconds.
fn now_ms() -> i64 {
    // 时钟早于 epoch 属于系统级异常，取 0 保底即可（不允许 panic）。
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as i64)
}

/// Whether `error` is the SQLite UNIQUE-constraint failure (extended code
/// 2067, `SQLITE_CONSTRAINT_UNIQUE`).
fn is_unique_violation(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(ffi, _)
            if ffi.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE
    )
}

#[cfg(test)]
mod tests {
    use super::{ProjectSort, ProjectsError, create, list, remove, touch_last_opened};
    use crate::Store;
    use rusqlite::{Connection, params};
    use tempfile::tempdir;

    /// Creates a migrated store backed by a fresh temporary directory.
    fn open_temp_store() -> (Store, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path().join("vega.db")).unwrap();
        store.migrate().unwrap();
        (store, dir)
    }

    /// 测试所需 project 行用裸 SQL 插入（补齐 DDL 必填字段），
    /// 不依赖本模块被测函数。
    fn insert_project(conn: &Connection, id: &str, name: &str, opened_at: i64) {
        conn.execute(
            "INSERT INTO projects (id, path, name, git_default_branch, created_at, last_opened_at) \
             VALUES (?1, ?2, ?3, NULL, ?4, ?5)",
            params![id, format!("/tmp/{id}"), name, opened_at, opened_at],
        )
        .unwrap();
    }

    #[test]
    fn create_assigns_ulid_and_stamps_times() {
        let (store, _dir) = open_temp_store();
        let project = create(store.conn(), "/tmp/repo", "repo", Some("main")).unwrap();
        // ULID：26 字符、全大写 Crockford base32。
        assert_eq!(project.id.len(), 26);
        assert!(project.id.chars().all(|c| c.is_ascii_alphanumeric()));
        assert_eq!(project.path, "/tmp/repo");
        assert_eq!(project.name, "repo");
        assert_eq!(project.git_default_branch, Some("main".to_string()));
        assert_eq!(project.created_at, project.last_opened_at);
        assert!(project.created_at > 0);
    }

    #[test]
    fn non_git_directory_registers_with_null_branch() {
        let (store, _dir) = open_temp_store();
        let project = create(store.conn(), "/tmp/plain", "plain", None).unwrap();
        assert_eq!(project.git_default_branch, None);
    }

    #[test]
    fn duplicate_path_is_rejected_as_already_registered() {
        let (store, _dir) = open_temp_store();
        create(store.conn(), "/tmp/repo", "repo", Some("main")).unwrap();
        let err = create(store.conn(), "/tmp/repo", "again", None).unwrap_err();
        assert!(matches!(err, ProjectsError::PathAlreadyRegistered(p) if p == "/tmp/repo"));
        // 库里仍只有一行。
        assert_eq!(list(store.conn(), ProjectSort::Name).unwrap().len(), 1);
    }

    #[test]
    fn list_sorts_by_name_case_insensitively() {
        let (store, _dir) = open_temp_store();
        // 字典序大小写不敏感：alpha < beta < gamma。
        create(store.conn(), "/tmp/beta", "beta", None).unwrap();
        create(store.conn(), "/tmp/alpha", "Alpha", None).unwrap();
        create(store.conn(), "/tmp/gamma", "GAMMA", None).unwrap();
        let names: Vec<String> = list(store.conn(), ProjectSort::Name)
            .unwrap()
            .into_iter()
            .map(|project| project.name)
            .collect();
        assert_eq!(names, vec!["Alpha", "beta", "GAMMA"]);
    }

    #[test]
    fn list_sorts_recently_opened_first() {
        let (store, _dir) = open_temp_store();
        // 直接写死时间戳，避免对真实时钟的依赖。
        store
            .conn()
            .execute(
                "INSERT INTO projects (id, path, name, git_default_branch, created_at, last_opened_at) \
                 VALUES ('old', '/tmp/old', 'old', NULL, 1000, 1000)",
                [],
            )
            .unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO projects (id, path, name, git_default_branch, created_at, last_opened_at) \
                 VALUES ('new', '/tmp/new', 'new', NULL, 2000, 2000)",
                [],
            )
            .unwrap();

        let recent: Vec<String> = list(store.conn(), ProjectSort::RecentlyOpened)
            .unwrap()
            .into_iter()
            .map(|project| project.id)
            .collect();
        assert_eq!(recent, vec!["new", "old"]);

        // 打开旧项目后它应排到最前。
        touch_last_opened(store.conn(), "old").unwrap();
        let recent: Vec<String> = list(store.conn(), ProjectSort::RecentlyOpened)
            .unwrap()
            .into_iter()
            .map(|project| project.id)
            .collect();
        assert_eq!(recent, vec!["old", "new"]);
    }

    #[test]
    fn remove_deletes_only_the_target_row() {
        let (store, _dir) = open_temp_store();
        let kept = create(store.conn(), "/tmp/kept", "kept", None).unwrap();
        let gone = create(store.conn(), "/tmp/gone", "gone", None).unwrap();

        assert!(remove(store.conn(), &gone.id).unwrap());
        let remaining = list(store.conn(), ProjectSort::Name).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, kept.id);

        // 重复删除同一 id：无行受影响，返回 false。
        assert!(!remove(store.conn(), &gone.id).unwrap());
    }

    #[test]
    fn project_exists_matches_inserted_rows_only() {
        let (store, _dir) = open_temp_store();
        insert_project(store.conn(), "p1", "alpha", 10);
        assert!(super::project_exists(store.conn(), "p1").unwrap());
        assert!(!super::project_exists(store.conn(), "missing").unwrap());
    }

    #[test]
    fn latest_project_prefers_the_most_recently_opened() {
        let (store, _dir) = open_temp_store();
        assert!(super::latest_project(store.conn()).unwrap().is_none());
        insert_project(store.conn(), "p1", "alpha", 100);
        insert_project(store.conn(), "p2", "beta", 200);
        let latest = super::latest_project(store.conn()).unwrap().unwrap();
        assert_eq!(latest.id, "p2");
        assert_eq!(latest.name, "beta");
    }
}
