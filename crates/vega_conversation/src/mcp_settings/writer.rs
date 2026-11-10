use super::*;

pub(super) struct BeginMcpReconciliation {
    pub registry: Arc<Mutex<RegistryState>>,
    pub prepared: vega_store::mcp_config::PreparedReconciliation,
}

impl crate::database_writer::Sealed for BeginMcpReconciliation {}

impl crate::database_writer::DatabaseCommand for BeginMcpReconciliation {
    type Output = Result<vega_store::mcp_config::ReconciliationIntent, McpStoreError>;

    fn apply(self, store: &Store) -> Self::Output {
        let mut state = self.registry.lock().map_err(|_| McpStoreError::Conflict)?;
        let intent = self.prepared.begin(store)?;
        for id in intent.changed() {
            state.revoke(id);
        }
        Ok(intent)
    }
}

pub(super) struct ConfirmMcpReconciliation {
    pub registry: Arc<Mutex<RegistryState>>,
    pub prepared: vega_store::mcp_config::PreparedConfirmation,
}

impl crate::database_writer::Sealed for ConfirmMcpReconciliation {}

impl crate::database_writer::DatabaseCommand for ConfirmMcpReconciliation {
    type Output = Result<(), McpStoreError>;

    fn apply(self, store: &Store) -> Self::Output {
        let _state = self.registry.lock().map_err(|_| McpStoreError::Conflict)?;
        self.prepared.confirm(store)
    }
}

pub(super) enum McpMetadataWrite {
    Create(McpServerDraft),
    LocalRotation {
        id: String,
        revision: u64,
        variable: String,
    },
    OAuthInstall {
        id: String,
        revision: u64,
        issuer: String,
        client_id: String,
    },
    LastError {
        id: String,
        revision: u64,
        code: Option<String>,
    },
    Replace {
        id: String,
        revision: u64,
        draft: McpServerDraft,
    },
    Enable {
        id: String,
        revision: u64,
        enabled: bool,
    },
    Disconnect {
        id: String,
        revision: u64,
    },
    Remove {
        id: String,
        revision: u64,
    },
}

pub(super) struct SaveMcpMetadata {
    pub registry: Arc<Mutex<RegistryState>>,
    pub expected_epoch: Option<(String, u64)>,
    pub projection: CleanupProjection,
    pub command: McpMetadataWrite,
}

impl crate::database_writer::Sealed for SaveMcpMetadata {}

impl crate::database_writer::DatabaseCommand for SaveMcpMetadata {
    type Output = Result<McpServerRow, McpSettingsError>;

