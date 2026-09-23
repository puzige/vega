//! Finite command stubs for application policy tests, not a Git implementation.
//! command-fixtures.json contains raw outputs captured from owned Git states.
//! Only commit OIDs are normalized consistently; real parsers consume every byte.
use super::*;

#[derive(Clone, serde::Deserialize)]
struct FixtureData {
    raw: BTreeMap<String, String>,
    files: BTreeMap<String, String>,
    executable_files: Vec<String>,
    #[serde(default)]
    attrs_by_input: BTreeMap<String, String>,
}

/// One explicit external mutation response: the exact argv the production code
/// must build, the captured state the real Git mutation produced (if any), and
/// the typed result the process boundary reports. `record` distinguishes a real
/// spawn from a modelled pre-spawn denial (missing executable / pre-cancel).
struct ExpectedMutation {
    argv: Vec<OsString>,
    post: Option<FixtureData>,
    result: Option<GitWorkspaceErrorCode>,
    record: bool,
}

struct StubState {
    data: FixtureData,
    requests: Vec<Vec<OsString>>,
    unexpected: Vec<Vec<OsString>>,
    mutations_expected: std::collections::VecDeque<ExpectedMutation>,
    mutations: Vec<(Vec<OsString>, Vec<u8>)>,
    selected_attrs_expected: std::collections::VecDeque<(String, String)>,
    proof_plan: Option<String>,
    /// Armed by the test: the next declared mutation that is served converts
    /// this into `status_fault_pending`, so no capture before the mutation can
    /// consume the fault.
    fail_owner_capture_after_mutation: bool,
    /// Set when the declared mutation is served: the next `build_snapshot`
    /// capture must report one transient process failure.
    status_fault_pending: bool,
    /// Latched by the filter-identity read that begins a `build_snapshot`
    /// capture while `status_fault_pending` is set, then consumed by that same
    /// capture's first `status` read. The fault therefore lands exactly on the
    /// owner's first post-mutation authoritative capture and nowhere else.
    status_fault_ready: bool,
    /// How many transient status faults the boundary actually reported. The
    /// owning test asserts this is exactly one, so a silently unarmed fault
    /// cannot make the retry assertion pass without exercising the retry.
    faults_served: u32,
    /// At most one declared completion barrier. When armed, the matching
    /// command signals that it was reached and then waits for an explicit
    /// release before the production call continues. This replaces a shell
    /// `sleep` gate with a deterministic in-process handshake, so a concurrent
    /// ordinary poll can be ordered between one mutation and its owner capture.
    gate: Option<Gate>,
}

/// Which declared command of the in-flight operation the barrier holds.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum GateTarget {
    /// The declared external mutation, held *after* its captured post-state is
    /// applied so a concurrent ordinary poll observes the post state.
    Mutation,
    /// The commit-summary `diff --cached --patch` read.
    Summary,
}

struct Gate {
    target: GateTarget,
    entered: Option<tokio::sync::oneshot::Sender<()>>,
    release: std::sync::mpsc::Receiver<()>,
}

/// Handle to one armed completion barrier.
pub(super) struct GateHandle {
    entered: Option<tokio::sync::oneshot::Receiver<()>>,
    release: std::sync::mpsc::Sender<()>,
}

impl GateHandle {
    /// Awaits the moment the in-flight operation reaches the declared command.
    pub(super) async fn wait_entered(&mut self) {
        let entered = self.entered.take().expect("gate entered receiver");
        tokio::time::timeout(Duration::from_secs(10), entered)
            .await
            .expect("gated command entered the barrier")
            .expect("gate entered signal");
    }

    /// Lets the gated operation continue past the barrier.
    pub(super) fn release(&self) {
        self.release.send(()).expect("release gated command");
    }
}

struct CommandStub {
    root: PathBuf,
    state: Mutex<StubState>,
}

pub(super) struct PolicyFixture {
    dir: tempfile::TempDir,
    backend: Arc<CommandStub>,
    fixtures: BTreeMap<String, FixtureData>,
}

impl PolicyFixture {
    pub(super) fn new(case: &str) -> Self {
        Self::new_from_json(include_str!("command-fixtures.json"), case)
    }

    /// Captured owned-Git states for the service mutation-outcome cases. The
    /// raw bytes are the same adapter inputs as `command-fixtures.json`; they
    /// live separately only so one mutation may apply a real post-state.
    pub(super) fn service(case: &str) -> Self {
        Self::new_from_json(include_str!("service-fixtures.json"), case)
    }

