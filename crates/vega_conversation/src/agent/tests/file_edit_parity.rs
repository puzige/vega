use super::*;

fn file_provider(id: &str, tool: &str, input: serde_json::Value) -> MockProvider {
    MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: id.into(),
                name: tool.into(),
                input_json: input.to_string(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
    ])
}

fn terminal(run: &ConversationRun, id: &str) -> crate::types::ToolResult {
    run.events
        .iter()
        .find_map(|event| match event {
            ConversationEvent::ToolCallFinished { call_id, result } if call_id == id => {
                Some(result.clone())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing terminal for {id}: {:?}", run.events))
}

#[tokio::test]
async fn issue112_external_canonical_edit_survives_new_tools_and_history() {
    for mode in ["full_access", "confirm", "auto"] {
        let (store, project, _data, _) = setup_external(mode);
        let outside = tempdir().unwrap();
        let path = outside.path().canonicalize().unwrap().join("note.txt");
        fs::write(&path, "alpha alpha\n").unwrap();
        let reads = vega_tools::ReadState::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let hook = FixedPermissionHook {
            calls: calls.clone(),
            decision: PermissionDecision::Always,
        };
        let tools = vega_tools::Tools::new(project.path())
            .unwrap()
            .with_read_state(reads.clone());
        let read = run_thread_task_with_permission_sink(
            &store,
            &file_provider("read-ext", "Read", serde_json::json!({"file_path":path})),
            &tools,
            "thread-1",
            "read file",
            "system",
            CancellationToken::new(),
            &hook,
            |_| Ok(()),
        )
        .await
        .unwrap();
        assert_eq!(terminal(&read, "read-ext").status, ToolCallStatus::Success);
        let tools = vega_tools::Tools::new(project.path())
            .unwrap()
            .with_read_state(reads);
        let edit = run_thread_task_with_permission_sink(&store, &file_provider("edit-ext", "Edit", serde_json::json!({"file_path":path,"old_string":"alpha","new_string":"beta","replace_all":true})), &tools, "thread-1", "edit file", "system", CancellationToken::new(), &hook, |_| Ok(())).await.unwrap();
        let result = terminal(&edit, "edit-ext");
        assert_eq!(result.status, ToolCallStatus::Success, "{}", result.output);
        assert_eq!(fs::read_to_string(&path).unwrap(), "beta beta\n");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&result.output).unwrap()["replacements"],
            2
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            if mode == "full_access" { 0 } else { 2 }
        );
        // Loading this follow-up proves canonical tool names and replace-all
        // results pass the production persisted-history decoder.
        let next = run_thread_task_with_permission_sink(
            &store,
            &file_provider("read-again", "Read", serde_json::json!({"file_path":path})),
            &tools,
            "thread-1",
            "read again",
            "system",
            CancellationToken::new(),
            &hook,
            |_| Ok(()),
        )
        .await
        .unwrap();
        assert_eq!(
            terminal(&next, "read-again").status,
            ToolCallStatus::Success
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            if mode == "full_access" { 0 } else { 2 },
            "remembered canonical Read rule survives history reconstruction"
        );
        let audit: String = store
            .conn()
            .query_row(
                "SELECT input_json FROM tool_calls WHERE id = 'edit-ext'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!audit.contains("alpha"));
        assert!(!audit.contains("beta"));
    }
}

#[tokio::test]
async fn issue112_external_read_denial_plan_and_unread_edit_do_not_mutate() {
    for (mode, decision, expected) in [
        (
            "execute",
            PermissionDecision::Deny { note: None },
            ToolCallStatus::Rejected,
        ),
        ("plan", PermissionDecision::Once, ToolCallStatus::Success),
    ] {
        let (store, project, _data, _) = setup_external("confirm");
        store
            .conn()
            .execute("UPDATE threads SET mode = ?1 WHERE id = 'thread-1'", [mode])
            .unwrap();
        let outside = tempdir().unwrap();
        let path = outside.path().canonicalize().unwrap().join("note.txt");
        fs::write(&path, "original").unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let hook = FixedPermissionHook {
            calls: calls.clone(),
            decision,
        };
        let tools = vega_tools::Tools::new(project.path()).unwrap();
        let read = run_thread_task_with_permission_sink(
            &store,
            &file_provider("read-ext", "Read", serde_json::json!({"file_path":path})),
            &tools,
            "thread-1",
            "read",
            "system",
            CancellationToken::new(),
            &hook,
            |_| Ok(()),
        )
        .await
        .unwrap();
        assert_eq!(terminal(&read, "read-ext").status, expected);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let edit = run_thread_task_with_permission_sink(&store, &file_provider("edit-ext", "Edit", serde_json::json!({"file_path":path,"old_string":"original","new_string":"changed"})), &tools, "thread-1", "edit", "system", CancellationToken::new(), &hook, |_| Ok(())).await.unwrap();
        assert_eq!(terminal(&edit, "edit-ext").status, ToolCallStatus::Rejected);
        assert_eq!(fs::read_to_string(path).unwrap(), "original");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn issue112_unread_task_state_and_changed_after_read_fail_closed() {
    let (store, project, _data, _) = setup_external("full_access");
    let path = project.path().canonicalize().unwrap().join("note.txt");
    fs::write(&path, "original").unwrap();
    let reads = vega_tools::ReadState::default();
    let tools = vega_tools::Tools::new(project.path())
        .unwrap()
        .with_read_state(reads.clone());
    let hook = FixedPermissionHook {
        calls: Arc::new(AtomicUsize::new(0)),
        decision: PermissionDecision::Once,
    };
    run_thread_task_with_permission_sink(
        &store,
        &file_provider("read", "Read", serde_json::json!({"file_path":path})),
        &tools,
        "thread-1",
        "read",
        "system",
        CancellationToken::new(),
        &hook,
        |_| Ok(()),
    )
    .await
    .unwrap();
    for (id, state, changed, expected) in [
        (
            "unread",
            vega_tools::ReadState::default(),
            false,
            "file_not_read",
        ),
        ("stale", reads, true, "target_changed"),
    ] {
        if changed {
            fs::write(&path, "external change").unwrap();
        }
        let tools = vega_tools::Tools::new(project.path())
            .unwrap()
            .with_read_state(state);
        let run = run_thread_task_with_permission_sink(&store, &file_provider(id, "Edit", serde_json::json!({"file_path":path,"old_string":"original","new_string":"changed"})), &tools, "thread-1", "edit", "system", CancellationToken::new(), &hook, |_| Ok(())).await.unwrap();
        let result = terminal(&run, id);
        assert_eq!(result.status, ToolCallStatus::Rejected);
        assert!(result.output.contains(expected), "{}", result.output);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            if changed {
                "external change"
            } else {
                "original"
            }
        );
    }
}

#[tokio::test]
async fn issue112_approval_binds_external_target_and_revalidates_after_wait() {
    struct ChangeOnEdit {
        path: PathBuf,
        requests: Arc<Mutex<Vec<PermissionRequest>>>,
    }
    impl PermissionHook for ChangeOnEdit {
        fn request(
            &self,
            request: PermissionRequest,
            _cancel: CancellationToken,
        ) -> BoxFuture<'static, Result<PermissionDecision, VegaError>> {
            self.requests.lock().unwrap().push(request.clone());
            if request.tool == "edit" {
                fs::write(&self.path, "changed while waiting").unwrap();
            }
            async { Ok(PermissionDecision::Once) }.boxed()
        }
    }
    let (store, project, _data, _) = setup_external("confirm");
    let outside = tempdir().unwrap();
    let path = outside.path().canonicalize().unwrap().join("note.txt");
    fs::write(&path, "original").unwrap();
    let alias = project.path().join("alias.txt");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&path, &alias).unwrap();
    #[cfg(not(unix))]
    let alias = path.clone();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let hook = ChangeOnEdit {
        path: path.clone(),
        requests: requests.clone(),
    };
    let read = run_thread_task_with_permission_sink(
        &store,
        &file_provider("read-alias", "Read", serde_json::json!({"file_path":alias})),
        &tools,
        "thread-1",
        "read",
        "system",
        CancellationToken::new(),
        &hook,
        |_| Ok(()),
    )
    .await
    .unwrap();
    assert_eq!(
        terminal(&read, "read-alias").status,
        ToolCallStatus::Success
    );
    let run = run_thread_task_with_permission_sink(&store, &file_provider("edit-alias", "Edit", serde_json::json!({"file_path":alias,"old_string":"original","new_string":"replacement"})), &tools, "thread-1", "edit", "system", CancellationToken::new(), &hook, |_| Ok(())).await.unwrap();
    let result = terminal(&run, "edit-alias");
    assert_eq!(result.status, ToolCallStatus::Failed);
    assert!(
        result.output.contains("target_changed"),
        "{}",
        result.output
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "changed while waiting");
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(
        requests
            .iter()
            .all(|request| request.display_target == path.to_string_lossy())
    );
}

#[tokio::test]
async fn issue112_encoded_write_has_exact_disk_bytes_and_restores_history() {
    for encoding in ["utf8_bom", "utf16le"] {
        let (store, project, _data, _) = setup_external("full_access");
        let path = project.path().canonicalize().unwrap().join("encoded.txt");
        let encode = |text: &str| -> Vec<u8> {
            if encoding == "utf16le" {
                [
                    vec![0xff, 0xfe],
                    text.encode_utf16().flat_map(u16::to_le_bytes).collect(),
                ]
                .concat()
            } else {
                [vec![0xef, 0xbb, 0xbf], text.as_bytes().to_vec()].concat()
            }
        };
        fs::write(&path, encode("before\r\n")).unwrap();
        let tools = vega_tools::Tools::new(project.path()).unwrap();
        let hook = FixedPermissionHook {
            calls: Arc::new(AtomicUsize::new(0)),
            decision: PermissionDecision::Once,
        };
        run_thread_task_with_permission_sink(
            &store,
            &file_provider(
                "read-encoded",
                "Read",
                serde_json::json!({"file_path":path}),
            ),
            &tools,
            "thread-1",
            "read",
            "system",
            CancellationToken::new(),
            &hook,
            |_| Ok(()),
        )
        .await
        .unwrap();
        let write = run_thread_task_with_permission_sink(
            &store,
            &file_provider(
                "write-encoded",
                "Write",
                serde_json::json!({"file_path":path,"content":"after\n"}),
            ),
            &tools,
            "thread-1",
            "write",
            "system",
            CancellationToken::new(),
            &hook,
            |_| Ok(()),
        )
        .await
        .unwrap();
        let result = terminal(&write, "write-encoded");
        assert_eq!(result.status, ToolCallStatus::Success, "{}", result.output);
        // Write honors the supplied newlines; encoding and BOM are preserved.
        let expected = encode("after\n");
        assert_eq!(fs::read(&path).unwrap(), expected);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&result.output).unwrap()["bytes_written"],
            expected.len()
        );
        let next = run_thread_task_with_permission_sink(
            &store,
            &file_provider(
                "read-written",
                "Read",
                serde_json::json!({"file_path":path}),
            ),
            &tools,
            "thread-1",
            "read again",
            "system",
            CancellationToken::new(),
            &hook,
            |_| Ok(()),
        )
        .await
        .unwrap();
        assert_eq!(
            terminal(&next, "read-written").status,
            ToolCallStatus::Success
        );
    }
}