    fn apply(self, store: &Store) -> Self::Output {
        let mut state = self.registry.lock().map_err(|_| McpSettingsError::Store)?;
        if self
            .expected_epoch
            .as_ref()
            .is_some_and(|(id, epoch)| state.epoch(id) != *epoch)
        {
            return Err(McpSettingsError::Conflict);
        }
        let _permit = vega_store::write_coordination::acquire(
            store.conn(),
            vega_store::write_coordination::WriteCategory::McpServers,
        )
        .map_err(|_| McpSettingsError::Store)?;
        let tx = vega_store::write_coordination::WriteTransaction::new(
            store.conn(),
            vega_store::write_coordination::WriteCategory::McpServers,
        )
        .map_err(|_| McpSettingsError::Store)?;
        if CleanupProjection::read(&tx)? != self.projection
            || self.projection.pending_declarations.is_some()
        {
            return Err(McpSettingsError::Conflict);
        }
        let resume = matches!(
            &self.command,
            McpMetadataWrite::Enable { enabled: true, .. }
        );
        let row = match self.command {
            McpMetadataWrite::Create(draft) => mcp_servers::create_in_transaction(&tx, &draft)?,
            McpMetadataWrite::LocalRotation {
                id,
                revision,
                variable,
            } => {
                mcp_servers::begin_local_secret_rotation_in_transaction(
                    &tx, &id, revision, &variable,
                )?
                .0
            }
            McpMetadataWrite::OAuthInstall {
                id,
                revision,
                issuer,
                client_id,
            } => {
                mcp_servers::begin_oauth_install_in_transaction(
                    &tx, &id, revision, &issuer, &client_id,
                )?
                .0
            }
            McpMetadataWrite::LastError { id, revision, code } => {
                mcp_servers::set_last_error_in_transaction(&tx, &id, revision, code.as_deref())?;
                mcp_servers::find(&tx, &id)?.ok_or(McpSettingsError::NotFound)?
            }
            McpMetadataWrite::Replace {
                id,
                revision,
                draft,
            } => mcp_servers::replace_in_transaction(&tx, &id, revision, &draft)?,
            McpMetadataWrite::Enable {
                id,
                revision,
                enabled,
            } => mcp_servers::set_enabled_in_transaction(&tx, &id, revision, enabled)?,
            McpMetadataWrite::Disconnect { id, revision } => {
                mcp_servers::begin_oauth_disconnect_in_transaction(&tx, &id, revision)?
            }
            McpMetadataWrite::Remove { id, revision } => {
                mcp_servers::begin_remove_in_transaction(&tx, &id, revision)?
            }
        };
        tx.commit_with_rollback()
            .map_err(|_| McpSettingsError::Store)?;
        if resume {
            state.suspended.remove(&row.id);
        }
        Ok(row)
    }
}

#[derive(PartialEq, Eq)]
pub(super) struct CleanupProjection {
    accepted_digest: Option<Vec<u8>>,
    generation: i64,
    pending_declarations: Option<String>,
}

impl CleanupProjection {
    pub(super) fn read(conn: &vega_store::Connection) -> Result<Self, McpStoreError> {
        Ok(conn.query_row(
            "SELECT accepted_digest, generation, pending_declarations FROM mcp_config_state WHERE singleton = 1",
            [], |row| Ok(Self { accepted_digest: row.get(0)?, generation: row.get(1)?, pending_declarations: row.get(2)? }),
        )?)
    }
}

pub(super) struct FinishMcpCleanup {
    pub registry: Arc<Mutex<RegistryState>>,
    pub projection: CleanupProjection,
    pub acknowledged: Vec<mcp_servers::McpSecretCleanup>,
    pub deleting: Vec<McpServerRow>,
}

impl crate::database_writer::Sealed for FinishMcpCleanup {}

impl crate::database_writer::DatabaseCommand for FinishMcpCleanup {
    type Output = Result<(), McpStoreError>;

    fn apply(self, store: &Store) -> Self::Output {
        let _state = self.registry.lock().map_err(|_| McpStoreError::Conflict)?;
        let _permit = vega_store::write_coordination::acquire(
            store.conn(),
            vega_store::write_coordination::WriteCategory::McpServers,
        )?;
        let tx = store.immediate_transaction()?;
        if self.projection != CleanupProjection::read(&tx)?
            || self.projection.pending_declarations.is_some()
        {
            return Err(McpStoreError::Conflict);
        }
        let pending = mcp_servers::pending_cleanup(&tx)?;
        for entry in &self.acknowledged {
            if !is_owned_secret_ref(&entry.server_id, &entry.credential_ref)
                || !pending.contains(entry)
                || !mcp_servers::cleanup_target_is_stale(&tx, entry)?
            {
                return Err(McpStoreError::Conflict);
            }
        }
        for row in &self.deleting {
            if mcp_servers::find(&tx, &row.id)?.as_ref() != Some(row) || !row.deleting {
                return Err(McpStoreError::Conflict);
            }
        }
        for entry in self.acknowledged {
            mcp_servers::acknowledge_cleanup(&tx, &entry)?;
        }
        tx.commit().map_err(McpStoreError::from)?;
        for row in self.deleting {
            mcp_servers::finish_remove(store.conn(), &row.id)?;
        }
        Ok(())
    }
}
