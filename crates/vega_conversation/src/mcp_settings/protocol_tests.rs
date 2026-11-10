use super::*;
use vega_store::mcp_config::{Declaration, Transport};

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Fixture {
    root: PathBuf,
    store: Store,
    service: McpServerSettingsService,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let root =
            std::env::temp_dir().join(format!("vega-mcp-protocol-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root)?;
        let store = Store::open(root.join("database.db"))?;
        store.migrate()?;
        let service = McpServerSettingsService::new(root.join("database.db"), root.clone());
        Ok(Self {
            root,
            store,
            service,
        })
    }

    fn config(&self) -> PathBuf {
        self.root.join("config.yaml")
    }

    fn declaration(&self) -> Declaration {
        Declaration {
            id: ulid::Ulid::generate().to_string(),
            display_name: "Protocol test".into(),
            enabled: true,
            transport: Transport::Local {
                executable: "/usr/bin/true".into(),
                args: Vec::new(),
                working_directory: None,
                environment: Vec::new(),
            },
        }
    }

    fn write(&self, entries: &[Declaration]) -> Result<(), Box<dyn std::error::Error>> {
        let edit = vega_store::yaml_config::begin(&self.config())?;
        edit.save_fields(&serde_json::json!({"mcp_servers": entries}))?;
        Ok(())
    }

    fn begin(
        &self,
    ) -> Result<vega_store::mcp_config::ReconciliationIntent, Box<dyn std::error::Error>> {
        let bytes = vega_store::yaml_config::read(&self.config())?;
        let prepared = vega_store::mcp_config::prepare_reconciliation(
            &self.store,
            bytes.as_deref(),
            validate_yaml_row,
        )?;
        Ok(crate::database_writer::commit(
            &self.root.join("database.db"),
            self.store.database_identity(),
            writer::BeginMcpReconciliation {
                registry: self.service.registry.clone(),
                prepared,
            },
        )??)
    }

    fn save(
        &self,
        intent: vega_store::mcp_config::ReconciliationIntent,
    ) -> Result<vega_store::mcp_config::PreparedConfirmation, Box<dyn std::error::Error>> {
        Ok(intent.save(
            &self.store,
            &self.config(),
            vega_store::yaml_config::begin(&self.config())?,
        )?)
    }

    fn confirm(
        &self,
        prepared: vega_store::mcp_config::PreparedConfirmation,
    ) -> Result<(), Box<dyn std::error::Error>> {
        crate::database_writer::commit(
            &self.root.join("database.db"),
            self.store.database_identity(),
            writer::ConfirmMcpReconciliation {
                registry: self.service.registry.clone(),
                prepared,
            },
        )??;
        Ok(())
    }

    fn pending(&self) -> Result<bool, Box<dyn std::error::Error>> {
        Ok(self.store.conn().query_row(
            "SELECT pending_declarations IS NOT NULL FROM mcp_config_state",
            [],
            |row| row.get(0),
        )?)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn import_is_disabled_until_file_and_sql_confirmation_and_recovers_lost_intent() -> TestResult {
    let fixture = Fixture::new()?;
    let declaration = fixture.declaration();
    fixture.write(std::slice::from_ref(&declaration))?;
    let intent = fixture.begin()?;
    assert!(fixture.pending()?);
    assert!(mcp_servers::list(fixture.store.conn())?.is_empty());
    assert!(
        vega_store::mcp_config::projection_current(fixture.store.conn(), &fixture.config())
            .is_err()
    );
    drop(intent);
    let recovered = fixture.begin()?;
    let confirmation = fixture.save(recovered)?;
    fixture.confirm(confirmation)?;
    assert!(!fixture.pending()?);
    let row =
        mcp_servers::find(fixture.store.conn(), &declaration.id)?.ok_or("missing projection")?;
    assert!(!row.enabled);
    assert_eq!(
        vega_store::mcp_config::projection_current(fixture.store.conn(), &fixture.config())?,
        vec![declaration]
    );
    assert!(
        vega_store::mcp_config::admitted(
            fixture.store.conn(),
            &fixture.config(),
            &row.id,
            row.config_revision
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn saved_but_unconfirmed_file_recovers_without_resaving_or_changing_identity() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.write(&[fixture.declaration()])?;
    drop(fixture.save(fixture.begin()?)?);
    let before = std::fs::read(fixture.config())?;
    fixture.confirm(fixture.save(fixture.begin()?)?)?;
    assert_eq!(std::fs::read(fixture.config())?, before);
    assert!(!fixture.pending()?);
    assert_eq!(mcp_servers::list(fixture.store.conn())?.len(), 1);
    Ok(())
}

#[test]
fn metadata_change_after_prepare_rejects_intent_without_partial_projection() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.write(&[fixture.declaration()])?;
    let bytes = vega_store::yaml_config::read(&fixture.config())?;
    let prepared = vega_store::mcp_config::prepare_reconciliation(
        &fixture.store,
        bytes.as_deref(),
        validate_yaml_row,
    )?;
    fixture
        .store
        .conn()
        .execute("UPDATE mcp_config_state SET generation=generation+1", [])?;
    let result = crate::database_writer::commit(
        &fixture.root.join("database.db"),
        fixture.store.database_identity(),
        writer::BeginMcpReconciliation {
            registry: fixture.service.registry.clone(),
            prepared,
        },
    )?;
    assert!(result.is_err());
    assert!(!fixture.pending()?);
    assert!(mcp_servers::list(fixture.store.conn())?.is_empty());
    Ok(())
}

#[test]
fn changed_file_after_save_cannot_admit_stale_projection_and_reconciles_new_source() -> TestResult {
    let fixture = Fixture::new()?;
    let mut declaration = fixture.declaration();
    fixture.write(std::slice::from_ref(&declaration))?;
    let confirmation = fixture.save(fixture.begin()?)?;
    declaration.display_name = "Externally changed".into();
    fixture.write(std::slice::from_ref(&declaration))?;
    fixture.confirm(confirmation)?;
    assert!(
        vega_store::mcp_config::projection_current(fixture.store.conn(), &fixture.config())
            .is_err()
    );
    fixture
        .service
        .reconcile_declarations()
        .map_err(|_| "reconcile new source")?;
    assert_eq!(
        vega_store::mcp_config::projection_current(fixture.store.conn(), &fixture.config())?,
        vec![declaration]
    );
    Ok(())
}

#[test]
fn prepared_confirmation_is_bound_to_database_and_sql_source() -> TestResult {
    let fixture = Fixture::new()?;
    let other = Fixture::new()?;
    fixture.write(&[fixture.declaration()])?;
    let confirmation = fixture.save(fixture.begin()?)?;
    let result = crate::database_writer::commit(
        &other.root.join("database.db"),
        other.store.database_identity(),
        writer::ConfirmMcpReconciliation {
            registry: other.service.registry.clone(),
            prepared: confirmation,
        },
    )?;
    assert!(result.is_err());
    assert!(mcp_servers::list(other.store.conn())?.is_empty());
    let confirmation = fixture.save(fixture.begin()?)?;
    fixture
        .store
        .conn()
        .execute("UPDATE mcp_config_state SET generation=generation+1", [])?;
    assert!(fixture.confirm(confirmation).is_err());
    assert!(fixture.pending()?);
    assert!(mcp_servers::list(fixture.store.conn())?.is_empty());
    Ok(())
}

#[derive(Clone, Copy, Debug)]
enum MetadataCase {
    Create,
    LocalRotation,
    OAuthInstall,
    LastError,
    Replace,
    Enable,
    Disconnect,
    Remove,
}

const METADATA_CASES: [MetadataCase; 8] = [
    MetadataCase::Create,
    MetadataCase::LocalRotation,
    MetadataCase::OAuthInstall,
    MetadataCase::LastError,
    MetadataCase::Replace,
    MetadataCase::Enable,
    MetadataCase::Disconnect,
    MetadataCase::Remove,
];

impl MetadataCase {
    fn command(self, local: &McpServerRow, remote: &McpServerRow) -> writer::McpMetadataWrite {
        let mut draft = draft_from_row(local);
        draft.local_env_refs_json = Some("[\"TOKEN\"]".into());
        match self {
            Self::Create => writer::McpMetadataWrite::Create(draft),
            Self::LocalRotation => writer::McpMetadataWrite::LocalRotation {
                id: local.id.clone(),
                revision: local.config_revision,
                variable: "TOKEN".into(),
            },
            Self::OAuthInstall => writer::McpMetadataWrite::OAuthInstall {
                id: remote.id.clone(),
                revision: remote.config_revision,
                issuer: "https://auth.example.com".into(),
                client_id: "client".into(),
            },
            Self::LastError => writer::McpMetadataWrite::LastError {
                id: local.id.clone(),
                revision: local.config_revision,
                code: Some("connection_failed".into()),
            },
            Self::Replace => {
                draft.display_name = "Replaced".into();
                writer::McpMetadataWrite::Replace {
                    id: local.id.clone(),
                    revision: local.config_revision,
                    draft,
                }
            }
            Self::Enable => writer::McpMetadataWrite::Enable {
                id: local.id.clone(),
                revision: local.config_revision,
                enabled: true,
            },
            Self::Disconnect => writer::McpMetadataWrite::Disconnect {
                id: remote.id.clone(),
                revision: remote.config_revision,
            },
            Self::Remove => writer::McpMetadataWrite::Remove {
                id: local.id.clone(),
                revision: local.config_revision,
            },
        }
    }
}

type ConfigState = (Option<Vec<u8>>, i64, Option<Vec<u8>>, Option<String>);

#[derive(Debug, PartialEq, Eq)]
struct SqlSnapshot {
    rows: Vec<McpServerRow>,
    cleanup: Vec<mcp_servers::McpSecretCleanup>,
    identities: Vec<(String, String, i64, bool)>,
    state: ConfigState,
}

impl Fixture {
    fn seeded() -> Result<(Self, McpServerRow, McpServerRow), Box<dyn std::error::Error>> {
        let fixture = Self::new()?;
        let mut local = fixture.declaration();
        if let Transport::Local { environment, .. } = &mut local.transport {
            environment.push(mcp_servers::McpEnvSlot {
                variable: "TOKEN".into(),
                credential_ref: format!("mcp-{}-r1-env-TOKEN", local.id),
            });
        }
        let mut remote = fixture.declaration();
        remote.transport = Transport::Remote {
            endpoint: "https://mcp.example.com".into(),
            allow_loopback_http: false,
            authorization: vega_store::mcp_config::Authorization::OAuth {
                credential_ref: None,
                issuer: None,
                client_id: None,
            },
        };
        fixture.write(&[local.clone(), remote.clone()])?;
        fixture.confirm(fixture.save(fixture.begin()?)?)?;
        let local = mcp_servers::find(fixture.store.conn(), &local.id)?.ok_or("missing local")?;
        let remote =
            mcp_servers::find(fixture.store.conn(), &remote.id)?.ok_or("missing remote")?;
        Ok((fixture, local, remote))
    }

    fn metadata(
        &self,
        command: writer::McpMetadataWrite,
    ) -> Result<writer::SaveMcpMetadata, McpStoreError> {
        Ok(writer::SaveMcpMetadata {
            registry: self.service.registry.clone(),
            expected_epoch: None,
            projection: writer::CleanupProjection::read(self.store.conn())?,
            command,
        })
    }

    fn commit_metadata(
        &self,
        command: writer::SaveMcpMetadata,
    ) -> Result<Result<McpServerRow, McpSettingsError>, Box<dyn std::error::Error>> {
        Ok(crate::database_writer::commit(
            &self.root.join("database.db"),
            self.store.database_identity(),
            command,
        )?)
    }

    fn snapshot(&self) -> Result<SqlSnapshot, McpStoreError> {
        let conn = self.store.conn();
        let mut stmt = conn.prepare("SELECT id, declaration, projected_revision, tombstoned FROM mcp_config_identities ORDER BY id")?;
        let identities = stmt
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(SqlSnapshot {
            rows: mcp_servers::list(conn)?, cleanup: mcp_servers::pending_cleanup(conn)?, identities,
            state: conn.query_row("SELECT accepted_digest, generation, pending_base_digest, pending_declarations FROM mcp_config_state WHERE singleton = 1", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))?,
        })
    }
}

#[test]
fn metadata_rejects_import_registered_by_another_connection_and_intent_recovers() -> TestResult {
    for case in METADATA_CASES {
        let (fixture, local, remote) = Fixture::seeded()?;
        let stale = fixture.metadata(case.command(&local, &remote))?;
        let mut changed = Declaration::from_row(&local)?;
        changed.display_name = "Imported edit".into();
        fixture.write(&[changed.clone(), Declaration::from_row(&remote)?])?;
        let other = Store::open(fixture.root.join("database.db"))?;
        let bytes = vega_store::yaml_config::read(&fixture.config())?;
        let prepared = vega_store::mcp_config::prepare_reconciliation(
            &other,
            bytes.as_deref(),
            validate_yaml_row,
        )?;
        drop(prepared.begin(&other)?);
        let before = fixture.snapshot()?;
        assert!(fixture.pending()?);
        assert!(
            matches!(
                fixture.commit_metadata(stale)?,
                Err(McpSettingsError::Conflict)
            ),
            "{case:?}"
        );
        assert_eq!(fixture.snapshot()?, before, "{case:?}");
        let pending = fixture.metadata(case.command(&local, &remote))?;
        assert!(
            matches!(
                fixture.commit_metadata(pending)?,
                Err(McpSettingsError::Conflict)
            ),
            "{case:?}"
        );
        assert_eq!(fixture.snapshot()?, before, "{case:?}");
        fixture.confirm(fixture.save(fixture.begin()?)?)?;
        assert!(!fixture.pending()?);
        let recovered = mcp_servers::find(fixture.store.conn(), &local.id)?.ok_or("lost local")?;
        assert_eq!(recovered.display_name, changed.display_name, "{case:?}");
        assert_eq!(
            recovered.config_revision,
            local.config_revision + 1,
            "{case:?}"
        );
        assert_eq!(recovered.last_error_code, None, "{case:?}");
    }
    Ok(())
}

#[test]
fn metadata_commands_commit_normal_changes_and_preserve_recovery() -> TestResult {
    for case in METADATA_CASES {
        let (fixture, local, mut remote) = Fixture::seeded()?;
        if matches!(case, MetadataCase::Disconnect) {
            let installed =
                fixture.metadata(MetadataCase::OAuthInstall.command(&local, &remote))?;
            remote = fixture.commit_metadata(installed)??;
            fixture.confirm(fixture.save(fixture.begin()?)?)?;
        }
        fixture
            .service
            .registry
            .lock()
            .map_err(|_| "registry poisoned")?
            .suspended
            .insert(local.id.clone());
        let before = fixture.snapshot()?;
        let command = fixture.metadata(case.command(&local, &remote))?;
        let row = fixture.commit_metadata(command)??;
        assert_eq!(
            mcp_servers::find(fixture.store.conn(), &row.id)?,
            Some(row.clone()),
            "{case:?}"
        );
        let cleanup = mcp_servers::pending_cleanup(fixture.store.conn())?;
        match case {
            MetadataCase::Create => {
                assert_ne!(row.id, local.id);
                assert_eq!(row.config_revision, 1);
                assert!(!row.enabled);
                assert_eq!(mcp_servers::list(fixture.store.conn())?.len(), 3);
            }
            MetadataCase::LocalRotation => {
                assert_eq!(row.config_revision, local.config_revision + 1);
                assert!(!row.enabled);
                assert!(
                    row.local_env_refs_json
                        .as_deref()
                        .ok_or("missing slots")?
                        .contains(&format!("mcp-{}-r2-env-TOKEN", local.id))
                );
                assert_eq!(
                    cleanup,
                    vec![mcp_servers::McpSecretCleanup {
                        server_id: local.id.clone(),
                        credential_ref: format!("mcp-{}-r1-env-TOKEN", local.id)
                    }]
                );
            }
            MetadataCase::OAuthInstall => {
                assert_eq!(row.config_revision, remote.config_revision + 1);
                assert_eq!(
                    row.remote_credential_ref,
                    Some(format!("mcp-{}-r2-oauth", remote.id))
                );
                assert_eq!(
                    row.remote_oauth_issuer.as_deref(),
                    Some("https://auth.example.com")
                );
                assert!(!row.enabled);
            }
            MetadataCase::LastError => {
                let mut expected = local.clone();
                expected.last_error_code = Some("connection_failed".into());
                assert_eq!(row, expected);
                let after = fixture.snapshot()?;
                assert_eq!(after.state, before.state);
                assert_eq!(after.identities, before.identities);
                assert_eq!(after.cleanup, before.cleanup);
            }
            MetadataCase::Replace => {
                assert_eq!(row.display_name, "Replaced");
                assert_eq!(row.config_revision, local.config_revision + 1);
                assert!(!row.enabled);
                assert_eq!(cleanup.len(), 1);
            }
            MetadataCase::Enable => {
                assert!(row.enabled);
                assert_eq!(row.config_revision, local.config_revision + 1);
                assert!(
                    !fixture
                        .service
                        .registry
                        .lock()
                        .map_err(|_| "registry poisoned")?
                        .suspended
                        .contains(&local.id)
                );
            }
            MetadataCase::Disconnect => {
                assert!(!row.enabled);
                assert_eq!(row.config_revision, remote.config_revision + 1);
                assert_eq!(row.remote_credential_ref, None);
                assert_eq!(row.remote_oauth_issuer, None);
                assert_eq!(
                    cleanup,
                    vec![mcp_servers::McpSecretCleanup {
                        server_id: remote.id.clone(),
                        credential_ref: remote
                            .remote_credential_ref
                            .clone()
                            .ok_or("missing oauth slot")?
                    }]
                );
            }
            MetadataCase::Remove => {
                assert!(row.deleting);
                assert!(!row.enabled);
                assert_eq!(row.config_revision, local.config_revision + 1);
                assert_eq!(cleanup.len(), 1);
            }
        }
        if !matches!(case, MetadataCase::LastError) {
            assert!(fixture.pending()?, "{case:?}");
            fixture.confirm(fixture.save(fixture.begin()?)?)?;
            assert!(!fixture.pending()?, "{case:?}");
        }
    }
    Ok(())
}

#[test]
fn confirmation_rejects_changed_rows_identities_and_digest_without_partial_writes() -> TestResult {
    for sql in [
        "UPDATE mcp_servers SET last_error_code = 'connection_failed'",
        "UPDATE mcp_config_identities SET projected_revision = projected_revision + 1",
        "UPDATE mcp_config_state SET accepted_digest = X'0102'",
    ] {
        let (fixture, local, remote) = Fixture::seeded()?;
        let mut changed = Declaration::from_row(&local)?;
        changed.display_name = "New import".into();
        fixture.write(&[changed, Declaration::from_row(&remote)?])?;
        let confirmation = fixture.save(fixture.begin()?)?;
        let other = Store::open(fixture.root.join("database.db"))?;
        other.conn().execute(sql, [])?;
        let before = fixture.snapshot()?;
        assert!(fixture.confirm(confirmation).is_err(), "{sql}");
        assert_eq!(fixture.snapshot()?, before, "{sql}");
        assert!(fixture.pending()?);
    }
    Ok(())
}

#[test]
fn typed_metadata_helpers_use_outer_transaction_and_nested_begin_stays_rejected() -> TestResult {
    let (fixture, local, _) = Fixture::seeded()?;
    let tx = fixture.store.immediate_transaction()?;
    assert!(fixture.store.immediate_transaction().is_err());
    assert!(
        mcp_servers::set_last_error(
            &tx,
            &local.id,
            local.config_revision,
            Some("connection_failed")
        )
        .is_err()
    );
    mcp_servers::set_last_error_in_transaction(
        &tx,
        &local.id,
        local.config_revision,
        Some("connection_failed"),
    )?;
    let other = Store::open_with_busy_timeout(fixture.root.join("database.db"), Duration::ZERO)?;
    assert_eq!(
        mcp_servers::find(other.conn(), &local.id)?,
        Some(local.clone())
    );
    assert!(other.conn().execute_batch("BEGIN IMMEDIATE").is_err());
    tx.rollback()?;
    assert_eq!(
        mcp_servers::find(fixture.store.conn(), &local.id)?,
        Some(local)
    );
    other.conn().execute_batch("BEGIN IMMEDIATE; ROLLBACK")?;
    Ok(())
}

#[test]
fn metadata_rejects_stale_revisions_without_sql_or_registry_changes() -> TestResult {
    for case in METADATA_CASES {
        if matches!(case, MetadataCase::Create) {
            continue;
        }
        let (fixture, mut local, mut remote) = Fixture::seeded()?;
        fixture
            .service
            .registry
            .lock()
            .map_err(|_| "registry poisoned")?
            .suspended
            .insert(local.id.clone());
        local.config_revision += 1;
        remote.config_revision += 1;
        let before = fixture.snapshot()?;
        let command = fixture.metadata(case.command(&local, &remote))?;
        assert!(
            matches!(
                fixture.commit_metadata(command)?,
                Err(McpSettingsError::Conflict)
            ),
            "{case:?}"
        );
        assert_eq!(fixture.snapshot()?, before, "{case:?}");
        assert!(
            fixture
                .service
                .registry
                .lock()
                .map_err(|_| "registry poisoned")?
                .suspended
                .contains(&local.id),
            "{case:?}"
        );
    }
    Ok(())
}
