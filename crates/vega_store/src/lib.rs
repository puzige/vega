//! SQLite persistence: projects, threads, messages, tool_calls, and token_usage.
//!
//! [`Store`] owns a single SQLite connection opened in WAL mode
//! (`journal_mode=WAL`, `synchronous=NORMAL`, `foreign_keys=ON`).
//!
//! # Migrations
//!
//! Schema migrations are embedded into the binary at compile time
//! ([`MIGRATIONS`], an ordered array of `include_str!`'d SQL files under
//! `migrations/`, so the SQL ships with the binary instead of being resolved
//! from a runtime directory). [`Store::migrate`] reads `PRAGMA user_version`
//! and applies every pending migration in order: `MIGRATIONS[i]` upgrades the
//! schema from `user_version == i` to `user_version == i + 1`. Each migration
//! runs in its own transaction (SQL batch + `user_version` bump) and is rolled
//! back entirely on failure, so `user_version` and the schema always agree.
//!
//! # Accessing the connection
//!
//! [`Store::conn`] hands out the single connection for other crates' actors
//! (e.g. `vega_conversation`) to run statements on. It is intentionally *not*
//! `Sync`-friendly magic — do **not** call it inside `tokio::select!` branches:
//! rusqlite calls are blocking synchronous IO, and running them in a `select!`
//! branch blocks the whole executor and breaks cancellation semantics. Wrap
//! such work in blocking-safe tasks / dedicated DB actors instead.
//!
//! # Error type evolution path
//!
//! This crate's API currently returns `rusqlite::Error` directly. The unified
//! [`VegaError`](https://github.com/puzige/vega/blob/master/docs/vega-tech-spec-p1.md)
//! (tech-spec §7) will live in `vega_conversation::types`, and the dependency
//! direction is `vega_conversation → vega_store` (tech-spec §1), so
//! `vega_store` must not depend back on `vega_conversation`. Once `VegaError`
//! lands, a `From<rusqlite::Error>` bridge into `VegaError::Store` will be
//! added at the `vega_conversation` layer.

//! Module layout: `paths` owns the config/data roots (tech-spec §6), `config`
//! owns `config.toml` under the config root, `keystore` owns
//! Keychain-backed credentials; `projects` / `git_detect` are T10 additions.

pub mod config;
pub mod git_detect;
pub mod keystore;
pub mod messages;
pub mod paths;
pub mod permissions;
pub mod projects;
pub mod recovery;
// T11（A1-02）：threads 表 SQL 层（projects 域函数归 T10）。
pub mod threads;
pub mod token_usage;
pub mod tool_calls;

use std::path::{Path, PathBuf};

use rusqlite::{Connection, Transaction, TransactionBehavior};

/// Ordered, compile-time-embedded schema migrations.
///
/// `MIGRATIONS[i]` migrates the schema from `user_version == i` to
/// `user_version == i + 1`.
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_init.sql"),
    include_str!("../migrations/0002_plan_review.sql"),
    include_str!("../migrations/0003_token_usage_pricing.sql"),
];

/// Single-connection SQLite store for the six-table Vega schema.
pub struct Store {
    conn: Connection,
    database_path: Option<PathBuf>,
}

