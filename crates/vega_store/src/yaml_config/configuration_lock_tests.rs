use super::*;

#[test]
fn syscall_lock_errors_retain_typed_stage_and_kind_and_are_not_retryable_busy() {
    for code in [
        libc::EIO,
        libc::EBADF,
        libc::ENOTSUP,
        libc::EACCES,
        libc::EINTR,
        libc::EAGAIN,
    ] {
        let source = io::Error::from_raw_os_error(code);
        let kind = source.kind();
        let error = lock_error(std::fs::TryLockError::Error(source));
        assert_eq!(
            failure(&error),
            Some(ConfigurationFailure {
                stage: ConfigurationFailureStage::LockAcquire,
                kind,
            })
        );
        assert_eq!(error.kind(), kind);
        let retained = error
            .get_ref()
            .unwrap()
            .downcast_ref::<ConfigurationLockError>()
            .unwrap();
        assert_eq!(retained.0.raw_os_error(), Some(code));
        assert!(lock_busy(&error).is_none());
    }
    let busy = lock_error(std::fs::TryLockError::WouldBlock);
    assert_eq!(lock_busy(&busy), Some(ConfigurationLockBusy::File));
    assert!(failure(&busy).is_none());
}

#[test]
fn edit_mutex_poison_remains_fail_closed_without_a_busy_marker() {
    let panicked = std::thread::spawn(|| {
        let _guard = EDIT_LOCK.lock().unwrap();
        panic!("synthetic poison");
    });
    assert!(panicked.join().is_err());
    for _ in 0..2 {
        let error = try_begin(Path::new("/synthetic/config.yaml"))
            .err()
            .unwrap();
        assert_eq!(
            failure(&error),
            Some(ConfigurationFailure {
                stage: ConfigurationFailureStage::Poisoned,
                kind: io::ErrorKind::Other,
            })
        );
        assert!(lock_busy(&error).is_none());
        assert!(edit_lock_poisoned());
    }
}

#[test]
fn edit_mutex_contention_is_busy_only_until_the_guard_is_released() {
    let root = std::env::temp_dir()
        .canonicalize()
        .unwrap()
        .join(format!("configuration-edit-{}", ulid::Ulid::generate()));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("config.yaml");
    let guard = EDIT_LOCK.lock().unwrap();
    let error = try_begin(&path).err().unwrap();
    assert_eq!(lock_busy(&error), Some(ConfigurationLockBusy::Edit));
    assert!(failure(&error).is_none());
    drop(guard);
    drop(try_begin(&path).unwrap());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn configuration_diagnostics_upgrade_preserves_prior_reasons_and_rejects_unknown_values() {
    let root = std::env::temp_dir()
        .canonicalize()
        .unwrap()
        .join(format!("configuration-upgrade-{}", ulid::Ulid::generate()));
    std::fs::create_dir(&root).unwrap();
    let store = crate::Store::open(root.join("synthetic.db")).unwrap();
    for migration in &crate::MIGRATIONS[..59] {
        store.conn().execute_batch(migration).unwrap();
    }
    store
        .conn()
        .pragma_update(None, "user_version", 59)
        .unwrap();
    store.conn().execute_batch("INSERT INTO projects(id,path,name,created_at,last_opened_at) VALUES ('p','/synthetic','Synthetic',1,1); INSERT INTO threads(id,project_id,title,model,created_at,updated_at) VALUES ('t','p','Synthetic','synthetic',1,1)").unwrap();
    let reasons = [
        "configuration",
        "configuration_poisoned",
        "configuration_directory",
        "configuration_lock_open",
        "configuration_lock_validation",
        "configuration_read",
        "configuration_parse",
    ];
    for (index, reason) in reasons.iter().enumerate() {
        store.conn().execute("INSERT INTO run_diagnostic_events(thread_id,run_id,attempt_id,phase,state,failure_code,occurred_at,duration_ms,failure_kind,credential_stage,credential_reason) VALUES ('t','r',?1,'run','failed','unknown_safe_failure',1,1,'credential_check','mcp',?2)", rusqlite::params![format!("a{index}"), reason]).unwrap();
    }
    store.migrate().unwrap();
    store.migrate().unwrap();
    let actual: Vec<String> = store
        .conn()
        .prepare("SELECT credential_reason FROM run_diagnostic_events ORDER BY id")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(actual, reasons);
    store.conn().execute("UPDATE run_diagnostic_events SET credential_reason='configuration_lock_acquire', configuration_lock=NULL, configuration_lock_attempts=NULL, configuration_lock_wait_ms=NULL WHERE id=1", []).unwrap();
    assert!(
        store
            .conn()
            .execute(
                "UPDATE run_diagnostic_events SET credential_reason='unknown' WHERE id=1",
                []
            )
            .is_err()
    );
    assert!(
        store
            .conn()
            .execute(
                "UPDATE run_diagnostic_events SET configuration_lock_attempts=7 WHERE id=1",
                []
            )
            .is_err()
    );
    assert!(
        store
            .conn()
            .execute(
                "UPDATE run_diagnostic_events SET configuration_lock_wait_ms=501 WHERE id=1",
                []
            )
            .is_err()
    );
    let integrity: String = store
        .conn()
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
