use super::*;

#[derive(Clone)]
enum CapturedCommitEvent {
    Prepare(CommitPrepareRequested),
    Draft(CommitDraftRequested),
    Commit(CommitRequested),
    Close,
}
#[gpui_kit::test]
async fn commit_panel_accepts_canonical_mixed_staged_and_unstaged_identity(
    cx: &mut gpui_kit::TestAppContext,
) {
    let repo = diff_controller_repo();
    run_fixture_git(repo.path(), &["add", "--", "tracked.rs"]);
    fs::write(
        repo.path().join("tracked.rs"),
        "fn base() {}\nfn changed() {}\nfn later() {}\n",
    )
    .expect("mixed worktree update");
    let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
    let service = TrustedGitService::new(repo.path(), workspace.clone()).expect("service");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let checklist = runtime.block_on(async {
        workspace
            .refresh(tokio_util::sync::CancellationToken::new())
            .await
            .expect("refresh");
        service
            .open_checklist(tokio_util::sync::CancellationToken::new())
            .await
            .expect("mixed checklist")
    });
    assert_eq!(checklist.staged.len(), 1);
    assert_eq!(checklist.optional.len(), 1);
    assert_eq!(checklist.staged[0].file_id, checklist.optional[0].file_id);
    let panel = cx.new(|cx| CommitPanel::new("thread".into(), "project".into(), cx));
    panel.update(cx, |panel, cx| {
        assert!(panel.request_open(cx));
        assert!(panel.apply_checklist(checklist, cx));
    });
}

