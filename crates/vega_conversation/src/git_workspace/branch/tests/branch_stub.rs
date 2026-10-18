//! Finite in-process command stub for branch lease/cleanup race policy.
//!
//! The production `BranchWorkspaceService` still runs its complete read
//! protocol (`rev-parse`, operation markers, `for-each-ref`, filter identity,
//! `status`, target-tree authority diff and `check-attr`). Only the external
//! `git` process is replaced by the exact bytes captured in
//! `branch-fixtures.json`, and declared commands may block on explicit
//! channels so a test controls completion order instead of a shell
//! `mkdir`/`sleep` gate. Unknown commands and unknown stdin fail closed; no
//! command falls back to a real process and no repository is created.
use super::*;

#[derive(Clone, serde::Deserialize)]
struct BranchData {
    raw: BTreeMap<String, String>,
    files: BTreeMap<String, String>,
    #[serde(default)]
    executable_files: Vec<String>,
}

/// Which declared command a completion barrier holds.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum GateTarget {
    /// The first `rev-parse --show-toplevel` read of a capture.
    TopLevel,
    /// The trusted `switch` mutation.
    Switch,
}

struct Gate {
    target: GateTarget,
    entered: Option<tokio::sync::oneshot::Sender<()>>,
    release: std::sync::mpsc::Receiver<()>,
    fail: bool,
}

/// One queued mutation response: the post-state to install (if any) and the
/// typed result the process boundary reports. Only declared mutations are
/// served; every other `switch` fails closed.
#[derive(Clone)]
struct ExpectedMutation {
    post: Option<String>,
    result: Option<GitWorkspaceErrorCode>,
}

struct StubInner {
    data: BranchData,
    requests: Vec<Vec<OsString>>,
    unexpected: Vec<Vec<OsString>>,
    switches: Vec<OsString>,
    mutations: Vec<ExpectedMutation>,
    gates: Vec<Gate>,
}

struct BranchStub {
    root: PathBuf,
    cases: BTreeMap<String, BranchData>,
    inner: Mutex<StubInner>,
}

pub(super) struct BranchFixture {
    dir: tempfile::TempDir,
    backend: Arc<BranchStub>,
    cases: BTreeMap<String, BranchData>,
}

pub(super) struct GateHandle {
    entered: Option<tokio::sync::oneshot::Receiver<()>>,
    release: std::sync::mpsc::Sender<()>,
}

impl GateHandle {
    /// Awaits the moment the gated command is reached.
    pub(super) async fn wait_entered(&mut self) {
        let entered = self.entered.take().expect("gate entered receiver");
        tokio::time::timeout(Duration::from_secs(10), entered)
            .await
            .expect("gated command entered the barrier")
            .expect("gate entered signal");
    }

    /// Lets the gated command finish.
    pub(super) fn release(&self) {
        self.release.send(()).expect("release gated command");
    }
}

