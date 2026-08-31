use super::*;
use std::fs;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::PermissionsExt;

use tempfile::TempDir;

const PROJECT_ID: &str = "project";
const THREAD_ID: &str = "thread";

mod caps_retained;
mod capture_reconcile;
mod preview_open;

struct Repo {
    dir: TempDir,
}

impl Repo {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
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
    let mut command = Command::new("/usr/bin/git");
    command
        .current_dir(root)
        .args(args)
        .env("GIT_DIR", root.join(".poison-git-dir"))
        .env("GIT_WORK_TREE", root.join(".poison-work-tree"))
        .env("GIT_INDEX_FILE", root.join(".poison-index"));
    scrub_all_git_environment(&mut command);
    let status = command.status().unwrap();
    assert!(status.success(), "git {args:?}");
    for poison in [".poison-git-dir", ".poison-work-tree", ".poison-index"] {
        assert!(!root.join(poison).exists(), "poison target {poison}");
    }
}

fn scrub_all_git_environment(command: &mut Command) {
    let explicit = command
        .get_envs()
        .filter(|(key, _)| key.as_bytes().starts_with(b"GIT_"))
        .map(|(key, _)| key.to_owned())
        .collect::<Vec<_>>();
    for key in explicit {
        command.env_remove(key);
    }
    for (key, _) in std::env::vars_os() {
        if key.as_os_str().as_bytes().starts_with(b"GIT_") {
            command.env_remove(key);
        }
    }
    command
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C");
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

fn launcher_script(root: &Path, body: &str) -> PathBuf {
    let script = root.join("fake-open");
    fs::write(&script, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    script
}

fn raw_argv(path: &Path) -> Vec<Vec<u8>> {
    let bytes = fs::read(path).unwrap();
    let payload = bytes.strip_suffix(&[0]).unwrap_or(&bytes);
    payload
        .split(|byte| *byte == 0)
        .map(<[u8]>::to_vec)
        .collect()
}

fn pid_is_alive(pid: u32) -> bool {
    for _ in 0..100 {
        let alive = Command::new("/bin/kill")
            .args(["-0", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        if !alive {
            return false;
        }
        thread::sleep(Duration::from_millis(5));
    }
    true
}
