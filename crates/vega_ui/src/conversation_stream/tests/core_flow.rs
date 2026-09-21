use super::*;

#[gpui_kit::test]
async fn issue73_enabled_mcp_failure_is_visible_without_remote_content(cx: &mut TestAppContext) {
    let (_, stream, _) = open_controller_stream(cx, "mcp-unavailable");
    let diagnostic = vega_conversation::types::McpServerDiagnostic {
        server_id: "01J00000000000000000000000".into(),
        code: "authorization_required".into(),
    };
    stream.update(cx, |stream, cx| {
        stream.apply_mcp_unavailable(&[diagnostic], cx)
    });
    stream.update(cx, |stream, cx| stream.apply_agent_error(cx));
    let (warning, primary_error) = stream.read_with(cx, |stream, _| {
        (stream.mcp_warning.clone(), stream.controller_error.clone())
    });
    assert_eq!(primary_error.as_deref(), Some("执行未完成，可安全重试"));
    assert!(warning.as_deref().is_some_and(|message| {
        message.contains("MCP 服务器不可用")
            && message.contains("设置 → MCP")
            && !message.contains("authorization_required")
            && !message.contains("01J000")
    }));
    stream.update(cx, ConversationStream::begin_composer_run);
    let warning = stream.read_with(cx, |stream, _| stream.mcp_warning.clone());
    assert!(
        warning.is_none(),
        "new run must not inherit old MCP omission"
    );
}

#[gpui_kit::test]
async fn r57_plus_menu_permission_rows_emit_scoped_requests_without_optimistic_state(
    cx: &mut TestAppContext,
) {
    let global_escapes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed_escapes = global_escapes.clone();
    cx.update(|cx| {
        cx.on_action(move |_: &crate::settings::CloseSettings, _| {
            observed_escapes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        cx.bind_keys([gpui_kit::KeyBinding::new(
            "escape",
            crate::settings::CloseSettings,
            Some("VegaWindow"),
        )])
    });
    let (window, stream, events) = open_controller_stream(cx, "settings-thread");
    // R57 P2b: the bottom-row mode/permission dropdowns are gone, so the
    // request path is exercised through the `+` menu rows that replaced them
    // (P1). Rows: file(0) ask(1) plan(2) execute(3) readonly(4) confirm(5)
    // auto(6).
    click_composer_add(window, cx);
    cx.simulate_keystrokes(window.into(), "down down enter");
    assert_eq!(
        events.lock().expect("settings event capture").as_slice(),
        &[ThreadSettingsRequested {
            thread_id: "settings-thread".into(),
            mode: Some(ThreadMode::Plan),
            permission_mode: None,
        }]
    );

    click_composer_add(window, cx);
    cx.simulate_keystrokes(window.into(), "down down down down down down");
    cx.simulate_keystrokes(window.into(), "enter");
    assert_eq!(
        events.lock().expect("settings event capture").as_slice(),
        &[
            ThreadSettingsRequested {
                thread_id: "settings-thread".into(),
                mode: Some(ThreadMode::Plan),
                permission_mode: None,
            },
            ThreadSettingsRequested {
                thread_id: "settings-thread".into(),
                mode: None,
                permission_mode: Some(PermissionMode::Auto),
            },
        ]
    );

    // Esc in the open `+` menu closes it inside the menu scope and never
    // reaches the window-level settings binding.
    click_composer_add(window, cx);
    cx.simulate_keystrokes(window.into(), "escape");
    assert_eq!(global_escapes.load(std::sync::atomic::Ordering::SeqCst), 0);
    cx.simulate_keystrokes(window.into(), "escape");
    assert_eq!(global_escapes.load(std::sync::atomic::Ordering::SeqCst), 1);

    // No optimistic state: the displayed thread still holds the fixture's
    // authoritative values until the app applies a durable thread.
    let selected = stream.read_with(cx, |stream, _| {
        (stream.thread.mode, stream.thread.permission_mode)
    });
    assert_eq!(selected, (ThreadMode::Execute, PermissionMode::Confirm));
    stream.update(cx, ConversationStream::apply_controller_error);
    let selected = stream.read_with(cx, |stream, _| {
        (stream.thread.mode, stream.thread.permission_mode)
    });
    assert_eq!(selected, (ThreadMode::Execute, PermissionMode::Confirm));

    let mut persisted = permission_thread();
    persisted.id = "settings-thread".into();
    persisted.mode = ThreadMode::Plan;
    persisted.permission_mode = PermissionMode::Auto;
    stream.update(cx, |stream, cx| stream.apply_thread(persisted, cx));
    let selected = stream.read_with(cx, |stream, _| {
        (stream.thread.mode, stream.thread.permission_mode)
    });
    assert_eq!(selected, (ThreadMode::Plan, PermissionMode::Auto));
}

/// R59 rewrite of the R57 popover-exclusivity check.
///
/// R57 asserted that the `+` actions menu and the model menu are mutually
/// exclusive, and that the model menu keeps its fixed `MENU_MAX_WIDTH`. R59
/// makes the model button open the **tier slider** first, so the surviving
/// pair is the `+` menu versus the *picker* (whichever level it shows), and the
/// fixed width now belongs to the level-two list. Both directions are asserted
/// here, and the width check moves to the level-two layer in the R59 tests
/// below.
#[gpui_kit::test]
async fn r21_composer_popovers_are_exclusive_and_model_labels_keep_menu_width(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "composer-popovers");
    stream.update(cx, |stream, cx| {
        stream.apply_model_options(vec!["mock".into(), "deepseek-v4-flash".into()], cx);
        // R59 R1: level one is the tier slider, which only mounts for a model
        // that declares tiers (R57 R12). The exclusivity this test protects is
        // about the picker as a whole, so the fixture declares the capability
        // rather than asserting on an empty slider slot.
        let mut profile = ReasoningProfileProjection::unknown("owned", "mock");
        profile.support = ReasoningSupport::Optional;
        profile.efforts = vec!["low".into(), "medium".into(), "high".into()];
        profile.preference = ReasoningChoice::ProviderDefault;
        stream.apply_reasoning_profile(profile, cx);
    });
    cx.run_until_parked();

    let click = |selector: &'static str, cx: &mut TestAppContext| {
        let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
        let bounds = visual
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("missing {selector}"));
        visual.simulate_click(bounds.center(), gpui_kit::Modifiers::default());
        visual.run_until_parked();
    };
    let actions_menu_visible = |cx: &mut TestAppContext| {
        // `composer-actions-menu` is the always-mounted container (zero-height
        // while closed); the rows only exist in the frame while it is open.
        gpui_kit::VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("composer-action-file")
            .is_some()
    };
    let picker_visible = |cx: &mut TestAppContext| {
        let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
        visual.debug_bounds("composer-thinking-slider").is_some()
            || visual.debug_bounds("composer-model-menu").is_some()
    };

    // R59 R1: the model button opens the slider, not the list.
    click("composer-model", cx);
    assert!(
        picker_visible(cx),
        "the model button must mount a picker level"
    );
    assert_eq!(
        stream.read_with(cx, |stream, _| (
            stream.model_picker_level,
            stream.actions.visible(),
        )),
        (ModelPickerLevel::Slider, false)
    );

    // Opening the `+` menu closes the picker.
    click("composer-add", cx);
    assert!(
        !picker_visible(cx),
        "the `+` menu must close the model picker"
    );
    assert!(actions_menu_visible(cx), "the `+` menu is open");
    assert_eq!(
        stream.read_with(cx, |stream, _| (
            stream.model_picker_level,
            stream.actions.visible(),
        )),
        (ModelPickerLevel::Closed, true)
    );

    // Re-opening the picker closes the `+` menu.
    click("composer-model", cx);
    assert!(
        !actions_menu_visible(cx),
        "the model picker must close the `+` menu"
    );
    assert_eq!(
        stream.read_with(cx, |stream, _| (
            stream.model_picker_level,
            stream.actions.visible(),
        )),
        (ModelPickerLevel::Slider, false)
    );
}

