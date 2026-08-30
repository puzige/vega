use std::collections::VecDeque;
use std::error::Error;
use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};

use futures::future::BoxFuture;
use tempfile::tempdir;
use tokio_util::sync::CancellationToken;
use vega_conversation::agent::{PermissionHook, run_thread_task_with_permission_sink};
use vega_conversation::types::{
    Approval, ArtifactSource, BranchSwitchOutcome, CommitOutcome, CommitSelectionKind,
    ConversationEvent, DiffLayer, GitWorkspaceErrorCode, PermissionDecision, PermissionRequest,
    ToolCall, ToolCallStatus, ToolResult, WorkspaceChangeKind,
};
use vega_conversation::{
    ArtifactService, BranchWorkspaceService, GitWorkspaceService, TrustedGitService,
};
use vega_runtime::{MockProvider, ProviderEvent, ScriptStep, StopReason, VegaError};
use vega_store::{Store, projects, threads};
use vega_tools::Tools;

const THREAD_ID: &str = "s6-acceptance-thread";
const PROJECT_NAME: &str = "s6-acceptance";

#[derive(Clone)]
struct ScriptedPermissionHook {
    decisions: Arc<Mutex<VecDeque<PermissionDecision>>>,
}

impl ScriptedPermissionHook {
    fn once() -> Self {
        Self {
            decisions: Arc::new(Mutex::new(VecDeque::from([PermissionDecision::Once]))),
        }
    }
}

impl PermissionHook for ScriptedPermissionHook {
    fn request(
        &self,
        _request: PermissionRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<PermissionDecision, VegaError>> {
        let decision = self
            .decisions
            .lock()
            .ok()
            .and_then(|mut decisions| decisions.pop_front())
            .unwrap_or(PermissionDecision::Timeout);
        Box::pin(async move { Ok(decision) })
    }
}

fn git(repo: &Path, args: &[&str]) -> Result<Output, Box<dyn Error>> {
    let mut command = Command::new("/usr/bin/git");
    command.arg("-C").arg(repo).args(args);
    let explicit_git_keys: Vec<OsString> = command
        .get_envs()
        .filter(|(key, _)| key.as_bytes().starts_with(b"GIT_"))
        .map(|(key, _)| key.to_owned())
        .collect();
    for key in explicit_git_keys {
        command.env_remove(key);
    }
    for (key, _) in std::env::vars_os() {
        if key.as_os_str().as_bytes().starts_with(b"GIT_") {
            command.env_remove(key);
        }
    }
    let output = command
        .env("LC_ALL", "C")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_PAGER", "cat")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()?;
    if !output.status.success() {
        return Err(std::io::Error::other("fixture git command failed").into());
    }
    Ok(output)
}

fn init_repo(repo: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(repo.join("src"))?;
    git(repo, &["init", "-b", "main"])?;
    git(repo, &["config", "user.name", "Vega S6 Fixture"])?;
    git(
        repo,
        &["config", "user.email", "s6-fixture@example.invalid"],
    )?;
    fs::write(repo.join("README.md"), "# S6 fixture\n")?;
    fs::write(
        repo.join("src/original.rs"),
        "pub fn stable_one() {}\nfn original() {}\npub fn stable_two() {}\n",
    )?;
    git(repo, &["add", "--", "README.md", "src/original.rs"])?;
    git(repo, &["commit", "-m", "fixture: initial"])?;
    git(repo, &["branch", "topic"])?;
    Ok(())
}

async fn agent_tool_turn(
    store: &Store,
    tools: &Tools,
    call_id: &str,
    tool: &str,
    input_json: String,
) -> Result<(ToolCall, ToolResult), Box<dyn Error>> {
    let provider = MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: call_id.to_owned(),
                name: tool.to_owned(),
                input_json,
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("fixture turn complete".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ]);
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    let run = run_thread_task_with_permission_sink(
        store,
        &provider,
        tools,
        THREAD_ID,
        "apply the deterministic S6 fixture mutation",
        "Use only the requested tool call.",
        CancellationToken::new(),
        &ScriptedPermissionHook::once(),
        move |event| {
            sink.lock()
                .map_err(|_| VegaError::Io(std::io::Error::other("event sink poisoned")))?
                .push(event.clone());
            Ok(())
        },
    )
    .await?;
    assert!(!run.failed && !run.interrupted);
    let events = events
        .lock()
        .map_err(|_| std::io::Error::other("event sink poisoned"))?;
    let proposed = events
        .iter()
        .position(|event| matches!(event, ConversationEvent::ToolCallProposed { call } if call.id == call_id))
        .ok_or_else(|| std::io::Error::other("missing tool proposal"))?;
    let approved = events
        .iter()
        .position(|event| matches!(event, ConversationEvent::ToolCallApproved { call_id: approved_id, .. } if approved_id == call_id))
        .ok_or_else(|| std::io::Error::other("missing tool approval"))?;
    let finished = events
        .iter()
        .position(|event| matches!(event, ConversationEvent::ToolCallFinished { call_id: terminal_id, .. } if terminal_id == call_id))
        .ok_or_else(|| std::io::Error::other("missing tool terminal"))?;
    assert!(proposed < approved && approved < finished);
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, ConversationEvent::ToolCallProposed { call } if call.id == call_id))
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, ConversationEvent::ToolCallApproved { call_id: approved_id, .. } if approved_id == call_id))
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, ConversationEvent::ToolCallFinished { call_id: terminal_id, .. } if terminal_id == call_id))
            .count(),
        1
    );
    assert!(events.iter().any(|event| {
        matches!(event, ConversationEvent::ToolCallApproved { call_id: approved_id, approval: Approval::Once } if approved_id == call_id)
    }));
    let call = events
        .iter()
        .find_map(|event| match event {
            ConversationEvent::ToolCallProposed { call } if call.id == call_id => {
                Some(call.clone())
            }
            _ => None,
        })
        .ok_or_else(|| std::io::Error::other("missing tool proposal"))?;
    let result = events
        .iter()
        .find_map(|event| match event {
            ConversationEvent::ToolCallFinished {
                call_id: terminal_id,
                result,
            } if terminal_id == call_id => Some(result.clone()),
            _ => None,
        })
        .ok_or_else(|| std::io::Error::other("missing tool terminal"))?;
    assert_eq!(result.status, ToolCallStatus::Success);
    if tool == "bash" {
        assert_eq!(result.exit_code, Some(0));
        assert!(result.duration_ms.is_some());
    }
    Ok((call, result))
}

