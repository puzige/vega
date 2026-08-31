use super::*;

#[test]
fn summary_escape_is_chunk_independent_and_bounded() {
    assert_eq!(
        escape_summary(b"a\xff\0b").expect("escape").rendered,
        "a\\xFF\\x00b"
    );
    assert_eq!(
        escape_summary(b"a\0\xff\r\n\tb")
            .expect("binary escape")
            .rendered,
        "a\\x00\\xFF\\x0D\n\tb"
    );
    for (raw, raw_overflow, expected_truncated) in [
        (vec![b'a'; SUMMARY_LIMIT], false, false),
        (vec![b'a'; SUMMARY_LIMIT + 1], false, true),
        (vec![b'a'; SUMMARY_LIMIT], true, true),
        (vec![0xff; SUMMARY_LIMIT / 2], false, true),
    ] {
        let (summary, truncated) =
            truncate_summary(escape_summary(&raw).expect("summary"), raw_overflow);
        assert_eq!(truncated, expected_truncated);
        assert!(summary.len() <= SUMMARY_LIMIT);
        if truncated {
            assert!(summary.ends_with(std::str::from_utf8(SUMMARY_MARKER).expect("marker")));
            assert_eq!(summary.matches("[vega-summary truncated=true]").count(), 1);
        }
    }
}