    /// Captured owned-Git states for the explicit filter-value, `.gitattributes`
    /// and attribute-drift policy cases.
    pub(super) fn filter(case: &str) -> Self {
        Self::new_from_json(include_str!("filter-fixtures.json"), case)
    }

    pub(super) fn new_from_json(json: &str, case: &str) -> Self {
        let fixtures: BTreeMap<String, FixtureData> =
            serde_json::from_str(json).expect("raw fixtures");
        let dir = tempfile::tempdir().expect("policy fixture directory");
        let root = fs::canonicalize(dir.path()).expect("policy root");
        // A plain directory for real marker/path validation; never a Git repository.
        fs::create_dir(root.join("metadata")).expect("marker directory");
        let backend = Arc::new(CommandStub {
            root,
            state: Mutex::new(StubState {
                data: fixtures[case].clone(),
                requests: Vec::new(),
                unexpected: Vec::new(),
                mutations_expected: std::collections::VecDeque::new(),
                mutations: Vec::new(),
                selected_attrs_expected: std::collections::VecDeque::new(),
                proof_plan: None,
                fail_owner_capture_after_mutation: false,
                status_fault_pending: false,
                status_fault_ready: false,
                faults_served: 0,
                gate: None,
            }),
        });
        let fixture = Self {
            dir,
            backend,
            fixtures,
        };
        fixture.set_case(case);
        fixture
    }

    fn data(case: &str) -> FixtureData {
        let mut fixtures: BTreeMap<String, FixtureData> =
            serde_json::from_str(include_str!("command-fixtures.json")).expect("raw Git fixtures");
        fixtures.remove(case).expect("known policy case")
    }

    pub(super) fn set_case(&self, case: &str) {
        let mut state = self.backend.state.lock().expect("stub state");
        let next = self.fixtures[case].clone();
        Self::apply_files(self.dir.path(), &state.data, &next);
        state.data = next;
    }

    /// Replaces the served raw bytes with a *different* captured case from
    /// `command-fixtures.json`, leaving the service fixture's own recorded state
    /// behind. Used to drive one authoritative drift that only a re-read of the
    /// external bytes can observe, without publishing a new workspace generation.
    pub(super) fn set_captured_case(&self, case: &str) {
        let next = Self::data(case);
        let mut state = self.backend.state.lock().expect("stub state");
        Self::apply_files(self.dir.path(), &state.data, &next);
        state.data = next;
    }