/// R57 P2b (spec §2.3 R5 / acceptance A1), amended by R62 R7: the bottom
/// control row renders exactly `+` | permission status | spacer | model |
/// send, in that left-to-right order. The removed `Execute` dropdown and the
/// thinking chip must not appear in any frame.
///
/// R62 R7 reverses one half of P2b: the permission status is a real
/// affordance again (the reference implementation's `⚠ Full access` opens a
/// picker), so this test now asserts the chip **keeps its frozen slot** and
/// that clicking it opens the permission picker instead of the `+` menu. The
/// picker's own contents are asserted in `permission_picker.rs`.
#[gpui_kit::test]
async fn r57_bottom_row_renders_permission_status_between_add_and_model(cx: &mut TestAppContext) {
    let (window, stream, events) = open_controller_stream(cx, "bottom-row");
    cx.run_until_parked();

    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let add = visual.debug_bounds("composer-add").expect("add button");
    let permission = visual
        .debug_bounds("composer-permission-status")
        .expect("permission status chip");
    let model = visual
        .debug_bounds("composer-model")
        .expect("model trigger");
    let send = visual.debug_bounds("composer-send").expect("send button");

    // The status chip sits between the `+` button and the model trigger.
    assert!(
        f32::from(add.right()) <= f32::from(permission.left()),
        "permission status must follow the `+` button: add.right={:?} permission.left={:?}",
        add.right(),
        permission.left()
    );
    assert!(
        f32::from(permission.right()) <= f32::from(model.left()),
        "the model trigger must follow the permission status: permission.right={:?} model.left={:?}",
        permission.right(),
        model.left()
    );
    assert!(
        f32::from(model.right()) <= f32::from(send.left()),
        "send must be the trailing control: model.right={:?} send.left={:?}",
        model.right(),
        send.left()
    );
    assert_eq!(
        f32::from(send.size.width),
        Layout::COMPOSER_SEND_SIZE,
        "the send button keeps its fixed size"
    );

    // The removed controls leave no selector in the frame.
    for removed in [
        "composer-mode",
        "composer-mode-menu",
        "composer-permission",
        "composer-permission-menu",
        "thinking-control",
    ] {
        assert!(
            visual.debug_bounds(removed).is_none(),
            "{removed} must not be rendered after R57 P2b"
        );
    }

    // R62 R7: the chip is a real affordance. Clicking it opens the permission
    // picker — and nothing else: not the `+` menu, not the model picker, and
    // no settings request (a click that merely opens a menu changes no
    // durable state).
    visual.simulate_click(permission.center(), gpui_kit::Modifiers::default());
    visual.run_until_parked();
    assert!(
        visual.debug_bounds("composer-permission-picker").is_some(),
        "clicking the permission status must open the permission picker"
    );
    assert!(
        !stream.read_with(&visual, |stream, _| stream.actions.visible()),
        "clicking the permission status must not open the `+` menu"
    );
    assert!(
        events.lock().expect("settings events").is_empty(),
        "opening the permission picker must not emit ThreadSettingsRequested"
    );
    assert!(
        visual.debug_bounds("composer-model-menu").is_none(),
        "clicking the permission status must not open the model menu"
    );
}

