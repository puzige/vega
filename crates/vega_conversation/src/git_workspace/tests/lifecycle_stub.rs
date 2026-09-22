//! Finite in-process snapshot-read stub for the workspace lifecycle race tests.
//!
//! The production `GitWorkspaceService` still runs its complete read protocol
//! over this boundary: `rev-parse --show-toplevel`, the filter identity reads,
//! `status`, both raw and numstat diffs and every re-verification. Only the
//! external `git` process is replaced by the exact bytes captured in
//! `lifecycle-fixtures.json`. One declared command may block on an explicit
//! channel so a test controls completion order instead of a shell
//! `mkdir`/`sleep` gate. Unknown commands fail closed; no command falls back to
//! a real process and no repository is created.
use super::*;

#[derive(Clone, serde::Deserialize)]
struct SnapshotData {
    raw: BTreeMap<String, String>,
    files: BTreeMap<String, String>,
    #[serde(default)]
    executable_files: Vec<String>,
}

/// Which declared read of one in-flight refresh the completion barrier holds.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum GateTarget {
    /// The first `rev-parse --show-toplevel` of the refresh.
    TopLevel,
    /// The first `status --porcelain=v2` read of the refresh.
    Status,
}

/// One declared completion barrier: the gated read signals that it was reached,
/// then waits for an explicit release before serving the fixture or reporting
/// the single modelled process failure.
struct Gate {
    target: GateTarget,
    entered: Option<tokio::sync::oneshot::Sender<()>>,
    release: std::sync::mpsc::Receiver<()>,
    fail: bool,
}

struct StubInner {
    data: SnapshotData,
    requests: Vec<Vec<OsString>>,
    unexpected: Vec<Vec<OsString>>,
    gate: Option<Gate>,
}

struct SnapshotStub {
    root: PathBuf,
    inner: Mutex<StubInner>,
}

pub(super) struct LifecycleFixture {
    dir: tempfile::TempDir,
    backend: Arc<SnapshotStub>,
    cases: BTreeMap<String, SnapshotData>,
}

pub(super) struct GateHandle {
    entered: Option<tokio::sync::oneshot::Receiver<()>>,
    release: std::sync::mpsc::Sender<()>,
}

impl GateHandle {
    /// Awaits the moment the gated refresh reaches the declared command.
    pub(super) async fn wait_entered(&mut self) {
        let entered = self.entered.take().expect("gate entered receiver");
        tokio::time::timeout(Duration::from_secs(10), entered)
            .await
            .expect("gated refresh entered the barrier")
            .expect("gate entered signal");
    }

    /// Lets the gated refresh finish its declared command.
    pub(super) fn release(&self) {
        self.release.send(()).expect("release gated refresh");
    }
}

impl LifecycleFixture {
    pub(super) fn new(case: &str) -> Self {
        let cases: BTreeMap<String, SnapshotData> =
            serde_json::from_str(include_str!("lifecycle-fixtures.json"))
                .expect("lifecycle fixtures");
        assert!(cases.contains_key(case), "unknown lifecycle case {case}");
        let dir = tempfile::tempdir().expect("lifecycle directory");
        let root = fs::canonicalize(dir.path()).expect("lifecycle root");
        let backend = Arc::new(SnapshotStub {
            root,
            inner: Mutex::new(StubInner {
                data: cases[case].clone(),
                requests: Vec::new(),
                unexpected: Vec::new(),
                gate: None,
            }),
        });
        let fixture = Self {
            dir,
            backend,
            cases,
        };
        fixture.apply_case(case);
        fixture
    }

    fn apply_case(&self, case: &str) {
        let next = self.cases[case].clone();
        let mut inner = self.backend.inner.lock().expect("lifecycle stub");
        Self::apply_files(&self.backend.root, &inner.data, &next);
        inner.data = next;
    }

    /// Switches to another captured state and its exact plain worktree files.
    pub(super) fn set_case(&self, case: &str) {
        self.apply_case(case);
    }

    fn apply_files(root: &Path, before: &SnapshotData, next: &SnapshotData) {
        for path in before.files.keys() {
            if !next.files.contains_key(path) {
                let _ = fs::remove_file(root.join(path));
            }
        }
        for (path, content) in &next.files {
            let path = root.join(path);
            if fs::read(&path).ok().as_deref() != Some(content.as_bytes()) {
                fs::write(&path, content).expect("lifecycle worktree file");
            }
            let mode = if next
                .executable_files
                .iter()
                .any(|name| path.ends_with(name))
            {
                0o755
            } else {
                0o644
            };
            fs::set_permissions(&path, fs::Permissions::from_mode(mode))
                .expect("lifecycle file mode");
        }
    }

