use std::path::PathBuf;

use vega_conversation::automation;
use vega_conversation::permission_reset::recover_pending;
use vega_conversation::types::{
    AutomationCommand, AutomationControl, AutomationKind, AutomationPermissionAuthorization,
    AutomationReason, AutomationStatus, PermissionFence, PermissionResetError,
};
use vega_store::Store;
use vega_store::config::{self, AppConfig};
use vega_store::permission_reset::{self as reset, ResetConfigSource, ResetRecord};

struct Fixture {
    root: PathBuf,
    config: PathBuf,
    store: Store,
}

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("vega-reset-recovery-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root).expect("isolated directory");
        let config = root.join("config.toml");
        let mut value = AppConfig::default();
        value.defaults.permission_mode = "full_access".into();
        value.defaults.permission_revision = 7;
        value.defaults.full_access_grant = Some("synthetic authority metadata".into());
        value.ui.language = "zh-CN".into();
        value.save_to(&config).expect("isolated configuration");
        let store = Store::open(":memory:").expect("isolated database");
        store.migrate().expect("migrate fixture");
        store.conn().execute("INSERT INTO threads(id,model,created_at,updated_at) VALUES('thread','synthetic',1,1)", []).expect("fixture thread");
        Self {
            root,
            config,
            store,
        }
    }

    fn begin(&self) -> ResetRecord {
        let config = config::read_from(&self.config).expect("read original config");
        let source = ResetConfigSource {
            configuration_id: self
                .config
                .canonicalize()
                .expect("canonical path")
                .to_str()
                .expect("utf8")
                .into(),
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
        tx.commit().expect("commit journal");
        record
    }

    fn pending(&self, record: &ResetRecord) {
        assert_eq!(
            reset::load(self.store.conn())
                .expect("pending journal")
                .as_ref(),
            Some(record)
        );
        assert!(
            vega_store::permission_contracts::load_state(self.store.conn())
                .expect("pending state")
                .reset_pending
        );
    }

    fn task(&self, kind: AutomationKind, paused: bool) {
        automation::command(
            &self.store,
            "thread",
            &AutomationCommand::Create {
                kind,
                instruction: "retained automation instruction".into(),
                token_budget: Some(100),
                interval_seconds: (kind == AutomationKind::Loop).then_some(60),
            },
            2,
        )
        .expect("create typed task");
        if paused {
            automation::command(
                &self.store,
                "thread",
                &AutomationCommand::Control(kind, AutomationControl::Pause),
                3,
            )
            .expect("pause task");
        }
        let mut task = automation::load(&self.store, "thread")
            .expect("load tasks")
            .into_iter()
            .find(|task| task.kind == kind)
            .expect("created task");
        task.reason = Some("retained progress".into());
        task.permission_authorization = Some(AutomationPermissionAuthorization {
            kind,
            identity: task.identity.clone(),
            revision: task.revision,
            fence: PermissionFence {
                schema_version: 1,
                execution_policy_version: 1,
                authority_epoch: vega_store::permission_contracts::load_state(self.store.conn())
                    .expect("epoch")
                    .authority_epoch,
                database_identity: self.store.database_identity().permission_fence_key(),
                thread_id: "thread".into(),
                project_id: String::new(),
                workspace: self.root.to_string_lossy().into_owned(),
                run_mode: "execute".into(),
                permission_mode: "confirm".into(),
                permission_revision: 0,
                exact_rules: Vec::new(),
                full_access_grant: None,
                delegation: None,
            },
        });
        vega_store::automation::save_task(
            self.store.conn(),
            "thread",
            kind.key(),
            &task.identity,
            task.revision,
            &serde_json::to_string(&task).expect("encode fixture task"),
        )
        .expect("store authorized task fixture");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn recovery_accepts_source_or_target_without_repeating_epoch_or_task_revision() {
    for already_saved in [false, true] {
        let fixture = Fixture::new();
        fixture.task(AutomationKind::Goal, false);
        fixture.task(AutomationKind::Loop, true);
        let before = automation::load(&fixture.store, "thread").expect("before reset tasks");
        let record = fixture.begin();
        config::update_from(&fixture.config, |config| {
            config.ui.theme = "light".into();
            if already_saved {
                config.defaults.permission_mode = "confirm".into();
                config.defaults.permission_revision = record.target_revision;
                config.defaults.full_access_grant = None;
            }
        })
        .expect("unrelated preference and optional saved target");
        let expected = config::read_from(&fixture.config).expect("latest unrelated preferences");
        let receipt = recover_pending(&fixture.store, &fixture.config)
            .expect("recover pending reset")
            .expect("completion receipt");
        assert_eq!(receipt.reset_id, record.id);
        assert_eq!(receipt.authority_epoch, record.authority_epoch);
        assert_eq!(receipt.default_permission_revision, record.target_revision);
        let saved = config::read_from(&fixture.config).expect("read reset config");
        assert_eq!(saved.defaults.permission_mode, "confirm");
        assert_eq!(saved.defaults.permission_revision, 8);
        assert!(saved.defaults.full_access_grant.is_none());
        assert_eq!(saved.ui, expected.ui);
        let tasks = automation::load(&fixture.store, "thread").expect("revoked tasks");
        for task in &tasks {
            let prior = before
                .iter()
                .find(|prior| prior.kind == task.kind)
                .expect("prior task");
            assert_eq!(task.status, AutomationStatus::Paused);
            assert_eq!(task.revision, prior.revision + 1);
            assert!(task.permission_authorization.is_none());
            assert_eq!(task.instruction, prior.instruction);
            assert_eq!(task.reason, prior.reason);
            if prior.status == AutomationStatus::Active {
                assert_eq!(task.system_reason, Some(AutomationReason::PermissionReset));
            }
        }
        let completed = reset::load(fixture.store.conn())
            .expect("completed journal")
            .expect("receipt retained");
        assert!(completed.completed);
        assert_eq!(recover_pending(&fixture.store, &fixture.config), Ok(None));
        assert_eq!(
            automation::load(&fixture.store, "thread").expect("idempotent tasks"),
            tasks
        );
        assert_eq!(
            reset::load(fixture.store.conn()).expect("unchanged completed journal"),
            Some(completed)
        );
    }
}

#[test]
fn recovery_rejects_missing_malformed_and_changed_config_without_overwriting_it() {
    for fault in ["missing", "malformed", "changed"] {
        let fixture = Fixture::new();
        let record = fixture.begin();
        match fault {
            "missing" => std::fs::remove_file(&fixture.config).expect("remove isolated config"),
            "malformed" => {
                std::fs::write(&fixture.config, "[broken").expect("malformed isolated config")
            }
            _ => {
                config::update_from(&fixture.config, |config| {
                    config.defaults.permission_revision += 2
                })
                .expect("competing permissions");
            }
        }
        let before = std::fs::read(&fixture.config).ok();
        let expected = if fault == "changed" {
            PermissionResetError::ConfigurationConflict
        } else {
            PermissionResetError::ConfigurationUnavailable
        };
        assert_eq!(
            recover_pending(&fixture.store, &fixture.config),
            Err(expected)
        );
        assert_eq!(std::fs::read(&fixture.config).ok(), before);
        fixture.pending(&record);
    }
}

#[test]
fn active_owners_and_unknown_effects_block_recovery_without_normalizing_history() {
    for fault in ["owner", "streaming", "tool", "automation"] {
        let fixture = Fixture::new();
        let record = fixture.begin();
        let mut owner = None;
        match fault {
            "owner" => {
                owner = vega_store::thread_admission::try_acquire(
                    fixture.store.database_identity(),
                    "thread",
                )
                .expect("existing owner")
            }
            "streaming" => {
                fixture.store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES('message','thread',0,'assistant','','streaming',1)", []).expect("streaming evidence");
            }
            "tool" => {
                fixture.store.conn().execute("INSERT INTO tool_calls(id,thread_id,message_id,seq,tool,input_json,status,created_at) VALUES('call','thread','message',0,'bash','{}','cancelled',1)", []).expect("unknown effect evidence");
            }
            _ => {
                fixture.store.conn().execute("INSERT INTO automation_runs(id,thread_id,data) VALUES('run','thread','{}')", []).expect("unsettled automation evidence");
            }
        }
        let before = std::fs::read(&fixture.config).expect("unchanged config bytes");
        let expected = if owner.is_some() {
            PermissionResetError::Busy
        } else {
            PermissionResetError::UnsettledWork
        };
        assert_eq!(
            recover_pending(&fixture.store, &fixture.config),
            Err(expected)
        );
        assert_eq!(
            std::fs::read(&fixture.config).expect("config retained"),
            before
        );
        fixture.pending(&record);
        if fault == "automation" {
            assert!(automation::recover_startup(&fixture.store).is_err());
            let finished: bool = fixture
                .store
                .conn()
                .query_row(
                    "SELECT finished FROM automation_runs WHERE id='run'",
                    [],
                    |row| row.get(0),
                )
                .expect("claim preserved");
            assert!(!finished);
        }
    }
}

#[test]
fn inconsistent_or_overflowing_automation_rolls_back_recovery_and_keeps_config_source() {
    for overflow in [false, true] {
        let fixture = Fixture::new();
        fixture.task(AutomationKind::Goal, false);
        fixture.task(AutomationKind::Loop, false);
        if overflow {
            fixture.store.conn().execute("UPDATE automation_tasks SET revision=?1,data=json_set(data,'$.revision',?1) WHERE kind='loop'", [i64::MAX]).expect("overflow fixture");
        } else {
            fixture
                .store
                .conn()
                .execute(
                    "UPDATE automation_tasks SET revision=revision+1 WHERE kind='loop'",
                    [],
                )
                .expect("mismatched indexed revision");
        }
        let before =
            vega_store::automation::all_tasks(fixture.store.conn()).expect("before task blobs");
        let record = fixture.begin();
        let config = std::fs::read(&fixture.config).expect("before config");
        assert_eq!(
            recover_pending(&fixture.store, &fixture.config),
            Err(PermissionResetError::Unavailable)
        );
        assert_eq!(
            vega_store::automation::all_tasks(fixture.store.conn()).expect("rollback task blobs"),
            before
        );
        assert_eq!(
            std::fs::read(&fixture.config).expect("config source retained"),
            config
        );
        fixture.pending(&record);
    }
}

#[test]
fn pending_reset_blocks_automation_commands_and_startup_before_normalization() {
    let fixture = Fixture::new();
    fixture.task(AutomationKind::Goal, false);
    let record = fixture.begin();
    let before = automation::load(&fixture.store, "thread").expect("pending task");
    assert!(
        automation::command(
            &fixture.store,
            "thread",
            &AutomationCommand::Control(AutomationKind::Goal, AutomationControl::Pause),
            20
        )
        .is_err()
    );
    assert!(automation::recover_startup(&fixture.store).is_err());
    assert_eq!(
        automation::load(&fixture.store, "thread").expect("unchanged tasks"),
        before
    );
    fixture.pending(&record);
}

#[test]
fn ready_database_does_not_create_or_touch_a_missing_configuration() {
    let fixture = Fixture::new();
    std::fs::remove_file(&fixture.config).expect("remove fixture config");
    assert_eq!(recover_pending(&fixture.store, &fixture.config), Ok(None));
    assert!(!fixture.config.exists());
    assert!(
        reset::load(fixture.store.conn())
            .expect("no reset journal")
            .is_none()
    );
}
