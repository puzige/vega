use super::*;

#[test]
fn commit_redaction_all_public_provider_carriers_hide_sentinels() {
    const SENTINEL: &str = "VEGA_T34_SECRET_SENTINEL";
    let call = vega_runtime::ChatToolCall {
        id: SENTINEL.into(),
        name: SENTINEL.into(),
        input_json: SENTINEL.into(),
    };
    let request = ChatRequest {
        model: SENTINEL.into(),
        messages: vec![ChatMessage::assistant_with_tools(SENTINEL, vec![call])],
        tools: vec![vega_runtime::ToolDefinition {
            name: SENTINEL.into(),
            description: SENTINEL.into(),
            input_schema: serde_json::json!({"sentinel": SENTINEL}),
        }],
        max_tokens: Some(256),
    };
    let event = ProviderEvent::ToolUse {
        id: SENTINEL.into(),
        name: SENTINEL.into(),
        input_json: SENTINEL.into(),
    };
    let step = vega_runtime::ScriptStep::Error {
        status: Some(500),
        message: SENTINEL.into(),
        retryable: false,
    };
    let mock = vega_runtime::MockProvider::new(vec![step.clone()]);
    let draft = CommitDraft::new(SENTINEL.into());
    let runtime_result = vega_runtime::RuntimeToolResult {
        call_id: SENTINEL.into(),
        output: SENTINEL.into(),
        status: vega_runtime::RuntimeToolStatus::Failed,
        reused: false,
        exit_code: None,
        duration_ms: None,
        truncated: None,
        approval: None,
        remember_rule: None,
    };
    let runtime_call = vega_runtime::RuntimeToolCall {
        id: SENTINEL.into(),
        name: SENTINEL.into(),
        input_json: SENTINEL.into(),
    };
    let runtime_event =
        vega_runtime::RuntimeEvent::Error(Arc::new(vega_runtime::VegaError::Provider {
            status: Some(500),
            message: SENTINEL.into(),
            retryable: false,
        }));
    let agent_request = vega_runtime::AgentRequest {
        model: SENTINEL.into(),
        system_prompt: SENTINEL.into(),
        history: vec![ChatMessage::new(ChatRole::User, SENTINEL)],
        max_tokens: Some(256),
        completed_tool_results: std::collections::HashMap::from([(
            SENTINEL.into(),
            vega_runtime::CompletedToolCall {
                tool: SENTINEL.into(),
                input_json: SENTINEL.into(),
                result: runtime_result.clone(),
            },
        )]),
        tool_config: vega_runtime::RuntimeToolConfig::new(
            vega_runtime::RuntimeRunMode::Execute,
            vega_runtime::RuntimePermissionMode::Confirm,
            SENTINEL.into(),
            SENTINEL.into(),
            PathBuf::from(SENTINEL),
            vec![vega_runtime::RuntimeExactRule {
                tool: vega_runtime::RuntimeMutatingTool::Write,
                pattern: SENTINEL.into(),
            }],
        ),
        pricing_catalog: None,
    };
    let agent_outcome = vega_runtime::AgentOutcome {
        events: vec![runtime_event.clone()],
        messages: vec![ChatMessage::new(ChatRole::Assistant, SENTINEL)],
        final_text: SENTINEL.into(),
        tool_call_count: 1,
        executed_tool_call_count: 0,
        interrupted: false,
        failed: true,
    };
    let conversation_result = crate::types::ToolResult {
        status: crate::types::ToolCallStatus::Failed,
        output: SENTINEL.into(),
        reused: false,
        exit_code: None,
        duration_ms: None,
        truncated: None,
        invalid: None,
    };
    let conversation_event = crate::types::ConversationEvent::ToolCallFinished {
        call_id: SENTINEL.into(),
        result: conversation_result.clone(),
    };
    let conversation_error = crate::types::ConversationEvent::Error {
        message_id: Some(SENTINEL.into()),
        error: Arc::new(vega_runtime::VegaError::Provider {
            status: None,
            message: SENTINEL.into(),
            retryable: false,
        }),
    };
    let conversation_run = crate::agent::ConversationRun {
        user_message_id: SENTINEL.into(),
        assistant_message_id: SENTINEL.into(),
        events: vec![conversation_event.clone(), conversation_error.clone()],
        content: SENTINEL.into(),
        interrupted: false,
        failed: true,
    };
    for debug in [
        format!("{request:?}"),
        format!("{event:?}"),
        format!("{step:?}"),
        format!("{mock:?}"),
        format!("{draft:?}"),
        format!("{runtime_result:?}"),
        format!("{runtime_call:?}"),
        format!("{runtime_event:?}"),
        format!("{agent_request:?}"),
        format!("{agent_outcome:?}"),
        format!("{conversation_result:?}"),
        format!("{conversation_event:?}"),
        format!("{conversation_error:?}"),
        format!("{conversation_run:?}"),
    ] {
        assert!(!debug.contains(SENTINEL), "debug leaked sentinel");
    }
}