    /// Arms the completion barrier on one declared read. The first matching
    /// command of the in-flight refresh blocks until released and then either
    /// serves the fixture or reports one typed process failure. The barrier is
    /// consumed once, so a later concurrent refresh is never reordered.
    pub(super) fn arm_gate(&self, target: GateTarget, fail: bool) -> GateHandle {
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let mut inner = self.backend.inner.lock().expect("lifecycle stub");
        assert!(inner.gate.is_none(), "gate already armed");
        inner.gate = Some(Gate {
            target,
            entered: Some(entered_tx),
            release: release_rx,
            fail,
        });
        GateHandle {
            entered: Some(entered_rx),
            release: release_tx,
        }
    }

    pub(super) fn service(&self) -> GitWorkspaceService {
        let mut workspace =
            GitWorkspaceService::new(&self.backend.root).expect("lifecycle workspace");
        workspace.command_backend = Some(self.backend.clone());
        workspace
    }

    pub(super) fn assert_clean(&self) {
        let inner = self.backend.inner.lock().expect("lifecycle stub");
        assert!(!inner.requests.is_empty(), "real service must issue reads");
        assert!(
            inner.unexpected.is_empty(),
            "unexpected command: {:?}",
            inner.unexpected
        );
        assert!(
            inner.requests.iter().all(|args| {
                !args
                    .iter()
                    .any(|arg| matches!(arg.to_str(), Some("add" | "commit" | "switch")))
            }),
            "lifecycle policy attempted a mutation"
        );
        drop(inner);
        assert!(
            !self.dir.path().join(".git").exists(),
            "lifecycle test created a Git repository"
        );
    }
}

impl GitCommandBackend for SnapshotStub {
    fn execute(
        &self,
        command: &Command,
        input: Option<&[u8]>,
        stdout_limit: usize,
    ) -> Result<Output, GitWorkspaceError> {
        let args: Vec<OsString> = command.get_args().map(OsStr::to_os_string).collect();
        let prefix: Vec<OsString> = PREFIX.iter().map(OsString::from).collect();
        let strings: Vec<&str> = args.iter().filter_map(|arg| arg.to_str()).collect();
        let tail = strings.get(PREFIX.len()..).unwrap_or_default();
        let valid_command = command.get_program() == OsStr::new("/dev/null")
            && command.get_current_dir() == Some(self.root.as_path())
            && args.starts_with(&prefix)
            && strings.len() == args.len();
        let mut inner = self.inner.lock().expect("lifecycle stub");
        inner.requests.push(args.clone());
        let gated = valid_command
            && inner.gate.as_ref().is_some_and(|gate| match gate.target {
                GateTarget::TopLevel => {
                    tail == ["--no-optional-locks", "rev-parse", "--show-toplevel"]
                }
                GateTarget::Status => {
                    tail == [
                        "--no-optional-locks",
                        "status",
                        "--porcelain=v2",
                        "-z",
                        "--branch",
                        "--renames",
                        "--untracked-files=all",
                    ]
                }
            });
        if gated {
            let gate = inner.gate.take().expect("armed gate");
            drop(inner);
            if let Some(entered) = gate.entered {
                let _ = entered.send(());
            }
            let _ = gate.release.recv_timeout(Duration::from_secs(30));
            if gate.fail {
                return Err(error(GitWorkspaceErrorCode::GitFailed));
            }
            inner = self.inner.lock().expect("lifecycle stub");
        }
        let bytes = if valid_command {
            match tail {
                ["--no-optional-locks", "rev-parse", "--show-toplevel"] => {
                    Some(format!("{}\n", self.root.display()).into_bytes())
                }
                [
                    "--no-optional-locks",
                    "ls-files",
                    "-z",
                    "--cached",
                    "--deduplicate",
                ] => Some(inner.data.raw["paths"].as_bytes().to_vec()),
                [
                    "--no-optional-locks",
                    "status",
                    "--porcelain=v2",
                    "-z",
                    "--branch",
                    "--renames",
                    "--untracked-files=all",
                ] => Some(inner.data.raw["status"].as_bytes().to_vec()),
                [
                    "--no-optional-locks",
                    "check-attr",
                    "-z",
                    "--stdin",
                    "--all",
                ] => input.map(|_| inner.data.raw["attrs"].as_bytes().to_vec()),
                [
                    "--no-optional-locks",
                    "diff",
                    "--cached",
                    "--raw",
                    "-z",
                    "--abbrev=64",
                    "--find-renames",
                    "--no-ext-diff",
                    "--no-textconv",
                ] => Some(inner.data.raw["staged_raw"].as_bytes().to_vec()),
                [
                    "--no-optional-locks",
                    "diff",
                    "--raw",
                    "-z",
                    "--abbrev=64",
                    "--find-renames",
                    "--no-ext-diff",
                    "--no-textconv",
                ] => Some(inner.data.raw["unstaged_raw"].as_bytes().to_vec()),
                [
                    "--no-optional-locks",
                    "diff",
                    "--cached",
                    "--numstat",
                    "-z",
                    "--find-renames",
                    "--no-ext-diff",
                    "--no-textconv",
                ] => Some(inner.data.raw["staged_numstat"].as_bytes().to_vec()),
                [
                    "--no-optional-locks",
                    "diff",
                    "--numstat",
                    "-z",
                    "--find-renames",
                    "--no-ext-diff",
                    "--no-textconv",
                ] => Some(inner.data.raw["unstaged_numstat"].as_bytes().to_vec()),
                _ => None,
            }
        } else {
            None
        };
        let Some(bytes) = bytes else {
            inner.unexpected.push(args);
            return Err(error(GitWorkspaceErrorCode::GitFailed));
        };
        if input.is_some() && !tail.contains(&"check-attr") {
            inner.unexpected.push(args);
            return Err(error(GitWorkspaceErrorCode::GitFailed));
        }
        if bytes.len() > stdout_limit {
            return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
        }
        Ok(Output {
            stdout: bytes,
            overflow: false,
        })
    }
}

