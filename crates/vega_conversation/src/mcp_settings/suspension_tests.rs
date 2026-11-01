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
        let root = std::env::temp_dir().join(format!("vega-mcp-suspension-{}", ulid::Ulid::new()));
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let fixture = Self {
            service: McpServerSettingsService::new(root.join("vega.db"), root.clone()),
            root,
            id: ulid::Ulid::new().to_string(),
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
        json!({
            "id": self.id,
            "display_name": name,
            "enabled": enabled,
            "transport": {
                "type": "remote",
                "endpoint": "https://example.invalid/mcp",
                "authorization": {"type": authorization}
            }
        })
    }

    fn write(&self, name: &str, enabled: bool, authorization: &str) {
        self.write_bytes(
            &serde_json::to_vec(&json!({
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
    fixture.row();
    let token = fixture.token();
    fixture.write_bytes(b"mcp_servers: [");
    assert!(fixture.service.store().is_err());
    assert!(token.is_cancelled());
    fixture.write("Initial", true, "bearer");
    fixture.assert_credential_admission().await;
    assert!(!fixture.token().is_cancelled());
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
    let token = fixture.token();
    let view = fixture
        .service
        .disconnect_oauth(&fixture.id, row.config_revision, true)
        .unwrap();
    assert!(!view.enabled);
    assert!(token.is_cancelled());
    let current = fixture.row();
    assert!(current.remote_credential_ref.is_none());
    assert!(current.remote_oauth_issuer.is_none());
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
