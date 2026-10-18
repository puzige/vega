use super::*;
use std::sync::atomic::AtomicBool;

#[tokio::test]
async fn credential_continuation_reopens_database_without_replaying_side_effect_or_user() {
    let (store, dir, _) = setup();
    let effects = Arc::new(AtomicUsize::new(0));
    let counter = effects.clone();
    let tools = vega_tools::Tools::new(dir.path())
        .unwrap()
        .with_bash_test_executor(Arc::new(move |_, _, _| {
            counter.fetch_add(1, Ordering::SeqCst);
            Box::pin(async {
                Ok(vega_tools::collect_bash_output_for_test(
                    b"effect completed",
                ))
            })
        }));
    let blocked = Arc::new(AtomicBool::new(false));
    let flag = blocked.clone();
    let reader: vega_runtime::CredentialReader = Arc::new(move || {
        if flag.load(Ordering::SeqCst) {
            Err(())
        } else {
            Ok(Vec::new())
        }
    });
    let transport = Arc::new(MockProvider::new_rounds(vec![vec![ScriptStep::events(
        vec![
            ProviderEvent::ToolUse {
                id: "once-only".into(),
                name: "bash".into(),
                input_json: r#"{"cmd":"synthetic-effect","timeout_ms":null}"#.into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ],
    )]]));
    let guarded = OwnerCredentialProvider::new(transport, reader.clone());
    let permission = FixedPermissionHook {
        calls: Arc::new(AtomicUsize::new(0)),
        decision: PermissionDecision::Once,
    };
    let first = run_thread_task_with_images_reasoning_and_mcp(
        &store,
        &guarded,
        &tools,
        "thread-1",
        "perform once",
        "System",
        CancellationToken::new(),
        &permission,
        |event| {
            if matches!(event, ConversationEvent::ToolCallFinished { .. }) {
                blocked.store(true, Ordering::SeqCst);
            }
            Ok(())
        },
        PersistenceActorConfig::default().with_owner_credential_reader(reader),
        None,
        None,
        None,
        Vec::new(),
        Vec::new(),
    )
    .await
    .unwrap();
    assert!(first.failed);
    assert_eq!(effects.load(Ordering::SeqCst), 1);
    let receipt =
        vega_store::credential_continuation::find(store.conn(), &first.assistant_message_id)
            .unwrap()
            .unwrap();
    assert_eq!(receipt.state, "ready");
    let path = store.database_path().unwrap().to_owned();
    drop(store);
    let store = Store::open(path).unwrap();
    let provider = MockProvider::new(vec![
        ScriptStep::text("Continued"),
        ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }]),
    ]);
    let resumed = resume_credential_continuation_with_mcp(
        &store,
        &provider,
        &tools,
        "thread-1",
        &receipt.id,
        "System",
        CancellationToken::new(),
        &permission,
        |_| Ok(()),
        PersistenceActorConfig::default(),
        None,
        None,
        Vec::new(),
    )
    .await
    .unwrap();
    assert!(!resumed.failed);
    assert_eq!(resumed.user_message_id, first.user_message_id);
    assert_eq!(effects.load(Ordering::SeqCst), 1);
    let users: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE role = 'user'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(users, 1);
    assert!(
        provider.requests()[0]
            .messages
            .iter()
            .any(|message| message.content.contains("effect completed"))
    );
    assert!(
        resume_credential_continuation_with_mcp(
            &store,
            &provider,
            &tools,
            "thread-1",
            &receipt.id,
            "System",
            CancellationToken::new(),
            &permission,
            |_| Ok(()),
            PersistenceActorConfig::default(),
            None,
            None,
            Vec::new(),
        )
        .await
        .is_err()
    );
    assert_eq!(provider.requests().len(), 1);
}