#[test]
fn summary_rendered_exact_plus_one_escape_and_multibyte_boundaries() {
    let marker = std::str::from_utf8(SUMMARY_MARKER).expect("marker");
    for length in [SUMMARY_LIMIT - 1, SUMMARY_LIMIT, SUMMARY_LIMIT + 1] {
        let raw = "a".repeat(length);
        let (summary, truncated) = truncate_summary(
            escape_summary(raw.as_bytes()).expect("ASCII summary"),
            false,
        );
        assert_eq!(truncated, length > SUMMARY_LIMIT);
        assert!(summary.len() <= SUMMARY_LIMIT);
        if truncated {
            let payload = summary.strip_suffix(marker).expect("plus-one marker");
            assert_eq!(payload.len(), SUMMARY_LIMIT - SUMMARY_MARKER.len());
        } else {
            assert_eq!(summary, raw);
        }
    }

    let literal_slash = format!("{}\\", "s".repeat(SUMMARY_LIMIT - 1));
    let (literal_slash, truncated) = truncate_summary(
        escape_summary(literal_slash.as_bytes()).expect("literal slash summary"),
        false,
    );
    assert!(!truncated);
    assert_eq!(literal_slash.len(), SUMMARY_LIMIT);
    assert!(literal_slash.ends_with('\\'));

    let escaped_exact = escape_summary(&vec![0xff; SUMMARY_LIMIT / 4]).expect("escaped exact");
    assert_eq!(escaped_exact.rendered.len(), SUMMARY_LIMIT);
    let (escaped_exact, truncated) = truncate_summary(escaped_exact, false);
    assert!(!truncated);
    assert_eq!(escaped_exact.len(), SUMMARY_LIMIT);

    let mut escaped_plus_one_raw = vec![0xff; SUMMARY_LIMIT / 4];
    escaped_plus_one_raw.push(b'a');
    let escaped_plus_one = escape_summary(&escaped_plus_one_raw).expect("escaped plus one");
    assert_eq!(escaped_plus_one.rendered.len(), SUMMARY_LIMIT + 1);
    let (escaped_plus_one, truncated) = truncate_summary(escaped_plus_one, false);
    assert!(truncated);
    let escaped_payload = escaped_plus_one
        .strip_suffix(marker)
        .expect("escaped marker");
    assert_eq!(escaped_payload.len() % 4, 0);
    let (escape_chunks, escape_remainder) = escaped_payload.as_bytes().as_chunks::<4>();
    assert!(escape_remainder.is_empty());
    assert!(
        escape_chunks.iter().all(|chunk| chunk == b"\\xFF"),
        "partial escape retained before marker"
    );

    let target = SUMMARY_LIMIT - SUMMARY_MARKER.len();
    for literal_suffix in ["\\", "\\x", "\\xF"] {
        let mut raw = "l".repeat(target - literal_suffix.len());
        raw.push_str(literal_suffix);
        raw.push_str(&"tail".repeat(10));
        let (summary, truncated) = truncate_summary(
            escape_summary(raw.as_bytes()).expect("literal suffix"),
            false,
        );
        assert!(truncated);
        let payload = summary.strip_suffix(marker).expect("literal marker");
        assert_eq!(payload.len(), target);
        assert!(payload.ends_with(literal_suffix));
        assert_eq!(summary.matches(marker).count(), 1);
        assert_eq!(summary.len(), SUMMARY_LIMIT);
    }

    for generated_cut in 1..=3 {
        let prefix_len = target - generated_cut;
        let mut raw = vec![b'g'; prefix_len];
        raw.push(0xff);
        raw.extend_from_slice(&[b't'; 40]);
        let (summary, truncated) =
            truncate_summary(escape_summary(&raw).expect("generated escape"), false);
        assert!(truncated);
        let payload = summary.strip_suffix(marker).expect("generated marker");
        assert_eq!(payload.len(), prefix_len);
        assert!(payload.bytes().all(|byte| byte == b'g'));
        assert_eq!(summary.matches(marker).count(), 1);
        assert!(summary.len() <= SUMMARY_LIMIT);
    }

    let mut generated_exact_target = vec![b'x'];
    generated_exact_target.extend(std::iter::repeat_n(0xff, (target - 1) / 4));
    generated_exact_target.extend_from_slice(&[b't'; 40]);
    let (generated_exact_target, truncated) = truncate_summary(
        escape_summary(&generated_exact_target).expect("generated exact target"),
        false,
    );
    assert!(truncated);
    let generated_exact_payload = generated_exact_target
        .strip_suffix(marker)
        .expect("generated exact marker");
    assert_eq!(generated_exact_payload.len(), target);
    assert!(generated_exact_payload.ends_with("\\xFF"));
    assert_eq!(generated_exact_target.len(), SUMMARY_LIMIT);

    let mut multibyte_raw = "m".repeat(target - 1);
    multibyte_raw.push('é');
    multibyte_raw.push_str(&"tail".repeat(10));
    let (multibyte, truncated) = truncate_summary(
        escape_summary(multibyte_raw.as_bytes()).expect("multibyte summary"),
        false,
    );
    assert!(truncated);
    let multibyte_payload = multibyte.strip_suffix(marker).expect("multibyte marker");
    assert_eq!(multibyte_payload.len(), target - 1);
    assert!(multibyte_payload.chars().all(|character| character == 'm'));

    let literal_at_marker_cut = format!("{}\\{}", "q".repeat(target - 1), "tail".repeat(10));
    let (literal_at_marker_cut, truncated) = truncate_summary(
        escape_summary(literal_at_marker_cut.as_bytes()).expect("literal at marker cut"),
        false,
    );
    assert!(truncated);
    let payload = literal_at_marker_cut
        .strip_suffix(marker)
        .expect("literal marker cut marker");
    assert_eq!(payload.len(), target);
    assert!(payload.ends_with('\\'));
    assert_eq!(literal_at_marker_cut.len(), SUMMARY_LIMIT);
    assert_eq!(literal_at_marker_cut.matches(marker).count(), 1);
}

#[test]
fn commit_summary_binary_bytes_are_deterministically_escaped() {
    let repo = Repo::new();
    let fixture = tempfile::tempdir().expect("binary summary fixture");
    let script = fixture.path().join("summary-git");
    fs::write(
            &script,
            "#!/bin/sh\nexec python3 -c 'import sys; sys.stdout.buffer.write(b\"a\\x00\\xff\\r\\n\\tb\")'\n",
        )
        .expect("binary summary script");
    let mut permissions = fs::metadata(&script)
        .expect("script metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&script, permissions).expect("script executable");
    let runner = test_runner(repo.path());
    let runner = Runner::new(runner.root, runner.identity, Some(script));
    let output = runner
        .run_commit_summary(SUMMARY_LIMIT, &CancellationToken::new())
        .expect("binary summary");
    assert_eq!(
        escape_summary(&output.stdout)
            .expect("escaped binary summary")
            .rendered,
        "a\\x00\\xFF\\x0D\n\tb"
    );
}

#[tokio::test]
async fn provider_draft_uses_strict_done_eof_grammar_and_redacted_output() {
    let provider = Arc::new(vega_runtime::MockProvider::new(vec![
        vega_runtime::ScriptStep::text("feat: safe"),
        vega_runtime::ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }]),
    ]));
    let draft = collect_draft(
        provider.clone(),
        ChatRequest {
            model: "mock".into(),
            messages: Vec::new(),
            tools: Vec::new(),
            max_tokens: Some(256),
        },
        CancellationToken::new(),
    )
    .await
    .expect("draft");
    assert_eq!(draft, "feat: safe");
    assert_eq!(provider.requests().len(), 1);
    let invalid = Arc::new(vega_runtime::MockProvider::new(vec![
        vega_runtime::ScriptStep::text("partial"),
    ]));
    assert_eq!(
        collect_draft(invalid, ChatRequest::default(), CancellationToken::new()).await,
        Err(CommitErrorCode::DraftFailed)
    );
}

