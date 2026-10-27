use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::*;

pub struct PermissionResetSession {
    database_path: PathBuf,
    config_path: PathBuf,
    intent: PermissionResetIntent,
    pause: DatabaseAdmissionPause,
    scheduling_paused: bool,
    receipt: Option<PermissionResetReceipt>,
    threads: Vec<String>,
}

impl PermissionResetSession {
    pub fn begin(
        database_path: PathBuf,
        config_path: PathBuf,
        intent: PermissionResetIntent,
    ) -> Result<Self, PermissionResetError> {
        let store = Store::open_existing(&database_path).map_err(unavailable)?;
        let pause = pause_admission(&store)?;
        let _coordination = crate::permission_contracts::coordinate().map_err(unavailable)?;
        if store.database_identity() != intent.database {
            return Err(PermissionResetError::ConfigurationConflict);
        }
        let identity = configuration_identity(&config_path)?;
        let edit = config::begin_existing_edit(Path::new(&identity))
            .map_err(|_| PermissionResetError::ConfigurationUnavailable)?;
        let state =
            vega_store::permission_contracts::load_state(store.conn()).map_err(unavailable)?;
        if state.reset_pending
            || state.authority_epoch != intent.authority_epoch
            || config_source(&identity, &edit.config) != intent.source
        {
            return Err(PermissionResetError::ConfigurationConflict);
        }
        let mut statement = store
            .conn()
            .prepare("SELECT id FROM threads")
            .map_err(unavailable)?;
        let threads = statement
            .query_map([], |row| row.get(0))
            .map_err(unavailable)?
            .collect::<Result<Vec<String>, _>>()
            .map_err(unavailable)?;
        Ok(Self {
            database_path,
            config_path,
            intent,
            pause,
            scheduling_paused: false,
            receipt: None,
            threads,
        })
    }

    pub fn threads(&self) -> &[String] {
        &self.threads
    }