#[gpui_kit::test]
async fn multiline_history_continues_and_is_thread_scoped(cx: &mut TestAppContext) {
    let (first_window, first, _) = open_controller_stream(cx, "history-a");
    let (_second_window, second, _) = open_controller_stream(cx, "history-b");
    first.update(cx, |stream, cx| {
        stream.composer_history = vec!["older\nfirst".into(), "newer\nfirst".into()];
        stream
            .input
            .update(cx, |input, cx| input.set_text("draft", cx));
    });
    second.update(cx, |stream, cx| {
        stream.composer_history = vec!["only\nsecond".into()];
        stream
            .input
            .update(cx, |input, cx| input.set_text("second draft", cx));
    });
    focus_composer(first_window, &first, cx);
    cx.simulate_keystrokes(first_window.into(), "up");
    assert_eq!(
        first.read_with(cx, |stream, cx| stream.input.read(cx).text().to_string()),
        "newer\nfirst"
    );
    cx.simulate_keystrokes(first_window.into(), "up");
    assert_eq!(
        first.read_with(cx, |stream, cx| stream.input.read(cx).text().to_string()),
        "older\nfirst"
    );
    assert_eq!(
        second.read_with(cx, |stream, cx| stream.input.read(cx).text().to_string()),
        "second draft"
    );
}

#[gpui_kit::test]
async fn composer_echo_waits_for_durable_acceptance(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "durable-submit");
    stream.update(cx, |stream, cx| {
        stream
            .input
            .update(cx, |input, cx| input.set_text("keep this draft", cx));
        stream.submit_message(cx);
    });
    let pending = stream.read_with(cx, |stream, cx| {
        (
            stream.composer_submit_pending,
            stream.input.read(cx).text().to_string(),
            stream.composer_history.len(),
            stream.entries.len(),
        )
    });
    assert_eq!(pending, (true, "keep this draft".into(), 0, 0));

    stream.update(cx, ConversationStream::reject_composer_submission);
    let rejected = stream.read_with(cx, |stream, cx| {
        (
            stream.composer_submit_pending,
            stream.input.read(cx).text().to_string(),
            stream.composer_history.len(),
            stream.entries.len(),
        )
    });
    assert_eq!(rejected, (false, "keep this draft".into(), 0, 0));

    stream.update(cx, |stream, cx| {
        stream.submit_message(cx);
        stream.accept_composer_submission("keep this draft", cx);
    });
    let accepted = stream.read_with(cx, |stream, cx| {
        (
            stream.composer_submit_pending,
            stream.input.read(cx).text().to_string(),
            stream.composer_history.clone(),
            stream.entries.len(),
        )
    });
    assert_eq!(
        accepted,
        (false, String::new(), vec!["keep this draft".into()], 1)
    );
}

#[gpui_kit::test]
async fn credential_failure_keeps_draft_and_renders_recovery_error(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "credential-submit");
    stream.update(cx, |stream, cx| {
        stream
            .input
            .update(cx, |input, cx| input.set_text("keep this draft", cx));
        stream.submit_message(cx);
        stream.apply_credential_error(cx);
    });
    cx.run_until_parked();
    stream.read_with(cx, |stream, cx| {
        assert!(!stream.composer_submit_pending);
        assert_eq!(stream.input.read(cx).text(), "keep this draft");
        assert!(stream.entries.is_empty());
        assert!(stream.composer_history.is_empty());
        assert_eq!(
            stream.controller_error.as_deref(),
            Some("本地凭据缺失或无法读取，请在设置中重新填写 API Key 后重试")
        );
    });
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    assert!(
        visual
            .debug_bounds("conversation-controller-error")
            .is_some()
    );
}

#[gpui_kit::test]
async fn reference_rejection_releases_submit_and_keeps_editable_draft(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "reference-rejection");
    stream.update(cx, |stream, cx| {
        stream
            .input
            .update(cx, |input, cx| input.set_text("总结 @missing.txt", cx));
        stream.submit_message(cx);
        stream.reject_composer_submission(cx);
        stream.apply_reference_error(FileReferenceFailureCode::Missing, cx);
    });
    let state = stream.read_with(cx, |stream, cx| {
        (
            stream.composer_submit_pending,
            stream.input.read(cx).text().to_string(),
            stream.entries.len(),
            stream.controller_error.clone(),
        )
    });
    assert!(!state.0, "failed reference must release submit owner");
    assert_eq!(state.1, "总结 @missing.txt");
    assert_eq!(state.2, 0, "failed reference must not create a user echo");
    assert_eq!(
        state.3.as_deref(),
        Some(FileReferenceFailureCode::Missing.message())
    );
}

#[gpui_kit::test]
async fn file_index_generation_overflow_fails_closed(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "reference-generation-overflow");
    stream.update(cx, |stream, _| {
        stream.file_index_generation = u64::MAX;
        assert!(
            stream.next_file_index_generation().is_none(),
            "generation overflow must not wrap and reuse an owner"
        );
        assert_eq!(stream.file_index_generation, u64::MAX);
    });
}

#[gpui_kit::test]
async fn standalone_at_query_stays_plain_text_without_file_index_state(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "standalone-reference");
    let requests = Arc::new(Mutex::new(Vec::<FileIndexRequested>::new()));
    let captured = requests.clone();
    cx.update(|cx| {
        cx.subscribe(&stream, move |_, event: &FileIndexRequested, _| {
            captured.lock().unwrap().push(event.clone());
        })
        .detach();
    });
    let input = stream.read_with(cx, |stream, _| stream.composer_input());
    stream.update(cx, |stream, cx| {
        stream.thread.project_id.clear();
        input.update(cx, |input, cx| input.set_text("@", cx));
        stream.sync_at_query(&input, cx);
    });

    assert!(requests.lock().unwrap().is_empty());
    stream.read_with(cx, |stream, cx| {
        assert_eq!(stream.input.read(cx).text(), "@");
        assert!(!stream.file_selector_wanted);
        assert!(!stream.file_index_loading);
        assert!(!stream.file_index_loaded);
        assert!(stream.file_index_failure.is_none());
        assert_eq!(stream.file_index_generation, 0);
        assert!(stream.file_index_candidates().is_empty());
    });
}