impl Store {
    /// Opens (creating if necessary) the database file at `path` and applies
    /// the standard Vega pragmas: `journal_mode=WAL`, `synchronous=NORMAL`,
    /// `foreign_keys=ON`.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, rusqlite::Error> {
        let path = path.as_ref();
        let database_path = if path == Path::new(":memory:") || path.as_os_str().is_empty() {
            None
        } else {
            Some(path.to_path_buf())
        };
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Ok(Self {
            conn,
            database_path,
        })
    }

    /// Brings the schema up to date by applying pending migrations in order.
    ///
    /// Idempotent: already-applied migrations (tracked via `PRAGMA
    /// user_version`) are skipped. Each migration runs in its own transaction;
    /// on failure the transaction is rolled back (drop) and the error is
    /// returned.
    pub fn migrate(&self) -> Result<(), rusqlite::Error> {
        let current: u32 = self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;
        for (index, sql) in MIGRATIONS.iter().enumerate() {
            let target = (index + 1) as u32;
            if current >= target {
                continue;
            }
            // 每个迁移独立事务：SQL 批 + user_version 推进，任一步失败
            // 事务 drop 即整体回滚，? 直接向上返回
            let tx = self.conn.unchecked_transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", target)?;
            tx.commit()?;
        }
        Ok(())
    }

    /// Borrows the underlying single connection.
    ///
    /// Intended for other crates' actors (e.g. `vega_conversation`) that need
    /// direct statement access. **Do not use inside `tokio::select!`
    /// branches**: rusqlite calls are blocking synchronous IO and would stall
    /// the executor and break cancellation.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Opens an unchecked `BEGIN IMMEDIATE` transaction through the shared
    /// connection reference. Conversation orchestration uses this to claim
    /// persisted one-shot work before any provider activity.
    pub fn immediate_transaction(&self) -> Result<Transaction<'_>, rusqlite::Error> {
        Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)
    }

    /// Returns the file backing this store, or `None` for SQLite in-memory or
    /// temporary databases that cannot be reopened by a dedicated DB actor.
    pub fn database_path(&self) -> Option<&Path> {
        self.database_path.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::Store;
    use rusqlite::ErrorCode;
    use tempfile::tempdir;

    /// Creates a migrated store backed by a fresh temporary directory.
    fn open_temp_store() -> (Store, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path().join("vega.db")).unwrap();
        store.migrate().unwrap();
        (store, dir)
    }

    fn user_version(store: &Store) -> u32 {
        store
            .conn()
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn migrate_creates_exactly_the_six_tables() {
        let (store, _dir) = open_temp_store();
        let mut stmt = store
            .conn()
            .prepare(
                "SELECT name FROM sqlite_master \
                 WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
            )
            .unwrap();
        let tables: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            tables,
            vec![
                "messages",
                "permissions",
                "projects",
                "threads",
                "token_usage",
                "tool_calls",
            ]
        );
    }

    #[test]
    fn database_path_distinguishes_file_backing_from_memory() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vega.db");
        let file_store = Store::open(&path).unwrap();
        assert_eq!(file_store.database_path(), Some(path.as_path()));

        let memory_store = Store::open(":memory:").unwrap();
        assert_eq!(memory_store.database_path(), None);
    }

    #[test]
    fn migrated_store_is_wal_at_user_version_3() {
        let (store, _dir) = open_temp_store();
        assert_eq!(user_version(&store), 3);
        let journal_mode: String = store
            .conn()
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(journal_mode, "wal");
        // T38 (C5)：追加列存在且旧行读回 NULL（legacy/unpriced 语义）。
        let legacy: (Option<String>, Option<String>, Option<i64>) = store
            .conn()
            .query_row(
                "INSERT INTO token_usage (thread_id, model, input_tokens, output_tokens, \
                        cost_microcents, created_at) \
                 VALUES ('t', 'm', 1, 2, 0, 3) \
                 RETURNING pricing_version, pricing_profile, call_started_at",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(legacy, (None, None, None));
    }

    #[test]
    fn migrate_is_idempotent() {
        let (store, _dir) = open_temp_store();
        // 第二次调用不报错
        store.migrate().unwrap();
        // 版本不前进
        assert_eq!(user_version(&store), 3);
        // 数据未被破坏：threads 仍为空
        let thread_count: i64 = store
            .conn()
            .query_row("SELECT COUNT(*) FROM threads", [], |row| row.get(0))
            .unwrap();
        assert_eq!(thread_count, 0);
    }

    #[test]
    fn version_one_database_upgrades_in_place_without_a_seventh_table() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("legacy.db");
        let store = Store::open(&path).unwrap();
        store
            .conn()
            .execute_batch(include_str!("../migrations/0001_init.sql"))
            .unwrap();
        store
            .conn()
            .pragma_update(None, "user_version", 1_u32)
            .unwrap();
        store
            .conn()
            .execute_batch(
                "INSERT INTO projects VALUES ('p','/tmp/p','project',NULL,1,2); \
                 INSERT INTO threads VALUES ('t','p','thread','plan','confirm','mock','active',0,0,3,4); \
                 INSERT INTO messages VALUES ('m','t',1,'assistant','text','kept','done',5); \
                 INSERT INTO tool_calls (id,thread_id,message_id,seq,tool,input_json,status,created_at) \
                   VALUES ('c','t','m',1,'read','{}','success',6); \
                 INSERT INTO token_usage (thread_id,message_id,model,input_tokens,output_tokens,cost_microcents,created_at) \
                   VALUES ('t','m','mock',1,2,0,7); \
                 INSERT INTO permissions (project_id,tool,pattern,created_at) VALUES ('p','write','safe',8);",
            )
            .unwrap();
        store.migrate().unwrap();
        assert_eq!(user_version(&store), 3);
        let kept: (String, Option<String>, Option<String>, Option<i64>) = store
            .conn()
            .query_row(
                "SELECT content,plan_status,plan_review_note,plan_reviewed_at FROM messages WHERE id='m'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(kept, ("kept".into(), None, None, None));
        let tables: i64 = store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(tables, 6);
        for table in [
            "projects",
            "threads",
            "messages",
            "tool_calls",
            "token_usage",
            "permissions",
        ] {
            let count: i64 = store
                .conn()
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 1, "lost data from {table}");
        }
    }

    #[test]
    fn plan_status_check_rejects_unknown_values() {
        let (store, _dir) = open_temp_store();
        store
            .conn()
            .execute_batch(
                "INSERT INTO projects VALUES ('p','/tmp/p','p',NULL,0,0); \
                 INSERT INTO threads (id,project_id,model,created_at,updated_at) VALUES ('t','p','m',0,0);",
            )
            .unwrap();
        let error = store
            .conn()
            .execute(
                "INSERT INTO messages (id,thread_id,seq,role,kind,content,status,created_at,plan_status) \
                 VALUES ('m','t',1,'assistant','plan','x','done',0,'unknown')",
                [],
            )
            .unwrap_err();
        assert!(matches!(
            error,
            rusqlite::Error::SqliteFailure(error, _)
                if error.code == ErrorCode::ConstraintViolation
        ));
    }

    #[test]
    fn foreign_keys_are_enforced() {
        let (store, _dir) = open_temp_store();
        let fk: u32 = store
            .conn()
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap();
        assert_eq!(fk, 1);
        // project_id 指向不存在的 projects 行 → 必须报外键约束错
        let err = store
            .conn()
            .execute(
                "INSERT INTO threads (id, project_id, model, created_at, updated_at) \
                 VALUES ('t1', 'missing-project', 'test-model', 0, 0)",
                [],
            )
            .unwrap_err();
        assert!(matches!(
            err,
            rusqlite::Error::SqliteFailure(e, _)
                if e.code == ErrorCode::ConstraintViolation
        ));
    }
}
