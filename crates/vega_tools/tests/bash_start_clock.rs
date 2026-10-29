#![cfg(target_os = "macos")]

use std::fs::{self, DirBuilder, OpenOptions};
use std::future::Future;
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;
use vega_tools::{
    BashError, BashErrorCode, BashLaunchPolicy, BashOutput, PreparedBash, PreparedBashScope,
    ToolCredentialPolicy, Tools,
};

const WAIT_LIMIT: Duration = Duration::from_secs(12);
const COMMAND_TIMEOUT_SECS: u64 = 20;
static PROJECT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

type StartClock = Arc<OnceLock<Instant>>;

struct TestProject {
    base: PathBuf,
    root: PathBuf,
}

impl TestProject {
    fn new() -> Self {
        for _ in 0..128 {
            let sequence = PROJECT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let base = std::env::temp_dir().join(format!(
                "vega-bash-start-clock-{}-{sequence}",
                std::process::id()
            ));
            match DirBuilder::new().mode(0o700).create(&base) {
                Ok(()) => {
                    let base = base.canonicalize().expect("canonical temporary directory");
                    let root = base.join("project");
                    fs::create_dir(&root).expect("create project directory");
                    return Self { base, root };
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create temporary directory: {error}"),
            }
        }
        panic!("could not create a unique temporary directory")
    }

    fn tools(&self) -> Tools {
        Tools::new(&self.root).expect("bind tools to project directory")
    }

    fn directory(&self, name: &str) -> PathBuf {
        let path = self.base.join(name);
        fs::create_dir(&path).expect("create temporary directory");
        path.canonicalize().expect("canonical temporary directory")
    }

    async fn create_gate(&self) {
        let mut command = Command::new("/usr/bin/mkfifo");
        command
            .arg(self.root.join("gate"))
            .stdin(Stdio::null())
            .kill_on_drop(true);
        let output = tokio::time::timeout(WAIT_LIMIT, command.output())
            .await
            .expect("mkfifo did not finish")
            .expect("run mkfifo");
        assert!(output.status.success(), "mkfifo failed: {output:?}");
    }

    fn ready(&self) -> PathBuf {
        self.root.join("ready")
    }

    fn gate(&self) -> PathBuf {
        self.root.join("gate")
    }
}

impl Drop for TestProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn prepare(
    tools: &Tools,
    command: &str,
    requested_scope: Option<Value>,
    timeout_secs: u64,
) -> PreparedBash {
    let mut input = json!({"command": command, "timeout": timeout_secs});
    if let Some(requested_scope) = requested_scope {
        input["requested_scope"] = requested_scope;
    }
    tools
        .prepare_bash_json(&input.to_string())
        .expect("prepare valid bash input")
}

fn write_scope(directory: &Path) -> Value {
    json!({
        "network": false,
        "write_roots": [directory.to_str().expect("UTF-8 scope directory")],
        "reason": "Write a temporary test result"
    })
}

fn prepare_scope(tools: &Tools, prepared: &PreparedBash) -> PreparedBashScope {
    let request = prepared.requested_scope().expect("requested scope").clone();
    tools
        .prepare_bash_scope(prepared, BashLaunchPolicy::WorkspaceWrite, request, vec![])
        .expect("prepare matching scope")
}

fn tools_with_empty_bindings(project: &TestProject) -> Tools {
    let protected = project.directory("protected");
    let policy = ToolCredentialPolicy::new(
        protected,
        vec![],
        Arc::new(|_: &str| panic!("empty bindings must not read credentials")),
    )
    .expect("configure empty credential policy")
    .with_bindings_reader(Arc::new(|| Ok(vec![])));
    project.tools().with_credential_policy(policy)
}

fn shell_quote(path: &Path) -> String {
    let text = path.to_str().expect("UTF-8 shell path");
    format!("'{}'", text.replace('\'', "'\\''"))
}

async fn read_ready_pid(path: &Path) -> u32 {
    loop {
        match fs::read_to_string(path) {
            Ok(text) => {
                if let Some(line) = text.strip_suffix('\n') {
                    let pid = line.parse::<u32>().expect("complete shell process ID");
                    assert!(pid > 0);
                    assert_ne!(pid, std::process::id());
                    return pid;
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => panic!("read ready marker: {error}"),
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn wait_until_running<F>(path: &Path, execution: Pin<&mut F>) -> u32
where
    F: Future<Output = Result<BashOutput, BashError>>,
{
    tokio::select! {
        result = execution => panic!("command ended before reaching the gate: {result:?}"),
        result = tokio::time::timeout(WAIT_LIMIT, read_ready_pid(path)) => {
            result.expect("command did not reach the gate")
        }
    }
}

async fn assert_still_running<F>(execution: Pin<&mut F>)
where
    F: Future<Output = Result<BashOutput, BashError>>,
{
    tokio::select! {
        result = execution => panic!("command ended while the gate was closed: {result:?}"),
        _ = tokio::time::sleep(Duration::from_millis(120)) => {}
    }
}

async fn release_gate(path: &Path) {
    tokio::time::timeout(WAIT_LIMIT, async {
        loop {
            match OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(path)
            {
                Ok(mut gate) => {
                    gate.write_all(b"continue\n").expect("release gate");
                    return;
                }
                Err(error) if error.raw_os_error() == Some(libc::ENXIO) => {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                Err(error) => panic!("open gate: {error}"),
            }
        }
    })
    .await
    .expect("command did not open the gate");
}

fn published_start(clock: &StartClock, before: Instant) -> Instant {
    let observed = Instant::now();
    let started = *clock
        .get()
        .expect("start published while command is running");
    assert!(before <= started && started <= observed);
    started
}

fn assert_duration_between(
    output: &BashOutput,
    before: Instant,
    started: Instant,
    release: Instant,
    completed: Instant,
) {
    let minimum_ms = release.duration_since(started).as_millis();
    let maximum_ms = completed.duration_since(before).as_millis();
    assert!(minimum_ms > 0, "the command was not held at the gate");
    assert!(
        (minimum_ms..=maximum_ms).contains(&u128::from(output.duration_ms)),
        "duration {}ms is outside {minimum_ms}..={maximum_ms}ms",
        output.duration_ms
    );
}

async fn assert_process_gone(pid: u32) {
    let mut command = Command::new("/bin/kill");
    command
        .args(["-0", "--", &pid.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let status = tokio::time::timeout(WAIT_LIMIT, command.status())
        .await
        .expect("process inspection did not finish")
        .expect("inspect child process");
    assert!(!status.success(), "shell process {pid} is still running");
}

async fn assert_unstarted_error(
    tools: &Tools,
    prepared: PreparedBash,
    policy: BashLaunchPolicy,
    scope: Option<PreparedBashScope>,
    cancel: CancellationToken,
    expected: BashErrorCode,
) {
    let clock = StartClock::default();
    let result = tokio::time::timeout(
        WAIT_LIMIT,
        tools.execute_bash_scoped_with_start(prepared, policy, scope, cancel, Some(clock.clone())),
    )
    .await
    .expect("unstarted command did not finish");
    assert_eq!(result.expect_err("command must not start").code(), expected);
    assert!(clock.get().is_none(), "unstarted command published a clock");
}

enum RunMode {
    FullAccess,
    Sandbox,
    ScopedSandbox,
}

async fn assert_running_command(mode: RunMode) {
    let project = TestProject::new();
    project.create_gate().await;
    let mut command =
        String::from("printf '%s\\n' \"$$\" > ready && IFS= read -r token < gate && ");
    let (tools, policy, requested_scope, scope_output) = match mode {
        RunMode::FullAccess => (project.tools(), BashLaunchPolicy::FullAccess, None, None),
        RunMode::Sandbox => (
            project.tools(),
            BashLaunchPolicy::WorkspaceWrite,
            None,
            None,
        ),
        RunMode::ScopedSandbox => {
            let directory = project.directory("additional-write-root");
            let output = directory.join("result");
            command.push_str(&format!("printf 'scoped' > {} && ", shell_quote(&output)));
            (
                tools_with_empty_bindings(&project),
                BashLaunchPolicy::WorkspaceWrite,
                Some(write_scope(&directory)),
                Some(output),
            )
        }
    };
    command.push_str("printf 'stdout-line\\n' && printf 'stderr-line\\n' >&2");
    let prepared = prepare(&tools, &command, requested_scope, COMMAND_TIMEOUT_SECS);
    let scope = scope_output
        .as_ref()
        .map(|_| prepare_scope(&tools, &prepared));
    let clock = StartClock::default();
    let execution = tools.execute_bash_scoped_with_start(
        prepared,
        policy,
        scope,
        CancellationToken::new(),
        Some(clock.clone()),
    );
    tokio::pin!(execution);
    assert!(clock.get().is_none());
    let before = Instant::now();
    wait_until_running(&project.ready(), execution.as_mut()).await;
    let started = published_start(&clock, before);
    assert_still_running(execution.as_mut()).await;
    assert_eq!(clock.get(), Some(&started));
    let release = Instant::now();
    release_gate(&project.gate()).await;
    let output = tokio::time::timeout(WAIT_LIMIT, execution.as_mut())
        .await
        .expect("released command did not finish")
        .expect("released command failed");
    let completed = Instant::now();
    assert_eq!(output.exit_code, 0);
    assert_eq!(output.text, "stdout-line\nstderr-line");
    assert!(!output.truncated);
    assert_eq!(clock.get(), Some(&started));
    assert_duration_between(&output, before, started, release, completed);
    if let Some(path) = scope_output {
        assert_eq!(
            fs::read_to_string(path).expect("read scoped result"),
            "scoped"
        );
    }
}

#[tokio::test]
async fn full_access_publishes_start_during_execution_and_preserves_output() {
    assert_running_command(RunMode::FullAccess).await;
}

#[tokio::test]
async fn sandbox_publishes_start_during_execution_and_preserves_output() {
    assert_running_command(RunMode::Sandbox).await;
}

#[tokio::test]
async fn scoped_sandbox_publishes_start_during_execution_and_preserves_output() {
    assert_running_command(RunMode::ScopedSandbox).await;
}

fn assert_sandbox_preparation_before_start(cancel_before_release: bool) {
    let gate_limit = WAIT_LIMIT + WAIT_LIMIT;
    let project = TestProject::new();
    let directory = project.directory("additional-write-root");
    let protected = project.directory("protected");
    let (arrived, arrived_rx) = mpsc::sync_channel(1);
    let (release, release_rx) = mpsc::sync_channel(1);
    let release_rx = Mutex::new(release_rx);
    let snapshot_count = AtomicU64::new(0);
    let policy = ToolCredentialPolicy::new(
        protected,
        vec![],
        Arc::new(|_: &str| panic!("empty bindings must not read credentials")),
    )
    .expect("configure empty credential policy")
    .with_bindings_reader(Arc::new(move || {
        if snapshot_count.fetch_add(1, Ordering::SeqCst) == 1 {
            arrived.send(()).expect("report pending preparation");
            release_rx
                .lock()
                .expect("lock preparation gate")
                .recv_timeout(gate_limit)
                .expect("release preparation gate");
        }
        Ok(vec![])
    }));
    let tools = project.tools().with_credential_policy(policy);
    let prepared = prepare(
        &tools,
        "printf scoped",
        Some(write_scope(&directory)),
        COMMAND_TIMEOUT_SECS,
    );
    let scope = prepare_scope(&tools, &prepared);
    let clock = StartClock::default();
    let worker_clock = clock.clone();
    let cancel = CancellationToken::new();
    let worker_cancel = cancel.clone();
    let (finished, finished_rx) = mpsc::sync_channel(1);
    let before = Instant::now();
    let worker = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("create worker runtime");
        let result = runtime.block_on(async {
            tokio::time::timeout(
                WAIT_LIMIT,
                tools.execute_bash_scoped_with_start(
                    prepared,
                    BashLaunchPolicy::WorkspaceWrite,
                    Some(scope),
                    worker_cancel,
                    Some(worker_clock),
                ),
            )
            .await
            .expect("scoped execution did not finish")
        });
        finished.send(result).expect("return scoped result");
    });
    arrived_rx
        .recv_timeout(WAIT_LIMIT)
        .expect("preparation did not reach binding snapshot after sandbox self-test");
    assert!(clock.get().is_none(), "preparation published a start");
    assert!(
        matches!(finished_rx.try_recv(), Err(mpsc::TryRecvError::Empty)),
        "execution ended while preparation was blocked"
    );
    if cancel_before_release {
        cancel.cancel();
    }
    let released = Instant::now();
    release.send(()).expect("release preparation");
    let result = finished_rx
        .recv_timeout(WAIT_LIMIT)
        .expect("scoped execution did not return");
    let completed = Instant::now();
    let worker_deadline = Instant::now() + WAIT_LIMIT;
    while !worker.is_finished() {
        assert!(Instant::now() < worker_deadline, "worker did not terminate");
        std::thread::sleep(Duration::from_millis(20));
    }
    worker.join().expect("join scoped execution");
    if cancel_before_release {
        assert_eq!(
            result.expect_err("preparation was cancelled").code(),
            BashErrorCode::Cancelled
        );
        assert!(
            clock.get().is_none(),
            "cancelled preparation published a start"
        );
    } else {
        let output = result.expect("scoped command failed");
        let started = *clock.get().expect("scoped execution published a start");
        assert!(before <= released && released <= started && started <= completed);
        assert_eq!(output.text, "scoped");
        assert_eq!(output.exit_code, 0);
        assert!(!output.truncated);
        assert!(u128::from(output.duration_ms) <= completed.duration_since(released).as_millis());
    }
}

#[test]
fn sandbox_self_test_does_not_publish_start_before_managed_spawn() {
    assert_sandbox_preparation_before_start(false);
}

#[test]
fn cancellation_after_sandbox_self_test_does_not_publish_start() {
    assert_sandbox_preparation_before_start(true);
}

#[tokio::test]
async fn cancelled_before_launch_does_not_publish_start() {
    let project = TestProject::new();
    let tools = project.tools();
    let prepared = prepare(&tools, "printf never", None, COMMAND_TIMEOUT_SECS);
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_unstarted_error(
        &tools,
        prepared,
        BashLaunchPolicy::FullAccess,
        None,
        cancel,
        BashErrorCode::Cancelled,
    )
    .await;
}

#[tokio::test]
async fn requested_scope_without_capability_does_not_publish_start() {
    let project = TestProject::new();
    let directory = project.directory("additional-write-root");
    let tools = project.tools();
    let prepared = prepare(
        &tools,
        "printf never",
        Some(write_scope(&directory)),
        COMMAND_TIMEOUT_SECS,
    );
    assert_unstarted_error(
        &tools,
        prepared,
        BashLaunchPolicy::WorkspaceWrite,
        None,
        CancellationToken::new(),
        BashErrorCode::ScopeMismatch,
    )
    .await;
}

#[tokio::test]
async fn scope_bound_to_another_command_does_not_publish_start() {
    let project = TestProject::new();
    let directory = project.directory("additional-write-root");
    let tools = project.tools();
    let first = prepare(
        &tools,
        "printf first",
        Some(write_scope(&directory)),
        COMMAND_TIMEOUT_SECS,
    );
    let scope = prepare_scope(&tools, &first);
    let second = prepare(
        &tools,
        "printf second",
        Some(write_scope(&directory)),
        COMMAND_TIMEOUT_SECS,
    );
    assert_unstarted_error(
        &tools,
        second,
        BashLaunchPolicy::WorkspaceWrite,
        Some(scope),
        CancellationToken::new(),
        BashErrorCode::ScopeMismatch,
    )
    .await;
}

#[tokio::test]
async fn scope_directory_replaced_before_launch_does_not_publish_start() {
    let project = TestProject::new();
    let directory = project.directory("additional-write-root");
    let tools = project.tools();
    let prepared = prepare(
        &tools,
        "printf never",
        Some(write_scope(&directory)),
        COMMAND_TIMEOUT_SECS,
    );
    let scope = prepare_scope(&tools, &prepared);
    fs::rename(&directory, project.base.join("original-write-root"))
        .expect("move original scope directory");
    fs::create_dir(&directory).expect("replace scope directory");
    assert_unstarted_error(
        &tools,
        prepared,
        BashLaunchPolicy::WorkspaceWrite,
        Some(scope),
        CancellationToken::new(),
        BashErrorCode::ScopeMismatch,
    )
    .await;
}

#[tokio::test]
async fn command_bound_to_another_tools_instance_does_not_publish_start() {
    let project = TestProject::new();
    let first = project.tools();
    let second = project.tools();
    let prepared = prepare(&first, "printf never", None, COMMAND_TIMEOUT_SECS);
    assert_unstarted_error(
        &second,
        prepared,
        BashLaunchPolicy::FullAccess,
        None,
        CancellationToken::new(),
        BashErrorCode::ScopeMismatch,
    )
    .await;
}

#[tokio::test]
async fn sandbox_preflight_failure_does_not_publish_start() {
    let project = TestProject::new();
    let original = project.root.join("original");
    fs::write(&original, "fixture").expect("create preflight fixture");
    fs::hard_link(&original, project.root.join("hardlink")).expect("create hardlink fixture");
    let tools = project.tools();
    let prepared = prepare(&tools, "printf never", None, COMMAND_TIMEOUT_SECS);
    assert_unstarted_error(
        &tools,
        prepared,
        BashLaunchPolicy::WorkspaceWrite,
        None,
        CancellationToken::new(),
        BashErrorCode::HardlinkPreflight,
    )
    .await;
}

#[tokio::test]
async fn spawn_failure_does_not_publish_start() {
    let project = TestProject::new();
    let tools = project.tools();
    let prepared = prepare(&tools, "printf never", None, COMMAND_TIMEOUT_SECS);
    fs::remove_dir(&project.root).expect("remove empty project before spawn");
    assert_unstarted_error(
        &tools,
        prepared,
        BashLaunchPolicy::FullAccess,
        None,
        CancellationToken::new(),
        BashErrorCode::SpawnFailed,
    )
    .await;
}

async fn assert_cancelled_running_command(policy: BashLaunchPolicy) {
    let project = TestProject::new();
    project.create_gate().await;
    let tools = project.tools();
    let prepared = prepare(
        &tools,
        "printf '%s\\n' \"$$\" > ready && IFS= read -r token < gate && printf unexpected",
        None,
        COMMAND_TIMEOUT_SECS,
    );
    let cancel = CancellationToken::new();
    let clock = StartClock::default();
    let execution = tools.execute_bash_scoped_with_start(
        prepared,
        policy,
        None,
        cancel.clone(),
        Some(clock.clone()),
    );
    tokio::pin!(execution);
    let before = Instant::now();
    let pid = wait_until_running(&project.ready(), execution.as_mut()).await;
    let started = published_start(&clock, before);
    assert_still_running(execution.as_mut()).await;
    cancel.cancel();
    let result = tokio::time::timeout(WAIT_LIMIT, execution.as_mut())
        .await
        .expect("cancelled command did not finish");
    assert_eq!(
        result.expect_err("command must be cancelled").code(),
        BashErrorCode::Cancelled
    );
    assert_eq!(clock.get(), Some(&started));
    assert_process_gone(pid).await;
}

#[tokio::test]
async fn full_access_cancellation_after_start_reaps_the_blocked_command() {
    assert_cancelled_running_command(BashLaunchPolicy::FullAccess).await;
}

#[tokio::test]
async fn sandbox_cancellation_after_start_reaps_the_blocked_command() {
    assert_cancelled_running_command(BashLaunchPolicy::WorkspaceWrite).await;
}

#[tokio::test]
async fn timeout_after_start_reaps_the_blocked_command() {
    let project = TestProject::new();
    project.create_gate().await;
    let tools = project.tools();
    let prepared = prepare(
        &tools,
        "printf '%s\\n' \"$$\" > ready && IFS= read -r token < gate && printf unexpected",
        None,
        8,
    );
    let clock = StartClock::default();
    let execution = tools.execute_bash_scoped_with_start(
        prepared,
        BashLaunchPolicy::FullAccess,
        None,
        CancellationToken::new(),
        Some(clock.clone()),
    );
    tokio::pin!(execution);
    let before = Instant::now();
    let pid = wait_until_running(&project.ready(), execution.as_mut()).await;
    let started = published_start(&clock, before);
    let result = tokio::time::timeout(WAIT_LIMIT, execution.as_mut())
        .await
        .expect("timed-out command did not finish");
    assert_eq!(
        result.expect_err("command must time out").code(),
        BashErrorCode::TimedOut
    );
    assert_eq!(clock.get(), Some(&started));
    assert_process_gone(pid).await;
}

#[tokio::test]
async fn short_command_does_not_change_a_running_neighbors_clock() {
    let project = TestProject::new();
    project.create_gate().await;
    let tools = project.tools();
    let long = prepare(
        &tools,
        "printf '%s\\n' \"$$\" > ready && IFS= read -r token < gate && printf long",
        None,
        COMMAND_TIMEOUT_SECS,
    );
    let long_clock = StartClock::default();
    let short_clock = StartClock::default();
    let long_execution = tools.execute_bash_scoped_with_start(
        long,
        BashLaunchPolicy::FullAccess,
        None,
        CancellationToken::new(),
        Some(long_clock.clone()),
    );
    tokio::pin!(long_execution);
    let long_before = Instant::now();
    wait_until_running(&project.ready(), long_execution.as_mut()).await;
    let long_started = published_start(&long_clock, long_before);
    assert_still_running(long_execution.as_mut()).await;
    assert!(short_clock.get().is_none());

    let short = prepare(&tools, "printf short; exit 7", None, COMMAND_TIMEOUT_SECS);
    let short_before = Instant::now();
    let short_execution = tools.execute_bash_scoped_with_start(
        short,
        BashLaunchPolicy::FullAccess,
        None,
        CancellationToken::new(),
        Some(short_clock.clone()),
    );
    let short_output = tokio::select! {
        result = long_execution.as_mut() => panic!("neighbor ended before release: {result:?}"),
        result = tokio::time::timeout(WAIT_LIMIT, short_execution) => {
            result.expect("short command did not finish").expect("short command failed to execute")
        }
    };
    let short_completed = Instant::now();
    let short_started = *short_clock.get().expect("short command published a start");
    assert!(short_before <= short_started && short_started <= short_completed);
    assert!(long_started < short_started);
    assert_eq!(short_output.text, "short");
    assert_eq!(short_output.exit_code, 7);
    assert!(!short_output.truncated);
    assert!(
        u128::from(short_output.duration_ms)
            <= short_completed.duration_since(short_before).as_millis()
    );
    assert_eq!(long_clock.get(), Some(&long_started));
    assert_still_running(long_execution.as_mut()).await;

    let release = Instant::now();
    release_gate(&project.gate()).await;
    let long_output = tokio::time::timeout(WAIT_LIMIT, long_execution.as_mut())
        .await
        .expect("neighbor did not finish after release")
        .expect("neighbor failed to execute");
    let long_completed = Instant::now();
    assert_eq!(long_output.text, "long");
    assert_eq!(long_output.exit_code, 0);
    assert!(!long_output.truncated);
    assert_duration_between(
        &long_output,
        long_before,
        long_started,
        release,
        long_completed,
    );
    assert_eq!(long_clock.get(), Some(&long_started));
    assert_eq!(short_clock.get(), Some(&short_started));
}