#[gpui_kit::test]
async fn file_index_late_success_is_fenced_after_cancel(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "reference-late-result");
    let input = stream.read_with(cx, |stream, _| stream.composer_input());
    stream.update(cx, |stream, cx| {
        input.update(cx, |input, cx| input.set_text("@", cx));
        stream.sync_at_query(&input, cx);
    });
    let generation = stream.read_with(cx, |stream, _| stream.file_index_generation);
    assert!(stream.read_with(cx, |stream, _| stream.file_index_loading()));
    stream.update(cx, |stream, cx| stream.close_file_selector_and_cancel(cx));
    let accepted = stream.update(cx, |stream, cx| {
        stream.apply_file_index_result(
            generation,
            Ok(vega_conversation::types::FileIndexSnapshot {
                entries: vec!["late.txt".into()],
            }),
            cx,
        )
    });
    assert!(!accepted, "cancelled generation cannot reopen the selector");
    assert!(!stream.read_with(cx, |stream, _| stream.file_index_loaded()));
    assert!(stream.read_with(cx, |stream, _| stream.file_index_candidates().is_empty()));
}

#[gpui_kit::test]
async fn failed_file_index_keys_restore_composer_scope(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "reference-failed-keys");
    let input = stream.read_with(cx, |stream, _| stream.composer_input());
    stream.update(cx, |stream, cx| {
        input.update(cx, |input, cx| input.set_text("@missing", cx));
        stream.sync_at_query(&input, cx);
    });
    let generation = stream.read_with(cx, |stream, _| stream.file_index_generation);
    stream.update(cx, |stream, cx| {
        assert!(stream.apply_file_index_result(
            generation,
            Err(FileIndexFailureCode::Traversal),
            cx
        ));
    });
    cx.run_until_parked();
    focus_composer(window, &stream, cx);

    // Tab reaches the real Retry focus stop without activating it.
    cx.simulate_keystrokes(window.into(), "tab");
    assert!(
        window
            .update(cx, |_, window, cx| {
                stream.read_with(cx, |stream, _| stream.file_retry_focus.is_focused(window))
            })
            .expect("retry focus")
    );
    assert!(stream.read_with(cx, |stream, _| stream.file_index_failure.is_some()));

    // Esc from that stop closes Failed and explicitly returns focus to input;
    // the next Enter is the Composer newline action, not FileSelect capture.
    cx.simulate_keystrokes(window.into(), "escape");
    assert!(
        window
            .update(cx, |_, window, cx| {
                stream
                    .read_with(cx, |stream, cx| stream.input.read(cx).focus_handle(cx))
                    .is_focused(window)
            })
            .expect("composer focus after escape")
    );
    assert_eq!(
        stream.read_with(cx, |stream, _| {
            (
                stream.file_selector_wanted,
                stream.file_index_failure,
                stream.file_index_loading,
            )
        }),
        (false, None, false)
    );
    cx.simulate_keystrokes(window.into(), "shift-enter");
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream.input.read(cx).text().to_string()),
        "@missing\n"
    );
}

#[gpui_kit::test]
async fn failed_file_index_enter_retries_through_key_dispatch(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "reference-retry-key");
    let retries = Arc::new(Mutex::new(Vec::<FileIndexRetryRequested>::new()));
    let captured = retries.clone();
    cx.update(|cx| {
        cx.subscribe(&stream, move |_, event: &FileIndexRetryRequested, _| {
            if let Ok(mut retries) = captured.lock() {
                retries.push(event.clone());
            }
        })
        .detach();
    });
    let input = stream.read_with(cx, |stream, _| stream.composer_input());
    stream.update(cx, |stream, cx| {
        input.update(cx, |input, cx| input.set_text("@retry", cx));
        stream.sync_at_query(&input, cx);
    });
    let generation = stream.read_with(cx, |stream, _| stream.file_index_generation);
    stream.update(cx, |stream, cx| {
        assert!(stream.apply_file_index_result(
            generation,
            Err(FileIndexFailureCode::DeadlineExceeded),
            cx
        ));
    });
    cx.run_until_parked();
    focus_composer(window, &stream, cx);

    // The failed panel's Enter binding retries directly from the composer.
    cx.simulate_keystrokes(window.into(), "enter");
    assert_eq!(
        stream.read_with(cx, |stream, _| {
            (
                stream.file_selector_wanted,
                stream.file_index_loading,
                stream.file_index_failure,
            )
        }),
        (true, true, None)
    );
    let retries = retries.lock().expect("retry event capture");
    assert_eq!(retries.len(), 1);
    assert_eq!(retries[0].thread_id, "reference-retry-key");
}

#[gpui_kit::test]
async fn unresolved_reasoning_authority_rejects_submit_but_missing_profile_defaults(
    cx: &mut TestAppContext,
) {
    let (_window, stream, _) = open_controller_stream(cx, "reasoning-authority");
    stream.update(cx, ConversationStream::mark_reasoning_unavailable);
    assert!(stream.read_with(cx, |stream, _| {
        stream.reasoning_unavailable() && stream.frozen_reasoning_for_submit().is_err()
    }));

    stream.update(cx, ConversationStream::clear_reasoning_profile);
    assert!(stream.read_with(cx, |stream, _| {
        !stream.reasoning_unavailable() && stream.frozen_reasoning_for_submit() == Ok(None)
    }));
}

