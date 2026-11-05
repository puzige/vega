use super::*;
use crate::Store;
use std::os::unix::fs::DirBuilderExt;
use std::path::PathBuf;
use std::time::Duration;

struct Fixture {
    root: PathBuf,
    store: Store,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("vega-config-{}", ulid::Ulid::generate()));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .unwrap();
        let root = root.canonicalize().unwrap();
        let store = Store::open(root.join("state.db")).unwrap();
        store.migrate().unwrap();
        store.conn().busy_timeout(Duration::ZERO).unwrap();
        Self { root, store }
    }

    fn config(&self) -> PathBuf {
        self.root.join("config.yaml")
    }

    fn declaration(&self) -> Declaration {
        Declaration {
            id: ulid::Ulid::generate().to_string(),
            display_name: "synthetic".into(),
            enabled: false,
            transport: Transport::Remote {
                endpoint: "https://example.invalid/mcp".into(),
                allow_loopback_http: false,
                authorization: Authorization::None,
            },
        }
    }

    fn publish(&self, declarations: &[Declaration]) {
        crate::yaml_config::begin(&self.config())
            .unwrap()
            .save_fields(&serde_json::json!({"mcp_servers": declarations}))
            .unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn unchanged_projection_reads_while_another_connection_owns_writer() {
    let fixture = Fixture::new();
    let declaration = fixture.declaration();
    fixture.publish(std::slice::from_ref(&declaration));
    assert_eq!(
        reconcile(fixture.store.conn(), &fixture.config(), |_| Ok(())).unwrap(),
        vec![declaration.id.clone()]
    );
    let writer = Store::open_existing(fixture.root.join("state.db")).unwrap();
    let held = writer.immediate_transaction().unwrap();
    let before = crate::write_coordination::recent_observations(fixture.store.conn());
    let generation: i64 = fixture
        .store
        .conn()
        .query_row("SELECT generation FROM mcp_config_state", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert!(
        reconcile(fixture.store.conn(), &fixture.config(), |_| Ok(()))
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        crate::write_coordination::recent_observations(fixture.store.conn()),
        before
    );
    assert_eq!(
        fixture
            .store
            .conn()
            .query_row("SELECT generation FROM mcp_config_state", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        generation
    );
    held.rollback().unwrap();
}

#[test]
fn enabled_yaml_with_disabled_projection_does_not_wait_for_writer() {
    let fixture = Fixture::new();
    let mut declaration = fixture.declaration();
    declaration.enabled = true;
    fixture.publish(std::slice::from_ref(&declaration));
    assert_eq!(
        reconcile(fixture.store.conn(), &fixture.config(), |_| Ok(())).unwrap(),
        vec![declaration.id.clone()]
    );
    let row = mcp_servers::find(fixture.store.conn(), &declaration.id)
        .unwrap()
        .unwrap();
    assert!(!row.enabled);
    let generation: i64 = fixture
        .store
        .conn()
        .query_row("SELECT generation FROM mcp_config_state", [], |row| {
            row.get(0)
        })
        .unwrap();
    let bytes = std::fs::read(fixture.config()).unwrap();
    let writer = Store::open_existing(fixture.root.join("state.db")).unwrap();
    let held = writer.immediate_transaction().unwrap();
    let before = crate::write_coordination::recent_observations(fixture.store.conn());
    assert!(
        reconcile(fixture.store.conn(), &fixture.config(), |_| Ok(()))
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        crate::write_coordination::recent_observations(fixture.store.conn()),
        before
    );
    assert_eq!(
        mcp_servers::find(fixture.store.conn(), &declaration.id)
            .unwrap()
            .unwrap(),
        row
    );
    assert_eq!(
        fixture
            .store
            .conn()
            .query_row("SELECT generation FROM mcp_config_state", [], |row| row
                .get::<_, i64>(0),)
            .unwrap(),
        generation
    );
    assert_eq!(std::fs::read(fixture.config()).unwrap(), bytes);
    held.rollback().unwrap();
}

#[test]
fn projection_fast_path_rejects_tombstoned_or_changed_revision() {
    let fixture = Fixture::new();
    let declaration = fixture.declaration();
    fixture.publish(std::slice::from_ref(&declaration));
    reconcile(fixture.store.conn(), &fixture.config(), |_| Ok(())).unwrap();
    fixture
        .store
        .conn()
        .execute(
            "UPDATE mcp_config_identities SET tombstoned=1 WHERE id=?1",
            [&declaration.id],
        )
        .unwrap();
    assert!(matches!(
        reconcile(fixture.store.conn(), &fixture.config(), |_| Ok(())),
        Err(McpStoreError::Conflict)
    ));
    fixture.store.conn().execute("UPDATE mcp_config_identities SET tombstoned=0,projected_revision=projected_revision+1 WHERE id=?1", [&declaration.id]).unwrap();
    let bytes = crate::yaml_config::read(&fixture.config()).unwrap();
    assert!(
        !projection_unchanged(
            fixture.store.conn(),
            bytes.as_deref(),
            &[declaration],
            &|_| Ok(())
        )
        .unwrap()
    );
}

#[test]
fn changed_configuration_reconciles_then_removal_revokes_projection() {
    let fixture = Fixture::new();
    let mut declaration = fixture.declaration();
    fixture.publish(std::slice::from_ref(&declaration));
    reconcile(fixture.store.conn(), &fixture.config(), |_| Ok(())).unwrap();
    let original = mcp_servers::find(fixture.store.conn(), &declaration.id)
        .unwrap()
        .unwrap();
    declaration.display_name = "updated".into();
    fixture.publish(std::slice::from_ref(&declaration));
    assert_eq!(
        reconcile(fixture.store.conn(), &fixture.config(), |_| Ok(())).unwrap(),
        vec![declaration.id.clone()]
    );
    let changed = mcp_servers::find(fixture.store.conn(), &declaration.id)
        .unwrap()
        .unwrap();
    assert_eq!(changed.display_name, "updated");
    assert_eq!(changed.config_revision, original.config_revision + 1);
    fixture.publish(&[]);
    assert_eq!(
        reconcile(fixture.store.conn(), &fixture.config(), |_| Ok(())).unwrap(),
        vec![declaration.id.clone()]
    );
    let removed = mcp_servers::find(fixture.store.conn(), &declaration.id)
        .unwrap()
        .unwrap();
    assert!(removed.deleting);
    assert!(!removed.enabled);
}

#[test]
fn published_pending_configuration_recovers_after_database_busy() {
    let fixture = Fixture::new();
    reconcile(fixture.store.conn(), &fixture.config(), |_| Ok(())).unwrap();
    let row = mcp_servers::create(
        fixture.store.conn(),
        &mcp_servers::McpServerDraft {
            display_name: "synthetic".into(),
            transport: "remote".into(),
            local_executable: None,
            local_args_json: None,
            local_working_directory: None,
            local_env_refs_json: None,
            remote_endpoint: Some("https://example.invalid/mcp".into()),
            remote_allow_loopback_http: false,
            remote_auth_mode: Some("none".into()),
            remote_credential_ref: None,
            remote_oauth_issuer: None,
            remote_oauth_client_id: None,
        },
    )
    .unwrap();
    let declaration = Declaration::from_row(&row).unwrap();
    let writer = Connection::open(fixture.root.join("state.db")).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    let error = reconcile(fixture.store.conn(), &fixture.config(), |_| Ok(())).unwrap_err();
    assert!(
        matches!(error, McpStoreError::Sql(rusqlite::Error::SqliteFailure(code, _)) if code.code == rusqlite::ErrorCode::DatabaseBusy)
    );
    assert_eq!(
        parse(
            crate::yaml_config::read(&fixture.config())
                .unwrap()
                .as_deref()
        )
        .unwrap(),
        vec![declaration.clone()]
    );
    assert_eq!(
        mcp_servers::find(fixture.store.conn(), &declaration.id)
            .unwrap()
            .unwrap(),
        row
    );
    assert!(
        fixture
            .store
            .conn()
            .query_row(
                "SELECT pending_declarations IS NOT NULL FROM mcp_config_state",
                [],
                |row| row.get::<_, bool>(0)
            )
            .unwrap()
    );
    writer.execute_batch("ROLLBACK").unwrap();
    assert!(
        reconcile(fixture.store.conn(), &fixture.config(), |_| Ok(()))
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        mcp_servers::find(fixture.store.conn(), &declaration.id)
            .unwrap()
            .unwrap()
            .display_name,
        declaration.display_name
    );
    assert!(
        fixture
            .store
            .conn()
            .query_row(
                "SELECT pending_declarations IS NULL FROM mcp_config_state",
                [],
                |row| row.get::<_, bool>(0)
            )
            .unwrap()
    );
}
