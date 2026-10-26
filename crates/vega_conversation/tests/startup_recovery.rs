use std::path::PathBuf;

use vega_conversation::types::{
    AutomationCommand, AutomationKind, AutomationReason, AutomationStatus, PermissionResetError,
    StartupError,
};
use vega_conversation::{automation, startup::prepare_workspace};
use vega_store::{
    Store,
    config::{self, AppConfig},
    permission_reset::{self as reset, ResetConfigSource, ResetRecord},
};

struct Fixture {
    root: PathBuf,
    database: PathBuf,
    config: PathBuf,
    store: Store,
    thread: String,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("vega-startup-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root).expect("isolated directory");
        let root = root.canonicalize().expect("canonical fixture root");
        let database = root.join("vega.db");
        let config = root.join("config.toml");
        let store = Store::open(&database).expect("isolated database");
        store.migrate().expect("migrate fixture");
        let thread = ulid::Ulid::generate().to_string();
        store
            .conn()
            .execute(
                "INSERT INTO threads(id,model,created_at,updated_at) VALUES(?1,'synthetic',1,1)",
                [&thread],
            )
            .expect("fixture thread");
        Self {
            root,
            database,
            config,
            store,
            thread,
        }
    }

    fn save_config(&self) {
        let mut value = AppConfig::default();
        value.defaults.permission_mode = "full_access".into();
        value.defaults.permission_revision = 7;
        value.defaults.full_access_grant = Some("synthetic authority metadata".into());
        value.ui.language = "zh-CN".into();
        value.save_to(&self.config).expect("isolated config");
    }

    fn begin(&self) -> ResetRecord {
        let config = config::read_from(&self.config).expect("source config");
        let source = ResetConfigSource {
            configuration_id: self.config.to_str().expect("utf8 path").into(),
            mode: config.defaults.permission_mode,
            revision: config.defaults.permission_revision,
            grant: config.defaults.full_access_grant,
        };
        let epoch = vega_store::permission_contracts::load_state(self.store.conn())
            .expect("epoch")
            .authority_epoch;
        let tx = self
            .store
            .immediate_transaction()
            .expect("begin reset write");
        let record = reset::begin(&tx, &epoch, &source, 10).expect("pending reset");
        tx.commit().expect("commit reset");
        record
    }