/// R57 P3: the profile the app worker already pushes onto the stream is what
/// the mounted slider renders — no second capability path, and no fixed tier
/// count. A model with no declared tiers renders no slider at all (R12).
#[gpui_kit::test]
async fn r57_thinking_slider_follows_the_projected_profile_tier_count(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "tier-count");

    // A model that declares two tiers. The reference implementation renders
    // seven because its own model supports seven (spec §3.4 R7).
    let mut profile = ReasoningProfileProjection::unknown("owned", "mock");
    profile.support = ReasoningSupport::Optional;
    profile.efforts = vec!["low".into(), "max".into()];
    profile.preference = ReasoningChoice::Effort("max".into());
    stream.update(cx, |stream, cx| {
        stream.apply_reasoning_profile(profile.clone(), cx)
    });
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream
            .thinking_slider()
            .read(cx)
            .dot_count()),
        2,
        "the slider renders one dot per declared tier"
    );
    assert_eq!(
        stream.read_with(cx, |stream, _| stream.thinking_choice().to_string()),
        "max",
        "the projected preference is the displayed tier"
    );

    // The same profile with no declared efforts: no slider, and the composer
    // stays at provider default rather than inventing a tier.
    let mut empty = profile;
    empty.efforts = Vec::new();
    empty.preference = ReasoningChoice::ProviderDefault;
    stream.update(cx, |stream, cx| stream.apply_reasoning_profile(empty, cx));
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream
            .thinking_slider()
            .read(cx)
            .dot_count()),
        0,
        "a model with no declared tiers renders no dots"
    );
    assert!(!stream.read_with(cx, |stream, cx| {
        stream.thinking_slider().read(cx).has_tiers()
    }));
    assert_eq!(
        stream.read_with(cx, |stream, _| stream.thinking_choice().to_string()),
        "provider_default"
    );

    // A profile whose persisted preference is no longer among its declared
    // efforts (the file was edited externally while the preference stayed)
    // resolves through the slider's fallback instead of showing an
    // unsupported tier (R10). The composer's own projection still reports the
    // raw preference; the slider is what refuses to render it as selected.
    let mut narrowed = ReasoningProfileProjection::unknown("owned", "mock");
    narrowed.support = ReasoningSupport::Optional;
    narrowed.efforts = vec!["low".into(), "medium".into()];
    narrowed.preference = ReasoningChoice::Effort("max".into());
    stream.update(cx, |stream, cx| {
        stream.apply_reasoning_profile(narrowed, cx);
    });
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream
            .thinking_slider()
            .read(cx)
            .tier()
            .map(str::to_string)),
        Some("medium".to_string()),
        "an unsupported preference falls back to the nearest lower supported tier"
    );
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream
            .thinking_slider()
            .read(cx)
            .dot_count()),
        2,
        "the fallback does not change the declared tier count"
    );
}

/// R58 R2: the Off position appears exactly when the projected profile
/// declares `supports_disabled` **and** a `disabled_wire`, and the composer
/// routes an Off selection to `"disabled"` — never to an effort.
#[gpui_kit::test]
async fn r58_off_position_follows_the_disabled_capability(cx: &mut TestAppContext) {
    use vega_conversation::types::{ReasoningDisabledWire, ReasoningProtocol};

    let (_window, stream, _) = open_controller_stream(cx, "r58-off");

    let profile = |supports_disabled: bool, wire: Option<ReasoningDisabledWire>| {
        let mut profile = ReasoningProfileProjection::unknown("owned", "mock");
        profile.support = ReasoningSupport::Optional;
        profile.protocol = ReasoningProtocol::OpenAiChatCompletions;
        profile.efforts = vec!["low".into(), "medium".into(), "high".into()];
        profile.supports_disabled = supports_disabled;
        profile.disabled_wire = wire;
        profile.preference = ReasoningChoice::Effort("medium".into());
        profile
    };
    let slider = |cx: &mut TestAppContext| {
        stream.read_with(cx, |stream, cx| {
            let slider = stream.thinking_slider().read(cx);
            (
                slider.dot_count(),
                slider.tiers().to_vec(),
                slider.is_off(),
                slider.choice_name().map(str::to_string),
            )
        })
    };
    // Drives the real gesture path: the mounted slider emits
    // `ThinkingTierSelected`, the stream's own subscription handles it.
    let select_position = |cx: &mut TestAppContext, index: usize| {
        let slider = stream.read_with(cx, |stream, _| stream.thinking_slider());
        slider.update(cx, |slider, cx| {
            slider.select_index(index, cx);
        });
        cx.run_until_parked();
    };

    // Capability declared: three efforts plus the Off position.
    stream.update(cx, |stream, cx| {
        stream.apply_reasoning_profile(
            profile(true, Some(ReasoningDisabledWire::ReasoningEffortNone)),
            cx,
        );
    });
    let (dots, tiers, is_off, choice) = slider(cx);
    assert_eq!(dots, 4, "efforts.len() + 1 with Off shown");
    assert_eq!(
        tiers,
        vec!["low".to_string(), "medium".to_string(), "high".to_string()],
        "the tier list is exactly the profile's efforts — Off is not a member"
    );
    assert!(!is_off);
    assert_eq!(choice, Some("medium".to_string()));

    // Selecting the Off position persists `"disabled"` and never an effort.
    select_position(cx, 0);
    assert_eq!(
        stream.read_with(cx, |stream, _| stream.thinking_choice().to_string()),
        "disabled"
    );
    let (dots, tiers, is_off, choice) = slider(cx);
    assert!(is_off, "the Off position is selected");
    assert_eq!(choice, Some(OFF_CHOICE_NAME.to_string()));
    assert_eq!(dots, 4, "selecting Off does not change the dot count");
    assert!(
        !tiers
            .iter()
            .any(|tier| matches!(tier.as_str(), "off" | "none" | "disabled")),
        "no off-like string may enter the effort list"
    );
    // The frozen request for the next run is `Disabled`, not `Effort`.
    let frozen = stream
        .read_with(cx, |stream, _| stream.frozen_reasoning_for_submit())
        .expect("frozen profile")
        .expect("declared profile");
    assert_eq!(frozen.choice, ReasoningChoice::Disabled);
    assert_eq!(
        frozen.disabled_wire,
        Some(ReasoningDisabledWire::ReasoningEffortNone)
    );

    // A tier selection still yields the effort (A4). With Off at 0, `high` is
    // position 3.
    select_position(cx, 3);
    assert_eq!(
        stream.read_with(cx, |stream, _| stream.thinking_choice().to_string()),
        "high"
    );
    let frozen = stream
        .read_with(cx, |stream, _| stream.frozen_reasoning_for_submit())
        .expect("frozen profile")
        .expect("declared profile");
    assert_eq!(
        frozen.choice,
        ReasoningChoice::Effort("high".to_string()),
        "tier selection is unchanged by R58"
    );

    // Capability absent: no Off position, and the dot count is efforts.len().
    stream.update(cx, |stream, cx| {
        stream.apply_reasoning_profile(profile(false, None), cx);
    });
    let (dots, _tiers, is_off, _choice) = slider(cx);
    assert_eq!(dots, 3, "efforts.len() with no Off");
    assert!(!is_off);
    // Position 0 is now the first effort, not Off.
    select_position(cx, 0);
    assert_eq!(
        stream.read_with(cx, |stream, _| stream.thinking_choice().to_string()),
        "low"
    );
    let (_dots, _tiers, is_off, choice) = slider(cx);
    assert!(!is_off, "without the capability position 0 is an effort");
    assert_eq!(choice, Some("low".to_string()));

    // A `supports_disabled` without a wire is the inconsistent pairing the
    // store validator rejects. The slider must not offer Off for it either,
    // because the wire could not carry the choice.
    stream.update(cx, |stream, cx| {
        stream.apply_reasoning_profile(profile(true, None), cx);
    });
    let (dots, _tiers, is_off, _choice) = slider(cx);
    assert_eq!(dots, 3, "Off needs both the flag and the wire");
    assert!(!is_off);

    // R4: a persisted `"disabled"` preference lands on the Off position.
    stream.update(cx, |stream, cx| {
        let mut declared = profile(true, Some(ReasoningDisabledWire::ReasoningEffortNone));
        declared.preference = ReasoningChoice::Disabled;
        stream.apply_reasoning_profile(declared, cx);
    });
    assert_eq!(
        stream.read_with(cx, |stream, _| stream.thinking_choice().to_string()),
        "disabled"
    );
    let (dots, _tiers, is_off, choice) = slider(cx);
    assert_eq!(dots, 4);
    assert!(is_off, "a persisted disabled preference selects Off");
    assert_eq!(choice, Some(OFF_CHOICE_NAME.to_string()));

    // R6: `provider_default` selects nothing at all — it is not Off.
    stream.update(cx, |stream, cx| {
        let mut defaulted = profile(true, Some(ReasoningDisabledWire::ReasoningEffortNone));
        defaulted.preference = ReasoningChoice::ProviderDefault;
        stream.apply_reasoning_profile(defaulted, cx);
    });
    let (dots, _tiers, is_off, choice) = slider(cx);
    assert_eq!(dots, 4, "the ladder still renders for provider_default");
    assert!(!is_off, "provider_default is not Off");
    assert_eq!(choice, None, "provider_default selects no position");
    assert_eq!(
        stream.read_with(cx, |stream, _| stream.thinking_choice().to_string()),
        "provider_default"
    );
}

