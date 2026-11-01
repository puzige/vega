use super::*;
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;

struct Fixture {
    root: PathBuf,
    service: McpServerSettingsService,
    id: String,
}

impl Fixture {
    fn imported(authorization: &str) -> Self {
        let root = std::fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("vega-mcp-suspension-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let fixture = Self {
            service: McpServerSettingsService::new(root.join("vega.db"), root.clone()),
            root,
            id: ulid::Ulid::generate().to_string(),
        };
        fixture.write("Initial", true, authorization);
        fixture
    }

    async fn new(authorization: &str) -> Self {
        let fixture = Self::imported(authorization);
        let row = fixture.row();
        let enabled = fixture
            .service
            .set_enabled(&fixture.id, row.config_revision, true, true)
            .await
            .unwrap();
        assert!(enabled.enabled);
        fixture
    }

    fn declaration(&self, name: &str, enabled: bool, authorization: &str) -> Value {
        use vega_store::mcp_config::Authorization;
        let authorization = match authorization {
            "bearer" => Authorization::Bearer {
                credential_ref: None,
            },
            "oauth" => Authorization::OAuth {
                credential_ref: None,
                issuer: None,
                client_id: None,
            },
            _ => panic!("unsupported fixture authorization"),
        };
        json!({
            "id": self.id,
            "display_name": name,
            "enabled": enabled,
            "transport": {
                "type": "remote",
                "endpoint": "https://example.invalid/mcp",
                "authorization": authorization
            }
        })
    }

    fn write(&self, name: &str, enabled: bool, authorization: &str) {
        self.write_bytes(
            &serde_json::to_vec(&json!({
                "schema_version": 1,
                "mcp_servers": [self.declaration(name, enabled, authorization)]
            }))
            .unwrap(),
        );
    }