#[test]
fn three_source_parsers_reject_conflicting_duplicate_paths() {
    let oid_a = b"1111111111111111111111111111111111111111";
    let oid_b = b"2222222222222222222222222222222222222222";
    let mut stages = Vec::new();
    stages.extend_from_slice(b"100644 ");
    stages.extend_from_slice(oid_a);
    stages.extend_from_slice(b" 0\tduplicate.txt\x00100755 ");
    stages.extend_from_slice(oid_b);
    stages.extend_from_slice(b" 0\tduplicate.txt\0");
    assert!(matches!(
        parse_stages(&stages, 40),
        Err(CommitErrorCode::MalformedOutput)
    ));

    let mut tree = Vec::new();
    tree.extend_from_slice(b"100644 blob ");
    tree.extend_from_slice(oid_a);
    tree.extend_from_slice(b"\tduplicate.txt\x00100755 blob ");
    tree.extend_from_slice(oid_b);
    tree.extend_from_slice(b"\tduplicate.txt\0");
    assert!(matches!(
        parse_tree(&tree, 40),
        Err(CommitErrorCode::MalformedOutput)
    ));
}

#[test]
fn stage_and_tree_codecs_reject_noncanonical_nul_framing() {
    let oid = b"1111111111111111111111111111111111111111";
    let mut stage = b"100644 ".to_vec();
    stage.extend_from_slice(oid);
    stage.extend_from_slice(b" 0\tfile.txt");
    let mut tree = b"100644 blob ".to_vec();
    tree.extend_from_slice(oid);
    tree.extend_from_slice(b"\tfile.txt");

    assert!(matches!(parse_stages(b"", 40), Ok(entries) if entries.is_empty()));
    assert!(matches!(parse_tree(b"", 40), Ok(entries) if entries.is_empty()));
    for record in [stage, tree] {
        let is_tree = record.starts_with(b"100644 blob");
        let parser = |bytes: &[u8]| {
            if is_tree {
                parse_tree(bytes, 40).map(|entries| entries.len())
            } else {
                parse_stages(bytes, 40).map(|entries| entries.len())
            }
        };
        assert_eq!(parser(b"\0"), Err(CommitErrorCode::MalformedOutput));
        let mut leading = vec![0];
        leading.extend_from_slice(&record);
        leading.push(0);
        assert_eq!(parser(&leading), Err(CommitErrorCode::MalformedOutput));
        let mut doubled = record.clone();
        doubled.extend_from_slice(b"\0\0");
        assert_eq!(parser(&doubled), Err(CommitErrorCode::MalformedOutput));
        assert_eq!(parser(&record), Err(CommitErrorCode::MalformedOutput));
        let mut canonical = record;
        canonical.push(0);
        assert_eq!(parser(&canonical), Ok(1));
    }
}