#[tokio::test]
async fn provider_draft_grammar_table_is_closed_and_usage_star_is_accepted() {
    use vega_runtime::ScriptStep;

    let success = Arc::new(vega_runtime::MockProvider::new(vec![
        ScriptStep::text("x".repeat(MESSAGE_LIMIT)),
        ScriptStep::events(vec![
            ProviderEvent::Usage {
                input: 1,
                output: 2,
                cache_read: 3,
                cache_write: 4,
            },
            ProviderEvent::Usage {
                input: 5,
                output: 6,
                cache_read: 7,
                cache_write: 8,
            },
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ]),
    ]));
    assert_eq!(
        collect_draft(success, ChatRequest::default(), CancellationToken::new())
            .await
            .expect("Usage* grammar"),
        "x".repeat(MESSAGE_LIMIT)
    );

    let invalid_scripts = vec![
        vec![],
        vec![ScriptStep::text("partial")],
        vec![ScriptStep::events(vec![ProviderEvent::Done {
            stop_reason: StopReason::End,
        }])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("text".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("text".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
            ProviderEvent::TextDelta("after done".into()),
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("text".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
            ProviderEvent::Usage {
                input: 1,
                output: 1,
                cache_read: 0,
                cache_write: 0,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("text".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::Length,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("text".into()),
            ProviderEvent::Usage {
                input: 1,
                output: 1,
                cache_read: 0,
                cache_write: 0,
            },
            ProviderEvent::TextDelta("late".into()),
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::ThinkingDelta("secret".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
        vec![ScriptStep::events(vec![
            ProviderEvent::ToolUse {
                id: "id".into(),
                name: "tool".into(),
                input_json: "{}".into(),
            },
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
        vec![
            ScriptStep::text("partial"),
            ScriptStep::Error {
                status: Some(500),
                message: "provider payload".into(),
                retryable: false,
            },
        ],
        vec![ScriptStep::Error {
            status: Some(500),
            message: "provider setup payload".into(),
            retryable: false,
        }],
        vec![
            ScriptStep::events(vec![
                ProviderEvent::TextDelta("text".into()),
                ProviderEvent::Done {
                    stop_reason: StopReason::End,
                },
            ]),
            ScriptStep::Error {
                status: Some(500),
                message: "provider after done payload".into(),
                retryable: false,
            },
        ],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta("nul\0text".into()),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
        vec![
            ScriptStep::text("x".repeat(MESSAGE_LIMIT + 1)),
            ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }]),
        ],
    ];
    for (case, script) in invalid_scripts.into_iter().enumerate() {
        let provider = Arc::new(vega_runtime::MockProvider::new(script));
        assert_eq!(
            collect_draft(provider, ChatRequest::default(), CancellationToken::new()).await,
            Err(CommitErrorCode::DraftFailed),
            "invalid grammar case {case}"
        );
    }
    assert_eq!(
        checked_draft_len(usize::MAX, 1),
        Err(CommitErrorCode::DraftFailed)
    );
}

#[tokio::test]
async fn draft_deadline_covers_setup_pre_done_and_post_done_stalls() {
    #[derive(Clone, Copy)]
    enum Phase {
        Setup,
        PreDone,
        PostDone,
    }
    struct StallingProvider(Phase);
    impl Provider for StallingProvider {
        fn chat_stream(
            &self,
            _request: ChatRequest,
            _cancel: CancellationToken,
        ) -> futures::future::BoxFuture<
            'static,
            Result<vega_runtime::EventStream, vega_runtime::VegaError>,
        > {
            match self.0 {
                Phase::Setup => Box::pin(std::future::pending()),
                Phase::PreDone => Box::pin(async {
                    let stream =
                        futures::stream::iter(vec![Ok(ProviderEvent::TextDelta("partial".into()))])
                            .chain(futures::stream::pending());
                    Ok(Box::pin(stream) as vega_runtime::EventStream)
                }),
                Phase::PostDone => Box::pin(async {
                    let stream = futures::stream::iter(vec![
                        Ok(ProviderEvent::TextDelta("complete".into())),
                        Ok(ProviderEvent::Done {
                            stop_reason: StopReason::End,
                        }),
                    ])
                    .chain(futures::stream::pending());
                    Ok(Box::pin(stream) as vega_runtime::EventStream)
                }),
            }
        }
    }
    for phase in [Phase::Setup, Phase::PreDone, Phase::PostDone] {
        let started = Instant::now();
        assert_eq!(
            collect_draft_with_deadline(
                Arc::new(StallingProvider(phase)),
                ChatRequest::default(),
                CancellationToken::new(),
                Duration::from_millis(25),
            )
            .await,
            Err(CommitErrorCode::DraftFailed)
        );
        assert!(started.elapsed() >= Duration::from_millis(20));
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}

#[tokio::test]
async fn draft_cancel_is_biased_at_setup_event_and_post_done_stalls() {
    #[derive(Clone, Copy)]
    enum Phase {
        Setup,
        PreDone,
        PostDone,
    }
    struct CancelStallProvider {
        phase: Phase,
        ready: Arc<tokio::sync::Notify>,
    }
    impl Provider for CancelStallProvider {
        fn chat_stream(
            &self,
            _request: ChatRequest,
            _cancel: CancellationToken,
        ) -> futures::future::BoxFuture<
            'static,
            Result<vega_runtime::EventStream, vega_runtime::VegaError>,
        > {
            let ready = self.ready.clone();
            match self.phase {
                Phase::Setup => Box::pin(async move {
                    ready.notify_one();
                    std::future::pending().await
                }),
                phase => Box::pin(async move {
                    let prefix = match phase {
                        Phase::PreDone => vec![Ok(ProviderEvent::TextDelta("partial".into()))],
                        Phase::PostDone => vec![
                            Ok(ProviderEvent::TextDelta("complete".into())),
                            Ok(ProviderEvent::Done {
                                stop_reason: StopReason::End,
                            }),
                        ],
                        Phase::Setup => unreachable!(),
                    };
                    let tail = futures::stream::once(async move {
                        ready.notify_one();
                        std::future::pending::<Result<ProviderEvent, vega_runtime::VegaError>>()
                            .await
                    });
                    Ok(Box::pin(futures::stream::iter(prefix).chain(tail))
                        as vega_runtime::EventStream)
                }),
            }
        }
    }
    for phase in [Phase::Setup, Phase::PreDone, Phase::PostDone] {
        let ready = Arc::new(tokio::sync::Notify::new());
        let cancel = CancellationToken::new();
        let worker = tokio::spawn(collect_draft_with_deadline(
            Arc::new(CancelStallProvider {
                phase,
                ready: ready.clone(),
            }),
            ChatRequest::default(),
            cancel.clone(),
            Duration::from_secs(1),
        ));
        tokio::time::timeout(Duration::from_secs(1), ready.notified())
            .await
            .expect("stall reached");
        cancel.cancel();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), worker)
                .await
                .expect("cancel bounded")
                .expect("draft task"),
            Err(CommitErrorCode::DraftFailed)
        );
    }
}

#[tokio::test]
async fn draft_cancel_wins_when_cancel_and_provider_branch_are_both_ready() {
    #[derive(Clone, Copy)]
    enum Phase {
        Setup,
        PreDone,
        PostDone,
    }
    struct GatedProvider {
        phase: Phase,
        ready: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
        provider_branch_selected: Arc<AtomicUsize>,
    }
    impl Provider for GatedProvider {
        fn chat_stream(
            &self,
            _request: ChatRequest,
            _cancel: CancellationToken,
        ) -> futures::future::BoxFuture<
            'static,
            Result<vega_runtime::EventStream, vega_runtime::VegaError>,
        > {
            let ready = self.ready.clone();
            let release = self.release.clone();
            let selected = self.provider_branch_selected.clone();
            match self.phase {
                Phase::Setup => Box::pin(async move {
                    ready.notify_one();
                    release.notified().await;
                    selected.fetch_add(1, Ordering::SeqCst);
                    Ok(Box::pin(futures::stream::empty()) as vega_runtime::EventStream)
                }),
                Phase::PreDone => Box::pin(async move {
                    let gated_done = futures::stream::once(async move {
                        ready.notify_one();
                        release.notified().await;
                        selected.fetch_add(1, Ordering::SeqCst);
                        Ok(ProviderEvent::Done {
                            stop_reason: StopReason::End,
                        })
                    });
                    Ok(Box::pin(
                        futures::stream::iter(vec![Ok(ProviderEvent::TextDelta("partial".into()))])
                            .chain(gated_done),
                    ) as vega_runtime::EventStream)
                }),
                Phase::PostDone => Box::pin(async move {
                    let gated_eof = futures::stream::unfold((), move |()| {
                        let ready = ready.clone();
                        let release = release.clone();
                        let selected = selected.clone();
                        async move {
                            ready.notify_one();
                            release.notified().await;
                            selected.fetch_add(1, Ordering::SeqCst);
                            None::<(Result<ProviderEvent, vega_runtime::VegaError>, ())>
                        }
                    });
                    Ok(Box::pin(
                        futures::stream::iter(vec![
                            Ok(ProviderEvent::TextDelta("complete".into())),
                            Ok(ProviderEvent::Done {
                                stop_reason: StopReason::End,
                            }),
                        ])
                        .chain(gated_eof),
                    ) as vega_runtime::EventStream)
                }),
            }
        }
    }
    for phase in [Phase::Setup, Phase::PreDone, Phase::PostDone] {
        let ready = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let selected = Arc::new(AtomicUsize::new(0));
        let cancel = CancellationToken::new();
        let worker = tokio::spawn(collect_draft_with_deadline(
            Arc::new(GatedProvider {
                phase,
                ready: ready.clone(),
                release: release.clone(),
                provider_branch_selected: selected.clone(),
            }),
            ChatRequest::default(),
            cancel.clone(),
            Duration::from_secs(1),
        ));
        tokio::time::timeout(Duration::from_secs(1), ready.notified())
            .await
            .expect("provider branch reached gate");
        // There is deliberately no await between these operations. The
        // worker's next poll observes both select branches as ready.
        cancel.cancel();
        release.notify_one();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), worker)
                .await
                .expect("biased cancel bounded")
                .expect("draft task"),
            Err(CommitErrorCode::DraftFailed)
        );
        assert_eq!(
            selected.load(Ordering::SeqCst),
            0,
            "provider branch won a simultaneous-ready race"
        );
    }
}

#[tokio::test]
async fn commit_draft_request_matches_frozen_literals_for_both_truncation_flags() {
    const FIXTURE_SUMMARY: &str = "fixture staged summary";
    const EXPECTED_SYSTEM: &str = "Generate one concise Git commit message for the exact staged diff. Return only the commit message text. Do not call tools.";
    for truncated in [false, true] {
        let (_repo, _recorder, trusted, prepared, _argv, _input) =
            staged_service_with_recorder().await;
        {
            let mut state = trusted
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let stored = state.prepared.as_mut().expect("stored prepared");
            stored.summary = FIXTURE_SUMMARY.into();
            stored.summary_truncated = truncated;
        }
        let provider = Arc::new(vega_runtime::MockProvider::new(vec![
            vega_runtime::ScriptStep::text("feat: exact request"),
            vega_runtime::ScriptStep::events(vec![ProviderEvent::Done {
                stop_reason: StopReason::End,
            }]),
        ]));
        let draft = trusted
            .draft(
                prepared.id,
                "commit-model-sentinel".into(),
                provider.clone(),
                CancellationToken::new(),
            )
            .await
            .expect("draft");
        assert!(draft.text() == "feat: exact request", "draft mismatch");
        let requests = provider.requests();
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert!(request.model == "commit-model-sentinel", "model mismatch");
        assert!(request.tools.is_empty(), "commit request advertised tools");
        assert_eq!(request.max_tokens, Some(256));
        assert_eq!(request.messages.len(), 2);
        assert!(
            request.messages[0] == ChatMessage::new(ChatRole::System, EXPECTED_SYSTEM),
            "system prompt mismatch"
        );
        let expected_user = format!(
            "Generate the commit message for the staged diff below.\ntruncated={}\n--- staged diff ---\n{FIXTURE_SUMMARY}",
            if truncated { "true" } else { "false" }
        );
        assert!(
            request.messages[1] == ChatMessage::new(ChatRole::User, expected_user),
            "user prompt mismatch"
        );
    }
}

#[tokio::test]
async fn failed_draft_keeps_prepared_authority_usable() {
    let (_repo, _recorder, trusted, prepared, argv, input) = staged_service_with_recorder().await;
    assert!(
        !argv.exists() && !input.exists(),
        "draft fixture mutated before provider"
    );
    let invalid = Arc::new(vega_runtime::MockProvider::new(vec![
        vega_runtime::ScriptStep::text("partial"),
    ]));
    assert_eq!(
        trusted
            .draft(
                prepared.id,
                "model".into(),
                invalid,
                CancellationToken::new(),
            )
            .await,
        Err(CommitErrorCode::DraftFailed)
    );
    let valid = Arc::new(vega_runtime::MockProvider::new(vec![
        vega_runtime::ScriptStep::text("feat: recovered draft"),
        vega_runtime::ScriptStep::events(vec![
            ProviderEvent::Usage {
                input: 1,
                output: 1,
                cache_read: 0,
                cache_write: 0,
            },
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ]),
    ]));
    assert_eq!(
        trusted
            .draft(prepared.id, "model".into(), valid, CancellationToken::new(),)
            .await
            .expect("recovered draft")
            .text(),
        "feat: recovered draft"
    );
    let state = trusted
        .state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    assert_eq!(
        state.prepared.as_ref().map(|stored| stored.id),
        Some(prepared.id)
    );
    assert!(!state.mutation_active);
    assert!(
        !argv.exists() && !input.exists(),
        "draft path started a Git mutation"
    );
}

#[tokio::test]
async fn summary_authority_change_after_capture_fails_before_provider() {
    let repo = Repo::new();
    fs::write(repo.path().join("staged.txt"), "staged\n").expect("staged file");
    fs::write(repo.path().join("outside.txt"), "base\n").expect("outside file");
    run_git(repo.path(), &["add", "staged.txt", "outside.txt"]);
    run_git(repo.path(), &["commit", "-qm", "summary base"]);
    fs::write(repo.path().join("staged.txt"), "staged changed\n").expect("staged change");
    run_git(repo.path(), &["add", "staged.txt"]);
    let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
    workspace
        .refresh(CancellationToken::new())
        .await
        .expect("workspace A");
    let (_gate, read, ready, release) = blocking_summary_reader();
    let trusted = Arc::new(
        TrustedGitService::new_with_executables_for_test(
            repo.path(),
            workspace,
            PathBuf::from(GIT),
            read,
        )
        .expect("trusted summary barrier"),
    );
    let checklist = trusted
        .open_checklist(CancellationToken::new())
        .await
        .expect("checklist");
    let provider = Arc::new(vega_runtime::MockProvider::new(vec![]));
    let worker = tokio::spawn({
        let trusted = trusted.clone();
        async move {
            trusted
                .prepare(checklist.id, Vec::new(), CancellationToken::new())
                .await
        }
    });
    wait_for_path(&ready).await;
    fs::write(repo.path().join("outside.txt"), "outside drift\n").expect("outside drift");
    run_git(repo.path(), &["add", "outside.txt"]);
    fs::write(release, b"release").expect("release summary");
    let completion = worker.await.expect("prepare worker");
    assert_eq!(completion.error, Some(CommitErrorCode::ChangedDuringRead));
    assert!(completion.prepared.is_none());
    assert!(completion.workspace.is_some());
    assert_eq!(
        provider.requests().len(),
        0,
        "provider must remain uncalled"
    );
}
