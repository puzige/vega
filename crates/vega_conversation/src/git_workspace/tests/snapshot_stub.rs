use super::*;
use std::collections::VecDeque;

#[derive(Clone, serde::Deserialize)]
struct Reply {
    args: Vec<String>,
    input: Option<String>,
    stdout: Option<String>,
    overflow: Option<bool>,
    error: Option<String>,
}

#[derive(serde::Deserialize)]
struct Case {
    replies: Vec<Reply>,
    sequence: Vec<usize>,
}

pub(super) struct NotRepositoryBackend;

impl GitCommandBackend for NotRepositoryBackend {
    fn execute(
        &self,
        command: &Command,
        input: Option<&[u8]>,
        _: usize,
    ) -> Result<Output, GitWorkspaceError> {
        assert!(command.get_args().any(|arg| arg == "rev-parse"));
        assert!(input.is_none());
        Err(error(GitWorkspaceErrorCode::NotRepository))
    }
}

struct SnapshotBackend {
    root: PathBuf,
    replies: Mutex<VecDeque<Reply>>,
}

pub(super) struct SnapshotFixture {
    dir: TempDir,
    backend: Arc<SnapshotBackend>,
}

impl SnapshotFixture {
    pub(super) fn new(case: &str) -> Self {
        let mut cases: BTreeMap<String, Case> =
            serde_json::from_str(include_str!("snapshot-fixtures.json")).unwrap();
        let case = cases.remove(case).expect("declared snapshot case");
        let replies = case
            .sequence
            .into_iter()
            .map(|index| case.replies[index].clone())
            .collect();
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".git/info")).unwrap();
        let backend = Arc::new(SnapshotBackend {
            root: fs::canonicalize(dir.path()).unwrap(),
            replies: Mutex::new(replies),
        });
        Self { dir, backend }
    }

    pub(super) fn path(&self) -> &Path {
        self.dir.path()
    }

    pub(super) fn write(&self, path: &str, bytes: &[u8]) {
        let path = self.path().join(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, bytes).unwrap();
    }

    pub(super) fn service(&self) -> GitWorkspaceService {
        let mut service = GitWorkspaceService::new(self.path()).unwrap();
        service.command_backend = Some(self.backend.clone());
        service
    }
}

impl Drop for SnapshotFixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            assert!(
                self.backend.replies.lock().unwrap().is_empty(),
                "unconsumed Git replies"
            );
        }
    }
}

impl GitCommandBackend for SnapshotBackend {
    fn execute(
        &self,
        command: &Command,
        input: Option<&[u8]>,
        stdout_limit: usize,
    ) -> Result<Output, GitWorkspaceError> {
        let reply = self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected Git command");
        let args: Vec<Vec<u8>> = command
            .get_args()
            .map(|arg| arg.as_bytes().to_vec())
            .collect();
        let expand = |bytes: String| {
            let bytes = bytes.into_bytes();
            let marker = b"<ROOT>";
            let mut expanded = Vec::new();
            let mut remaining = bytes.as_slice();
            while let Some(at) = remaining
                .windows(marker.len())
                .position(|part| part == marker)
            {
                expanded.extend_from_slice(&remaining[..at]);
                expanded.extend_from_slice(self.root.as_os_str().as_bytes());
                remaining = &remaining[at + marker.len()..];
            }
            expanded.extend_from_slice(remaining);
            expanded
        };
        assert_eq!(
            args,
            reply.args.into_iter().map(&expand).collect::<Vec<_>>()
        );
        assert_eq!(input, reply.input.as_deref().map(str::as_bytes));
        if let Some(code) = reply.error {
            return Err(error(match code.as_str() {
                "GitFailed" => GitWorkspaceErrorCode::GitFailed,
                "NotRepository" => GitWorkspaceErrorCode::NotRepository,
                "OutputTooLarge" => GitWorkspaceErrorCode::OutputTooLarge,
                other => panic!("unsupported captured failure: {other}"),
            }));
        }
        let stdout = expand(reply.stdout.unwrap());
        if stdout.len() > stdout_limit {
            return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
        }
        Ok(Output {
            stdout,
            overflow: reply.overflow.unwrap_or(false),
        })
    }
}
