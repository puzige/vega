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
    let _permit = crate::write_coordination::acquire(
        conn,
        crate::write_coordination::WriteCategory::Projects,
    )?;
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

/// Loads one project by its opaque id. App-level agent preparation uses this
/// typed boundary to obtain the fenced project root without issuing raw SQL.
pub fn find(conn: &Connection, id: &str) -> Result<Option<Project>, ProjectsError> {
    conn.query_row(
        "SELECT id, path, name, git_default_branch, created_at, last_opened_at \
         FROM projects WHERE id = ?1",
        [id],
        |row| {
            Ok(Project {
                id: row.get(0)?,
                path: row.get(1)?,
                name: row.get(2)?,
                git_default_branch: row.get(3)?,
                created_at: row.get(4)?,
                last_opened_at: row.get(5)?,
            })
        },
    )
    .optional()
    .map_err(ProjectsError::from)
}

/// Finds the existing registration for an exact persisted folder path.
pub fn find_by_path(conn: &Connection, path: &str) -> Result<Option<Project>, ProjectsError> {
    conn.query_row(
        "SELECT id, path, name, git_default_branch, created_at, last_opened_at \
         FROM projects WHERE path = ?1",
        [path],
        |row| {
            Ok(Project {
                id: row.get(0)?,
                path: row.get(1)?,
                name: row.get(2)?,
                git_default_branch: row.get(3)?,
                created_at: row.get(4)?,
                last_opened_at: row.get(5)?,
            })
        },
    )
    .optional()
    .map_err(ProjectsError::from)
}

pub fn remove(conn: &Connection, id: &str) -> Result<bool, ProjectsError> {
    let _permit = crate::write_coordination::acquire(
        conn,
        crate::write_coordination::WriteCategory::Projects,
    )?;
    let transaction = crate::write_coordination::WriteTransaction::new(
        conn,
        crate::write_coordination::WriteCategory::Projects,
    )?;
    transaction.execute(
        "UPDATE threads SET project_id = NULL WHERE project_id = ?1",
        rusqlite::params![id],
    )?;
    let deleted =
        transaction.execute("DELETE FROM projects WHERE id = ?1", rusqlite::params![id])?;
    transaction.commit()?;
    Ok(deleted > 0)
}

/// Updates `last_opened_at` to "now" for the project with `id`.
pub fn touch_last_opened(conn: &Connection, id: &str) -> Result<(), ProjectsError> {
    let _permit = crate::write_coordination::acquire(
        conn,
        crate::write_coordination::WriteCategory::Projects,
    )?;
    conn.execute(
        "UPDATE projects SET last_opened_at = ?1 WHERE id = ?2",
        rusqlite::params![now_ms(), id],
    )?;
    Ok(())
}

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
