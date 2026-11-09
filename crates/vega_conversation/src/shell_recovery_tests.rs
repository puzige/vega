use super::shell_recovery::{ShellRecoveryError, ShellRecoveryService};
use crate::{
    threads,
    types::{TaskBackend, Thread, ThreadStatus},
};
use std::path::{Path, PathBuf};
use vega_store::{
    Store,
    shell_operations::{self, ShellOperationRecord},
};

struct Workspace {
    root: PathBuf,
    database: PathBuf,
    project: vega_store::projects::Project,
    store: Store,
}

impl Workspace {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("vega-draft-recovery-{}", ulid::Ulid::generate()));
        std::fs::create_dir_all(root.join("project")).unwrap();
        let database = root.join("vega.db");
        let store = Store::open(&database).unwrap();
        store.migrate().unwrap();
        let project = vega_store::projects::create(
            store.conn(),
            root.join("project").to_str().unwrap(),
            "project",
            None,
        )
        .unwrap();
        Self {
            root,
            database,
            project,
            store,
        }
    }

    fn draft(&self) -> Thread {
        threads::draft_thread(Some(&self.project.id), "model", "confirm")
    }

    fn service(&self) -> ShellRecoveryService {
        ShellRecoveryService::for_draft(self.database.clone())
    }

    fn blocker(&self, workspace: &Path) -> (Thread, String) {
        let thread =
            threads::create_thread(&self.store, &self.project.id, "model", "confirm").unwrap();
        let run = ulid::Ulid::generate().to_string();
        let call = ulid::Ulid::generate().to_string();
        self.store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES(?1,?2,1,'assistant','','streaming',1)", [&run, &thread.id]).unwrap();
        self.store.conn().execute("INSERT INTO tool_calls(id,thread_id,message_id,seq,tool,input_json,status,created_at) VALUES(?1,?2,?3,1,'bash','{\"command\":\"echo test\"}','running',1)", [&call, &thread.id, &run]).unwrap();
        let operation = ulid::Ulid::generate().to_string();
        shell_operations::record(
            self.store.conn(),
            &ShellOperationRecord {
                operation_id: &operation,
                thread_id: &thread.id,
                run_id: &run,
                call_id: &call,
                generation: vega_runtime::shell_generation(),
                workspace: workspace.to_str().unwrap(),
                state: "prepared",
                exit_code: None,
                error: None,
                duration_ms: None,
            },
            1,
        )
        .unwrap();
        shell_operations::record(
            self.store.conn(),
            &ShellOperationRecord {
                operation_id: &operation,
                thread_id: &thread.id,
                run_id: &run,
                call_id: &call,
                generation: vega_runtime::shell_generation(),
                workspace: workspace.to_str().unwrap(),
                state: "outcome_unknown",
                exit_code: None,
                error: Some("cleanup_unresolved"),
                duration_ms: None,
            },
            2,
        )
        .unwrap();
        (thread, operation)
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn unpersisted_draft_reads_clean_workspace_without_materializing() {
    let env = Workspace::new();
    let draft = env.draft();
    let projection = env.service().read(&draft).unwrap();
    assert_eq!(
        PathBuf::from(&projection.workspace),
        PathBuf::from(&env.project.path).canonicalize().unwrap()
    );
    assert!(projection.rows.is_empty());
    assert!(projection.active.is_empty());
    assert!(
        !vega_runtime::RuntimeWorkspaceMutationGuard::is_blocked(Path::new(&projection.workspace))
            .unwrap()
    );
    assert!(
        vega_store::threads::find(env.store.conn(), &draft.id)
            .unwrap()
            .is_none()
    );
    assert!(
        ShellRecoveryService::new(env.database.clone())
            .read(&draft)
            .is_err()
    );
}

#[test]
fn draft_preserves_other_conversations_real_workspace_blocker() {
    let env = Workspace::new();
    let (owner, operation) = env.blocker(Path::new(&env.project.path));
    let draft = env.draft();
    let service = env.service();
    let projection = service.read(&draft).unwrap();
    assert_eq!(projection.rows.len(), 1);
    assert_eq!(projection.rows[0].operation_id, operation);
    assert_eq!(projection.rows[0].thread_id, owner.id);
    assert!(!projection.rows[0].can_resume);
    assert!(
        shell_operations::has_unknown_workspace(env.store.conn(), &projection.workspace).unwrap()
    );
    assert!(
        vega_runtime::RuntimeWorkspaceMutationGuard::acquire(Path::new(&projection.workspace))
            .is_err()
    );
    assert!(matches!(
        service.prepare(&draft, &projection),
        Err(ShellRecoveryError::KnownProcessOrResource)
    ));
    vega_runtime::RuntimeWorkspaceMutationGuard::set_persisted_block(
        Path::new(&projection.workspace),
        &operation,
        false,
    )
    .unwrap();
}

#[test]
fn draft_preserves_blockers_in_overlapping_child_workspace() {
    let env = Workspace::new();
    let child = Path::new(&env.project.path).join("child");
    std::fs::create_dir(&child).unwrap();
    let (_, operation) = env.blocker(&child);
    let projection = env.service().read(&env.draft()).unwrap();
    assert_eq!(projection.rows.len(), 1);
    assert_eq!(projection.rows[0].operation_id, operation);
    assert!(
        vega_runtime::RuntimeWorkspaceMutationGuard::acquire(Path::new(&projection.workspace))
            .is_err()
    );
    vega_runtime::RuntimeWorkspaceMutationGuard::set_persisted_block(
        Path::new(&projection.workspace),
        &operation,
        false,
    )
    .unwrap();
}

#[test]
fn draft_does_not_report_unrelated_workspace_as_blocked() {
    let env = Workspace::new();
    let unrelated = env.root.join("other");
    std::fs::create_dir(&unrelated).unwrap();
    env.blocker(&unrelated);
    let projection = env.service().read(&env.draft()).unwrap();
    assert!(projection.rows.is_empty());
    assert!(
        !shell_operations::has_unknown_workspace(env.store.conn(), &projection.workspace).unwrap()
    );
}

#[test]
fn missing_project_or_directory_is_not_a_clean_recovery_result() {
    let env = Workspace::new();
    let mut draft = env.draft();
    draft.project_id = ulid::Ulid::generate().to_string();
    assert!(matches!(
        env.service().read(&draft),
        Err(ShellRecoveryError::Changed)
    ));
    draft.project_id = env.project.id.clone();
    std::fs::remove_dir(&env.project.path).unwrap();
    assert!(matches!(
        env.service().read(&draft),
        Err(ShellRecoveryError::Unavailable)
    ));
}

#[test]
fn materialized_or_invalid_draft_identity_is_rejected() {
    let env = Workspace::new();
    let stored = threads::create_thread(&env.store, &env.project.id, "model", "confirm").unwrap();
    assert!(matches!(
        env.service().read(&stored),
        Err(ShellRecoveryError::Changed)
    ));
    let draft = env.draft();
    let mut invalid = draft.clone();
    invalid.backend = TaskBackend::Codex;
    assert!(matches!(
        env.service().read(&invalid),
        Err(ShellRecoveryError::Changed)
    ));
    invalid = draft.clone();
    invalid.parent_thread_id = Some(stored.id.clone());
    assert!(matches!(
        env.service().read(&invalid),
        Err(ShellRecoveryError::Changed)
    ));
    invalid = draft.clone();
    invalid.status = ThreadStatus::Archived;
    assert!(matches!(
        env.service().read(&invalid),
        Err(ShellRecoveryError::Changed)
    ));
    invalid = draft;
    invalid.draft_authority_epoch = Some("stale".into());
    assert!(matches!(
        env.service().read(&invalid),
        Err(ShellRecoveryError::Changed)
    ));
}

#[test]
fn persisted_conversation_retains_strict_revision_check() {
    let env = Workspace::new();
    let mut stored =
        threads::create_thread(&env.store, &env.project.id, "model", "confirm").unwrap();
    let service = ShellRecoveryService::new(env.database.clone());
    assert!(service.read(&stored).unwrap().rows.is_empty());
    stored.permission_revision += 1;
    assert!(matches!(
        service.read(&stored),
        Err(ShellRecoveryError::Changed)
    ));
}

#[test]
fn draft_recovery_requires_fresh_confirmation_before_releasing_real_blocker() {
    let env = Workspace::new();
    let (_, operation) = env.blocker(Path::new(&env.project.path));
    let draft = env.draft();
    let service = env.service();
    let unresolved = service.read(&draft).unwrap();
    let facts = shell_operations::ShellFacts {
        cleanup_state: "unresolved".into(),
        resources_closed: true,
        process_resolved: true,
        output_incomplete: true,
        ..Default::default()
    };
    shell_operations::append_cleanup_observation(env.store.conn(), &operation, "closed", &facts, 3)
        .unwrap();
    assert!(matches!(
        service.prepare(&draft, &unresolved),
        Err(ShellRecoveryError::Changed)
    ));
    let ready = service.read(&draft).unwrap();
    assert!(ready.rows[0].can_resume);
    let confirmation = service.prepare(&draft, &ready).unwrap();
    assert!(matches!(
        ShellRecoveryService::new(env.database.clone()).resume(&confirmation),
        Err(ShellRecoveryError::Changed)
    ));
    assert!(shell_operations::has_unknown_workspace(env.store.conn(), &ready.workspace).unwrap());
    let recovered = service.resume(&confirmation).unwrap();
    assert!(recovered.rows.is_empty());
    assert!(!shell_operations::has_unknown_workspace(env.store.conn(), &ready.workspace).unwrap());
    assert!(
        vega_runtime::RuntimeWorkspaceMutationGuard::acquire(Path::new(&ready.workspace)).is_ok()
    );
    assert!(
        vega_store::threads::find(env.store.conn(), &draft.id)
            .unwrap()
            .is_none()
    );
}

#[test]
fn unavailable_database_does_not_become_clean_draft_result() {
    let env = Workspace::new();
    let service = ShellRecoveryService::for_draft(env.root.join("missing.db"));
    assert!(matches!(
        service.read(&env.draft()),
        Err(ShellRecoveryError::Unavailable)
    ));
}
