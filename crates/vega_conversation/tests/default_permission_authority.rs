use std::path::PathBuf;

use vega_conversation::permission_contracts::{
    DefaultPermissionService, default_permission_snapshot,
};
use vega_conversation::threads::{
    draft_thread, draft_thread_from_defaults, get_thread, materialize_draft,
};
use vega_conversation::types::{
    DefaultPermissionChange, DefaultPermissionError, DefaultPermissionValues, FullAccessGrant,
    FullAccessScope, PermissionMode,
};
use vega_store::Store;
use vega_store::config::{self, AppConfig};

struct Fixture {
    root: PathBuf,
    database: PathBuf,
    config: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "vega-default-permission-{}",
            ulid::Ulid::generate()
        ));
        std::fs::create_dir(&root).expect("create isolated fixture");
        let fixture = Self {
            database: root.join("state.sqlite"),
            config: root.join("config.toml"),
            root,
        };
        let store = Store::open(&fixture.database).expect("open fixture database");
        store.migrate().expect("migrate fixture database");
        AppConfig::default()
            .save_to(&fixture.config)
            .expect("create fixture config");
        fixture
    }

    fn service(&self) -> DefaultPermissionService {
        DefaultPermissionService::new(self.database.clone(), self.config.clone())
    }

    fn values(&self) -> DefaultPermissionValues {
        let config = config::read_from(&self.config).expect("read fixture config");
        DefaultPermissionValues::from(&config.defaults)
    }

    fn prepare(&self, mode: PermissionMode) -> DefaultPermissionChange {
        self.service()
            .prepare(&self.values(), mode)
            .expect("prepare change")
    }

    fn authorize(&self) -> DefaultPermissionValues {
        self.service()
            .save(&self.prepare(PermissionMode::FullAccess), true)
            .expect("confirm full access")
    }

    fn store(&self) -> Store {
        Store::open(&self.database).expect("reopen fixture database")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn grant(values: &DefaultPermissionValues) -> FullAccessGrant {
    serde_json::from_str(
        values
            .full_access_grant
            .as_deref()
            .expect("saved grant exists"),
    )
    .expect("decode saved grant")
}

#[test]
fn preparing_and_declining_full_access_never_changes_authority() {
    let fixture = Fixture::new();
    let before = fixture.values();
    let prepared = fixture.prepare(PermissionMode::FullAccess);
    assert_eq!(fixture.values(), before);
    assert_eq!(
        fixture.service().save(&prepared, false),
        Err(DefaultPermissionError::ConfirmationRequired)
    );
    assert_eq!(fixture.values(), before);
    drop(prepared);
    assert_eq!(fixture.values(), before);
}

#[test]
fn explicit_reauthorization_rotates_grant_and_survives_service_reopen() {
    let fixture = Fixture::new();
    let initial = fixture.values();
    let saved = fixture.authorize();
    assert_eq!(saved.permission_mode, "full_access");
    assert_eq!(saved.permission_revision, initial.permission_revision + 1);
    let state = vega_store::permission_contracts::load_state(fixture.store().conn())
        .expect("read authority state");
    assert!(
        grant(&saved).verifies(
            &state.authority_epoch,
            &FullAccessScope::FutureThreads {
                configuration_id: fixture
                    .config
                    .canonicalize()
                    .expect("canonical fixture source")
                    .to_str()
                    .expect("utf8 fixture source")
                    .to_owned(),
            }
        )
    );
    assert_eq!(fixture.service().read(), Ok(saved.clone()));
    let renewed = fixture.authorize();
    assert_eq!(renewed.permission_revision, saved.permission_revision + 1);
    assert_ne!(grant(&renewed).identity(), grant(&saved).identity());
    assert_eq!(fixture.service().read(), Ok(renewed));
}

#[test]
fn restricted_selection_revokes_default_and_old_confirmation_cannot_restore_it() {
    let fixture = Fixture::new();
    fixture.authorize();
    let stale = fixture.prepare(PermissionMode::FullAccess);
    let restricted = fixture
        .service()
        .save(&fixture.prepare(PermissionMode::Confirm), false)
        .expect("save restricted default");
    assert_eq!(restricted.permission_mode, "confirm");
    assert!(restricted.full_access_grant.is_none());
    assert_eq!(
        fixture.service().save(&stale, true),
        Err(DefaultPermissionError::Conflict)
    );
    assert_eq!(fixture.values(), restricted);
}

#[test]
fn mode_and_grant_changes_conflict_even_without_revision_change() {
    let fixture = Fixture::new();
    let stale_display = fixture.values();
    let prepared = fixture.prepare(PermissionMode::FullAccess);
    config::update_from(&fixture.config, |config| {
        config.defaults.permission_mode = "auto".into();
    })
    .expect("simulate external mode edit");
    assert_eq!(
        fixture
            .service()
            .prepare(&stale_display, PermissionMode::FullAccess),
        Err(DefaultPermissionError::Conflict)
    );
    assert_eq!(
        fixture.service().save(&prepared, true),
        Err(DefaultPermissionError::Conflict)
    );
    let prepared = fixture.prepare(PermissionMode::FullAccess);
    config::update_from(&fixture.config, |config| {
        config.defaults.full_access_grant = Some("unrecognized-grant".into());
    })
    .expect("simulate external grant edit");
    let changed = fixture.values();
    assert_eq!(
        fixture.service().save(&prepared, true),
        Err(DefaultPermissionError::Conflict)
    );
    assert_eq!(fixture.values(), changed);
}

#[test]
fn epoch_rotation_and_pending_reset_reject_prior_confirmation() {
    let fixture = Fixture::new();
    let before = fixture.values();
    let prepared = fixture.prepare(PermissionMode::FullAccess);
    fixture
        .store()
        .conn()
        .execute(
            "UPDATE permission_contract_state SET authority_epoch=?1 WHERE singleton=1",
            [ulid::Ulid::generate().to_string()],
        )
        .expect("rotate fixture epoch");
    assert_eq!(
        fixture.service().save(&prepared, true),
        Err(DefaultPermissionError::Conflict)
    );
    let prepared = fixture.prepare(PermissionMode::FullAccess);
    fixture
        .store()
        .conn()
        .execute(
            "UPDATE permission_contract_state SET reset_state='pending' WHERE singleton=1",
            [],
        )
        .expect("mark fixture reset pending");
    assert_eq!(
        fixture.service().save(&prepared, true),
        Err(DefaultPermissionError::ResetPending)
    );
    assert_eq!(
        fixture.service().prepare(&before, PermissionMode::Confirm),
        Err(DefaultPermissionError::ResetPending)
    );
    assert_eq!(fixture.values(), before);
}

#[test]
fn confirmation_cannot_move_to_another_config_or_database() {
    let fixture = Fixture::new();
    let alternate = Fixture::new();
    let prepared = fixture.prepare(PermissionMode::FullAccess);
    let epoch = vega_store::permission_contracts::load_state(fixture.store().conn())
        .expect("read fixture epoch")
        .authority_epoch;
    alternate
        .store()
        .conn()
        .execute(
            "UPDATE permission_contract_state SET authority_epoch=?1 WHERE singleton=1",
            [epoch],
        )
        .expect("match epoch while keeping database identity distinct");
    let other_database =
        DefaultPermissionService::new(alternate.database.clone(), fixture.config.clone());
    assert_eq!(
        other_database.save(&prepared, true),
        Err(DefaultPermissionError::Conflict)
    );
    let other_config =
        DefaultPermissionService::new(fixture.database.clone(), alternate.config.clone());
    assert_eq!(
        other_config.save(&prepared, true),
        Err(DefaultPermissionError::Conflict)
    );
    assert_eq!(fixture.values().permission_revision, 0);
    assert_eq!(alternate.values().permission_revision, 0);
}

#[test]
fn permission_commit_preserves_concurrent_unrelated_preferences() {
    let fixture = Fixture::new();
    let prepared = fixture.prepare(PermissionMode::FullAccess);
    config::update_from(&fixture.config, |config| {
        config.defaults.model = "user-selected-model".into();
        config.ui.language = "en".into();
    })
    .expect("save unrelated preferences during confirmation");
    fixture
        .service()
        .save(&prepared, true)
        .expect("save permission without overwriting unrelated fields");
    let saved = config::read_from(&fixture.config).expect("read committed config");
    assert_eq!(saved.defaults.model, "user-selected-model");
    assert_eq!(saved.ui.language, "en");
    assert_eq!(saved.defaults.permission_mode, "full_access");
}

#[test]
fn failed_config_write_does_not_publish_new_authority() {
    let fixture = Fixture::new();
    let before = fixture.values();
    let prepared = fixture.prepare(PermissionMode::FullAccess);
    std::fs::create_dir(fixture.config.with_extension("toml.tmp"))
        .expect("block only the fixture temporary output");
    assert_eq!(
        fixture.service().save(&prepared, true),
        Err(DefaultPermissionError::Unavailable)
    );
    assert_eq!(fixture.values(), before);
}

#[test]
fn missing_sources_are_not_created_by_permission_actions() {
    let fixture = Fixture::new();
    let missing_config = fixture.root.join("missing.toml");
    let service = DefaultPermissionService::new(fixture.database.clone(), missing_config.clone());
    assert_eq!(service.read(), Err(DefaultPermissionError::Unavailable));
    assert!(!missing_config.exists());
    let missing_database = fixture.root.join("missing.sqlite");
    let service = DefaultPermissionService::new(missing_database.clone(), fixture.config.clone());
    assert_eq!(service.read(), Err(DefaultPermissionError::Unavailable));
    assert!(!missing_database.exists());
    fixture
        .store()
        .conn()
        .execute("DELETE FROM permission_contract_state", [])
        .expect("remove fixture authority state");
    assert_eq!(
        fixture.service().read(),
        Err(DefaultPermissionError::Unavailable)
    );
    let count: i64 = fixture
        .store()
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM permission_contract_state",
            [],
            |row| row.get(0),
        )
        .expect("count fixture authority rows");
    assert_eq!(count, 0);
}