fn commit_error(code: vega_conversation::types::CommitErrorCode) -> std::io::Error {
    std::io::Error::other(code.as_str())
}

#[tokio::test]
async fn agent_diff_artifact_dirty_reject_and_two_stage_commit() -> Result<(), Box<dyn Error>> {
    let fixture = tempdir()?;
    let repo = fixture.path().join("repo");
    let state = fixture.path().join("state");
    fs::create_dir_all(&repo)?;
    fs::create_dir_all(&state)?;
    init_repo(&repo)?;
    let base = git(&repo, &["rev-parse", "HEAD"])?.stdout;

    let store = Store::open(state.join("vega.db"))?;
    store.migrate()?;
    let project = projects::create(
        store.conn(),
        repo.to_str()
            .ok_or_else(|| std::io::Error::other("fixture path is not UTF-8"))?,
        PROJECT_NAME,
        Some("main"),
    )?;
    threads::create(
        store.conn(),
        threads::NewThread {
            id: THREAD_ID,
            project_id: &project.id,
            title: "S6 acceptance",
            mode: "execute",
            permission_mode: "confirm",
            model: "mock-s6-acceptance",
            status: "active",
            pinned: false,
            unread: false,
            created_at: 1,
            updated_at: 1,
        },
    )?;
    let tools = Tools::new(&repo)?;
    let workspace = Arc::new(GitWorkspaceService::new(&repo)?);
    let artifacts = ArtifactService::new(
        Arc::clone(&workspace),
        project.id.clone(),
        THREAD_ID.into(),
        1,
    )?;

    let (edit_call, edit_result) = agent_tool_turn(
        &store,
        &tools,
        "s6-edit",
        "edit",
        r##"{"path":"src/original.rs","old_string":"fn original() {}","new_string":"fn accepted() {}"}"##.into(),
    )
    .await?;
    workspace.refresh(CancellationToken::new()).await?;
    let edit_card = artifacts
        .capture(&edit_call, &edit_result, CancellationToken::new())
        .await?
        .ok_or_else(|| std::io::Error::other("edit artifact missing"))?;
    assert_eq!(edit_card.label, "src/original.rs");
    assert_eq!(edit_card.source, ArtifactSource::AgentArtifact);
    assert!(edit_card.current_file_id.is_some());
    assert!(edit_card.preview_available);

    let (readme_call, readme_result) = agent_tool_turn(
        &store,
        &tools,
        "s6-readme-edit",
        "edit",
        r##"{"path":"README.md","old_string":"# S6 fixture","new_string":"# S6 accepted fixture"}"##.into(),
    )
    .await?;
    workspace.refresh(CancellationToken::new()).await?;
    let readme_card = artifacts
        .capture(&readme_call, &readme_result, CancellationToken::new())
        .await?
        .ok_or_else(|| std::io::Error::other("README artifact missing"))?;
    assert_eq!(readme_card.label, "README.md");
    assert_eq!(readme_card.source, ArtifactSource::AgentArtifact);
    assert!(readme_card.current_file_id.is_some());
    assert!(readme_card.preview_available);

    agent_tool_turn(
        &store,
        &tools,
        "s6-rename",
        "bash",
        r#"{"cmd":"mv -- src/original.rs src/renamed.rs"}"#.into(),
    )
    .await?;
    assert!(repo.join("src/renamed.rs").is_file());
    assert!(!repo.join("src/original.rs").exists());
    workspace.refresh(CancellationToken::new()).await?;
    let reconciled = artifacts.reconcile(CancellationToken::new()).await?;
    assert_eq!(reconciled.len(), 2);
    let stale_edit = reconciled
        .iter()
        .find(|card| card.id == edit_card.id)
        .ok_or_else(|| std::io::Error::other("stale edit card missing"))?;
    assert_eq!(stale_edit.label, "src/original.rs");
    assert_eq!(stale_edit.source, ArtifactSource::WorkspaceChange);
    assert_eq!(stale_edit.current_file_id, None);
    assert!(!stale_edit.preview_available);
    let live_readme = reconciled
        .iter()
        .find(|card| card.id == readme_card.id)
        .ok_or_else(|| std::io::Error::other("README card missing after reconcile"))?;
    assert_eq!(live_readme.label, "README.md");
    assert_eq!(live_readme.source, ArtifactSource::AgentArtifact);
    assert!(live_readme.current_file_id.is_some());
    assert!(live_readme.preview_available);

    let (write_call, write_result) = agent_tool_turn(
        &store,
        &tools,
        "s6-write",
        "write",
        r##"{"path":"artifact.md","content":"# Accepted artifact\n"}"##.into(),
    )
    .await?;
    let snapshot = workspace.refresh(CancellationToken::new()).await?;
    let write_card = artifacts
        .capture(&write_call, &write_result, CancellationToken::new())
        .await?
        .ok_or_else(|| std::io::Error::other("write artifact missing"))?;
    assert_eq!(write_card.label, "artifact.md");
    assert_eq!(write_card.source, ArtifactSource::AgentArtifact);
    let preview = artifacts
        .preview(write_card.id, CancellationToken::new())
        .await?;
    assert_eq!(preview.text(), "# Accepted artifact\n");

    let renamed = snapshot
        .files
        .iter()
        .find(|file| file.label == "src/renamed.rs")
        .ok_or_else(|| std::io::Error::other("renamed path row missing"))?;
    assert_eq!(renamed.unstaged, WorkspaceChangeKind::Untracked);
    let deleted = snapshot
        .files
        .iter()
        .find(|file| file.label == "src/original.rs")
        .ok_or_else(|| std::io::Error::other("deleted source row missing"))?;
    assert_eq!(deleted.unstaged, WorkspaceChangeKind::Deleted);
    let untracked = snapshot
        .files
        .iter()
        .find(|file| file.label == "artifact.md")
        .ok_or_else(|| std::io::Error::other("untracked row missing"))?;
    for file in [renamed, deleted, untracked] {
        let projection = workspace.diff(file.id, CancellationToken::new()).await?;
        assert!(!projection.sections().is_empty());
    }
    let untracked_projection = workspace
        .diff(untracked.id, CancellationToken::new())
        .await?;
    assert!(
        untracked_projection
            .sections()
            .iter()
            .any(|section| section.layer() == DiffLayer::Untracked)
    );

    let branch = BranchWorkspaceService::new(&repo)?;
    let dirty = branch
        .refresh(CancellationToken::new())
        .await
        .expect_err("dirty repository must reject branch listing");
    assert_eq!(dirty.code(), GitWorkspaceErrorCode::BranchDirty);
    assert_eq!(git(&repo, &["rev-parse", "HEAD"])?.stdout, base);

    let trusted = TrustedGitService::new(&repo, Arc::clone(&workspace)).map_err(commit_error)?;
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .map_err(commit_error)?;
    assert!(checklist.staged.is_empty());
    assert_eq!(checklist.optional.len(), 4);
    assert!(checklist.optional.iter().all(|row| !row.forced));
    for (label, kind) in [
        ("README.md", CommitSelectionKind::Modified),
        ("artifact.md", CommitSelectionKind::Added),
        ("src/original.rs", CommitSelectionKind::Deleted),
        ("src/renamed.rs", CommitSelectionKind::Added),
    ] {
        assert_eq!(
            checklist
                .optional
                .iter()
                .filter(|row| row.label == label && row.kind == kind)
                .count(),
            1
        );
    }
    let selected = checklist
        .optional
        .iter()
        .map(|row| row.file_id)
        .collect::<Vec<_>>();
    let prepared = trusted
        .prepare(checklist.id, selected, CancellationToken::new())
        .await;
    assert_eq!(prepared.error, None);
    let prepared = prepared
        .prepared
        .ok_or_else(|| std::io::Error::other("prepared authority missing"))?;
    assert_eq!(git(&repo, &["rev-parse", "HEAD"])?.stdout, base);

    let provider = Arc::new(MockProvider::new(vec![
        ScriptStep::text("feat: generated S6 message"),
        ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }]),
    ]));
    let draft = trusted
        .draft(
            prepared.id,
            "mock-s6-draft".into(),
            provider.clone(),
            CancellationToken::new(),
        )
        .await
        .map_err(commit_error)?;
    assert_eq!(provider.requests().len(), 1);
    assert!(provider.requests()[0].tools.is_empty());
    let edited_message = format!("{}\n\nReviewed-by: S6 acceptance", draft.text());
    let completion = trusted
        .commit(
            prepared.id,
            edited_message.clone(),
            CancellationToken::new(),
        )
        .await;
    assert_eq!(completion.outcome, CommitOutcome::Committed);
    assert!(completion.workspace.is_some());

    let head = git(&repo, &["rev-parse", "HEAD"])?.stdout;
    assert_ne!(head, base);
    let parents = git(&repo, &["rev-list", "--parents", "-n", "1", "HEAD"])?.stdout;
    let parent_fields = parents
        .strip_suffix(b"\n")
        .unwrap_or(&parents)
        .split(|byte| *byte == b' ')
        .collect::<Vec<_>>();
    assert_eq!(parent_fields.len(), 2);
    assert_eq!(parent_fields[1], base.strip_suffix(b"\n").unwrap_or(&base));
    assert_eq!(git(&repo, &["status", "--porcelain=v2", "-z"])?.stdout, b"");
    assert_eq!(
        git(&repo, &["symbolic-ref", "--short", "HEAD"])?.stdout,
        b"main\n"
    );
    assert_eq!(
        String::from_utf8(git(&repo, &["log", "-1", "--pretty=%B"])?.stdout)?.trim_end(),
        edited_message
    );
    let tree = git(&repo, &["ls-tree", "-rz", "--name-only", "HEAD"])?.stdout;
    assert_eq!(tree, b"README.md\0artifact.md\0src/renamed.rs\0");
    assert_eq!(
        fs::read(repo.join("README.md"))?,
        b"# S6 accepted fixture\n"
    );
    assert_eq!(
        fs::read(repo.join("artifact.md"))?,
        b"# Accepted artifact\n"
    );
    assert_eq!(
        fs::read(repo.join("src/renamed.rs"))?,
        b"pub fn stable_one() {}\nfn accepted() {}\npub fn stable_two() {}\n"
    );
    assert!(git(&repo, &["remote"])?.stdout.is_empty());
    Ok(())
}

