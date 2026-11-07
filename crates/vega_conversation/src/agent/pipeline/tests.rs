use super::super::persistence::DelegatedAuthority;
use super::*;

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Fixture {
    root: PathBuf,
    store: Store,
    child: String,
    user: String,
    assistant: String,
    config: PersistenceActorConfig,
}

impl Fixture {
    fn new(with_history: bool) -> Result<Self, Box<dyn std::error::Error>> {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!(
            "vega-delegated-preparation-{}",
            ulid::Ulid::generate()
        ));
        fs::create_dir(&root)?;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
        let store = Store::open(root.join("fixture.db"))?;
        store.migrate()?;
        let parent = ulid::Ulid::generate().to_string();
        let child = ulid::Ulid::generate().to_string();
        let parent_message = ulid::Ulid::generate().to_string();
        let user = ulid::Ulid::generate().to_string();
        let assistant = ulid::Ulid::generate().to_string();
        store.conn().execute("INSERT INTO threads(id,model,mode,permission_mode,created_at,updated_at) VALUES(?1,'model','execute','confirm',1,1)", [&parent])?;
        store.conn().execute("INSERT INTO threads(id,parent_thread_id,model,mode,permission_mode,created_at,updated_at) VALUES(?1,?2,'model','execute','confirm',1,1)", [&child, &parent])?;
        store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES(?1,?2,1,'assistant','','streaming',1)", [&parent_message, &parent])?;
        let seq = if with_history {
            store.conn().execute_batch("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) SELECT 'old-user',id,1,'user','earlier task','done',1 FROM threads WHERE parent_thread_id IS NOT NULL; INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) SELECT 'old-assistant',id,2,'assistant','earlier final','done',2 FROM threads WHERE parent_thread_id IS NOT NULL; INSERT INTO assistant_responses(message_id,response_id,text_start_bytes,text_end_bytes,kind) VALUES('old-assistant','response',0,13,'final');")?;
            3
        } else {
            1
        };
        store.conn().execute("INSERT INTO messages(id,thread_id,seq,role,content,status,created_at) VALUES(?1,?2,?3,'user','current task','done',3)", [user.as_str(), child.as_str(), &seq.to_string()])?;
        crate::threads::task_workspace_root(&store, &parent)?;
        let parent_row =
            vega_store::threads::find(store.conn(), &parent)?.ok_or("parent missing")?;
        let parent_fence = crate::permission_contracts::capture_fence(&store, &parent_row)?;
        let tx = store.immediate_transaction()?;
        vega_store::permission_contracts::record_run_fence(
            &tx,
            &parent_message,
            &parent,
            &serde_json::to_string(&parent_fence)?,
        )?;
        tx.execute("INSERT INTO delegated_executions(child_message_id,child_thread_id,parent_thread_id,parent_message_id,request_call_id,ordinal,kind,instruction_message_id,name,task,state,created_at) VALUES(?1,?2,?3,?4,'spawn-call',1,'spawn',?5,'fixture','current task','running',3)", [&assistant, &child, &parent, &parent_message, &user])?;
        tx.commit()?;
        let child_row = vega_store::threads::find(store.conn(), &child)?.ok_or("child missing")?;
        let child_fence = crate::permission_contracts::capture_delegated_fence(
            &store,
            &child_row,
            &parent,
            &parent_message,
            &assistant,
            &parent_fence,
        )?;
        let tx = store.immediate_transaction()?;
        vega_store::permission_contracts::record_delegated_fence(
            &tx,
            &child,
            &assistant,
            &serde_json::to_string(&child_fence)?,
        )?;
        vega_store::delegated_stages::register(
            &tx,
            &child,
            &assistant,
            None,
            &serde_json::json!([]),
        )?;
        tx.commit()?;
        let config = PersistenceActorConfig {
            delegated: true,
            delegated_parent: Some((parent, parent_message)),
            delegated_message_id: Some(assistant.clone()),
            delegated_authority: Some(DelegatedAuthority {
                permission_fence: parent_fence,
                run_mode: RuntimeRunMode::Execute,
                permission_mode: RuntimePermissionMode::Confirm,
                exact_rules: Vec::new(),
            }),
            turn_limit: 10,
            ..Default::default()
        };
        Ok(Self {
            root,
            store,
            child,
            user,
            assistant,
            config,
        })
    }

    fn prepare(&self) -> Result<PreparedRun, ConversationError> {
        prepare_run_with_images_and_reasoning(
            self.root.join("fixture.db"),
            self.child.clone(),
            "current task".into(),
            "system".into(),
            self.user.clone(),
            self.assistant.clone(),
            self.config.clone(),
            true,
            None,
            None,
            Vec::new(),
            None,
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn delegated_first_preparation_commits_frozen_source_and_fence() -> TestResult {
    let fixture = Fixture::new(false)?;
    let prepared = fixture.prepare()?;
    assert_eq!(prepared.assistant_message_id, fixture.assistant);
    assert_eq!(prepared.assistant_seq, 2);
    assert!(prepared.skill_launch.is_none());
    assert_eq!(prepared.request.history.len(), 1);
    assert!(prepared.request.completed_tool_results.is_empty());
    let tx = fixture.store.conn().unchecked_transaction()?;
    let actual = vega_store::context_compaction::load_source_in_transaction(&tx, &fixture.child)?;
    assert_eq!(
        prepared.request.context_source_version,
        Some(actual.source_version)
    );
    assert_eq!(
        prepared.request.context_source_fingerprint.as_deref(),
        Some(actual.fingerprint.as_str())
    );
    assert!(
        vega_store::permission_contracts::load_run_fence(&tx, &fixture.assistant, &fixture.child)?
            .is_some()
    );
    tx.commit()?;
    Ok(())
}

#[test]
fn delegated_history_projection_matches_canonical_committed_history() -> TestResult {
    let fixture = Fixture::new(true)?;
    let prepared = fixture.prepare()?;
    let tx = fixture.store.conn().unchecked_transaction()?;
    let source = vega_store::context_compaction::load_source_in_transaction(&tx, &fixture.child)?;
    let expected = delegated_history(&tx, &source, &fixture.assistant)?;
    assert!(prepared.request.history == expected);
    assert_eq!(prepared.request.history.len(), 3);
    let encoded = prepared
        .request
        .history
        .iter()
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(encoded.contains("earlier final"));
    assert!(encoded.contains("untrusted"));
    tx.commit()?;
    Ok(())
}

#[test]
fn cancellation_before_final_commit_rolls_back_assistant_and_fence() -> TestResult {
    let mut fixture = Fixture::new(true)?;
    let cancel = CancellationToken::new();
    cancel.cancel();
    fixture.config.steering_cancel = Some(cancel);
    let result = fixture.prepare();
    assert!(
        matches!(result, Err(ConversationError::Runtime(error)) if matches!(error.as_ref(), VegaError::Cancelled))
    );
    assert!(messages::find(fixture.store.conn(), &fixture.assistant)?.is_none());
    assert!(
        vega_store::permission_contracts::load_run_fence(
            fixture.store.conn(),
            &fixture.assistant,
            &fixture.child
        )?
        .is_none()
    );
    assert_eq!(
        messages::find(fixture.store.conn(), "old-assistant")?.map(|row| row.content),
        Some("earlier final".into())
    );
    Ok(())
}

#[test]
fn stale_delegated_authority_rejects_without_consuming_instruction() -> TestResult {
    let fixture = Fixture::new(false)?;
    fixture.store.conn().execute("UPDATE threads SET permission_revision=permission_revision+1 WHERE parent_thread_id IS NULL", [])?;
    assert!(fixture.prepare().is_err());
    assert!(messages::find(fixture.store.conn(), &fixture.assistant)?.is_none());
    assert_eq!(messages::next_seq(fixture.store.conn(), &fixture.child)?, 2);
    assert!(
        vega_store::permission_contracts::load_run_fence(
            fixture.store.conn(),
            &fixture.assistant,
            &fixture.child
        )?
        .is_none()
    );
    Ok(())
}