    pub fn complete(
        &mut self,
        replay: &ReplaySessionRegistry,
        timeout: Duration,
    ) -> Result<PermissionResetReceipt, PermissionResetError> {
        let store = self.store()?;
        if let Some(receipt) = &self.receipt {
            self.verify_completed(&store, receipt)?;
            return Ok(receipt.clone());
        }
        self.pause_scheduling(&store)?;
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or(PermissionResetError::Unavailable)?;
        while !self.pause.owners_released().map_err(unavailable)? {
            if Instant::now() >= deadline {
                return Err(PermissionResetError::Busy);
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let record = persistence::load(store.conn()).map_err(unavailable)?;
        let receipt = if let Some(record) = record.filter(|record| !record.completed) {
            if record.source != self.intent.source {
                return Err(PermissionResetError::ConfigurationConflict);
            }
            recover_pending_with_pause(&store, &self.config_path, &self.pause, replay)?
                .ok_or(PermissionResetError::OutcomeUnknown)?
        } else if let Some(record) = persistence::load(store.conn())
            .map_err(unavailable)?
            .filter(|record| {
                record.completed
                    && record.source == self.intent.source
                    && record.authority_epoch != self.intent.authority_epoch
            })
        {
            let receipt = PermissionResetReceipt {
                reset_id: record.id,
                authority_epoch: record.authority_epoch,
                default_permission_revision: record.target_revision,
            };
            self.verify_completed(&store, &receipt)?;
            receipt
        } else {
            reset_quiescent(&store, &self.config_path, &self.intent, &self.pause, replay)?
        };
        self.verify_completed(&store, &receipt)?;
        self.receipt = Some(receipt.clone());
        Ok(receipt)
    }

    pub fn can_cancel(&self) -> bool {
        if !self.scheduling_paused || self.receipt.is_some() {
            return false;
        }
        let Ok(store) = self.store() else {
            return false;
        };
        let Ok(state) = vega_store::permission_contracts::load_state(store.conn()) else {
            return false;
        };
        if state.reset_pending || state.authority_epoch != self.intent.authority_epoch {
            return false;
        }
        persistence::load(store.conn())
            .is_ok_and(|record| record.is_none_or(|record| record.completed))
    }

    pub fn completed(&self) -> bool {
        self.receipt.is_some()
    }

    pub fn refresh(
        &self,
        threads: &[String],
    ) -> Result<crate::types::PermissionResetRefresh, PermissionResetError> {
        let store = self.store()?;
        let receipt = self
            .receipt
            .as_ref()
            .ok_or(PermissionResetError::OutcomeUnknown)?;
        self.verify_completed(&store, receipt)?;
        let config = config::read_from(&self.config_path)
            .map_err(|_| PermissionResetError::ConfigurationUnavailable)?;
        let mut projections = Vec::new();
        for id in threads {
            let thread = match crate::threads::get_thread(&store, id) {
                Ok(thread) => thread,
                Err(crate::types::ConversationError::NotFound(_)) => continue,
                Err(_) => return Err(PermissionResetError::Unavailable),
            };
            if thread.permission_mode == crate::types::PermissionMode::FullAccess
                || thread.full_access_grant.is_some()
            {
                return Err(PermissionResetError::OutcomeUnknown);
            }
            let pending = crate::pending_messages::load(&store, id).map_err(unavailable)?;
            let tasks = crate::automation::load(&store, id).map_err(unavailable)?;
            if tasks.iter().any(|task| {
                task.status == crate::types::AutomationStatus::Active
                    || task.permission_authorization.is_some()
            }) {
                return Err(PermissionResetError::OutcomeUnknown);
            }
            projections.push(crate::types::PermissionResetThreadProjection {
                thread,
                pending,
                automation: crate::types::AutomationProjection {
                    tasks,
                    busy: false,
                    error: None,
                },
            });
        }
        self.verify_completed(&store, receipt)?;
        Ok(crate::types::PermissionResetRefresh {
            database: store.database_identity(),
            config,
            threads: projections,
        })
    }

    pub fn blocked_thread(&self) -> Option<String> {
        let store = self.store().ok()?;
        for sql in [
            "SELECT thread_id FROM tool_calls WHERE (status IN ('approved','running','failed','cancelled') AND lower(tool) NOT IN ('read','glob','grep','ask_user_question','checkpoint_stage')) OR (status='success' AND lower(tool) NOT IN ('read','glob','grep','ask_user_question','checkpoint_stage','write','edit')) ORDER BY created_at DESC LIMIT 1",
            "SELECT thread_id FROM messages WHERE status='streaming' LIMIT 1",
            "SELECT thread_id FROM automation_runs WHERE finished=0 LIMIT 1",
            "SELECT thread_id FROM credential_continuations WHERE state IN ('claimed','unknown') LIMIT 1",
            "SELECT child_thread_id FROM delegated_executions WHERE state IN ('running','cancelling') LIMIT 1",
            "SELECT child_thread_id FROM delegated_execution_recovery WHERE release_state IN ('held','quarantined') LIMIT 1",
            "SELECT thread_id FROM command_scope_audits WHERE state IN ('consumed','unknown') LIMIT 1",
        ] {
            let mut statement = store.conn().prepare(sql).ok()?;
            let mut rows = statement
                .query_map([], |row| row.get::<_, String>(0))
                .ok()?;
            if let Some(thread) = rows.next() {
                return thread.ok();
            }
        }
        None
    }

    fn store(&self) -> Result<Store, PermissionResetError> {
        let store = Store::open_existing(&self.database_path).map_err(unavailable)?;
        if store.database_identity() != self.intent.database {
            return Err(PermissionResetError::ConfigurationConflict);
        }
        self.pause.owners_released().map_err(unavailable)?;
        Ok(store)
    }

    fn pause_scheduling(&mut self, store: &Store) -> Result<(), PermissionResetError> {
        if self.scheduling_paused {
            return Ok(());
        }
        let _coordination = crate::permission_contracts::coordinate().map_err(unavailable)?;
        let tx = store.immediate_transaction().map_err(unavailable)?;
        crate::automation::pause_for_reset_in_transaction(&tx, crate::threads::now_ms())
            .map_err(unavailable)?;
        tx.commit_with_rollback()
            .map_err(|_| PermissionResetError::OutcomeUnknown)?;
        self.scheduling_paused = true;
        Ok(())
    }

    fn verify_completed(
        &self,
        store: &Store,
        receipt: &PermissionResetReceipt,
    ) -> Result<(), PermissionResetError> {
        let state =
            vega_store::permission_contracts::load_state(store.conn()).map_err(unavailable)?;
        let record = persistence::load(store.conn())
            .map_err(unavailable)?
            .ok_or(PermissionResetError::OutcomeUnknown)?;
        let identity = configuration_identity(&self.config_path)?;
        let saved = config::read_from(&self.config_path)
            .map_err(|_| PermissionResetError::ConfigurationUnavailable)?;
        if state.reset_pending
            || state.authority_epoch != receipt.authority_epoch
            || record.id != receipt.reset_id
            || !record.completed
            || record.authority_epoch != receipt.authority_epoch
            || record.target_revision != receipt.default_permission_revision
            || record.source.configuration_id != identity
            || !record.config_is_reset(&config_source(&identity, &saved))
        {
            return Err(PermissionResetError::OutcomeUnknown);
        }
        Ok(())
    }
}
