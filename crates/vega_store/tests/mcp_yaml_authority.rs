use std::path::PathBuf;
use vega_store::{Store, config, mcp_config, mcp_servers, yaml_config};

struct Installation(PathBuf);
impl Installation {
    fn new() -> Self {
        Self(
            std::env::temp_dir()
                .canonicalize()
                .expect("temporary root")
                .join(format!("vega-mcp-yaml-{}", ulid::Ulid::new())),
        )
    }
    fn config(&self) -> PathBuf {
        self.0.join("config.yaml")
    }
    fn store(&self) -> Store {
        config::load_from(&self.config()).expect("initialize config");
        let store = Store::open(self.0.join("vega.db")).expect("database");
        store.migrate().expect("schema");
        mcp_config::reconcile(store.conn(), &self.config(), |_| Ok(()))
            .expect("initial projection");
        store
    }
}
impl Drop for Installation {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn draft() -> mcp_servers::McpServerDraft {
    mcp_servers::McpServerDraft {
        display_name: "Synthetic".into(),
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
    }
}

#[test]
fn interrupted_publication_recovers_stable_id_and_revision() {
    let root = Installation::new();
    let store = root.store();
    let row = mcp_servers::create(store.conn(), &draft()).expect("create pending");
    assert!(mcp_config::projection_current(store.conn(), &root.config()).is_err());
    drop(store);
    let store = Store::open(root.0.join("vega.db")).expect("reopen");
    mcp_config::reconcile(store.conn(), &root.config(), |_| Ok(())).expect("recover");
    let declarations =
        mcp_config::projection_current(store.conn(), &root.config()).expect("current");
    assert_eq!(declarations.len(), 1);
    assert_eq!(declarations[0].id, row.id);
    assert!(!declarations[0].enabled);
    let recovered = mcp_servers::find(store.conn(), &row.id)
        .expect("lookup")
        .expect("row");
    assert_eq!(recovered.config_revision, row.config_revision);
    assert!(!recovered.enabled);
    assert!(
        mcp_config::reconcile(store.conn(), &root.config(), |_| Ok(()))
            .expect("repeat")
            .is_empty()
    );
}

#[test]
fn hand_edit_cannot_activate_and_removed_identity_cannot_be_resurrected() {
    let root = Installation::new();
    let store = root.store();
    let row = mcp_servers::create(store.conn(), &draft()).expect("create");
    mcp_config::reconcile(store.conn(), &root.config(), |_| Ok(())).expect("publish");
    let mut declarations =
        mcp_config::projection_current(store.conn(), &root.config()).expect("current");
    declarations[0].enabled = true;
    yaml_config::update_section(&root.config(), "mcp_servers", &declarations, None)
        .expect("manual intent");
    assert!(
        mcp_config::admitted(store.conn(), &root.config(), &row.id, row.config_revision).is_err()
    );
    mcp_config::reconcile(store.conn(), &root.config(), |_| Ok(())).expect("reconcile intent");
    let projected = mcp_servers::find(store.conn(), &row.id)
        .expect("find")
        .expect("row");
    assert!(!projected.enabled);
    assert!(
        mcp_config::admitted(
            store.conn(),
            &root.config(),
            &row.id,
            projected.config_revision
        )
        .is_err()
    );
    yaml_config::update_section(
        &root.config(),
        "mcp_servers",
        &Vec::<mcp_config::Declaration>::new(),
        None,
    )
    .expect("remove intent");
    mcp_config::reconcile(store.conn(), &root.config(), |_| Ok(())).expect("tombstone");
    yaml_config::update_section(&root.config(), "mcp_servers", &declarations, None)
        .expect("attempt reuse");
    assert!(mcp_config::reconcile(store.conn(), &root.config(), |_| Ok(())).is_err());
}

#[test]
fn unrelated_external_edit_does_not_overwrite_pending_publication() {
    let root = Installation::new();
    let store = root.store();
    let row = mcp_servers::create(store.conn(), &draft()).expect("create pending");
    let external = "schema_version: 1\nexternal: keep\n";
    std::fs::write(root.config(), external).expect("external edit");
    assert!(mcp_config::reconcile(store.conn(), &root.config(), |_| Ok(())).is_err());
    assert_eq!(
        std::fs::read_to_string(root.config()).expect("read"),
        external
    );
    assert!(
        mcp_config::admitted(store.conn(), &root.config(), &row.id, row.config_revision).is_err()
    );
}