#[tokio::test]
async fn clean_fixture_branch_switches_authoritatively() -> Result<(), Box<dyn Error>> {
    let fixture = tempdir()?;
    let repo = fixture.path().join("repo");
    fs::create_dir_all(&repo)?;
    init_repo(&repo)?;
    git(&repo, &["switch", "topic"])?;
    fs::write(repo.join("topic.txt"), "topic tree\n")?;
    git(&repo, &["add", "--", "topic.txt"])?;
    git(&repo, &["commit", "-m", "fixture: topic"])?;
    git(&repo, &["switch", "main"])?;

    let service = BranchWorkspaceService::new(&repo)?;
    let snapshot = service.refresh(CancellationToken::new()).await?;
    assert_eq!(
        snapshot
            .branches
            .iter()
            .filter(|branch| branch.current)
            .count(),
        1
    );
    let target = snapshot
        .branches
        .iter()
        .find(|branch| branch.label == "topic")
        .ok_or_else(|| std::io::Error::other("topic branch missing"))?;
    let before = git(&repo, &["rev-parse", "HEAD"])?.stdout;
    let permit = service
        .prepare_switch(target.id, CancellationToken::new())
        .await?;
    assert_eq!(git(&repo, &["rev-parse", "HEAD"])?.stdout, before);
    let completion = service
        .execute_switch(permit, CancellationToken::new())
        .await;
    assert_eq!(completion.outcome, BranchSwitchOutcome::Switched);
    let refreshed = completion
        .snapshot
        .ok_or_else(|| std::io::Error::other("authoritative branch snapshot missing"))?;
    assert_eq!(
        refreshed
            .branches
            .iter()
            .filter(|branch| branch.current && branch.label == "topic")
            .count(),
        1
    );
    assert_eq!(
        git(&repo, &["symbolic-ref", "--short", "HEAD"])?.stdout,
        b"topic\n"
    );
    assert_eq!(fs::read_to_string(repo.join("topic.txt"))?, "topic tree\n");
    assert!(
        git(&repo, &["status", "--porcelain=v2", "-z"])?
            .stdout
            .is_empty()
    );
    assert!(git(&repo, &["remote"])?.stdout.is_empty());
    Ok(())
}