#[test]
fn status_codec_uses_closed_xy_shape_and_header_whitelists() {
    for &x in b".MTADRCU?" {
        for &y in b".MTADRCU?" {
            let ordinary = match x {
                b'.' => matches!(y, b'M' | b'T' | b'D'),
                b'M' | b'T' | b'A' => matches!(y, b'.' | b'M' | b'T' | b'D'),
                b'D' => y == b'.',
                _ => false,
            };
            assert_eq!(
                canonical_status_pair(StatusShape::Ordinary, x, y),
                ordinary,
                "ordinary {x:?}{y:?}"
            );
            let rename = x == b'R' && matches!(y, b'.' | b'M' | b'T' | b'D');
            assert_eq!(
                canonical_status_pair(StatusShape::Rename, x, y),
                rename,
                "rename {x:?}{y:?}"
            );
            let copy = x == b'C' && matches!(y, b'.' | b'M' | b'T' | b'D');
            assert_eq!(
                canonical_status_pair(StatusShape::Copy, x, y),
                copy,
                "copy {x:?}{y:?}"
            );
        }
    }

    let head = HeadAuthority {
        unborn: false,
        oid: b"1111111111111111111111111111111111111111".to_vec(),
        short: b"main".to_vec(),
        full_ref: b"refs/heads/main".to_vec(),
    };
    let mut unknown =
        b"# branch.oid 1111111111111111111111111111111111111111\0# branch.head main\0".to_vec();
    unknown.extend_from_slice(b"# branch.future value\0");
    assert!(matches!(
        parse_commit_status(&unknown, &head),
        Err(CommitErrorCode::MalformedOutput)
    ));
}

#[test]
fn special_selected_components_are_rejected_before_any_mutation() {
    let file_id = WorkspaceFileId {
        generation: 1,
        slot: 0,
        seal: 7,
    };
    for kind in [
        CommitSelectionKind::Added,
        CommitSelectionKind::Modified,
        CommitSelectionKind::TypeChanged,
        CommitSelectionKind::Renamed,
        CommitSelectionKind::Copied,
    ] {
        let row = ChecklistRow {
            public: CommitSelection {
                file_id,
                label: "special".into(),
                previous_label: None,
                kind,
                forced: false,
            },
            closure: vec![b"special".to_vec()],
            record: StatusRecord {
                shape: StatusShape::Untracked,
                x: b'?',
                y: b'?',
                sub: b"N...".to_vec(),
                head_mode: b"000000".to_vec(),
                index_mode: b"000000".to_vec(),
                worktree_mode: b"000000".to_vec(),
                head_oid: vec![b'0'; 40],
                index_oid: vec![b'0'; 40],
                path: b"special".to_vec(),
                previous: None,
            },
            optional_kind: kind,
            worktree_mode: None,
        };
        let checklist = StoredChecklist {
            id: IndexSnapshotId {
                generation: 0,
                slot: 0,
                seal: 0,
            },
            authority: IndexAuthority {
                head: HeadAuthority {
                    unborn: false,
                    oid: vec![b'1'; 40],
                    short: b"main".to_vec(),
                    full_ref: b"refs/heads/main".to_vec(),
                },
                status_raw: Vec::new(),
                stage_raw: Vec::new(),
                tree_raw: Vec::new(),
                records: Vec::new(),
                stages: Vec::new(),
                tree: Vec::new(),
                workspace_generation: 1,
            },
            optional: vec![row],
        };
        assert!(matches!(
            resolve_selected(&checklist, &[file_id]),
            Err(CommitErrorCode::InvalidSelection)
        ));
    }
}

