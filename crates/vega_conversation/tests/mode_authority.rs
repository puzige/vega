use vega_conversation::{
    permission_contracts::*, thread_permissions::ThreadPermissionService, threads, types::*,
};
use vega_store::{Store, config::AppConfig};

struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("vega-mode-{}", ulid::Ulid::generate()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn legacy_defaults_materialize_without_valid_grants() {
    for grant in [None, Some("malformed legacy grant".to_owned())] {
        let fixture = Fixture::new();
        let store = Store::open(fixture.0.join("db")).unwrap();
        store.migrate().unwrap();
        let path = fixture.0.join("config.toml");
        let mut config = AppConfig::default();
        config.defaults.permission_mode = "full_access".into();
        config.defaults.full_access_grant = grant;
        config.save_to(&path).unwrap();
        let draft = threads::draft_thread_from_defaults(
            None,
            "model",
            default_permission_snapshot(Some(&path), &config.defaults),
        );
        let saved = threads::materialize_draft(&store, &draft).unwrap();
        assert_eq!(saved.permission_mode, PermissionMode::FullAccess);
        assert!(saved.full_access_grant.is_none());
        store
            .conn()
            .execute(
                "UPDATE threads SET full_access_grant='invalid' WHERE id=?1",
                [&saved.id],
            )
            .unwrap();
        assert_eq!(
            threads::get_thread(&store, &saved.id)
                .unwrap()
                .permission_mode,
            PermissionMode::FullAccess
        );
    }
}

#[test]
fn mode_selection_stays_draft_and_old_epoch_is_rejected() {
    let fixture = Fixture::new();
    let path = fixture.0.join("db");
    let store = Store::open(&path).unwrap();
    store.migrate().unwrap();
    let service = ThreadPermissionService::new(path);
    let draft = threads::draft_thread(None, "model", "confirm");
    let change = service.prepare(&draft, PermissionMode::FullAccess).unwrap();
    let selected = service.save(&change).unwrap();
    assert!(
        vega_store::threads::find(store.conn(), &draft.id)
            .unwrap()
            .is_none()
    );
    assert_eq!(selected.permission_mode, PermissionMode::FullAccess);
    assert!(selected.full_access_grant.is_none());
    assert!(selected.draft_authority_epoch.is_some());
    store
        .conn()
        .execute(
            "UPDATE permission_contract_state SET authority_epoch=?1",
            [ulid::Ulid::generate().to_string()],
        )
        .unwrap();
    assert!(threads::materialize_draft(&store, &selected).is_err());
    assert!(
        service
            .prepare(&selected, PermissionMode::FullAccess)
            .is_err()
    );
}

#[test]
fn default_mode_saves_directly_and_stale_changes_conflict() {
    let fixture = Fixture::new();
    let database = fixture.0.join("db");
    let store = Store::open(&database).unwrap();
    store.migrate().unwrap();
    let path = fixture.0.join("config.toml");
    AppConfig::default().save_to(&path).unwrap();
    let service = DefaultPermissionService::new(database, path.clone());
    let initial = service.read().unwrap();
    let change = service
        .prepare(&initial, PermissionMode::FullAccess)
        .unwrap();
    let values = service.save(&change).unwrap();
    assert_eq!(values.permission_mode, "full_access");
    assert!(values.full_access_grant.is_none());
    assert!(service.save(&change).is_err());
    assert_eq!(
        vega_store::config::read_from(&path)
            .unwrap()
            .defaults
            .permission_mode,
        "full_access"
    );
}

#[test]
fn execution_fence_rejects_old_policy_without_requiring_grant() {
    let fixture = Fixture::new();
    let mut fence = PermissionFence {
        schema_version: 1,
        execution_policy_version: 2,
        authority_epoch: ulid::Ulid::generate().to_string(),
        database_identity: "database".into(),
        thread_id: ulid::Ulid::generate().to_string(),
        project_id: String::new(),
        workspace: fixture.0.to_str().unwrap().into(),
        run_mode: "execute".into(),
        permission_mode: "full_access".into(),
        permission_revision: 0,
        exact_rules: Vec::new(),
        full_access_grant: None,
        delegation: None,
    };
    assert!(fence.is_valid());
    fence.execution_policy_version = 1;
    assert!(!fence.is_valid());
    fence.execution_policy_version = 2;
    fence.permission_mode = "unknown".into();
    assert!(!fence.is_valid());
}
