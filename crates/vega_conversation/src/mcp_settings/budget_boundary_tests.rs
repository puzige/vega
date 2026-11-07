use super::*;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};

static TEST_LOCK: Mutex<()> = Mutex::new(());

struct Fixture {
    root: PathBuf,
    service: McpServerSettingsService,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("budget-boundary-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root).unwrap();
        let database = root.join("synthetic.db");
        let store = Store::open(&database).unwrap();
        store.migrate().unwrap();
        std::fs::write(root.join("config.yaml"), "schema_version: 1\n").unwrap();
        let service = McpServerSettingsService::new(database, root.clone())
            .with_configuration_cancel(CancellationToken::new());
        service.reconcile_declarations().unwrap();
        Self { root, service }
    }

    fn holder(&self) -> LockHolder {
        let mut child = Command::new("/usr/bin/python3")
            .arg("-c")
            .arg("import fcntl,sys\nf=open(sys.argv[1], 'r+')\nfcntl.flock(f, fcntl.LOCK_EX)\nprint('locked', flush=True)\nsys.stdin.readline()\n")
            .arg(self.root.join(".config.lock"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        assert_eq!(line.trim(), "locked");
        LockHolder(child)
    }

    fn change_configuration(&self) {
        std::fs::write(self.root.join("config.yaml"), "schema_version: 1\nmcp_servers:\n- id: 01ARZ3NDEKTSV4RRFFQ69G5FAV\n  display_name: Synthetic\n  enabled: false\n  transport:\n    type: remote\n    endpoint: https://example.com/mcp\n    allow_loopback_http: false\n    authorization:\n      type: none\n").unwrap();
    }

    fn server_count(&self) -> usize {
        let store = Store::open(&self.service.database_path).unwrap();
        mcp_servers::list(store.conn()).unwrap().len()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

struct LockHolder(Child);

impl Drop for LockHolder {
    fn drop(&mut self) {
        if let Some(mut input) = self.0.stdin.take() {
            input.write_all(b"release\n").unwrap();
        }
        assert!(self.0.wait().unwrap().success());
    }
}

#[test]
fn released_lock_after_backoff_deadline_cannot_be_acquired() {
    let _serial = TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let operation = fixture.service.configuration_operation();
    let holder = fixture.holder();
    let worker = operation.clone();
    let join = std::thread::spawn(move || worker.current_owner_credential_values());
    let shared = operation.configuration_budget.as_ref().unwrap();
    let start = Instant::now();
    let budget = loop {
        if let Ok(budget) = shared.try_lock()
            && budget.attempts > 0
        {
            break budget;
        }
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::sleep(Duration::from_millis(1));
    };
    let attempts = budget.attempts;
    let waited = budget.wait_ms;
    drop(holder);
    fixture.change_configuration();
    std::thread::sleep(Duration::from_millis(510));
    assert!(budget.started.unwrap().elapsed() >= Duration::from_millis(500));
    let expected = budget.last_failure.unwrap();
    assert_eq!(expected.configuration_lock, Some("file"));
    drop(budget);
    assert_eq!(join.join().unwrap().unwrap_err(), expected);
    assert_eq!(fixture.server_count(), 0);
    let budget = shared.lock().unwrap();
    assert_eq!(budget.attempts, attempts);
    assert_eq!(budget.wait_ms, waited);
    assert!(budget.exhausted);
    drop(budget);
    assert!(operation.current_owner_credential_values().is_err());
    assert!(
        fixture
            .service
            .current_owner_credential_values()
            .unwrap()
            .is_empty()
    );
    assert_eq!(fixture.server_count(), 1);
}

#[test]
fn exhausted_shared_budget_stays_terminal_after_lock_release() {
    let _serial = TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let fresh = fixture.service.configuration_operation();
    assert!(fresh.current_owner_credential_values().unwrap().is_empty());
    let budget = fresh.configuration_budget.as_ref().unwrap().lock().unwrap();
    assert_eq!(budget.attempts, 0);
    assert_eq!(budget.wait_ms, 0);
    assert!(budget.started.is_none());
    drop(budget);
    let operation = fixture.service.configuration_operation();
    let holder = fixture.holder();
    let expected = operation.current_owner_credential_values().unwrap_err();
    assert_eq!(expected.configuration_lock, Some("file"));
    assert!((1..=6).contains(&expected.configuration_lock_attempts.unwrap()));
    assert!(expected.configuration_lock_wait_ms.unwrap() <= 310);
    assert!(
        operation
            .configuration_budget
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .exhausted
    );
    drop(holder);
    fixture.change_configuration();
    for _ in 0..2 {
        assert_eq!(
            operation.current_owner_credential_values().unwrap_err(),
            expected
        );
        let nested = operation.configuration_operation();
        assert_eq!(nested.credential_store().err().unwrap(), expected);
        assert_eq!(fixture.server_count(), 0);
    }
    let budget = operation
        .configuration_budget
        .as_ref()
        .unwrap()
        .lock()
        .unwrap();
    assert_eq!(Some(budget.attempts), expected.configuration_lock_attempts);
    assert_eq!(Some(budget.wait_ms), expected.configuration_lock_wait_ms);
    drop(budget);
    assert!(
        fixture
            .service
            .current_owner_credential_values()
            .unwrap()
            .is_empty()
    );
    assert_eq!(fixture.server_count(), 1);
}
