use tempfile::TempDir;
use vega_store::{Store, codex_tasks as persisted, projects};

use super::{
    begin_codex_session_creation, bind_codex_task, codex_prompt_binding, codex_task_identity,
    confirm_codex_session_creation, materialize_codex_draft,
};
use crate::threads;
use crate::types::{
    CodexAdapterArgument, CodexAdapterKind, CodexApprovalPolicy, CodexExecutionSnapshot,
    CodexProfileReference, CodexReasoningEffort, CodexRunSettings, CodexSandboxMode,
    CodexSessionCreationState, CodexTaskIdentity, CodexWorkspaceSnapshot, PermissionMode,
    TaskBackend, Thread,
};

#[derive(Clone, Copy, Debug)]
enum CreationRoute {
    Materialize,
    Bind,
}

const CREATION_ROUTES: [CreationRoute; 2] = [CreationRoute::Materialize, CreationRoute::Bind];

struct Fixture {
    directory: TempDir,
    store: Store,
    project: Option<projects::Project>,
    snapshot: CodexExecutionSnapshot,
}

fn fixture(with_project: bool) -> Fixture {
    let directory = tempfile::tempdir().expect("owned workspace fixture");
    let root = directory
        .path()
        .canonicalize()
        .expect("canonical fixture root");
    let working_directory = root.join("existing-worktree");
    let additional_directory = root.join("additional");
    std::fs::create_dir(&working_directory).expect("create owned existing working directory");
    std::fs::create_dir(&additional_directory).expect("create owned additional directory");
    std::fs::write(
        working_directory.join("keep.txt"),
        b"preserved worktree history",
    )
    .expect("create owned preservation marker");
    let working_directory = working_directory
        .canonicalize()
        .expect("canonical working directory");
    let additional_directory = additional_directory
        .canonicalize()
        .expect("canonical additional directory");
    let store = Store::open(":memory:").expect("owned memory store");
    store.migrate().expect("initialize schema");
    let project = with_project.then(|| {
        projects::create(
            store.conn(),
            root.to_str().expect("UTF-8 project path"),
            "Worktree project",
            None,
        )
        .expect("register live project")
    });
    let snapshot = CodexExecutionSnapshot {
        profile: CodexProfileReference {
            id: "worktree-profile".into(),
            display_name: "Worktree profile".into(),
        },
        adapter: CodexAdapterKind::CodexAcp,
        executable: root
            .join("adapter-never-executed")
            .to_string_lossy()
            .into_owned(),
        arguments: CodexAdapterArgument::from_argv(&[
            "--model".into(),
            "fixture-model".into(),
            "--fixture".into(),
        ])
        .expect("safe public argv constructor"),
        adapter_version: "1.2.3".into(),
        codex_version: "4.5.6".into(),
        settings: CodexRunSettings {
            model: Some("fixture-model".into()),
            model_provider: Some("fixture-provider".into()),
            reasoning_effort: Some(CodexReasoningEffort::High),
            sandbox_mode: CodexSandboxMode::ReadOnly,
            approval_policy: CodexApprovalPolicy::OnRequest,
        },
        workspace: CodexWorkspaceSnapshot {
            project_id: project.as_ref().map(|project| project.id.clone()),
            worktree_id: Some("existing-worktree".into()),
            canonical_working_directory: working_directory.to_string_lossy().into_owned(),
            additional_directories: vec![additional_directory.to_string_lossy().into_owned()],
        },
    };
    Fixture {
        directory,
        store,
        project,
        snapshot,
    }
}

fn identity(store: &Store, thread_id: &str) -> CodexTaskIdentity {
    codex_task_identity(store, thread_id)
        .expect("read valid Codex identity")
        .expect("persisted Codex identity")
}