#[test]
fn selected_copy_components_share_only_the_source_invariant() {
    let oid = |byte: u8| vec![byte; 40];
    let head = HeadAuthority {
        unborn: false,
        oid: oid(b'a'),
        short: b"master".to_vec(),
        full_ref: b"refs/heads/master".to_vec(),
    };
    let source = StageEntry {
        mode: b"100644".to_vec(),
        oid: oid(b'1'),
        path: b"source.txt".to_vec(),
    };
    let staged_copy = |path: &[u8], object: u8| StageEntry {
        mode: b"100644".to_vec(),
        oid: oid(object),
        path: path.to_vec(),
    };
    let copy_record = |path: &[u8]| StatusRecord {
        shape: StatusShape::Copy,
        x: b'C',
        y: b'M',
        sub: b"N...".to_vec(),
        head_mode: b"100644".to_vec(),
        index_mode: b"100644".to_vec(),
        worktree_mode: b"100644".to_vec(),
        head_oid: oid(b'1'),
        index_oid: oid(b'1'),
        path: path.to_vec(),
        previous: Some(b"source.txt".to_vec()),
    };
    let final_record = |path: &[u8]| StatusRecord {
        shape: StatusShape::Ordinary,
        x: b'A',
        y: b'.',
        sub: b"N...".to_vec(),
        head_mode: b"000000".to_vec(),
        index_mode: b"100644".to_vec(),
        worktree_mode: b"100644".to_vec(),
        head_oid: oid(b'0'),
        index_oid: oid(b'2'),
        path: path.to_vec(),
        previous: None,
    };
    let make_row = |slot: u32, path: &[u8]| ChecklistRow {
        public: CommitSelection {
            file_id: WorkspaceFileId {
                generation: 1,
                slot,
                seal: u64::from(slot),
            },
            label: String::new(),
            previous_label: None,
            kind: CommitSelectionKind::Modified,
            forced: false,
        },
        closure: vec![path.to_vec()],
        record: copy_record(path),
        optional_kind: CommitSelectionKind::Modified,
        worktree_mode: Some(b"100644".to_vec()),
    };
    let rows = [make_row(1, b"copy-one.txt"), make_row(2, b"copy-two.txt")];
    let selected = vec![&rows[0], &rows[1]];
    let a = IndexAuthority {
        head: head.clone(),
        status_raw: Vec::new(),
        stage_raw: Vec::new(),
        tree_raw: Vec::new(),
        records: vec![copy_record(b"copy-one.txt"), copy_record(b"copy-two.txt")],
        stages: vec![
            staged_copy(b"copy-one.txt", b'1'),
            staged_copy(b"copy-two.txt", b'1'),
            source.clone(),
        ],
        tree: Vec::new(),
        workspace_generation: 1,
    };
    let b = IndexAuthority {
        head,
        status_raw: Vec::new(),
        stage_raw: Vec::new(),
        tree_raw: Vec::new(),
        records: vec![final_record(b"copy-one.txt"), final_record(b"copy-two.txt")],
        stages: vec![
            staged_copy(b"copy-one.txt", b'2'),
            staged_copy(b"copy-two.txt", b'2'),
            source,
        ],
        tree: Vec::new(),
        workspace_generation: 2,
    };
    let paths = vec![b"copy-one.txt".to_vec(), b"copy-two.txt".to_vec()];
    assert_eq!(validate_transition(&a, &b, &selected, &paths), Ok(()));

    let mut source_drift = b.clone();
    source_drift.records.push(StatusRecord {
        shape: StatusShape::Ordinary,
        x: b'.',
        y: b'M',
        sub: b"N...".to_vec(),
        head_mode: b"100644".to_vec(),
        index_mode: b"100644".to_vec(),
        worktree_mode: b"100644".to_vec(),
        head_oid: oid(b'1'),
        index_oid: oid(b'1'),
        path: b"source.txt".to_vec(),
        previous: None,
    });
    assert_eq!(
        validate_transition(&a, &source_drift, &selected, &paths),
        Err(CommitErrorCode::ChangedDuringRead)
    );

    let mut mode_flip = b.clone();
    mode_flip
        .stages
        .iter_mut()
        .find(|entry| entry.path == b"copy-one.txt")
        .expect("copy destination")
        .mode = b"100755".to_vec();
    assert_eq!(
        validate_transition(&a, &mode_flip, &selected, &paths),
        Err(CommitErrorCode::ChangedDuringRead)
    );

    let overlap = [make_row(3, b"copy-one.txt"), make_row(4, b"copy-one.txt")];
    assert_eq!(
        validate_transition(&a, &b, &[&overlap[0], &overlap[1]], &paths),
        Err(CommitErrorCode::InvalidSelection)
    );
}