#[test]
fn materialization_derives_distinct_thread_grants_and_revocation_only_affects_future_threads() {
    let fixture = Fixture::new();
    let saved = fixture.authorize();
    let config = config::read_from(&fixture.config).expect("read default source");
    let snapshot = default_permission_snapshot(Some(&fixture.config), &config.defaults);
    let first_draft = draft_thread_from_defaults(None, "model", snapshot.clone());
    let second_draft = draft_thread_from_defaults(None, "model", snapshot.clone());
    let stale_draft = draft_thread_from_defaults(None, "model", snapshot);
    let store = fixture.store();
    let first = materialize_draft(&store, &first_draft).expect("materialize first thread");
    let second = materialize_draft(&store, &second_draft).expect("materialize second thread");
    let first_grant = first
        .full_access_grant
        .as_ref()
        .expect("derived first grant");
    let second_grant = second
        .full_access_grant
        .as_ref()
        .expect("derived second grant");
    assert_ne!(first_grant.identity(), second_grant.identity());
    assert_ne!(first_grant.identity(), grant(&saved).identity());
    let epoch = vega_store::permission_contracts::load_state(store.conn())
        .expect("read authority")
        .authority_epoch;
    assert!(first_grant.verifies(
        &epoch,
        &FullAccessScope::Thread {
            thread_id: first.id.clone()
        }
    ));
    assert!(!first_grant.verifies(
        &epoch,
        &FullAccessScope::Thread {
            thread_id: second.id.clone()
        }
    ));
    assert!(first.default_permission.is_none());
    assert_eq!(
        get_thread(&store, &first.id).expect("read persisted thread"),
        first
    );
    fixture
        .service()
        .save(&fixture.prepare(PermissionMode::ReadOnly), false)
        .expect("revoke future full access");
    assert!(materialize_draft(&store, &stale_draft).is_err());
    assert!(
        vega_store::threads::find(store.conn(), &stale_draft.id)
            .expect("verify stale draft was not inserted")
            .is_none()
    );
    assert_eq!(
        get_thread(&store, &first.id).expect("read existing thread after revoke"),
        first
    );
    assert!(materialize_draft(&store, &draft_thread(None, "model", "full_access")).is_err());
    let config = config::read_from(&fixture.config).expect("read restricted default");
    let restricted = draft_thread_from_defaults(
        None,
        "model",
        default_permission_snapshot(Some(&fixture.config), &config.defaults),
    );
    let restricted = materialize_draft(&store, &restricted).expect("materialize restricted thread");
    assert_eq!(restricted.permission_mode, PermissionMode::ReadOnly);
    assert!(restricted.full_access_grant.is_none());
}