impl BranchFixture {
    pub(super) fn new(case: &str) -> Self {
        let cases: BTreeMap<String, BranchData> =
            serde_json::from_str(include_str!("branch-fixtures.json")).expect("branch fixtures");
        assert!(cases.contains_key(case), "unknown branch case {case}");
        let dir = tempfile::Builder::new()
            .prefix("vega-branch-stub-")
            .tempdir()
            .expect("branch fixture directory");
        let root = fs::canonicalize(dir.path()).expect("branch fixture root");
        // A plain directory for real operation-marker and root-identity checks.
        fs::create_dir(root.join("metadata")).expect("marker directory");
        let backend = Arc::new(BranchStub {
            root,
            cases: cases.clone(),
            inner: Mutex::new(StubInner {
                data: cases[case].clone(),
                requests: Vec::new(),
                unexpected: Vec::new(),
                switches: Vec::new(),
                mutations: Vec::new(),
                gates: Vec::new(),
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
        let mut inner = self.backend.inner.lock().expect("branch stub");
        Self::apply_files(&self.backend.root, &inner.data, &next);
        inner.data = next;
    }

    /// Switches to another captured state and its exact plain worktree files.
    pub(super) fn set_case(&self, case: &str) {
        self.apply_case(case);
    }

    fn apply_files(root: &Path, before: &BranchData, next: &BranchData) {
        for path in before.files.keys() {
            if !next.files.contains_key(path) {
                let _ = fs::remove_file(root.join(path));
            }
        }
        for (path, content) in &next.files {
            let path = root.join(path);
            if fs::read(&path).ok().as_deref() != Some(content.as_bytes()) {
                fs::write(&path, content).expect("branch worktree file");
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
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).expect("branch file mode");
        }
    }

    /// Arms one completion barrier on a declared command. The first matching
    /// command blocks until released and then either serves the fixture or
    /// reports one typed process failure. The barrier is consumed once, so a
    /// later concurrent capture is never reordered.
    pub(super) fn arm_gate(&self, target: GateTarget, fail: bool) -> GateHandle {
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let mut inner = self.backend.inner.lock().expect("branch stub");
        assert!(
            inner.gates.iter().all(|gate| gate.target != target),
            "gate already armed for target"
        );
        inner.gates.push(Gate {
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

    /// Declares the exact trusted `switch` the production code must issue and
    /// the captured state that mutation produced (if any). A declared mutation
    /// is always recorded; an undeclared `switch` fails closed.
    pub(super) fn expect_switch(&self, post_case: Option<&str>) {
        self.backend
            .inner
            .lock()
            .expect("branch stub")
            .mutations
            .push(ExpectedMutation {
                post: post_case.map(str::to_owned),
                result: None,
            });
    }

    /// Writes one real operation-marker file under the stub's plain `metadata`
    /// directory, so the production marker traversal sees a real filesystem fact.
    pub(super) fn marker(&self, name: &str) -> PathBuf {
        let path = self.backend.root.join("metadata").join(name);
        if name.contains('-') || name == "sequencer" {
            fs::create_dir(&path).expect("marker directory");
        } else {
            fs::write(&path, b"marker\n").expect("marker file");
        }
        path
    }

    pub(super) fn path(&self) -> &Path {
        self.dir.path()
    }

    pub(super) fn set_raw(&self, key: &str, value: &str) {
        self.backend
            .inner
            .lock()
            .unwrap()
            .data
            .raw
            .insert(key.into(), value.into());
    }

    pub(super) fn expect_switch_failure(&self, code: GitWorkspaceErrorCode) {
        self.backend
            .inner
            .lock()
            .unwrap()
            .mutations
            .push(ExpectedMutation {
                post: None,
                result: Some(code),
            });
    }

    pub(super) fn service(&self) -> BranchWorkspaceService {
        let mut service =
            BranchWorkspaceService::new(&self.backend.root).expect("branch workspace");
        service.command_backend = Some(self.backend.clone());
        service
    }

    pub(super) fn switch_attempts(&self) -> Vec<OsString> {
        self.backend
            .inner
            .lock()
            .expect("branch stub")
            .switches
            .clone()
    }

    pub(super) fn assert_clean(&self) {
        let inner = self.backend.inner.lock().expect("branch stub");
        assert!(!inner.requests.is_empty(), "real service must issue reads");
        assert!(
            inner.unexpected.is_empty(),
            "unexpected command: {:?}",
            inner.unexpected
        );
        assert!(
            inner.mutations.is_empty(),
            "declared switches were never requested"
        );
        assert!(inner.gates.is_empty(), "declared gates were never reached");
        drop(inner);
        assert!(
            !self.dir.path().join(".git").exists(),
            "branch policy test created a Git repository"
        );
    }
}

impl GitCommandBackend for BranchStub {
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
        let mut inner = self.inner.lock().expect("branch stub");
        inner.requests.push(args.clone());
        let gate_index = if valid_command {
            inner.gates.iter().position(|gate| match gate.target {
                GateTarget::TopLevel => {
                    tail == ["--no-optional-locks", "rev-parse", "--show-toplevel"]
                }
                GateTarget::Switch => {
                    tail.starts_with(&["-c", "core.hooksPath=/dev/null", "switch"])
                }
            })
        } else {
            None
        };
        if let Some(index) = gate_index {
            let gate = inner.gates.remove(index);
            drop(inner);
            if let Some(entered) = gate.entered {
                let _ = entered.send(());
            }
            let _ = gate.release.recv_timeout(Duration::from_secs(30));
            if gate.fail {
                return Err(error(GitWorkspaceErrorCode::GitFailed));
            }
            inner = self.inner.lock().expect("branch stub");
        }
        // The trusted `switch` mutation carries the target branch as its final
        // argv element; the captured post-state is installed afterwards.
        let is_switch =
            valid_command && tail.starts_with(&["-c", "core.hooksPath=/dev/null", "switch"]);
        if is_switch {
            let Some(expected) = inner.mutations.first().cloned() else {
                inner.unexpected.push(args);
                return Err(error(GitWorkspaceErrorCode::GitFailed));
            };
            inner.mutations.remove(0);
            inner
                .switches
                .push(args.last().cloned().unwrap_or_default());
            if let Some(post) = expected.post {
                let next = self.cases[&post].clone();
                BranchFixture::apply_files(&self.root, &inner.data, &next);
                inner.data = next;
            }
            if let Some(code) = expected.result {
                return Err(error(code));
            }
            return Ok(Output {
                stdout: Vec::new(),
                overflow: false,
            });
        }
        let bytes = if valid_command {
            match tail {
                ["--no-optional-locks", "rev-parse", "--show-toplevel"] => {
                    Some(format!("{}\n", self.root.display()).into_bytes())
                }
                [
                    "--no-optional-locks",
                    "rev-parse",
                    "--absolute-git-dir" | "--git-common-dir",
                ] => Some(
                    format!(
                        "{}\n",
                        inner
                            .data
                            .raw
                            .get("metadata")
                            .cloned()
                            .unwrap_or_else(|| format!("{}/metadata", self.root.display()))
                    )
                    .into_bytes(),
                ),
                ["--no-optional-locks", "rev-parse", "--git-path", marker]
                    if [
                        "MERGE_HEAD",
                        "CHERRY_PICK_HEAD",
                        "REVERT_HEAD",
                        "BISECT_START",
                        "BISECT_LOG",
                        "rebase-merge",
                        "rebase-apply",
                        "sequencer",
                    ]
                    .contains(marker) =>
                {
                    Some(
                        format!(
                            "{}/{marker}\n",
                            inner
                                .data
                                .raw
                                .get("metadata")
                                .cloned()
                                .unwrap_or_else(|| format!("{}/metadata", self.root.display()))
                        )
                        .into_bytes(),
                    )
                }
                [
                    "--no-optional-locks",
                    "for-each-ref",
                    "--sort=refname",
                    "--format=%(objectname)%00%(refname)%00",
                    "refs/heads/",
                ] => Some(inner.data.raw["refs"].as_bytes().to_vec()),
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
                    "ls-files",
                    "-z",
                    "--cached",
                    "--deduplicate",
                ] => Some(inner.data.raw["paths"].as_bytes().to_vec()),
                [
                    "--no-optional-locks",
                    "check-attr",
                    "-z",
                    "--stdin",
                    "--all",
                ] => input.and_then(|input| {
                    (input == inner.data.raw["paths"].as_bytes())
                        .then(|| inner.data.raw["attrs"].as_bytes().to_vec())
                }),
                [
                    "--no-optional-locks",
                    "diff",
                    "--name-status",
                    "-z",
                    "--diff-filter=ACMRT",
                    "-M",
                    "--no-ext-diff",
                    "--no-textconv",
                    current,
                    target,
                ] if *current == inner.data.raw["diff_current"]
                    && *target == inner.data.raw["diff_target"] =>
                {
                    Some(inner.data.raw["acmrt"].as_bytes().to_vec())
                }
                [
                    "--no-optional-locks",
                    "diff",
                    "--name-status",
                    "-z",
                    "--diff-filter=D",
                    "-M",
                    "--no-ext-diff",
                    "--no-textconv",
                    current,
                    target,
                ] if *current == inner.data.raw["diff_current"]
                    && *target == inner.data.raw["diff_target"] =>
                {
                    Some(inner.data.raw["deletes"].as_bytes().to_vec())
                }
                [
                    "--no-optional-locks",
                    "check-attr",
                    source,
                    "-z",
                    "--stdin",
                    "--all",
                ] if *source == format!("--source={}", inner.data.raw["diff_target"]) => {
                    input.map(|_| inner.data.raw["selected_attrs"].as_bytes().to_vec())
                }
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
