pub mod automation;
pub mod codex_tasks;
pub mod command_scope;
pub mod composer_prediction_events;
pub mod computer_use_delivery;
pub mod config;
pub mod context_compaction;
pub mod context_micro;
pub mod context_precompute;
pub mod context_projection;
pub mod credential_continuation;
pub mod credential_privacy;
pub mod git_detect;
pub mod image_attachments;
pub mod keystore;
pub mod mcp_servers;
pub mod messages;
pub mod model_capabilities;
pub mod palette;
pub mod paths;
pub mod pending_messages;
pub mod permission_contracts;
pub mod permission_reset;
pub mod permissions;
pub mod projects;
pub mod reasoning;
pub mod recovery;
pub mod run_diagnostics;
pub mod sidebar_organization;
pub mod skills;
pub mod submission_diagnostics;

pub mod delegated_ownership;
pub mod delegated_stages;
pub mod subagents;
pub mod thread_admission;
pub mod thread_fork;
pub mod threads;
pub mod token_usage;
pub mod tool_calls;
pub mod write_coordination;

use std::path::{Path, PathBuf};

pub use rusqlite::{Connection, Transaction};
pub use thread_admission::DatabaseIdentity;
use write_coordination::{WriteCategory, WriteTransaction};

/// Ordered, compile-time-embedded schema migrations.
///
/// `MIGRATIONS[i]` migrates the schema from `user_version == i` to
/// `user_version == i + 1`.
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_init.sql"),
    include_str!("../migrations/0002_plan_review.sql"),
    include_str!("../migrations/0003_token_usage_pricing.sql"),
    include_str!("../migrations/0004_sidebar_organization.sql"),
    include_str!("../migrations/0005_standalone_threads.sql"),
    include_str!("../migrations/0006_tool_text_offset.sql"),
    include_str!("../migrations/0007_image_attachments.sql"),
    include_str!("../migrations/0008_auto_titles.sql"),
    include_str!("../migrations/0009_context_compaction.sql"),
    include_str!("../migrations/0010_context_compaction_status.sql"),
    include_str!("../migrations/0011_model_context_policies.sql"),
    include_str!("../migrations/0012_mcp_servers.sql"),
    include_str!("../migrations/0013_skills.sql"),
    include_str!("../migrations/0014_execution_duration.sql"),
    include_str!("../migrations/0015_run_diagnostics.sql"),
    include_str!("../migrations/0016_codex_task_identity.sql"),
    include_str!("../migrations/0017_assistant_responses.sql"),
    include_str!("../migrations/0018_computer_use_delivery.sql"),
    include_str!("../migrations/0019_pending_messages.sql"),
    include_str!("../migrations/0020_context_compaction_timeout.sql"),
    include_str!("../migrations/0021_tool_call_rounds.sql"),
    include_str!("../migrations/0022_partial_compaction.sql"),
    include_str!("../migrations/0023_credential_continuations.sql"),
    include_str!("../migrations/0024_credential_privacy.sql"),
    include_str!("../migrations/0025_failure_details.sql"),
    include_str!("../migrations/0026_automation.sql"),
    include_str!("../migrations/0027_subagents.sql"),
    include_str!("../migrations/0028_runtime_failure_details.sql"),
    include_str!("../migrations/0029_sidebar_message_order.sql"),
    include_str!("../migrations/0030_delegated_executions.sql"),
    include_str!("../migrations/0031_sidebar_project_order.sql"),
    include_str!("../migrations/0032_codex_project_history.sql"),
    include_str!("../migrations/0033_delegated_stage_recovery.sql"),
    include_str!("../migrations/0034_run_continuations.sql"),
    include_str!("../migrations/0035_thread_fork.sql"),
    include_str!("../migrations/0036_generated_notices.sql"),
    include_str!("../migrations/0037_codex_display_metadata.sql"),
    include_str!("../migrations/0038_provider_failure_split.sql"),
    include_str!("../migrations/0039_pending_thread_model.sql"),
    include_str!("../migrations/0040_composer_prediction_events.sql"),
    include_str!("../migrations/0041_provider_overloaded.sql"),
    include_str!("../migrations/0042_post_tool_runtime_diagnostics.sql"),
    include_str!("../migrations/0043_permission_contracts.sql"),
    include_str!("../migrations/0044_permission_reset.sql"),
    include_str!("../migrations/0045_plan_instruction_authority.sql"),
    include_str!("../migrations/0046_provider_protocol_reason.sql"),
    include_str!("../migrations/0047_thread_reasoning.sql"),
    include_str!("../migrations/0048_native_tool_descriptions.sql"),
    include_str!("../migrations/0049_context_projection.sql"),
    include_str!("../migrations/0050_context_micro_compactions.sql"),
    include_str!("../migrations/0051_context_precompute.sql"),
    include_str!("../migrations/0052_submission_diagnostics.sql"),
    include_str!("../migrations/0053_response_start_times.sql"),
];

