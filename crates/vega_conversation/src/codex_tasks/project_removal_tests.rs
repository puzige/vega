use tempfile::TempDir;
use vega_store::{codex_tasks as persisted, projects, threads as persisted_threads};

use super::*;
use crate::threads;
use crate::types::{CodexProfileReference, PermissionMode};

const SESSION_STATES: [&str; 5] = [
    "absent",
    "intent",
    "confirmed",
    "uncertain",
    "definitively_failed",
];

struct Fixture {
    directory: TempDir,
    store: Store,
    project: projects::Project,
    snapshot: CodexExecutionSnapshot,
}

fn fixture() -> Fixture {
    let directory = tempfile::tempdir().expect("owned project directory");
    let path = directory
        .path()
        .canonicalize()
        .expect("canonical project directory");
    let extra = path.join("additional");
    std::fs::create_dir(&extra).expect("owned additional directory");
    std::fs::write(path.join("keep.txt"), b"preserved local content").expect("owned local file");
    let store = Store::open(":memory:").expect("owned store");
    store.migrate().expect("initialize schema");
    let project = projects::create(
        store.conn(),
        path.to_str().expect("UTF-8 fixture path"),
        "Historical project",
        None,
    )
    .expect("register project");
    let snapshot = CodexExecutionSnapshot {
        profile: CodexProfileReference {
            id: "fixture-profile".into(),
            display_name: "Fixture profile".into(),
        },
        adapter: CodexAdapterKind::CodexAcp,
        executable: path
            .join("adapter-never-executed")
            .to_string_lossy()
            .into_owned(),
        arguments: CodexAdapterArgument::from_argv(&[
            "--model".into(),
            "fixture-model".into(),
            "--fixture".into(),
        ])
        .expect("adapter arguments"),
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
            project_id: Some(project.id.clone()),
            worktree_id: None,
            canonical_working_directory: path.to_string_lossy().into_owned(),
            additional_directories: vec![extra.to_string_lossy().into_owned()],
        },
    };
    Fixture {
        directory,
        store,
        project,
        snapshot,
    }
}

fn create_task(fixture: &Fixture, state: &str, bind_existing: bool) -> Thread {
    let thread = if bind_existing {
        let thread = threads::create_thread(
            &fixture.store,
            &fixture.project.id,
            "fixture-model",
            "readonly",
        )
        .expect("create Native task");
        bind_codex_task(&fixture.store, &thread.id, &fixture.snapshot)
            .expect("bind Codex identity");
        let mut thread = thread;
        thread.backend = TaskBackend::Codex;
        thread
    } else {
        let mut draft =
            threads::draft_thread(Some(&fixture.project.id), "fixture-model", "readonly");
        draft.backend = TaskBackend::Codex;
        draft.title = format!("Preserved {state} history");
        materialize_codex_draft(&fixture.store, &draft, &fixture.snapshot)
            .expect("materialize Codex draft")
    };
    if state != "absent" {
        let CodexSessionCreationState::Intent { intent_id } =
            begin_codex_session_creation(&fixture.store, &thread.id)
                .expect("begin live-project session")
        else {
            panic!("session starts in intent");
        };
        match state {
            "intent" => {}
            "confirmed" => confirm_codex_session_creation(
                &fixture.store,
                &thread.id,
                &intent_id,
                &format!("session-{}", thread.id),
            )
            .expect("confirm session"),
            "uncertain" => mark_codex_session_uncertain(
                &fixture.store,
                &thread.id,
                &intent_id,
                CodexSessionUncertaintyCode::OutcomeUnknown,
            )
            .expect("mark uncertain"),
            "definitively_failed" => mark_codex_session_definitively_failed(
                &fixture.store,
                &thread.id,
                &intent_id,
                CodexSessionFailureCode::AuthenticationRequired,
            )
            .expect("mark failed"),
            _ => panic!("known session state"),
        }
    }
    thread
}

fn identity(store: &Store, id: &str) -> CodexTaskIdentity {
    codex_task_identity(store, id)
        .expect("read valid historical identity")
        .expect("Codex identity")
}