#[gpui_kit::test]
async fn commit_panel_real_key_handlers_are_scoped_and_first_wins(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(|cx| {
        cx.set_global(Theme::light());
        vega_ui::init(cx);
    });
    let repo = diff_controller_repo();
    run_fixture_git(repo.path(), &["add", "--", "tracked.rs"]);
    fs::write(repo.path().join("optional.rs"), "fn optional() {}\n").expect("optional fixture");
    let workspace = Arc::new(GitWorkspaceService::new(repo.path()).expect("workspace"));
    let service =
        Arc::new(TrustedGitService::new(repo.path(), workspace.clone()).expect("trusted service"));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let checklist = runtime.block_on(async {
        workspace
            .refresh(tokio_util::sync::CancellationToken::new())
            .await
            .expect("refresh");
        service
            .open_checklist(tokio_util::sync::CancellationToken::new())
            .await
            .expect("checklist")
    });
    assert!(!checklist.staged.is_empty());
    assert_eq!(checklist.optional.len(), 1);

    let panel = cx.new(|cx| CommitPanel::new("thread".into(), "project".into(), cx));
    let events = Arc::new(Mutex::new(Vec::<CapturedCommitEvent>::new()));
    let window_events = events.clone();
    let root = panel.clone();
    let window = cx
        .update(|cx| {
            cx.open_window(Default::default(), move |_, cx| {
                let events_prepare = window_events.clone();
                let events_draft = window_events.clone();
                let events_commit = window_events.clone();
                let events_close = window_events.clone();
                cx.new(|cx| {
                    cx.subscribe(&root, move |_, _, event: &CommitPrepareRequested, _| {
                        events_prepare
                            .lock()
                            .expect("events")
                            .push(CapturedCommitEvent::Prepare(event.clone()));
                    })
                    .detach();
                    cx.subscribe(&root, move |_, _, event: &CommitDraftRequested, _| {
                        events_draft
                            .lock()
                            .expect("events")
                            .push(CapturedCommitEvent::Draft(event.clone()));
                    })
                    .detach();
                    cx.subscribe(&root, move |_, _, event: &CommitRequested, _| {
                        events_commit
                            .lock()
                            .expect("events")
                            .push(CapturedCommitEvent::Commit(event.clone()));
                    })
                    .detach();
                    cx.subscribe(&root, move |_, _, _event: &CommitPanelClosed, _| {
                        events_close
                            .lock()
                            .expect("events")
                            .push(CapturedCommitEvent::Close);
                    })
                    .detach();
                    CommitPanelHarness { panel: root }
                })
            })
        })
        .expect("commit key window");
    window
        .update(cx, |_, window, cx| {
            assert!(panel.update(cx, |panel, cx| panel.request_open(cx)));
            assert!(panel.update(cx, |panel, cx| {
                panel.apply_checklist(checklist.clone(), cx)
            }));
            let focus = panel.read(cx).focus_handle(cx);
            focus.focus(window, cx);
        })
        .expect("open checklist");

    // Space at Cancel is inert; Tab skips the forced staged row and lands
    // on the sole optional worktree row.
    cx.simulate_keystrokes(window.into(), "space tab space cmd-enter cmd-enter");
    let prepare = events
        .lock()
        .expect("events")
        .iter()
        .filter_map(|event| match event {
            CapturedCommitEvent::Prepare(request) => Some(request.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(prepare.len(), 1, "prepare is exact first-wins");
    assert_eq!(prepare[0].selected.len(), 1, "optional Space toggles once");
    let completion = runtime.block_on(service.prepare(
        prepare[0].snapshot_id,
        prepare[0].selected.clone(),
        tokio_util::sync::CancellationToken::new(),
    ));
    let prepared = completion.prepared.expect("prepared authority");
    assert!(panel.update(cx, |panel, cx| {
        panel.finish_prepare(prepare[0].operation_id, Ok(prepared.clone()), cx)
    }));
    cx.run_until_parked();
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.focused_control()),
        vega_ui::commit_panel::CommitPanelFocus::Cancel
    );

    // Editor Enter remains newline and emits no draft. Generate Enter and
    // Space each emit exactly once; repeating the same key while pending
    // cannot duplicate the operation.
    cx.simulate_keystrokes(window.into(), "tab enter");
    assert!(
        panel
            .read_with(cx, |panel, cx| panel.commit_message(cx))
            .contains('\n')
    );
    assert_eq!(
        events
            .lock()
            .expect("events")
            .iter()
            .filter(|event| matches!(event, CapturedCommitEvent::Draft(_)))
            .count(),
        0
    );
    cx.simulate_keystrokes(window.into(), "tab enter enter space");
    let first_draft = events
        .lock()
        .expect("events")
        .iter()
        .find_map(|event| match event {
            CapturedCommitEvent::Draft(request) => Some(request.clone()),
            _ => None,
        })
        .expect("Enter draft");
    assert_eq!(
        events
            .lock()
            .expect("events")
            .iter()
            .filter(|event| matches!(event, CapturedCommitEvent::Draft(_)))
            .count(),
        1
    );
    let provider = Arc::new(vega_runtime::MockProvider::new(vec![
        vega_runtime::ScriptStep::events(vec![
            vega_runtime::ProviderEvent::TextDelta("feat: generated".into()),
            vega_runtime::ProviderEvent::Done {
                stop_reason: vega_runtime::StopReason::End,
            },
        ]),
    ]));
    let draft = runtime
        .block_on(service.draft(
            prepared.id,
            "mock".into(),
            provider,
            tokio_util::sync::CancellationToken::new(),
        ))
        .expect("mock draft");
    assert!(panel.update(cx, |panel, cx| {
        panel.finish_draft(first_draft.operation_id, Ok(draft), cx)
    }));
    cx.simulate_keystrokes(window.into(), "space space");
    assert_eq!(
        events
            .lock()
            .expect("events")
            .iter()
            .filter(|event| matches!(event, CapturedCommitEvent::Draft(_)))
            .count(),
        2,
        "Generate Space is first-wins"
    );
    let second_draft = events
        .lock()
        .expect("events")
        .iter()
        .filter_map(|event| match event {
            CapturedCommitEvent::Draft(request) => Some(request.clone()),
            _ => None,
        })
        .nth(1)
        .expect("Space draft");
    let provider = Arc::new(vega_runtime::MockProvider::new(vec![
        vega_runtime::ScriptStep::events(vec![
            vega_runtime::ProviderEvent::TextDelta("feat: generated".into()),
            vega_runtime::ProviderEvent::Done {
                stop_reason: vega_runtime::StopReason::End,
            },
        ]),
    ]));
    let draft = runtime
        .block_on(service.draft(
            prepared.id,
            "mock".into(),
            provider,
            tokio_util::sync::CancellationToken::new(),
        ))
        .expect("second mock draft");
    assert!(panel.update(cx, |panel, cx| {
        panel.finish_draft(second_draft.operation_id, Ok(draft), cx)
    }));
    cx.simulate_keystrokes(window.into(), "tab cmd-enter cmd-enter escape escape");
    let events = events.lock().expect("events");
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, CapturedCommitEvent::Commit(_)))
            .count(),
        1,
        "commit is exact first-wins"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, CapturedCommitEvent::Close))
            .count(),
        1,
        "Esc close is exact first-wins"
    );
    let commit = events.iter().find_map(|event| match event {
        CapturedCommitEvent::Commit(request) => Some(request),
        _ => None,
    });
    assert!(commit.is_some_and(|request| request.prepared_id == prepared.id));
}