    fn write_bytes(&self, bytes: &[u8]) {
        let path = self.root.join("config.yaml");
        std::fs::write(&path, bytes).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    fn row(&self) -> McpServerRow {
        let store = self.service.store().unwrap();
        mcp_servers::find(store.conn(), &self.id).unwrap().unwrap()
    }

    fn register_handle(&self) -> McpReadyServer {
        let row = self.row();
        let handle = McpReadyServer::isolated_test_handle(self.id.clone(), row.config_revision);
        self.service
            .lock()
            .unwrap()
            .leases
            .entry(self.id.clone())
            .or_default()
            .push((row.config_revision, handle.revocation_lease()));
        handle
    }

    fn token(&self) -> CancellationToken {
        self.service.lock().unwrap().connection_token(&self.id)
    }

    async fn assert_credential_admission(&self) {
        let row = self.row();
        assert!(matches!(
            self.service.ensure_ready(&row).await,
            Err(McpSettingsError::Credential)
        ));
        assert!(!self.service.lock().unwrap().suspended.contains(&self.id));
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

#[tokio::test]
async fn imported_configuration_remains_disabled_until_explicit_activation() {
    let fixture = Fixture::imported("bearer");
    assert!(!fixture.row().enabled);
    assert!(
        fixture
            .service
            .lock()
            .unwrap()
            .suspended
            .contains(&fixture.id)
    );
    let readiness = fixture.service.ready_for_run().await.unwrap();
    assert!(readiness.ready_servers.is_empty());
    assert!(readiness.unavailable.is_empty());
}

#[tokio::test]
async fn explicitly_enabled_configuration_reaches_fresh_connection_admission() {
    let fixture = Fixture::new("bearer").await;
    fixture.assert_credential_admission().await;
    let readiness = fixture.service.ready_for_run().await.unwrap();
    assert!(readiness.ready_servers.is_empty());
    assert_eq!(readiness.unavailable.len(), 1);
    assert_eq!(readiness.unavailable[0].code, "credential_missing");
}

#[tokio::test]
async fn accepted_configuration_change_still_requires_explicit_activation() {
    let fixture = Fixture::new("bearer").await;
    let old = fixture.row();
    let token = fixture.token();
    let epoch = fixture.service.lock().unwrap().epoch(&fixture.id);
    fixture.write("Updated", true, "bearer");
    let current = fixture.row();
    assert!(current.config_revision > old.config_revision);
    assert!(token.is_cancelled());
    assert!(fixture.service.lock().unwrap().epoch(&fixture.id) > epoch);
    assert!(!current.enabled);
    assert!(
        fixture
            .service
            .lock()
            .unwrap()
            .suspended
            .contains(&fixture.id)
    );
    let readiness = fixture.service.ready_for_run().await.unwrap();
    assert!(readiness.ready_servers.is_empty());
    assert!(readiness.unavailable.is_empty());
    assert!(token.is_cancelled());
    assert!(matches!(
        fixture
            .service
            .test_connection(&fixture.id, old.config_revision, true)
            .await,
        Err(McpSettingsError::Conflict)
    ));
}

#[tokio::test]
async fn configuration_error_fails_closed_then_recovers_without_reviving_old_attempt() {
    let fixture = Fixture::new("bearer").await;
    let old = fixture.register_handle();
    let old_clone = old.clone();
    let lease = old.revocation_lease();
    let token = fixture.token();
    let epoch = fixture.service.lock().unwrap().epoch(&fixture.id);
    assert!(lease.is_live());
    fixture.write_bytes(b"mcp_servers: [");
    assert!(fixture.service.store().is_err());
    assert!(token.is_cancelled());
    assert!(fixture.service.lock().unwrap().epoch(&fixture.id) > epoch);
    assert!(old.is_revoked());
    assert!(old_clone.is_revoked());
    assert!(!lease.is_live());
    fixture.write("Initial", true, "bearer");
    fixture.assert_credential_admission().await;
    assert!(!fixture.token().is_cancelled());
    let fresh = fixture.register_handle();
    assert!(fresh.revocation_lease().is_live());
    assert!(!lease.same_connection(&fresh.revocation_lease()));
    assert!(old.is_revoked());
    assert!(old_clone.is_revoked());
    assert!(!lease.is_live());
    assert!(token.is_cancelled());
}

#[tokio::test]
async fn explicit_disable_excludes_future_runs_and_cancels_old_attempt() {
    let fixture = Fixture::new("bearer").await;
    let row = fixture.row();
    let token = fixture.token();
    let view = fixture
        .service
        .set_enabled(&fixture.id, row.config_revision, false, false)
        .await
        .unwrap();
    assert!(!view.enabled);
    assert!(token.is_cancelled());
    let readiness = fixture.service.ready_for_run().await.unwrap();
    assert!(readiness.ready_servers.is_empty());
    assert!(readiness.unavailable.is_empty());
    assert!(
        fixture
            .service
            .lock()
            .unwrap()
            .suspended
            .contains(&fixture.id)
    );
}

#[tokio::test]
async fn explicit_delete_cannot_be_revived_by_reconciliation() {
    let fixture = Fixture::new("bearer").await;
    let row = fixture.row();
    let token = fixture.token();
    fixture
        .service
        .remove(&fixture.id, row.config_revision, true)
        .unwrap();
    assert!(token.is_cancelled());
    assert!(fixture.service.list().unwrap().is_empty());
    let readiness = fixture.service.ready_for_run().await.unwrap();
    assert!(readiness.ready_servers.is_empty());
    assert!(readiness.unavailable.is_empty());
    fixture.write("Restored", true, "bearer");
    assert!(fixture.service.store().is_err());
}

#[tokio::test]
async fn pending_delete_remains_disabled_and_is_never_connected() {
    let fixture = Fixture::new("bearer").await;
    let row = fixture.row();
    {
        let store = fixture.service.store().unwrap();
        mcp_servers::begin_remove(store.conn(), &fixture.id, row.config_revision).unwrap();
    }
    let pending = fixture.row();
    assert!(!pending.enabled);
    assert!(pending.deleting);
    let readiness = fixture.service.ready_for_run().await.unwrap();
    assert!(readiness.ready_servers.is_empty());
    assert!(readiness.unavailable.is_empty());
}

#[tokio::test]
async fn oauth_disconnect_preserves_revocation_and_requires_explicit_authorization() {
    let fixture = Fixture::new("oauth").await;
    let row = fixture.row();
    let reference = {
        let store = fixture.service.store().unwrap();
        let (_, reference) = mcp_servers::begin_oauth_install(
            store.conn(),
            &fixture.id,
            row.config_revision,
            "https://issuer.invalid",
            "synthetic-client",
        )
        .unwrap();
        vega_store::keystore::set_key(&fixture.root, &reference, "synthetic-authorization")
            .unwrap();
        reference
    };
    fixture.service.reconcile_pending().unwrap();
    let installed = fixture.row();
    assert_eq!(
        installed.remote_credential_ref.as_deref(),
        Some(reference.as_str())
    );
    assert_eq!(
        installed.remote_oauth_issuer.as_deref(),
        Some("https://issuer.invalid")
    );
    let old = fixture.register_handle();
    let token = fixture.token();
    let view = fixture
        .service
        .disconnect_oauth(&fixture.id, installed.config_revision, true)
        .unwrap();
    assert!(!view.enabled);
    assert!(token.is_cancelled());
    let current = fixture.row();
    assert!(current.remote_credential_ref.is_none());
    assert!(current.remote_oauth_issuer.is_none());
    assert!(matches!(
        vega_store::keystore::get_key(&fixture.root, &reference),
        Err(vega_store::keystore::Error::Missing)
    ));
    let repaired = std::fs::read(fixture.root.join("config.yaml")).unwrap();
    fixture.write_bytes(b"mcp_servers: [");
    assert!(fixture.service.store().is_err());
    fixture.write_bytes(&repaired);
    assert!(!fixture.row().enabled);
    assert!(old.is_revoked());
    let readiness = fixture.service.ready_for_run().await.unwrap();
    assert!(readiness.ready_servers.is_empty());
    assert!(readiness.unavailable.is_empty());
    assert!(
        fixture
            .service
            .lock()
            .unwrap()
            .suspended
            .contains(&fixture.id)
    );
}

#[tokio::test]
async fn repaired_uncredentialed_oauth_requires_authorization_without_starting_a_flow() {
    let fixture = Fixture::new("oauth").await;
    let saved = std::fs::read(fixture.root.join("config.yaml")).unwrap();
    let token = fixture.token();
    fixture.write_bytes(b"mcp_servers: [");
    assert!(fixture.service.store().is_err());
    fixture.write_bytes(&saved);
    let readiness = fixture.service.ready_for_run().await.unwrap();
    assert!(readiness.ready_servers.is_empty());
    assert_eq!(readiness.unavailable.len(), 1);
    assert_eq!(readiness.unavailable[0].code, "authorization_required");
    let state = fixture.service.lock().unwrap();
    assert!(!state.suspended.contains(&fixture.id));
    assert!(state.prepared_oauth.is_empty());
    assert!(state.pending_oauth.is_empty());
    assert!(state.active_oauth.is_empty());
    assert!(token.is_cancelled());
}

#[tokio::test]
async fn bearer_rotation_revokes_old_authority_and_does_not_reenable_service() {
    let fixture = Fixture::new("bearer").await;
    let row = fixture.row();
    let token = fixture.token();
    let view = fixture
        .service
        .set_bearer_secret(
            &fixture.id,
            row.config_revision,
            "synthetic-test-secret".into(),
        )
        .unwrap();
    assert!(!view.enabled);
    assert!(token.is_cancelled());
    let current = fixture.row();
    assert!(current.config_revision > row.config_revision);
    assert!(current.remote_credential_ref.is_some());
    let readiness = fixture.service.ready_for_run().await.unwrap();
    assert!(readiness.ready_servers.is_empty());
    assert!(readiness.unavailable.is_empty());
    assert!(
        fixture
            .service
            .lock()
            .unwrap()
            .suspended
            .contains(&fixture.id)
    );
}

#[tokio::test]
async fn failed_explicit_disable_stays_suspended_after_valid_configuration_change() {
    let fixture = Fixture::new("bearer").await;
    let row = fixture.row();
    let token = fixture.token();
    assert!(matches!(
        fixture
            .service
            .set_enabled(&fixture.id, row.config_revision + 1, false, false)
            .await,
        Err(McpSettingsError::Conflict)
    ));
    assert!(token.is_cancelled());
    let path = fixture.root.join("config.yaml");
    let mut config: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    config["providers"] = json!([]);
    fixture.write_bytes(&serde_json::to_vec(&config).unwrap());
    let current = fixture.row();
    assert!(current.enabled);
    assert!(matches!(
        fixture.service.ensure_ready(&current).await,
        Err(McpSettingsError::Conflict)
    ));
}

#[tokio::test]
async fn failed_explicit_delete_stays_suspended_after_configuration_repair() {
    let fixture = Fixture::new("bearer").await;
    let row = fixture.row();
    let token = fixture.token();
    assert!(matches!(
        fixture
            .service
            .remove(&fixture.id, row.config_revision + 1, true),
        Err(McpSettingsError::Conflict)
    ));
    assert!(token.is_cancelled());
    fixture.write_bytes(b"mcp_servers: [");
    assert!(fixture.service.store().is_err());
    fixture.write("Initial", true, "bearer");
    let current = fixture.row();
    assert!(current.enabled);
    assert!(matches!(
        fixture.service.ensure_ready(&current).await,
        Err(McpSettingsError::Conflict)
    ));
}

#[tokio::test]
async fn explicit_reenable_clears_only_current_suspension_not_old_attempt_cancellation() {
    let fixture = Fixture::new("bearer").await;
    let row = fixture.row();
    let token = fixture.token();
    let disabled = fixture
        .service
        .set_enabled(&fixture.id, row.config_revision, false, false)
        .await
        .unwrap();
    let enabled = fixture
        .service
        .set_enabled(&fixture.id, disabled.config_revision, true, true)
        .await
        .unwrap();
    assert!(enabled.enabled);
    fixture.assert_credential_admission().await;
    assert!(token.is_cancelled());
}