fn remove(fixture: &Fixture) {
    assert!(
        projects::remove(fixture.store.conn(), &fixture.project.id)
            .expect("remove project registration")
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
}

#[test]
fn removed_project_history_reads_original_identity_and_every_session_state() {
    let fixture = fixture();
    let tasks: Vec<_> = SESSION_STATES
        .iter()
        .enumerate()
        .map(|(index, state)| {
            let thread = create_task(&fixture, state, index % 2 == 0);
            let expected = identity(&fixture.store, &thread.id);
            let snapshot = persisted::find_task_snapshot(fixture.store.conn(), &thread.id)
                .expect("snapshot before removal");
            let session = persisted::find_session_creation(fixture.store.conn(), &thread.id)
                .expect("session before removal");
            (thread, expected, snapshot, session)
        })
        .collect();
    remove(&fixture);
    for (thread, expected, snapshot, session) in tasks {
        assert_eq!(identity(&fixture.store, &thread.id), expected);
        assert_eq!(
            persisted::find_task_snapshot(fixture.store.conn(), &thread.id)
                .expect("preserved snapshot"),
            snapshot
        );
        assert_eq!(
            persisted::find_session_creation(fixture.store.conn(), &thread.id)
                .expect("preserved session"),
            session
        );
        assert_eq!(
            persisted_threads::find(fixture.store.conn(), &thread.id)
                .expect("preserved thread")
                .expect("thread")
                .project_id,
            ""
        );
    }
    assert_eq!(
        std::fs::read(fixture.directory.path().join("keep.txt")).expect("local file remains"),
        b"preserved local content"
    );
}

#[test]
fn removed_project_cannot_begin_session_or_produce_prompt_binding() {
    let fixture = fixture();
    let tasks: Vec<_> = SESSION_STATES
        .iter()
        .map(|state| create_task(&fixture, state, false))
        .collect();
    remove(&fixture);
    for thread in tasks {
        let expected = identity(&fixture.store, &thread.id);
        let before = persisted::find_session_creation(fixture.store.conn(), &thread.id)
            .expect("session before denied execution");
        assert!(
            begin_codex_session_creation(&fixture.store, &thread.id).is_err(),
            "historical identity must not authorize session creation"
        );
        assert!(
            matches!(
                codex_prompt_binding(&fixture.store, &thread.id),
                Ok(None)
                    | Err(ConversationError::NoProject | ConversationError::InvalidTaskIdentity)
            ),
            "historical identity must not produce an executable binding or be mistaken for corruption"
        );
        assert_eq!(
            persisted::find_session_creation(fixture.store.conn(), &thread.id)
                .expect("session after denied execution"),
            before
        );
        assert_eq!(identity(&fixture.store, &thread.id), expected);
    }
}

#[test]
fn reregistering_same_directory_does_not_reattach_or_reauthorize_history() {
    let fixture = fixture();
    let thread = create_task(&fixture, "confirmed", false);
    let before = identity(&fixture.store, &thread.id);
    remove(&fixture);
    let replacement = projects::create(
        fixture.store.conn(),
        &fixture.project.path,
        "Replacement project",
        None,
    )
    .expect("register same path again");
    assert_ne!(replacement.id, fixture.project.id);
    assert_eq!(identity(&fixture.store, &thread.id), before);
    assert!(
        threads::list_threads(&fixture.store, &replacement.id, None)
            .expect("new project task list")
            .is_empty()
    );
    assert!(
        threads::list_standalone_threads(&fixture.store, None)
            .expect("standalone history")
            .iter()
            .any(|task| task.id == thread.id)
    );
    assert!(begin_codex_session_creation(&fixture.store, &thread.id).is_err());
    assert!(matches!(
        codex_prompt_binding(&fixture.store, &thread.id),
        Ok(None) | Err(ConversationError::NoProject | ConversationError::InvalidTaskIdentity)
    ));
    let mut stale = threads::draft_thread(Some(&fixture.project.id), "fixture-model", "readonly");
    stale.backend = TaskBackend::Codex;
    assert!(matches!(
        materialize_codex_draft(&fixture.store, &stale, &fixture.snapshot),
        Err(ConversationError::NoProject)
    ));
    let native =
        threads::create_thread(&fixture.store, &replacement.id, "fixture-model", "readonly")
            .expect("new replacement task");
    assert!(bind_codex_task(&fixture.store, &native.id, &fixture.snapshot).is_err());
    assert_eq!(
        codex_task_identity(&fixture.store, &native.id).expect("Native identity remains"),
        None
    );
}

#[test]
fn live_project_session_flow_and_permissions_remain_unchanged() {
    let fixture = fixture();
    for bind_existing in [false, true] {
        let task = create_task(&fixture, "absent", bind_existing);
        let initial = identity(&fixture.store, &task.id);
        assert_eq!(initial.snapshot, fixture.snapshot);
        assert_eq!(initial.session_creation, CodexSessionCreationState::Absent);
        assert_eq!(task.permission_mode, PermissionMode::ReadOnly);
        assert_eq!(
            codex_prompt_binding(&fixture.store, &task.id).expect("no session yet"),
            None
        );
        let CodexSessionCreationState::Intent { intent_id } =
            begin_codex_session_creation(&fixture.store, &task.id).expect("begin session")
        else {
            panic!("intent state");
        };
        assert!(matches!(
            begin_codex_session_creation(&fixture.store, &task.id),
            Err(ConversationError::InvalidCodexSessionTransition)
        ));
        assert_eq!(
            codex_prompt_binding(&fixture.store, &task.id).expect("intent not prompt eligible"),
            None
        );
        confirm_codex_session_creation(
            &fixture.store,
            &task.id,
            &intent_id,
            &format!("session-{}", task.id),
        )
        .expect("confirm live session");
        let binding = codex_prompt_binding(&fixture.store, &task.id)
            .expect("prompt binding")
            .expect("eligible session");
        assert_eq!(binding.thread_id, task.id);
        assert_eq!(binding.session_id, format!("session-{}", task.id));
        assert_eq!(binding.snapshot, fixture.snapshot);
    }
}

#[test]
fn uncertain_and_failed_sessions_cannot_retry_or_become_prompt_eligible() {
    let fixture = fixture();
    for state in ["uncertain", "definitively_failed"] {
        let task = create_task(&fixture, state, false);
        let expected = identity(&fixture.store, &task.id);
        assert!(matches!(
            begin_codex_session_creation(&fixture.store, &task.id),
            Err(ConversationError::InvalidCodexSessionTransition)
        ));
        assert_eq!(
            codex_prompt_binding(&fixture.store, &task.id).expect("terminal non-executable state"),
            None
        );
        assert_eq!(identity(&fixture.store, &task.id), expected);
    }
}

#[test]
fn identity_rejects_unbound_task_while_original_project_still_exists() {
    let fixture = fixture();
    let task = create_task(&fixture, "absent", false);
    fixture
        .store
        .conn()
        .execute(
            "UPDATE threads SET project_id = NULL WHERE id = ?1",
            [&task.id],
        )
        .expect("inject corrupt unbinding");
    assert!(matches!(
        codex_task_identity(&fixture.store, &task.id),
        Err(ConversationError::CorruptRow(_))
    ));
    let before = persisted::find_session_creation(fixture.store.conn(), &task.id)
        .expect("session before rejected identity");
    assert!(begin_codex_session_creation(&fixture.store, &task.id).is_err());
    assert!(codex_prompt_binding(&fixture.store, &task.id).is_err());
    assert_eq!(
        persisted::find_session_creation(fixture.store.conn(), &task.id)
            .expect("unchanged session"),
        before
    );
}

#[test]
fn identity_rejects_other_project_binding_even_after_original_removal() {
    let fixture = fixture();
    let task = create_task(&fixture, "confirmed", false);
    let other = projects::create(fixture.store.conn(), "/synthetic/other", "Other", None)
        .expect("other project");
    fixture
        .store
        .conn()
        .execute(
            "UPDATE threads SET project_id = ?1 WHERE id = ?2",
            [&other.id, &task.id],
        )
        .expect("inject wrong current project");
    assert!(matches!(
        codex_task_identity(&fixture.store, &task.id),
        Err(ConversationError::CorruptRow(_))
    ));
    remove(&fixture);
    assert!(matches!(
        codex_task_identity(&fixture.store, &task.id),
        Err(ConversationError::CorruptRow(_))
    ));
    assert!(codex_prompt_binding(&fixture.store, &task.id).is_err());
}

#[test]
fn identity_rejects_native_task_with_codex_snapshot() {
    let fixture = fixture();
    let task = create_task(&fixture, "absent", false);
    fixture
        .store
        .conn()
        .execute_batch("DROP TRIGGER thread_codex_backend_immutable")
        .expect("allow corruption fixture");
    fixture
        .store
        .conn()
        .execute(
            "UPDATE threads SET backend = 'native' WHERE id = ?1",
            [&task.id],
        )
        .expect("inject Native backend corruption");
    assert!(matches!(
        codex_task_identity(&fixture.store, &task.id),
        Err(ConversationError::CorruptRow(_))
    ));
    assert!(begin_codex_session_creation(&fixture.store, &task.id).is_err());
}

#[test]
fn identity_rejects_codex_task_without_snapshot() {
    let fixture = fixture();
    let task = create_task(&fixture, "absent", false);
    fixture
        .store
        .conn()
        .execute(
            "DELETE FROM codex_task_snapshots WHERE thread_id = ?1",
            [&task.id],
        )
        .expect("inject missing snapshot");
    assert!(matches!(
        codex_task_identity(&fixture.store, &task.id),
        Err(ConversationError::CorruptRow(_))
    ));
    assert!(codex_prompt_binding(&fixture.store, &task.id).is_err());
}

#[test]
fn new_codex_identity_requires_current_project_and_exact_snapshot_binding() {
    let fixture = fixture();
    let native = threads::create_thread(
        &fixture.store,
        &fixture.project.id,
        "fixture-model",
        "readonly",
    )
    .expect("Native task");
    let mut wrong = fixture.snapshot.clone();
    wrong.workspace.project_id = Some("missing-project".into());
    assert!(matches!(
        bind_codex_task(&fixture.store, &native.id, &wrong),
        Err(ConversationError::InvalidTaskIdentity)
    ));
    assert_eq!(
        codex_task_identity(&fixture.store, &native.id).expect("Native task remains unchanged"),
        None
    );
    let mut draft = threads::draft_thread(Some("missing-project"), "fixture-model", "readonly");
    draft.backend = TaskBackend::Codex;
    assert!(matches!(
        materialize_codex_draft(&fixture.store, &draft, &wrong),
        Err(ConversationError::NoProject)
    ));
    assert!(
        persisted_threads::find(fixture.store.conn(), &draft.id)
            .expect("missing draft remains unmaterialized")
            .is_none()
    );
}