#[tokio::test]
async fn issue112_completed_write_replay_uses_frozen_identity_after_delete() {
    let (store, project, _data, _) = setup_external("full_access");
    let path = project.path().canonicalize().unwrap().join("encoded.txt");
    fs::write(
        &path,
        [
            vec![0xff, 0xfe],
            "before".encode_utf16().flat_map(u16::to_le_bytes).collect(),
        ]
        .concat(),
    )
    .unwrap();
    let tools = vega_tools::Tools::new(project.path()).unwrap();
    let hook = FixedPermissionHook {
        calls: Arc::new(AtomicUsize::new(0)),
        decision: PermissionDecision::Once,
    };
    run_thread_task_with_permission_sink(
        &store,
        &file_provider(
            "read-original",
            "Read",
            serde_json::json!({"file_path":path}),
        ),
        &tools,
        "thread-1",
        "read",
        "system",
        CancellationToken::new(),
        &hook,
        |_| Ok(()),
    )
    .await
    .unwrap();
    let input = serde_json::json!({"file_path":path,"content":"after"});
    let first = run_thread_task_with_permission_sink(
        &store,
        &file_provider("write-original", "Write", input.clone()),
        &tools,
        "thread-1",
        "write",
        "system",
        CancellationToken::new(),
        &hook,
        |_| Ok(()),
    )
    .await
    .unwrap();
    let original = terminal(&first, "write-original");
    assert_eq!(original.status, ToolCallStatus::Success);
    fs::remove_file(&path).unwrap();
    let fresh_tools = vega_tools::Tools::new(project.path()).unwrap();
    let replay = run_thread_task_with_permission_sink(
        &store,
        &file_provider("write-original", "Write", input),
        &fresh_tools,
        "thread-1",
        "retry",
        "system",
        CancellationToken::new(),
        &hook,
        |_| Ok(()),
    )
    .await
    .unwrap();
    let result = terminal(&replay, "write-original");
    assert!(result.reused);
    assert_eq!(result.output, original.output);
    assert!(!path.exists());
    let conflict = run_thread_task_with_permission_sink(
        &store,
        &file_provider(
            "write-original",
            "Write",
            serde_json::json!({"file_path":path,"content":"different"}),
        ),
        &fresh_tools,
        "thread-1",
        "conflict",
        "system",
        CancellationToken::new(),
        &hook,
        |_| Ok(()),
    )
    .await
    .unwrap();
    assert!(conflict.events.iter().any(|event| matches!(event, ConversationEvent::ToolCallFinished { result, .. } if result.status == ToolCallStatus::Failed)));
    assert!(!path.exists());
}