fn create_task(fixture: &Fixture, route: CreationRoute) -> Thread {
    let project_id = fixture.snapshot.workspace.project_id.as_deref();
    let thread = match route {
        CreationRoute::Materialize => {
            let mut draft = threads::draft_thread(project_id, "fixture-model", "readonly");
            assert_eq!(draft.backend, TaskBackend::Native);
            assert_eq!(draft.project_binding(), project_id);
            draft.backend = TaskBackend::Codex;
            draft.title = "Preserved worktree task".into();
            materialize_codex_draft(&fixture.store, &draft, &fixture.snapshot)
                .expect("materialize project or worktree-only Codex draft")
        }
        CreationRoute::Bind => {
            let native = match project_id {
                Some(project_id) => {
                    threads::create_thread(&fixture.store, project_id, "fixture-model", "readonly")
                }
                None => {
                    threads::create_standalone_thread(&fixture.store, "fixture-model", "readonly")
                }
            }
            .expect("create Native task through real project or standalone API");
            assert_eq!(native.backend, TaskBackend::Native);
            let bound = bind_codex_task(&fixture.store, &native.id, &fixture.snapshot)
                .expect("bind existing Native task to Codex");
            assert_eq!(bound.backend, TaskBackend::Codex);
            assert_eq!(bound.snapshot, fixture.snapshot);
            threads::get_thread(&fixture.store, &native.id).expect("reload bound task")
        }
    };
    assert_eq!(thread.backend, TaskBackend::Codex);
    assert_eq!(thread.project_binding(), project_id);
    assert_eq!(thread.permission_mode, PermissionMode::ReadOnly);
    let stored = identity(&fixture.store, &thread.id);
    assert_eq!(stored.thread_id, thread.id);
    assert_eq!(stored.backend, TaskBackend::Codex);
    assert_eq!(stored.snapshot, fixture.snapshot);
    assert_eq!(stored.session_creation, CodexSessionCreationState::Absent);
    thread
}

fn confirm_task(fixture: &Fixture, thread_id: &str) -> String {
    assert_eq!(
        codex_prompt_binding(&fixture.store, thread_id).expect("absent session prompt observation"),
        None
    );
    let CodexSessionCreationState::Intent { intent_id } =
        begin_codex_session_creation(&fixture.store, thread_id)
            .expect("begin live project or worktree-only session")
    else {
        panic!("session must enter intent before confirmation");
    };
    assert_eq!(
        identity(&fixture.store, thread_id).session_creation,
        CodexSessionCreationState::Intent {
            intent_id: intent_id.clone(),
        }
    );
    assert_eq!(
        codex_prompt_binding(&fixture.store, thread_id).expect("intent session prompt observation"),
        None
    );
    let session_id = format!("session-{thread_id}");
    confirm_codex_session_creation(&fixture.store, thread_id, &intent_id, &session_id)
        .expect("confirm valid session");
    assert_eq!(
        identity(&fixture.store, thread_id),
        CodexTaskIdentity {
            thread_id: thread_id.to_string(),
            backend: TaskBackend::Codex,
            snapshot: fixture.snapshot.clone(),
            session_creation: CodexSessionCreationState::Confirmed {
                intent_id,
                session_id: session_id.clone(),
            },
        }
    );
    let binding = codex_prompt_binding(&fixture.store, thread_id)
        .expect("live confirmed prompt binding")
        .expect("live project or worktree-only task is prompt eligible");
    assert_eq!(binding.thread_id, thread_id);
    assert_eq!(binding.session_id, session_id);
    assert_eq!(binding.snapshot, fixture.snapshot);
    session_id
}

fn assert_preserved_and_enforced(fixture: &Fixture) {
    assert!(fixture.directory.path().is_dir());
    assert_eq!(
        std::fs::read(
            std::path::Path::new(&fixture.snapshot.workspace.canonical_working_directory)
                .join("keep.txt"),
        )
        .expect("worktree preservation marker"),
        b"preserved worktree history"
    );
    assert_eq!(
        fixture
            .store
            .conn()
            .query_row("PRAGMA foreign_keys", [], |row| row.get::<_, i64>(0))
            .expect("foreign-key enforcement"),
        1
    );
    assert_eq!(
        fixture
            .store
            .conn()
            .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("foreign-key integrity"),
        0
    );
    assert!(fixture.store.conn().is_autocommit());
}

#[test]
fn worktree_only_materialize_and_bind_preserve_identity_and_full_session_prompt_flow() {
    for route in CREATION_ROUTES {
        let fixture = fixture(false);
        let thread = create_task(&fixture, route);
        assert!(fixture.project.is_none());
        assert!(fixture.snapshot.workspace.project_id.is_none());
        assert_eq!(
            fixture.snapshot.workspace.worktree_id.as_deref(),
            Some("existing-worktree")
        );
        confirm_task(&fixture, &thread.id);
        assert_eq!(
            threads::get_thread(&fixture.store, &thread.id).expect("unchanged worktree-only task"),
            thread
        );
        assert_preserved_and_enforced(&fixture);
    }
}

#[test]
fn live_project_with_worktree_materialize_and_bind_remain_prompt_eligible() {
    for route in CREATION_ROUTES {
        let fixture = fixture(true);
        let thread = create_task(&fixture, route);
        let project = fixture.project.as_ref().expect("registered project");
        assert_eq!(thread.project_binding(), Some(project.id.as_str()));
        confirm_task(&fixture, &thread.id);
        assert_eq!(
            threads::get_thread(&fixture.store, &thread.id).expect("unchanged live-project task"),
            thread
        );
        assert_preserved_and_enforced(&fixture);
    }
}