    fn unsettled(&self) {
        self.store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,kind,content,status,created_at) VALUES('stream',?1,1,'assistant','text','retained partial answer','streaming',1)", [&self.thread]).expect("streaming evidence");
        self.store.conn().execute("INSERT INTO tool_calls(id,thread_id,message_id,seq,tool,input_json,status,created_at) VALUES('call',?1,'stream',1,'bash','{}','running',1)", [&self.thread]).expect("tool evidence");
        self.store
            .conn()
            .execute(
                "INSERT INTO automation_runs(id,thread_id,finished,data) VALUES('claim',?1,0,'{}')",
                [&self.thread],
            )
            .expect("unfinished claim");
    }

    fn assert_unsettled(&self, record: &ResetRecord) {
        assert_eq!(
            reset::load(self.store.conn()).expect("journal"),
            Some(record.clone())
        );
        let states: (String, String, bool, String) = self.store.conn().query_row(
            "SELECT m.status,t.status,a.finished,m.content FROM messages m JOIN tool_calls t ON t.message_id=m.id JOIN automation_runs a ON a.thread_id=m.thread_id WHERE m.id='stream'",
            [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)),
        ).expect("retained unsettled work");
        assert_eq!(
            states,
            (
                "streaming".into(),
                "running".into(),
                false,
                "retained partial answer".into()
            )
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn first_start_creates_configuration_only_with_ready_storage() {
    let fixture = Fixture::new();
    assert!(!fixture.config.exists());
    let ready = prepare_workspace(&fixture.database, &fixture.config).expect("ready workspace");
    let (store, value, path, project) = ready.into_parts();
    assert_eq!(path, fixture.config);
    assert!(project.is_none());
    assert_eq!(store.database_identity(), fixture.store.database_identity());
    assert_eq!(value.defaults.permission_mode, "confirm");
    assert!(value.defaults.full_access_grant.is_none());
    assert_eq!(
        config::read_from(&path).expect("created config").defaults,
        value.defaults
    );
    assert!(
        !vega_store::permission_contracts::load_state(store.conn())
            .expect("ready state")
            .reset_pending
    );
    assert!(
        reset::load(store.conn())
            .expect("no implicit reset")
            .is_none()
    );
}

#[test]
fn pending_startup_preserves_missing_malformed_and_conflicting_configuration() {
    for mutation in ["missing", "malformed", "conflict"] {
        let fixture = Fixture::new();
        fixture.save_config();
        fixture.unsettled();
        let record = fixture.begin();
        match mutation {
            "missing" => std::fs::remove_file(&fixture.config).expect("missing fixture config"),
            "malformed" => {
                std::fs::write(&fixture.config, "[broken").expect("malformed fixture config")
            }
            _ => {
                config::update_from(&fixture.config, |config| {
                    config.defaults.permission_revision += 1
                })
                .expect("conflicting config");
            }
        }
        let before = std::fs::read(&fixture.config).ok();
        let result = prepare_workspace(&fixture.database, &fixture.config);
        let expected = if mutation == "conflict" {
            PermissionResetError::ConfigurationConflict
        } else {
            PermissionResetError::ConfigurationUnavailable
        };
        assert!(matches!(result, Err(StartupError::PermissionReset(error)) if error == expected));
        assert_eq!(std::fs::read(&fixture.config).ok(), before);
        fixture.assert_unsettled(&record);
    }
}

#[test]
fn pending_startup_does_not_normalize_unsettled_work_to_make_recovery_succeed() {
    let fixture = Fixture::new();
    fixture.save_config();
    fixture.unsettled();
    let record = fixture.begin();
    let before = std::fs::read(&fixture.config).expect("source bytes");
    assert!(matches!(
        prepare_workspace(&fixture.database, &fixture.config),
        Err(StartupError::PermissionReset(
            PermissionResetError::UnsettledWork
        ))
    ));
    assert_eq!(
        std::fs::read(&fixture.config).expect("unchanged config"),
        before
    );
    fixture.assert_unsettled(&record);
}

#[test]
fn startup_publishes_only_completed_reset_and_retry_keeps_the_same_authority_epoch() {
    let fixture = Fixture::new();
    fixture.save_config();
    automation::command(
        &fixture.store,
        &fixture.thread,
        &AutomationCommand::Create {
            kind: AutomationKind::Goal,
            instruction: "retained automation instruction".into(),
            token_budget: Some(100),
            interval_seconds: None,
        },
        2,
    )
    .expect("active task");
    let before_task = automation::load(&fixture.store, &fixture.thread)
        .expect("task")
        .remove(0);
    let record = fixture.begin();
    let original = std::fs::read(&fixture.config).expect("source configuration");
    std::fs::remove_file(&fixture.config).expect("simulate missing configuration");
    assert!(matches!(
        prepare_workspace(&fixture.database, &fixture.config),
        Err(StartupError::PermissionReset(
            PermissionResetError::ConfigurationUnavailable
        ))
    ));
    assert!(!fixture.config.exists());
    assert_eq!(
        reset::load(fixture.store.conn()).expect("pending"),
        Some(record.clone())
    );
    std::fs::write(&fixture.config, original).expect("restore exact source fixture");
    let ready = prepare_workspace(&fixture.database, &fixture.config).expect("recovered workspace");
    assert_eq!(ready.config().defaults.permission_mode, "confirm");
    assert_eq!(
        ready.config().defaults.permission_revision,
        record.target_revision
    );
    assert!(ready.config().defaults.full_access_grant.is_none());
    assert_eq!(ready.config().ui.language, "zh-CN");
    let (store, _, _, _) = ready.into_parts();
    let completed = reset::load(store.conn())
        .expect("journal")
        .expect("retained journal");
    assert!(completed.completed);
    assert_eq!(completed.id, record.id);
    assert_eq!(completed.authority_epoch, record.authority_epoch);
    assert!(
        !vega_store::permission_contracts::load_state(store.conn())
            .expect("ready state")
            .reset_pending
    );
    let task = automation::load(&store, &fixture.thread)
        .expect("revoked task")
        .remove(0);
    assert_eq!(task.status, AutomationStatus::Paused);
    assert_eq!(task.revision, before_task.revision + 1);
    assert!(task.permission_authorization.is_none());
    assert_eq!(task.system_reason, Some(AutomationReason::PermissionReset));
    assert_eq!(task.instruction, before_task.instruction);
    drop(store);
    let retry = prepare_workspace(&fixture.database, &fixture.config).expect("ready restart");
    let (store, value, _, _) = retry.into_parts();
    assert_eq!(value.defaults.permission_revision, record.target_revision);
    assert_eq!(
        reset::load(store.conn()).expect("same journal"),
        Some(completed)
    );
    assert_eq!(
        automation::load(&store, &fixture.thread)
            .expect("same task")
            .remove(0)
            .revision,
        task.revision
    );
}

#[test]
fn unsupported_database_version_does_not_initialize_configuration() {
    let fixture = Fixture::new();
    fixture
        .store
        .conn()
        .pragma_update(None, "user_version", i32::MAX)
        .expect("future version fixture");
    assert!(matches!(
        prepare_workspace(&fixture.database, &fixture.config),
        Err(StartupError::StorageUnavailable)
    ));
    assert!(!fixture.config.exists());
    let version: i32 = fixture
        .store
        .conn()
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("schema preserved");
    assert_eq!(version, i32::MAX);
}
