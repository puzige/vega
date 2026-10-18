use super::*;
use std::fs;
use std::os::unix::ffi::OsStringExt;

use crate::git_workspace::{GitCommandFixture, fixture_git_command};

const PROJECT_ID: &str = "project";
const THREAD_ID: &str = "thread";

mod caps_retained;
mod capture_reconcile;
mod preview_open;

struct Repo {
    dir: GitCommandFixture,
}

impl Repo {
    fn new() -> Self {
        let dir = GitCommandFixture::for_current_test(
            include_str!("artifact-git-fixtures.json"),
            "artifact fixture",
        );
        git(dir.path(), &["init", "-q"]);
        git(dir.path(), &["config", "user.name", "Vega Test"]);
        git(
            dir.path(),
            &["config", "user.email", "vega@example.invalid"],
        );
        Self { dir }
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn write(&self, path: &str, body: &[u8]) {
        let target = self.path().join(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(target, body).unwrap();
    }

    fn commit_all(&self) {
        git(self.path(), &["add", "-A"]);
        git(self.path(), &["commit", "-q", "-m", "fixture"]);
    }
}

fn git(root: &Path, args: &[&str]) {
    if let ["mv", from, to] = args {
        fs::rename(root.join(from), root.join(to)).unwrap();
    }
    assert!(
        fixture_git_command(root, args).status().unwrap().success(),
        "git {args:?}"
    );
}

fn write_call(call_id: &str, path: &str, bytes: u64) -> ToolCall {
    write_call_with_fingerprint(call_id, path, bytes, 'a')
}

fn write_call_with_fingerprint(
    call_id: &str,
    path: &str,
    bytes: u64,
    fingerprint: char,
) -> ToolCall {
    ToolCall {
        id: call_id.to_owned(),
        tool: "write".to_owned(),
        input_json: format!(
            r#"{{"audit_version":"write_edit_v1","tool":"write","path":"{path}","content_bytes":{bytes},"fingerprint_v1":"{}"}}"#,
            fingerprint.to_string().repeat(64)
        ),
    }
}

fn edit_call(call_id: &str, path: &str) -> ToolCall {
    ToolCall {
        id: call_id.to_owned(),
        tool: "edit".to_owned(),
        input_json: format!(
            r#"{{"audit_version":"write_edit_v1","tool":"edit","path":"{path}","old_string_bytes":1,"new_string_bytes":1,"fingerprint_v1":"{}"}}"#,
            "b".repeat(64)
        ),
    }
}

fn write_result(call_id: &str, path: &str, bytes: u64, reused: bool) -> ToolResult {
    write_result_for_scope(PROJECT_ID, THREAD_ID, call_id, path, bytes, reused)
}

fn write_result_for_scope(
    project_id: &str,
    thread_id: &str,
    call_id: &str,
    path: &str,
    bytes: u64,
    reused: bool,
) -> ToolResult {
    let checkpoint_ref = vega_tools::CheckpointIds::new(project_id, thread_id, call_id)
        .unwrap()
        .checkpoint_ref();
    ToolResult {
        status: ToolCallStatus::Success,
        output: vega_tools::WriteSuccessOutput {
            path: path.to_owned(),
            bytes_written: bytes,
            checkpoint_ref,
        }
        .to_json()
        .unwrap(),
        reused,
        exit_code: None,
        duration_ms: None,
        truncated: (!reused).then_some(false),
        invalid: None,
    }
}

fn failed_result() -> ToolResult {
    ToolResult {
        status: ToolCallStatus::Failed,
        output: "Tool error: write failed".to_owned(),
        reused: false,
        exit_code: None,
        duration_ms: None,
        truncated: None,
        invalid: None,
    }
}

fn rejected_or_cancelled_result(status: ToolCallStatus) -> ToolResult {
    let output = match status {
        ToolCallStatus::Rejected => "Tool error: permission denied",
        ToolCallStatus::Cancelled => vega_runtime::CANCELLED_BEFORE_EXECUTION_OUTPUT,
        _ => panic!("test helper accepts rejected/cancelled only"),
    };
    ToolResult {
        status,
        output: output.to_owned(),
        reused: false,
        exit_code: None,
        duration_ms: None,
        truncated: None,
        invalid: None,
    }
}

fn edit_result(call_id: &str, path: &str, bytes: u64) -> ToolResult {
    let checkpoint_ref = vega_tools::CheckpointIds::new(PROJECT_ID, THREAD_ID, call_id)
        .unwrap()
        .checkpoint_ref();
    ToolResult {
        status: ToolCallStatus::Success,
        output: vega_tools::EditSuccessOutput {
            path: path.to_owned(),
            bytes_written: bytes,
            replacements: 1,
            checkpoint_ref,
        }
        .to_json()
        .unwrap(),
        reused: false,
        exit_code: None,
        duration_ms: None,
        truncated: Some(false),
        invalid: None,
    }
}

async fn refreshed_workspace(repo: &Repo) -> Arc<GitWorkspaceService> {
    let workspace = Arc::new(GitWorkspaceService::new(repo.path()).unwrap());
    workspace.refresh(CancellationToken::new()).await.unwrap();
    workspace
}

async fn captured_text_artifact(
    repo: &Repo,
    route_epoch: u64,
) -> (Arc<GitWorkspaceService>, ArtifactService, ArtifactCard) {
    captured_artifact_at(repo, "artifact.txt", route_epoch).await
}

async fn captured_artifact_at(
    repo: &Repo,
    path: &str,
    route_epoch: u64,
) -> (Arc<GitWorkspaceService>, ArtifactService, ArtifactCard) {
    let workspace = refreshed_workspace(repo).await;
    let service = ArtifactService::new(
        workspace.clone(),
        PROJECT_ID.to_owned(),
        THREAD_ID.to_owned(),
        route_epoch,
    )
    .unwrap();
    let bytes = fs::metadata(repo.path().join(path)).unwrap().len();
    let call_id = "call-1";
    let card = service
        .capture(
            &write_call(call_id, path, bytes),
            &write_result(call_id, path, bytes, false),
            CancellationToken::new(),
        )
        .await
        .unwrap()
        .unwrap();
    (workspace, service, card)
}

#[derive(Clone)]
pub(crate) struct LaunchMock {
    calls: Arc<Mutex<Vec<Vec<Vec<u8>>>>>,
    outcome: Option<GitWorkspaceErrorCode>,
    expected: bool,
}

impl LaunchMock {
    fn new(outcome: Option<GitWorkspaceErrorCode>) -> Self {
        Self {
            calls: Arc::default(),
            outcome,
            expected: true,
        }
    }

    pub(super) fn unexpected() -> Self {
        Self {
            expected: false,
            ..Self::new(None)
        }
    }

    pub(super) fn execute(&self, command: &Command) -> Option<Result<(), GitWorkspaceError>> {
        assert!(self.expected, "unexpected external application launch");
        assert_eq!(command.get_program(), "/usr/bin/open");
        self.calls.lock().unwrap().push(
            command
                .get_args()
                .map(|arg| arg.as_bytes().to_vec())
                .collect(),
        );
        Some(
            self.outcome
                .map_or(Ok(()), |code| Err(workspace_error(code))),
        )
    }

    fn last_args(&self) -> Vec<Vec<u8>> {
        self.calls.lock().unwrap().last().unwrap().clone()
    }
}