#[test]
fn removed_project_with_worktree_keeps_absent_and_confirmed_history_without_execution_authority() {
    for route in CREATION_ROUTES {
        for confirmed in [false, true] {
            let fixture = fixture(true);
            let thread = create_task(&fixture, route);
            if confirmed {
                confirm_task(&fixture, &thread.id);
            }
            let before = identity(&fixture.store, &thread.id);
            let raw_snapshot = persisted::find_task_snapshot(fixture.store.conn(), &thread.id)
                .expect("original raw snapshot");
            let raw_session = persisted::find_session_creation(fixture.store.conn(), &thread.id)
                .expect("original raw session including timestamp");
            let project = fixture.project.as_ref().expect("registered project");
            assert!(
                projects::remove(fixture.store.conn(), &project.id)
                    .expect("unregister project with worktree history")
            );
            let detached = threads::get_thread(&fixture.store, &thread.id)
                .expect("detached task remains readable");
            assert!(detached.project_binding().is_none());
            assert_eq!(detached.backend, TaskBackend::Codex);
            assert_eq!(detached.permission_mode, PermissionMode::ReadOnly);
            assert_eq!(identity(&fixture.store, &thread.id), before);

            assert!(
                begin_codex_session_creation(&fixture.store, &thread.id).is_err(),
                "detached worktree must not authorize a new intent: {route:?}, confirmed={confirmed}"
            );
            assert!(
                !matches!(
                    codex_prompt_binding(&fixture.store, &thread.id),
                    Ok(Some(_))
                ),
                "detached worktree must not yield an executable prompt: {route:?}, confirmed={confirmed}"
            );
            assert_eq!(identity(&fixture.store, &thread.id), before);
            assert_eq!(
                persisted::find_task_snapshot(fixture.store.conn(), &thread.id)
                    .expect("historical snapshot after denied execution"),
                raw_snapshot
            );
            assert_eq!(
                persisted::find_session_creation(fixture.store.conn(), &thread.id)
                    .expect("historical session after denied execution"),
                raw_session
            );
            assert_preserved_and_enforced(&fixture);
        }
    }
}

#[test]
fn reregistering_same_path_cannot_start_absent_project_worktree_history_or_change_its_session() {
    for route in CREATION_ROUTES {
        let fixture = fixture(true);
        let thread = create_task(&fixture, route);
        let before = identity(&fixture.store, &thread.id);
        assert_eq!(before.session_creation, CodexSessionCreationState::Absent);
        let raw_snapshot = persisted::find_task_snapshot(fixture.store.conn(), &thread.id)
            .expect("original absent-history snapshot");
        let raw_session = persisted::find_session_creation(fixture.store.conn(), &thread.id)
            .expect("original absent-history session");
        let old_project = fixture.project.as_ref().expect("old project registration");
        assert!(
            projects::remove(fixture.store.conn(), &old_project.id)
                .expect("remove original registration")
        );
        let replacement = projects::create(
            fixture.store.conn(),
            &old_project.path,
            "Replacement registration",
            None,
        )
        .expect("register the exact same owned directory with a new identity");
        assert_ne!(replacement.id, old_project.id);
        assert_eq!(replacement.path, old_project.path);
        assert!(
            projects::find(fixture.store.conn(), &old_project.id)
                .expect("old project observation")
                .is_none()
        );
        assert!(
            threads::get_thread(&fixture.store, &thread.id)
                .expect("old task remains detached")
                .project_binding()
                .is_none()
        );
        assert_eq!(identity(&fixture.store, &thread.id), before);

        assert!(
            begin_codex_session_creation(&fixture.store, &thread.id).is_err(),
            "same-path registration must not authorize absent historical session: {route:?}"
        );
        assert!(!matches!(
            codex_prompt_binding(&fixture.store, &thread.id),
            Ok(Some(_))
        ));
        assert_eq!(identity(&fixture.store, &thread.id), before);
        assert_eq!(
            persisted::find_task_snapshot(fixture.store.conn(), &thread.id)
                .expect("snapshot after same-path execution denial"),
            raw_snapshot
        );
        assert_eq!(
            persisted::find_session_creation(fixture.store.conn(), &thread.id)
                .expect("session after same-path execution denial"),
            raw_session
        );
        assert_preserved_and_enforced(&fixture);
    }
}