fn run_git_output(root: &Path, args: &[&str]) -> Vec<u8> {
    let output = git_command(root, args)
        .output()
        .expect("lifecycle git output");
    assert!(output.status.success(), "lifecycle git failed: {args:?}");
    output.stdout
}

// Compare the captured lifecycle raw bytes with actual Git, normalizing only the
// nondeterministic commit identity. Paths, modes, status records, raw diff
// headers and numstat counts remain exact.
pub(super) fn assert_lifecycle_adapter_state(root: &Path, case: &str) {
    let fixtures: serde_json::Value =
        serde_json::from_str(include_str!("lifecycle-fixtures.json")).expect("lifecycle fixtures");
    let raw = &fixtures[case]["raw"];
    let born = raw["head"].as_str().expect("fixture head") != "(initial)";
    let head = if born {
        String::from_utf8(run_git_output(root, &["rev-parse", "HEAD"])).expect("head")
    } else {
        String::new()
    };
    let mut outputs = vec![
        (
            "status",
            run_git_output(
                root,
                &[
                    "status",
                    "--porcelain=v2",
                    "-z",
                    "--branch",
                    "--renames",
                    "--untracked-files=all",
                ],
            ),
        ),
        (
            "paths",
            run_git_output(root, &["ls-files", "-z", "--cached", "--deduplicate"]),
        ),
    ];
    for (key, args) in [
        (
            "staged_raw",
            vec![
                "diff",
                "--cached",
                "--raw",
                "-z",
                "--abbrev=64",
                "--find-renames",
                "--no-ext-diff",
                "--no-textconv",
            ],
        ),
        (
            "unstaged_raw",
            vec![
                "diff",
                "--raw",
                "-z",
                "--abbrev=64",
                "--find-renames",
                "--no-ext-diff",
                "--no-textconv",
            ],
        ),
        (
            "staged_numstat",
            vec![
                "diff",
                "--cached",
                "--numstat",
                "-z",
                "--find-renames",
                "--no-ext-diff",
                "--no-textconv",
            ],
        ),
        (
            "unstaged_numstat",
            vec![
                "diff",
                "--numstat",
                "-z",
                "--find-renames",
                "--no-ext-diff",
                "--no-textconv",
            ],
        ),
    ] {
        outputs.push((key, run_git_output(root, &args)));
    }
    for (key, actual) in outputs {
        let actual = String::from_utf8(actual).expect("captured UTF-8 fixture");
        let actual = if born {
            actual.replace(head.trim(), raw["head"].as_str().expect("fixture head"))
        } else {
            actual
        };
        assert_eq!(actual, raw[key].as_str().expect("raw key"), "{case}: {key}");
    }
    let paths = run_git_output(root, &["ls-files", "-z", "--cached", "--deduplicate"]);
    let mut command = Command::new(GIT);
    command
        .current_dir(root)
        .args(["check-attr", "-z", "--stdin", "--all"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped());
    scrub_git_environment(&mut command);
    let mut child = command.spawn().expect("real attribute adapter");
    std::io::Write::write_all(&mut child.stdin.take().expect("attribute stdin"), &paths)
        .expect("attribute input");
    let attrs = child.wait_with_output().expect("attribute result");
    assert!(attrs.status.success());
    assert_eq!(
        String::from_utf8(attrs.stdout).expect("attrs"),
        raw["attrs"].as_str().expect("attrs"),
        "{case}: attrs"
    );
}