/// Single-connection SQLite store for the Vega content and sidebar metadata schema.
pub struct Store {
    conn: Connection,
    database_path: Option<PathBuf>,
    database_identity: DatabaseIdentity,
}

impl Store {
    /// Open an existing database read-only without migration or file creation.
    pub fn open_read_only(path: impl AsRef<Path>) -> Result<Self, rusqlite::Error> {
        let conn = Connection::open_with_flags(
            path.as_ref(),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(std::time::Duration::from_secs(2))?;
        let database_identity = DatabaseIdentity::for_connection(&conn)?;
        Ok(Self {
            conn,
            database_path: Some(path.as_ref().to_path_buf()),
            database_identity,
        })
    }

    /// Opens (creating if necessary) the database file at `path` and applies
    /// the standard Vega pragmas: `journal_mode=WAL`, `synchronous=NORMAL`,
    /// `foreign_keys=ON`.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, rusqlite::Error> {
        Self::open_with_busy_timeout(path, std::time::Duration::from_secs(2))
    }

    pub fn open_existing(path: impl AsRef<Path>) -> Result<Self, rusqlite::Error> {
        let conn = Connection::open_with_flags(
            path.as_ref(),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        Self::configure_connection(
            conn,
            Some(path.as_ref().to_path_buf()),
            std::time::Duration::from_secs(2),
        )
    }

    pub fn open_with_busy_timeout(
        path: impl AsRef<Path>,
        busy_timeout: std::time::Duration,
    ) -> Result<Self, rusqlite::Error> {
        let path = path.as_ref();
        let database_path = if path == Path::new(":memory:") || path.as_os_str().is_empty() {
            None
        } else {
            Some(path.to_path_buf())
        };
        let conn = Connection::open(path)?;
        Self::configure_connection(conn, database_path, busy_timeout)
    }

    fn configure_connection(
        conn: Connection,
        database_path: Option<PathBuf>,
        busy_timeout: std::time::Duration,
    ) -> Result<Self, rusqlite::Error> {
        conn.busy_timeout(busy_timeout)?;
        let mode: String = conn.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
        if !mode.eq_ignore_ascii_case("wal") {
            let _permit = write_coordination::acquire(&conn, WriteCategory::Initialization)?;
            conn.pragma_update(None, "journal_mode", "WAL")?;
        }
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let database_identity = DatabaseIdentity::for_connection(&conn)?;
        Ok(Self {
            conn,
            database_path,
            database_identity,
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
        if current as usize > MIGRATIONS.len() {
            return Err(rusqlite::Error::InvalidQuery);
        }
        if current as usize == MIGRATIONS.len() {
            permission_contracts::load_state(&self.conn)?;
            return Ok(());
        }
        let _permit = write_coordination::acquire(&self.conn, WriteCategory::Migration)?;
        let current: u32 = self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if current as usize > MIGRATIONS.len() {
            return Err(rusqlite::Error::InvalidQuery);
        }
        for (index, sql) in MIGRATIONS.iter().enumerate() {
            let target = (index + 1) as u32;
            if current >= target {
                continue;
            }

            let rebuilds_threads = target == 5;
            if rebuilds_threads {
                self.conn.pragma_update(None, "foreign_keys", false)?;
            }
            let result = (|| -> Result<(), rusqlite::Error> {
                let tx = WriteTransaction::new(&self.conn, WriteCategory::Migration)?;
                tx.execute_batch(sql)?;
                if target == 43 {
                    permission_contracts::initialize(&tx)?;
                }
                tx.pragma_update(None, "user_version", target)?;
                tx.commit()
            })();
            if rebuilds_threads {
                self.conn.pragma_update(None, "foreign_keys", true)?;
            }
            result?;
        }
        permission_contracts::load_state(&self.conn)?;
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
    pub fn immediate_transaction(&self) -> Result<WriteTransaction<'_>, rusqlite::Error> {
        WriteTransaction::new(&self.conn, WriteCategory::RuntimeEvent)
    }

    /// Returns the file backing this store, or `None` for SQLite in-memory or
    /// temporary databases that cannot be reopened by a dedicated DB actor.
    pub fn database_path(&self) -> Option<&Path> {
        self.database_path.as_deref()
    }

    pub fn database_identity(&self) -> DatabaseIdentity {
        self.database_identity
    }
}
