use super::*;
type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Fixture {
    root: std::path::PathBuf,
    store: Store,
    config: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!("vega-intent-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root)?;
        let store = Store::open(root.join("database.db"))?;
        store.migrate()?;
        let config = root.join("config.yaml");
        vega_store::config::update_from(&config, |_| {})?;
        Ok(Self {
            root,
            store,
            config,
        })
    }
    fn draft(&self) -> Result<Thread, Box<dyn std::error::Error>> {
        let config = vega_store::config::read_from(&self.config)?;
        Ok(crate::threads::draft_thread_from_defaults(
            None,
            "model",
            default_permission_snapshot(Some(&self.config), &config.defaults),
        ))
    }
    fn change(&self) -> DefaultPermissionService {
        DefaultPermissionService::new(self.root.join("database.db"), self.config.clone())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn changed_default_generation_rejects_prepared_draft_without_materializing_it() -> TestResult {
    let fixture = Fixture::new()?;
    let draft = fixture.draft()?;
    let creation = DraftCreationGuard::begin(&fixture.store, &draft)?;
    let service = fixture.change();
    let expected = service.read().map_err(|_| "default read")?;
    let change = service
        .prepare(&expected, PermissionMode::FullAccess)
        .map_err(|_| "default prepare")?;
    service.save(&change).map_err(|_| "default save")?;
    let _coordination = coordinate()?;
    let tx = fixture.store.immediate_transaction()?;
    assert!(creation.materialize(&tx).is_err());
    drop(tx);
    assert!(vega_store::threads::find(fixture.store.conn(), &draft.id)?.is_none());
    Ok(())
}

#[test]
fn fresh_draft_materializes_via_writer_and_clears_default_source() -> TestResult {
    let fixture = Fixture::new()?;
    let draft = fixture.draft()?;
    let materialized = crate::threads::materialize_draft(&fixture.store, &draft)?;
    assert!(materialized.default_permission.is_none());
    assert!(vega_store::threads::find(fixture.store.conn(), &draft.id)?.is_some());
    let state: String = fixture.store.conn().query_row(
        "SELECT state FROM draft_creation_intents WHERE thread_id=?1",
        [&draft.id],
        |row| row.get(0),
    )?;
    assert_eq!(state, "committed");
    Ok(())
}

#[test]
fn unconfirmed_default_change_reconciles_actual_file_without_replaying_save() -> TestResult {
    let fixture = Fixture::new()?;
    let config = vega_store::config::read_from(&fixture.config)?;
    let source = default_permission_snapshot(Some(&fixture.config), &config.defaults);
    let mut target = source.clone();
    target.permission_mode = "full_access".into();
    target.permission_revision += 1;
    let source = source_key(&source)?;
    let target_key = source_key(&target)?;
    let epoch = vega_store::permission_contracts::load_state(fixture.store.conn())?.authority_epoch;
    let id = default_commit(
        &fixture.store,
        DefaultIntent::Begin {
            operation: ulid::Ulid::generate().to_string(),
            source: source.clone(),
            target: target_key.clone(),
            epoch: epoch.clone(),
        },
    )
    .map_err(|_| "begin")?;
    assert!(!id.is_empty());
    assert!(DraftCreationGuard::begin(&fixture.store, &fixture.draft()?).is_err());
    fixture
        .change()
        .read()
        .map_err(|_| "reconcile unchanged source")?;
    let pending: Option<String> = fixture.store.conn().query_row(
        "SELECT pending_id FROM default_permission_intent",
        [],
        |row| row.get(0),
    )?;
    assert!(pending.is_none());
    default_commit(
        &fixture.store,
        DefaultIntent::Begin {
            operation: ulid::Ulid::generate().to_string(),
            source,
            target: target_key,
            epoch,
        },
    )
    .map_err(|_| "second begin")?;
    vega_store::config::update_from(&fixture.config, |config| {
        config.defaults.permission_mode = target.permission_mode.clone();
        config.defaults.permission_revision = target.permission_revision;
    })?;
    let saved = fixture
        .change()
        .read()
        .map_err(|_| "reconcile committed target")?;
    assert_eq!(saved.permission_revision, target.permission_revision);
    assert_eq!(saved.permission_mode, target.permission_mode);
    assert!(fixture.change().read().is_ok());
    Ok(())
}

struct PausedDraftInstall {
    creation: DraftCreationGuard,
    entered: std::sync::mpsc::SyncSender<()>,
    release: std::sync::mpsc::Receiver<()>,
}
impl crate::database_writer::Sealed for PausedDraftInstall {}
impl crate::database_writer::DatabaseCommand for PausedDraftInstall {
    type Output = Result<Thread, ConversationError>;
    fn apply(self, store: &Store) -> Self::Output {
        self.entered
            .send(())
            .map_err(|_| ConversationError::InvalidTaskIdentity)?;
        self.release
            .recv()
            .map_err(|_| ConversationError::InvalidTaskIdentity)?;
        let _coordination = coordinate()?;
        let transaction = store.immediate_transaction().map_err(storage_error)?;
        let thread = self.creation.materialize(&transaction)?;
        transaction.commit().map_err(storage_error)?;
        Ok(thread)
    }
}

#[test]
fn external_configuration_changes_reject_prepared_drafts_in_actor_without_partial_install()
-> TestResult {
    for change in ["in_place", "replace", "remove"] {
        let fixture = Fixture::new()?;
        vega_store::config::update_from(&fixture.config, |config| {
            config.defaults.permission_mode = "full_access".into();
        })?;
        let draft = fixture.draft()?;
        assert_eq!(draft.permission_mode, PermissionMode::FullAccess);
        let creation = DraftCreationGuard::begin(&fixture.store, &draft)?;
        let generation: String = fixture.store.conn().query_row(
            "SELECT generation FROM default_permission_intent",
            [],
            |row| row.get(0),
        )?;
        let (entered, started) = std::sync::mpsc::sync_channel(1);
        let (release, resume) = std::sync::mpsc::sync_channel(1);
        let path = fixture.root.join("database.db");
        let identity = fixture.store.database_identity();
        let worker = std::thread::spawn(move || {
            crate::database_writer::commit(
                &path,
                identity,
                PausedDraftInstall {
                    creation,
                    entered,
                    release: resume,
                },
            )
        });
        started.recv_timeout(std::time::Duration::from_secs(5))?;
        let bytes = std::fs::read_to_string(&fixture.config)?;
        assert!(bytes.contains("full_access"));
        let changed = bytes.replace("full_access", "confirm");
        match change {
            "in_place" => std::fs::write(&fixture.config, changed)?,
            "replace" => {
                let replacement = fixture.root.join("replacement.yaml");
                std::fs::write(&replacement, changed)?;
                std::fs::rename(replacement, &fixture.config)?;
            }
            "remove" => std::fs::remove_file(&fixture.config)?,
            _ => unreachable!(),
        }
        release.send(())?;
        assert!(
            worker
                .join()
                .map_err(|_| "draft worker panicked")??
                .is_err()
        );
        assert!(vega_store::threads::find(fixture.store.conn(), &draft.id)?.is_none());
        let state: String = fixture.store.conn().query_row(
            "SELECT state FROM draft_creation_intents WHERE thread_id=?1",
            [&draft.id],
            |row| row.get(0),
        )?;
        assert_eq!(state, "prepared");
        let current: String = fixture.store.conn().query_row(
            "SELECT generation FROM default_permission_intent",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(current, generation);
    }
    Ok(())
}
