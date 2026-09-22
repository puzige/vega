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