#[gpui_kit::test]
async fn approved_not_started_projection_preserves_and_blocks_new_draft(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "approved-recovery");
    stream.update(cx, |stream, cx| {
        stream
            .input
            .update(cx, |input, cx| input.set_text("do not lose", cx));
        stream.apply_approved_not_started(cx);
        stream.submit_message(cx);
    });
    let state = stream.read_with(cx, |stream, cx| {
        (
            stream.approved_not_started,
            stream.composer_submit_pending,
            stream.input.read(cx).text().to_string(),
            stream.entries.len(),
        )
    });
    assert_eq!(state, (true, false, "do not lose".into(), 0));
}

#[gpui_kit::test]
async fn durable_assistant_events_require_exact_active_message(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "durable-events");
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "assistant".into(),
                seq: 2,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "foreign".into(),
                delta: "hidden".into(),
            },
            cx,
        );
    });
    let foreign_ignored = stream.read_with(cx, |stream, _| {
        let (_, index) = stream
            .active_agent_message
            .as_ref()
            .expect("active message");
        match &stream.entries[*index] {
            StreamEntry::Assistant { stream, .. } => stream.snapshot().pending.is_none(),
            _ => false,
        }
    });
    assert!(foreign_ignored);

    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "assistant".into(),
                delta: "visible".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: "foreign".into(),
                stop_reason: vega_conversation::types::ConversationStopReason::End,
            },
            cx,
        );
    });
    assert!(stream.read_with(cx, |stream, _| stream.active_agent_message.is_some()));
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: "assistant".into(),
                stop_reason: vega_conversation::types::ConversationStopReason::End,
            },
            cx,
        );
    });
    assert!(stream.read_with(cx, |stream, _| stream.active_agent_message.is_none()));
}

#[gpui_kit::test]
async fn completed_plan_replaces_streaming_assistant_after_older_plan_refresh(
    cx: &mut TestAppContext,
) {
    let (_window, stream, _) = open_controller_stream(cx, "plan-dedup");
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "plan-message".into(),
                seq: 2,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "plan-message".into(),
                delta: "1. inspect".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: "plan-message".into(),
                stop_reason: vega_conversation::types::ConversationStopReason::End,
            },
            cx,
        );
        stream.apply_plan(
            Plan {
                id: "older-plan".into(),
                thread_id: "plan-dedup".into(),
                content: "older".into(),
                status: PlanStatus::Abandoned,
                review_note: Some("superseded".into()),
                reviewed_at: Some(1),
            },
            cx,
        );
        stream.apply_plan(
            Plan {
                id: "plan-message".into(),
                thread_id: "plan-dedup".into(),
                content: "1. inspect".into(),
                status: PlanStatus::Pending,
                review_note: None,
                reviewed_at: None,
            },
            cx,
        );
    });
    let (plans, assistants, entries) = stream.read_with(cx, |stream, _| {
        let plans = stream
            .entries
            .iter()
            .filter(|entry| matches!(entry, StreamEntry::Plan { .. }))
            .count();
        let assistants = stream
            .entries
            .iter()
            .filter(|entry| matches!(entry, StreamEntry::Assistant { .. }))
            .count();
        (plans, assistants, stream.entries.len())
    });
    assert_eq!((plans, assistants, entries), (2, 0, 2));
}