#[tokio::test]
async fn sha256_repository_completes_checklist_prepare_and_commit() {
    let repo = match Repo::try_sha256() {
        Ok(repo) => repo,
        Err(reason) => {
            eprintln!("SKIP sha256 repository E2E: {reason}");
            return;
        }
    };
    assert_eq!(
        run_git_output(repo.path(), &["rev-parse", "--show-object-format"]),
        b"sha256\n"
    );
    fs::write(repo.path().join("tracked.txt"), "sha256 change\n").expect("sha256 worktree change");
    let base = run_git_output(repo.path(), &["rev-parse", "HEAD"])
        .strip_suffix(b"\n")
        .expect("sha256 base newline")
        .to_vec();
    assert!(valid_nonzero_oid(&base, 64));
    let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("sha256 workspace"));
    workspace
        .refresh(CancellationToken::new())
        .await
        .expect("sha256 workspace refresh");
    let (_read_dir, read, read_log) = proof_read_recorder(repo.path(), &base, "ok");
    let (mutation_dir, mutation, _mutation_argv, _mutation_input) = mutation_recorder();
    let trusted =
        TrustedGitService::new_with_executables_for_test(repo.path(), workspace, mutation, read)
            .expect("sha256 trusted service");
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("sha256 checklist");
    assert_eq!(checklist.optional.len(), 1);
    let prepared = trusted
        .prepare(
            checklist.id,
            vec![checklist.optional[0].file_id],
            CancellationToken::new(),
        )
        .await
        .prepared
        .expect("sha256 prepared");
    {
        let state = trusted
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let stored = state.prepared.as_ref().expect("stored sha256 B");
        assert!(valid_nonzero_oid(&stored.authority.head.oid, 64));
        assert!(
            stored
                .authority
                .records
                .iter()
                .all(|record| { record.head_oid.len() == 64 && record.index_oid.len() == 64 })
        );
        assert!(
            stored
                .authority
                .stages
                .iter()
                .all(|stage| valid_nonzero_oid(&stage.oid, 64))
        );
        assert!(
            stored
                .authority
                .tree
                .iter()
                .all(|entry| valid_nonzero_oid(&entry.oid, 64))
        );
    }
    let provider = Arc::new(vega_runtime::MockProvider::new(vec![
        vega_runtime::ScriptStep::text("test: sha256 commit"),
        vega_runtime::ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }]),
    ]));
    let draft = trusted
        .draft(
            prepared.id,
            "mock-sha256".into(),
            provider.clone(),
            CancellationToken::new(),
        )
        .await
        .expect("sha256 mock draft");
    assert_eq!(provider.requests().len(), 1);
    assert!(provider.requests()[0].tools.is_empty());
    assert_eq!(provider.requests()[0].max_tokens, Some(256));
    let completion = trusted
        .commit(
            prepared.id,
            draft.text().to_owned(),
            CancellationToken::new(),
        )
        .await;
    assert_eq!(completion.outcome, CommitOutcome::Committed);
    let oid = run_git_output(repo.path(), &["rev-parse", "HEAD"]);
    let oid = oid.strip_suffix(b"\n").expect("sha256 oid newline");
    assert!(valid_nonzero_oid(oid, 64));
    assert_eq!(
        fs::read(mutation_dir.path().join("mutation-attempts"))
            .expect("sha256 add and commit attempts"),
        b"xx"
    );
    let reads = read_invocations(&read_log);
    assert!(reads.iter().flatten().any(|argument| {
        argument.len() == 66 && argument.ends_with(b"^@") && valid_nonzero_oid(&argument[..64], 64)
    }));
    assert!(reads.iter().any(|invocation| {
        invocation.iter().any(|argument| argument == b"ls-tree")
            && invocation.iter().any(|argument| argument == oid)
    }));
    assert_terminal_workspace(
        &trusted,
        completion.workspace.as_ref().expect("sha256 workspace"),
    );
}
