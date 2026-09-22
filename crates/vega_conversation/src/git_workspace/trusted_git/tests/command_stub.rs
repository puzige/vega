//! Finite command stubs for application policy tests, not a Git implementation.
//! command-fixtures.json contains raw outputs captured from nine owned Git states.
//! Only commit OIDs are normalized consistently; real parsers consume every byte.
use super::*;

#[derive(Clone, serde::Deserialize)]
struct FixtureData {
    raw: BTreeMap<String, String>,
    files: BTreeMap<String, String>,
    executable_files: Vec<String>,
}

struct StubState {
    data: FixtureData,
    requests: Vec<Vec<OsString>>,
    unexpected: Vec<Vec<OsString>>,
}

struct CommandStub {
    root: PathBuf,
    state: Mutex<StubState>,
}

pub(super) struct PolicyFixture {
    dir: tempfile::TempDir,
    backend: Arc<CommandStub>,
}

impl PolicyFixture {
    pub(super) fn new(case: &str) -> Self {
        let dir = tempfile::tempdir().expect("policy fixture directory");
        let root = fs::canonicalize(dir.path()).expect("policy root");
        // A plain directory for real marker/path validation; never a Git repository.
        fs::create_dir(root.join("metadata")).expect("marker directory");
        let backend = Arc::new(CommandStub {
            root,
            state: Mutex::new(StubState {
                data: Self::data(case),
                requests: Vec::new(),
                unexpected: Vec::new(),
            }),
        });
        let fixture = Self { dir, backend };
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
        let next = Self::data(case);
        for path in state.data.files.keys() {
            if !next.files.contains_key(path) {
                fs::remove_file(self.dir.path().join(path)).expect("remove old fixture file");
            }
        }
        for (path, content) in &next.files {
            let path = self.dir.path().join(path);
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
        state.data = next;
    }

    pub(super) fn operation_marker(&self) {
        fs::write(self.dir.path().join("metadata/MERGE_HEAD"), b"marker")
            .expect("operation marker");
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
        let raw = &state.data.raw;
        let bytes = if command.get_program() == OsStr::new("/dev/null")
            && command.get_current_dir() == Some(self.root.as_path())
            && args.starts_with(&prefix)
            && strings.len() == args.len()
        {
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
                ] if input == Some(raw["paths"].as_bytes()) => Some(Vec::new()),
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