#[gpui_kit::test]
async fn task_summary_card_appends_once_and_ignores_duplicates(cx: &mut TestAppContext) {
    let (_window, stream, _) = open_controller_stream(cx, "summary-card");
    let summary = TaskCostSummary {
        message_id: "assistant-summary".into(),
        outcome: TaskSummaryOutcome::Completed,
        usage: Some(vega_conversation::types::TokenUsage {
            input: 150_000,
            output: 15_000,
            cache_read: 50_000,
            cache_write: 0,
        }),
        cost: vega_conversation::types::SummaryCost::Priced(vega_conversation::types::Microcents(
            135_000,
        )),
        duration_ms: Some(12_400),
        tool_count: 2,
        cache_hit_percent: Some(33),
    };
    stream.update(cx, |stream, cx| {
        stream.apply_task_summary(summary.clone(), cx);
        stream.apply_task_summary(summary, cx);
    });
    let (summaries, rows, text) = stream.read_with(cx, |stream, cx| {
        let mut text = String::new();
        let mut summaries = 0;
        let mut rows = 0;
        for entry in &stream.entries {
            rows += entry.row_count(cx);
            if let StreamEntry::Summary { card } = entry {
                summaries += 1;
                text = card.read(cx).visible_text();
            }
        }
        (summaries, rows, text)
    });
    assert_eq!(summaries, 1, "duplicate/stale summaries are ignored");
    assert_eq!(rows, 0, "completed summaries occupy no transcript rows");
    assert!(text.contains("任务摘要 · 完成"));
    assert!(text.contains("成本 US$0.135000"));
    assert!(text.contains("耗时 12.4s"));
    assert!(text.contains("工具 2 · 缓存命中 33%"));
}

#[gpui_kit::test]
async fn transcript_hides_completed_statistics_but_keeps_failure_status(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "quiet-usage");
    stream.update(cx, |stream, cx| {
        for (id, outcome) in [
            ("complete", TaskSummaryOutcome::Completed),
            ("failed", TaskSummaryOutcome::Failed),
        ] {
            stream.apply_task_summary(
                TaskCostSummary {
                    message_id: id.into(),
                    outcome,
                    usage: None,
                    cost: SummaryCost::Unavailable,
                    duration_ms: None,
                    tool_count: 0,
                    cache_hit_percent: None,
                },
                cx,
            );
        }
    });
    cx.run_until_parked();
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    assert_eq!(
        visual
            .debug_bounds("completed-task-summary-hidden")
            .expect("retained hidden summary")
            .size
            .height,
        px(0.)
    );
    assert!(
        visual
            .debug_bounds("task-outcome-status")
            .expect("visible failure")
            .size
            .height
            > px(0.)
    );
    assert_eq!(
        stream.read_with(&visual, |stream, _| stream.summary_cards.len()),
        2
    );
}

#[gpui_kit::test]
async fn batch_finished_flush_materializes_the_final_committed_tail(cx: &mut TestAppContext) {
    // S8-T44 review P1-1: a batched ingress tail [TextDelta…, MessageFinished]
    // must materialize the frozen committed tail in finish_agent_message
    // itself — render's sync only covers the *active* turn, which is already
    // taken by the time the next frame runs.
    let (_window, stream, _) = open_controller_stream(cx, "batch-finish");
    stream.update(cx, |stream, cx| {
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "assistant".into(),
                seq: 2,
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::TextDelta {
                message_id: "assistant".into(),
                delta: "```rust\nfn tail() {}\n```".into(),
            },
            cx,
        );
        stream.apply_event(
            ConversationEvent::MessageFinished {
                message_id: "assistant".into(),
                stop_reason: vega_conversation::types::ConversationStopReason::End,
            },
            cx,
        );
    });
    let (frozen, committed, last_text, tail_kind) = stream.read_with(cx, |stream, _| {
        let index = match stream.active_agent_message.as_ref() {
            Some((_, index)) => *index,
            None => stream
                .last_finished_agent_message
                .as_ref()
                .map(|(_, index)| *index)
                .expect("the finished turn stays booked"),
        };
        match &stream.entries[index] {
            StreamEntry::Assistant { model, .. } => {
                let lines = &model.committed_lines;
                (
                    stream
                        .counters
                        .frozen_rematerializations
                        .load(Ordering::Relaxed),
                    stream
                        .counters
                        .committed_materializations
                        .load(Ordering::Relaxed),
                    lines
                        .last()
                        .map(|line| {
                            line.spans
                                .iter()
                                .map(|span| span.text.as_str())
                                .collect::<String>()
                        })
                        .unwrap_or_default(),
                    lines.last().map(|line| line.kind),
                )
            }
            _ => panic!("the finished entry is an assistant turn"),
        }
    });
    assert_eq!(frozen, 0, "finish materialization is not a frozen remat");
    assert!(committed >= 1, "the final tail block was materialized");
    assert_eq!(tail_kind, Some(LineKind::Code), "the tail block froze");
    assert_eq!(last_text, "fn tail() {}", "the closing fence line survives");
}

// ---------- Issue #66 · Composer Enter 键位契约（ui-spec §4.4 v0.18） ----------

/// Captures every `ComposerSubmitted` emitted by the production submit path.
fn capture_submitted(
    cx: &mut TestAppContext,
    stream: &Entity<ConversationStream>,
) -> Arc<Mutex<Vec<ComposerSubmitted>>> {
    let events = Arc::new(Mutex::new(Vec::<ComposerSubmitted>::new()));
    let captured = events.clone();
    cx.update(|cx| {
        cx.subscribe(stream, move |_, event: &ComposerSubmitted, _| {
            captured
                .lock()
                .expect("submitted capture")
                .push(event.clone());
        })
        .detach();
    });
    events
}