    fn apply_files(root: &Path, before: &FixtureData, next: &FixtureData) {
        for path in before.files.keys() {
            if !next.files.contains_key(path) {
                fs::remove_file(root.join(path)).expect("remove old fixture file");
            }
        }
        for (path, content) in &next.files {
            let path = root.join(path);
            if fs::read(&path).ok().as_deref() != Some(content.as_bytes()) {
                fs::write(&path, content).expect("plain worktree file");
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
            let existing = fs::metadata(&path)
                .expect("fixture file metadata")
                .permissions();
            if existing.mode() & 0o777 != mode {
                fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("file mode");
            }
        }
    }

    pub(super) fn operation_marker(&self) {
        fs::write(self.dir.path().join("metadata/MERGE_HEAD"), b"marker")
            .expect("operation marker");
    }

    /// Replaces only the captured status raw for subsequent reads. Used to feed
    /// one malformed branch header through the real parser without any Git
    /// process or second repository. No other command response changes.
    pub(super) fn override_status(&self, status: &str) {
        self.backend
            .state
            .lock()
            .expect("stub state")
            .data
            .raw
            .insert("status".to_owned(), status.to_owned());
    }

    // Explicit external protocol responses, never simulated Git operations.
    // Only the expected mutations are ordered; ordinary reads remain unordered.
    pub(super) fn expect_mutation(&self, verb: &str, args: &[&str], post_case: &str) {
        self.expect_mutation_result(verb, args, Some(post_case), None, true);
    }

    /// Declares the single expected external mutation and the finite result the
    /// process boundary reports: an optional captured post-state, an optional
    /// typed process error, and whether the boundary actually spawned.
    pub(super) fn expect_mutation_result(
        &self,
        verb: &str,
        args: &[&str],
        post_case: Option<&str>,
        result: Option<GitWorkspaceErrorCode>,
        record: bool,
    ) {
        let mut state = self.backend.state.lock().expect("stub state");
        let argv = PREFIX
            .iter()
            .copied()
            .chain(["-c", "core.hooksPath=/dev/null", verb])
            .chain(args.iter().copied())
            .map(OsString::from)
            .collect();
        state.mutations_expected.push_back(ExpectedMutation {
            argv,
            post: post_case.map(|case| self.fixtures[case].clone()),
            result,
            record,
        });
    }

    /// Declares the exact captured `check-attr` response for the *next* selected
    /// path read, keyed by the NUL-joined stdin the production code must build.
    /// Ordinary whole-index filter reads remain the fixture's `attrs`. Unknown
    /// inputs still fail, so a missing declaration cannot silently pass.
    pub(super) fn expect_selected_attrs(&self, input: &[u8], response: &str) {
        let key = std::str::from_utf8(input)
            .expect("attribute stdin is UTF-8")
            .to_owned();
        assert!(
            self.fixtures
                .values()
                .any(|fixture| fixture.attrs_by_input.contains_key(&key)),
            "selected attrs key {key:?} is not a captured state"
        );
        self.backend
            .state
            .lock()
            .expect("stub state")
            .selected_attrs_expected
            .push_back((key, response.to_owned()));
    }

    /// Models exactly one transient process failure on the owner's first
    /// post-mutation authoritative capture: serving the next declared mutation
    /// arms the fault, the capture-opening filter-identity read latches it, and
    /// that capture's first `status` read reports `GitFailed` once. No other
    /// response changes, so the real owner-refresh retry runs.
    pub(super) fn fail_status_after_next_mutation(&self) {
        self.backend
            .state
            .lock()
            .expect("stub state")
            .fail_owner_capture_after_mutation = true;
    }

    /// Arms one deterministic completion barrier on a declared command of the
    /// in-flight operation. The matching command signals that it was reached and
    /// then waits for an explicit release, so a test can order a concurrent
    /// ordinary poll between the mutation and its owner capture without a shell
    /// `sleep` gate. The barrier is consumed once; a later operation is never
    /// reordered.
    pub(super) fn arm_gate(&self, target: GateTarget) -> GateHandle {
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let mut state = self.backend.state.lock().expect("stub state");
        assert!(state.gate.is_none(), "gate already armed");
        state.gate = Some(Gate {
            target,
            entered: Some(entered_tx),
            release: release_rx,
        });
        GateHandle {
            entered: Some(entered_rx),
            release: release_tx,
        }
    }

    /// Declares the exact captured selected-path attribute response for the
    /// fixture's own recorded path input.
    pub(super) fn expect_fixture_selected_attrs(&self, response: &str) {
        let state = self.backend.state.lock().expect("stub state");
        let key = state
            .data
            .attrs_by_input
            .keys()
            .next()
            .expect("fixture has a selected attrs key")
            .clone();
        drop(state);
        self.expect_selected_attrs(key.as_bytes(), response);
    }

    pub(super) fn proof_plan(&self, plan: &str) {
        assert!(
            [
                "pass",
                "zero-parent",
                "wrong-parent",
                "two-parent",
                "tree-diff",
                "malformed-parent",
                "short-parent",
                "mixed-parent",
                "object-missing",
                "ref-moved",
                "ref-deleted",
                "ref-renamed"
            ]
            .contains(&plan)
        );
        self.backend.state.lock().expect("stub state").proof_plan = Some(plan.into());
    }

    /// The current captured raw value for one command-fixture key (for example
    /// `head`), so a test can compare the pre/post-mutation captured identity
    /// without asserting a fabricated result.
    pub(super) fn raw(&self, key: &str) -> String {
        self.backend
            .state
            .lock()
            .expect("stub state")
            .data
            .raw
            .get(key)
            .unwrap_or_else(|| panic!("fixture raw key {key}"))
            .clone()
    }

    /// Number of transient status faults the boundary reported. Used to prove
    /// the owner-refresh retry path was actually exercised exactly once.
    pub(super) fn faults_served(&self) -> u32 {
        self.backend.state.lock().expect("stub state").faults_served
    }

    pub(super) fn mutation_argv(&self) -> Vec<u8> {
        let state = self.backend.state.lock().expect("stub state");
        self.assert_valid(&state);
        state
            .mutations
            .iter()
            .flat_map(|(args, _)| {
                args.iter()
                    .flat_map(|arg| arg.as_bytes().iter().copied().chain([0]))
            })
            .collect()
    }

    pub(super) fn mutation_inputs(&self) -> Vec<Vec<u8>> {
        let state = self.backend.state.lock().expect("stub state");
        self.assert_valid(&state);
        state
            .mutations
            .iter()
            .map(|(_, input)| input.clone())
            .collect()
    }

    fn assert_valid(&self, state: &StubState) {
        assert!(
            state.unexpected.is_empty(),
            "unexpected command: {:?}",
            state.unexpected
        );
        assert!(
            state.selected_attrs_expected.is_empty(),
            "declared selected attrs were never requested: {:?}",
            state.selected_attrs_expected
        );
        assert!(
            !self.dir.path().join(".git").exists(),
            "policy test created a Git repository"
        );
    }

    pub(super) async fn prepared(&self) -> (TrustedGitService, PreparedCommit) {
        let (_, trusted) = self.services().await;
        let checklist = trusted
            .open_checklist(CancellationToken::new())
            .await
            .expect("checklist");
        let prepared = trusted
            .prepare(checklist.id, Vec::new(), CancellationToken::new())
            .await
            .prepared
            .expect("real prepared capability");
        self.assert_no_mutation();
        (trusted, prepared)
    }

    pub(super) async fn services(&self) -> (Arc<GitWorkspaceService>, TrustedGitService) {
        let mut workspace = GitWorkspaceService::new(self.dir.path()).expect("workspace");
        workspace.command_backend = Some(self.backend.clone());
        let workspace = Arc::new(workspace);
        workspace
            .refresh(CancellationToken::new())
            .await
            .expect("stub refresh");
        let trusted = TrustedGitService::new(self.dir.path(), workspace.clone()).expect("trusted");
        (workspace, trusted)
    }

    pub(super) fn assert_no_mutation(&self) {
        let state = self.backend.state.lock().expect("stub state");
        assert!(
            !state.requests.is_empty(),
            "real service must request external reads"
        );
        assert!(
            state.unexpected.is_empty(),
            "unexpected command: {:?}",
            state.unexpected
        );
        assert!(
            state.requests.iter().all(|args| {
                !args
                    .iter()
                    .any(|arg| matches!(arg.to_str(), Some("add" | "commit" | "switch")))
            }),
            "policy attempted mutation"
        );
        assert!(
            !self.dir.path().join(".git").exists(),
            "policy test created a Git repository"
        );
    }
}

impl CommandStub {
    /// Signals that a gated command was reached, then blocks the calling
    /// operation until the test releases it. The stub lock is always dropped by
    /// the caller first, so a concurrent ordinary poll can still be served.
    fn serve_gate(gate: Gate) {
        if let Some(entered) = gate.entered {
            let _ = entered.send(());
        }
        let _ = gate.release.recv_timeout(Duration::from_secs(30));
    }
}

impl GitCommandBackend for CommandStub {
    fn execute(
        &self,
        command: &Command,
        input: Option<&[u8]>,
        stdout_limit: usize,
    ) -> Result<Output, GitWorkspaceError> {
        let args: Vec<OsString> = command.get_args().map(OsStr::to_os_string).collect();
        let mut state = self.state.lock().expect("stub state");
        state.requests.push(args.clone());
        let prefix: Vec<OsString> = PREFIX.iter().map(OsString::from).collect();
        let strings: Vec<&str> = args.iter().filter_map(|arg| arg.to_str()).collect();
        let tail = strings.get(PREFIX.len()..).unwrap_or_default();
        let valid_command = command.get_program() == OsStr::new("/dev/null")
            && command.get_current_dir() == Some(self.root.as_path())
            && args.starts_with(&prefix)
            && strings.len() == args.len();
        // The owner-refresh fault is structural, never an nth-read counter: the
        // filter-identity read that opens a `build_snapshot` capture latches it,
        // and only that capture's first `status` read fails once. An ordinary
        // `capture_head`/authority read does not read filter identity, so the
        // fault cannot be consumed by an earlier proof read.
        let opens_snapshot_capture = valid_command
            && tail
                == [
                    "--no-optional-locks",
                    "ls-files",
                    "-z",
                    "--cached",
                    "--deduplicate",
                ];
        if opens_snapshot_capture && state.status_fault_pending {
            state.status_fault_pending = false;
            state.status_fault_ready = true;
        }
        if valid_command
            && state.status_fault_ready
            && tail
                == [
                    "--no-optional-locks",
                    "status",
                    "--porcelain=v2",
                    "-z",
                    "--branch",
                    "--renames",
                    "--untracked-files=all",
                ]
        {
            state.status_fault_ready = false;
            state.faults_served = state.faults_served.saturating_add(1);
            return Err(error(GitWorkspaceErrorCode::GitFailed));
        }
        if valid_command
            && state
                .mutations_expected
                .front()
                .is_some_and(|expected| expected.argv == args)
        {
            let expected = state
                .mutations_expected
                .pop_front()
                .expect("expected mutation");
            // Only a boundary that actually spawned records an external attempt;
            // a modelled pre-spawn denial (missing executable / pre-cancel) is not
            // an observed process invocation.
            if expected.record {
                state
                    .mutations
                    .push((args, input.map_or_else(Vec::new, <[u8]>::to_vec)));
            }
            if let Some(post) = expected.post {
                PolicyFixture::apply_files(&self.root, &state.data, &post);
                state.data = post;
            }
            // Serving the declared mutation arms the one-shot capture fault;
            // the capture boundary then latches it, so it cannot fire on any
            // capture that ran before this mutation.
            if state.fail_owner_capture_after_mutation {
                state.fail_owner_capture_after_mutation = false;
                state.status_fault_pending = true;
            }
            // The mutation barrier holds *after* the captured post-state is
            // applied, so a concurrent ordinary poll started here observes the
            // post state instead of the pre state.
            if state
                .gate
                .as_ref()
                .is_some_and(|gate| gate.target == GateTarget::Mutation)
            {
                let gate = state.gate.take().expect("armed mutation gate");
                drop(state);
                Self::serve_gate(gate);
            }
            if let Some(code) = expected.result {
                return Err(error(code));
            }
            return Ok(Output {
                stdout: Vec::new(),
                overflow: false,
            });
        }
        // A declared selected-path attribute response is consumed by exact stdin
        // match before any ordinary read, so one drift is fed through the real
        // parser without changing any other command response.
        if valid_command
            && tail
                == [
                    "--no-optional-locks",
                    "check-attr",
                    "-z",
                    "--stdin",
                    "--all",
                ]
            && let Some(input) = input
            && let Ok(text) = std::str::from_utf8(input)
            && state
                .selected_attrs_expected
                .front()
                .is_some_and(|(key, _)| key == text)
        {
            let (_, response) = state
                .selected_attrs_expected
                .pop_front()
                .expect("declared selected attrs");
            let bytes = response.into_bytes();
            if bytes.len() > stdout_limit {
                return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
            }
            return Ok(Output {
                stdout: bytes,
                overflow: false,
            });
        }
        // Faults are raw responses at immutable proof reads; ordinary reads stay unordered.
        let post_commit = state.mutations.iter().any(|(args, _)| {
            args.get(PREFIX.len() + 2)
                .is_some_and(|verb| verb == "commit")
        });
        let plan = state.proof_plan.clone();
        if valid_command && post_commit && input.is_none() {
            if tail
                == [
                    "--no-optional-locks",
                    "rev-parse",
                    &format!("{}^@", state.data.raw["head"]),
                ]
            {
                if plan.as_deref() == Some("object-missing") {
                    return Err(error(GitWorkspaceErrorCode::GitFailed));
                }
                let parents = match plan.as_deref() {
                    Some("zero-parent") => String::new(),
                    Some("wrong-parent") => format!("{}\n", "b".repeat(40)),
                    Some("two-parent") => format!("{0}\n{0}\n", "a".repeat(40)),
                    Some("malformed-parent") => "not-an-oid\n".into(),
                    Some("short-parent") => "0123456789abcdef\n".into(),
                    Some("mixed-parent") => format!("{}\n", "0".repeat(64)),
                    _ => state.data.raw["parents"].clone(),
                };
                return Ok(Output {
                    stdout: parents.into_bytes(),
                    overflow: false,
                });
            }
            if tail
                == [
                    "--no-optional-locks",
                    "ls-tree",
                    "-r",
                    "-z",
                    "--full-tree",
                    &state.data.raw["head"],
                ]
            {
                let tree = if plan.as_deref() == Some("tree-diff") {
                    PolicyFixture::data("staged").raw["tree"].clone()
                } else {
                    state.data.raw["tree"].clone()
                };
                // Drift occurs after the immutable tree read, before the final head proof.
                // No unrelated status/read count is part of this contract.
                if let Some(drift @ ("ref-moved" | "ref-deleted" | "ref-renamed")) = plan.as_deref()
                {
                    state.data = PolicyFixture::data(&format!("proof-{drift}"));
                    state.proof_plan = Some("pass".into());
                }
                return Ok(Output {
                    stdout: tree.into_bytes(),
                    overflow: false,
                });
            }
        }
        let raw = &state.data.raw;
        let bytes = if valid_command {
            match tail {
                ["--no-optional-locks", "rev-parse", "--show-toplevel"] => {
                    Some(format!("{}\n", self.root.display()).into_bytes())
                }
                [
                    "--no-optional-locks",
                    "rev-parse",
                    "--absolute-git-dir" | "--git-common-dir",
                ] => Some(format!("{}/metadata\n", self.root.display()).into_bytes()),
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
                    Some(format!("{}/metadata/{marker}\n", self.root.display()).into_bytes())
                }
                ["--no-optional-locks", "rev-parse", "--show-object-format"] => {
                    Some(b"sha1\n".to_vec())
                }
                [
                    "--no-optional-locks",
                    "status",
                    "--porcelain=v2",
                    "-z",
                    "--branch",
                    "--renames",
                    "--untracked-files=all",
                ] => Some(raw["status"].as_bytes().to_vec()),
                [
                    "--no-optional-locks",
                    "ls-files",
                    "-z",
                    "--cached",
                    "--deduplicate",
                ] => Some(raw["paths"].as_bytes().to_vec()),
                ["--no-optional-locks", "ls-files", "--stage", "-z"] => {
                    Some(raw["stage"].as_bytes().to_vec())
                }
                [
                    "--no-optional-locks",
                    "ls-tree",
                    "-r",
                    "-z",
                    "--full-tree",
                    head,
                ] if *head == raw["head"] => Some(raw["tree"].as_bytes().to_vec()),
                [
                    "--no-optional-locks",
                    "for-each-ref",
                    "--sort=refname",
                    "--format=%(objectname)%00%(refname)%00",
                    "refs/heads/",
                ] => Some(raw["refs"].as_bytes().to_vec()),
                [
                    "--no-optional-locks",
                    "check-attr",
                    "-z",
                    "--stdin",
                    "--all",
                ] => input.and_then(|input| {
                    state
                        .data
                        .attrs_by_input
                        .get(std::str::from_utf8(input).ok()?)
                        .map(|attrs| attrs.as_bytes().to_vec())
                        .or_else(|| {
                            (input == raw["paths"].as_bytes()).then(|| {
                                raw.get("attrs")
                                    .map_or_else(Vec::new, |attrs| attrs.as_bytes().to_vec())
                            })
                        })
                }),
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
                ] => Some(raw["staged_raw"].as_bytes().to_vec()),
                [
                    "--no-optional-locks",
                    "diff",
                    "--raw",
                    "-z",
                    "--abbrev=64",
                    "--find-renames",
                    "--no-ext-diff",
                    "--no-textconv",
                ] => Some(raw["unstaged_raw"].as_bytes().to_vec()),
                [
                    "--no-optional-locks",
                    "diff",
                    "--cached",
                    "--numstat",
                    "-z",
                    "--find-renames",
                    "--no-ext-diff",
                    "--no-textconv",
                ] => Some(raw["staged_numstat"].as_bytes().to_vec()),
                [
                    "--no-optional-locks",
                    "diff",
                    "--numstat",
                    "-z",
                    "--find-renames",
                    "--no-ext-diff",
                    "--no-textconv",
                ] => Some(raw["unstaged_numstat"].as_bytes().to_vec()),
                [
                    "-c",
                    "core.quotePath=true",
                    "--no-optional-locks",
                    "diff",
                    "--cached",
                    "--patch",
                    "--find-renames",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--full-index",
                    "--",
                ] => Some(raw["summary"].as_bytes().to_vec()),
                _ => None,
            }
        } else {
            None
        };
        let Some(bytes) = bytes else {
            state.unexpected.push(args);
            return Err(error(GitWorkspaceErrorCode::GitFailed));
        };
        // The summary barrier holds the commit-summary read after its captured
        // bytes are resolved, so a test can drive an authoritative drift while
        // the real `capture_summary` re-verification is still in flight.
        let summary_read = tail
            == [
                "-c",
                "core.quotePath=true",
                "--no-optional-locks",
                "diff",
                "--cached",
                "--patch",
                "--find-renames",
                "--no-ext-diff",
                "--no-textconv",
                "--full-index",
                "--",
            ];
        if valid_command
            && summary_read
            && state
                .gate
                .as_ref()
                .is_some_and(|gate| gate.target == GateTarget::Summary)
        {
            let gate = state.gate.take().expect("armed summary gate");
            drop(state);
            Self::serve_gate(gate);
            state = self.state.lock().expect("stub state");
        }
        if input.is_some() && !tail.contains(&"check-attr") {
            state.unexpected.push(args);
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