fn set_composer_text(
    window: WindowHandle<StreamHarness>,
    stream: &Entity<ConversationStream>,
    text: &str,
    cx: &mut TestAppContext,
) {
    window
        .update(cx, |_, _, cx| {
            stream.update(cx, |stream, cx| {
                stream
                    .input
                    .update(cx, |input, cx| input.set_text(text, cx));
            });
        })
        .expect("composer text");
    cx.run_until_parked();
}

/// E1/R1: `Enter` sends through the real key-dispatch path.
#[gpui_kit::test]
async fn issue66_enter_sends_through_key_dispatch(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "enter-sends");
    let submitted = capture_submitted(cx, &stream);
    focus_composer(window, &stream, cx);
    set_composer_text(window, &stream, "ship it", cx);

    cx.simulate_keystrokes(window.into(), "enter");
    cx.run_until_parked();

    let events = submitted.lock().expect("submitted");
    assert_eq!(events.len(), 1, "Enter must submit exactly once");
    assert_eq!(events[0].content, "ship it");
    assert!(
        stream.read_with(cx, |stream, _| stream.composer_submit_pending),
        "Enter must arm the single-flight submit guard"
    );
}

/// E2/R2: `Shift+Enter` inserts a newline and never sends.
#[gpui_kit::test]
async fn issue66_shift_enter_inserts_newline_without_sending(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "shift-enter-newline");
    let submitted = capture_submitted(cx, &stream);
    focus_composer(window, &stream, cx);
    set_composer_text(window, &stream, "line one", cx);

    cx.simulate_keystrokes(window.into(), "shift-enter");
    cx.run_until_parked();

    assert!(
        submitted.lock().expect("submitted").is_empty(),
        "Shift+Enter must never send"
    );
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream.input.read(cx).text().to_string()),
        "line one\n"
    );
}

/// E3/R3: `Cmd+Enter` keeps sending (no regression on the legacy binding).
#[gpui_kit::test]
async fn issue66_cmd_enter_still_sends(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "cmd-enter-sends");
    let submitted = capture_submitted(cx, &stream);
    focus_composer(window, &stream, cx);
    set_composer_text(window, &stream, "legacy path", cx);

    cx.simulate_keystrokes(window.into(), "cmd-enter");
    cx.run_until_parked();

    let events = submitted.lock().expect("submitted");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].content, "legacy path");
}

/// E6/R5: while an IME composition owns the text, `Enter` must not submit —
/// even if the platform layer lets the key reach the keymap (defence in depth).
#[gpui_kit::test]
async fn issue66_enter_never_sends_during_ime_composition(cx: &mut TestAppContext) {
    use gpui_kit::EntityInputHandler;
    let (window, stream, _) = open_controller_stream(cx, "enter-ime");
    let submitted = capture_submitted(cx, &stream);
    focus_composer(window, &stream, cx);
    window
        .update(cx, |_, window, cx| {
            let input = stream.read(cx).composer_input();
            input.update(cx, |input, cx| {
                input.replace_and_mark_text_in_range(None, "ni", Some(2..2), window, cx)
            });
        })
        .expect("platform marked text");
    cx.run_until_parked();
    assert!(stream.read_with(cx, |stream, cx| stream.input.read(cx).is_composing()));

    // The headless platform cannot emulate the native IME interception, so
    // drive the production handler directly while composition owns the text
    // (same convention as `r11_composer_marked_text_is_not_a_command`).
    window
        .update(cx, |_, window, cx| {
            stream.update(cx, |stream, cx| {
                stream.on_send_action(&SendMessage, window, cx);
            });
        })
        .expect("scoped send handler");
    cx.run_until_parked();

    assert!(
        submitted.lock().expect("submitted").is_empty(),
        "a composing Enter belongs to the IME, not to send"
    );
    assert!(!stream.read_with(cx, |stream, _| stream.composer_submit_pending));
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream.input.read(cx).text().to_string()),
        "ni"
    );
}

/// E7/R1: the existing submit guard still makes `Enter` single-flight.
#[gpui_kit::test]
async fn issue66_enter_respects_the_submit_guard(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "enter-guard");
    let submitted = capture_submitted(cx, &stream);
    focus_composer(window, &stream, cx);
    set_composer_text(window, &stream, "once", cx);

    cx.simulate_keystrokes(window.into(), "enter");
    cx.run_until_parked();
    cx.simulate_keystrokes(window.into(), "enter");
    cx.run_until_parked();

    assert_eq!(
        submitted.lock().expect("submitted").len(),
        1,
        "a pending submit must not be re-armed by a second Enter"
    );
}

/// E4/R4: an open `@file` overlay keeps `Enter` — it accepts the highlighted
/// candidate instead of sending the draft.
#[gpui_kit::test]
async fn issue66_enter_accepts_file_candidate_instead_of_sending(cx: &mut TestAppContext) {
    let (window, stream, _) = open_controller_stream(cx, "enter-file-select");
    let submitted = capture_submitted(cx, &stream);
    let input = stream.read_with(cx, |stream, _| stream.composer_input());
    stream.update(cx, |stream, cx| {
        input.update(cx, |input, cx| input.set_text("@", cx));
        stream.sync_at_query(&input, cx);
    });
    let generation = stream.read_with(cx, |stream, _| stream.file_index_generation);
    stream.update(cx, |stream, cx| {
        assert!(stream.apply_file_index_result(
            generation,
            Ok(vega_conversation::types::FileIndexSnapshot {
                entries: vec!["src/lib.rs".into()],
            }),
            cx,
        ));
    });
    cx.run_until_parked();
    focus_composer(window, &stream, cx);

    cx.simulate_keystrokes(window.into(), "enter");
    cx.run_until_parked();

    assert!(
        submitted.lock().expect("submitted").is_empty(),
        "an open FileSelect overlay owns Enter"
    );
    assert_eq!(
        stream.read_with(cx, |stream, cx| stream.input.read(cx).text().to_string()),
        "@src/lib.rs ",
        "Enter must accept the highlighted candidate"
    );
}
